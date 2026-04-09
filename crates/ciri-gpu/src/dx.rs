//! Direct3D 11 backend for Windows.
//!
//! Uses D3D11 instanced rendering for rects, alpha text, and color emoji.
//! HLSL shaders are compiled at runtime via D3DCompile (Fxc).

use anyhow::Result;
use ciri_config::config::RenderConfig;
use ciri_render::FrameScene;
use ciri_render::glyph_cache::{GlyphCache, GlyphInstance, PendingUpload, ScissoredRange};
use ciri_render::rect::Rect;
use std::sync::Arc;
use winit::window::Window;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{BOOL, HANDLE, RECT};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D::Fxc::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::Threading::WaitForSingleObjectEx;
use windows::core::*;

use ciri_render::glyph_cache::PendingDwriteGlyph;

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
    output.color = input.color;
    return output;
}

float4 ps_main(PSInput input) : SV_TARGET {
    return float4(input.color.rgb * input.color.a, input.color.a);
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

float4 ps_main(PSInput input) : SV_TARGET {
    float alpha = atlas_tex.Sample(atlas_sampler, input.uv).a;
    float out_alpha = input.color.a * alpha;
    return float4(input.color.rgb * out_alpha, out_alpha);
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
    unsafe {
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
    swizzle_rgba_to_bgra: bool,
    /// D2D render target for direct DWrite glyph rendering to this atlas layer.
    d2d_rt: ID2D1RenderTarget,
}

struct DxAtlasLayerConfig<'a> {
    atlas_size: u32,
    max_instances: usize,
    tex_format: DXGI_FORMAT,
    bpp: u32,
    swizzle_rgba_to_bgra: bool,
    vs_hlsl: &'a str,
    ps_hlsl: &'a str,
    filter: D3D11_FILTER,
    d2d_factory: &'a ID2D1Factory,
    /// D2D text antialias mode for this layer.
    text_antialias: D2D1_TEXT_ANTIALIAS_MODE,
}

impl DxAtlasLayer {
    unsafe fn new(device: &ID3D11Device, cfg: &DxAtlasLayerConfig<'_>) -> Result<Self> {
        let atlas_size = cfg.atlas_size;
        let max_instances = cfg.max_instances;
        let tex_format = cfg.tex_format;
        let bpp = cfg.bpp;
        let filter = cfg.filter;
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
            BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut texture = None;
        device.CreateTexture2D(&tex_desc, None, Some(&mut texture))?;
        let texture = texture.unwrap();

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
        let resource: ID3D11Resource = texture.cast()?;
        let mut srv = None;
        device.CreateShaderResourceView(&resource, Some(&srv_desc), Some(&mut srv))?;
        let srv = srv.unwrap();

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
        let mut sampler = None;
        device.CreateSamplerState(&sampler_desc, Some(&mut sampler))?;
        let sampler = sampler.unwrap();

        // Shaders
        let vs_blob = compile_shader(cfg.vs_hlsl, "vs_main", "vs_5_0")?;
        let vs_code = std::slice::from_raw_parts(
            vs_blob.GetBufferPointer() as *const u8,
            vs_blob.GetBufferSize(),
        );
        let mut vs = None;
        device.CreateVertexShader(vs_code, None, Some(&mut vs))?;
        let vs = vs.unwrap();

        let ps_blob = compile_shader(cfg.ps_hlsl, "ps_main", "ps_5_0")?;
        let ps_code = std::slice::from_raw_parts(
            ps_blob.GetBufferPointer() as *const u8,
            ps_blob.GetBufferSize(),
        );
        let mut ps = None;
        device.CreatePixelShader(ps_code, None, Some(&mut ps))?;
        let ps = ps.unwrap();

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
        let mut input_layout = None;
        device.CreateInputLayout(&layout_desc, vs_code, Some(&mut input_layout))?;
        let input_layout = input_layout.unwrap();

        // Instance buffer
        let buf_desc = D3D11_BUFFER_DESC {
            ByteWidth: (max_instances * std::mem::size_of::<GlyphInstance>()) as u32,
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            ..Default::default()
        };
        let mut instance_buffer = None;
        device.CreateBuffer(&buf_desc, None, Some(&mut instance_buffer))?;
        let instance_buffer = instance_buffer.unwrap();

        // Constant buffer (viewport)
        let cb_desc = D3D11_BUFFER_DESC {
            ByteWidth: 16, // vec4 (viewport_size + pad)
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            ..Default::default()
        };
        let mut cbuffer = None;
        device.CreateBuffer(&cb_desc, None, Some(&mut cbuffer))?;
        let cbuffer = cbuffer.unwrap();

        // D2D render target from the atlas texture's DXGI surface.
        let dxgi_surface: IDXGISurface = texture.cast()?;
        let rt_props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: tex_format,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let d2d_rt = cfg
            .d2d_factory
            .CreateDxgiSurfaceRenderTarget(&dxgi_surface, &rt_props)?;
        d2d_rt.SetTextAntialiasMode(cfg.text_antialias);

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
            swizzle_rgba_to_bgra: cfg.swizzle_rgba_to_bgra,
            d2d_rt,
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
            let upload_data = if self.swizzle_rgba_to_bgra && self.bpp == 4 {
                rgba_to_bgra(&upload.data)
            } else {
                upload.data
            };
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
                upload_data.as_ptr() as *const _,
                row_pitch,
                0,
            );
        }
    }

    /// Render pending DWrite glyphs to the atlas via D2D DrawGlyphRun.
    unsafe fn flush_dwrite_glyphs(
        &self,
        pending: &mut Vec<PendingDwriteGlyph>,
        pending_clear: bool,
    ) {
        if pending.is_empty() && !pending_clear {
            return;
        }

        self.d2d_rt.BeginDraw();

        if pending_clear {
            let clear_color = D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            };
            self.d2d_rt.Clear(Some(&clear_color));
        }

        for glyph in pending.drain(..) {
            let white = D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            };
            let brush = self
                .d2d_rt
                .CreateSolidColorBrush(std::ptr::from_ref(&white), None)
                .unwrap();

            let glyph_index = glyph.glyph_index;
            let glyph_run = DWRITE_GLYPH_RUN {
                fontFace: std::mem::ManuallyDrop::new(Some(glyph.face.clone())),
                fontEmSize: glyph.pixel_size,
                glyphCount: 1,
                glyphIndices: &glyph_index,
                glyphAdvances: std::ptr::null(),
                glyphOffsets: std::ptr::null(),
                isSideways: BOOL(0),
                bidiLevel: 0,
            };

            let origin = D2D_POINT_2F {
                x: glyph.baseline_x,
                y: glyph.baseline_y,
            };

            self.d2d_rt
                .DrawGlyphRun(origin, &glyph_run, &brush, DWRITE_MEASURING_MODE_NATURAL);

            // Release the cloned face reference.
            let mut glyph_run = glyph_run;
            std::mem::ManuallyDrop::drop(&mut glyph_run.fontFace);
        }

        let _ = self.d2d_rt.EndDraw(None, None);
    }

    /// Upload glyph instances to the GPU. Called once per frame per layer.
    unsafe fn upload_instances(
        &self,
        ctx: &ID3D11DeviceContext,
        instances: &[GlyphInstance],
        vp: &crate::ViewportDims,
    ) {
        if instances.is_empty() {
            return;
        }
        let count = instances.len().min(self.max_instances);

        // Update viewport cbuffer
        let viewport = [vp.width, vp.height, 0.0f32, 0.0f32];
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        ctx.Map(
            &self.cbuffer,
            0,
            D3D11_MAP_WRITE_DISCARD,
            0,
            Some(&mut mapped),
        )
        .unwrap();
        std::ptr::copy_nonoverlapping(viewport.as_ptr() as *const u8, mapped.pData as *mut u8, 16);
        ctx.Unmap(&self.cbuffer, 0);

        // Update instance buffer
        let data = bytemuck::cast_slice(&instances[..count]);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        ctx.Map(
            &self.instance_buffer,
            0,
            D3D11_MAP_WRITE_DISCARD,
            0,
            Some(&mut mapped),
        )
        .unwrap();
        std::ptr::copy_nonoverlapping(data.as_ptr(), mapped.pData as *mut u8, data.len());
        ctx.Unmap(&self.instance_buffer, 0);
    }

    /// Bind pipeline state and draw scissored glyph batches.
    /// Can be called multiple times after a single `upload_instances`.
    unsafe fn draw_batches(
        &self,
        ctx: &ID3D11DeviceContext,
        instance_count: usize,
        batches: &[ScissoredRange],
    ) {
        if instance_count == 0 || batches.is_empty() {
            return;
        }
        let count = instance_count.min(self.max_instances);
        let stride = std::mem::size_of::<GlyphInstance>() as u32;

        // Bind pipeline state (may have been changed by rect draws between calls)
        ctx.IASetInputLayout(Some(&self.input_layout));
        ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
        ctx.VSSetShader(Some(&self.vs), None);
        ctx.PSSetShader(Some(&self.ps), None);
        ctx.VSSetConstantBuffers(0, Some(&[Some(self.cbuffer.clone())]));
        ctx.PSSetShaderResources(0, Some(&[Some(self.srv.clone())]));
        ctx.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));

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
    }
}

fn rgba_to_bgra(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for px in data.chunks_exact(4) {
        out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
    }
    out
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
        let vs_code = std::slice::from_raw_parts(
            vs_blob.GetBufferPointer() as *const u8,
            vs_blob.GetBufferSize(),
        );
        let mut vs = None;
        device.CreateVertexShader(vs_code, None, Some(&mut vs))?;
        let vs = vs.unwrap();

        let ps_blob = compile_shader(RECT_HLSL, "ps_main", "ps_5_0")?;
        let ps_code = std::slice::from_raw_parts(
            ps_blob.GetBufferPointer() as *const u8,
            ps_blob.GetBufferSize(),
        );
        let mut ps = None;
        device.CreatePixelShader(ps_code, None, Some(&mut ps))?;
        let ps = ps.unwrap();

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
        let mut input_layout = None;
        device.CreateInputLayout(&layout_desc, vs_code, Some(&mut input_layout))?;
        let input_layout = input_layout.unwrap();

        let buf_desc = D3D11_BUFFER_DESC {
            ByteWidth: (max_rects * std::mem::size_of::<Rect>()) as u32,
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            ..Default::default()
        };
        let mut instance_buffer = None;
        device.CreateBuffer(&buf_desc, None, Some(&mut instance_buffer))?;
        let instance_buffer = instance_buffer.unwrap();

        let cb_desc = D3D11_BUFFER_DESC {
            ByteWidth: 16,
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            ..Default::default()
        };
        let mut cbuffer = None;
        device.CreateBuffer(&cb_desc, None, Some(&mut cbuffer))?;
        let cbuffer = cbuffer.unwrap();

        Ok(DxRectPipeline {
            vs,
            ps,
            input_layout,
            instance_buffer,
            cbuffer,
            max_rects,
        })
    }

    /// Upload all rect instance data to the GPU buffer.
    unsafe fn upload(
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
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        ctx.Map(
            &self.cbuffer,
            0,
            D3D11_MAP_WRITE_DISCARD,
            0,
            Some(&mut mapped),
        )
        .unwrap();
        std::ptr::copy_nonoverlapping(viewport.as_ptr() as *const u8, mapped.pData as *mut u8, 16);
        ctx.Unmap(&self.cbuffer, 0);

        // Instance data
        let data = bytemuck::cast_slice(&rects[..count]);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        ctx.Map(
            &self.instance_buffer,
            0,
            D3D11_MAP_WRITE_DISCARD,
            0,
            Some(&mut mapped),
        )
        .unwrap();
        std::ptr::copy_nonoverlapping(data.as_ptr(), mapped.pData as *mut u8, data.len());
        ctx.Unmap(&self.instance_buffer, 0);
    }

    /// Draw a range of previously uploaded rects.
    unsafe fn draw_range(&self, ctx: &ID3D11DeviceContext, start: usize, count: usize) {
        if count == 0 {
            return;
        }
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
        ctx.DrawInstanced(4, count as u32, 0, start as u32);
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
    d2d_factory: ID2D1Factory,
    rects: DxRectPipeline,
    width: u32,
    height: u32,
    sync_interval: u32,
    /// Waitable object for DXGI frame latency — lets the CPU sleep instead of
    /// busy-waiting in `Present(1)`. `None` if the driver doesn't support it.
    frame_waitable: Option<HANDLE>,
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
            OutputWindow: unsafe { std::mem::transmute(hwnd) },
            Windowed: true.into(),
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            Flags: DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0 as u32,
        };

        let mut device = None;
        let mut ctx = None;
        let mut swap_chain = None;

        unsafe {
            D3D11CreateDeviceAndSwapChain(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
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

        // Try to get the waitable object for low-CPU vsync.
        // IDXGISwapChain2 is available on Windows 8.1+.
        let frame_waitable = swap_chain
            .cast::<IDXGISwapChain2>()
            .ok()
            .and_then(|sc2| unsafe {
                let _ = sc2.SetMaximumFrameLatency(render_config.frame_latency);
                let handle = sc2.GetFrameLatencyWaitableObject();
                if handle.is_invalid() {
                    log::warn!("DXGI frame latency waitable object not available");
                    None
                } else {
                    log::info!("using DXGI frame latency waitable object for low-CPU vsync");
                    Some(handle)
                }
            });

        let rtv = unsafe { create_rtv(&device, &swap_chain)? };

        // Rasterizer state with scissor enabled
        let raster_desc = D3D11_RASTERIZER_DESC {
            FillMode: D3D11_FILL_SOLID,
            CullMode: D3D11_CULL_NONE,
            ScissorEnable: true.into(),
            ..Default::default()
        };
        let rasterizer = unsafe {
            let mut rasterizer = None;
            device.CreateRasterizerState(&raster_desc, Some(&mut rasterizer))?;
            rasterizer.unwrap()
        };

        // Blend state (alpha blending)
        let mut blend_desc = D3D11_BLEND_DESC::default();
        blend_desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: true.into(),
            SrcBlend: D3D11_BLEND_ONE,
            DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOp: D3D11_BLEND_OP_ADD,
            SrcBlendAlpha: D3D11_BLEND_ONE,
            DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOpAlpha: D3D11_BLEND_OP_ADD,
            RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
        };
        let blend = unsafe {
            let mut blend = None;
            device.CreateBlendState(&blend_desc, Some(&mut blend))?;
            blend.unwrap()
        };

        let rects = unsafe { DxRectPipeline::new(&device, render_config.max_rectangles)? };

        let sync_interval = match render_config.present_mode {
            ciri_config::config::PresentMode::Immediate
            | ciri_config::config::PresentMode::Mailbox => 0,
            _ => 1,
        };

        let d2d_factory: ID2D1Factory =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };

        log::info!(
            "D3D11 renderer initialized ({}x{})",
            size.width,
            size.height
        );

        Ok(Renderer {
            device,
            ctx,
            swap_chain,
            rtv,
            rasterizer,
            blend,
            d2d_factory,
            rects,
            width: size.width.max(1),
            height: size.height.max(1),
            sync_interval,
            frame_waitable,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.width = width;
        self.height = height;

        unsafe {
            // Drop the old RTV before resizing the swap chain.
            std::ptr::drop_in_place(&mut self.rtv);

            self.swap_chain
                .ResizeBuffers(
                    0,
                    width,
                    height,
                    DXGI_FORMAT_UNKNOWN,
                    if self.frame_waitable.is_some() {
                        DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT
                    } else {
                        DXGI_SWAP_CHAIN_FLAG(0)
                    },
                )
                .expect("ResizeBuffers failed");

            // Write the new RTV without dropping the (now-invalid) old value.
            std::ptr::write(
                &mut self.rtv,
                create_rtv(&self.device, &self.swap_chain).expect("create_rtv failed"),
            );
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
        params: &ciri_render::glyph_cache::FontInitParams,
    ) -> (GlyphCache, GlyphAtlasGpu) {
        let mut cache = GlyphCache::new(params);
        cache.set_d2d_rendering(true);

        // Both atlas layers use B8G8R8A8 for D2D render target compatibility.
        let alpha = unsafe {
            DxAtlasLayer::new(
                &self.device,
                &DxAtlasLayerConfig {
                    atlas_size: cache.atlas_size,
                    max_instances: cache.max_instances,
                    tex_format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    bpp: 4,
                    swizzle_rgba_to_bgra: false,
                    vs_hlsl: GLYPH_HLSL,
                    ps_hlsl: ALPHA_PS_HLSL,
                    filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                    d2d_factory: &self.d2d_factory,
                    text_antialias: D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
                },
            )
            .expect("alpha atlas creation failed")
        };

        let color = unsafe {
            DxAtlasLayer::new(
                &self.device,
                &DxAtlasLayerConfig {
                    atlas_size: cache.atlas_size,
                    max_instances: cache.max_instances,
                    tex_format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    bpp: 4,
                    swizzle_rgba_to_bgra: true,
                    vs_hlsl: GLYPH_HLSL,
                    ps_hlsl: COLOR_PS_HLSL,
                    filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                    d2d_factory: &self.d2d_factory,
                    text_antialias: D2D1_TEXT_ANTIALIAS_MODE_DEFAULT,
                },
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
            // Flush pending glyph uploads (CPU path, used when D2D is off)
            let (mut ap, mut cp, ac, cc) = cache.take_pending();
            atlas_gpu.alpha.flush_uploads(&self.ctx, &mut ap, ac);
            atlas_gpu.color.flush_uploads(&self.ctx, &mut cp, cc);

            // Flush DWrite glyph render commands via D2D direct-to-atlas
            let (mut dwa, mut dwc) = cache.take_dwrite_pending();
            atlas_gpu.alpha.flush_dwrite_glyphs(&mut dwa, ac);
            atlas_gpu.color.flush_dwrite_glyphs(&mut dwc, cc);

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
                .ClearRenderTargetView(&self.rtv, &scene.clear_color);

            // Full-screen scissor for rects
            let full_rect = RECT {
                left: 0,
                top: 0,
                right: self.width as i32,
                bottom: self.height as i32,
            };
            self.ctx.RSSetScissorRects(Some(&[full_rect]));

            // 1. Upload all background rects (clear + pane + overlay) once.
            let mut all_bg = Vec::with_capacity(1 + scene.bg_rects.len());
            all_bg.push(Rect {
                x: 0.0,
                y: 0.0,
                w: vw,
                h: vh,
                color: scene.clear_color,
            });
            all_bg.extend_from_slice(scene.bg_rects);
            let active_bg_idx = 1 + scene.active_bg_start;
            let overlay_bg_idx = 1 + scene.overlay_bg_start;
            let total_bg = all_bg.len().min(self.rects.max_rects);
            self.rects.upload(&self.ctx, &all_bg, vw, vh);

            // 2. Draw non-focused pane background rects.
            let inactive_bg_count = active_bg_idx.min(total_bg);
            self.rects.draw_range(&self.ctx, 0, inactive_bg_count);

            let vp = crate::ViewportDims {
                width: vw,
                height: vh,
                width_px: self.width,
                height_px: self.height,
            };

            // 3. Upload alpha + color glyph instances once.
            atlas_gpu
                .alpha
                .upload_instances(&self.ctx, scene.glyphs, &vp);
            atlas_gpu
                .color
                .upload_instances(&self.ctx, scene.color_glyphs, &vp);
            let alpha_count = scene.glyphs.len();
            let color_count = scene.color_glyphs.len();

            // 4. Draw inactive pane glyphs (scissored).
            atlas_gpu
                .alpha
                .draw_batches(&self.ctx, alpha_count, scene.glyph_batches);
            atlas_gpu
                .color
                .draw_batches(&self.ctx, color_count, scene.color_glyph_batches);

            // 5. Focused pane background rects.
            let active_bg_count = overlay_bg_idx.saturating_sub(active_bg_idx);
            if active_bg_count > 0 {
                self.ctx.RSSetScissorRects(Some(&[full_rect]));
                self.rects
                    .draw_range(&self.ctx, active_bg_idx, active_bg_count);
            }

            // 6. Draw active pane glyphs (scissored, no re-upload).
            atlas_gpu
                .alpha
                .draw_batches(&self.ctx, alpha_count, scene.active_glyph_batches);
            atlas_gpu
                .color
                .draw_batches(&self.ctx, color_count, scene.active_color_glyph_batches);

            // 7. Overlay background rects.
            let overlay_bg_count = total_bg.saturating_sub(overlay_bg_idx);
            if overlay_bg_count > 0 {
                self.ctx.RSSetScissorRects(Some(&[full_rect]));
                self.rects
                    .draw_range(&self.ctx, overlay_bg_idx, overlay_bg_count);
            }

            // 8. Overlay glyphs (no re-upload, just draw remaining range).
            let overlay_alpha = ScissoredRange {
                x: 0,
                y: 0,
                w: self.width,
                h: self.height,
                start: scene.pane_glyph_end,
                end: scene.glyphs.len(),
            };
            let overlay_color = ScissoredRange {
                x: 0,
                y: 0,
                w: self.width,
                h: self.height,
                start: scene.pane_color_glyph_end,
                end: scene.color_glyphs.len(),
            };
            atlas_gpu
                .alpha
                .draw_batches(&self.ctx, alpha_count, &[overlay_alpha]);
            atlas_gpu
                .color
                .draw_batches(&self.ctx, color_count, &[overlay_color]);

            // Wait for the previous frame to finish presentation before
            // submitting the next one. With the waitable object this is a true
            // kernel wait (CPU sleeps), not a busy-wait spin loop.
            if let Some(handle) = self.frame_waitable {
                WaitForSingleObjectEx(handle, 1000, false);
            }

            // Present — sync_interval=0 when using waitable object (latency
            // is controlled by SetMaximumFrameLatency instead).
            let interval = if self.frame_waitable.is_some() {
                0
            } else {
                self.sync_interval
            };
            let _ = self.swap_chain.Present(interval, DXGI_PRESENT(0)).ok();
        }
    }
}

unsafe fn create_rtv(
    device: &ID3D11Device,
    swap_chain: &IDXGISwapChain,
) -> Result<ID3D11RenderTargetView> {
    let back_buffer: ID3D11Texture2D = swap_chain.GetBuffer(0)?;
    let resource: ID3D11Resource = back_buffer.cast()?;
    let mut rtv = None;
    device.CreateRenderTargetView(&resource, None, Some(&mut rtv))?;
    Ok(rtv.unwrap())
}
