//! OpenGL 3.3+ / EGL backend.
//!
//! Designed for Linux Wayland where `eglSwapBuffers` handles resize implicitly,
//! avoiding the `vkDeviceWaitIdle` / swapchain rebuild overhead of Vulkan.
//!
//! Uses `glutin` for EGL context management and `glow` for GL calls.

use ciri_config::config::RenderConfig;
use ciri_render::FrameScene;
use ciri_render::glyph_cache::{GlyphCache, GlyphInstance, PendingUpload, ScissoredRange};
use ciri_render::rect::Rect;
use glow::HasContext;
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
        batches: &[ScissoredRange],
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
        gl.bind_vertex_array(Some(self.vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.instance_vbo));

        gl.enable(glow::SCISSOR_TEST);

        for batch in batches {
            let start = batch.start.min(count);
            let end = batch.end.min(count);
            if start >= end || batch.w == 0 || batch.h == 0 {
                continue;
            }
            let sy = vp.height_px.saturating_sub(batch.y + batch.h);
            gl.scissor(batch.x as i32, sy as i32, batch.w as i32, batch.h as i32);

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
    max_rects: usize,
}

impl GlRectPipeline {
    unsafe fn new(gl: &glow::Context, max_rects: usize) -> crate::Result<Self> {
        let program = compile_program(gl, RECT_VS, RECT_FS, "rect")?;
        let loc_viewport = gl
            .get_uniform_location(program, "u_viewport")
            .ok_or_else(|| {
                crate::GpuError::ShaderCompile("u_viewport uniform not found in rect shader".into())
            })?;

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
            max_rects,
        })
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

    /// Draw a range of previously uploaded rects.
    unsafe fn draw_range(
        &self,
        gl: &glow::Context,
        start: usize,
        count: usize,
        viewport_w: f32,
        viewport_h: f32,
    ) {
        if count == 0 {
            return;
        }
        gl.use_program(Some(self.program));
        gl.uniform_2_f32(Some(&self.loc_viewport), viewport_w, viewport_h);
        gl.bind_vertex_array(Some(self.vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.instance_vbo));

        let stride = std::mem::size_of::<Rect>() as i32;
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

        gl.bind_vertex_array(None);
        gl.use_program(None);
    }

    unsafe fn destroy(&self, gl: &glow::Context) {
        gl.delete_program(self.program);
        gl.delete_vertex_array(self.vao);
        gl.delete_buffer(self.instance_vbo);
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
        let alpha = GlAtlasLayer::new(
            gl,
            &GlAtlasLayerConfig {
                atlas_size,
                max_instances,
                internal_format: glow::R8,
                format: glow::RED,
                vs_src: GLYPH_VS,
                fs_src: ALPHA_FS,
                bpp: 1,
                label: "alpha_atlas",
            },
        )?;
        let color = GlAtlasLayer::new(
            gl,
            &GlAtlasLayerConfig {
                atlas_size,
                max_instances,
                internal_format: glow::RGBA8,
                format: glow::RGBA,
                vs_src: GLYPH_VS,
                fs_src: COLOR_FS,
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
    width: u32,
    height: u32,
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
            // No GL_FRAMEBUFFER_SRGB — colors are passed as sRGB directly,
            // matching ghostty's non-linear-blending pipeline.
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
        }

        let rects = unsafe { GlRectPipeline::new(&gl, render_config.max_rectangles)? };

        Ok(Renderer {
            gl,
            gl_surface,
            gl_context,
            rects,
            width: size.width.max(1),
            height: size.height.max(1),
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

        unsafe {
            self.gl
                .viewport(0, 0, self.width as i32, self.height as i32);

            // Flush pending glyph uploads
            let (mut ap, mut cp, ac, cc) = cache.take_pending();
            atlas_gpu.alpha.flush_uploads(&self.gl, &mut ap, ac);
            atlas_gpu.color.flush_uploads(&self.gl, &mut cp, cc);

            // Clear
            self.gl.clear_color(
                scene.clear_color[0],
                scene.clear_color[1],
                scene.clear_color[2],
                scene.clear_color[3],
            );
            self.gl.clear(glow::COLOR_BUFFER_BIT);

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
            let active_bg_idx = 1 + scene.active_bg_start; // +1 for clear rect
            let overlay_bg_idx = 1 + scene.overlay_bg_start; // +1 for clear rect
            let total_bg = all_bg.len().min(self.rects.max_rects);
            self.rects.upload(&self.gl, &all_bg, vw, vh);

            // 2. Draw non-focused pane background rects.
            let inactive_bg_count = active_bg_idx.min(total_bg);
            self.rects
                .draw_range(&self.gl, 0, inactive_bg_count, vw, vh);

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

            // 4. Draw inactive pane glyphs (scissored).
            atlas_gpu
                .alpha
                .draw_batches(&self.gl, alpha_count, &vp, scene.glyph_batches);
            atlas_gpu
                .color
                .draw_batches(&self.gl, color_count, &vp, scene.color_glyph_batches);

            // 5. Focused pane background rects.
            let active_bg_count = overlay_bg_idx.saturating_sub(active_bg_idx);
            if active_bg_count > 0 {
                self.rects
                    .draw_range(&self.gl, active_bg_idx, active_bg_count, vw, vh);
            }

            // 6. Draw active pane glyphs (scissored, no re-upload).
            atlas_gpu
                .alpha
                .draw_batches(&self.gl, alpha_count, &vp, scene.active_glyph_batches);
            atlas_gpu.color.draw_batches(
                &self.gl,
                color_count,
                &vp,
                scene.active_color_glyph_batches,
            );

            // 7. Overlay background rects (rendered after pane glyphs so they
            //    occlude terminal text underneath popups like the context menu).
            let overlay_bg_count = total_bg.saturating_sub(overlay_bg_idx);
            if overlay_bg_count > 0 {
                self.rects
                    .draw_range(&self.gl, overlay_bg_idx, overlay_bg_count, vw, vh);
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
                .draw_batches(&self.gl, alpha_count, &vp, &[overlay_alpha]);
            atlas_gpu
                .color
                .draw_batches(&self.gl, color_count, &vp, &[overlay_color]);
        }

        // Present — on Wayland EGL this implicitly handles resize
        self.gl_surface
            .swap_buffers(&self.gl_context)
            .map_err(|e| crate::GpuError::SurfaceLost(format!("swap_buffers: {e}")))?;
        Ok(())
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            self.rects.destroy(&self.gl);
        }
    }
}

// ─── Vertex attrib helpers ──────────────────────────────────────────

unsafe fn setup_glyph_vertex_attribs(gl: &glow::Context) {
    setup_glyph_vertex_attribs_offset(gl, 0);
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

out vec4 v_color;

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
}
"#;

const RECT_FS: &str = r#"#version 330 core

in vec4 v_color;
out vec4 frag_color;

void main() {
    frag_color = vec4(v_color.rgb * v_color.a, v_color.a);
}
"#;

const GLYPH_VS: &str = r#"#version 330 core

layout(location = 0) in vec2 a_pos;
layout(location = 1) in vec2 a_size;
layout(location = 2) in vec2 a_uv_pos;
layout(location = 3) in vec2 a_uv_size;
layout(location = 4) in vec4 a_color;

uniform vec2 u_viewport;

out vec2 v_uv;
out vec4 v_color;

void main() {
    float x = float(gl_VertexID & 1);
    float y = float((gl_VertexID >> 1) & 1);

    v_uv = a_uv_pos + vec2(x, y) * a_uv_size;
    v_color = a_color;

    vec2 px = a_pos + vec2(x, y) * a_size;
    vec2 ndc = vec2(
        px.x / u_viewport.x * 2.0 - 1.0,
        1.0 - px.y / u_viewport.y * 2.0
    );
    gl_Position = vec4(ndc, 0.0, 1.0);
}
"#;

const ALPHA_FS: &str = r#"#version 330 core

in vec2 v_uv;
in vec4 v_color;

uniform sampler2D u_atlas;

out vec4 frag_color;

void main() {
    float alpha = texture(u_atlas, v_uv).r;
    float out_alpha = v_color.a * alpha;
    frag_color = vec4(v_color.rgb * out_alpha, out_alpha);
}
"#;

const COLOR_FS: &str = r#"#version 330 core

in vec2 v_uv;
in vec4 v_color;

uniform sampler2D u_atlas;

out vec4 frag_color;

void main() {
    vec4 texel = texture(u_atlas, v_uv);
    frag_color = vec4(texel.rgb * v_color.rgb, texel.a * v_color.a);
}
"#;
