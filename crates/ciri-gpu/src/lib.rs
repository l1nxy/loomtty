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

// DX binding layer: every function is inherently unsafe (D3D11 API).
#[cfg(all(feature = "dx", windows))]
#[allow(unsafe_op_in_unsafe_fn)]
pub mod dx;

use ciri_config::config::RenderConfig;

/// Structured error type for GPU operations.
///
/// Callers (e.g. ciri-app) can match on variants to decide whether to
/// fallback to another backend, show a user-facing message, or abort.
#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    #[error("shader compilation failed: {0}")]
    ShaderCompile(String),

    #[error("GPU device initialization failed: {0}")]
    DeviceInit(String),

    #[error("GPU resource creation failed: {0}")]
    ResourceCreate(String),

    #[error("surface lost or resize failed: {0}")]
    SurfaceLost(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, GpuError>;

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
use std::time::{Duration, Instant};
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

fn log_backend_init_error(backend: &str, error: &dyn std::fmt::Display) {
    match backend {
        DX_BACKEND => log::warn!("DX11 backend failed: {error:#}"),
        BLADE_BACKEND => log::warn!("blade backend failed: {error:#}"),
        GL_BACKEND => log::warn!("GL backend failed: {error:#}"),
        other => log::warn!("{other} backend failed: {error:#}"),
    }
}

#[derive(Default)]
pub(crate) struct DrawFrameTimings {
    pub cpu_upload: Duration,
    pub draw: Duration,
    pub sync_wait: Duration,
    pub present: Duration,
}

pub(crate) struct DrawFrameProfiler {
    backend: &'static str,
    started_at: Instant,
    timings: DrawFrameTimings,
}

impl DrawFrameProfiler {
    pub fn begin(backend: &'static str) -> Option<Self> {
        if !log::log_enabled!(log::Level::Debug) {
            return None;
        }
        Some(Self {
            backend,
            started_at: Instant::now(),
            timings: DrawFrameTimings::default(),
        })
    }

    pub fn record_cpu_upload(&mut self, started_at: Instant) {
        self.timings.cpu_upload += started_at.elapsed();
    }

    pub fn record_draw(&mut self, started_at: Instant) {
        self.timings.draw += started_at.elapsed();
    }

    pub fn record_sync_wait(&mut self, started_at: Instant) {
        self.timings.sync_wait += started_at.elapsed();
    }

    pub fn record_present(&mut self, started_at: Instant) {
        self.timings.present += started_at.elapsed();
    }

    pub fn finish(self, bg_rects: usize, glyphs: usize, color_glyphs: usize) {
        const FRAME_LOG_THRESHOLD: Duration = Duration::from_millis(2);

        let total = self.started_at.elapsed();
        if total < FRAME_LOG_THRESHOLD
            && self.timings.sync_wait < FRAME_LOG_THRESHOLD
            && self.timings.present < FRAME_LOG_THRESHOLD
        {
            return;
        }

        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        log::debug!(
            "draw_frame backend={} total={:.2}ms upload={:.2}ms draw={:.2}ms wait={:.2}ms present={:.2}ms bg={} glyph={} color={}",
            self.backend,
            ms(total),
            ms(self.timings.cpu_upload),
            ms(self.timings.draw),
            ms(self.timings.sync_wait),
            ms(self.timings.present),
            bg_rects,
            glyphs,
            color_glyphs,
        );
    }
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
                        return Err(GpuError::DeviceInit(format!("{DX_BACKEND}: {e}")));
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
                        return Err(GpuError::DeviceInit(format!("{BLADE_BACKEND}: {e}")));
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
                    return Err(GpuError::DeviceInit(format!(
                        "backend '{other}' not compiled into this build"
                    )));
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

        Err(GpuError::DeviceInit(format!(
            "no GPU backend available (tried: {})",
            choice.resolved
        )))
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
    ) -> Result<(GlyphCache, GlyphAtlasGpu)> {
        match self {
            #[cfg(feature = "blade")]
            Renderer::Blade(r) => {
                let (cache, atlas) = r.create_atlas(params);
                Ok((cache, GlyphAtlasGpu::Blade(atlas)))
            }
            #[cfg(feature = "gl")]
            Renderer::Gl(r) => {
                let (cache, atlas) = r.create_atlas(params)?;
                Ok((cache, GlyphAtlasGpu::Gl(atlas)))
            }
            #[cfg(all(feature = "dx", windows))]
            Renderer::Dx(r) => {
                let (cache, atlas) = r.create_atlas(params);
                Ok((cache, GlyphAtlasGpu::Dx(atlas)))
            }
        }
    }

    /// Upload an RGBA8 image to the renderer's overview-wallpaper slot.
    ///
    /// Replaces any previously-uploaded image. The renderer then draws this
    /// image as a fullscreen `cover`-fitted quad whenever
    /// `FrameScene::overview_bg_image_opacity > 0.0` (typically only in
    /// overview mode). Backends that haven't implemented the textured-quad
    /// pipeline yet silently no-op — overview mode falls back to a solid
    /// `clear_color` fill, so no visual breakage rolls out across backends.
    ///
    /// `width` and `height` are in image pixels; `rgba.len()` must equal
    /// `width * height * 4`. Caller-side image decoding lives in
    /// `ciri::app::overview_bg`.
    pub fn set_overview_background_image(
        &mut self,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<()> {
        match self {
            #[cfg(feature = "blade")]
            Renderer::Blade(_) => Ok(()),
            #[cfg(feature = "gl")]
            Renderer::Gl(r) => r.set_overview_background_image(rgba, width, height),
            #[cfg(all(feature = "dx", windows))]
            Renderer::Dx(r) => r
                .set_overview_background_image(rgba, width, height)
                .map_err(|e| GpuError::ResourceCreate(format!("dx overview bg: {e}"))),
        }
    }

    /// Drop the uploaded overview wallpaper, if any. Subsequent frames
    /// fall back to the solid `clear_color` fill.
    pub fn clear_overview_background_image(&mut self) {
        match self {
            #[cfg(feature = "blade")]
            Renderer::Blade(_) => {}
            #[cfg(feature = "gl")]
            Renderer::Gl(r) => r.clear_overview_background_image(),
            #[cfg(all(feature = "dx", windows))]
            Renderer::Dx(r) => r.clear_overview_background_image(),
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
    ) -> Result<()> {
        match (self, atlas_gpu) {
            #[cfg(feature = "blade")]
            (Renderer::Blade(r), GlyphAtlasGpu::Blade(a)) => {
                r.draw_frame(a, cache, scene);
                Ok(())
            }
            #[cfg(feature = "gl")]
            (Renderer::Gl(r), GlyphAtlasGpu::Gl(a)) => r.draw_frame(a, cache, scene),
            #[cfg(all(feature = "dx", windows))]
            (Renderer::Dx(r), GlyphAtlasGpu::Dx(a)) => {
                r.draw_frame(a, cache, scene);
                Ok(())
            }
            _ => {
                log_backend_mismatch(BackendMismatchOp::DrawFrame);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AUTO_BACKEND, BLADE_BACKEND, BackendChoice, BackendMismatchOp, BackendRequest, DX_BACKEND,
        GL_BACKEND, default_backend_for_platform,
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

    fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
        let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    fn sdf_rounded_box(p: [f32; 2], b: [f32; 2], radii: [f32; 4]) -> f32 {
        let rx = if p[0] > 0.0 { radii[1] } else { radii[0] };
        let bx = if p[0] > 0.0 { radii[2] } else { radii[3] };
        let radius = if p[1] > 0.0 { bx } else { rx };
        let q = [p[0].abs() - b[0] + radius, p[1].abs() - b[1] + radius];
        q[0].max(q[1]).min(0.0) + q[0].max(0.0).hypot(q[1].max(0.0)) - radius
    }

    fn corner_alpha(px: [f32; 2], size: [f32; 2], radii: [f32; 4]) -> f32 {
        if radii.iter().all(|r| *r <= 0.0) {
            return 1.0;
        }
        let d = sdf_rounded_box(
            [px[0] - 0.5 * size[0], px[1] - 0.5 * size[1]],
            [0.5 * size[0], 0.5 * size[1]],
            radii,
        );
        1.0 - smoothstep(-0.5, 0.5, d)
    }

    #[test]
    fn corner_alpha_all_zero_radii_is_noop() {
        assert_eq!(corner_alpha([0.0, 0.0], [100.0, 60.0], [0.0; 4]), 1.0);
        assert_eq!(corner_alpha([99.0, 59.0], [100.0, 60.0], [0.0; 4]), 1.0);
    }

    #[test]
    fn corner_alpha_clips_pixel_outside_corner_arc() {
        let alpha = corner_alpha([0.0, 0.0], [100.0, 60.0], [12.0; 4]);
        assert!(alpha <= 0.001, "alpha={alpha}");
    }

    #[test]
    fn corner_alpha_arc_center_is_smoothstep_midpoint() {
        let alpha = corner_alpha([12.0, 0.0], [100.0, 60.0], [12.0; 4]);
        assert!((alpha - 0.5).abs() <= 0.001, "alpha={alpha}");
    }

    #[test]
    fn corner_alpha_rectangular_interior_is_opaque() {
        let alpha = corner_alpha([50.0, 30.0], [100.0, 60.0], [12.0; 4]);
        assert!((alpha - 1.0).abs() <= 0.001, "alpha={alpha}");
    }

    #[test]
    fn corner_alpha_zero_radius_corner_is_not_clipped() {
        let alpha = corner_alpha([99.0, 1.0], [100.0, 60.0], [12.0, 0.0, 0.0, 0.0]);
        assert!((alpha - 1.0).abs() <= 0.001, "alpha={alpha}");
    }

    #[test]
    /// Negative radii hit the shader's all-zero short-circuit only when every
    /// radius is negative. Mixed-sign radii still enter SDF math and can produce
    /// invalid output; production relies on config validation to reject them.
    fn corner_alpha_negative_radii_all_negative_hit_short_circuit() {
        assert_eq!(
            corner_alpha([0.0, 0.0], [100.0, 60.0], [-8.0, -1.0, -4.0, -2.0]),
            1.0
        );
    }

    #[test]
    fn corner_alpha_overlarge_radii_stay_finite() {
        let alpha = corner_alpha([10.0, 10.0], [32.0, 20.0], [64.0; 4]);
        assert!(alpha.is_finite(), "alpha={alpha}");
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
                ui_family_name: None,
                primary_font_path: None,
                emoji_font_path: None,
                emoji_font_id: None,
                cjk_font_path: None,
                cjk_font_id: None,
                ui_font_path: None,
                ui_font_id: None,
                ui_pixel_size: None,
                render_config: &render_config,
                font_resolver: std::sync::Arc::new(ciri_render::font_resolver::CmapResolver::new(
                    (&[], 0),
                    None,
                    None,
                )),
                #[cfg(windows)]
                dwrite_resolver: None,
                cell_width_scale: None,
                cell_height_scale: None,
            });
        let scene = ciri_render::FrameScene {
            clear_color: [0.0, 0.0, 0.0, 1.0],
            overview_bg_image_opacity: 0.0,
            bg_rects: &[],
            bg_rect_ranges: &[],
            glyphs: &[],
            color_glyphs: &[],
            glyph_batches: &[],
            color_glyph_batches: &[],
            active_bg_start: 0,
            active_glyph_batches: &[],
            active_color_glyph_batches: &[],
            pane_glyph_end: 0,
            pane_color_glyph_end: 0,
            overlay_bg_start: 0,
            sdf_rects: &[],
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = renderer.draw_frame(&mut atlas, &mut cache, scene);
        }));
        std::mem::forget(renderer);
        std::mem::forget(atlas);
        assert!(result.is_ok(), "draw_frame mismatch path should not panic");
    }
}
