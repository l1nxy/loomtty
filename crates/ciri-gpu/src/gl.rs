//! OpenGL 3.3+ / EGL backend.
//!
//! Designed for Linux Wayland where `eglSwapBuffers` handles resize implicitly,
//! avoiding the `vkDeviceWaitIdle` / swapchain rebuild overhead of Vulkan.
//!
//! Uses `glutin` for EGL context management and `glow` for GL calls.

use ciri_config::config::RenderConfig;
use ciri_render::FrameScene;
use ciri_render::glyph_cache::{GlyphCache, GlyphInstance, PaneGlyphRange, PendingUpload};
use ciri_render::rect::{PaneRectRange, Rect};
use ciri_render::sdf_rect::SdfRect;
use glow::HasContext;

/// Upper bound on SDF chrome rects per frame. Mirrors `MAX_SDF_RECTS` in
/// the blade backend so both backends drop the same tail under a flood.
const MAX_SDF_RECTS: usize = 256;
#[cfg(not(target_os = "macos"))]
use glutin::config::ConfigTemplateBuilder;
use glutin::context::PossiblyCurrentContext;
#[cfg(not(target_os = "macos"))]
use glutin::context::{ContextApi, ContextAttributesBuilder, Version};
use glutin::prelude::*;
use glutin::surface::WindowSurface;
#[cfg(not(target_os = "macos"))]
use glutin::surface::{SurfaceAttributesBuilder, SwapInterval};
#[cfg(not(target_os = "macos"))]
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::num::NonZeroU32;
use std::sync::Arc;
use winit::window::Window;

// ─── sRGB render target (FBO) ───────────────────────────────────────

/// Offscreen sRGB renderbuffer-backed FBO for linear-correct alpha blending.
/// When `use_linear_blending` is true, all rendering targets this FBO
/// (format `GL_SRGB8_ALPHA8`), so the GPU automatically linearises reads
/// and gamma-encodes writes. The final image is blitted to the default
/// framebuffer with `GL_FRAMEBUFFER_SRGB` disabled to avoid double gamma.
struct GlSrgbTarget {
    framebuffer: glow::Framebuffer,
    renderbuffer: glow::Renderbuffer,
    width: u32,
    height: u32,
}

impl GlSrgbTarget {
    unsafe fn new(gl: &glow::Context, width: u32, height: u32) -> crate::Result<Self> {
        let framebuffer = gl
            .create_framebuffer()
            .map_err(|e| crate::GpuError::ResourceCreate(format!("sRGB FBO: {e}")))?;
        let renderbuffer = gl
            .create_renderbuffer()
            .map_err(|e| crate::GpuError::ResourceCreate(format!("sRGB RBO: {e}")))?;

        gl.bind_renderbuffer(glow::RENDERBUFFER, Some(renderbuffer));
        gl.renderbuffer_storage(
            glow::RENDERBUFFER,
            glow::SRGB8_ALPHA8,
            width.max(1) as i32,
            height.max(1) as i32,
        );
        gl.bind_renderbuffer(glow::RENDERBUFFER, None);

        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
        gl.framebuffer_renderbuffer(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::RENDERBUFFER,
            Some(renderbuffer),
        );
        let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);

        if status != glow::FRAMEBUFFER_COMPLETE {
            gl.delete_framebuffer(framebuffer);
            gl.delete_renderbuffer(renderbuffer);
            return Err(crate::GpuError::ResourceCreate(format!(
                "sRGB FBO incomplete: status 0x{status:04X}"
            )));
        }

        Ok(GlSrgbTarget {
            framebuffer,
            renderbuffer,
            width: width.max(1),
            height: height.max(1),
        })
    }

    unsafe fn resize(&mut self, gl: &glow::Context, width: u32, height: u32) {
        let w = width.max(1);
        let h = height.max(1);
        if w == self.width && h == self.height {
            return;
        }
        gl.bind_renderbuffer(glow::RENDERBUFFER, Some(self.renderbuffer));
        gl.renderbuffer_storage(glow::RENDERBUFFER, glow::SRGB8_ALPHA8, w as i32, h as i32);
        gl.bind_renderbuffer(glow::RENDERBUFFER, None);
        self.width = w;
        self.height = h;
    }

    unsafe fn destroy(&self, gl: &glow::Context) {
        gl.delete_framebuffer(self.framebuffer);
        gl.delete_renderbuffer(self.renderbuffer);
    }
}

// ─── Softness (post-process blur) ───────────────────────────────────

/// Texture-backed sRGB FBO used as ping-pong target for the blur pass.
/// Sampling reads auto-linearise; writes auto-encode (`GL_FRAMEBUFFER_SRGB`).
struct GlBlurTarget {
    framebuffer: glow::Framebuffer,
    texture: glow::Texture,
    width: u32,
    height: u32,
}

impl GlBlurTarget {
    unsafe fn new(gl: &glow::Context, width: u32, height: u32) -> crate::Result<Self> {
        let w = width.max(1);
        let h = height.max(1);
        let texture = gl
            .create_texture()
            .map_err(|e| crate::GpuError::ResourceCreate(format!("blur tex: {e}")))?;
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::SRGB8_ALPHA8 as i32,
            w as i32,
            h as i32,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(None),
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.bind_texture(glow::TEXTURE_2D, None);

        let framebuffer = gl
            .create_framebuffer()
            .map_err(|e| crate::GpuError::ResourceCreate(format!("blur FBO: {e}")))?;
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(texture),
            0,
        );
        let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        if status != glow::FRAMEBUFFER_COMPLETE {
            gl.delete_framebuffer(framebuffer);
            gl.delete_texture(texture);
            return Err(crate::GpuError::ResourceCreate(format!(
                "blur FBO incomplete: status 0x{status:04X}"
            )));
        }

        Ok(GlBlurTarget {
            framebuffer,
            texture,
            width: w,
            height: h,
        })
    }

    unsafe fn resize(&mut self, gl: &glow::Context, width: u32, height: u32) {
        let w = width.max(1);
        let h = height.max(1);
        if w == self.width && h == self.height {
            return;
        }
        gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::SRGB8_ALPHA8 as i32,
            w as i32,
            h as i32,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(None),
        );
        gl.bind_texture(glow::TEXTURE_2D, None);
        self.width = w;
        self.height = h;
    }

    unsafe fn destroy(&self, gl: &glow::Context) {
        gl.delete_framebuffer(self.framebuffer);
        gl.delete_texture(self.texture);
    }
}

/// Separable Gaussian blur pipeline. Runs twice per frame (horizontal + vertical)
/// when softness > 0. Strength controls the per-axis tap offset in texels
/// (0 = identity, up to ~2 texels at strength=1).
struct GlBlurPipeline {
    program: glow::Program,
    vao: glow::VertexArray,
    loc_tex: glow::UniformLocation,
    loc_direction: glow::UniformLocation,
    loc_strength: glow::UniformLocation,
}

impl GlBlurPipeline {
    unsafe fn new(gl: &glow::Context) -> crate::Result<Self> {
        let program = compile_program(gl, BLUR_VS, BLUR_FS, "blur")?;
        let loc_tex = gl
            .get_uniform_location(program, "u_tex")
            .ok_or_else(|| crate::GpuError::ShaderCompile("u_tex uniform not found".into()))?;
        let loc_direction = gl
            .get_uniform_location(program, "u_direction")
            .ok_or_else(|| {
                crate::GpuError::ShaderCompile("u_direction uniform not found".into())
            })?;
        let loc_strength = gl
            .get_uniform_location(program, "u_strength")
            .ok_or_else(|| crate::GpuError::ShaderCompile("u_strength uniform not found".into()))?;
        let vao = gl
            .create_vertex_array()
            .map_err(|e| crate::GpuError::ResourceCreate(format!("blur VAO: {e}")))?;
        Ok(GlBlurPipeline {
            program,
            vao,
            loc_tex,
            loc_direction,
            loc_strength,
        })
    }

    unsafe fn pass(
        &self,
        gl: &glow::Context,
        src_texture: glow::Texture,
        dst_framebuffer: glow::Framebuffer,
        dst_width: u32,
        dst_height: u32,
        direction: (f32, f32),
        strength: f32,
    ) {
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(dst_framebuffer));
        gl.viewport(0, 0, dst_width as i32, dst_height as i32);
        gl.disable(glow::BLEND);
        gl.use_program(Some(self.program));
        gl.bind_vertex_array(Some(self.vao));
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(src_texture));
        gl.uniform_1_i32(Some(&self.loc_tex), 0);
        // Direction is expressed in texel units (1.0 = one texel in the axis).
        // The shader divides by textureSize to get normalized UV offsets.
        gl.uniform_2_f32(Some(&self.loc_direction), direction.0, direction.1);
        gl.uniform_1_f32(Some(&self.loc_strength), strength);
        gl.draw_arrays(glow::TRIANGLES, 0, 3);
        gl.bind_texture(glow::TEXTURE_2D, None);
        gl.bind_vertex_array(None);
        gl.enable(glow::BLEND);
    }

    unsafe fn destroy(&self, gl: &glow::Context) {
        gl.delete_program(self.program);
        gl.delete_vertex_array(self.vao);
    }
}

// ─── GL atlas layer ─────────────────────────────────────────────────

struct GlAtlasLayer {
    texture: glow::Texture,
    program: glow::Program,
    vao: glow::VertexArray,
    instance_vbo: glow::Buffer,
    atlas_size: u32,
    bpp: u32,
    loc_viewport: glow::UniformLocation,
    loc_atlas: glow::UniformLocation,
    loc_pane_origin: glow::UniformLocation,
    loc_pane_size: glow::UniformLocation,
    loc_pane_radii: glow::UniformLocation,
    loc_use_linear_blending: Option<glow::UniformLocation>,
    loc_use_linear_correction: Option<glow::UniformLocation>,
    max_instances: usize,
}

struct GlAtlasLayerConfig<'a> {
    atlas_size: u32,
    max_instances: usize,
    internal_format: u32,
    format: u32,
    vs_src: &'a str,
    fs_src: &'a str,
    bpp: u32,
    label: &'a str,
}

impl GlAtlasLayer {
    unsafe fn new(gl: &glow::Context, cfg: &GlAtlasLayerConfig<'_>) -> crate::Result<Self> {
        let atlas_size = cfg.atlas_size;
        let max_instances = cfg.max_instances;
        let internal_format = cfg.internal_format;
        let format = cfg.format;
        let bpp = cfg.bpp;
        let program = compile_program(gl, cfg.vs_src, cfg.fs_src, cfg.label)?;
        let loc_viewport = gl
            .get_uniform_location(program, "u_viewport")
            .ok_or_else(|| crate::GpuError::ShaderCompile("u_viewport uniform not found".into()))?;
        let loc_atlas = gl
            .get_uniform_location(program, "u_atlas")
            .ok_or_else(|| crate::GpuError::ShaderCompile("u_atlas uniform not found".into()))?;
        let loc_pane_origin = gl
            .get_uniform_location(program, "u_pane_origin")
            .ok_or_else(|| {
                crate::GpuError::ShaderCompile(format!(
                    "u_pane_origin uniform not found in {} shader",
                    cfg.label
                ))
            })?;
        let loc_pane_size = gl
            .get_uniform_location(program, "u_pane_size")
            .ok_or_else(|| {
                crate::GpuError::ShaderCompile(format!(
                    "u_pane_size uniform not found in {} shader",
                    cfg.label
                ))
            })?;
        let loc_pane_radii = gl
            .get_uniform_location(program, "u_pane_radii")
            .ok_or_else(|| {
                crate::GpuError::ShaderCompile(format!(
                    "u_pane_radii uniform not found in {} shader",
                    cfg.label
                ))
            })?;
        let loc_use_linear_blending = gl.get_uniform_location(program, "u_use_linear_blending");
        let loc_use_linear_correction = gl.get_uniform_location(program, "u_use_linear_correction");
        gl.use_program(Some(program));
        gl.uniform_2_f32(Some(&loc_pane_origin), 0.0, 0.0);
        gl.uniform_2_f32(Some(&loc_pane_size), 0.0, 0.0);
        gl.uniform_4_f32(Some(&loc_pane_radii), 0.0, 0.0, 0.0, 0.0);
        gl.use_program(None);

        let texture = gl
            .create_texture()
            .map_err(|e| crate::GpuError::ResourceCreate(format!("texture: {e}")))?;
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            internal_format as i32,
            atlas_size as i32,
            atlas_size as i32,
            0,
            format,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(Some(&vec![
                0u8;
                (atlas_size * atlas_size * bpp) as usize
            ])),
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.bind_texture(glow::TEXTURE_2D, None);

        let vao = gl
            .create_vertex_array()
            .map_err(|e| crate::GpuError::ResourceCreate(format!("VAO: {e}")))?;
        let instance_vbo = gl
            .create_buffer()
            .map_err(|e| crate::GpuError::ResourceCreate(format!("VBO: {e}")))?;

        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(instance_vbo));
        gl.buffer_data_size(
            glow::ARRAY_BUFFER,
            (max_instances * std::mem::size_of::<GlyphInstance>()) as i32,
            glow::DYNAMIC_DRAW,
        );
        setup_glyph_vertex_attribs(gl);
        gl.bind_vertex_array(None);

        Ok(GlAtlasLayer {
            texture,
            program,
            vao,
            instance_vbo,
            atlas_size,
            bpp,
            loc_viewport,
            loc_atlas,
            loc_pane_origin,
            loc_pane_size,
            loc_pane_radii,
            loc_use_linear_blending,
            loc_use_linear_correction,
            max_instances,
        })
    }

    unsafe fn flush_uploads(
        &self,
        gl: &glow::Context,
        pending: &mut Vec<PendingUpload>,
        pending_clear: bool,
    ) {
        gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));

        if pending_clear {
            let zeros = vec![0u8; (self.atlas_size * self.atlas_size * self.bpp) as usize];
            let format = if self.bpp == 1 { glow::RED } else { glow::RGBA };
            gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                0,
                0,
                self.atlas_size as i32,
                self.atlas_size as i32,
                format,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&zeros)),
            );
        }

        let format = if self.bpp == 1 { glow::RED } else { glow::RGBA };

        for upload in pending.drain(..) {
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                upload.x as i32,
                upload.y as i32,
                upload.w as i32,
                upload.h as i32,
                format,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(upload.data.as_slice())),
            );
        }

        gl.bind_texture(glow::TEXTURE_2D, None);
    }

    /// Upload glyph instances to the GPU. Called once per frame per layer.
    unsafe fn upload_instances(
        &self,
        gl: &glow::Context,
        instances: &[GlyphInstance],
        vp: &crate::ViewportDims,
    ) {
        if instances.is_empty() {
            return;
        }
        let count = instances.len().min(self.max_instances);

        gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.instance_vbo));
        let data = bytemuck::cast_slice(&instances[..count]);
        gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, data);

        // Update viewport uniform (shared across all subsequent draw_batches calls)
        gl.use_program(Some(self.program));
        gl.uniform_2_f32(Some(&self.loc_viewport), vp.width, vp.height);
        gl.use_program(None);
    }

    /// Bind pipeline state and draw scissored glyph batches.
    /// Can be called multiple times after a single `upload_instances`.
    unsafe fn draw_batches(
        &self,
        gl: &glow::Context,
        instance_count: usize,
        vp: &crate::ViewportDims,
        batches: &[PaneGlyphRange],
        use_linear_blending: bool,
        use_linear_correction: bool,
    ) {
        if instance_count == 0 || batches.is_empty() {
            return;
        }
        let count = instance_count.min(self.max_instances);

        // Bind pipeline state (may have been changed by rect draws between calls)
        gl.use_program(Some(self.program));
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
        gl.uniform_1_i32(Some(&self.loc_atlas), 0);
        if let Some(ref loc) = self.loc_use_linear_blending {
            gl.uniform_1_i32(Some(loc), use_linear_blending as i32);
        }
        if let Some(ref loc) = self.loc_use_linear_correction {
            gl.uniform_1_i32(Some(loc), use_linear_correction as i32);
        }
        gl.bind_vertex_array(Some(self.vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.instance_vbo));

        gl.enable(glow::SCISSOR_TEST);

        for batch in batches {
            let start = (batch.start as usize).min(count);
            let end = start.saturating_add(batch.count as usize).min(count);
            let (x, y, w, h) = batch.scissor;
            if start >= end || w == 0 || h == 0 {
                continue;
            }
            let sy = vp.height_px.saturating_sub(y + h);
            gl.scissor(x as i32, sy as i32, w as i32, h as i32);
            gl.uniform_2_f32(
                Some(&self.loc_pane_origin),
                batch.pane_origin[0],
                batch.pane_origin[1],
            );
            gl.uniform_2_f32(
                Some(&self.loc_pane_size),
                batch.pane_size[0],
                batch.pane_size[1],
            );
            gl.uniform_4_f32(
                Some(&self.loc_pane_radii),
                batch.pane_radii[0],
                batch.pane_radii[1],
                batch.pane_radii[2],
                batch.pane_radii[3],
            );

            let base_offset = start * std::mem::size_of::<GlyphInstance>();
            setup_glyph_vertex_attribs_offset(gl, base_offset as i32);

            gl.draw_arrays_instanced(glow::TRIANGLE_STRIP, 0, 4, (end - start) as i32);
        }

        gl.disable(glow::SCISSOR_TEST);
        gl.bind_vertex_array(None);
        gl.use_program(None);
    }

    unsafe fn destroy(&self, gl: &glow::Context) {
        gl.delete_texture(self.texture);
        gl.delete_program(self.program);
        gl.delete_vertex_array(self.vao);
        gl.delete_buffer(self.instance_vbo);
    }
}

// ─── GL rect pipeline ───────────────────────────────────────────────

struct GlRectPipeline {
    program: glow::Program,
    vao: glow::VertexArray,
    instance_vbo: glow::Buffer,
    loc_viewport: glow::UniformLocation,
    loc_pane_origin: glow::UniformLocation,
    loc_pane_size: glow::UniformLocation,
    loc_pane_radii: glow::UniformLocation,
    loc_use_linear_blending: Option<glow::UniformLocation>,
    max_rects: usize,
}

impl GlRectPipeline {
    unsafe fn new(gl: &glow::Context, max_rects: usize) -> crate::Result<Self> {
        let rect_fs = RECT_FS
            .replace("// COLOR_FUNCS_PLACEHOLDER", GLSL_COLOR_FUNCS)
            .replace("// CORNER_FUNCS_PLACEHOLDER", GLSL_CORNER_FUNCS);
        debug_assert!(
            !rect_fs.contains("PLACEHOLDER"),
            "shader source still contains unfilled placeholder after all replacements"
        );
        let program = compile_program(gl, RECT_VS, &rect_fs, "rect")?;
        let loc_viewport = gl
            .get_uniform_location(program, "u_viewport")
            .ok_or_else(|| {
                crate::GpuError::ShaderCompile("u_viewport uniform not found in rect shader".into())
            })?;
        let loc_use_linear_blending = gl.get_uniform_location(program, "u_use_linear_blending");
        let loc_pane_origin = gl
            .get_uniform_location(program, "u_pane_origin")
            .ok_or_else(|| {
                crate::GpuError::ShaderCompile(
                    "u_pane_origin uniform not found in rect shader".into(),
                )
            })?;
        let loc_pane_size = gl
            .get_uniform_location(program, "u_pane_size")
            .ok_or_else(|| {
                crate::GpuError::ShaderCompile(
                    "u_pane_size uniform not found in rect shader".into(),
                )
            })?;
        let loc_pane_radii = gl
            .get_uniform_location(program, "u_pane_radii")
            .ok_or_else(|| {
                crate::GpuError::ShaderCompile(
                    "u_pane_radii uniform not found in rect shader".into(),
                )
            })?;
        gl.use_program(Some(program));
        gl.uniform_2_f32(Some(&loc_pane_origin), 0.0, 0.0);
        gl.uniform_2_f32(Some(&loc_pane_size), 0.0, 0.0);
        gl.uniform_4_f32(Some(&loc_pane_radii), 0.0, 0.0, 0.0, 0.0);
        gl.use_program(None);

        let vao = gl
            .create_vertex_array()
            .map_err(|e| crate::GpuError::ResourceCreate(format!("rect VAO: {e}")))?;
        let instance_vbo = gl
            .create_buffer()
            .map_err(|e| crate::GpuError::ResourceCreate(format!("rect VBO: {e}")))?;

        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(instance_vbo));
        gl.buffer_data_size(
            glow::ARRAY_BUFFER,
            (max_rects * std::mem::size_of::<Rect>()) as i32,
            glow::DYNAMIC_DRAW,
        );

        let stride = std::mem::size_of::<Rect>() as i32;
        // pos (x, y)
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, stride, 0);
        gl.vertex_attrib_divisor(0, 1);
        // size (w, h)
        gl.enable_vertex_attrib_array(1);
        gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, stride, 8);
        gl.vertex_attrib_divisor(1, 1);
        // color (r, g, b, a)
        gl.enable_vertex_attrib_array(2);
        gl.vertex_attrib_pointer_f32(2, 4, glow::FLOAT, false, stride, 16);
        gl.vertex_attrib_divisor(2, 1);

        gl.bind_vertex_array(None);

        Ok(GlRectPipeline {
            program,
            vao,
            instance_vbo,
            loc_viewport,
            loc_pane_origin,
            loc_pane_size,
            loc_pane_radii,
            loc_use_linear_blending,
            max_rects,
        })
    }

    /// Grow the instance VBO if `needed` exceeds current capacity. Reuses the
    /// same buffer object name so the VAO's vertex attribute pointers remain
    /// valid; `glBufferData` orphans the old storage cleanly.
    unsafe fn ensure_capacity(&mut self, gl: &glow::Context, needed: usize) {
        if needed <= self.max_rects {
            return;
        }
        let new_cap = needed
            .next_power_of_two()
            .max(self.max_rects.saturating_mul(2));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.instance_vbo));
        gl.buffer_data_size(
            glow::ARRAY_BUFFER,
            (new_cap * std::mem::size_of::<Rect>()) as i32,
            glow::DYNAMIC_DRAW,
        );
        gl.bind_buffer(glow::ARRAY_BUFFER, None);
        self.max_rects = new_cap;
    }

    /// Upload all rect instance data to the GPU buffer.
    unsafe fn upload(&self, gl: &glow::Context, rects: &[Rect], viewport_w: f32, viewport_h: f32) {
        if rects.is_empty() {
            return;
        }
        let count = rects.len().min(self.max_rects);
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.instance_vbo));
        let data = bytemuck::cast_slice(&rects[..count]);
        gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, data);
        gl.bind_buffer(glow::ARRAY_BUFFER, None);
        // Store viewport for draw_range calls
        let _ = (viewport_w, viewport_h);
    }

    /// Draw previously uploaded rects grouped by pane clipping uniforms.
    unsafe fn draw_ranges(
        &self,
        gl: &glow::Context,
        ranges: &[PaneRectRange],
        viewport_w: f32,
        viewport_h: f32,
        use_linear_blending: bool,
    ) {
        if ranges.is_empty() {
            return;
        }
        let any_non_empty = ranges.iter().any(|range| range.count > 0);
        if !any_non_empty {
            return;
        }
        let stride = std::mem::size_of::<Rect>() as i32;
        gl.use_program(Some(self.program));
        gl.uniform_2_f32(Some(&self.loc_viewport), viewport_w, viewport_h);
        if let Some(ref loc) = self.loc_use_linear_blending {
            gl.uniform_1_i32(Some(loc), use_linear_blending as i32);
        }
        gl.bind_vertex_array(Some(self.vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.instance_vbo));

        for range in ranges {
            let start = range.start as usize;
            let count = range.count as usize;
            if count == 0 || start >= self.max_rects {
                continue;
            }
            let count = count.min(self.max_rects - start);
            gl.uniform_2_f32(
                Some(&self.loc_pane_origin),
                range.pane_origin[0],
                range.pane_origin[1],
            );
            gl.uniform_2_f32(
                Some(&self.loc_pane_size),
                range.pane_size[0],
                range.pane_size[1],
            );
            gl.uniform_4_f32(
                Some(&self.loc_pane_radii),
                range.pane_radii[0],
                range.pane_radii[1],
                range.pane_radii[2],
                range.pane_radii[3],
            );
            let base = (start * std::mem::size_of::<Rect>()) as i32;
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, stride, base);
            gl.vertex_attrib_divisor(0, 1);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, stride, base + 8);
            gl.vertex_attrib_divisor(1, 1);
            gl.enable_vertex_attrib_array(2);
            gl.vertex_attrib_pointer_f32(2, 4, glow::FLOAT, false, stride, base + 16);
            gl.vertex_attrib_divisor(2, 1);

            gl.draw_arrays_instanced(glow::TRIANGLE_STRIP, 0, 4, count as i32);
        }

        gl.bind_vertex_array(None);
        gl.use_program(None);
    }

    unsafe fn destroy(&self, gl: &glow::Context) {
        gl.delete_program(self.program);
        gl.delete_vertex_array(self.vao);
        gl.delete_buffer(self.instance_vbo);
    }
}

// ─── GL SDF rect pipeline ───────────────────────────────────────────
//
// Mirrors blade's `SdfPipeline`: rounded corners + optional border + optional
// shadow, instanced with one `SdfRect` per quad. Drawn after flat overlay
// backgrounds and before overlay glyphs so chrome (palette / context menu)
// sits on top of pane text but the labels stay crisp on top of the panel.

struct GlSdfPipeline {
    program: glow::Program,
    vao: glow::VertexArray,
    instance_vbo: glow::Buffer,
    loc_viewport: glow::UniformLocation,
    loc_use_linear_blending: Option<glow::UniformLocation>,
    max_rects: usize,
}

impl GlSdfPipeline {
    unsafe fn new(gl: &glow::Context, max_rects: usize) -> crate::Result<Self> {
        let sdf_fs = SDF_FS.replace("// COLOR_FUNCS_PLACEHOLDER", GLSL_COLOR_FUNCS);
        debug_assert!(
            !sdf_fs.contains("PLACEHOLDER"),
            "shader source still contains unfilled placeholder after all replacements"
        );
        let program = compile_program(gl, SDF_VS, &sdf_fs, "sdf")?;
        let loc_viewport = gl
            .get_uniform_location(program, "u_viewport")
            .ok_or_else(|| {
                crate::GpuError::ShaderCompile("u_viewport uniform not found in sdf shader".into())
            })?;
        let loc_use_linear_blending = gl.get_uniform_location(program, "u_use_linear_blending");

        let vao = gl
            .create_vertex_array()
            .map_err(|e| crate::GpuError::ResourceCreate(format!("sdf VAO: {e}")))?;
        let instance_vbo = gl
            .create_buffer()
            .map_err(|e| crate::GpuError::ResourceCreate(format!("sdf VBO: {e}")))?;

        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(instance_vbo));
        gl.buffer_data_size(
            glow::ARRAY_BUFFER,
            (max_rects * SdfRect::SIZE) as i32,
            glow::DYNAMIC_DRAW,
        );
        setup_sdf_vertex_attribs(gl, 0);
        gl.bind_vertex_array(None);

        Ok(GlSdfPipeline {
            program,
            vao,
            instance_vbo,
            loc_viewport,
            loc_use_linear_blending,
            max_rects,
        })
    }

    unsafe fn upload(&self, gl: &glow::Context, rects: &[SdfRect]) {
        if rects.is_empty() {
            return;
        }
        let count = rects.len().min(self.max_rects);
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.instance_vbo));
        let data = bytemuck::cast_slice(&rects[..count]);
        gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, data);
        gl.bind_buffer(glow::ARRAY_BUFFER, None);
    }

    unsafe fn draw(
        &self,
        gl: &glow::Context,
        count: usize,
        viewport_w: f32,
        viewport_h: f32,
        use_linear_blending: bool,
    ) {
        if count == 0 {
            return;
        }
        let count = count.min(self.max_rects);
        gl.use_program(Some(self.program));
        gl.uniform_2_f32(Some(&self.loc_viewport), viewport_w, viewport_h);
        if let Some(ref loc) = self.loc_use_linear_blending {
            gl.uniform_1_i32(Some(loc), use_linear_blending as i32);
        }
        gl.bind_vertex_array(Some(self.vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.instance_vbo));
        setup_sdf_vertex_attribs(gl, 0);

        gl.draw_arrays_instanced(glow::TRIANGLE_STRIP, 0, 4, count as i32);

        gl.bind_vertex_array(None);
        gl.use_program(None);
    }

    unsafe fn destroy(&self, gl: &glow::Context) {
        gl.delete_program(self.program);
        gl.delete_vertex_array(self.vao);
        gl.delete_buffer(self.instance_vbo);
    }
}

// ─── GL background image pipeline ───────────────────────────────────
//
// One vertexless fullscreen quad textured with the user-supplied image.
// Mirrors the DX `DxBackgroundImagePipeline`: cover-fit UV math, premult-alpha
// output, draws after the clear and before pane bgs. The `image` crate's
// row-major top-down RGBA8 layout combined with GL's lower-left texture
// origin and the existing `ndc.y = 1 - py/vh*2` Y-flip in the renderer
// cancel out — screen-top samples image-top without a manual flip.

struct GlBackgroundImageTexture {
    texture: glow::Texture,
    width: u32,
    height: u32,
}

struct GlBackgroundImagePipeline {
    program: glow::Program,
    /// Empty VAO bound during draw — core-profile GL requires a VAO be
    /// bound for any draw call, even with vertexless shaders.
    vao: glow::VertexArray,
    loc_viewport_tex: glow::UniformLocation,
    loc_params: glow::UniformLocation,
    loc_tex: glow::UniformLocation,
    texture: Option<GlBackgroundImageTexture>,
}

impl GlBackgroundImagePipeline {
    unsafe fn new(gl: &glow::Context) -> crate::Result<Self> {
        let fs_src = BACKGROUND_IMAGE_FS.replace("// COLOR_FUNCS_PLACEHOLDER", GLSL_COLOR_FUNCS);
        debug_assert!(
            !fs_src.contains("PLACEHOLDER"),
            "overview-bg shader source still contains unfilled placeholder"
        );
        let program = compile_program(gl, BACKGROUND_IMAGE_VS, &fs_src, "background_image")?;
        let loc_viewport_tex = gl
            .get_uniform_location(program, "u_viewport_tex")
            .ok_or_else(|| {
                crate::GpuError::ShaderCompile(
                    "u_viewport_tex uniform not found in background_image shader".into(),
                )
            })?;
        let loc_params = gl
            .get_uniform_location(program, "u_params")
            .ok_or_else(|| {
                crate::GpuError::ShaderCompile(
                    "u_params uniform not found in background_image shader".into(),
                )
            })?;
        let loc_tex = gl.get_uniform_location(program, "u_tex").ok_or_else(|| {
            crate::GpuError::ShaderCompile("u_tex uniform not found in background_image shader".into())
        })?;

        let vao = gl
            .create_vertex_array()
            .map_err(|e| crate::GpuError::ResourceCreate(format!("background_image VAO: {e}")))?;

        Ok(GlBackgroundImagePipeline {
            program,
            vao,
            loc_viewport_tex,
            loc_params,
            loc_tex,
            texture: None,
        })
    }

    unsafe fn upload(
        &mut self,
        gl: &glow::Context,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> crate::Result<()> {
        if width == 0 || height == 0 {
            return Err(crate::GpuError::ResourceCreate(format!(
                "overview bg image has zero dimension ({width}x{height})"
            )));
        }
        let expected = (width as usize) * (height as usize) * 4;
        if rgba.len() != expected {
            return Err(crate::GpuError::ResourceCreate(format!(
                "overview bg image byte count mismatch: got {}, expected {} ({width}x{height} RGBA8)",
                rgba.len(),
                expected
            )));
        }
        if let Some(prev) = self.texture.take() {
            gl.delete_texture(prev.texture);
        }
        let texture = gl
            .create_texture()
            .map_err(|e| crate::GpuError::ResourceCreate(format!("background_image texture: {e}")))?;
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        // Non-sRGB internal format — the `BACKGROUND_IMAGE_FS` linearises
        // explicitly when `use_linear_blending` is on. The color glyph
        // atlas (`GlAtlasLayer` in this file) takes the other path
        // (`SRGB8_ALPHA8` + hardware auto-linearise), but the wallpaper
        // routes through the `RECT_FS` convention because both
        // `linear` and `native` modes need a path through the same
        // shader and toggling the texture's internal format per-frame
        // isn't possible in GL. Either path produces the correct
        // round-trip; the divergence from the color atlas is
        // intentional, not an oversight.
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA8 as i32,
            width as i32,
            height as i32,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(Some(rgba)),
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.bind_texture(glow::TEXTURE_2D, None);
        self.texture = Some(GlBackgroundImageTexture {
            texture,
            width,
            height,
        });
        Ok(())
    }

    unsafe fn clear(&mut self, gl: &glow::Context) {
        if let Some(prev) = self.texture.take() {
            gl.delete_texture(prev.texture);
        }
    }

    unsafe fn destroy(&self, gl: &glow::Context) {
        if let Some(prev) = self.texture.as_ref() {
            gl.delete_texture(prev.texture);
        }
        gl.delete_program(self.program);
        gl.delete_vertex_array(self.vao);
    }

    /// Draw the wallpaper if a texture is bound and `opacity > 0`. Returns
    /// `true` iff a draw was actually issued — callers use that to suppress
    /// the prepended baseline rect that would otherwise erase the image.
    /// Caller must have set the framebuffer / blend state already; this
    /// rebinds program + VAO + texture only.
    unsafe fn draw(
        &self,
        gl: &glow::Context,
        vw: f32,
        vh: f32,
        opacity: f32,
        use_linear_blending: bool,
    ) -> bool {
        let Some(tex) = self.texture.as_ref() else {
            return false;
        };
        if opacity <= 0.0 {
            return false;
        }
        gl.use_program(Some(self.program));
        gl.uniform_4_f32(
            Some(&self.loc_viewport_tex),
            vw,
            vh,
            tex.width as f32,
            tex.height as f32,
        );
        gl.uniform_4_f32(
            Some(&self.loc_params),
            opacity,
            if use_linear_blending { 1.0 } else { 0.0 },
            0.0,
            0.0,
        );
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(tex.texture));
        gl.uniform_1_i32(Some(&self.loc_tex), 0);
        gl.bind_vertex_array(Some(self.vao));
        gl.draw_arrays(glow::TRIANGLES, 0, 6);
        gl.bind_vertex_array(None);
        gl.bind_texture(glow::TEXTURE_2D, None);
        gl.use_program(None);
        true
    }
}

// ─── GlyphAtlasGpu ─────────────────────────────────────────────────

pub struct GlyphAtlasGpu {
    alpha: GlAtlasLayer,
    color: GlAtlasLayer,
}

impl GlyphAtlasGpu {
    unsafe fn new(
        gl: &glow::Context,
        atlas_size: u32,
        max_instances: usize,
    ) -> crate::Result<Self> {
        // Inject shared color functions into fragment shaders.
        let alpha_fs = ALPHA_FS
            .replace("// COLOR_FUNCS_PLACEHOLDER", GLSL_COLOR_FUNCS)
            .replace("// CORNER_FUNCS_PLACEHOLDER", GLSL_CORNER_FUNCS);
        let color_fs = COLOR_FS
            .replace("// COLOR_FUNCS_PLACEHOLDER", GLSL_COLOR_FUNCS)
            .replace("// CORNER_FUNCS_PLACEHOLDER", GLSL_CORNER_FUNCS);
        debug_assert!(
            !alpha_fs.contains("PLACEHOLDER"),
            "shader source still contains unfilled placeholder after all replacements"
        );
        debug_assert!(
            !color_fs.contains("PLACEHOLDER"),
            "shader source still contains unfilled placeholder after all replacements"
        );

        let alpha = GlAtlasLayer::new(
            gl,
            &GlAtlasLayerConfig {
                atlas_size,
                max_instances,
                internal_format: glow::R8,
                format: glow::RED,
                vs_src: GLYPH_VS,
                fs_src: &alpha_fs,
                bpp: 1,
                label: "alpha_atlas",
            },
        )?;
        // Use SRGB8_ALPHA8 for the color atlas so the GPU auto-linearizes
        // on texture sample. This prevents double gamma when writing to
        // the sRGB FBO. In native mode the shader unlinearizes before output.
        let color = GlAtlasLayer::new(
            gl,
            &GlAtlasLayerConfig {
                atlas_size,
                max_instances,
                internal_format: glow::SRGB8_ALPHA8,
                format: glow::RGBA,
                vs_src: GLYPH_VS,
                fs_src: &color_fs,
                bpp: 4,
                label: "color_atlas",
            },
        )?;
        Ok(GlyphAtlasGpu { alpha, color })
    }
}

// ─── Renderer ───────────────────────────────────────────────────────

pub struct Renderer {
    gl: glow::Context,
    gl_surface: glutin::surface::Surface<WindowSurface>,
    gl_context: PossiblyCurrentContext,
    rects: GlRectPipeline,
    sdf: GlSdfPipeline,
    background_image: GlBackgroundImagePipeline,
    width: u32,
    height: u32,
    use_linear_blending: bool,
    use_linear_correction: bool,
    /// sRGB FBO for linear-correct blending. `None` in native mode.
    srgb_target: Option<GlSrgbTarget>,
    /// Post-process softness pass. Populated when
    /// `render.softness > 0` and the sRGB FBO is available.
    softness: f32,
    blur: Option<GlBlurResources>,
}

struct GlBlurResources {
    pipeline: GlBlurPipeline,
    /// Horizontal-pass target: written after the H pass, read by the V pass.
    ping: GlBlurTarget,
    /// Vertical-pass target: written after the V pass, blitted to the screen.
    pong: GlBlurTarget,
}

impl Renderer {
    pub fn new(window: Arc<Window>, _render_config: &RenderConfig) -> crate::Result<Self> {
        #[cfg(target_os = "macos")]
        {
            // macOS deprecated OpenGL; entire GL backend is unavailable
            let _ = window;
            Err(crate::GpuError::DeviceInit(
                "GL backend is not supported on macOS (OpenGL is deprecated). \
                 Use blade (Metal) instead: backend = \"blade\""
                    .into(),
            ))
        }

        #[cfg(not(target_os = "macos"))]
        Self::new_impl(window, _render_config)
    }

    #[cfg(not(target_os = "macos"))]
    fn new_impl(window: Arc<Window>, render_config: &RenderConfig) -> crate::Result<Self> {
        let size = window.inner_size();

        // Build glutin display from existing window
        let raw_window_handle = window
            .window_handle()
            .map_err(|e| anyhow::anyhow!("window handle error: {e}"))?
            .as_raw();
        let raw_display_handle = window
            .display_handle()
            .map_err(|e| anyhow::anyhow!("display handle error: {e}"))?
            .as_raw();

        let display_api_preference = glutin::display::DisplayApiPreference::Egl;

        let display = unsafe {
            glutin::display::Display::new(raw_display_handle, display_api_preference)
                .map_err(|e| anyhow::anyhow!("GL display creation failed: {e}"))?
        };

        let config_template = ConfigTemplateBuilder::new()
            .with_alpha_size(8)
            .with_transparency(false)
            .build();

        let config = unsafe {
            display
                .find_configs(config_template)
                .map_err(|e| anyhow::anyhow!("EGL config search failed: {e}"))?
                .next()
                .ok_or_else(|| anyhow::anyhow!("no suitable EGL config found"))?
        };

        let context_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
            .build(Some(raw_window_handle));

        let gl_context = unsafe {
            display
                .create_context(&config, &context_attrs)
                .map_err(|e| anyhow::anyhow!("GL context creation failed: {e}"))?
        };

        let surface_attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
            raw_window_handle,
            NonZeroU32::new(size.width.max(1)).unwrap(),
            NonZeroU32::new(size.height.max(1)).unwrap(),
        );

        let gl_surface = unsafe {
            display
                .create_window_surface(&config, &surface_attrs)
                .map_err(|e| anyhow::anyhow!("EGL surface creation failed: {e}"))?
        };

        let gl_context = gl_context
            .make_current(&gl_surface)
            .map_err(|e| anyhow::anyhow!("make current failed: {e}"))?;

        // Set swap interval based on present mode
        let interval = match render_config.present_mode {
            ciri_config::config::PresentMode::Immediate
            | ciri_config::config::PresentMode::Mailbox => SwapInterval::DontWait,
            ciri_config::config::PresentMode::Fifo => {
                SwapInterval::Wait(NonZeroU32::new(1).unwrap())
            }
        };
        let _ = gl_surface.set_swap_interval(&gl_context, interval);

        let gl = unsafe {
            glow::Context::from_loader_function_cstr(|name| {
                display.get_proc_address(name) as *const _
            })
        };

        let renderer_name = unsafe { gl.get_parameter_string(glow::RENDERER) };
        let version_str = unsafe { gl.get_parameter_string(glow::VERSION) };
        log::info!("GL renderer: {renderer_name} ({version_str})");

        unsafe {
            gl.enable(glow::BLEND);
            gl.blend_func_separate(
                glow::ONE,
                glow::ONE_MINUS_SRC_ALPHA,
                glow::ONE,
                glow::ONE_MINUS_SRC_ALPHA,
            );
            gl.disable(glow::DEPTH_TEST);
            // GL_FRAMEBUFFER_SRGB is enabled later if linear blending is on
            // (after sRGB FBO creation). When rendering to a SRGB8_ALPHA8 FBO,
            // the GPU auto-linearises blending and auto-encodes writes.
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
        }

        let rects = unsafe { GlRectPipeline::new(&gl, render_config.max_rectangles)? };
        let sdf = unsafe { GlSdfPipeline::new(&gl, MAX_SDF_RECTS)? };
        let background_image = unsafe { GlBackgroundImagePipeline::new(&gl)? };

        let use_linear_blending = render_config.alpha_blending.is_linear();
        let use_linear_correction = render_config.alpha_blending.use_correction();

        // Create sRGB FBO for linear-correct blending.
        let (use_linear_blending, use_linear_correction, srgb_target) = if use_linear_blending {
            unsafe {
                gl.enable(glow::FRAMEBUFFER_SRGB);
            }
            match unsafe { GlSrgbTarget::new(&gl, size.width.max(1), size.height.max(1)) } {
                Ok(target) => {
                    log::info!("sRGB FBO created for linear blending");
                    (use_linear_blending, use_linear_correction, Some(target))
                }
                Err(e) => {
                    log::warn!("Failed to create sRGB FBO, falling back to native blending: {e}");
                    unsafe {
                        gl.disable(glow::FRAMEBUFFER_SRGB);
                    }
                    (false, false, None)
                }
            }
        } else {
            (use_linear_blending, use_linear_correction, None)
        };

        // Softness post-process: needs a sampleable sRGB texture, i.e. a
        // sRGB FBO must already exist (linear blending enabled).
        let softness = render_config.softness.clamp(0.0, 1.0);
        let blur = if softness > 0.0 && srgb_target.is_some() {
            let w = size.width.max(1);
            let h = size.height.max(1);
            match (unsafe { GlBlurTarget::new(&gl, w, h) }, unsafe {
                GlBlurTarget::new(&gl, w, h)
            }) {
                (Ok(ping), Ok(pong)) => match unsafe { GlBlurPipeline::new(&gl) } {
                    Ok(pipeline) => {
                        log::info!("softness pass enabled: strength={softness:.2}");
                        Some(GlBlurResources {
                            pipeline,
                            ping,
                            pong,
                        })
                    }
                    Err(e) => {
                        log::warn!("softness pipeline init failed, disabling: {e}");
                        unsafe {
                            ping.destroy(&gl);
                            pong.destroy(&gl);
                        }
                        None
                    }
                },
                (ping_res, pong_res) => {
                    log::warn!("softness FBO init failed, disabling");
                    if let Ok(t) = ping_res {
                        unsafe { t.destroy(&gl) }
                    }
                    if let Ok(t) = pong_res {
                        unsafe { t.destroy(&gl) }
                    }
                    None
                }
            }
        } else {
            None
        };

        Ok(Renderer {
            gl,
            gl_surface,
            gl_context,
            rects,
            sdf,
            background_image,
            width: size.width.max(1),
            height: size.height.max(1),
            use_linear_blending,
            use_linear_correction,
            srgb_target,
            softness,
            blur,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.width = width;
            self.height = height;
            // On EGL/Wayland, resize is implicit in eglSwapBuffers.
            // Just update the surface size for glutin.
            self.gl_surface.resize(
                &self.gl_context,
                NonZeroU32::new(width).unwrap(),
                NonZeroU32::new(height).unwrap(),
            );
        }
    }

    /// No-op for GL/EGL — resize is implicit in swap_buffers.
    pub fn apply_surface(&mut self) {}

    pub fn surface_size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Upload the background image RGBA8 texture.
    pub fn set_background_image(
        &mut self,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> crate::Result<()> {
        unsafe { self.background_image.upload(&self.gl, rgba, width, height) }
    }

    /// Drop the wallpaper texture, if any.
    pub fn clear_background_image(&mut self) {
        unsafe { self.background_image.clear(&self.gl) }
    }

    pub fn create_atlas(
        &mut self,
        params: &ciri_render::glyph_cache::FontInitParams,
    ) -> crate::Result<(GlyphCache, GlyphAtlasGpu)> {
        let cache = GlyphCache::new(params);
        let atlas_gpu =
            unsafe { GlyphAtlasGpu::new(&self.gl, cache.atlas_size, cache.max_instances)? };
        Ok((cache, atlas_gpu))
    }

    pub fn destroy_atlas(&self, atlas_gpu: &mut GlyphAtlasGpu) {
        unsafe {
            atlas_gpu.alpha.destroy(&self.gl);
            atlas_gpu.color.destroy(&self.gl);
        }
    }

    pub fn draw_frame(
        &mut self,
        atlas_gpu: &mut GlyphAtlasGpu,
        cache: &mut GlyphCache,
        scene: FrameScene,
    ) -> crate::Result<()> {
        let vw = self.width as f32;
        let vh = self.height as f32;
        let mut profiler = crate::DrawFrameProfiler::begin("gl");

        unsafe {
            let upload_start = std::time::Instant::now();
            self.gl
                .viewport(0, 0, self.width as i32, self.height as i32);

            // Resize sRGB FBO if needed, then bind it as render target.
            if let Some(ref mut target) = self.srgb_target {
                target.resize(&self.gl, self.width, self.height);
                self.gl
                    .bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
            }

            // Flush pending glyph uploads
            let (mut ap, mut cp, ac, cc) = cache.take_pending();
            atlas_gpu.alpha.flush_uploads(&self.gl, &mut ap, ac);
            atlas_gpu.color.flush_uploads(&self.gl, &mut cp, cc);

            // Clear — linearize when rendering to sRGB FBO so the GPU's
            // automatic sRGB encoding produces the correct sRGB value.
            let cc = if self.use_linear_blending {
                ciri_config::theme::ThemeConfig::srgb_to_linear(scene.clear_color)
            } else {
                scene.clear_color
            };
            self.gl.clear_color(cc[0], cc[1], cc[2], cc[3]);
            self.gl.clear(glow::COLOR_BUFFER_BIT);

            // Overview wallpaper (if any). Self-checks the texture/opacity
            // gate, so passing 0.0 is a no-op. Drawn after the clear and
            // before pane bgs so the image sits behind everything else.
            // Returns true iff a draw was actually issued.
            let wallpaper_drawn = self.background_image.draw(
                &self.gl,
                vw,
                vh,
                scene.background_image_opacity,
                self.use_linear_blending,
            );

            // 1. Upload all background rects (clear + pane + overlay) once.
            // Mirrors the DX path: the prepended baseline rect would erase
            // the wallpaper we just drew, so make it transparent while the
            // wallpaper is showing. The slot itself stays so the bg_rect
            // index math (active_bg_idx / overlay_bg_idx) still matches
            // `scene.bg_rect_ranges`.
            let baseline_color = if wallpaper_drawn {
                [0.0; 4]
            } else {
                scene.clear_color
            };
            let mut all_bg = Vec::with_capacity(1 + scene.bg_rects.len());
            all_bg.push(Rect {
                x: 0.0,
                y: 0.0,
                w: vw,
                h: vh,
                color: baseline_color,
            });
            all_bg.extend_from_slice(scene.bg_rects);
            let active_bg_idx = 1 + scene.active_bg_start; // +1 for clear rect
            let overlay_bg_idx = 1 + scene.overlay_bg_start; // +1 for clear rect
            self.rects.ensure_capacity(&self.gl, all_bg.len());
            let total_bg = all_bg.len().min(self.rects.max_rects);
            self.rects.upload(&self.gl, &all_bg, vw, vh);
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

            // Blending flags for the frame.
            let lb = self.use_linear_blending;
            let lc = self.use_linear_correction;

            // 2. Draw non-focused pane background rects.
            let inactive_bg_count = active_bg_idx.min(total_bg);
            let inactive_bg_ranges = split_ranges(&all_bg_ranges, 0, inactive_bg_count);
            self.rects
                .draw_ranges(&self.gl, &inactive_bg_ranges, vw, vh, lb);

            let vp = crate::ViewportDims {
                width: vw,
                height: vh,
                width_px: self.width,
                height_px: self.height,
            };

            // 3. Upload alpha + color glyph instances once.
            atlas_gpu
                .alpha
                .upload_instances(&self.gl, scene.glyphs, &vp);
            atlas_gpu
                .color
                .upload_instances(&self.gl, scene.color_glyphs, &vp);
            let alpha_count = scene.glyphs.len();
            let color_count = scene.color_glyphs.len();
            if let Some(profiler) = profiler.as_mut() {
                profiler.record_cpu_upload(upload_start);
            }

            // 4. Draw inactive pane glyphs (scissored).
            let draw_start = std::time::Instant::now();
            atlas_gpu
                .alpha
                .draw_batches(&self.gl, alpha_count, &vp, scene.glyph_batches, lb, lc);
            atlas_gpu.color.draw_batches(
                &self.gl,
                color_count,
                &vp,
                scene.color_glyph_batches,
                lb,
                lc,
            );

            // 5. Focused pane background rects.
            let active_bg_count = overlay_bg_idx.saturating_sub(active_bg_idx);
            if active_bg_count > 0 {
                let active_bg_ranges =
                    split_ranges(&all_bg_ranges, active_bg_idx, overlay_bg_idx.min(total_bg));
                self.rects
                    .draw_ranges(&self.gl, &active_bg_ranges, vw, vh, lb);
            }

            // 6. Draw active pane glyphs (scissored, no re-upload).
            atlas_gpu.alpha.draw_batches(
                &self.gl,
                alpha_count,
                &vp,
                scene.active_glyph_batches,
                lb,
                lc,
            );
            atlas_gpu.color.draw_batches(
                &self.gl,
                color_count,
                &vp,
                scene.active_color_glyph_batches,
                lb,
                lc,
            );

            // 7. Overlay background rects (rendered after pane glyphs so they
            //    occlude terminal text underneath popups like the context menu).
            let overlay_bg_count = total_bg.saturating_sub(overlay_bg_idx);
            if overlay_bg_count > 0 {
                let overlay_bg_ranges = split_ranges(&all_bg_ranges, overlay_bg_idx, total_bg);
                self.rects
                    .draw_ranges(&self.gl, &overlay_bg_ranges, vw, vh, lb);
            }

            // 7b. SDF chrome (rounded corners + border + shadow) for popups
            //     like the command palette and context menu. Drawn after flat
            //     overlay bgs and before overlay glyphs so the panel sits on
            //     top of pane text and labels paint crisply on top of it.
            if !scene.sdf_rects.is_empty() {
                self.sdf.upload(&self.gl, scene.sdf_rects);
                self.sdf.draw(&self.gl, scene.sdf_rects.len(), vw, vh, lb);
            }

            // 8. Overlay glyphs (no re-upload, just draw remaining range).
            let overlay_alpha = PaneGlyphRange {
                start: scene.pane_glyph_end as u32,
                count: scene.glyphs.len().saturating_sub(scene.pane_glyph_end) as u32,
                scissor: (0, 0, self.width, self.height),
                ..PaneGlyphRange::default()
            };
            let overlay_color = PaneGlyphRange {
                start: scene.pane_color_glyph_end as u32,
                count: scene
                    .color_glyphs
                    .len()
                    .saturating_sub(scene.pane_color_glyph_end) as u32,
                scissor: (0, 0, self.width, self.height),
                ..PaneGlyphRange::default()
            };
            atlas_gpu
                .alpha
                .draw_batches(&self.gl, alpha_count, &vp, &[overlay_alpha], lb, lc);
            atlas_gpu
                .color
                .draw_batches(&self.gl, color_count, &vp, &[overlay_color], lb, lc);
            if let Some(profiler) = profiler.as_mut() {
                profiler.record_draw(draw_start);
            }

            // Blit sRGB FBO → default framebuffer (if using sRGB target).
            // When the softness post-process is active, first run two blur
            // passes into the ping-pong textures, then blit from the final
            // pass's target.
            if let Some(ref target) = self.srgb_target {
                let w = self.width as i32;
                let h = self.height as i32;

                let read_fb = if let Some(ref mut blur) = self.blur {
                    blur.ping.resize(&self.gl, self.width, self.height);
                    blur.pong.resize(&self.gl, self.width, self.height);

                    // 1. Copy sRGB renderbuffer into blur.ping texture so it
                    //    becomes sampleable. Both are SRGB8_ALPHA8 → no gamma
                    //    conversion happens in the blit.
                    self.gl
                        .bind_framebuffer(glow::READ_FRAMEBUFFER, Some(target.framebuffer));
                    self.gl
                        .bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(blur.ping.framebuffer));
                    self.gl.blit_framebuffer(
                        0,
                        0,
                        w,
                        h,
                        0,
                        0,
                        w,
                        h,
                        glow::COLOR_BUFFER_BIT,
                        glow::NEAREST,
                    );

                    // 2. Horizontal pass: ping.texture → pong.framebuffer.
                    blur.pipeline.pass(
                        &self.gl,
                        blur.ping.texture,
                        blur.pong.framebuffer,
                        blur.pong.width,
                        blur.pong.height,
                        (1.0, 0.0),
                        self.softness,
                    );

                    // 3. Vertical pass: pong.texture → ping.framebuffer.
                    blur.pipeline.pass(
                        &self.gl,
                        blur.pong.texture,
                        blur.ping.framebuffer,
                        blur.ping.width,
                        blur.ping.height,
                        (0.0, 1.0),
                        self.softness,
                    );

                    blur.ping.framebuffer
                } else {
                    target.framebuffer
                };

                // Disable GL_FRAMEBUFFER_SRGB during blit to avoid double gamma.
                self.gl.disable(glow::FRAMEBUFFER_SRGB);
                self.gl
                    .bind_framebuffer(glow::READ_FRAMEBUFFER, Some(read_fb));
                self.gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, None);
                self.gl.blit_framebuffer(
                    0,
                    0,
                    w,
                    h,
                    0,
                    0,
                    w,
                    h,
                    glow::COLOR_BUFFER_BIT,
                    glow::NEAREST,
                );
                self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
                self.gl.enable(glow::FRAMEBUFFER_SRGB);
            }
        }

        // Present — on Wayland EGL this implicitly handles resize
        let present_start = std::time::Instant::now();
        self.gl_surface
            .swap_buffers(&self.gl_context)
            .map_err(|e| crate::GpuError::SurfaceLost(format!("swap_buffers: {e}")))?;
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
        Ok(())
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            if let Some(ref blur) = self.blur {
                blur.pipeline.destroy(&self.gl);
                blur.ping.destroy(&self.gl);
                blur.pong.destroy(&self.gl);
            }
            if let Some(ref target) = self.srgb_target {
                target.destroy(&self.gl);
            }
            self.rects.destroy(&self.gl);
            self.sdf.destroy(&self.gl);
            self.background_image.destroy(&self.gl);
        }
    }
}

// ─── Vertex attrib helpers ──────────────────────────────────────────

unsafe fn setup_glyph_vertex_attribs(gl: &glow::Context) {
    setup_glyph_vertex_attribs_offset(gl, 0);
}

/// Vertex attribute layout for `SdfRect`. Field offsets must match the Rust
/// struct exactly — they form a three-way contract with the Rust layout and
/// the GLSL `in` declarations in `SDF_VS`.
unsafe fn setup_sdf_vertex_attribs(gl: &glow::Context, base_offset: i32) {
    let stride = SdfRect::SIZE as i32;
    // pos
    gl.enable_vertex_attrib_array(0);
    gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, stride, base_offset);
    gl.vertex_attrib_divisor(0, 1);
    // size
    gl.enable_vertex_attrib_array(1);
    gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, stride, base_offset + 8);
    gl.vertex_attrib_divisor(1, 1);
    // color
    gl.enable_vertex_attrib_array(2);
    gl.vertex_attrib_pointer_f32(2, 4, glow::FLOAT, false, stride, base_offset + 16);
    gl.vertex_attrib_divisor(2, 1);
    // radii
    gl.enable_vertex_attrib_array(3);
    gl.vertex_attrib_pointer_f32(3, 4, glow::FLOAT, false, stride, base_offset + 32);
    gl.vertex_attrib_divisor(3, 1);
    // border_color
    gl.enable_vertex_attrib_array(4);
    gl.vertex_attrib_pointer_f32(4, 4, glow::FLOAT, false, stride, base_offset + 48);
    gl.vertex_attrib_divisor(4, 1);
    // border_width
    gl.enable_vertex_attrib_array(5);
    gl.vertex_attrib_pointer_f32(5, 1, glow::FLOAT, false, stride, base_offset + 64);
    gl.vertex_attrib_divisor(5, 1);
    // shadow_blur
    gl.enable_vertex_attrib_array(6);
    gl.vertex_attrib_pointer_f32(6, 1, glow::FLOAT, false, stride, base_offset + 68);
    gl.vertex_attrib_divisor(6, 1);
    // shadow_offset
    gl.enable_vertex_attrib_array(7);
    gl.vertex_attrib_pointer_f32(7, 2, glow::FLOAT, false, stride, base_offset + 72);
    gl.vertex_attrib_divisor(7, 1);
    // shadow_color
    gl.enable_vertex_attrib_array(8);
    gl.vertex_attrib_pointer_f32(8, 4, glow::FLOAT, false, stride, base_offset + 80);
    gl.vertex_attrib_divisor(8, 1);
}

unsafe fn setup_glyph_vertex_attribs_offset(gl: &glow::Context, base_offset: i32) {
    let stride = std::mem::size_of::<GlyphInstance>() as i32;
    // pos
    gl.enable_vertex_attrib_array(0);
    gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, stride, base_offset);
    gl.vertex_attrib_divisor(0, 1);
    // size
    gl.enable_vertex_attrib_array(1);
    gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, stride, base_offset + 8);
    gl.vertex_attrib_divisor(1, 1);
    // uv_pos
    gl.enable_vertex_attrib_array(2);
    gl.vertex_attrib_pointer_f32(2, 2, glow::FLOAT, false, stride, base_offset + 16);
    gl.vertex_attrib_divisor(2, 1);
    // uv_size
    gl.enable_vertex_attrib_array(3);
    gl.vertex_attrib_pointer_f32(3, 2, glow::FLOAT, false, stride, base_offset + 24);
    gl.vertex_attrib_divisor(3, 1);
    // color
    gl.enable_vertex_attrib_array(4);
    gl.vertex_attrib_pointer_f32(4, 4, glow::FLOAT, false, stride, base_offset + 32);
    gl.vertex_attrib_divisor(4, 1);
    // bg_color
    gl.enable_vertex_attrib_array(5);
    gl.vertex_attrib_pointer_f32(5, 4, glow::FLOAT, false, stride, base_offset + 48);
    gl.vertex_attrib_divisor(5, 1);
}

// ─── Shader compilation ─────────────────────────────────────────────

unsafe fn compile_program(
    gl: &glow::Context,
    vs_src: &str,
    fs_src: &str,
    label: &str,
) -> std::result::Result<glow::Program, crate::GpuError> {
    let vs = gl.create_shader(glow::VERTEX_SHADER).map_err(|e| {
        crate::GpuError::ShaderCompile(format!("[{label}] create vertex shader: {e}"))
    })?;
    gl.shader_source(vs, vs_src);
    gl.compile_shader(vs);
    if !gl.get_shader_compile_status(vs) {
        let log = gl.get_shader_info_log(vs);
        gl.delete_shader(vs);
        return Err(crate::GpuError::ShaderCompile(format!(
            "[{label}] vertex shader compile error: {log}"
        )));
    }

    let fs = gl.create_shader(glow::FRAGMENT_SHADER).map_err(|e| {
        crate::GpuError::ShaderCompile(format!("[{label}] create fragment shader: {e}"))
    })?;
    gl.shader_source(fs, fs_src);
    gl.compile_shader(fs);
    if !gl.get_shader_compile_status(fs) {
        let log = gl.get_shader_info_log(fs);
        gl.delete_shader(vs);
        gl.delete_shader(fs);
        return Err(crate::GpuError::ShaderCompile(format!(
            "[{label}] fragment shader compile error: {log}"
        )));
    }

    let program = gl
        .create_program()
        .map_err(|e| crate::GpuError::ShaderCompile(format!("[{label}] create program: {e}")))?;
    gl.attach_shader(program, vs);
    gl.attach_shader(program, fs);
    gl.link_program(program);
    if !gl.get_program_link_status(program) {
        let log = gl.get_program_info_log(program);
        gl.delete_shader(vs);
        gl.delete_shader(fs);
        gl.delete_program(program);
        return Err(crate::GpuError::ShaderCompile(format!(
            "[{label}] program link error: {log}"
        )));
    }

    gl.delete_shader(vs);
    gl.delete_shader(fs);
    Ok(program)
}

// ─── GLSL shaders ───────────────────────────────────────────────────

const RECT_VS: &str = r#"#version 330 core

layout(location = 0) in vec2 a_pos;
layout(location = 1) in vec2 a_size;
layout(location = 2) in vec4 a_color;

uniform vec2 u_viewport;
uniform vec2 u_pane_origin;

out vec4 v_color;
out vec2 v_pane_local;

void main() {
    float x = float(gl_VertexID & 1);
    float y = float((gl_VertexID >> 1) & 1);

    vec2 px = a_pos + vec2(x, y) * a_size;
    vec2 ndc = vec2(
        px.x / u_viewport.x * 2.0 - 1.0,
        1.0 - px.y / u_viewport.y * 2.0
    );

    gl_Position = vec4(ndc, 0.0, 1.0);
    v_color = a_color;
    v_pane_local = px - u_pane_origin;
}
"#;

const RECT_FS: &str = r#"#version 330 core

in vec4 v_color;
in vec2 v_pane_local;
uniform vec2 u_pane_size;
uniform vec4 u_pane_radii;
uniform bool u_use_linear_blending;
out vec4 frag_color;

// COLOR_FUNCS_PLACEHOLDER

// CORNER_FUNCS_PLACEHOLDER

void main() {
    vec4 color = v_color;
    // When linear blending is on, linearize sRGB input so the
    // sRGB FBO + GL_FRAMEBUFFER_SRGB auto-encodes correctly.
    if (u_use_linear_blending) {
        color = linearize(color);
    }
    frag_color = vec4(color.rgb * color.a, color.a);
    frag_color *= ciri_corner_alpha(v_pane_local, u_pane_size, u_pane_radii);
}
"#;

// ─── SDF shaders ────────────────────────────────────────────────────
//
// Port of `SDF_SHADER` (WGSL) from the blade backend. All math is in
// logical pixels; the body, border, and shadow are composited in shader
// so a single instance produces the full chrome rect.
//
// `u_use_linear_blending` mirrors the rect / glyph shaders: when on, sRGB
// inputs are linearized so the sRGB FBO + `GL_FRAMEBUFFER_SRGB` path
// re-encodes correctly. Native (non-linear) mode outputs the input sRGB
// premultiplied directly.

const SDF_VS: &str = r#"#version 330 core

layout(location = 0) in vec2 a_pos;
layout(location = 1) in vec2 a_size;
layout(location = 2) in vec4 a_color;
layout(location = 3) in vec4 a_radii;
layout(location = 4) in vec4 a_border_color;
layout(location = 5) in float a_border_width;
layout(location = 6) in float a_shadow_blur;
layout(location = 7) in vec2 a_shadow_offset;
layout(location = 8) in vec4 a_shadow_color;

uniform vec2 u_viewport;

out vec2 v_local;
out vec2 v_half_size;
out vec4 v_color;
out vec4 v_radii;
out vec4 v_border_color;
out float v_border_width;
out float v_shadow_blur;
out vec2 v_shadow_offset;
out vec4 v_shadow_color;

void main() {
    float x = float(gl_VertexID & 1);
    float y = float((gl_VertexID >> 1) & 1);

    // Inflate the quad so shadow blur + offset spill outside the rect's
    // bounds without clipping. 3σ covers ~99.7% of a Gaussian envelope.
    float pad = a_shadow_blur * 3.0
              + max(abs(a_shadow_offset.x), abs(a_shadow_offset.y));
    vec2 padded_pos = a_pos - vec2(pad);
    vec2 padded_size = a_size + vec2(pad * 2.0);

    vec2 px = padded_pos + vec2(x, y) * padded_size;
    vec2 ndc = vec2(
        px.x / u_viewport.x * 2.0 - 1.0,
        1.0 - px.y / u_viewport.y * 2.0
    );

    vec2 centre = a_pos + a_size * 0.5;
    v_local = px - centre;
    v_half_size = a_size * 0.5;
    v_color = a_color;
    v_radii = a_radii;
    v_border_color = a_border_color;
    v_border_width = a_border_width;
    v_shadow_blur = a_shadow_blur;
    v_shadow_offset = a_shadow_offset;
    v_shadow_color = a_shadow_color;

    gl_Position = vec4(ndc, 0.0, 1.0);
}
"#;

const SDF_FS: &str = r#"#version 330 core

in vec2 v_local;
in vec2 v_half_size;
in vec4 v_color;
in vec4 v_radii;
in vec4 v_border_color;
in float v_border_width;
in float v_shadow_blur;
in vec2 v_shadow_offset;
in vec4 v_shadow_color;

uniform bool u_use_linear_blending;

out vec4 frag_color;

// COLOR_FUNCS_PLACEHOLDER

// SDF of a rounded box centred at the origin. Per-corner radii order
// matches CSS: tl, tr, br, bl. Picks the corner from the sample quadrant.
float sdf_rounded_box(vec2 p, vec2 b, vec4 r) {
    float r_top_x = (p.x > 0.0) ? r.y : r.x;   // tl | tr
    float r_bot_x = (p.x > 0.0) ? r.z : r.w;   // bl | br
    float radius = (p.y > 0.0) ? r_bot_x : r_top_x;
    vec2 q = abs(p) - b + vec2(radius);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2(0.0))) - radius;
}

float shadow_envelope(float d, float blur) {
    if (blur <= 0.0) { return 0.0; }
    return clamp(0.5 - 0.5 * d / blur, 0.0, 1.0);
}

void main() {
    float d_body = sdf_rounded_box(v_local, v_half_size, v_radii);

    // DPR-aware AA: half a pixel-derivative either side of the edge.
    float aa = max(fwidth(d_body) * 0.5, 1e-5);
    float body_alpha = clamp(0.5 - d_body / (aa * 2.0), 0.0, 1.0);

    // Border SDF band: half-width centred at d = -border_width/2, i.e.
    // sitting inside the body's outer edge so the AA fringes line up.
    float border_alpha = 0.0;
    if (v_border_width > 0.0) {
        float bw = v_border_width * 0.5;
        float d_band = abs(d_body + bw) - bw;
        border_alpha = clamp(0.5 - d_band / (aa * 2.0), 0.0, 1.0);
    }

    // Linearize sRGB inputs so blending against the sRGB FBO is correct.
    // In native mode, outputs are written as sRGB directly.
    vec4 fill = v_color;
    vec4 border_col = v_border_color;
    vec4 shadow_in = v_shadow_color;
    if (u_use_linear_blending) {
        fill = linearize(fill);
        border_col = linearize(border_col);
        shadow_in = linearize(shadow_in);
    }

    vec4 shadow_col = vec4(0.0);
    if (v_shadow_blur > 0.0 && shadow_in.a > 0.0) {
        float d_shadow = sdf_rounded_box(v_local - v_shadow_offset, v_half_size, v_radii);
        float env = shadow_envelope(d_shadow, v_shadow_blur);
        // Body occludes its own shadow to avoid a double-dark inner ring.
        float occlusion = 1.0 - body_alpha;
        float a = env * shadow_in.a * occlusion;
        shadow_col = vec4(shadow_in.rgb * a, a);
    }

    // Premultiply fill + border so OVER compositing works directly.
    vec4 body = vec4(fill.rgb * fill.a * body_alpha, fill.a * body_alpha);
    vec4 border = vec4(border_col.rgb * border_col.a * border_alpha,
                       border_col.a * border_alpha);

    // Shadow under body, border over body.
    vec3 out_rgb = shadow_col.rgb * (1.0 - body.a)
                 + body.rgb * (1.0 - border.a)
                 + border.rgb;
    float out_a = shadow_col.a * (1.0 - body.a)
                + body.a * (1.0 - border.a)
                + border.a;
    frag_color = vec4(out_rgb, out_a);
}
"#;

const GLYPH_VS: &str = r#"#version 330 core

layout(location = 0) in vec2 a_pos;
layout(location = 1) in vec2 a_size;
layout(location = 2) in vec2 a_uv_pos;
layout(location = 3) in vec2 a_uv_size;
layout(location = 4) in vec4 a_color;
layout(location = 5) in vec4 a_bg_color;

uniform vec2 u_viewport;
uniform vec2 u_pane_origin;

out vec2 v_uv;
out vec4 v_color;
out vec4 v_bg_color;
out vec2 v_pane_local;

void main() {
    float x = float(gl_VertexID & 1);
    float y = float((gl_VertexID >> 1) & 1);

    v_uv = a_uv_pos + vec2(x, y) * a_uv_size;
    v_color = a_color;
    v_bg_color = a_bg_color;

    vec2 px = a_pos + vec2(x, y) * a_size;
    v_pane_local = px - u_pane_origin;
    vec2 ndc = vec2(
        px.x / u_viewport.x * 2.0 - 1.0,
        1.0 - px.y / u_viewport.y * 2.0
    );
    gl_Position = vec4(ndc, 0.0, 1.0);
}
"#;

// ─── Shared GLSL functions for sRGB ↔ linear conversion ────────────
// Per-corner rounding alpha mask, built on Inigo Quilez's classic
// rounded-box signed distance function (see iquilezles.org/articles/
// distfunctions/). The same recipe powers ciri's existing SDF chrome
// path (`sdf_rounded_box` further down this file) — this helper is the
// alpha-mask packaging of it for callers that just want corner clipping
// on top of an already-rendered fragment (cell bg, glyphs, pane bg,
// focus ring).
//
// `radii` order is CSS: tl, tr, br, bl. The early return on all-zero
// radii means non-rounded callers pay one branch and zero ALU.
const GLSL_CORNER_FUNCS: &str = r#"
float ciri_sdf_rounded_box(vec2 p, vec2 b, vec4 r) {
    float rx = (p.x > 0.0) ? r.y : r.x;
    float bx = (p.x > 0.0) ? r.z : r.w;
    float radius = (p.y > 0.0) ? bx : rx;
    vec2 q = abs(p) - b + vec2(radius);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2(0.0))) - radius;
}

float ciri_corner_alpha(vec2 px, vec2 size, vec4 radii) {
    if (radii.x <= 0.0 && radii.y <= 0.0 && radii.z <= 0.0 && radii.w <= 0.0) {
        return 1.0;
    }
    float d = ciri_sdf_rounded_box(px - 0.5 * size, 0.5 * size, radii);
    // smoothstep spans 2 * aa, so use half-pixel derivative to land
    // a one-pixel-wide AA transition — matches the chrome SDF path's
    // `fwidth(d_body) * 0.5` further down this file.
    float aa = max(fwidth(d) * 0.5, 1e-5);
    return 1.0 - smoothstep(-aa, aa, d);
}
"#;

const GLSL_COLOR_FUNCS: &str = r#"
vec4 linearize(vec4 srgb) {
    bvec3 c = lessThanEqual(srgb.rgb, vec3(0.04045));
    vec3 hi = pow((srgb.rgb + vec3(0.055)) / vec3(1.055), vec3(2.4));
    vec3 lo = srgb.rgb / vec3(12.92);
    return vec4(mix(hi, lo, c), srgb.a);
}

float linearize_f(float v) {
    return v <= 0.04045 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4);
}

vec4 unlinearize(vec4 lin) {
    bvec3 c = lessThanEqual(lin.rgb, vec3(0.0031308));
    vec3 hi = pow(lin.rgb, vec3(1.0 / 2.4)) * vec3(1.055) - vec3(0.055);
    vec3 lo = lin.rgb * vec3(12.92);
    return vec4(mix(hi, lo, c), lin.a);
}

float unlinearize_f(float v) {
    return v <= 0.0031308 ? v * 12.92 : pow(v, 1.0 / 2.4) * 1.055 - 0.055;
}

float luminance(vec3 col) {
    return dot(col, vec3(0.2126, 0.7152, 0.0722));
}
"#;

const ALPHA_FS: &str = r#"#version 330 core

in vec2 v_uv;
in vec4 v_color;
in vec4 v_bg_color;
in vec2 v_pane_local;

uniform sampler2D u_atlas;
uniform vec2 u_pane_size;
uniform vec4 u_pane_radii;
uniform bool u_use_linear_blending;
uniform bool u_use_linear_correction;

out vec4 frag_color;

// COLOR_FUNCS_PLACEHOLDER

// CORNER_FUNCS_PLACEHOLDER

void main() {
    // Input color is sRGB non-premultiplied. Always linearize first.
    vec4 color = linearize(v_color);
    // Premultiply in linear space.
    color.rgb *= color.a;

    // When NOT using linear blending (native mode, RGBA8 FBO):
    // un-premultiply, convert back to sRGB, re-premultiply.
    // GPU writes these sRGB values directly (no auto-encode).
    if (!u_use_linear_blending) {
        color.rgb /= max(color.a, 0.00001);
        color = unlinearize(color);
        color.rgb *= color.a;
    }

    // Fetch alpha mask from atlas.
    float a = texture(u_atlas, v_uv).r;

    // Weight correction: adjust alpha so that linear-space hardware
    // blending (via sRGB FBO) produces stroke weight matching
    // gamma-space rendering. Uses premultiplied linear luminance
    // (matching Ghostty's approach).
    if (u_use_linear_correction) {
        vec4 bg_linear = linearize(v_bg_color);
        // Premultiplied linear luminances — Ghostty uses luminance(color.rgb)
        // directly on the premultiplied linear color.
        vec4 fg_linear_premul = linearize(v_color);
        fg_linear_premul.rgb *= fg_linear_premul.a;
        float fg_l = luminance(fg_linear_premul.rgb);
        float bg_l = luminance(bg_linear.rgb);
        if (abs(fg_l - bg_l) > 0.001) {
            float blend_l = linearize_f(
                unlinearize_f(fg_l) * a + unlinearize_f(bg_l) * (1.0 - a)
            );
            a = clamp((blend_l - bg_l) / (fg_l - bg_l), 0.0, 1.0);
        }
    }

    // Apply alpha mask. Output is:
    // - linear premultiplied (when use_linear_blending) → GPU auto sRGB-encodes via SRGB8_ALPHA8 FBO
    // - sRGB premultiplied (when native) → written directly to RGBA8 FBO
    color *= a;
    color *= ciri_corner_alpha(v_pane_local, u_pane_size, u_pane_radii);
    frag_color = color;
}
"#;

const COLOR_FS: &str = r#"#version 330 core

in vec2 v_uv;
in vec4 v_color;
in vec4 v_bg_color;
in vec2 v_pane_local;

uniform sampler2D u_atlas;
uniform vec2 u_pane_size;
uniform vec4 u_pane_radii;
uniform bool u_use_linear_blending;
uniform bool u_use_linear_correction;

out vec4 frag_color;

// COLOR_FUNCS_PLACEHOLDER

// CORNER_FUNCS_PLACEHOLDER

void main() {
    // Atlas is SRGB8_ALPHA8 — GPU auto-linearizes on sample.
    // texel is now linear premultiplied.
    vec4 texel = texture(u_atlas, v_uv);

    if (u_use_linear_blending) {
        // Output linear premultiplied → GPU auto sRGB-encodes to FBO.
        frag_color = vec4(texel.rgb * v_color.rgb, texel.a * v_color.a);
    } else {
        // Native mode: unlinearize back to sRGB before output.
        vec3 unpre = texel.rgb / max(texel.a, 0.00001);
        vec4 srgb_texel = unlinearize(vec4(unpre, texel.a));
        srgb_texel.rgb *= srgb_texel.a; // re-premultiply
        frag_color = vec4(srgb_texel.rgb * v_color.rgb, srgb_texel.a * v_color.a);
    }
    frag_color *= ciri_corner_alpha(v_pane_local, u_pane_size, u_pane_radii);
}
"#;

// Fullscreen triangle — no VBO needed.
const BLUR_VS: &str = r#"#version 330 core
out vec2 v_uv;
void main() {
    // Large triangle that covers the viewport (-1,-1) to (+3,+3 or -1,+3 etc.)
    // Tap the three corners so v_uv interpolates 0..1 across the visible area.
    float x = float((gl_VertexID & 1) << 2);  // 0, 4, 0
    float y = float((gl_VertexID & 2) << 1);  // 0, 0, 4
    gl_Position = vec4(x - 1.0, y - 1.0, 0.0, 1.0);
    v_uv = vec2(x, y) * 0.5;
}
"#;

// 5-tap separable Gaussian. Runs once horizontally then once vertically.
// When `u_strength` is 0, all five samples collapse onto v_uv and weights
// sum to 1.0 — so the output equals the input (identity). As strength
// grows, taps spread out in the chosen direction up to ~2 texels.
//
// The shader works in the sRGB FBO pipeline: sampling a GL_SRGB8_ALPHA8
// texture auto-linearises, writing with GL_FRAMEBUFFER_SRGB auto-encodes,
// so the blur is physically correct (linear-space filtering).
const BLUR_FS: &str = r#"#version 330 core
in vec2 v_uv;
out vec4 frag_color;

uniform sampler2D u_tex;
uniform vec2  u_direction;   // (1,0) for H pass, (0,1) for V pass
uniform float u_strength;    // 0..1

void main() {
    vec2 texel = 1.0 / vec2(textureSize(u_tex, 0));
    // Offset per tap, in texels, scaled by strength. Max 2 texels → ~σ=1.2.
    vec2 off = u_direction * texel * (u_strength * 2.0);

    vec4 s0 = texture(u_tex, v_uv - 2.0 * off);
    vec4 s1 = texture(u_tex, v_uv -       off);
    vec4 s2 = texture(u_tex, v_uv            );
    vec4 s3 = texture(u_tex, v_uv +       off);
    vec4 s4 = texture(u_tex, v_uv + 2.0 * off);

    // Binomial weights: 1, 4, 6, 4, 1 / 16.
    frag_color = s0 * 0.0625 + s1 * 0.25 + s2 * 0.375 + s3 * 0.25 + s4 * 0.0625;
}
"#;

// ─── Overview wallpaper shaders ─────────────────────────────────────
//
// VS: vertexless fullscreen quad. `gl_VertexID` indexes a hard-coded
// corner table (two triangles), and the same NDC Y-flip the rect / glyph
// pipelines use lines screen-top up with image-top.
// FS: samples the user-uploaded RGBA8 texture, optionally linearizes
// when rendering to an sRGB FBO (so `GL_FRAMEBUFFER_SRGB` re-encodes
// correctly), and outputs `(rgb*opacity, opacity)` for premult-alpha
// blending over the already-cleared `clear_color` framebuffer.

const BACKGROUND_IMAGE_VS: &str = r#"#version 330 core

uniform vec4 u_viewport_tex;  // vw, vh, tw, th

out vec2 v_uv;

void main() {
    vec2 corners[6] = vec2[](
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
        vec2(0.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0)
    );
    vec2 vuv = corners[gl_VertexID];
    gl_Position = vec4(vuv.x * 2.0 - 1.0, 1.0 - vuv.y * 2.0, 0.0, 1.0);

    float vw = u_viewport_tex.x;
    float vh = u_viewport_tex.y;
    float tw = u_viewport_tex.z;
    float th = u_viewport_tex.w;
    float v_aspect = vw / max(vh, 1e-6);
    float t_aspect = tw / max(th, 1e-6);
    vec2 scale = vec2(1.0);
    if (t_aspect > v_aspect) {
        // texture wider than viewport — crop the sides
        scale.x = v_aspect / max(t_aspect, 1e-6);
    } else {
        scale.y = t_aspect / max(v_aspect, 1e-6);
    }
    v_uv = (vuv - 0.5) * scale + 0.5;
}
"#;

const BACKGROUND_IMAGE_FS: &str = r#"#version 330 core
in vec2 v_uv;
out vec4 frag_color;

uniform sampler2D u_tex;
uniform vec4 u_params;  // .x = opacity, .y = use_linear_blending (0/1)

// COLOR_FUNCS_PLACEHOLDER

void main() {
    vec4 col = texture(u_tex, v_uv);
    float opacity = clamp(u_params.x, 0.0, 1.0);
    if (u_params.y > 0.5) {
        col = linearize(col);
    }
    frag_color = vec4(col.rgb * opacity, opacity);
}
"#;
