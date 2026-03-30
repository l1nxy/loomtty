//! GPU backend crate for ciri terminal renderer.
//!
//! Provides runtime backend selection via config (`render.backend`):
//! - **"auto"** (default): tries blade (Vulkan), falls back to GL
//! - **"blade"**: Vulkan on Linux, Metal on macOS
//! - **"gl"**: OpenGL 3.3+ / EGL — smooth Wayland resize
//! - **"dx"**: Direct3D 11 — native Windows backend

#[cfg(feature = "blade")]
pub mod blade;

// GL binding layer: every function is inherently unsafe (glow API).
#[cfg(feature = "gl")]
#[allow(unsafe_op_in_unsafe_fn)]
pub mod gl;

#[cfg(all(feature = "dx", windows))]
pub mod dx;

use anyhow::Result;
use ciri_config::config::RenderConfig;

/// Viewport dimensions used by render functions.
pub struct ViewportDims {
    pub width: f32,
    pub height: f32,
    pub width_px: u32,
    pub height_px: u32,
}
use ciri_render::FrameScene;
use ciri_render::glyph_cache::GlyphCache;
use std::sync::Arc;
use winit::window::Window;

const AUTO_BACKEND: &str = "auto";
const BLADE_BACKEND: &str = "blade";
const GL_BACKEND: &str = "gl";
const DX_BACKEND: &str = "dx";

/// Runtime-selected GPU atlas.
#[allow(clippy::large_enum_variant)]
pub enum GlyphAtlasGpu {
    #[cfg(feature = "blade")]
    Blade(blade::GlyphAtlasGpu),
    #[cfg(feature = "gl")]
    Gl(gl::GlyphAtlasGpu),
    #[cfg(all(feature = "dx", windows))]
    Dx(dx::GlyphAtlasGpu),
}

/// Runtime-selected GPU renderer.
#[allow(clippy::large_enum_variant)]
pub enum Renderer {
    #[cfg(feature = "blade")]
    Blade(blade::Renderer),
    #[cfg(feature = "gl")]
    Gl(gl::Renderer),
    #[cfg(all(feature = "dx", windows))]
    Dx(dx::Renderer),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BackendMismatchOp {
    DestroyAtlas,
    DrawFrame,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BackendRequest<'a> {
    Auto,
    Explicit(&'a str),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BackendChoice<'a> {
    requested: BackendRequest<'a>,
    resolved: &'a str,
}

impl<'a> BackendChoice<'a> {
    fn from_config(backend: &'a str) -> Self {
        let requested = if backend.is_empty() || backend == AUTO_BACKEND {
            BackendRequest::Auto
        } else {
            BackendRequest::Explicit(backend)
        };

        let resolved = match requested {
            BackendRequest::Auto => default_backend_for_platform(),
            BackendRequest::Explicit("vulkan") => BLADE_BACKEND,
            BackendRequest::Explicit(other) => other,
        };

        Self {
            requested,
            resolved,
        }
    }

    fn is_explicit(self) -> bool {
        matches!(self.requested, BackendRequest::Explicit(_))
    }
}

fn default_backend_for_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        BLADE_BACKEND
    } else if cfg!(windows) {
        if cfg!(feature = "dx") {
            DX_BACKEND
        } else {
            GL_BACKEND
        }
    } else {
        GL_BACKEND
    }
}

fn log_backend_init_error(backend: &str, error: &anyhow::Error) {
    match backend {
        DX_BACKEND => log::warn!("DX11 backend failed: {error:#}"),
        BLADE_BACKEND => log::warn!("blade backend failed: {error:#}"),
        GL_BACKEND => log::warn!("GL backend failed: {error:#}"),
        other => log::warn!("{other} backend failed: {error:#}"),
    }
}

fn backend_not_compiled_error(backend: &str) -> anyhow::Error {
    anyhow::anyhow!("backend {backend:?} not compiled (available features: blade, gl, dx)")
}

pub(crate) fn log_backend_mismatch(op: BackendMismatchOp) {
    match op {
        BackendMismatchOp::DestroyAtlas => {
            log::error!("renderer/atlas backend mismatch in destroy_atlas")
        }
        BackendMismatchOp::DrawFrame => {
            log::error!("renderer/atlas backend mismatch in draw_frame")
        }
    }
}

impl Renderer {
    /// Create a renderer with the backend specified in config.
    ///
    /// `"auto"` (default) picks the most stable native API per platform:
    /// - Linux: GL (EGL/Wayland native, most mature drivers)
    /// - macOS: blade (Metal)
    /// - Windows: DX11 (when compiled with `dx` feature)
    ///
    /// Explicit values: `"gl"`, `"vulkan"` / `"blade"`, `"dx"`.
    pub fn new(window: Arc<Window>, render_config: &RenderConfig) -> Result<Self> {
        let backend_str = match render_config.backend {
            ciri_config::config::RenderBackend::Auto => "auto",
            ciri_config::config::RenderBackend::Blade => "blade",
            ciri_config::config::RenderBackend::Gl => "gl",
        };
        let choice = BackendChoice::from_config(backend_str);

        // Try requested backend
        match choice.resolved {
            #[cfg(all(feature = "dx", windows))]
            DX_BACKEND => match dx::Renderer::new(window.clone(), render_config) {
                Ok(r) => {
                    log::info!("using DX11 (Direct3D 11) backend");
                    return Ok(Renderer::Dx(r));
                }
                Err(e) => {
                    log_backend_init_error(DX_BACKEND, &e);
                    if choice.is_explicit() {
                        return Err(e);
                    }
                }
            },
            #[cfg(feature = "blade")]
            BLADE_BACKEND => match blade::Renderer::new(window.clone(), render_config) {
                Ok(r) => {
                    log::info!("using blade (Vulkan/Metal) backend");
                    return Ok(Renderer::Blade(r));
                }
                Err(e) => {
                    log_backend_init_error(BLADE_BACKEND, &e);
                    if choice.is_explicit() {
                        return Err(e);
                    }
                }
            },
            #[cfg(feature = "gl")]
            GL_BACKEND => match gl::Renderer::new(window.clone(), render_config) {
                Ok(r) => {
                    log::info!("using GL (OpenGL/EGL) backend");
                    return Ok(Renderer::Gl(r));
                }
                Err(e) => {
                    log_backend_init_error(GL_BACKEND, &e);
                    if choice.is_explicit() {
                        return Err(e);
                    }
                }
            },
            other => {
                if choice.is_explicit() {
                    return Err(backend_not_compiled_error(other));
                }
            }
        }

        // Auto fallback: try all compiled backends
        #[cfg(all(feature = "dx", windows))]
        if let Ok(r) = dx::Renderer::new(window.clone(), render_config) {
            log::info!("fallback: using DX11 backend");
            return Ok(Renderer::Dx(r));
        }
        #[cfg(feature = "gl")]
        if let Ok(r) = gl::Renderer::new(window.clone(), render_config) {
            log::info!("fallback: using GL backend");
            return Ok(Renderer::Gl(r));
        }
        #[cfg(feature = "blade")]
        if let Ok(r) = blade::Renderer::new(window.clone(), render_config) {
            log::info!("fallback: using blade backend");
            return Ok(Renderer::Blade(r));
        }

        anyhow::bail!("no GPU backend available (tried: {})", choice.resolved)
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        match self {
            #[cfg(feature = "blade")]
            Renderer::Blade(r) => r.resize(width, height),
            #[cfg(feature = "gl")]
            Renderer::Gl(r) => r.resize(width, height),
            #[cfg(all(feature = "dx", windows))]
            Renderer::Dx(r) => r.resize(width, height),
        }
    }

    pub fn apply_surface(&mut self) {
        match self {
            #[cfg(feature = "blade")]
            Renderer::Blade(r) => r.apply_surface(),
            #[cfg(feature = "gl")]
            Renderer::Gl(r) => r.apply_surface(),
            #[cfg(all(feature = "dx", windows))]
            Renderer::Dx(r) => r.apply_surface(),
        }
    }

    pub fn surface_size(&self) -> (u32, u32) {
        match self {
            #[cfg(feature = "blade")]
            Renderer::Blade(r) => r.surface_size(),
            #[cfg(feature = "gl")]
            Renderer::Gl(r) => r.surface_size(),
            #[cfg(all(feature = "dx", windows))]
            Renderer::Dx(r) => r.surface_size(),
        }
    }

    pub fn create_atlas(
        &mut self,
        params: &ciri_render::glyph_cache::FontInitParams,
    ) -> (GlyphCache, GlyphAtlasGpu) {
        match self {
            #[cfg(feature = "blade")]
            Renderer::Blade(r) => {
                let (cache, atlas) = r.create_atlas(params);
                (cache, GlyphAtlasGpu::Blade(atlas))
            }
            #[cfg(feature = "gl")]
            Renderer::Gl(r) => {
                let (cache, atlas) = r.create_atlas(params);
                (cache, GlyphAtlasGpu::Gl(atlas))
            }
            #[cfg(all(feature = "dx", windows))]
            Renderer::Dx(r) => {
                let (cache, atlas) = r.create_atlas(params);
                (cache, GlyphAtlasGpu::Dx(atlas))
            }
        }
    }

    pub fn destroy_atlas(&self, atlas_gpu: &mut GlyphAtlasGpu) {
        match (self, atlas_gpu) {
            #[cfg(feature = "blade")]
            (Renderer::Blade(r), GlyphAtlasGpu::Blade(a)) => r.destroy_atlas(a),
            #[cfg(feature = "gl")]
            (Renderer::Gl(r), GlyphAtlasGpu::Gl(a)) => r.destroy_atlas(a),
            #[cfg(all(feature = "dx", windows))]
            (Renderer::Dx(r), GlyphAtlasGpu::Dx(a)) => r.destroy_atlas(a),
            _ => log_backend_mismatch(BackendMismatchOp::DestroyAtlas),
        }
    }

    pub fn draw_frame(
        &mut self,
        atlas_gpu: &mut GlyphAtlasGpu,
        cache: &mut GlyphCache,
        scene: FrameScene,
    ) {
        match (self, atlas_gpu) {
            #[cfg(feature = "blade")]
            (Renderer::Blade(r), GlyphAtlasGpu::Blade(a)) => r.draw_frame(a, cache, scene),
            #[cfg(feature = "gl")]
            (Renderer::Gl(r), GlyphAtlasGpu::Gl(a)) => r.draw_frame(a, cache, scene),
            #[cfg(all(feature = "dx", windows))]
            (Renderer::Dx(r), GlyphAtlasGpu::Dx(a)) => r.draw_frame(a, cache, scene),
            _ => log_backend_mismatch(BackendMismatchOp::DrawFrame),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AUTO_BACKEND, BLADE_BACKEND, BackendChoice, BackendMismatchOp, BackendRequest, DX_BACKEND,
        GL_BACKEND, backend_not_compiled_error, default_backend_for_platform,
    };
    #[cfg(all(feature = "blade", feature = "gl"))]
    use crate::{GlyphAtlasGpu, Renderer};

    #[cfg(all(feature = "blade", feature = "gl"))]
    #[allow(invalid_value)]
    fn backend_mismatch_renderer_and_atlas() -> (Renderer, GlyphAtlasGpu) {
        // Safety: these values are never read — only used to test backend-mismatch
        // error paths, and are immediately forgotten via `mem::forget`.
        let renderer = Renderer::Blade(unsafe { std::mem::MaybeUninit::zeroed().assume_init() });
        let atlas = GlyphAtlasGpu::Gl(unsafe { std::mem::MaybeUninit::zeroed().assume_init() });
        (renderer, atlas)
    }

    #[test]
    fn auto_backend_uses_platform_default() {
        let choice = BackendChoice::from_config(AUTO_BACKEND);
        assert_eq!(choice.requested, BackendRequest::Auto);
        assert_eq!(choice.resolved, default_backend_for_platform());
        assert!(!choice.is_explicit());
    }

    #[test]
    fn empty_backend_uses_platform_default() {
        let choice = BackendChoice::from_config("");
        assert_eq!(choice.requested, BackendRequest::Auto);
        assert_eq!(choice.resolved, default_backend_for_platform());
    }

    #[test]
    fn vulkan_alias_resolves_to_blade() {
        let choice = BackendChoice::from_config("vulkan");
        assert_eq!(choice.requested, BackendRequest::Explicit("vulkan"));
        assert_eq!(choice.resolved, BLADE_BACKEND);
        assert!(choice.is_explicit());
    }

    #[test]
    fn explicit_backend_is_preserved() {
        for backend in [BLADE_BACKEND, GL_BACKEND, DX_BACKEND, "custom"] {
            let choice = BackendChoice::from_config(backend);
            assert_eq!(choice.requested, BackendRequest::Explicit(backend));
            assert_eq!(choice.resolved, backend);
            assert!(choice.is_explicit());
        }
    }

    #[test]
    fn backend_not_compiled_error_mentions_backend_name() {
        let error = backend_not_compiled_error("mystery");
        assert!(
            error
                .to_string()
                .contains("backend \"mystery\" not compiled"),
            "unexpected error: {error:#}"
        );
    }

    #[test]
    fn backend_mismatch_ops_are_distinct() {
        assert_ne!(
            BackendMismatchOp::DestroyAtlas,
            BackendMismatchOp::DrawFrame
        );
    }

    #[cfg(all(feature = "blade", feature = "gl"))]
    #[test]
    fn destroy_atlas_backend_mismatch_is_safe() {
        let (renderer, mut atlas) = backend_mismatch_renderer_and_atlas();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            renderer.destroy_atlas(&mut atlas);
        }));
        std::mem::forget(renderer);
        std::mem::forget(atlas);
        assert!(
            result.is_ok(),
            "destroy_atlas mismatch path should not panic"
        );
    }

    #[cfg(all(feature = "blade", feature = "gl"))]
    #[test]
    fn draw_frame_backend_mismatch_is_safe() {
        let (mut renderer, mut atlas) = backend_mismatch_renderer_and_atlas();
        let render_config = ciri_config::config::RenderConfig::default();
        let mut cache =
            ciri_render::glyph_cache::GlyphCache::new(&ciri_render::glyph_cache::FontInitParams {
                font_size_pt: 32.0,
                dpi_scale: 1.0,
                family_name: "monospace",
                primary_font_path: None,
                emoji_font_path: None,
                emoji_font_id: None,
                cjk_font_path: None,
                cjk_font_id: None,
                render_config: &render_config,
            });
        let scene = ciri_render::FrameScene {
            clear_color: [0.0, 0.0, 0.0, 1.0],
            bg_rects: &[],
            glyphs: &[],
            color_glyphs: &[],
            glyph_batches: &[],
            color_glyph_batches: &[],
            pane_glyph_end: 0,
            pane_color_glyph_end: 0,
            overlay_bg_start: 0,
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            renderer.draw_frame(&mut atlas, &mut cache, scene);
        }));
        std::mem::forget(renderer);
        std::mem::forget(atlas);
        assert!(result.is_ok(), "draw_frame mismatch path should not panic");
    }
}
