//! OpenGL 3.3+ / EGL backend.
//!
//! Designed for Linux Wayland where `eglSwapBuffers` handles resize implicitly,
//! avoiding the `vkDeviceWaitIdle` / swapchain rebuild overhead of Vulkan.
//!
//! Uses `glutin` for EGL context management and `glow` for GL calls.

use anyhow::Result;
use ciri_config::config::RenderConfig;
use ciri_render::glyph_cache::{GlyphCache, GlyphInstance, PendingUpload, ScissoredRange};
use ciri_render::rect::Rect;
use ciri_render::FrameScene;
use glow::HasContext;
use glutin::config::ConfigTemplateBuilder;
use glutin::context::{ContextApi, ContextAttributesBuilder, PossiblyCurrentContext, Version};
use glutin::prelude::*;
use glutin::surface::{SurfaceAttributesBuilder, SwapInterval, WindowSurface};
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

impl GlAtlasLayer {
    unsafe fn new(
        gl: &glow::Context,
        atlas_size: u32,
        max_instances: usize,
        internal_format: u32,
        format: u32,
        vs_src: &str,
        fs_src: &str,
        bpp: u32,
        label: &str,
    ) -> Self {
        let program = compile_program(gl, vs_src, fs_src, label);
        let loc_viewport = gl
            .get_uniform_location(program, "u_viewport")
            .expect("u_viewport uniform not found");
        let loc_atlas = gl
            .get_uniform_location(program, "u_atlas")
            .expect("u_atlas uniform not found");

        let texture = gl.create_texture().unwrap();
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
            glow::PixelUnpackData::Slice(Some(&vec![0u8; (atlas_size * atlas_size * bpp) as usize])),
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            if bpp == 1 {
                glow::NEAREST as i32
            } else {
                glow::LINEAR as i32
            },
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            if bpp == 1 {
                glow::NEAREST as i32
            } else {
                glow::LINEAR as i32
            },
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

        let vao = gl.create_vertex_array().unwrap();
        let instance_vbo = gl.create_buffer().unwrap();

        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(instance_vbo));
        gl.buffer_data_size(
            glow::ARRAY_BUFFER,
            (max_instances * std::mem::size_of::<GlyphInstance>()) as i32,
            glow::DYNAMIC_DRAW,
        );
        setup_glyph_vertex_attribs(gl);
        gl.bind_vertex_array(None);

        GlAtlasLayer {
            texture,
            program,
            vao,
            instance_vbo,
            atlas_size,
            bpp,
            loc_viewport,
            loc_atlas,
            max_instances,
        }
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
            let format = if self.bpp == 1 {
                glow::RED
            } else {
                glow::RGBA
            };
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

        let format = if self.bpp == 1 {
            glow::RED
        } else {
            glow::RGBA
        };

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

    unsafe fn render_scissored(
        &self,
        gl: &glow::Context,
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

        gl.use_program(Some(self.program));
        gl.uniform_2_f32(Some(&self.loc_viewport), viewport_w, viewport_h);

        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
        gl.uniform_1_i32(Some(&self.loc_atlas), 0);

        gl.bind_vertex_array(Some(self.vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.instance_vbo));
        let data = bytemuck::cast_slice(&instances[..count]);
        gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, data);

        gl.enable(glow::SCISSOR_TEST);

        for batch in batches {
            let start = batch.start.min(count);
            let end = batch.end.min(count);
            if start >= end || batch.w == 0 || batch.h == 0 {
                continue;
            }
            let sy = viewport_h_px.saturating_sub(batch.y + batch.h);
            gl.scissor(batch.x as i32, sy as i32, batch.w as i32, batch.h as i32);

            // Re-bind VAO with offset into the instance buffer
            let base_offset = start * std::mem::size_of::<GlyphInstance>();
            setup_glyph_vertex_attribs_offset(gl, base_offset as i32);

            gl.draw_arrays_instanced(
                glow::TRIANGLE_STRIP,
                0,
                4,
                (end - start) as i32,
            );
        }

        // Overlay (status bar, etc.) — no scissor clipping
        let overlay_start = overlay_start.min(count);
        if overlay_start < count {
            gl.scissor(
                0,
                0,
                viewport_w_px.max(1) as i32,
                viewport_h_px.max(1) as i32,
            );
            let base_offset = overlay_start * std::mem::size_of::<GlyphInstance>();
            setup_glyph_vertex_attribs_offset(gl, base_offset as i32);
            gl.draw_arrays_instanced(
                glow::TRIANGLE_STRIP,
                0,
                4,
                (count - overlay_start) as i32,
            );
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
    unsafe fn new(gl: &glow::Context, max_rects: usize) -> Self {
        let program = compile_program(gl, RECT_VS, RECT_FS, "rect");
        let loc_viewport = gl
            .get_uniform_location(program, "u_viewport")
            .expect("u_viewport uniform not found in rect shader");

        let vao = gl.create_vertex_array().unwrap();
        let instance_vbo = gl.create_buffer().unwrap();

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

        GlRectPipeline {
            program,
            vao,
            instance_vbo,
            loc_viewport,
            max_rects,
        }
    }

    unsafe fn render(
        &self,
        gl: &glow::Context,
        rects: &[Rect],
        viewport_w: f32,
        viewport_h: f32,
    ) {
        if rects.is_empty() {
            return;
        }
        let count = rects.len().min(self.max_rects);

        gl.use_program(Some(self.program));
        gl.uniform_2_f32(Some(&self.loc_viewport), viewport_w, viewport_h);

        gl.bind_vertex_array(Some(self.vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.instance_vbo));
        let data = bytemuck::cast_slice(&rects[..count]);
        gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, data);

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
    unsafe fn new(gl: &glow::Context, atlas_size: u32, max_instances: usize) -> Self {
        let alpha = GlAtlasLayer::new(
            gl,
            atlas_size,
            max_instances,
            glow::R8,
            glow::RED,
            GLYPH_VS,
            ALPHA_FS,
            1,
            "alpha_atlas",
        );
        let color = GlAtlasLayer::new(
            gl,
            atlas_size,
            max_instances,
            glow::SRGB8_ALPHA8,
            glow::RGBA,
            GLYPH_VS,
            COLOR_FS,
            4,
            "color_atlas",
        );
        GlyphAtlasGpu { alpha, color }
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
    pub fn new(window: Arc<Window>, render_config: &RenderConfig) -> Result<Self> {
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

        let display = unsafe {
            glutin::display::Display::new(
                raw_display_handle,
                glutin::display::DisplayApiPreference::Egl,
            )
            .map_err(|e| anyhow::anyhow!("EGL display creation failed: {e}"))?
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
        let interval = match render_config.present_mode.as_str() {
            "immediate" => SwapInterval::DontWait,
            "mailbox" => SwapInterval::DontWait,
            _ => SwapInterval::Wait(NonZeroU32::new(1).unwrap()),
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
                glow::SRC_ALPHA,
                glow::ONE_MINUS_SRC_ALPHA,
                glow::ONE,
                glow::ONE_MINUS_SRC_ALPHA,
            );
            gl.disable(glow::DEPTH_TEST);
            // Disable sRGB framebuffer conversion — our color values are already
            // in sRGB space (parsed from hex like #282C34), so we write them
            // directly without linear→sRGB re-encoding.
            gl.disable(glow::FRAMEBUFFER_SRGB);
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
        }

        let rects = unsafe { GlRectPipeline::new(&gl, render_config.max_rectangles) };

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
        let atlas_gpu = unsafe {
            GlyphAtlasGpu::new(&self.gl, cache.atlas_size, cache.max_instances)
        };
        (cache, atlas_gpu)
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
    ) {
        let vw = self.width as f32;
        let vh = self.height as f32;

        unsafe {
            self.gl.viewport(0, 0, self.width as i32, self.height as i32);

            // Flush pending glyph uploads
            let (mut ap, mut cp, ac, cc) = cache.take_pending();
            atlas_gpu.alpha.flush_uploads(&self.gl, &mut ap, ac);
            atlas_gpu.color.flush_uploads(&self.gl, &mut cp, cc);

            // Clear
            self.gl.clear_color(0.0, 0.0, 0.0, 1.0);
            self.gl.clear(glow::COLOR_BUFFER_BIT);

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
            self.rects.render(&self.gl, &all_bg, vw, vh);

            // 2. Alpha text glyphs (scissored)
            atlas_gpu.alpha.render_scissored(
                &self.gl,
                scene.glyphs,
                vw,
                vh,
                self.width,
                self.height,
                scene.glyph_batches,
                scene.pane_glyph_end,
            );

            // 3. Color emoji (scissored)
            atlas_gpu.color.render_scissored(
                &self.gl,
                scene.color_glyphs,
                vw,
                vh,
                self.width,
                self.height,
                scene.color_glyph_batches,
                scene.pane_color_glyph_end,
            );
        }

        // Present — on Wayland EGL this implicitly handles resize
        self.gl_surface
            .swap_buffers(&self.gl_context)
            .expect("swap_buffers failed");
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
) -> glow::Program {
    let vs = gl.create_shader(glow::VERTEX_SHADER).unwrap();
    gl.shader_source(vs, vs_src);
    gl.compile_shader(vs);
    if !gl.get_shader_compile_status(vs) {
        let log = gl.get_shader_info_log(vs);
        panic!("[{label}] vertex shader compile error: {log}");
    }

    let fs = gl.create_shader(glow::FRAGMENT_SHADER).unwrap();
    gl.shader_source(fs, fs_src);
    gl.compile_shader(fs);
    if !gl.get_shader_compile_status(fs) {
        let log = gl.get_shader_info_log(fs);
        panic!("[{label}] fragment shader compile error: {log}");
    }

    let program = gl.create_program().unwrap();
    gl.attach_shader(program, vs);
    gl.attach_shader(program, fs);
    gl.link_program(program);
    if !gl.get_program_link_status(program) {
        let log = gl.get_program_info_log(program);
        panic!("[{label}] program link error: {log}");
    }

    gl.delete_shader(vs);
    gl.delete_shader(fs);
    program
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
    frag_color = v_color;
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
    frag_color = vec4(v_color.rgb, v_color.a * alpha);
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
