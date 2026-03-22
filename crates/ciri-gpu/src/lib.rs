//! GPU backend crate for ciri terminal renderer.
//!
//! Provides runtime backend selection via config (`render.backend`):
//! - **"auto"** (default): tries blade (Vulkan), falls back to GL
//! - **"blade"**: Vulkan on Linux, Metal on macOS
//! - **"gl"**: OpenGL 3.3+ / EGL — smooth Wayland resize
//! - **"dx"**: Direct3D 11 — native Windows backend

#![allow(unsafe_op_in_unsafe_fn)]

#[cfg(feature = "blade")]
pub mod blade;

#[cfg(feature = "gl")]
pub mod gl;

#[cfg(all(feature = "dx", windows))]
pub mod dx;

use anyhow::Result;
use ciri_config::config::RenderConfig;
use ciri_render::glyph_cache::GlyphCache;
use ciri_render::FrameScene;
use std::sync::Arc;
use winit::window::Window;

/// Runtime-selected GPU atlas.
pub enum GlyphAtlasGpu {
    #[cfg(feature = "blade")]
    Blade(blade::GlyphAtlasGpu),
    #[cfg(feature = "gl")]
    Gl(gl::GlyphAtlasGpu),
}

/// Runtime-selected GPU renderer.
pub enum Renderer {
    #[cfg(feature = "blade")]
    Blade(blade::Renderer),
    #[cfg(feature = "gl")]
    Gl(gl::Renderer),
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
        let backend = render_config.backend.as_str();

        // Resolve "auto" to the platform-native default
        let resolved = if backend == "auto" || backend.is_empty() {
            if cfg!(target_os = "macos") {
                "blade" // Metal via blade
            } else if cfg!(windows) {
                if cfg!(feature = "dx") { "dx" } else { "gl" }
            } else {
                "gl" // Linux: GL is the safest default
            }
        } else {
            backend
        };

        // Normalize "vulkan" → "blade"
        let resolved = if resolved == "vulkan" { "blade" } else { resolved };

        // Try requested backend
        match resolved {
            #[cfg(feature = "blade")]
            "blade" => {
                match blade::Renderer::new(window.clone(), render_config) {
                    Ok(r) => {
                        log::info!("using blade (Vulkan/Metal) backend");
                        return Ok(Renderer::Blade(r));
                    }
                    Err(e) => {
                        log::warn!("blade backend failed: {e:#}");
                        if backend != "auto" && !backend.is_empty() {
                            return Err(e);
                        }
                    }
                }
            }
            #[cfg(feature = "gl")]
            "gl" => {
                match gl::Renderer::new(window.clone(), render_config) {
                    Ok(r) => {
                        log::info!("using GL (OpenGL/EGL) backend");
                        return Ok(Renderer::Gl(r));
                    }
                    Err(e) => {
                        log::warn!("GL backend failed: {e:#}");
                        if backend != "auto" && !backend.is_empty() {
                            return Err(e);
                        }
                    }
                }
            }
            other => {
                if backend != "auto" && !backend.is_empty() {
                    anyhow::bail!("backend {other:?} not compiled (available features: blade, gl, dx)");
                }
            }
        }

        // Auto fallback: try all compiled backends
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

        anyhow::bail!("no GPU backend available (tried: {resolved})")
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        match self {
            #[cfg(feature = "blade")]
            Renderer::Blade(r) => r.resize(width, height),
            #[cfg(feature = "gl")]
            Renderer::Gl(r) => r.resize(width, height),
        }
    }

    pub fn apply_surface(&mut self) {
        match self {
            #[cfg(feature = "blade")]
            Renderer::Blade(r) => r.apply_surface(),
            #[cfg(feature = "gl")]
            Renderer::Gl(r) => r.apply_surface(),
        }
    }

    pub fn surface_size(&self) -> (u32, u32) {
        match self {
            #[cfg(feature = "blade")]
            Renderer::Blade(r) => r.surface_size(),
            #[cfg(feature = "gl")]
            Renderer::Gl(r) => r.surface_size(),
        }
    }

    pub fn create_atlas(
        &mut self,
        font_size_pt: f32,
        dpi_scale: f64,
        family_name: &str,
        primary_font_path: Option<(String, u32)>,
        emoji_font_path: Option<(String, u32)>,
        emoji_font_id: Option<fontdb::ID>,
        render_config: &RenderConfig,
    ) -> (GlyphCache, GlyphAtlasGpu) {
        match self {
            #[cfg(feature = "blade")]
            Renderer::Blade(r) => {
                let (cache, atlas) =
                    r.create_atlas(font_size_pt, dpi_scale, family_name, primary_font_path, emoji_font_path, emoji_font_id, render_config);
                (cache, GlyphAtlasGpu::Blade(atlas))
            }
            #[cfg(feature = "gl")]
            Renderer::Gl(r) => {
                let (cache, atlas) =
                    r.create_atlas(font_size_pt, dpi_scale, family_name, primary_font_path, emoji_font_path, emoji_font_id, render_config);
                (cache, GlyphAtlasGpu::Gl(atlas))
            }
        }
    }

    pub fn destroy_atlas(&self, atlas_gpu: &mut GlyphAtlasGpu) {
        match (self, atlas_gpu) {
            #[cfg(feature = "blade")]
            (Renderer::Blade(r), GlyphAtlasGpu::Blade(a)) => r.destroy_atlas(a),
            #[cfg(feature = "gl")]
            (Renderer::Gl(r), GlyphAtlasGpu::Gl(a)) => r.destroy_atlas(a),
            _ => log::error!("renderer/atlas backend mismatch in destroy_atlas"),
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
            _ => log::error!("renderer/atlas backend mismatch in draw_frame"),
        }
    }
}
