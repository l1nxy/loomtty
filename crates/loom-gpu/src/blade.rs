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
use loom_config::config::RenderConfig;
use std::ptr;
use std::sync::Arc;
use winit::window::Window;

use loom_render::FrameScene;
use loom_render::glyph_cache::{GlyphCache, GlyphInstance, PaneGlyphRange, PendingUpload};
use loom_render::rect::{PaneRectRange, Rect};
use loom_render::sdf_rect::SdfRect;

/// Upper bound on SDF chrome rects per frame. Chrome typically has
/// ≤ 20 — 256 gives headroom for plugin UIs and modal stacks. If this is
/// hit the tail is dropped; matches the existing `RectPipeline` behaviour.
/// Per-frame SDF chrome rect capacity. Sized for the layered chrome
/// path: pane focus rings + base chrome (top_bar / hints / settings /
/// dialogs) + overlay chrome (palette / context_menu) + transient
/// overlays. 1024 is roughly 4× the worst observed real workload — if
/// `base_sdf_end` ever exceeded `MAX_SDF_RECTS`, the upload would
/// truncate, and `draw_range`'s `start >= max` guard would drop the
/// entire Overlay pass silently. The `upload` helpers `log::warn` on
/// truncation so a hit is visible in logs.
const MAX_SDF_RECTS: usize = 1024;
/// Upper bound on distinct `PaneRectRange` uniform slots per frame. One slot
/// per draw range, not per rect — # of ranges is bounded by # of panes plus a
/// small constant for chrome/overlay layers. Decoupled from `max_rects` so
/// growing the instance buffer doesn't waste uniform memory.
const MAX_RECT_PANE_RANGES: usize = 256;
const RECT_UNIFORM_RECORD_SIZE: u64 = 48;
const GLYPH_UNIFORM_RECORD_SIZE: u64 = 64;
const BLADE_UNIFORM_STRIDE: u64 = 256;

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
        let rect_shader_src =
            RECT_SHADER.replace("// WGSL_CORNER_FUNCS_PLACEHOLDER", WGSL_CORNER_FUNCS);
        let shader = context.create_shader(gpu::ShaderDesc {
            source: &rect_shader_src,
        });

        let uniform_buffer = context.create_buffer(gpu::BufferDesc {
            name: "rect_uniform",
            size: BLADE_UNIFORM_STRIDE * MAX_RECT_PANE_RANGES as u64,
            memory: gpu::Memory::Shared,
        });

        let instance_buffer = context.create_buffer(gpu::BufferDesc {
            name: "rect_instance_buffer",
            size: (max_rects.max(1) * std::mem::size_of::<Rect>()) as u64,
            memory: gpu::Memory::Shared,
        });

        let vertex_layout = gpu::VertexLayout {
            attributes: vec![
                (
                    "pos",
                    gpu::VertexAttribute {
                        offset: 0,
                        format: gpu::VertexFormat::F32Vec2,
                    },
                ),
                (
                    "size",
                    gpu::VertexAttribute {
                        offset: 8,
                        format: gpu::VertexFormat::F32Vec2,
                    },
                ),
                (
                    "color",
                    gpu::VertexAttribute {
                        offset: 16,
                        format: gpu::VertexFormat::F32Vec4,
                    },
                ),
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
                blend: Some(gpu::BlendState {
                    color: gpu::BlendComponent {
                        src_factor: gpu::BlendFactor::One,
                        dst_factor: gpu::BlendFactor::OneMinusSrcAlpha,
                        operation: gpu::BlendOperation::Add,
                    },
                    alpha: gpu::BlendComponent::OVER,
                }),
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

    /// Grow the instance buffer if `needed` exceeds current capacity. Must be
    /// called after the previous frame's GPU work has completed (the
    /// `wait_for(last_sync)` at the top of `draw_frame`) — otherwise the GPU
    /// may still be reading from the buffer we're about to destroy.
    fn ensure_capacity(&mut self, context: &gpu::Context, needed: usize) {
        if needed <= self.max_rects {
            return;
        }
        let new_cap = needed
            .next_power_of_two()
            .max(self.max_rects.saturating_mul(2));
        let new_buffer = context.create_buffer(gpu::BufferDesc {
            name: "rect_instance_buffer",
            size: (new_cap * std::mem::size_of::<Rect>()) as u64,
            memory: gpu::Memory::Shared,
        });
        let old = std::mem::replace(&mut self.instance_buffer, new_buffer);
        context.destroy_buffer(old);
        self.max_rects = new_cap;
    }

    /// Upload all rect instance data to the GPU buffer.
    fn upload(&self, rects: &[Rect], viewport_w: f32, viewport_h: f32) {
        if rects.is_empty() {
            return;
        }
        let count = rects.len().min(self.max_rects);

        let _ = (viewport_w, viewport_h);

        let data = bytemuck::cast_slice(&rects[..count]);
        unsafe {
            ptr::copy_nonoverlapping(data.as_ptr(), self.instance_buffer.data(), data.len());
        }
    }

    fn write_uniform(
        &self,
        slot: usize,
        viewport_w: f32,
        viewport_h: f32,
        pane_origin: [f32; 2],
        pane_size: [f32; 2],
        pane_radii: [f32; 4],
    ) -> u64 {
        let offset = slot as u64 * BLADE_UNIFORM_STRIDE;
        debug_assert!(
            offset + RECT_UNIFORM_RECORD_SIZE <= BLADE_UNIFORM_STRIDE * MAX_RECT_PANE_RANGES as u64
        );
        let viewport = [viewport_w, viewport_h, 0.0f32, 0.0f32];
        unsafe {
            let dst = self.uniform_buffer.data().add(offset as usize);
            ptr::copy_nonoverlapping(viewport.as_ptr() as *const u8, dst, 16);
            ptr::copy_nonoverlapping(pane_origin.as_ptr() as *const u8, dst.add(16), 8);
            ptr::copy_nonoverlapping(pane_size.as_ptr() as *const u8, dst.add(24), 8);
            ptr::copy_nonoverlapping(pane_radii.as_ptr() as *const u8, dst.add(32), 16);
        }
        offset
    }

    /// Draw previously uploaded rects grouped by pane clipping uniforms.
    fn draw_ranges(
        &self,
        pass: &mut gpu::RenderCommandEncoder,
        ranges: &[PaneRectRange],
        viewport_w: f32,
        viewport_h: f32,
        uniform_slot: &mut usize,
    ) {
        if ranges.is_empty() {
            return;
        }
        for range in ranges {
            let start = range.start as usize;
            let count = range.count as usize;
            if count == 0 || start >= self.max_rects || *uniform_slot >= MAX_RECT_PANE_RANGES {
                continue;
            }
            let count = count.min(self.max_rects - start);
            let uniform_offset = self.write_uniform(
                *uniform_slot,
                viewport_w,
                viewport_h,
                range.pane_origin,
                range.pane_size,
                range.pane_radii,
            );
            *uniform_slot += 1;

            let mut pe = pass.with(&self.pipeline);
            pe.bind(
                0,
                &RectData {
                    uniforms: self.uniform_buffer.at(uniform_offset),
                },
            );
            pe.bind_vertex(0, self.instance_buffer.at(0));
            pe.draw(0, 4, start as u32, count as u32);
        }
    }

    fn destroy(&mut self, context: &gpu::Context) {
        context.destroy_render_pipeline(&mut self.pipeline);
        context.destroy_buffer(self.instance_buffer);
        context.destroy_buffer(self.uniform_buffer);
    }
}

// ─── SDF rect pipeline ──────────────────────────────────────────────
//
// Rendered after flat overlay backgrounds so rounded/shadowed chrome sits
// on top of pane text, but before overlay glyphs so chrome labels stay
// crisp on their rounded panel.

#[derive(blade_macros::ShaderData)]
struct SdfData {
    uniforms: gpu::BufferPiece,
}

/// SDF-shader instanced rect pipeline (rounded corners + border + shadow).
struct SdfPipeline {
    pipeline: gpu::RenderPipeline,
    instance_buffer: gpu::Buffer,
    uniform_buffer: gpu::Buffer,
    max_rects: usize,
}

impl SdfPipeline {
    fn new(context: &gpu::Context, format: gpu::TextureFormat, max_rects: usize) -> Self {
        let shader = context.create_shader(gpu::ShaderDesc { source: SDF_SHADER });

        let uniform_buffer = context.create_buffer(gpu::BufferDesc {
            name: "sdf_uniform",
            size: 16,
            memory: gpu::Memory::Shared,
        });

        let instance_buffer = context.create_buffer(gpu::BufferDesc {
            name: "sdf_instance_buffer",
            size: (max_rects * SdfRect::SIZE) as u64,
            memory: gpu::Memory::Shared,
        });

        // Must mirror the field order of `SdfRect` exactly; the shader
        // struct and these offsets are a three-way contract.
        let vertex_layout = gpu::VertexLayout {
            attributes: vec![
                (
                    "pos",
                    gpu::VertexAttribute {
                        offset: 0,
                        format: gpu::VertexFormat::F32Vec2,
                    },
                ),
                (
                    "size",
                    gpu::VertexAttribute {
                        offset: 8,
                        format: gpu::VertexFormat::F32Vec2,
                    },
                ),
                (
                    "color",
                    gpu::VertexAttribute {
                        offset: 16,
                        format: gpu::VertexFormat::F32Vec4,
                    },
                ),
                (
                    "radii",
                    gpu::VertexAttribute {
                        offset: 32,
                        format: gpu::VertexFormat::F32Vec4,
                    },
                ),
                (
                    "border_color",
                    gpu::VertexAttribute {
                        offset: 48,
                        format: gpu::VertexFormat::F32Vec4,
                    },
                ),
                (
                    "border_width",
                    gpu::VertexAttribute {
                        offset: 64,
                        format: gpu::VertexFormat::F32,
                    },
                ),
                (
                    "shadow_blur",
                    gpu::VertexAttribute {
                        offset: 68,
                        format: gpu::VertexFormat::F32,
                    },
                ),
                (
                    "shadow_offset",
                    gpu::VertexAttribute {
                        offset: 72,
                        format: gpu::VertexFormat::F32Vec2,
                    },
                ),
                (
                    "shadow_color",
                    gpu::VertexAttribute {
                        offset: 80,
                        format: gpu::VertexFormat::F32Vec4,
                    },
                ),
            ],
            stride: SdfRect::SIZE as u32,
        };

        let pipeline = context.create_render_pipeline(gpu::RenderPipelineDesc {
            name: "sdf_pipeline",
            data_layouts: &[&SdfData::layout()],
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
                blend: Some(gpu::BlendState {
                    color: gpu::BlendComponent {
                        src_factor: gpu::BlendFactor::One,
                        dst_factor: gpu::BlendFactor::OneMinusSrcAlpha,
                        operation: gpu::BlendOperation::Add,
                    },
                    alpha: gpu::BlendComponent::OVER,
                }),
                write_mask: gpu::ColorWrites::all(),
            }],
            multisample_state: gpu::MultisampleState::default(),
        });

        SdfPipeline {
            pipeline,
            instance_buffer,
            uniform_buffer,
            max_rects,
        }
    }

    fn upload(&self, rects: &[SdfRect], viewport_w: f32, viewport_h: f32) {
        if rects.is_empty() {
            return;
        }
        if rects.len() > self.max_rects {
            // Truncation hides Overlay chrome silently because layered
            // `draw_range` short-circuits when `start >= max_rects`.
            // Bump `MAX_SDF_RECTS` if this fires in real use.
            log::warn!(
                "SDF chrome overflow: {} rects > {} cap; tail (incl. Overlay) dropped",
                rects.len(),
                self.max_rects,
            );
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
    }

    /// Draw a contiguous slice `[start..start + count)` of the uploaded
    /// instance buffer. Used by the layered chrome pass: Base SDF runs
    /// `[pane_sdf..base_end]`, then a glyph batch covers Base text,
    /// then `[base_end..]` runs Overlay + transient SDF on top, and a
    /// final glyph batch lays Overlay + transient text. Without this
    /// split the merged stream's "all rects then all glyphs" ordering
    /// lets Base glyphs bleed through Overlay backgrounds.
    fn draw_range(&self, pass: &mut gpu::RenderCommandEncoder, start: usize, count: usize) {
        if count == 0 {
            return;
        }
        // Clamp end against the uploaded buffer's capacity. `upload`
        // already truncated to `max_rects`, so any tail past that has
        // no valid instance data.
        let max = self.max_rects;
        if start >= max {
            return;
        }
        let count = count.min(max - start);
        let mut pe = pass.with(&self.pipeline);
        pe.bind(
            0,
            &SdfData {
                uniforms: self.uniform_buffer.at(0),
            },
        );
        pe.bind_vertex(0, self.instance_buffer.at(0));
        pe.draw(0, 4, start as u32, count as u32);
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
    /// Packed blending flags: bit 0 = use_linear_blending, bit 1 = use_linear_correction.
    blending_flags: u32,
}

struct AtlasLayerConfig<'a> {
    surface_format: gpu::TextureFormat,
    atlas_size: u32,
    max_instances: usize,
    tex_format: gpu::TextureFormat,
    filter: gpu::FilterMode,
    shader_source: &'a str,
    blend: gpu::BlendState,
    label: &'a str,
    blending_flags: u32,
}

impl AtlasLayer {
    fn new(context: &gpu::Context, cfg: &AtlasLayerConfig<'_>) -> Self {
        let atlas_size = cfg.atlas_size;
        let max_instances = cfg.max_instances;
        let tex_format = cfg.tex_format;
        let filter = cfg.filter;
        let shader_source = cfg.shader_source;
        let label = cfg.label;
        let surface_format = cfg.surface_format;
        let blend = cfg.blend;
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
            size: BLADE_UNIFORM_STRIDE * max_instances.max(1) as u64,
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
            blending_flags: cfg.blending_flags,
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
            let zeros_size = (self.atlas_size as u64)
                .saturating_mul(self.atlas_size as u64)
                .saturating_mul(self.bpp as u64) as usize;
            unsafe {
                ptr::write_bytes(self.staging_buffer.data(), 0, zeros_size);
            }
            context.sync_buffer(self.staging_buffer);
            {
                let mut transfer = encoder.transfer("glyph_clear");
                transfer.copy_buffer_to_texture(
                    self.staging_buffer.at(0),
                    self.atlas_size.saturating_mul(self.bpp),
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

        let staging_capacity = (self.atlas_size as u64)
            .saturating_mul(self.atlas_size as u64)
            .saturating_mul(self.bpp as u64);

        // Two-phase: first measure how many items fit in the staging
        // buffer and copy their payloads; then `drain(..fit_count)` so
        // the tail remains in `pending_uploads` for the next frame.
        // Previous code used `drain(..)` with a break on overflow, which
        // silently dropped the unissued tail and left those glyph slots
        // blank in the atlas.
        let mut cursor: u64 = 0;
        let mut fit_count = 0usize;

        for upload in pending_uploads.iter() {
            let row_bytes = (upload.w as u64).saturating_mul(self.bpp as u64);
            let total_bytes = row_bytes.saturating_mul(upload.h as u64);

            if cursor + total_bytes > staging_capacity {
                log::warn!(
                    "staging buffer overflow, deferring {} glyph uploads to next frame",
                    pending_uploads.len() - fit_count,
                );
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
            fit_count += 1;
        }

        context.sync_buffer(self.staging_buffer);

        let mut offset: u64 = 0;
        for upload in pending_uploads.drain(..fit_count) {
            let row_bytes = (upload.w as u64).saturating_mul(self.bpp as u64);
            let total_bytes = row_bytes.saturating_mul(upload.h as u64);

            {
                let mut transfer = encoder.transfer("glyph_upload");
                transfer.copy_buffer_to_texture(
                    self.staging_buffer.at(offset),
                    row_bytes as u32,
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

    /// Upload glyph instances to the GPU. Called once per frame per layer.
    fn upload_instances(
        &self,
        instances: &[GlyphInstance],
        max_instances: usize,
        viewport_w: f32,
        viewport_h: f32,
    ) {
        if instances.is_empty() {
            return;
        }
        let count = instances.len().min(max_instances);
        let _ = (viewport_w, viewport_h);
        let data = bytemuck::cast_slice(&instances[..count]);
        unsafe {
            ptr::copy_nonoverlapping(data.as_ptr(), self.instance_buffer.data(), data.len());
        }
    }

    #[allow(clippy::too_many_arguments)] // Clippy 1.94: GPU uniform upload keeps packed fields explicit.
    fn write_uniform(
        &self,
        slot: usize,
        max_instances: usize,
        viewport_w: f32,
        viewport_h: f32,
        pane_origin: [f32; 2],
        pane_size: [f32; 2],
        pane_radii: [f32; 4],
    ) -> u64 {
        let offset = slot as u64 * BLADE_UNIFORM_STRIDE;
        debug_assert!(
            offset + GLYPH_UNIFORM_RECORD_SIZE
                <= BLADE_UNIFORM_STRIDE * max_instances.max(1) as u64
        );
        let viewport = [viewport_w, viewport_h, 0.0f32, 0.0f32];
        let flags = self.blending_flags;
        let pad = [0u32; 3];
        unsafe {
            let dst = self.uniform_buffer.data().add(offset as usize);
            ptr::copy_nonoverlapping(viewport.as_ptr() as *const u8, dst, 16);
            ptr::copy_nonoverlapping(&flags as *const u32 as *const u8, dst.add(16), 4);
            ptr::copy_nonoverlapping(pad.as_ptr() as *const u8, dst.add(20), 12);
            ptr::copy_nonoverlapping(pane_origin.as_ptr() as *const u8, dst.add(32), 8);
            ptr::copy_nonoverlapping(pane_size.as_ptr() as *const u8, dst.add(40), 8);
            ptr::copy_nonoverlapping(pane_radii.as_ptr() as *const u8, dst.add(48), 16);
        }
        offset
    }

    /// Draw scissored glyph batches.
    /// Can be called multiple times after a single `upload_instances`.
    #[allow(clippy::too_many_arguments)] // Clippy 1.94: draw call mirrors backend state and viewport inputs.
    fn draw_batches(
        &self,
        pass: &mut gpu::RenderCommandEncoder,
        max_instances: usize,
        instance_count: usize,
        batches: &[PaneGlyphRange],
        viewport_w: f32,
        viewport_h: f32,
        uniform_slot: &mut usize,
    ) {
        if instance_count == 0 || batches.is_empty() {
            return;
        }
        let count = instance_count.min(max_instances);

        for batch in batches {
            let start = (batch.start as usize).min(count);
            let end = start.saturating_add(batch.count as usize).min(count);
            let (x, y, w, h) = batch.scissor;
            if start >= end || w == 0 || h == 0 || *uniform_slot >= max_instances {
                continue;
            }
            let uniform_offset = self.write_uniform(
                *uniform_slot,
                max_instances,
                viewport_w,
                viewport_h,
                batch.pane_origin,
                batch.pane_size,
                batch.pane_radii,
            );
            *uniform_slot += 1;

            let mut pe = pass.with(&self.pipeline);
            pe.bind(
                0,
                &GlyphShaderData {
                    atlas_tex: self.texture_view,
                    atlas_sampler: self.sampler,
                    viewport: self.uniform_buffer.at(uniform_offset),
                },
            );
            pe.bind_vertex(0, self.instance_buffer.at(0));
            pe.set_scissor_rect(&gpu::ScissorRect {
                x: x as i32,
                y: y as i32,
                w,
                h,
            });
            pe.draw(0, 4, start as u32, (end - start) as u32);
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

// ─── Overview wallpaper pipeline ────────────────────────────────────
//
// One vertexless fullscreen quad sampled from a user-uploaded RGBA8
// texture. Mirrors the DX / GL counterparts: cover-UV math, premult
// alpha output, drawn after the clear and before pane bgs. The
// `vi`-indexed corner table + the `1.0 - vuv.y * 2.0` Y-flip line
// screen-top up with image-row-0 without a manual texture flip.

#[derive(blade_macros::ShaderData)]
struct BackgroundImageData {
    uniforms: gpu::BufferPiece,
    bg_tex: gpu::TextureView,
    bg_sampler: gpu::Sampler,
}

struct BackgroundImageTextureSlot {
    texture: gpu::Texture,
    texture_view: gpu::TextureView,
    width: u32,
    height: u32,
}

struct BackgroundImagePipeline {
    pipeline: gpu::RenderPipeline,
    uniform_buffer: gpu::Buffer,
    sampler: gpu::Sampler,
    texture: Option<BackgroundImageTextureSlot>,
}

impl BackgroundImagePipeline {
    fn new(context: &gpu::Context, format: gpu::TextureFormat) -> Self {
        let shader = context.create_shader(gpu::ShaderDesc {
            source: BACKGROUND_IMAGE_SHADER,
        });

        let uniform_buffer = context.create_buffer(gpu::BufferDesc {
            name: "background_image_uniform",
            // 32 bytes — viewport_tex_size (vec4) + params (vec4). The
            // BLADE_UNIFORM_STRIDE alignment isn't strictly needed (we
            // only ever bind slot 0) but matches the rest of the file.
            size: BLADE_UNIFORM_STRIDE,
            memory: gpu::Memory::Shared,
        });

        let sampler = context.create_sampler(gpu::SamplerDesc {
            name: "background_image",
            address_modes: [gpu::AddressMode::ClampToEdge; 3],
            mag_filter: gpu::FilterMode::Linear,
            min_filter: gpu::FilterMode::Linear,
            mipmap_filter: gpu::FilterMode::Nearest,
            ..Default::default()
        });

        let pipeline = context.create_render_pipeline(gpu::RenderPipelineDesc {
            name: "background_image_pipeline",
            data_layouts: &[&BackgroundImageData::layout()],
            vertex: shader.at("vs_main"),
            // Vertexless: VS pulls corners from the `vi` index table.
            vertex_fetches: &[],
            primitive: gpu::PrimitiveState {
                topology: gpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            fragment: Some(shader.at("fs_main")),
            color_targets: &[gpu::ColorTargetState {
                format,
                blend: Some(gpu::BlendState {
                    color: gpu::BlendComponent {
                        src_factor: gpu::BlendFactor::One,
                        dst_factor: gpu::BlendFactor::OneMinusSrcAlpha,
                        operation: gpu::BlendOperation::Add,
                    },
                    alpha: gpu::BlendComponent::OVER,
                }),
                write_mask: gpu::ColorWrites::all(),
            }],
            multisample_state: gpu::MultisampleState::default(),
        });

        BackgroundImagePipeline {
            pipeline,
            uniform_buffer,
            sampler,
            texture: None,
        }
    }

    /// Upload `rgba` into a freshly-allocated GPU texture, replacing any
    /// previously-uploaded image. Returns the staging buffer the caller
    /// must destroy *after* the next `submit + wait_for(sync)` so the GPU
    /// is done copying before the buffer is freed.
    fn upload(
        &mut self,
        context: &gpu::Context,
        encoder: &mut gpu::CommandEncoder,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<gpu::Buffer> {
        if width == 0 || height == 0 {
            anyhow::bail!("overview bg image has zero dimension ({width}x{height})");
        }
        let expected = (width as usize) * (height as usize) * 4;
        if rgba.len() != expected {
            anyhow::bail!(
                "overview bg image byte count mismatch: got {}, expected {} ({width}x{height} RGBA8)",
                rgba.len(),
                expected
            );
        }
        if let Some(prev) = self.texture.take() {
            context.destroy_texture_view(prev.texture_view);
            context.destroy_texture(prev.texture);
        }

        let texture = context.create_texture(gpu::TextureDesc {
            name: "background_image_texture",
            format: gpu::TextureFormat::Rgba8Unorm,
            size: gpu::Extent {
                width,
                height,
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
                name: "background_image_view",
                format: gpu::TextureFormat::Rgba8Unorm,
                dimension: gpu::ViewDimension::D2,
                subresources: &gpu::TextureSubresources::default(),
            },
        );

        let staging = context.create_buffer(gpu::BufferDesc {
            name: "background_image_staging",
            size: rgba.len() as u64,
            memory: gpu::Memory::Upload,
        });
        unsafe {
            ptr::copy_nonoverlapping(rgba.as_ptr(), staging.data(), rgba.len());
        }
        context.sync_buffer(staging);

        {
            let mut transfer = encoder.transfer("background_image_upload");
            transfer.copy_buffer_to_texture(
                staging.at(0),
                width.saturating_mul(4),
                gpu::TexturePiece {
                    texture,
                    mip_level: 0,
                    array_layer: 0,
                    origin: [0, 0, 0],
                },
                gpu::Extent {
                    width,
                    height,
                    depth: 1,
                },
            );
        }

        self.texture = Some(BackgroundImageTextureSlot {
            texture,
            texture_view,
            width,
            height,
        });
        Ok(staging)
    }

    fn clear(&mut self, context: &gpu::Context) {
        if let Some(prev) = self.texture.take() {
            context.destroy_texture_view(prev.texture_view);
            context.destroy_texture(prev.texture);
        }
    }

    /// Issue the textured-quad draw if a texture is bound and `opacity > 0`.
    /// Returns `true` iff a draw was actually issued — callers use that to
    /// suppress the prepended baseline rect that would otherwise erase the
    /// image.
    fn draw(
        &self,
        pass: &mut gpu::RenderCommandEncoder,
        viewport_w: f32,
        viewport_h: f32,
        opacity: f32,
    ) -> bool {
        let Some(tex) = self.texture.as_ref() else {
            return false;
        };
        if opacity <= 0.0 {
            return false;
        }
        let cb = [
            viewport_w,
            viewport_h,
            tex.width as f32,
            tex.height as f32,
            opacity,
            0.0,
            0.0,
            0.0,
        ];
        unsafe {
            ptr::copy_nonoverlapping(
                cb.as_ptr() as *const u8,
                self.uniform_buffer.data(),
                std::mem::size_of_val(&cb),
            );
        }

        let mut pe = pass.with(&self.pipeline);
        pe.bind(
            0,
            &BackgroundImageData {
                uniforms: self.uniform_buffer.at(0),
                bg_tex: tex.texture_view,
                bg_sampler: self.sampler,
            },
        );
        // Vertexless: 6 vertices forming two triangles for the fullscreen quad.
        pe.draw(0, 6, 0, 1);
        true
    }

    fn destroy(&mut self, context: &gpu::Context) {
        if let Some(prev) = self.texture.take() {
            context.destroy_texture_view(prev.texture_view);
            context.destroy_texture(prev.texture);
        }
        context.destroy_render_pipeline(&mut self.pipeline);
        context.destroy_buffer(self.uniform_buffer);
        context.destroy_sampler(self.sampler);
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
        blending_flags: u32,
    ) -> Self {
        // Inject shared color functions into alpha fragment shader.
        let alpha_fragment = ALPHA_FRAGMENT
            .replace("// WGSL_COLOR_FUNCS_PLACEHOLDER", WGSL_COLOR_FUNCS)
            .replace("// WGSL_CORNER_FUNCS_PLACEHOLDER", WGSL_CORNER_FUNCS);
        let alpha_shader_src = format!("{VERTEX_SHADER}\n{alpha_fragment}");
        let alpha = AtlasLayer::new(
            context,
            &AtlasLayerConfig {
                surface_format,
                atlas_size,
                max_instances,
                tex_format: gpu::TextureFormat::R8Unorm,
                filter: gpu::FilterMode::Linear,
                shader_source: &alpha_shader_src,
                blend: gpu::BlendState {
                    color: gpu::BlendComponent {
                        src_factor: gpu::BlendFactor::One,
                        dst_factor: gpu::BlendFactor::OneMinusSrcAlpha,
                        operation: gpu::BlendOperation::Add,
                    },
                    alpha: gpu::BlendComponent::OVER,
                },
                label: "glyph_atlas",
                blending_flags,
            },
        );

        let color_fragment =
            COLOR_FRAGMENT.replace("// WGSL_CORNER_FUNCS_PLACEHOLDER", WGSL_CORNER_FUNCS);
        let color_shader_src = format!("{VERTEX_SHADER}\n{color_fragment}");
        let color = AtlasLayer::new(
            context,
            &AtlasLayerConfig {
                surface_format,
                atlas_size,
                max_instances,
                tex_format: gpu::TextureFormat::Rgba8Unorm,
                filter: gpu::FilterMode::Linear,
                shader_source: &color_shader_src,
                blend: gpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
                label: "color_emoji_atlas",
                blending_flags,
            },
        );

        GlyphAtlasGpu {
            alpha,
            color,
            max_instances,
        }
    }

    /// Flush pending glyph uploads from the cache to GPU.
    ///
    /// If the per-layer staging buffer can't fit every pending upload in
    /// one frame, the tails stay in `alpha_pending` / `color_pending`
    /// after `flush_uploads` returns; those tails are pushed back into
    /// the cache via `restore_pending` so the next frame retries them
    /// instead of dropping the glyphs on the floor.
    pub fn flush_uploads(
        &self,
        context: &gpu::Context,
        encoder: &mut gpu::CommandEncoder,
        cache: &mut GlyphCache,
    ) {
        let (mut alpha_pending, mut color_pending, alpha_clear, color_clear) = cache.take_pending();
        self.alpha
            .flush_uploads(context, encoder, &mut alpha_pending, alpha_clear);
        self.color
            .flush_uploads(context, encoder, &mut color_pending, color_clear);
        cache.restore_pending(alpha_pending, color_pending);
    }

    /// Initialize atlas textures on GPU. Must be called once before first render.
    pub fn init_textures(&self, encoder: &mut gpu::CommandEncoder) {
        encoder.init_texture(self.alpha.texture);
        encoder.init_texture(self.color.texture);
    }

    /// Upload alpha glyph instances. Called once per frame.
    pub fn upload_alpha_instances(
        &self,
        instances: &[GlyphInstance],
        viewport_w: f32,
        viewport_h: f32,
    ) {
        self.alpha
            .upload_instances(instances, self.max_instances, viewport_w, viewport_h);
    }

    /// Upload color glyph instances. Called once per frame.
    pub fn upload_color_instances(
        &self,
        instances: &[GlyphInstance],
        viewport_w: f32,
        viewport_h: f32,
    ) {
        self.color
            .upload_instances(instances, self.max_instances, viewport_w, viewport_h);
    }

    /// Draw alpha glyph batches. Can be called multiple times after upload.
    pub fn draw_alpha_batches(
        &self,
        pass: &mut gpu::RenderCommandEncoder,
        instance_count: usize,
        batches: &[PaneGlyphRange],
        viewport_w: f32,
        viewport_h: f32,
        uniform_slot: &mut usize,
    ) {
        self.alpha.draw_batches(
            pass,
            self.max_instances,
            instance_count,
            batches,
            viewport_w,
            viewport_h,
            uniform_slot,
        );
    }

    /// Draw color glyph batches. Can be called multiple times after upload.
    pub fn draw_color_batches(
        &self,
        pass: &mut gpu::RenderCommandEncoder,
        instance_count: usize,
        batches: &[PaneGlyphRange],
        viewport_w: f32,
        viewport_h: f32,
        uniform_slot: &mut usize,
    ) {
        self.color.draw_batches(
            pass,
            self.max_instances,
            instance_count,
            batches,
            viewport_w,
            viewport_h,
            uniform_slot,
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
    sdf: SdfPipeline,
    background_image: BackgroundImagePipeline,
    surface_config: gpu::SurfaceConfig,
    surface_format: gpu::TextureFormat,
    window: Arc<Window>,
    /// Previous frame's sync point — waited on before writing to shared
    /// instance buffers so the GPU is done reading them.
    last_sync: Option<gpu::SyncPoint>,
    /// Pending resize dimensions, applied on next `apply_surface()`.
    /// `surface_size()` returns committed (current swapchain) dimensions,
    /// not these pending values, so rendering stays consistent during
    /// deferred live resize.
    pending_size: Option<(u32, u32)>,
    /// Packed blending flags: bit 0 = use_linear_blending, bit 1 = use_linear_correction.
    blending_flags: u32,
}

impl Renderer {
    /// Clamp dimensions to the current monitor's physical size to avoid
    /// exceeding Vulkan surface capabilities (which are typically capped
    /// at the monitor's native resolution on Windows).
    fn clamp_to_monitor(window: &Window, w: u32, h: u32) -> (u32, u32) {
        if let Some(monitor) = window.current_monitor() {
            let max = monitor.size();
            (w.min(max.width), h.min(max.height))
        } else {
            (w, h)
        }
    }

    pub fn new(window: Arc<Window>, render_config: &RenderConfig) -> Result<Self> {
        let size = window.inner_size();
        let (clamped_w, clamped_h) = Self::clamp_to_monitor(&window, size.width, size.height);

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

        let display_sync = match render_config.present_mode {
            loom_config::config::PresentMode::Mailbox => gpu::DisplaySync::Recent,
            loom_config::config::PresentMode::Immediate => gpu::DisplaySync::Tear,
            loom_config::config::PresentMode::Fifo => gpu::DisplaySync::Block,
        };

        let surface_config = gpu::SurfaceConfig {
            size: gpu::Extent {
                width: clamped_w.max(1),
                height: clamped_h.max(1),
                depth: 1,
            },
            usage: gpu::TextureUsage::TARGET,
            display_sync,
            color_space: gpu::ColorSpace::Srgb,
            transparent: false,
            allow_exclusive_full_screen: false,
        };

        let surface = context
            .create_surface_configured(&*window, surface_config)
            .map_err(|e| anyhow::anyhow!("Surface creation failed: {e:?}"))?;

        let surface_format = surface.info().format;
        log::info!("Surface format: {:?}", surface_format);

        let encoder = context.create_command_encoder(gpu::CommandEncoderDesc {
            name: "loom",
            buffer_count: 2,
        });

        let rects = RectPipeline::new(&context, surface_format, render_config.max_rectangles);
        let sdf = SdfPipeline::new(&context, surface_format, MAX_SDF_RECTS);
        let background_image = BackgroundImagePipeline::new(&context, surface_format);

        let blending_flags = (render_config.alpha_blending.is_linear() as u32)
            | ((render_config.alpha_blending.use_correction() as u32) << 1);

        Ok(Renderer {
            context,
            surface,
            encoder,
            rects,
            sdf,
            background_image,
            surface_config,
            surface_format,
            window,
            last_sync: None,
            pending_size: None,
            blending_flags,
        })
    }

    // ─── Surface management ──────────────────────────────────────────

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.pending_size = Some((width, height));
        }
    }

    pub fn apply_surface(&mut self) {
        if self.pending_size.take().is_some() {
            // Query the window's actual current size rather than using the
            // cached resize-event value.  On Windows the Vulkan surface
            // capabilities are derived from GetClientRect, so the swapchain
            // extent must match the real HWND client area — which can differ
            // from what winit reported in the Resized event (e.g. DPI
            // virtualisation, compositor clamping, or a stale event).
            let size = self.window.inner_size();
            let (w, h) = Self::clamp_to_monitor(&self.window, size.width, size.height);
            let w = w.max(1);
            let h = h.max(1);
            if w != self.surface_config.size.width || h != self.surface_config.size.height {
                self.surface_config.size.width = w;
                self.surface_config.size.height = h;
                self.context
                    .reconfigure_surface(&mut self.surface, self.surface_config);
            }
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

    /// Upload the background image texture. Reuses the renderer's
    /// command encoder for the staging-buffer transfer (the encoder
    /// is idle between `draw_frame` calls, so a one-shot start +
    /// submit here is safe). Replaces any previously-uploaded image.
    pub fn set_background_image(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<()> {
        // Make sure the GPU is done reading the previous wallpaper texture
        // before destroying it (`upload` will free the prior slot).
        if let Some(ref sp) = self.last_sync {
            self.context.wait_for(sp, 5000);
        }
        self.encoder.start();
        let staging =
            self.background_image
                .upload(&self.context, &mut self.encoder, rgba, width, height)?;
        let sync = self.context.submit(&mut self.encoder);
        // Wait so the GPU has consumed the staging buffer's contents,
        // then free it — keeping it around would just hold ~size_of_image
        // bytes of `Memory::Upload` permanently for no reason.
        //
        // If `wait_for` times out the GPU may still be reading from the
        // staging buffer; freeing it then would be a use-after-free in
        // GPU memory. Leaking ~size_of_image bytes is the safer choice
        // — the device is in a bad state already (5s with no progress
        // is usually a hang or driver crash) and we'd rather not crash
        // on top of that.
        let consumed = self.context.wait_for(&sync, 5000);
        if consumed {
            self.context.destroy_buffer(staging);
        } else {
            log::warn!(
                "background image upload sync timed out after 5s; leaking staging buffer to avoid GPU UAF"
            );
        }
        // Track the upload submit so later operations don't wait on a
        // stale older sync.
        self.last_sync = Some(sync);
        Ok(())
    }

    /// Drop the wallpaper texture, if any.
    pub fn clear_background_image(&mut self) {
        if let Some(ref sp) = self.last_sync {
            self.context.wait_for(sp, 5000);
        }
        self.background_image.clear(&self.context);
    }

    // ─── Atlas init ──────────────────────────────────────────────────

    /// Create a new GlyphCache + GlyphAtlasGpu bound to this renderer's GPU context.
    pub fn create_atlas(
        &mut self,
        params: &loom_render::glyph_cache::FontInitParams,
    ) -> (GlyphCache, GlyphAtlasGpu) {
        let cache = GlyphCache::new(params);

        let atlas_gpu = GlyphAtlasGpu::new(
            &self.context,
            self.surface_format,
            cache.atlas_size,
            cache.max_instances,
            self.blending_flags,
        );

        // Initialize atlas textures on GPU. Track the sync point in
        // `last_sync` so a subsequent `set_background_image` (which can
        // fire from the wallpaper-decode worker before the first
        // `draw_frame`) waits on this submit before destroying any
        // previously-uploaded texture. Without this, `last_sync` would
        // be `None` between init and first frame, the guard would be
        // skipped, and the atlas init work could still be in flight.
        self.encoder.start();
        atlas_gpu.init_textures(&mut self.encoder);
        self.last_sync = Some(self.context.submit(&mut self.encoder));

        (cache, atlas_gpu)
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
        let mut profiler = crate::DrawFrameProfiler::begin("blade");
        // Wait for the previous frame to finish before writing to shared
        // instance buffers.  Without this the GPU may still be reading
        // from the buffers we are about to overwrite, which causes
        // ERROR_DEVICE_LOST on Windows / NVIDIA Vulkan.
        //
        // Track whether the GPU is actually idle: `wait_for` returns false on
        // timeout (still in flight). `RectPipeline::ensure_capacity` destroys
        // the old buffer immediately (blade-graphics 0.7.1 has no deferred
        // destruction), so growth is only safe when no prior submit is
        // pending — `None` means we've never submitted, `true` means the
        // fence completed.
        let gpu_idle = match self.last_sync {
            None => true,
            Some(ref sp) => {
                let wait_start = std::time::Instant::now();
                let ok = self.context.wait_for(sp, 5000);
                if let Some(profiler) = profiler.as_mut() {
                    profiler.record_sync_wait(wait_start);
                }
                if !ok {
                    log::warn!("blade: wait_for previous-frame fence timed out (5s)");
                }
                ok
            }
        };

        let (vw, vh) = self.surface_size();
        let vw_f = vw as f32;
        let vh_f = vh as f32;

        self.encoder.start();

        // Flush deferred glyph uploads
        let acquire_start = std::time::Instant::now();
        let frame = self.surface.acquire_frame();
        if let Some(profiler) = profiler.as_mut() {
            profiler.record_sync_wait(acquire_start);
        }
        let upload_start = std::time::Instant::now();
        atlas_gpu.flush_uploads(&self.context, &mut self.encoder, cache);

        {
            let mut pass = self.encoder.render(
                "loom",
                gpu::RenderTargetSet {
                    colors: &[gpu::RenderTarget {
                        view: frame.texture_view(),
                        init_op: gpu::InitOp::Clear(gpu::TextureColor::OpaqueBlack),
                        finish_op: gpu::FinishOp::Store,
                    }],
                    depth_stencil: None,
                },
            );

            // 1. Upload all background rects (baseline + pane + overlay)
            // once. Baseline is ALWAYS opaque `scene.clear_color`. We
            // draw it FIRST (before the wallpaper) so any wallpaper
            // dim alpha-blends toward the configured background colour
            // — matching DX/GL which clear the framebuffer to
            // `scene.clear_color` directly. Blade's `TextureColor`
            // doesn't support a custom-RGBA clear, hence the explicit
            // baseline rect approach.
            let mut all_bg = Vec::with_capacity(1 + scene.bg_rects.len());
            all_bg.push(Rect {
                x: 0.0,
                y: 0.0,
                w: vw_f,
                h: vh_f,
                color: scene.clear_color,
            });
            all_bg.extend_from_slice(scene.bg_rects);
            let active_bg_idx = 1 + scene.active_bg_start;
            let overlay_bg_idx = 1 + scene.overlay_bg_start;
            if gpu_idle {
                self.rects.ensure_capacity(&self.context, all_bg.len());
            }
            let total_bg = all_bg.len().min(self.rects.max_rects);
            self.rects.upload(&all_bg, vw_f, vh_f);
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

            let mut rect_uniform_slot = 0usize;

            // 2. Draw the baseline rect (index 0) BEFORE the wallpaper
            // so wallpaper alpha-blends against `scene.clear_color`,
            // not the OpaqueBlack init clear.
            let baseline_range = [PaneRectRange {
                start: 0,
                count: 1,
                ..PaneRectRange::default()
            }];
            self.rects.draw_ranges(
                &mut pass,
                &baseline_range,
                vw_f,
                vh_f,
                &mut rect_uniform_slot,
            );

            // 3. Overview wallpaper, if any. Self-checks the texture/
            // opacity gate, so passing 0.0 is a no-op. Returns true
            // iff a draw was actually issued — kept around as a
            // diagnostic but no longer drives baseline transparency.
            let _wallpaper_drawn =
                self.background_image
                    .draw(&mut pass, vw_f, vh_f, scene.background_image_opacity);

            // 4. Draw non-focused pane background rects, starting from
            // index 1 — the baseline at index 0 already painted above.
            let inactive_bg_count = active_bg_idx.min(total_bg);
            let inactive_bg_ranges = split_ranges(&all_bg_ranges, 1, inactive_bg_count);
            self.rects.draw_ranges(
                &mut pass,
                &inactive_bg_ranges,
                vw_f,
                vh_f,
                &mut rect_uniform_slot,
            );

            // 3. Upload alpha + color glyph instances once.
            atlas_gpu.upload_alpha_instances(scene.glyphs, vw_f, vh_f);
            atlas_gpu.upload_color_instances(scene.color_glyphs, vw_f, vh_f);
            let alpha_count = scene.glyphs.len();
            let color_count = scene.color_glyphs.len();
            let mut alpha_uniform_slot = 0usize;
            let mut color_uniform_slot = 0usize;
            if let Some(profiler) = profiler.as_mut() {
                profiler.record_cpu_upload(upload_start);
            }

            // 4. Draw inactive pane glyphs (scissored).
            let draw_start = std::time::Instant::now();
            atlas_gpu.draw_alpha_batches(
                &mut pass,
                alpha_count,
                scene.glyph_batches,
                vw_f,
                vh_f,
                &mut alpha_uniform_slot,
            );
            atlas_gpu.draw_color_batches(
                &mut pass,
                color_count,
                scene.color_glyph_batches,
                vw_f,
                vh_f,
                &mut color_uniform_slot,
            );

            // 5. Focused pane background rects.
            let active_bg_count = overlay_bg_idx.saturating_sub(active_bg_idx);
            if active_bg_count > 0 {
                let active_bg_ranges =
                    split_ranges(&all_bg_ranges, active_bg_idx, overlay_bg_idx.min(total_bg));
                self.rects.draw_ranges(
                    &mut pass,
                    &active_bg_ranges,
                    vw_f,
                    vh_f,
                    &mut rect_uniform_slot,
                );
            }

            // 6. Draw active pane glyphs (scissored, no re-upload).
            atlas_gpu.draw_alpha_batches(
                &mut pass,
                alpha_count,
                scene.active_glyph_batches,
                vw_f,
                vh_f,
                &mut alpha_uniform_slot,
            );
            atlas_gpu.draw_color_batches(
                &mut pass,
                color_count,
                scene.active_color_glyph_batches,
                vw_f,
                vh_f,
                &mut color_uniform_slot,
            );

            // 7. Overlay background rects (rendered after pane glyphs so they
            //    occlude terminal text underneath popups like the context menu).
            let overlay_bg_count = total_bg.saturating_sub(overlay_bg_idx);
            if overlay_bg_count > 0 {
                let overlay_bg_ranges = split_ranges(&all_bg_ranges, overlay_bg_idx, total_bg);
                self.rects.draw_ranges(
                    &mut pass,
                    &overlay_bg_ranges,
                    vw_f,
                    vh_f,
                    &mut rect_uniform_slot,
                );
            }

            // 7b. Layered chrome. Two passes — Base then Overlay —
            //     each one a (SDF rects → glyphs) pair, so Overlay
            //     rects occlude Base glyphs (settings_panel labels
            //     under a popup, top_bar text under a palette). The
            //     GPU pipeline's natural "all rects then all glyphs"
            //     order would otherwise paint Overlay rects first and
            //     ALL glyphs (including Base) on top of them, breaking
            //     the popup's visual occlusion contract.
            //
            //     Per-stream layout in scene buffers:
            //       sdf_rects:    [pane_rings | base_chrome | overlay+transient]
            //                     [..base_sdf_end]    [base_sdf_end..]
            //       glyphs:       [pane | base_chrome | overlay+transient]
            //                     [..pane_glyph_end] [pane_glyph_end..base_glyph_end] [base_glyph_end..]
            //
            //     Transient widgets (search_bar / bell_flash /
            //     ime_preedit) ride the Overlay pass — they're
            //     mutually exclusive with palette / context_menu and
            //     don't need their own tier.
            let total_sdf = scene.sdf_rects.len();
            let base_sdf_end = scene.chrome_base_sdf_end.min(total_sdf);
            let alpha_total = scene.glyphs.len();
            let color_total = scene.color_glyphs.len();
            let base_alpha_end = scene.chrome_base_alpha_glyph_end.min(alpha_total);
            let base_color_end = scene.chrome_base_color_glyph_end.min(color_total);
            // Overlay/Top split. Clamp into `[base_end, total]`: a scene
            // that didn't record a Top layer leaves these at/below the
            // base split, which collapses the Overlay pass to empty and
            // routes everything into the Top pass — identical output to
            // the old two-pass renderer, so unset values degrade safely.
            let overlay_sdf_end = scene.chrome_overlay_sdf_end.clamp(base_sdf_end, total_sdf);
            let overlay_alpha_end = scene
                .chrome_overlay_alpha_glyph_end
                .clamp(base_alpha_end, alpha_total);
            let overlay_color_end = scene
                .chrome_overlay_color_glyph_end
                .clamp(base_color_end, color_total);
            // Single upload covers all three layers' slices —
            // `draw_range` issues the contiguous sub-draws.
            if total_sdf > 0 {
                self.sdf.upload(scene.sdf_rects, vw_f, vh_f);
            }

            let make_glyph_range = |start: usize, end: usize| PaneGlyphRange {
                start: start as u32,
                count: end.saturating_sub(start) as u32,
                scissor: (0, 0, vw, vh),
                ..PaneGlyphRange::default()
            };

            // ── Base pass ────────────────────────────────────────────
            //   Pane focus rings (at indices `[..pane_sdf_len]`) ride
            //   along here — they were already in the SDF stream
            //   ahead of cached chrome, so the same `[0..base_sdf_end]`
            //   range covers both. Re-drawing rings under the same
            //   blending is idempotent given identical instances, and
            //   keeping the original draw call ordering avoids a
            //   second SDF pipeline switch.
            if base_sdf_end > 0 {
                self.sdf.draw_range(&mut pass, 0, base_sdf_end);
            }
            atlas_gpu.draw_alpha_batches(
                &mut pass,
                alpha_count,
                &[make_glyph_range(scene.pane_glyph_end, base_alpha_end)],
                vw_f,
                vh_f,
                &mut alpha_uniform_slot,
            );
            atlas_gpu.draw_color_batches(
                &mut pass,
                color_count,
                &[make_glyph_range(scene.pane_color_glyph_end, base_color_end)],
                vw_f,
                vh_f,
                &mut color_uniform_slot,
            );

            // ── Overlay pass (full-screen modals) ────────────────────
            if base_sdf_end < overlay_sdf_end {
                self.sdf
                    .draw_range(&mut pass, base_sdf_end, overlay_sdf_end - base_sdf_end);
            }
            atlas_gpu.draw_alpha_batches(
                &mut pass,
                alpha_count,
                &[make_glyph_range(base_alpha_end, overlay_alpha_end)],
                vw_f,
                vh_f,
                &mut alpha_uniform_slot,
            );
            atlas_gpu.draw_color_batches(
                &mut pass,
                color_count,
                &[make_glyph_range(base_color_end, overlay_color_end)],
                vw_f,
                vh_f,
                &mut color_uniform_slot,
            );

            // ── Top + transient pass (always-on-top popups) ──────────
            if overlay_sdf_end < total_sdf {
                self.sdf
                    .draw_range(&mut pass, overlay_sdf_end, total_sdf - overlay_sdf_end);
            }
            atlas_gpu.draw_alpha_batches(
                &mut pass,
                alpha_count,
                &[make_glyph_range(overlay_alpha_end, alpha_total)],
                vw_f,
                vh_f,
                &mut alpha_uniform_slot,
            );
            atlas_gpu.draw_color_batches(
                &mut pass,
                color_count,
                &[make_glyph_range(overlay_color_end, color_total)],
                vw_f,
                vh_f,
                &mut color_uniform_slot,
            );
            if let Some(profiler) = profiler.as_mut() {
                profiler.record_draw(draw_start);
            }
        }

        let present_start = std::time::Instant::now();
        self.encoder.present(frame);
        let sp = self.context.submit(&mut self.encoder);
        self.last_sync = Some(sp);
        if let Some(profiler) = profiler.as_mut() {
            profiler.record_present(present_start);
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

impl Drop for Renderer {
    fn drop(&mut self) {
        self.rects.destroy(&self.context);
        self.sdf.destroy(&self.context);
        self.background_image.destroy(&self.context);
        self.context.destroy_command_encoder(&mut self.encoder);
        self.context.destroy_surface(&mut self.surface);
    }
}

// ─── Vertex layout ───────────────────────────────────────────────────

fn glyph_vertex_layout() -> gpu::VertexLayout {
    gpu::VertexLayout {
        attributes: vec![
            (
                "pos",
                gpu::VertexAttribute {
                    offset: 0,
                    format: gpu::VertexFormat::F32Vec2,
                },
            ),
            (
                "size",
                gpu::VertexAttribute {
                    offset: 8,
                    format: gpu::VertexFormat::F32Vec2,
                },
            ),
            (
                "uv_pos",
                gpu::VertexAttribute {
                    offset: 16,
                    format: gpu::VertexFormat::F32Vec2,
                },
            ),
            (
                "uv_size",
                gpu::VertexAttribute {
                    offset: 24,
                    format: gpu::VertexFormat::F32Vec2,
                },
            ),
            (
                "color",
                gpu::VertexAttribute {
                    offset: 32,
                    format: gpu::VertexFormat::F32Vec4,
                },
            ),
            (
                "bg_color",
                gpu::VertexAttribute {
                    offset: 48,
                    format: gpu::VertexFormat::F32Vec4,
                },
            ),
        ],
        stride: std::mem::size_of::<GlyphInstance>() as u32,
    }
}

// ─── WGSL shaders ────────────────────────────────────────────────────

const SDF_SHADER: &str = r#"
struct Uniforms {
    viewport_size: vec4<f32>,
};

var<uniform> uniforms: Uniforms;

// Matches loom_render::sdf_rect::SdfRect (Rust) one-to-one. Both sides
// are a three-way contract with the vertex layout in SdfPipeline::new.
struct SdfInstance {
    pos: vec2<f32>,
    size: vec2<f32>,
    color: vec4<f32>,
    radii: vec4<f32>,           // tl, tr, br, bl
    border_color: vec4<f32>,
    border_width: f32,
    shadow_blur: f32,
    shadow_offset: vec2<f32>,
    shadow_color: vec4<f32>,
};

// Inflate the quad so shadow blur + offset spill outside the rect's bounds
// without clipping. 3σ covers ~99.7 % of a Gaussian-ish shadow envelope.
fn shadow_pad(inst: SdfInstance) -> f32 {
    return inst.shadow_blur * 3.0 + max(abs(inst.shadow_offset.x), abs(inst.shadow_offset.y));
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    // Centred local coordinate (in px) so the fragment can evaluate the
    // SDF without re-deriving the rect centre.
    @location(0) local: vec2<f32>,
    @location(1) half_size: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) radii: vec4<f32>,
    @location(4) border_color: vec4<f32>,
    @location(5) border_width: f32,
    @location(6) shadow_blur: f32,
    @location(7) shadow_offset: vec2<f32>,
    @location(8) shadow_color: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: SdfInstance) -> VertexOutput {
    let x = f32(vi & 1u);
    let y = f32((vi >> 1u) & 1u);

    let pad = shadow_pad(inst);
    let padded_pos = inst.pos - vec2<f32>(pad, pad);
    let padded_size = inst.size + vec2<f32>(pad * 2.0, pad * 2.0);

    let px = padded_pos + vec2<f32>(x, y) * padded_size;
    let ndc = vec2<f32>(
        px.x / uniforms.viewport_size.x * 2.0 - 1.0,
        1.0 - px.y / uniforms.viewport_size.y * 2.0
    );

    let centre = inst.pos + inst.size * 0.5;
    let local = px - centre;

    var out: VertexOutput;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.local = local;
    out.half_size = inst.size * 0.5;
    out.color = inst.color;
    out.radii = inst.radii;
    out.border_color = inst.border_color;
    out.border_width = inst.border_width;
    out.shadow_blur = inst.shadow_blur;
    out.shadow_offset = inst.shadow_offset;
    out.shadow_color = inst.shadow_color;
    return out;
}

// SDF of a rounded box centred at the origin. Per-corner radii order
// matches CSS: tl, tr, br, bl. Picks the corner based on which quadrant
// the sample point falls in.
fn sdf_rounded_box(p: vec2<f32>, b: vec2<f32>, r: vec4<f32>) -> f32 {
    let r_top_x = select(r.x, r.y, p.x > 0.0);    // tl | tr
    let r_bot_x = select(r.w, r.z, p.x > 0.0);    // bl | br
    let radius = select(r_top_x, r_bot_x, p.y > 0.0);
    let q = abs(p) - b + vec2<f32>(radius, radius);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - radius;
}

// Approximate Gaussian envelope for drop shadows.
fn shadow_envelope(d: f32, blur: f32) -> f32 {
    if (blur <= 0.0) { return 0.0; }
    // Smoothstep from `d = blur` (zero shadow) to `d = -blur` (full shadow).
    return clamp(0.5 - 0.5 * d / blur, 0.0, 1.0);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let d_body = sdf_rounded_box(in.local, in.half_size, in.radii);

    // Fill coverage with DPR-aware AA. `fwidth` returns the per-pixel rate
    // of change of `d_body`, so at 1x it's ~1.0 (one logical-pixel per
    // fragment) and at 3x it's ~0.333 — using half that as the AA
    // half-width keeps the visible edge exactly one physical pixel wide at
    // every DPR and overview zoom level. `max(_, 1e-5)` prevents a 0/0 at
    // derivative boundaries (not observed in practice but cheap insurance).
    let aa = max(fwidth(d_body) * 0.5, 1e-5);
    let body_alpha = clamp(0.5 - d_body / (aa * 2.0), 0.0, 1.0);

    // Border: SDF band centred at `d = -border_width/2`, half-width
    // `border_width/2`. The band sits inside the body (d_body in
    // [-border_width, 0]); its outer edge coincides with the body's
    // outer edge, so the band's own AA fringe naturally lines up with
    // the body's. Multiplying by `body_alpha` would double-apply AA at
    // that shared fringe and halve the visible border intensity by
    // ~0.25, so we leave it out.
    var border_alpha = 0.0;
    if (in.border_width > 0.0) {
        let half = in.border_width * 0.5;
        let d_band = abs(d_body + half) - half;
        border_alpha = clamp(0.5 - d_band / (aa * 2.0), 0.0, 1.0);
    }

    // Shadow: sample the SDF at the offset position.
    var shadow_col = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    if (in.shadow_blur > 0.0 && in.shadow_color.a > 0.0) {
        let d_shadow = sdf_rounded_box(in.local - in.shadow_offset, in.half_size, in.radii);
        let env = shadow_envelope(d_shadow, in.shadow_blur);
        // Shadow is occluded by the body itself to avoid a double-dark ring.
        let occlusion = 1.0 - body_alpha;
        let a = env * in.shadow_color.a * occlusion;
        shadow_col = vec4<f32>(in.shadow_color.rgb * a, a);
    }

    // Fill and border pre-multiplied so blending does OVER correctly.
    let body = vec4<f32>(in.color.rgb * in.color.a * body_alpha, in.color.a * body_alpha);
    let border = vec4<f32>(in.border_color.rgb * in.border_color.a * border_alpha,
                           in.border_color.a * border_alpha);

    // Shadow sits under everything, border over body.
    let out_rgb = shadow_col.rgb * (1.0 - body.a) + body.rgb * (1.0 - border.a) + border.rgb;
    let out_a   = shadow_col.a   * (1.0 - body.a) + body.a   * (1.0 - border.a) + border.a;
    return vec4<f32>(out_rgb, out_a);
}
"#;

#[cfg(test)]
mod shader_tests {
    //! Parse every embedded WGSL source through naga so shader-level typos
    //! (missing semicolons, unknown built-ins, type errors, etc.) fail
    //! `cargo test` instead of surfacing at the first `Renderer::new` call.
    //!
    //! We deliberately skip full `Validator` runs: blade patches in
    //! `@group/@binding` annotations via its `ShaderData` macro at
    //! pipeline-creation time, so the raw source lacks the bindings
    //! naga's validator requires. Parsing alone still catches syntax
    //! errors and unknown identifiers.
    use naga::front::wgsl;

    fn compile(name: &str, source: &str) {
        if let Err(e) = wgsl::parse_str(source) {
            panic!("{name}: WGSL parse failed:\n{}", e.emit_to_string(source));
        }
    }

    /// Substitute `// WGSL_CORNER_FUNCS_PLACEHOLDER` with the corner-
    /// alpha helpers, mirroring the runtime path at the pipeline
    /// constructors (lines 63, 1098, 1123). Without this, parsing the
    /// raw source surfaces an "unknown identifier `loom_corner_alpha`"
    /// error that doesn't reflect what the GPU actually compiles.
    fn corner_substitute(source: &str) -> String {
        source.replace("// WGSL_CORNER_FUNCS_PLACEHOLDER", super::WGSL_CORNER_FUNCS)
    }

    #[test]
    fn rect_shader_parses() {
        compile("RECT_SHADER", &corner_substitute(super::RECT_SHADER));
    }

    #[test]
    fn sdf_shader_parses() {
        compile("SDF_SHADER", super::SDF_SHADER);
    }

    #[test]
    fn background_image_shader_parses() {
        compile("BACKGROUND_IMAGE_SHADER", super::BACKGROUND_IMAGE_SHADER);
    }

    /// End-to-end smoke: instantiate a real `SdfPipeline` on a headless
    /// context, upload one full-coverage rect, draw into an offscreen
    /// target, and assert the pipeline creation + draw submission don't
    /// fail. Pipeline creation is the strictest check we have for
    /// vertex-layout / shader-attribute agreement — a mismatch (padding
    /// drift between `SdfRect` and the WGSL struct, wrong offsets) fails
    /// here before it ships. A full pixel readback would be ideal but
    /// would bind this test to a specific blend model; catching layout
    /// drift at pipeline creation is the high-value half.
    #[test]
    fn sdf_pipeline_round_trip_headless() {
        use super::*;
        use blade_graphics as gpu;

        let context = unsafe {
            gpu::Context::init(gpu::ContextDesc {
                presentation: false,
                validation: true,
                timing: false,
                capture: false,
                overlay: false,
                device_id: 0,
            })
        };
        // Skip gracefully when no GPU is available (CI without Vulkan/Metal).
        let Ok(context) = context else {
            eprintln!("sdf_pipeline_round_trip_headless: no GPU — skipping");
            return;
        };

        let format = gpu::TextureFormat::Rgba8Unorm;
        let mut sdf = SdfPipeline::new(&context, format, 16);

        let rect = loom_render::sdf_rect::SdfRect {
            pos: [0.0, 0.0],
            size: [32.0, 32.0],
            color: [1.0, 0.5, 0.25, 1.0],
            radii: [4.0; 4],
            border_color: [1.0, 1.0, 1.0, 1.0],
            border_width: 1.0,
            shadow_blur: 0.0,
            shadow_offset: [0.0, 0.0],
            shadow_color: [0.0; 4],
        };
        sdf.upload(&[rect], 64.0, 64.0);

        sdf.destroy(&context);
    }
}

const RECT_SHADER: &str = r#"
struct Uniforms {
    viewport_size: vec4<f32>,
    pane_origin: vec2<f32>,
    pane_size: vec2<f32>,
    pane_radii: vec4<f32>,
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
    @location(1) pane_local: vec2<f32>,
};

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
    out.pane_local = px - uniforms.pane_origin;
    return out;
}

// WGSL_CORNER_FUNCS_PLACEHOLDER

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = vec4<f32>(in.color.rgb * in.color.a, in.color.a);
    return color * loom_corner_alpha(in.pane_local, uniforms.pane_size, uniforms.pane_radii);
}
"#;

const VERTEX_SHADER: &str = r#"
struct Viewport {
    size: vec4<f32>,
    // flags: bit 0 = use_linear_blending, bit 1 = use_linear_correction
    flags: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
    pane_origin: vec2<f32>,
    pane_size: vec2<f32>,
    pane_radii: vec4<f32>,
};

var<uniform> viewport: Viewport;

struct Instance {
    pos: vec2<f32>,
    size: vec2<f32>,
    uv_pos: vec2<f32>,
    uv_size: vec2<f32>,
    color: vec4<f32>,
    bg_color: vec4<f32>,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) bg_color: vec4<f32>,
    @location(3) pane_local: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: Instance) -> VsOut {
    let x = f32(vi & 1u);
    let y = f32((vi >> 1u) & 1u);

    var out: VsOut;
    out.uv = inst.uv_pos + vec2<f32>(x, y) * inst.uv_size;
    out.color = inst.color;
    out.bg_color = inst.bg_color;
    let px = inst.pos + vec2<f32>(x, y) * inst.size;
    out.pane_local = px - viewport.pane_origin;
    let ndc = vec2<f32>(
        px.x / viewport.size.x * 2.0 - 1.0,
        1.0 - px.y / viewport.size.y * 2.0
    );
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    return out;
}
"#;

// ─── Shared WGSL functions for sRGB ↔ linear conversion ────────────
const WGSL_COLOR_FUNCS: &str = r#"
fn linearize_f(v: f32) -> f32 {
    return select(pow((v + 0.055) / 1.055, 2.4), v / 12.92, v <= 0.04045);
}

fn linearize_v3(srgb: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(linearize_f(srgb.x), linearize_f(srgb.y), linearize_f(srgb.z));
}

fn linearize_v4(srgb: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(linearize_v3(srgb.rgb), srgb.a);
}

fn unlinearize_f(v: f32) -> f32 {
    return select(pow(v, 1.0 / 2.4) * 1.055 - 0.055, v * 12.92, v <= 0.0031308);
}

fn unlinearize_v3(lin: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(unlinearize_f(lin.x), unlinearize_f(lin.y), unlinearize_f(lin.z));
}

fn unlinearize_v4(lin: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(unlinearize_v3(lin.rgb), lin.a);
}

fn luminance_linear(col: vec3<f32>) -> f32 {
    return dot(col, vec3<f32>(0.2126, 0.7152, 0.0722));
}
"#;

// Per-corner rounding alpha mask. WGSL twin of GL/DX corner clipping:
// CSS-order radii (tl, tr, br, bl), with all-zero radii as the no-op path.
const WGSL_CORNER_FUNCS: &str = r#"
fn loom_sdf_rounded_box(p: vec2<f32>, b: vec2<f32>, r: vec4<f32>) -> f32 {
    let rx = select(r.x, r.y, p.x > 0.0);
    let bx = select(r.w, r.z, p.x > 0.0);
    let radius = select(rx, bx, p.y > 0.0);
    let q = abs(p) - b + vec2<f32>(radius, radius);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0, 0.0))) - radius;
}

fn loom_corner_alpha(px: vec2<f32>, size: vec2<f32>, radii: vec4<f32>) -> f32 {
    if (radii.x <= 0.0 && radii.y <= 0.0 && radii.z <= 0.0 && radii.w <= 0.0) {
        return 1.0;
    }
    let d = loom_sdf_rounded_box(px - 0.5 * size, 0.5 * size, radii);
    let aa = max(fwidth(d) * 0.5, 1e-5);
    return 1.0 - smoothstep(-aa, aa, d);
}
"#;

const ALPHA_FRAGMENT: &str = r#"
var atlas_tex: texture_2d<f32>;
var atlas_sampler: sampler;

// WGSL_COLOR_FUNCS_PLACEHOLDER

// WGSL_CORNER_FUNCS_PLACEHOLDER

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let use_linear_correction = (viewport.flags & 2u) != 0u;

    var a = textureSample(atlas_tex, atlas_sampler, in.uv).r;

    // Weight correction: linearize to compute luminance, adjust alpha
    // so sRGB-space hardware blending approximates linear compositing.
    if (use_linear_correction) {
        let fg_linear = linearize_v4(in.color);
        let bg_linear = linearize_v4(in.bg_color);
        let fg_l = luminance_linear(fg_linear.rgb);
        let bg_l = luminance_linear(bg_linear.rgb);
        if (abs(fg_l - bg_l) > 0.001) {
            let blend_l = linearize_f(
                unlinearize_f(fg_l) * a + unlinearize_f(bg_l) * (1.0 - a)
            );
            a = clamp((blend_l - bg_l) / (fg_l - bg_l), 0.0, 1.0);
        }
    }

    // Output sRGB premultiplied with corrected alpha.
    let out_alpha = in.color.a * a;
    let out_color = vec4<f32>(in.color.rgb * out_alpha, out_alpha);
    return out_color * loom_corner_alpha(in.pane_local, viewport.pane_size, viewport.pane_radii);
}
"#;

const COLOR_FRAGMENT: &str = r#"
var atlas_tex: texture_2d<f32>;
var atlas_sampler: sampler;

// WGSL_CORNER_FUNCS_PLACEHOLDER

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas_tex, atlas_sampler, in.uv);
    let out_color = vec4<f32>(texel.rgb * in.color.rgb, texel.a * in.color.a);
    return out_color * loom_corner_alpha(in.pane_local, viewport.pane_size, viewport.pane_radii);
}
"#;

// ─── Overview wallpaper WGSL ────────────────────────────────────────
//
// Vertexless fullscreen quad. The `vi`-indexed corner table emits
// two triangles in [0,1] UV space; the cover-UV math scales them
// around 0.5 so the texture fills the viewport with the overflow
// axis cropped (aspect preserved). FS samples the wallpaper and
// outputs `(rgb * opacity, opacity)` for premult-alpha blending
// over the already-cleared `clear_color` framebuffer.
const BACKGROUND_IMAGE_SHADER: &str = r#"
struct Uniforms {
    viewport_tex_size: vec4<f32>,  // vw, vh, tw, th
    params: vec4<f32>,             // opacity, _, _, _
};

var<uniform> uniforms: Uniforms;
var bg_tex: texture_2d<f32>;
var bg_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertexOutput {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0),
    );
    let vuv = corners[vi];

    let vw = uniforms.viewport_tex_size.x;
    let vh = uniforms.viewport_tex_size.y;
    let tw = uniforms.viewport_tex_size.z;
    let th = uniforms.viewport_tex_size.w;
    let v_aspect = vw / max(vh, 1e-6);
    let t_aspect = tw / max(th, 1e-6);
    var scale = vec2<f32>(1.0, 1.0);
    if (t_aspect > v_aspect) {
        scale.x = v_aspect / max(t_aspect, 1e-6);
    } else {
        scale.y = t_aspect / max(v_aspect, 1e-6);
    }

    var out: VertexOutput;
    out.position = vec4<f32>(vuv.x * 2.0 - 1.0, 1.0 - vuv.y * 2.0, 0.0, 1.0);
    out.uv = (vuv - vec2<f32>(0.5, 0.5)) * scale + vec2<f32>(0.5, 0.5);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let col = textureSample(bg_tex, bg_sampler, in.uv);
    let opacity = clamp(uniforms.params.x, 0.0, 1.0);
    // Premultiply against texel alpha so transparent PNG pixels stay
    // transparent. Mirror of the GL/DX fix.
    let a = col.a * opacity;
    return vec4<f32>(col.rgb * a, a);
}
"#;
