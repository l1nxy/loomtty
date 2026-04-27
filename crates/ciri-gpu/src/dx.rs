//! Direct3D 11 backend for Windows.
//!
//! Uses D3D11 instanced rendering for rects, alpha text, and color emoji.
//! HLSL shaders are compiled at runtime via D3DCompile (Fxc).

use anyhow::Result;
use ciri_config::config::RenderConfig;
use ciri_render::FrameScene;
use ciri_render::glyph_cache::{GlyphCache, GlyphInstance, PaneGlyphRange, PendingUpload};
use ciri_render::rect::{PaneRectRange, Rect};
use ciri_render::sdf_rect::SdfRect;

/// Upper bound on SDF chrome rects per frame. Mirrors the blade backend.
const MAX_SDF_RECTS: usize = 256;
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

const RECT_COMMON_HLSL: &str = r#"
cbuffer Viewport : register(b0) {
    float2 viewport_size;
    float2 _pad0;
    float2 pane_origin;
    float2 pane_size;
    float4 pane_radii;
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
    float2 pane_local : PANELOCAL;
};
"#;

const RECT_VS_HLSL: &str = r#"
// RECT_COMMON_HLSL_PLACEHOLDER
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
    output.pane_local = px - pane_origin;
    return output;
}
"#;

const RECT_PS_HLSL: &str = r#"
// RECT_COMMON_HLSL_PLACEHOLDER

// HLSL_CORNER_FUNCS_PLACEHOLDER

float4 ps_main(PSInput input) : SV_TARGET {
    float4 out_color = float4(input.color.rgb * input.color.a, input.color.a);
    return out_color * ciri_corner_alpha(input.pane_local, pane_size, pane_radii);
}
"#;

const SDF_HLSL: &str = r#"
cbuffer Viewport : register(b0) {
    float2 viewport_size;
    float2 _pad;
};

// Matches ciri_render::sdf_rect::SdfRect; offsets mirror the blade
// WGSL SdfInstance struct (pos@0, size@8, color@16, radii@32,
// border_color@48, border_width@64, shadow_blur@68, shadow_offset@72,
// shadow_color@80).
struct VSInput {
    float2 pos            : POS;
    float2 size           : SIZE;
    float4 color          : COL;
    float4 radii          : RADII;
    float4 border_color   : BCOL;
    float  border_width   : BW;
    float  shadow_blur    : SBLUR;
    float2 shadow_offset  : SOFF;
    float4 shadow_color   : SCOL;
    uint   vid            : SV_VertexID;
};

struct PSInput {
    float4 position       : SV_POSITION;
    float2 local          : LOCAL;
    float2 half_size      : HALFSIZE;
    float4 color          : COL;
    float4 radii          : RADII;
    float4 border_color   : BCOL;
    float  border_width   : BW;
    float  shadow_blur    : SBLUR;
    float2 shadow_offset  : SOFF;
    float4 shadow_color   : SCOL;
};

float shadow_pad(float shadow_blur, float2 shadow_offset) {
    return shadow_blur * 3.0 + max(abs(shadow_offset.x), abs(shadow_offset.y));
}

PSInput vs_main(VSInput input) {
    float x = float(input.vid & 1);
    float y = float((input.vid >> 1) & 1);

    // Inflate the quad so shadow blur + offset spill outside the rect's
    // bounds without clipping. 3σ covers ~99.7% of a Gaussian envelope.
    float pad = shadow_pad(input.shadow_blur, input.shadow_offset);
    float2 padded_pos = input.pos - float2(pad, pad);
    float2 padded_size = input.size + float2(pad * 2.0, pad * 2.0);

    float2 px = padded_pos + float2(x, y) * padded_size;
    float2 ndc = float2(
        px.x / viewport_size.x * 2.0 - 1.0,
        1.0 - px.y / viewport_size.y * 2.0
    );

    float2 centre = input.pos + input.size * 0.5;
    float2 local = px - centre;

    PSInput output;
    output.position = float4(ndc, 0.0, 1.0);
    output.local = local;
    output.half_size = input.size * 0.5;
    output.color = input.color;
    output.radii = input.radii;
    output.border_color = input.border_color;
    output.border_width = input.border_width;
    output.shadow_blur = input.shadow_blur;
    output.shadow_offset = input.shadow_offset;
    output.shadow_color = input.shadow_color;
    return output;
}

// SDF of a rounded box centred at the origin. Per-corner radii order
// matches CSS: tl, tr, br, bl. Picks the corner based on which quadrant
// the sample point falls in.
float sdf_rounded_box(float2 p, float2 b, float4 r) {
    float r_top_x = p.x > 0.0 ? r.y : r.x;   // tl | tr
    float r_bot_x = p.x > 0.0 ? r.z : r.w;   // bl | br
    float radius = p.y > 0.0 ? r_bot_x : r_top_x;
    float2 q = abs(p) - b + float2(radius, radius);
    return min(max(q.x, q.y), 0.0) + length(max(q, float2(0.0, 0.0))) - radius;
}

float shadow_envelope(float d, float blur) {
    if (blur <= 0.0) { return 0.0; }
    return clamp(0.5 - 0.5 * d / blur, 0.0, 1.0);
}

float4 ps_main(PSInput input) : SV_TARGET {
    float d_body = sdf_rounded_box(input.local, input.half_size, input.radii);

    // DPR-aware one-pixel AA via fwidth — matches blade WGSL.
    float aa = max(fwidth(d_body) * 0.5, 1e-5);
    float body_alpha = clamp(0.5 - d_body / (aa * 2.0), 0.0, 1.0);

    float border_alpha = 0.0;
    if (input.border_width > 0.0) {
        float half_bw = input.border_width * 0.5;
        float d_band = abs(d_body + half_bw) - half_bw;
        border_alpha = clamp(0.5 - d_band / (aa * 2.0), 0.0, 1.0);
    }

    float4 shadow_col = float4(0.0, 0.0, 0.0, 0.0);
    if (input.shadow_blur > 0.0 && input.shadow_color.a > 0.0) {
        float d_shadow = sdf_rounded_box(input.local - input.shadow_offset,
                                         input.half_size, input.radii);
        float env = shadow_envelope(d_shadow, input.shadow_blur);
        // Shadow occluded by the body itself to avoid a double-dark ring.
        float occlusion = 1.0 - body_alpha;
        float a = env * input.shadow_color.a * occlusion;
        shadow_col = float4(input.shadow_color.rgb * a, a);
    }

    // Pre-multiply so the backend's OVER blend composites correctly.
    float4 body = float4(input.color.rgb * input.color.a * body_alpha,
                         input.color.a * body_alpha);
    float4 border = float4(input.border_color.rgb * input.border_color.a * border_alpha,
                           input.border_color.a * border_alpha);

    // Shadow under everything, border over body.
    float3 out_rgb = shadow_col.rgb * (1.0 - body.a)
                   + body.rgb * (1.0 - border.a)
                   + border.rgb;
    float  out_a   = shadow_col.a   * (1.0 - body.a)
                   + body.a   * (1.0 - border.a)
                   + border.a;
    return float4(out_rgb, out_a);
}
"#;

const GLYPH_HLSL: &str = r#"
cbuffer Viewport : register(b0) {
    float2 viewport_size;
    float2 _pad;
    uint blending_flags; // bit 0 = linear, bit 1 = correction
    uint _pad1;
    uint _pad2;
    uint _pad3;
    float2 pane_origin;
    float2 pane_size;
    float4 pane_radii;
};

Texture2D atlas_tex : register(t0);
SamplerState atlas_sampler : register(s0);

struct VSInput {
    float2 pos      : POS;
    float2 size     : SIZE;
    float2 uv_pos   : UVPOS;
    float2 uv_size  : UVSIZE;
    float4 color    : COLOR;
    float4 bg_color : BGCOL;
    uint vid        : SV_VertexID;
};

struct PSInput {
    float4 position : SV_POSITION;
    float2 uv       : TEXCOORD0;
    float4 color    : COLOR;
    float4 bg_color : BGCOL;
    float2 pane_local : PANELOCAL;
};

PSInput vs_main(VSInput input) {
    float x = float(input.vid & 1);
    float y = float((input.vid >> 1) & 1);

    PSInput output;
    output.uv = input.uv_pos + float2(x, y) * input.uv_size;
    output.color = input.color;
    output.bg_color = input.bg_color;

    float2 px = input.pos + float2(x, y) * input.size;
    output.pane_local = px - pane_origin;
    float2 ndc = float2(
        px.x / viewport_size.x * 2.0 - 1.0,
        1.0 - px.y / viewport_size.y * 2.0
    );
    output.position = float4(ndc, 0.0, 1.0);
    return output;
}
"#;

// Per-corner rounding alpha mask. HLSL twin of `GLSL_CORNER_FUNCS` —
// same Inigo Quilez SDF recipe, same shape as ciri's existing
// `sdf_rounded_box` over in the SDF chrome path. `radii` order is CSS:
// tl, tr, br, bl. All-zero radii short-circuits to 1.0.
const HLSL_CORNER_FUNCS: &str = r#"
float ciri_sdf_rounded_box(float2 p, float2 b, float4 r) {
    float rx = (p.x > 0.0) ? r.y : r.x;
    float bx = (p.x > 0.0) ? r.z : r.w;
    float radius = (p.y > 0.0) ? bx : rx;
    float2 q = abs(p) - b + float2(radius, radius);
    return min(max(q.x, q.y), 0.0) + length(max(q, float2(0.0, 0.0))) - radius;
}

float ciri_corner_alpha(float2 px, float2 size, float4 radii) {
    if (radii.x <= 0.0 && radii.y <= 0.0 && radii.z <= 0.0 && radii.w <= 0.0) {
        return 1.0;
    }
    float d = ciri_sdf_rounded_box(px - 0.5 * size, 0.5 * size, radii);
    // smoothstep spans 2 * aa, so use half-pixel derivative to land a
    // one-pixel-wide AA transition. Matches the GL twin and the
    // chrome SDF path.
    float aa = max(fwidth(d) * 0.5, 1e-5);
    return 1.0 - smoothstep(-aa, aa, d);
}
"#;

const HLSL_COLOR_FUNCS: &str = r#"
float linearize_f(float v) {
    return (v <= 0.04045) ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4);
}
float4 linearize_v4(float4 srgb) {
    return float4(linearize_f(srgb.r), linearize_f(srgb.g), linearize_f(srgb.b), srgb.a);
}
float unlinearize_f(float v) {
    return (v <= 0.0031308) ? v * 12.92 : pow(v, 1.0 / 2.4) * 1.055 - 0.055;
}
float4 unlinearize_v4(float4 lin) {
    return float4(unlinearize_f(lin.r), unlinearize_f(lin.g), unlinearize_f(lin.b), lin.a);
}
float luminance_linear(float3 col) {
    return dot(col, float3(0.2126, 0.7152, 0.0722));
}
"#;

const ALPHA_PS_HLSL: &str = r#"
Texture2D atlas_tex : register(t0);
SamplerState atlas_sampler : register(s0);

cbuffer Viewport : register(b0) {
    float2 viewport_size;
    float2 _pad;
    uint blending_flags;
    uint _pad1;
    uint _pad2;
    uint _pad3;
    float2 pane_origin;
    float2 pane_size;
    float4 pane_radii;
};

struct PSInput {
    float4 position : SV_POSITION;
    float2 uv       : TEXCOORD0;
    float4 color    : COLOR;
    float4 bg_color : BGCOL;
    float2 pane_local : PANELOCAL;
};

// HLSL_COLOR_FUNCS_PLACEHOLDER

// HLSL_CORNER_FUNCS_PLACEHOLDER

float4 ps_main(PSInput input) : SV_TARGET {
    bool use_linear_correction = (blending_flags & 2u) != 0u;

    float a = atlas_tex.Sample(atlas_sampler, input.uv).a;

    // Weight correction: linearize to compute luminance, adjust alpha
    // so sRGB-space hardware blending approximates linear compositing.
    if (use_linear_correction) {
        float4 fg_linear = linearize_v4(input.color);
        float4 bg_linear = linearize_v4(input.bg_color);
        float fg_l = luminance_linear(fg_linear.rgb);
        float bg_l = luminance_linear(bg_linear.rgb);
        if (abs(fg_l - bg_l) > 0.001) {
            float blend_l = linearize_f(
                unlinearize_f(fg_l) * a + unlinearize_f(bg_l) * (1.0 - a)
            );
            a = clamp((blend_l - bg_l) / (fg_l - bg_l), 0.0, 1.0);
        }
    }

    // Output sRGB premultiplied with corrected alpha.
    float out_alpha = input.color.a * a;
    float4 out_color = float4(input.color.rgb * out_alpha, out_alpha);
    return out_color * ciri_corner_alpha(input.pane_local, pane_size, pane_radii);
}
"#;

const COLOR_PS_HLSL: &str = r#"
Texture2D atlas_tex : register(t0);
SamplerState atlas_sampler : register(s0);

cbuffer Viewport : register(b0) {
    float2 viewport_size;
    float2 _pad;
    uint blending_flags;
    uint _pad1;
    uint _pad2;
    uint _pad3;
    float2 pane_origin;
    float2 pane_size;
    float4 pane_radii;
};

struct PSInput {
    float4 position : SV_POSITION;
    float2 uv       : TEXCOORD0;
    float4 color    : COLOR;
    float4 bg_color : BGCOL;
    float2 pane_local : PANELOCAL;
};

// HLSL_CORNER_FUNCS_PLACEHOLDER

float4 ps_main(PSInput input) : SV_TARGET {
    float4 texel = atlas_tex.Sample(atlas_sampler, input.uv);
    float4 out_color = float4(texel.rgb * input.color.rgb, texel.a * input.color.a);
    return out_color * ciri_corner_alpha(input.pane_local, pane_size, pane_radii);
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
    /// Packed blending flags: bit 0 = use_linear_blending, bit 1 = use_linear_correction.
    blending_flags: u32,
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
    /// System text rendering params (gamma / enhanced contrast).
    text_rendering_params: Option<IDWriteRenderingParams>,
    blending_flags: u32,
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
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR::from_raw(b"BGCOL\0".as_ptr()),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 48,
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

        // Constant buffer: viewport, blending flags, pane origin/size, pane radii.
        let cb_desc = D3D11_BUFFER_DESC {
            ByteWidth: 64,
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

        // Apply system default text rendering params (gamma, enhanced contrast).
        // Without this, D2D uses built-in defaults that can produce visible
        // fringe artifacts, especially on proportional UI fonts like Segoe UI.
        if let Some(ref params) = cfg.text_rendering_params {
            d2d_rt.SetTextRenderingParams(params);
        }

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
            blending_flags: cfg.blending_flags,
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

        // Initialize viewport cbuffer. Pane fields are rewritten per batch.
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
        let flags_data: [u32; 4] = [self.blending_flags, 0, 0, 0];
        std::ptr::copy_nonoverlapping(
            flags_data.as_ptr() as *const u8,
            (mapped.pData as *mut u8).add(16),
            16,
        );
        let pane_data = [0.0f32; 8];
        std::ptr::copy_nonoverlapping(
            pane_data.as_ptr() as *const u8,
            (mapped.pData as *mut u8).add(32),
            32,
        );
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
        vp: &crate::ViewportDims,
        batches: &[PaneGlyphRange],
    ) {
        if instance_count == 0 || batches.is_empty() {
            return;
        }
        let count = instance_count.min(self.max_instances);
        let stride = std::mem::size_of::<GlyphInstance>() as u32;
        let viewport = [vp.width, vp.height, 0.0f32, 0.0f32];
        let flags_data: [u32; 4] = [self.blending_flags, 0, 0, 0];

        // Bind pipeline state (may have been changed by rect draws between calls)
        ctx.IASetInputLayout(Some(&self.input_layout));
        ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
        ctx.VSSetShader(Some(&self.vs), None);
        ctx.PSSetShader(Some(&self.ps), None);
        ctx.VSSetConstantBuffers(0, Some(&[Some(self.cbuffer.clone())]));
        ctx.PSSetConstantBuffers(0, Some(&[Some(self.cbuffer.clone())]));
        ctx.PSSetShaderResources(0, Some(&[Some(self.srv.clone())]));
        ctx.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));

        for batch in batches {
            let (sx, sy, sw, sh) = batch.scissor;
            let start = (batch.start as usize).min(count);
            let end = (start + batch.count as usize).min(count);
            if start >= end || sw == 0 || sh == 0 {
                continue;
            }
            let pane_data = [
                batch.pane_origin[0],
                batch.pane_origin[1],
                batch.pane_size[0],
                batch.pane_size[1],
                batch.pane_radii[0],
                batch.pane_radii[1],
                batch.pane_radii[2],
                batch.pane_radii[3],
            ];
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            ctx.Map(
                &self.cbuffer,
                0,
                D3D11_MAP_WRITE_DISCARD,
                0,
                Some(&mut mapped),
            )
            .unwrap();
            std::ptr::copy_nonoverlapping(
                viewport.as_ptr() as *const u8,
                mapped.pData as *mut u8,
                16,
            );
            std::ptr::copy_nonoverlapping(
                flags_data.as_ptr() as *const u8,
                (mapped.pData as *mut u8).add(16),
                16,
            );
            std::ptr::copy_nonoverlapping(
                pane_data.as_ptr() as *const u8,
                (mapped.pData as *mut u8).add(32),
                32,
            );
            ctx.Unmap(&self.cbuffer, 0);
            let rect = RECT {
                left: sx as i32,
                top: sy as i32,
                right: (sx + sw) as i32,
                bottom: (sy + sh) as i32,
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
        let rect_vs = RECT_VS_HLSL.replace("// RECT_COMMON_HLSL_PLACEHOLDER", RECT_COMMON_HLSL);
        let rect_ps = RECT_PS_HLSL
            .replace("// RECT_COMMON_HLSL_PLACEHOLDER", RECT_COMMON_HLSL)
            .replace("// HLSL_CORNER_FUNCS_PLACEHOLDER", HLSL_CORNER_FUNCS);

        let vs_blob = compile_shader(&rect_vs, "vs_main", "vs_5_0")?;
        let vs_code = std::slice::from_raw_parts(
            vs_blob.GetBufferPointer() as *const u8,
            vs_blob.GetBufferSize(),
        );
        let mut vs = None;
        device.CreateVertexShader(vs_code, None, Some(&mut vs))?;
        let vs = vs.unwrap();

        let ps_blob = compile_shader(&rect_ps, "ps_main", "ps_5_0")?;
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
            ByteWidth: 48,
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            ..Default::default()
        };
        let initial_cb = [0.0f32; 12];
        let initial_data = D3D11_SUBRESOURCE_DATA {
            pSysMem: initial_cb.as_ptr() as *const _,
            ..Default::default()
        };
        let mut cbuffer = None;
        device.CreateBuffer(&cb_desc, Some(&initial_data), Some(&mut cbuffer))?;
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

        let _ = (viewport_w, viewport_h);

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

    /// Draw previously uploaded rects grouped by pane clipping uniforms.
    unsafe fn draw_ranges(
        &self,
        ctx: &ID3D11DeviceContext,
        ranges: &[PaneRectRange],
        viewport_w: f32,
        viewport_h: f32,
    ) {
        if ranges.is_empty() || !ranges.iter().any(|range| range.count > 0) {
            return;
        }
        ctx.IASetInputLayout(Some(&self.input_layout));
        ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
        let stride = std::mem::size_of::<Rect>() as u32;
        ctx.VSSetShader(Some(&self.vs), None);
        ctx.PSSetShader(Some(&self.ps), None);
        ctx.VSSetConstantBuffers(0, Some(&[Some(self.cbuffer.clone())]));
        ctx.PSSetConstantBuffers(0, Some(&[Some(self.cbuffer.clone())]));

        for range in ranges {
            let start = range.start as usize;
            let count = range.count as usize;
            if count == 0 || start >= self.max_rects {
                continue;
            }
            let count = count.min(self.max_rects - start);
            let cb_data = [
                viewport_w,
                viewport_h,
                0.0,
                0.0,
                range.pane_origin[0],
                range.pane_origin[1],
                range.pane_size[0],
                range.pane_size[1],
                range.pane_radii[0],
                range.pane_radii[1],
                range.pane_radii[2],
                range.pane_radii[3],
            ];
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            ctx.Map(
                &self.cbuffer,
                0,
                D3D11_MAP_WRITE_DISCARD,
                0,
                Some(&mut mapped),
            )
            .unwrap();
            std::ptr::copy_nonoverlapping(
                cb_data.as_ptr() as *const u8,
                mapped.pData as *mut u8,
                48,
            );
            ctx.Unmap(&self.cbuffer, 0);

            let offset = (start * std::mem::size_of::<Rect>()) as u32;
            ctx.IASetVertexBuffers(
                0,
                1,
                Some(&Some(self.instance_buffer.clone())),
                Some(&stride),
                Some(&offset),
            );
            ctx.DrawInstanced(4, count as u32, 0, 0);
        }
    }
}

// ─── D3D SDF rect Pipeline ──────────────────────────────────────────
//
// Drawn after flat overlay backgrounds and before overlay glyphs so
// rounded chrome sits on top of pane text while its labels stay crisp.
// Mirrors the blade SdfPipeline in crates/ciri-gpu/src/blade.rs.

struct DxSdfPipeline {
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    input_layout: ID3D11InputLayout,
    instance_buffer: ID3D11Buffer,
    cbuffer: ID3D11Buffer,
    max_rects: usize,
}

impl DxSdfPipeline {
    unsafe fn new(device: &ID3D11Device, max_rects: usize) -> Result<Self> {
        let vs_blob = compile_shader(SDF_HLSL, "vs_main", "vs_5_0")?;
        let vs_code = std::slice::from_raw_parts(
            vs_blob.GetBufferPointer() as *const u8,
            vs_blob.GetBufferSize(),
        );
        let mut vs = None;
        device.CreateVertexShader(vs_code, None, Some(&mut vs))?;
        let vs = vs.unwrap();

        let ps_blob = compile_shader(SDF_HLSL, "ps_main", "ps_5_0")?;
        let ps_code = std::slice::from_raw_parts(
            ps_blob.GetBufferPointer() as *const u8,
            ps_blob.GetBufferSize(),
        );
        let mut ps = None;
        device.CreatePixelShader(ps_code, None, Some(&mut ps))?;
        let ps = ps.unwrap();

        // Offsets mirror `SdfRect` exactly — the three-way Rust ⇄ vertex
        // layout ⇄ HLSL contract enforced by `sdf_rect.rs` tests.
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
                SemanticName: PCSTR::from_raw(b"COL\0".as_ptr()),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 16,
                InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
                InstanceDataStepRate: 1,
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR::from_raw(b"RADII\0".as_ptr()),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 32,
                InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
                InstanceDataStepRate: 1,
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR::from_raw(b"BCOL\0".as_ptr()),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 48,
                InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
                InstanceDataStepRate: 1,
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR::from_raw(b"BW\0".as_ptr()),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 64,
                InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
                InstanceDataStepRate: 1,
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR::from_raw(b"SBLUR\0".as_ptr()),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 68,
                InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
                InstanceDataStepRate: 1,
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR::from_raw(b"SOFF\0".as_ptr()),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 72,
                InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
                InstanceDataStepRate: 1,
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: PCSTR::from_raw(b"SCOL\0".as_ptr()),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 80,
                InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
                InstanceDataStepRate: 1,
            },
        ];
        let mut input_layout = None;
        device.CreateInputLayout(&layout_desc, vs_code, Some(&mut input_layout))?;
        let input_layout = input_layout.unwrap();

        let buf_desc = D3D11_BUFFER_DESC {
            ByteWidth: (max_rects * SdfRect::SIZE) as u32,
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

        Ok(DxSdfPipeline {
            vs,
            ps,
            input_layout,
            instance_buffer,
            cbuffer,
            max_rects,
        })
    }

    unsafe fn upload(
        &self,
        ctx: &ID3D11DeviceContext,
        rects: &[SdfRect],
        viewport_w: f32,
        viewport_h: f32,
    ) {
        if rects.is_empty() {
            return;
        }
        let count = rects.len().min(self.max_rects);

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

    unsafe fn draw(&self, ctx: &ID3D11DeviceContext, count: usize) {
        if count == 0 {
            return;
        }
        let count = count.min(self.max_rects);
        ctx.IASetInputLayout(Some(&self.input_layout));
        ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
        let stride = SdfRect::SIZE as u32;
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
    d2d_factory: ID2D1Factory,
    text_rendering_params: Option<IDWriteRenderingParams>,
    rects: DxRectPipeline,
    sdf: DxSdfPipeline,
    width: u32,
    height: u32,
    sync_interval: u32,
    /// Waitable object for DXGI frame latency — lets the CPU sleep instead of
    /// busy-waiting in `Present(1)`. `None` if the driver doesn't support it.
    frame_waitable: Option<HANDLE>,
    blending_flags: u32,
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
        let sdf = unsafe { DxSdfPipeline::new(&device, MAX_SDF_RECTS)? };

        let sync_interval = match render_config.present_mode {
            ciri_config::config::PresentMode::Immediate
            | ciri_config::config::PresentMode::Mailbox => 0,
            _ => 1,
        };

        let d2d_factory: ID2D1Factory =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };

        // Obtain system default text rendering params for D2D render targets.
        let dwrite_factory: IDWriteFactory =
            unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        let text_rendering_params = unsafe { dwrite_factory.CreateRenderingParams().ok() };

        log::info!(
            "D3D11 renderer initialized ({}x{})",
            size.width,
            size.height
        );

        let blending_flags = (render_config.alpha_blending.is_linear() as u32)
            | ((render_config.alpha_blending.use_correction() as u32) << 1);

        Ok(Renderer {
            device,
            ctx,
            swap_chain,
            rtv,
            rasterizer,
            blend,
            d2d_factory,
            text_rendering_params,
            rects,
            sdf,
            width: size.width.max(1),
            height: size.height.max(1),
            sync_interval,
            frame_waitable,
            blending_flags,
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
        // Inject color functions into the alpha pixel shader.
        let alpha_ps = ALPHA_PS_HLSL
            .replace("// HLSL_COLOR_FUNCS_PLACEHOLDER", HLSL_COLOR_FUNCS)
            .replace("// HLSL_CORNER_FUNCS_PLACEHOLDER", HLSL_CORNER_FUNCS);

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
                    ps_hlsl: &alpha_ps,
                    filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                    d2d_factory: &self.d2d_factory,
                    text_antialias: D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
                    text_rendering_params: self.text_rendering_params.clone(),
                    blending_flags: self.blending_flags,
                },
            )
            .expect("alpha atlas creation failed")
        };

        let color_ps = COLOR_PS_HLSL
            .replace("// HLSL_CORNER_FUNCS_PLACEHOLDER", HLSL_CORNER_FUNCS);

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
                    ps_hlsl: &color_ps,
                    filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                    d2d_factory: &self.d2d_factory,
                    text_antialias: D2D1_TEXT_ANTIALIAS_MODE_DEFAULT,
                    text_rendering_params: self.text_rendering_params.clone(),
                    blending_flags: self.blending_flags,
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
        let mut profiler = crate::DrawFrameProfiler::begin("dx");

        unsafe {
            let upload_start = std::time::Instant::now();
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
            let mut all_bg_ranges = Vec::with_capacity(1 + scene.bg_rect_ranges.len());
            all_bg_ranges.push(PaneRectRange {
                start: 0,
                count: 1,
                ..PaneRectRange::default()
            });
            all_bg_ranges.extend(scene.bg_rect_ranges.iter().map(|range| PaneRectRange {
                start: range.start.saturating_add(1),
                ..*range
            }));
            if scene.bg_rect_ranges.is_empty() && !scene.bg_rects.is_empty() {
                all_bg_ranges.push(PaneRectRange {
                    start: 1,
                    count: scene.bg_rects.len() as u32,
                    ..PaneRectRange::default()
                });
            }
            let split_ranges =
                |ranges: &[PaneRectRange], start: usize, end: usize| -> Vec<PaneRectRange> {
                    ranges
                        .iter()
                        .filter_map(|range| {
                            let range_start = range.start as usize;
                            let range_end = range_start.saturating_add(range.count as usize);
                            let clipped_start = range_start.max(start);
                            let clipped_end = range_end.min(end);
                            (clipped_start < clipped_end).then(|| PaneRectRange {
                                start: clipped_start as u32,
                                count: (clipped_end - clipped_start) as u32,
                                ..*range
                            })
                        })
                        .collect()
                };

            // 2. Draw non-focused pane background rects.
            let inactive_bg_count = active_bg_idx.min(total_bg);
            let inactive_bg_ranges = split_ranges(&all_bg_ranges, 0, inactive_bg_count);
            self.rects
                .draw_ranges(&self.ctx, &inactive_bg_ranges, vw, vh);

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
            if let Some(profiler) = profiler.as_mut() {
                profiler.record_cpu_upload(upload_start);
            }

            // 4. Draw inactive pane glyphs (scissored).
            let draw_start = std::time::Instant::now();
            atlas_gpu
                .alpha
                .draw_batches(&self.ctx, alpha_count, &vp, scene.glyph_batches);
            atlas_gpu
                .color
                .draw_batches(&self.ctx, color_count, &vp, scene.color_glyph_batches);

            // 5. Focused pane background rects.
            let active_bg_count = overlay_bg_idx.saturating_sub(active_bg_idx);
            if active_bg_count > 0 {
                self.ctx.RSSetScissorRects(Some(&[full_rect]));
                let active_bg_ranges =
                    split_ranges(&all_bg_ranges, active_bg_idx, overlay_bg_idx.min(total_bg));
                self.rects
                    .draw_ranges(&self.ctx, &active_bg_ranges, vw, vh);
            }

            // 6. Draw active pane glyphs (scissored, no re-upload).
            atlas_gpu
                .alpha
                .draw_batches(&self.ctx, alpha_count, &vp, scene.active_glyph_batches);
            atlas_gpu
                .color
                .draw_batches(&self.ctx, color_count, &vp, scene.active_color_glyph_batches);

            // 7. Overlay background rects.
            let overlay_bg_count = total_bg.saturating_sub(overlay_bg_idx);
            if overlay_bg_count > 0 {
                self.ctx.RSSetScissorRects(Some(&[full_rect]));
                let overlay_bg_ranges = split_ranges(&all_bg_ranges, overlay_bg_idx, total_bg);
                self.rects
                    .draw_ranges(&self.ctx, &overlay_bg_ranges, vw, vh);
            }

            // 7b. SDF chrome (rounded / shadow / border). Drawn after flat
            //     overlay bgs and before overlay glyphs so chrome labels
            //     paint crisply on top of their rounded panel.
            if !scene.sdf_rects.is_empty() {
                self.ctx.RSSetScissorRects(Some(&[full_rect]));
                self.sdf.upload(&self.ctx, scene.sdf_rects, vw, vh);
                // C4 verified: DxSdfPipeline consumes the same SdfRect fields
                // and draw order as GL (focus rings before cached/transient chrome).
                self.sdf.draw(&self.ctx, scene.sdf_rects.len());
            }

            // 8. Overlay glyphs (no re-upload, just draw remaining range).
            // Zero pane_size hits the helper short-circuit (no clipping)
            // — once C5 wires the uniforms into the DX path.
            let overlay_alpha = PaneGlyphRange {
                start: scene.pane_glyph_end as u32,
                count: (scene.glyphs.len() - scene.pane_glyph_end) as u32,
                scissor: (0, 0, self.width, self.height),
                ..PaneGlyphRange::default()
            };
            let overlay_color = PaneGlyphRange {
                start: scene.pane_color_glyph_end as u32,
                count: (scene.color_glyphs.len() - scene.pane_color_glyph_end) as u32,
                scissor: (0, 0, self.width, self.height),
                ..PaneGlyphRange::default()
            };
            atlas_gpu
                .alpha
                .draw_batches(&self.ctx, alpha_count, &vp, &[overlay_alpha]);
            atlas_gpu
                .color
                .draw_batches(&self.ctx, color_count, &vp, &[overlay_color]);
            if let Some(profiler) = profiler.as_mut() {
                profiler.record_draw(draw_start);
            }

            // Wait for the previous frame to finish presentation before
            // submitting the next one. With the waitable object this is a true
            // kernel wait (CPU sleeps), not a busy-wait spin loop.
            if let Some(handle) = self.frame_waitable {
                let wait_start = std::time::Instant::now();
                WaitForSingleObjectEx(handle, 1000, false);
                if let Some(profiler) = profiler.as_mut() {
                    profiler.record_sync_wait(wait_start);
                }
            }

            // Present — sync_interval=0 when using waitable object (latency
            // is controlled by SetMaximumFrameLatency instead).
            let interval = if self.frame_waitable.is_some() {
                0
            } else {
                self.sync_interval
            };
            let present_start = std::time::Instant::now();
            let _ = self.swap_chain.Present(interval, DXGI_PRESENT(0)).ok();
            if let Some(profiler) = profiler.as_mut() {
                profiler.record_present(present_start);
            }
        }

        if let Some(profiler) = profiler {
            profiler.finish(
                scene.bg_rects.len(),
                scene.glyphs.len(),
                scene.color_glyphs.len(),
            );
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
