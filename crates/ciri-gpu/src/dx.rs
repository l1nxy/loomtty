//! Direct3D 11 backend for Windows.
//!
//! Uses D3D11 instanced rendering for rects, alpha text, and color emoji.
//! HLSL shaders are compiled at runtime via D3DCompile (Fxc).

use anyhow::Result;
use ciri_config::config::RenderConfig;
use ciri_render::glyph_cache::{GlyphCache, GlyphInstance, PendingUpload, ScissoredRange};
use ciri_render::rect::Rect;
use ciri_render::FrameScene;
use std::sync::Arc;
use winit::window::Window;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::core::*;
use windows::Win32::Graphics::Direct3D::Fxc::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;

// ─── HLSL shaders ───────────────────────────────────────────────────

const RECT_HLSL: &str = r#"
cbuffer Viewport : register(b0) {
    float2 viewport_size;
    float2 _pad;
};

struct VSInput {
    float2 pos   : POS;
    float2 size  : SIZE;
    float4 color : COLOR;
    uint vid     : SV_VertexID;
};

struct PSInput {
    float4 position : SV_POSITION;
    float4 color    : COLOR;
};

float srgb_to_linear(float c) {
    return (c <= 0.04045) ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4);
}

PSInput vs_main(VSInput input) {
    float x = float(input.vid & 1);
    float y = float((input.vid >> 1) & 1);

    float2 px = input.pos + float2(x, y) * input.size;
    float2 ndc = float2(
        px.x / viewport_size.x * 2.0 - 1.0,
        1.0 - px.y / viewport_size.y * 2.0
    );

    PSInput output;
    output.position = float4(ndc, 0.0, 1.0);
    output.color = float4(
        srgb_to_linear(input.color.r),
        srgb_to_linear(input.color.g),
        srgb_to_linear(input.color.b),
        input.color.a
    );
    return output;
}

float4 ps_main(PSInput input) : SV_TARGET {
    return input.color;
}
"#;

const GLYPH_HLSL: &str = r#"
cbuffer Viewport : register(b0) {
    float2 viewport_size;
    float2 _pad;
};

Texture2D atlas_tex : register(t0);
SamplerState atlas_sampler : register(s0);

struct VSInput {
    float2 pos     : POS;
    float2 size    : SIZE;
    float2 uv_pos  : UVPOS;
    float2 uv_size : UVSIZE;
    float4 color   : COLOR;
    uint vid       : SV_VertexID;
};

struct PSInput {
    float4 position : SV_POSITION;
    float2 uv       : TEXCOORD0;
    float4 color    : COLOR;
};

PSInput vs_main(VSInput input) {
    float x = float(input.vid & 1);
    float y = float((input.vid >> 1) & 1);

    PSInput output;
    output.uv = input.uv_pos + float2(x, y) * input.uv_size;
    output.color = input.color;

    float2 px = input.pos + float2(x, y) * input.size;
    float2 ndc = float2(
        px.x / viewport_size.x * 2.0 - 1.0,
        1.0 - px.y / viewport_size.y * 2.0
    );
    output.position = float4(ndc, 0.0, 1.0);
    return output;
}
"#;

const ALPHA_PS_HLSL: &str = r#"
Texture2D atlas_tex : register(t0);
SamplerState atlas_sampler : register(s0);

struct PSInput {
    float4 position : SV_POSITION;
    float2 uv       : TEXCOORD0;
    float4 color    : COLOR;
};

float srgb_to_linear(float c) {
    return (c <= 0.04045) ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4);
}

float4 ps_main(PSInput input) : SV_TARGET {
    float alpha = atlas_tex.Sample(atlas_sampler, input.uv).r;
    float r = srgb_to_linear(input.color.r);
    float g = srgb_to_linear(input.color.g);
    float b = srgb_to_linear(input.color.b);
    return float4(r, g, b, input.color.a * alpha);
}
"#;

const COLOR_PS_HLSL: &str = r#"
Texture2D atlas_tex : register(t0);
SamplerState atlas_sampler : register(s0);

struct PSInput {
    float4 position : SV_POSITION;
    float2 uv       : TEXCOORD0;
    float4 color    : COLOR;
};

float4 ps_main(PSInput input) : SV_TARGET {
    float4 texel = atlas_tex.Sample(atlas_sampler, input.uv);
    return float4(texel.rgb * input.color.rgb, texel.a * input.color.a);
}
"#;

// ─── Shader compilation ─────────────────────────────────────────────

unsafe fn compile_shader(source: &str, entry: &str, target: &str) -> Result<ID3DBlob> {
    let mut blob = None;
    let mut errors = None;
    let hr = D3DCompile(
        source.as_bytes().as_ptr() as *const _,
        source.len(),
        None,
        None,
        None,
        PCSTR::from_raw(format!("{entry}\0").as_ptr()),
        PCSTR::from_raw(format!("{target}\0").as_ptr()),
        D3DCOMPILE_OPTIMIZATION_LEVEL3,
        0,
        &mut blob,
        Some(&mut errors),
    );
    if hr.is_err() {
        let msg = if let Some(err) = errors {
            let ptr = err.GetBufferPointer() as *const u8;
            let len = err.GetBufferSize();
            String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len)).to_string()
        } else {
            format!("HRESULT: {hr:?}")
        };
        anyhow::bail!("shader compile failed ({entry}): {msg}");
    }
    blob.ok_or_else(|| anyhow::anyhow!("no shader blob"))
}

// ─── D3D Atlas Layer ────────────────────────────────────────────────

struct DxAtlasLayer {
    texture: ID3D11Texture2D,
    srv: ID3D11ShaderResourceView,
    sampler: ID3D11SamplerState,
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    input_layout: ID3D11InputLayout,
    instance_buffer: ID3D11Buffer,
    cbuffer: ID3D11Buffer,
    atlas_size: u32,
    bpp: u32,
    max_instances: usize,
}

impl DxAtlasLayer {
    unsafe fn new(
        device: &ID3D11Device,
        atlas_size: u32,
        max_instances: usize,
        tex_format: DXGI_FORMAT,
        bpp: u32,
        vs_hlsl: &str,
        ps_hlsl: &str,
        filter: D3D11_FILTER,
    ) -> Result<Self> {
        // Texture
        let tex_desc = D3D11_TEXTURE2D_DESC {
            Width: atlas_size,
            Height: atlas_size,
            MipLevels: 1,
            ArraySize: 1,
            Format: tex_format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let texture = device.CreateTexture2D(&tex_desc, None)?;

        let srv_desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
            Format: tex_format,
            ViewDimension: D3D_SRV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_SRV {
                    MostDetailedMip: 0,
                    MipLevels: 1,
                },
            },
        };
        let srv = device.CreateShaderResourceView(&texture, Some(&srv_desc))?;

        let sampler_desc = D3D11_SAMPLER_DESC {
            Filter: filter,
            AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
            MaxAnisotropy: 1,
            ComparisonFunc: D3D11_COMPARISON_NEVER,
            MaxLOD: f32::MAX,
            ..Default::default()
        };
        let sampler = device.CreateSamplerState(&sampler_desc)?;

        // Shaders
        let vs_blob = compile_shader(vs_hlsl, "vs_main", "vs_5_0")?;
        let vs_code =
            std::slice::from_raw_parts(vs_blob.GetBufferPointer() as *const u8, vs_blob.GetBufferSize());
        let vs = device.CreateVertexShader(vs_code, None)?;

        let ps_blob = compile_shader(ps_hlsl, "ps_main", "ps_5_0")?;
        let ps_code =
            std::slice::from_raw_parts(ps_blob.GetBufferPointer() as *const u8, ps_blob.GetBufferSize());
        let ps = device.CreatePixelShader(ps_code, None)?;

        // Input layout for glyph instances
        let layout_desc = [
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR::from_raw(b"POS\0".as_ptr()),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 0,
                InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
                InstanceDataStepRate: 1,
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR::from_raw(b"SIZE\0".as_ptr()),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 8,
                InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
                InstanceDataStepRate: 1,
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR::from_raw(b"UVPOS\0".as_ptr()),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 16,
                InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
                InstanceDataStepRate: 1,
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR::from_raw(b"UVSIZE\0".as_ptr()),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 24,
                InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
                InstanceDataStepRate: 1,
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR::from_raw(b"COLOR\0".as_ptr()),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 32,
                InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
                InstanceDataStepRate: 1,
            },
        ];
        let input_layout = device.CreateInputLayout(&layout_desc, vs_code)?;

        // Instance buffer
        let buf_desc = D3D11_BUFFER_DESC {
            ByteWidth: (max_instances * std::mem::size_of::<GlyphInstance>()) as u32,
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            ..Default::default()
        };
        let instance_buffer = device.CreateBuffer(&buf_desc, None)?;

        // Constant buffer (viewport)
        let cb_desc = D3D11_BUFFER_DESC {
            ByteWidth: 16, // vec4 (viewport_size + pad)
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            ..Default::default()
        };
        let cbuffer = device.CreateBuffer(&cb_desc, None)?;

        Ok(DxAtlasLayer {
            texture,
            srv,
            sampler,
            vs,
            ps,
            input_layout,
            instance_buffer,
            cbuffer,
            atlas_size,
            bpp,
            max_instances,
        })
    }

    unsafe fn flush_uploads(
        &self,
        ctx: &ID3D11DeviceContext,
        pending: &mut Vec<PendingUpload>,
        pending_clear: bool,
    ) {
        if pending_clear {
            let zeros = vec![0u8; (self.atlas_size * self.atlas_size * self.bpp) as usize];
            let row_pitch = self.atlas_size * self.bpp;
            let box_ = D3D11_BOX {
                left: 0,
                top: 0,
                front: 0,
                right: self.atlas_size,
                bottom: self.atlas_size,
                back: 1,
            };
            ctx.UpdateSubresource(
                &self.texture,
                0,
                Some(&box_),
                zeros.as_ptr() as *const _,
                row_pitch,
                0,
            );
        }

        for upload in pending.drain(..) {
            let row_pitch = upload.w * self.bpp;
            let box_ = D3D11_BOX {
                left: upload.x,
                top: upload.y,
                front: 0,
                right: upload.x + upload.w,
                bottom: upload.y + upload.h,
                back: 1,
            };
            ctx.UpdateSubresource(
                &self.texture,
                0,
                Some(&box_),
                upload.data.as_ptr() as *const _,
                row_pitch,
                0,
            );
        }
    }

    unsafe fn render_scissored(
        &self,
        ctx: &ID3D11DeviceContext,
        instances: &[GlyphInstance],
        viewport_w: f32,
        viewport_h: f32,
        viewport_w_px: u32,
        viewport_h_px: u32,
        batches: &[ScissoredRange],
        overlay_start: usize,
    ) {
        if instances.is_empty() {
            return;
        }
        let count = instances.len().min(self.max_instances);

        // Update viewport cbuffer
        let viewport = [viewport_w, viewport_h, 0.0f32, 0.0f32];
        let mapped = ctx.Map(&self.cbuffer, 0, D3D11_MAP_WRITE_DISCARD, 0).unwrap();
        std::ptr::copy_nonoverlapping(
            viewport.as_ptr() as *const u8,
            mapped.pData as *mut u8,
            16,
        );
        ctx.Unmap(&self.cbuffer, 0);

        // Update instance buffer
        let data = bytemuck::cast_slice(&instances[..count]);
        let mapped = ctx
            .Map(&self.instance_buffer, 0, D3D11_MAP_WRITE_DISCARD, 0)
            .unwrap();
        std::ptr::copy_nonoverlapping(data.as_ptr(), mapped.pData as *mut u8, data.len());
        ctx.Unmap(&self.instance_buffer, 0);

        // Bind pipeline state
        ctx.IASetInputLayout(Some(&self.input_layout));
        ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
        let stride = std::mem::size_of::<GlyphInstance>() as u32;
        ctx.VSSetShader(Some(&self.vs), None);
        ctx.PSSetShader(Some(&self.ps), None);
        ctx.VSSetConstantBuffers(0, Some(&[Some(self.cbuffer.clone())]));
        ctx.PSSetShaderResources(0, Some(&[Some(self.srv.clone())]));
        ctx.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));

        // Draw each scissored batch
        for batch in batches {
            let start = batch.start.min(count);
            let end = batch.end.min(count);
            if start >= end || batch.w == 0 || batch.h == 0 {
                continue;
            }
            let rect = RECT {
                left: batch.x as i32,
                top: batch.y as i32,
                right: (batch.x + batch.w) as i32,
                bottom: (batch.y + batch.h) as i32,
            };
            ctx.RSSetScissorRects(Some(&[rect]));
            let offset = (start * std::mem::size_of::<GlyphInstance>()) as u32;
            ctx.IASetVertexBuffers(
                0,
                1,
                Some(&Some(self.instance_buffer.clone())),
                Some(&stride),
                Some(&offset),
            );
            ctx.DrawInstanced(4, (end - start) as u32, 0, 0);
        }

        // Overlay (no scissor)
        let overlay_start = overlay_start.min(count);
        if overlay_start < count {
            let rect = RECT {
                left: 0,
                top: 0,
                right: viewport_w_px as i32,
                bottom: viewport_h_px as i32,
            };
            ctx.RSSetScissorRects(Some(&[rect]));
            let offset = (overlay_start * std::mem::size_of::<GlyphInstance>()) as u32;
            ctx.IASetVertexBuffers(
                0,
                1,
                Some(&Some(self.instance_buffer.clone())),
                Some(&stride),
                Some(&offset),
            );
            ctx.DrawInstanced(4, (count - overlay_start) as u32, 0, 0);
        }
    }
}

// ─── D3D Rect Pipeline ──────────────────────────────────────────────

struct DxRectPipeline {
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    input_layout: ID3D11InputLayout,
    instance_buffer: ID3D11Buffer,
    cbuffer: ID3D11Buffer,
    max_rects: usize,
}

impl DxRectPipeline {
    unsafe fn new(device: &ID3D11Device, max_rects: usize) -> Result<Self> {
        let vs_blob = compile_shader(RECT_HLSL, "vs_main", "vs_5_0")?;
        let vs_code =
            std::slice::from_raw_parts(vs_blob.GetBufferPointer() as *const u8, vs_blob.GetBufferSize());
        let vs = device.CreateVertexShader(vs_code, None)?;

        let ps_blob = compile_shader(RECT_HLSL, "ps_main", "ps_5_0")?;
        let ps_code =
            std::slice::from_raw_parts(ps_blob.GetBufferPointer() as *const u8, ps_blob.GetBufferSize());
        let ps = device.CreatePixelShader(ps_code, None)?;

        let layout_desc = [
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR::from_raw(b"POS\0".as_ptr()),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 0,
                InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
                InstanceDataStepRate: 1,
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR::from_raw(b"SIZE\0".as_ptr()),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 8,
                InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
                InstanceDataStepRate: 1,
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR::from_raw(b"COLOR\0".as_ptr()),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 16,
                InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
                InstanceDataStepRate: 1,
            },
        ];
        let input_layout = device.CreateInputLayout(&layout_desc, vs_code)?;

        let buf_desc = D3D11_BUFFER_DESC {
            ByteWidth: (max_rects * std::mem::size_of::<Rect>()) as u32,
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            ..Default::default()
        };
        let instance_buffer = device.CreateBuffer(&buf_desc, None)?;

        let cb_desc = D3D11_BUFFER_DESC {
            ByteWidth: 16,
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            ..Default::default()
        };
        let cbuffer = device.CreateBuffer(&cb_desc, None)?;

        Ok(DxRectPipeline {
            vs,
            ps,
            input_layout,
            instance_buffer,
            cbuffer,
            max_rects,
        })
    }

    unsafe fn render(
        &self,
        ctx: &ID3D11DeviceContext,
        rects: &[Rect],
        viewport_w: f32,
        viewport_h: f32,
    ) {
        if rects.is_empty() {
            return;
        }
        let count = rects.len().min(self.max_rects);

        // Viewport cbuffer
        let viewport = [viewport_w, viewport_h, 0.0f32, 0.0f32];
        let mapped = ctx.Map(&self.cbuffer, 0, D3D11_MAP_WRITE_DISCARD, 0).unwrap();
        std::ptr::copy_nonoverlapping(
            viewport.as_ptr() as *const u8,
            mapped.pData as *mut u8,
            16,
        );
        ctx.Unmap(&self.cbuffer, 0);

        // Instance data
        let data = bytemuck::cast_slice(&rects[..count]);
        let mapped = ctx
            .Map(&self.instance_buffer, 0, D3D11_MAP_WRITE_DISCARD, 0)
            .unwrap();
        std::ptr::copy_nonoverlapping(data.as_ptr(), mapped.pData as *mut u8, data.len());
        ctx.Unmap(&self.instance_buffer, 0);

        ctx.IASetInputLayout(Some(&self.input_layout));
        ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
        let stride = std::mem::size_of::<Rect>() as u32;
        let offset = 0u32;
        ctx.IASetVertexBuffers(
            0,
            1,
            Some(&Some(self.instance_buffer.clone())),
            Some(&stride),
            Some(&offset),
        );
        ctx.VSSetShader(Some(&self.vs), None);
        ctx.PSSetShader(Some(&self.ps), None);
        ctx.VSSetConstantBuffers(0, Some(&[Some(self.cbuffer.clone())]));
        ctx.DrawInstanced(4, count as u32, 0, 0);
    }
}

// ─── GlyphAtlasGpu ─────────────────────────────────────────────────

pub struct GlyphAtlasGpu {
    alpha: DxAtlasLayer,
    color: DxAtlasLayer,
}

// ─── Renderer ───────────────────────────────────────────────────────

pub struct Renderer {
    device: ID3D11Device,
    ctx: ID3D11DeviceContext,
    swap_chain: IDXGISwapChain,
    rtv: ID3D11RenderTargetView,
    rasterizer: ID3D11RasterizerState,
    blend: ID3D11BlendState,
    rects: DxRectPipeline,
    width: u32,
    height: u32,
}

impl Renderer {
    pub fn new(window: Arc<Window>, render_config: &RenderConfig) -> Result<Self> {
        let size = window.inner_size();
        let hwnd = match window.window_handle()?.as_raw() {
            RawWindowHandle::Win32(h) => h.hwnd,
            _ => anyhow::bail!("expected Win32 window handle"),
        };

        let swap_desc = DXGI_SWAP_CHAIN_DESC {
            BufferDesc: DXGI_MODE_DESC {
                Width: size.width.max(1),
                Height: size.height.max(1),
                RefreshRate: DXGI_RATIONAL {
                    Numerator: 0,
                    Denominator: 1,
                },
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                ..Default::default()
            },
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            OutputWindow: std::mem::transmute(hwnd),
            Windowed: true.into(),
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            Flags: 0,
        };

        let mut device = None;
        let mut ctx = None;
        let mut swap_chain = None;

        unsafe {
            D3D11CreateDeviceAndSwapChain(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_FLAG(0),
                None, // default feature levels
                D3D11_SDK_VERSION,
                Some(&swap_desc),
                Some(&mut swap_chain),
                Some(&mut device),
                None,
                Some(&mut ctx),
            )?;
        }

        let device = device.unwrap();
        let ctx = ctx.unwrap();
        let swap_chain = swap_chain.unwrap();

        let rtv = unsafe { create_rtv(&device, &swap_chain)? };

        // Rasterizer state with scissor enabled
        let raster_desc = D3D11_RASTERIZER_DESC {
            FillMode: D3D11_FILL_SOLID,
            CullMode: D3D11_CULL_NONE,
            ScissorEnable: true.into(),
            ..Default::default()
        };
        let rasterizer = unsafe { device.CreateRasterizerState(&raster_desc)? };

        // Blend state (alpha blending)
        let mut blend_desc = D3D11_BLEND_DESC::default();
        blend_desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: true.into(),
            SrcBlend: D3D11_BLEND_SRC_ALPHA,
            DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOp: D3D11_BLEND_OP_ADD,
            SrcBlendAlpha: D3D11_BLEND_ONE,
            DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOpAlpha: D3D11_BLEND_OP_ADD,
            RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
        };
        let blend = unsafe { device.CreateBlendState(&blend_desc)? };

        let rects = unsafe { DxRectPipeline::new(&device, render_config.max_rectangles)? };

        log::info!("D3D11 renderer initialized ({}x{})", size.width, size.height);

        Ok(Renderer {
            device,
            ctx,
            swap_chain,
            rtv,
            rasterizer,
            blend,
            rects,
            width: size.width.max(1),
            height: size.height.max(1),
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.width = width;
        self.height = height;

        // Release old RTV before resizing
        drop(std::mem::replace(
            &mut self.rtv,
            unsafe { std::mem::zeroed() },
        ));

        unsafe {
            self.swap_chain
                .ResizeBuffers(0, width, height, DXGI_FORMAT_UNKNOWN, 0)
                .expect("ResizeBuffers failed");
            self.rtv = create_rtv(&self.device, &self.swap_chain).expect("create_rtv failed");
        }
    }

    pub fn apply_surface(&mut self) {
        // D3D11 resize is handled synchronously in resize()
    }

    pub fn surface_size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn create_atlas(
        &mut self,
        font_size_pt: f32,
        dpi_scale: f64,
        family_name: &str,
        primary_font_path: Option<(String, u32)>,
        render_config: &RenderConfig,
    ) -> (GlyphCache, GlyphAtlasGpu) {
        let cache = GlyphCache::new(
            font_size_pt,
            dpi_scale,
            family_name,
            primary_font_path,
            render_config,
        );

        let alpha = unsafe {
            DxAtlasLayer::new(
                &self.device,
                cache.atlas_size,
                cache.max_instances,
                DXGI_FORMAT_R8_UNORM,
                1,
                GLYPH_HLSL,
                ALPHA_PS_HLSL,
                D3D11_FILTER_MIN_MAG_MIP_POINT,
            )
            .expect("alpha atlas creation failed")
        };

        let color = unsafe {
            DxAtlasLayer::new(
                &self.device,
                cache.atlas_size,
                cache.max_instances,
                DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
                4,
                GLYPH_HLSL,
                COLOR_PS_HLSL,
                D3D11_FILTER_MIN_MAG_MIP_LINEAR,
            )
            .expect("color atlas creation failed")
        };

        (cache, GlyphAtlasGpu { alpha, color })
    }

    pub fn destroy_atlas(&self, _atlas_gpu: &mut GlyphAtlasGpu) {
        // D3D11 COM objects are dropped automatically via Drop
    }

    pub fn draw_frame(
        &mut self,
        atlas_gpu: &mut GlyphAtlasGpu,
        cache: &mut GlyphCache,
        scene: FrameScene,
    ) {
        let vw = self.width as f32;
        let vh = self.height as f32;

        unsafe {
            // Flush pending glyph uploads
            let (mut ap, mut cp, ac, cc) = cache.take_pending();
            atlas_gpu.alpha.flush_uploads(&self.ctx, &mut ap, ac);
            atlas_gpu.color.flush_uploads(&self.ctx, &mut cp, cc);

            // Set render target
            self.ctx
                .OMSetRenderTargets(Some(&[Some(self.rtv.clone())]), None);
            self.ctx.RSSetState(Some(&self.rasterizer));
            self.ctx
                .OMSetBlendState(Some(&self.blend), None, 0xFFFFFFFF);

            // Viewport
            let vp = D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: vw,
                Height: vh,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            };
            self.ctx.RSSetViewports(Some(&[vp]));

            // Clear
            self.ctx
                .ClearRenderTargetView(&self.rtv, &[0.0, 0.0, 0.0, 1.0]);

            // Full-screen scissor for rects
            let full_rect = RECT {
                left: 0,
                top: 0,
                right: self.width as i32,
                bottom: self.height as i32,
            };
            self.ctx.RSSetScissorRects(Some(&[full_rect]));

            // Background rects: clear rect + per-cell rects in one draw call.
            // Must be a single batch because the rect pipeline reuses one
            // buffer — a second render() overwrites before the first draws.
            let mut all_bg = Vec::with_capacity(1 + scene.bg_rects.len());
            all_bg.push(Rect {
                x: 0.0,
                y: 0.0,
                w: vw,
                h: vh,
                color: scene.clear_color,
            });
            all_bg.extend_from_slice(scene.bg_rects);
            self.rects.render(&self.ctx, &all_bg, vw, vh);

            // 2. Alpha text glyphs
            atlas_gpu.alpha.render_scissored(
                &self.ctx,
                scene.glyphs,
                vw,
                vh,
                self.width,
                self.height,
                scene.glyph_batches,
                scene.pane_glyph_end,
            );

            // 3. Color emoji
            atlas_gpu.color.render_scissored(
                &self.ctx,
                scene.color_glyphs,
                vw,
                vh,
                self.width,
                self.height,
                scene.color_glyph_batches,
                scene.pane_color_glyph_end,
            );

            // Present
            self.swap_chain.Present(1, DXGI_PRESENT(0)).ok();
        }
    }
}

unsafe fn create_rtv(
    device: &ID3D11Device,
    swap_chain: &IDXGISwapChain,
) -> Result<ID3D11RenderTargetView> {
    let back_buffer: ID3D11Texture2D = swap_chain.GetBuffer(0)?;
    let rtv = device.CreateRenderTargetView(&back_buffer, None)?;
    Ok(rtv)
}

// Windows RECT type for scissor
#[repr(C)]
struct RECT {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
