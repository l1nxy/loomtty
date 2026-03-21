//! Blade GPU backend (Vulkan on Linux, Metal on macOS).
//!
//! Contains all blade-graphics specific GPU code:
//! - `Renderer`: surface management, frame rendering
//! - `GlyphAtlasGpu`: GPU-side atlas textures, pipelines, upload flushing
//! - `RectPipeline`: instanced colored rectangle rendering
//! - WGSL shaders for rects, alpha text, and color emoji

use anyhow::Result;
use blade_graphics as gpu;
use blade_graphics::ShaderData;
use ciri_config::config::RenderConfig;
use cosmic_text::FontSystem;
use std::ptr;
use std::sync::Arc;
use winit::window::Window;

use ciri_render::glyph_cache::{GlyphCache, GlyphInstance, PendingUpload, ScissoredRange};
use ciri_render::rect::Rect;
use ciri_render::FrameScene;

// ─── Rect pipeline ──────────────────────────────────────────────────

#[derive(blade_macros::ShaderData)]
struct RectData {
    uniforms: gpu::BufferPiece,
}

/// Renders colored rectangles using GPU instanced rendering.
/// Each rect is a single instance; the vertex shader generates 4 vertices
/// via TriangleStrip, performing pixel-to-NDC conversion on the GPU.
struct RectPipeline {
    pipeline: gpu::RenderPipeline,
    instance_buffer: gpu::Buffer,
    uniform_buffer: gpu::Buffer,
    max_rects: usize,
}

impl RectPipeline {
    fn new(context: &gpu::Context, format: gpu::TextureFormat, max_rects: usize) -> Self {
        let shader = context.create_shader(gpu::ShaderDesc {
            source: RECT_SHADER,
        });

        let uniform_buffer = context.create_buffer(gpu::BufferDesc {
            name: "rect_uniform",
            size: 16,
            memory: gpu::Memory::Shared,
        });

        let instance_buffer = context.create_buffer(gpu::BufferDesc {
            name: "rect_instance_buffer",
            size: (max_rects * std::mem::size_of::<Rect>()) as u64,
            memory: gpu::Memory::Shared,
        });

        let vertex_layout = gpu::VertexLayout {
            attributes: vec![
                ("pos", gpu::VertexAttribute {
                    offset: 0,
                    format: gpu::VertexFormat::F32Vec2,
                }),
                ("size", gpu::VertexAttribute {
                    offset: 8,
                    format: gpu::VertexFormat::F32Vec2,
                }),
                ("color", gpu::VertexAttribute {
                    offset: 16,
                    format: gpu::VertexFormat::F32Vec4,
                }),
            ],
            stride: std::mem::size_of::<Rect>() as u32,
        };

        let pipeline = context.create_render_pipeline(gpu::RenderPipelineDesc {
            name: "rect_pipeline",
            data_layouts: &[&RectData::layout()],
            vertex: shader.at("vs_main"),
            vertex_fetches: &[gpu::VertexFetchState {
                layout: &vertex_layout,
                instanced: true,
            }],
            primitive: gpu::PrimitiveState {
                topology: gpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            fragment: Some(shader.at("fs_main")),
            color_targets: &[gpu::ColorTargetState {
                format,
                blend: Some(gpu::BlendState::ALPHA_BLENDING),
                write_mask: gpu::ColorWrites::all(),
            }],
            multisample_state: gpu::MultisampleState::default(),
        });

        RectPipeline {
            pipeline,
            instance_buffer,
            uniform_buffer,
            max_rects,
        }
    }

    fn render(
        &self,
        pass: &mut gpu::RenderCommandEncoder,
        rects: &[Rect],
        viewport_w: f32,
        viewport_h: f32,
    ) {
        if rects.is_empty() {
            return;
        }

        let count = rects.len().min(self.max_rects);

        let viewport = [viewport_w, viewport_h, 0.0f32, 0.0f32];
        unsafe {
            ptr::copy_nonoverlapping(
                viewport.as_ptr() as *const u8,
                self.uniform_buffer.data(),
                16,
            );
        }

        let data = bytemuck::cast_slice(&rects[..count]);
        unsafe {
            ptr::copy_nonoverlapping(data.as_ptr(), self.instance_buffer.data(), data.len());
        }

        let mut pe = pass.with(&self.pipeline);
        pe.bind(
            0,
            &RectData {
                uniforms: self.uniform_buffer.at(0),
            },
        );
        pe.bind_vertex(0, self.instance_buffer.at(0));
        pe.draw(0, 4, 0, count as u32);
    }

    fn destroy(&mut self, context: &gpu::Context) {
        context.destroy_render_pipeline(&mut self.pipeline);
        context.destroy_buffer(self.instance_buffer);
        context.destroy_buffer(self.uniform_buffer);
    }
}

// ─── Atlas layer (GPU) ──────────────────────────────────────────────

#[derive(blade_macros::ShaderData)]
struct GlyphShaderData {
    atlas_tex: gpu::TextureView,
    atlas_sampler: gpu::Sampler,
    viewport: gpu::BufferPiece,
}

/// A single GPU texture atlas layer with its own pipeline.
struct AtlasLayer {
    texture: gpu::Texture,
    texture_view: gpu::TextureView,
    sampler: gpu::Sampler,
    pipeline: gpu::RenderPipeline,
    instance_buffer: gpu::Buffer,
    uniform_buffer: gpu::Buffer,
    staging_buffer: gpu::Buffer,
    /// Bytes per pixel (1 for R8Unorm, 4 for Rgba8UnormSrgb).
    bpp: u32,
    atlas_size: u32,
}

impl AtlasLayer {
    fn new(
        context: &gpu::Context,
        surface_format: gpu::TextureFormat,
        atlas_size: u32,
        max_instances: usize,
        tex_format: gpu::TextureFormat,
        filter: gpu::FilterMode,
        shader_source: &str,
        blend: gpu::BlendState,
        label: &str,
    ) -> Self {
        let bpp = match tex_format {
            gpu::TextureFormat::R8Unorm => 1,
            _ => 4,
        };

        let texture = context.create_texture(gpu::TextureDesc {
            name: label,
            format: tex_format,
            size: gpu::Extent {
                width: atlas_size,
                height: atlas_size,
                depth: 1,
            },
            array_layer_count: 1,
            mip_level_count: 1,
            sample_count: 1,
            dimension: gpu::TextureDimension::D2,
            usage: gpu::TextureUsage::RESOURCE | gpu::TextureUsage::COPY,
            external: None,
        });
        let texture_view = context.create_texture_view(
            texture,
            gpu::TextureViewDesc {
                name: label,
                format: tex_format,
                dimension: gpu::ViewDimension::D2,
                subresources: &gpu::TextureSubresources::default(),
            },
        );
        let sampler = context.create_sampler(gpu::SamplerDesc {
            name: label,
            address_modes: [gpu::AddressMode::ClampToEdge; 3],
            mag_filter: filter,
            min_filter: filter,
            mipmap_filter: gpu::FilterMode::Nearest,
            ..Default::default()
        });

        let uniform_buffer = context.create_buffer(gpu::BufferDesc {
            name: "glyph_viewport_uniform",
            size: 16,
            memory: gpu::Memory::Shared,
        });

        let buf_size = (max_instances * std::mem::size_of::<GlyphInstance>()) as u64;
        let instance_buffer = context.create_buffer(gpu::BufferDesc {
            name: "glyph_instance_buffer",
            size: buf_size,
            memory: gpu::Memory::Shared,
        });

        let staging_size = (atlas_size * atlas_size * bpp) as u64;
        let staging_buffer = context.create_buffer(gpu::BufferDesc {
            name: "glyph_staging",
            size: staging_size,
            memory: gpu::Memory::Upload,
        });

        let shader = context.create_shader(gpu::ShaderDesc {
            source: shader_source,
        });

        let vertex_layout = glyph_vertex_layout();

        let pipeline = context.create_render_pipeline(gpu::RenderPipelineDesc {
            name: label,
            data_layouts: &[&GlyphShaderData::layout()],
            vertex: shader.at("vs_main"),
            vertex_fetches: &[gpu::VertexFetchState {
                layout: &vertex_layout,
                instanced: true,
            }],
            primitive: gpu::PrimitiveState {
                topology: gpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            fragment: Some(shader.at("fs_main")),
            color_targets: &[gpu::ColorTargetState {
                format: surface_format,
                blend: Some(blend),
                write_mask: gpu::ColorWrites::all(),
            }],
            multisample_state: gpu::MultisampleState::default(),
        });

        AtlasLayer {
            texture,
            texture_view,
            sampler,
            pipeline,
            instance_buffer,
            uniform_buffer,
            staging_buffer,
            bpp,
            atlas_size,
        }
    }

    /// Flush pending uploads to GPU via staging buffer + transfer commands.
    fn flush_uploads(
        &self,
        context: &gpu::Context,
        encoder: &mut gpu::CommandEncoder,
        pending_uploads: &mut Vec<PendingUpload>,
        pending_clear: bool,
    ) {
        if pending_clear {
            let zeros_size = (self.atlas_size * self.atlas_size * self.bpp) as usize;
            unsafe {
                ptr::write_bytes(self.staging_buffer.data(), 0, zeros_size);
            }
            context.sync_buffer(self.staging_buffer);
            {
                let mut transfer = encoder.transfer("glyph_clear");
                transfer.copy_buffer_to_texture(
                    self.staging_buffer.at(0),
                    self.atlas_size * self.bpp,
                    gpu::TexturePiece {
                        texture: self.texture,
                        mip_level: 0,
                        array_layer: 0,
                        origin: [0, 0, 0],
                    },
                    gpu::Extent {
                        width: self.atlas_size,
                        height: self.atlas_size,
                        depth: 1,
                    },
                );
            }
        }

        if pending_uploads.is_empty() {
            return;
        }

        let staging_capacity = (self.atlas_size * self.atlas_size * self.bpp) as u64;
        let mut cursor: u64 = 0;

        for upload in pending_uploads.iter() {
            let row_bytes = upload.w * self.bpp;
            let total_bytes = (row_bytes * upload.h) as u64;

            if cursor + total_bytes > staging_capacity {
                log::warn!("staging buffer overflow, skipping glyph upload");
                break;
            }

            unsafe {
                ptr::copy_nonoverlapping(
                    upload.data.as_ptr(),
                    self.staging_buffer.data().add(cursor as usize),
                    total_bytes as usize,
                );
            }
            cursor += total_bytes;
        }

        context.sync_buffer(self.staging_buffer);

        let mut offset: u64 = 0;
        for upload in pending_uploads.drain(..) {
            let row_bytes = upload.w * self.bpp;
            let total_bytes = (row_bytes * upload.h) as u64;

            if offset + total_bytes > staging_capacity {
                break;
            }

            {
                let mut transfer = encoder.transfer("glyph_upload");
                transfer.copy_buffer_to_texture(
                    self.staging_buffer.at(offset),
                    row_bytes,
                    gpu::TexturePiece {
                        texture: self.texture,
                        mip_level: 0,
                        array_layer: 0,
                        origin: [upload.x, upload.y, 0],
                    },
                    gpu::Extent {
                        width: upload.w,
                        height: upload.h,
                        depth: 1,
                    },
                );
            }
            offset += total_bytes;
        }
    }

    fn render_scissored(
        &self,
        pass: &mut gpu::RenderCommandEncoder,
        instances: &[GlyphInstance],
        max_instances: usize,
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
        let count = instances.len().min(max_instances);
        let viewport = [viewport_w, viewport_h, 0.0f32, 0.0f32];
        unsafe {
            ptr::copy_nonoverlapping(
                viewport.as_ptr() as *const u8,
                self.uniform_buffer.data(),
                16,
            );
        }
        let data = bytemuck::cast_slice(&instances[..count]);
        unsafe {
            ptr::copy_nonoverlapping(data.as_ptr(), self.instance_buffer.data(), data.len());
        }

        for batch in batches {
            let start = batch.start.min(count);
            let end = batch.end.min(count);
            if start >= end || batch.w == 0 || batch.h == 0 {
                continue;
            }
            let mut pe = pass.with(&self.pipeline);
            pe.bind(
                0,
                &GlyphShaderData {
                    atlas_tex: self.texture_view,
                    atlas_sampler: self.sampler,
                    viewport: self.uniform_buffer.at(0),
                },
            );
            pe.bind_vertex(0, self.instance_buffer.at(0));
            pe.set_scissor_rect(&gpu::ScissorRect {
                x: batch.x as i32,
                y: batch.y as i32,
                w: batch.w,
                h: batch.h,
            });
            pe.draw(0, 4, start as u32, (end - start) as u32);
        }

        let overlay_start = overlay_start.min(count);
        if overlay_start < count {
            let mut pe = pass.with(&self.pipeline);
            pe.bind(
                0,
                &GlyphShaderData {
                    atlas_tex: self.texture_view,
                    atlas_sampler: self.sampler,
                    viewport: self.uniform_buffer.at(0),
                },
            );
            pe.bind_vertex(0, self.instance_buffer.at(0));
            pe.set_scissor_rect(&gpu::ScissorRect {
                x: 0,
                y: 0,
                w: viewport_w_px.max(1),
                h: viewport_h_px.max(1),
            });
            pe.draw(0, 4, overlay_start as u32, (count - overlay_start) as u32);
        }
    }

    fn destroy(&mut self, context: &gpu::Context) {
        context.destroy_render_pipeline(&mut self.pipeline);
        context.destroy_buffer(self.instance_buffer);
        context.destroy_buffer(self.uniform_buffer);
        context.destroy_buffer(self.staging_buffer);
        context.destroy_texture_view(self.texture_view);
        context.destroy_sampler(self.sampler);
        context.destroy_texture(self.texture);
    }
}

// ─── GlyphAtlasGpu ─────────────────────────────────────────────────

/// GPU-side glyph atlas: two texture layers (alpha text + color emoji)
/// with their pipelines and buffers. Paired with a `GlyphCache` for CPU data.
pub struct GlyphAtlasGpu {
    alpha: AtlasLayer,
    color: AtlasLayer,
    max_instances: usize,
}

impl GlyphAtlasGpu {
    /// Create GPU atlas layers. Called by `Renderer::create_atlas`.
    fn new(
        context: &gpu::Context,
        surface_format: gpu::TextureFormat,
        atlas_size: u32,
        max_instances: usize,
    ) -> Self {
        let alpha_shader_src = format!("{VERTEX_SHADER}\n{ALPHA_FRAGMENT}");
        let alpha = AtlasLayer::new(
            context,
            surface_format,
            atlas_size,
            max_instances,
            gpu::TextureFormat::R8Unorm,
            gpu::FilterMode::Nearest,
            &alpha_shader_src,
            gpu::BlendState {
                color: gpu::BlendComponent {
                    src_factor: gpu::BlendFactor::SrcAlpha,
                    dst_factor: gpu::BlendFactor::OneMinusSrcAlpha,
                    operation: gpu::BlendOperation::Add,
                },
                alpha: gpu::BlendComponent::OVER,
            },
            "glyph_atlas",
        );

        let color_shader_src = format!("{VERTEX_SHADER}\n{COLOR_FRAGMENT}");
        let color = AtlasLayer::new(
            context,
            surface_format,
            atlas_size,
            max_instances,
            gpu::TextureFormat::Rgba8UnormSrgb,
            gpu::FilterMode::Linear,
            &color_shader_src,
            gpu::BlendState::ALPHA_BLENDING,
            "color_emoji_atlas",
        );

        GlyphAtlasGpu {
            alpha,
            color,
            max_instances,
        }
    }

    /// Flush pending glyph uploads from the cache to GPU.
    pub fn flush_uploads(
        &self,
        context: &gpu::Context,
        encoder: &mut gpu::CommandEncoder,
        cache: &mut GlyphCache,
    ) {
        let (mut alpha_pending, mut color_pending, alpha_clear, color_clear) =
            cache.take_pending();
        self.alpha
            .flush_uploads(context, encoder, &mut alpha_pending, alpha_clear);
        self.color
            .flush_uploads(context, encoder, &mut color_pending, color_clear);
    }

    /// Initialize atlas textures on GPU. Must be called once before first render.
    pub fn init_textures(&self, encoder: &mut gpu::CommandEncoder) {
        encoder.init_texture(self.alpha.texture);
        encoder.init_texture(self.color.texture);
    }

    /// Render alpha text glyphs with scissor rects.
    pub fn render_scissored(
        &self,
        pass: &mut gpu::RenderCommandEncoder,
        instances: &[GlyphInstance],
        viewport_w: f32,
        viewport_h: f32,
        viewport_w_px: u32,
        viewport_h_px: u32,
        batches: &[ScissoredRange],
        overlay_start: usize,
    ) {
        self.alpha.render_scissored(
            pass,
            instances,
            self.max_instances,
            viewport_w,
            viewport_h,
            viewport_w_px,
            viewport_h_px,
            batches,
            overlay_start,
        );
    }

    /// Render color emoji with scissor rects.
    pub fn render_color_scissored(
        &self,
        pass: &mut gpu::RenderCommandEncoder,
        instances: &[GlyphInstance],
        viewport_w: f32,
        viewport_h: f32,
        viewport_w_px: u32,
        viewport_h_px: u32,
        batches: &[ScissoredRange],
        overlay_start: usize,
    ) {
        self.color.render_scissored(
            pass,
            instances,
            self.max_instances,
            viewport_w,
            viewport_h,
            viewport_w_px,
            viewport_h_px,
            batches,
            overlay_start,
        );
    }

    fn destroy(&mut self, context: &gpu::Context) {
        self.alpha.destroy(context);
        self.color.destroy(context);
    }
}

// ─── Renderer ───────────────────────────────────────────────────────

pub struct Renderer {
    context: gpu::Context,
    surface: gpu::Surface,
    encoder: gpu::CommandEncoder,
    rects: RectPipeline,
    surface_config: gpu::SurfaceConfig,
    surface_format: gpu::TextureFormat,
    surface_dirty: bool,
    /// Font system for glyph discovery and rasterization (CPU only).
    pub font_system: FontSystem,
}

impl Renderer {
    pub fn new(window: Arc<Window>, render_config: &RenderConfig) -> Result<Self> {
        let size = window.inner_size();

        let context = unsafe {
            gpu::Context::init(gpu::ContextDesc {
                presentation: true,
                validation: cfg!(debug_assertions),
                timing: false,
                capture: false,
                overlay: false,
                device_id: 0,
            })
            .map_err(|e| anyhow::anyhow!("GPU init failed: {e:?}"))?
        };

        let info = context.device_information();
        log::info!(
            "GPU adapter: {} (driver: {})",
            info.device_name,
            info.driver_name
        );

        let display_sync = match render_config.present_mode.as_str() {
            "mailbox" => gpu::DisplaySync::Recent,
            "immediate" => gpu::DisplaySync::Tear,
            _ => gpu::DisplaySync::Block,
        };

        let surface_config = gpu::SurfaceConfig {
            size: gpu::Extent {
                width: size.width.max(1),
                height: size.height.max(1),
                depth: 1,
            },
            usage: gpu::TextureUsage::TARGET,
            display_sync,
            color_space: gpu::ColorSpace::Linear,
            transparent: false,
            allow_exclusive_full_screen: false,
        };

        let surface = context
            .create_surface_configured(&*window, surface_config)
            .map_err(|e| anyhow::anyhow!("Surface creation failed: {e:?}"))?;

        let surface_format = surface.info().format;
        log::info!("Surface format: {:?}", surface_format);

        let encoder = context.create_command_encoder(gpu::CommandEncoderDesc {
            name: "ciri",
            buffer_count: 2,
        });

        let rects = RectPipeline::new(&context, surface_format, render_config.max_rectangles);

        Ok(Renderer {
            context,
            surface,
            encoder,
            rects,
            surface_config,
            surface_format,
            surface_dirty: false,
            font_system: FontSystem::new(),
        })
    }

    // ─── Surface management ──────────────────────────────────────────

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.surface_config.size.width = width;
            self.surface_config.size.height = height;
            self.surface_dirty = true;
        }
    }

    pub fn apply_surface(&mut self) {
        if self.surface_dirty {
            self.context
                .reconfigure_surface(&mut self.surface, self.surface_config);
            self.surface_dirty = false;
        }
    }

    pub fn surface_size(&self) -> (u32, u32) {
        (
            self.surface_config.size.width,
            self.surface_config.size.height,
        )
    }

    pub fn surface_format(&self) -> gpu::TextureFormat {
        self.surface_format
    }

    // ─── Atlas init ──────────────────────────────────────────────────

    /// Create a new GlyphCache + GlyphAtlasGpu bound to this renderer's GPU context.
    /// Returns `(cache, atlas_gpu, primary_font_id)`.
    pub fn create_atlas(
        &mut self,
        font_size_pt: f32,
        dpi_scale: f64,
        family_name: &str,
        render_config: &RenderConfig,
    ) -> (GlyphCache, GlyphAtlasGpu, Option<cosmic_text::fontdb::ID>) {
        let (cache, primary_font_id) = GlyphCache::new(
            &mut self.font_system,
            font_size_pt,
            dpi_scale,
            family_name,
            render_config,
        );

        let atlas_gpu = GlyphAtlasGpu::new(
            &self.context,
            self.surface_format,
            cache.atlas_size,
            cache.max_instances,
        );

        // Initialize atlas textures on GPU
        self.encoder.start();
        atlas_gpu.init_textures(&mut self.encoder);
        self.context.submit(&mut self.encoder);

        (cache, atlas_gpu, primary_font_id)
    }

    /// Destroy a GlyphAtlasGpu's GPU resources.
    pub fn destroy_atlas(&self, atlas_gpu: &mut GlyphAtlasGpu) {
        atlas_gpu.destroy(&self.context);
    }

    // ─── Frame rendering ─────────────────────────────────────────────

    pub fn draw_frame(
        &mut self,
        atlas_gpu: &mut GlyphAtlasGpu,
        cache: &mut GlyphCache,
        scene: FrameScene,
    ) {
        let (vw, vh) = self.surface_size();
        let vw_f = vw as f32;
        let vh_f = vh as f32;

        self.encoder.start();

        // Flush deferred glyph uploads
        atlas_gpu.flush_uploads(&self.context, &mut self.encoder, cache);

        let frame = self.surface.acquire_frame();

        {
            let mut pass = self.encoder.render(
                "ciri",
                gpu::RenderTargetSet {
                    colors: &[gpu::RenderTarget {
                        view: frame.texture_view(),
                        init_op: gpu::InitOp::Clear(gpu::TextureColor::OpaqueBlack),
                        finish_op: gpu::FinishOp::Store,
                    }],
                    depth_stencil: None,
                },
            );

            // 0. Full-screen clear rect (custom background color)
            let clear_rect = [Rect {
                x: 0.0,
                y: 0.0,
                w: vw_f,
                h: vh_f,
                color: scene.clear_color,
            }];
            self.rects.render(&mut pass, &clear_rect, vw_f, vh_f);
            // 1. Background rects
            self.rects.render(&mut pass, scene.bg_rects, vw_f, vh_f);
            // 2. Alpha text glyphs
            atlas_gpu.render_scissored(
                &mut pass,
                scene.glyphs,
                vw_f,
                vh_f,
                vw,
                vh,
                scene.glyph_batches,
                scene.pane_glyph_end,
            );
            // 3. Color emoji
            atlas_gpu.render_color_scissored(
                &mut pass,
                scene.color_glyphs,
                vw_f,
                vh_f,
                vw,
                vh,
                scene.color_glyph_batches,
                scene.pane_color_glyph_end,
            );
        }

        self.encoder.present(frame);
        self.context.submit(&mut self.encoder);
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        self.rects.destroy(&self.context);
        self.context.destroy_command_encoder(&mut self.encoder);
        self.context.destroy_surface(&mut self.surface);
    }
}

// ─── Vertex layout ───────────────────────────────────────────────────

fn glyph_vertex_layout() -> gpu::VertexLayout {
    gpu::VertexLayout {
        attributes: vec![
            ("pos", gpu::VertexAttribute {
                offset: 0,
                format: gpu::VertexFormat::F32Vec2,
            }),
            ("size", gpu::VertexAttribute {
                offset: 8,
                format: gpu::VertexFormat::F32Vec2,
            }),
            ("uv_pos", gpu::VertexAttribute {
                offset: 16,
                format: gpu::VertexFormat::F32Vec2,
            }),
            ("uv_size", gpu::VertexAttribute {
                offset: 24,
                format: gpu::VertexFormat::F32Vec2,
            }),
            ("color", gpu::VertexAttribute {
                offset: 32,
                format: gpu::VertexFormat::F32Vec4,
            }),
        ],
        stride: std::mem::size_of::<GlyphInstance>() as u32,
    }
}

// ─── WGSL shaders ────────────────────────────────────────────────────

const RECT_SHADER: &str = r#"
struct Uniforms {
    viewport_size: vec4<f32>,
};

var<uniform> uniforms: Uniforms;

struct RectInstance {
    pos: vec2<f32>,
    size: vec2<f32>,
    color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: RectInstance) -> VertexOutput {
    let x = f32(vi & 1u);
    let y = f32((vi >> 1u) & 1u);

    let px = inst.pos + vec2<f32>(x, y) * inst.size;
    let ndc = vec2<f32>(
        px.x / uniforms.viewport_size.x * 2.0 - 1.0,
        1.0 - px.y / uniforms.viewport_size.y * 2.0
    );

    var out: VertexOutput;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.color = inst.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(
        srgb_to_linear(in.color.r),
        srgb_to_linear(in.color.g),
        srgb_to_linear(in.color.b),
        in.color.a
    );
}
"#;

const VERTEX_SHADER: &str = r#"
struct Viewport {
    size: vec4<f32>,
};

var<uniform> viewport: Viewport;

struct Instance {
    pos: vec2<f32>,
    size: vec2<f32>,
    uv_pos: vec2<f32>,
    uv_size: vec2<f32>,
    color: vec4<f32>,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: Instance) -> VsOut {
    let x = f32(vi & 1u);
    let y = f32((vi >> 1u) & 1u);

    var out: VsOut;
    out.uv = inst.uv_pos + vec2<f32>(x, y) * inst.uv_size;
    out.color = inst.color;
    let px = inst.pos + vec2<f32>(x, y) * inst.size;
    let ndc = vec2<f32>(
        px.x / viewport.size.x * 2.0 - 1.0,
        1.0 - px.y / viewport.size.y * 2.0
    );
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    return out;
}
"#;

const ALPHA_FRAGMENT: &str = r#"
var atlas_tex: texture_2d<f32>;
var atlas_sampler: sampler;

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let alpha = textureSample(atlas_tex, atlas_sampler, in.uv).r;
    let r = srgb_to_linear(in.color.r);
    let g = srgb_to_linear(in.color.g);
    let b = srgb_to_linear(in.color.b);
    return vec4<f32>(r, g, b, in.color.a * alpha);
}
"#;

const COLOR_FRAGMENT: &str = r#"
var atlas_tex: texture_2d<f32>;
var atlas_sampler: sampler;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas_tex, atlas_sampler, in.uv);
    return vec4<f32>(texel.rgb * in.color.rgb, texel.a * in.color.a);
}
"#;
