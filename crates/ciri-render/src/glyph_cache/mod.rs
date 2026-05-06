//! CPU glyph cache — rasterizes and caches terminal text glyphs.
//!
//! Two atlas layers:
//! - **Text** (`R8Unorm`): grayscale alpha mask
//! - **Color** (`Rgba8Srgb`): color emoji
//!
//! Platform backends: FreeType (Linux), CoreText (macOS), DirectWrite (Windows).
//! The GPU upload/rendering is handled by the backend's `GlyphAtlasGpu`.

mod atlas;
#[cfg(target_os = "linux")]
mod cjk;
#[cfg(target_os = "macos")]
mod cjk_coretext;
#[cfg(target_os = "linux")]
mod metrics;
#[cfg(target_os = "macos")]
mod metrics_coretext;
mod rasterize;
#[cfg(target_os = "macos")]
mod rasterize_coretext;
#[cfg(windows)]
mod rasterize_dwrite;
pub mod types;

#[cfg(windows)]
pub use atlas::PendingDwriteGlyph;
pub use atlas::PendingUpload;
pub(crate) use atlas::ShelfPacker;
pub use types::{FontStyle, GlyphEntry, GlyphInstance, PaneGlyphRange};

#[cfg(target_os = "linux")]
use cjk::compute_cjk_pixel_size;
#[cfg(target_os = "macos")]
use cjk_coretext::compute_cjk_pixel_size_ct;
#[cfg(target_os = "linux")]
use metrics::compute_ft_metrics;
#[cfg(target_os = "macos")]
use metrics_coretext::compute_ct_metrics;
#[cfg(windows)]
use rasterize::cache_measured_dwrite_glyph;
use rasterize::{RasterizedGlyph, cache_rasterized_glyph};
#[cfg(target_os = "linux")]
use rasterize::{convert_crossfont_glyph, rasterize_glyph_id_ft};
#[cfg(target_os = "macos")]
use rasterize_coretext::CoreTextRasterizer;
use types::FontClass;
#[cfg(target_os = "linux")]
use types::FontKeySet;

#[cfg(windows)]
use rasterize_dwrite::{DWriteRasterizer, compute_cjk_pixel_size_dwrite, compute_dwrite_metrics};
#[cfg(windows)]
use windows::Win32::Graphics::DirectWrite::IDWriteFontFace;

use crate::font_resolver::FontResolver;
#[cfg(any(target_os = "macos", windows))]
use crate::font_resolver::ResolvedFont;

/// Given a preferred font, return the full fallback order (preferred first).
#[cfg(any(target_os = "macos", windows))]
fn resolved_font_order(preferred: ResolvedFont) -> [ResolvedFont; 3] {
    match preferred {
        ResolvedFont::Primary => [
            ResolvedFont::Primary,
            ResolvedFont::Cjk,
            ResolvedFont::Emoji,
        ],
        ResolvedFont::Cjk => [
            ResolvedFont::Cjk,
            ResolvedFont::Primary,
            ResolvedFont::Emoji,
        ],
        ResolvedFont::Emoji => [
            ResolvedFont::Emoji,
            ResolvedFont::Primary,
            ResolvedFont::Cjk,
        ],
    }
}
use ciri_config::config::RenderConfig;
#[cfg(target_os = "linux")]
use crossfont::{FontDesc, GlyphKey, Rasterize, Rasterizer, Size, Slant, Style, Weight};
#[cfg(target_os = "linux")]
use freetype::Library as FtLibrary;
use std::collections::HashMap;
use std::sync::Arc;

// ─── Font init params ───────────────────────────────────────────────

/// Parameters for initializing the glyph cache and font pipeline.
pub struct FontInitParams<'a> {
    pub font_size_pt: f32,
    pub dpi_scale: f64,
    pub family_name: &'a str,
    pub ui_family_name: Option<&'a str>,
    pub primary_font_path: Option<(String, u32)>,
    pub emoji_font_path: Option<(String, u32)>,
    pub emoji_font_id: Option<fontdb::ID>,
    pub cjk_font_path: Option<(String, u32)>,
    pub cjk_font_id: Option<fontdb::ID>,
    /// UI font (optional). `None` → UI text reuses the primary font path.
    pub ui_font_path: Option<(String, u32)>,
    pub ui_font_id: Option<fontdb::ID>,
    /// UI font pixel size (pre-computed from UiFontConfig.size × dpi_scale).
    /// Ignored if `ui_font_path` is `None`.
    pub ui_pixel_size: Option<f32>,
    pub render_config: &'a RenderConfig,
    /// Shared font resolver for determining font fallback order.
    pub font_resolver: Arc<dyn FontResolver>,
    /// DWrite resolver for system font face access (Windows only).
    #[cfg(windows)]
    pub dwrite_resolver: Option<Arc<crate::font_resolver::DWriteResolver>>,
    /// Optional multiplier applied to the platform-computed cell width.
    /// `None` (or any value `<= 0`) keeps the original metrics — the
    /// historical default — so existing callers don't need to opt in.
    #[doc(hidden)]
    pub cell_width_scale: Option<f32>,
    /// Optional multiplier applied to the platform-computed cell height.
    /// `None` keeps the original metrics.
    #[doc(hidden)]
    pub cell_height_scale: Option<f32>,
}

impl<'a> FontInitParams<'a> {
    /// Resolve `cell_width_scale`, falling back to `1.0` for the historical
    /// default behavior.
    fn cell_width_scale_or_default(&self) -> f32 {
        match self.cell_width_scale {
            Some(s) if s.is_finite() && s > 0.0 => s,
            _ => 1.0,
        }
    }

    /// Resolve `cell_height_scale`, falling back to `1.0`.
    fn cell_height_scale_or_default(&self) -> f32 {
        match self.cell_height_scale {
            Some(s) if s.is_finite() && s > 0.0 => s,
            _ => 1.0,
        }
    }
}

// ─── Glyph cache (CPU) ──────────────────────────────────────────────

/// CPU-side glyph cache: rasterization, packing, and caching.
/// GPU upload/rendering is delegated to backend's `GlyphAtlasGpu`.
///
/// Platform-specific rasterization: FreeType+crossfont on Linux,
/// CoreText+Core Graphics on macOS, DirectWrite on Windows.
pub struct GlyphCache {
    // Packing
    pub(crate) alpha_packer: ShelfPacker,
    pub(crate) color_packer: ShelfPacker,
    alpha_pending: Vec<PendingUpload>,
    color_pending: Vec<PendingUpload>,
    alpha_pending_clear: bool,
    color_pending_clear: bool,
    pub atlas_size: u32,
    pub max_instances: usize,
    // Caching
    cache: HashMap<(char, FontStyle), GlyphEntry>,
    glyph_id_cache: HashMap<(u32, FontClass, FontStyle, bool), GlyphEntry>,

    // ── Platform-specific rasterizer state ──

    // FreeType + crossfont (Linux)
    #[cfg(target_os = "linux")]
    rasterizer: Rasterizer,
    #[cfg(target_os = "linux")]
    font_keys: FontKeySet,
    #[cfg(target_os = "linux")]
    font_size: Size,
    #[cfg(target_os = "linux")]
    _ft_library: FtLibrary,
    #[cfg(target_os = "linux")]
    ft_face: Option<freetype::Face>,
    #[cfg(target_os = "linux")]
    emoji_ft_face: Option<freetype::Face>,
    #[cfg(target_os = "linux")]
    cjk_ft_face: Option<freetype::Face>,
    #[cfg(target_os = "linux")]
    ui_ft_face: Option<freetype::Face>,

    // CoreText (macOS)
    #[cfg(target_os = "macos")]
    coretext: CoreTextRasterizer,
    #[cfg(target_os = "macos")]
    ui_ct_font: Option<core_text::font::CTFont>,

    // DirectWrite (Windows)
    #[cfg(windows)]
    dwrite: DWriteRasterizer,
    #[cfg(windows)]
    dwrite_alpha_pending: Vec<PendingDwriteGlyph>,
    #[cfg(windows)]
    dwrite_color_pending: Vec<PendingDwriteGlyph>,
    /// When true, skip CPU rasterization and queue DWrite render commands
    /// for the DX backend to execute via D2D DrawGlyphRun.
    #[cfg(windows)]
    use_d2d_rendering: bool,

    // ── Common state ──
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    font_resolver: Arc<dyn FontResolver>,
    #[cfg(windows)]
    dwrite_resolver: Option<Arc<crate::font_resolver::DWriteResolver>>,
    emoji_font_id: Option<fontdb::ID>,
    cjk_font_id: Option<fontdb::ID>,
    ui_font_id: Option<fontdb::ID>,
    pixel_size: f32,
    /// CJK font pixel size, adjusted so that "水" advance matches 2 * cell_width.
    cjk_pixel_size: f32,
    /// UI font pixel size. Falls back to `pixel_size` when no UI font is set.
    ui_pixel_size: f32,
    // Public metrics
    pub cell_width: f32,
    pub cell_height: f32,
    pub ascent: f32,
    /// Unrounded face advance width — for centering compensation when cell_width > face_width.
    pub face_width: f32,
    pub atlas_needs_clear: bool,
}

impl GlyphCache {
    /// Create a new self-contained GlyphCache.
    ///
    /// `primary_font_path` is the file path + face index for the thin FreeType
    /// path used by `ensure_glyph_id()`. Obtained from `TextShaper::primary_font_path()`.
    pub fn new(params: &FontInitParams) -> Self {
        let atlas_size = params.render_config.atlas_size;
        let max_instances = params.render_config.max_glyph_instances;
        let pixel_size = params.font_size_pt * (96.0 * params.dpi_scale as f32) / 72.0;

        // ── Platform-specific init ──

        #[cfg(target_os = "linux")]
        let (
            rasterizer,
            font_keys,
            font_size,
            _ft_library,
            ft_face,
            emoji_ft_face,
            cjk_ft_face,
            ui_ft_face,
            cell_width,
            cell_height,
            ascent,
            face_width,
            cjk_pixel_size,
        ) = {
            // Crossfont setup
            let mut rasterizer = Rasterizer::new().expect("crossfont init failed");
            let font_size = Size::from_px(pixel_size);

            let regular_desc = FontDesc::new(
                params.family_name,
                Style::Description {
                    slant: Slant::Normal,
                    weight: Weight::Normal,
                },
            );
            let regular_key = rasterizer
                .load_font(&regular_desc, font_size)
                .or_else(|e| {
                    log::warn!(
                        "font '{}' not found ({:?}), trying monospace fallback",
                        params.family_name,
                        e
                    );
                    rasterizer.load_font(
                        &FontDesc::new(
                            "monospace",
                            Style::Description {
                                slant: Slant::Normal,
                                weight: Weight::Normal,
                            },
                        ),
                        font_size,
                    )
                })
                .expect("no usable font found (neither configured nor monospace fallback)");

            let bold_key = rasterizer
                .load_font(
                    &FontDesc::new(
                        params.family_name,
                        Style::Description {
                            slant: Slant::Normal,
                            weight: Weight::Bold,
                        },
                    ),
                    font_size,
                )
                .unwrap_or(regular_key);

            let italic_key = rasterizer
                .load_font(
                    &FontDesc::new(
                        params.family_name,
                        Style::Description {
                            slant: Slant::Italic,
                            weight: Weight::Normal,
                        },
                    ),
                    font_size,
                )
                .unwrap_or(regular_key);

            let bold_italic_key = rasterizer
                .load_font(
                    &FontDesc::new(
                        params.family_name,
                        Style::Description {
                            slant: Slant::Italic,
                            weight: Weight::Bold,
                        },
                    ),
                    font_size,
                )
                .unwrap_or(regular_key);

            // Force crossfont char size initialization
            let _ = rasterizer.get_glyph(GlyphKey {
                character: 'M',
                font_key: regular_key,
                size: font_size,
            });
            let crossfont_metrics = rasterizer
                .metrics(regular_key, font_size)
                .expect("failed to get font metrics");

            // Thin FreeType path for glyph-ID rendering
            let ft_library = FtLibrary::init().expect("FreeType init failed");
            let mut ft_face = params.primary_font_path.clone().and_then(|(path, index)| {
                match ft_library.new_face(&path, index as isize) {
                    Ok(face) => {
                        log::info!("FreeType face loaded for glyph-ID path: {path}");
                        Some(face)
                    }
                    Err(e) => {
                        log::warn!("failed to load FreeType face {path}: {e:?}");
                        None
                    }
                }
            });
            let emoji_ft_face = params.emoji_font_path.clone().and_then(|(path, index)| {
                match ft_library.new_face(&path, index as isize) {
                    Ok(face) => {
                        log::info!("FreeType emoji face loaded: {path}");
                        Some(face)
                    }
                    Err(e) => {
                        log::warn!("failed to load emoji FreeType face {path}: {e:?}");
                        None
                    }
                }
            });
            let cjk_ft_face =
                params.cjk_font_path.clone().and_then(|(path, index)| {
                    match ft_library.new_face(&path, index as isize) {
                        Ok(face) => {
                            log::info!("FreeType CJK face loaded: {path}");
                            Some(face)
                        }
                        Err(e) => {
                            log::warn!("failed to load CJK FreeType face {path}: {e:?}");
                            None
                        }
                    }
                });

            let ui_ft_face =
                params.ui_font_path.clone().and_then(|(path, index)| {
                    match ft_library.new_face(&path, index as isize) {
                        Ok(face) => {
                            log::info!("FreeType UI face loaded: {path}");
                            Some(face)
                        }
                        Err(e) => {
                            log::warn!("failed to load UI FreeType face {path}: {e:?}");
                            None
                        }
                    }
                });

            // Compute metrics from FreeType directly
            let (cell_width, cell_height, ascent, face_width) = if let Some(ref mut face) = ft_face
            {
                compute_ft_metrics(face, pixel_size)
            } else {
                let cw = (crossfont_metrics.average_advance as f32).ceil();
                let ch = (crossfont_metrics.line_height as f32).ceil();
                let asc = (crossfont_metrics.line_height as f32 + crossfont_metrics.descent)
                    .ceil()
                    .min(ch);
                (cw, ch, asc, cw)
            };

            // CJK font size adjustment
            let cjk_pixel_size = compute_cjk_pixel_size(
                pixel_size,
                cell_width,
                ft_face.as_ref(),
                cjk_ft_face.as_ref(),
            );

            let font_keys = FontKeySet {
                regular: regular_key,
                bold: bold_key,
                italic: italic_key,
                bold_italic: bold_italic_key,
            };

            (
                rasterizer,
                font_keys,
                font_size,
                ft_library,
                ft_face,
                emoji_ft_face,
                cjk_ft_face,
                ui_ft_face,
                cell_width,
                cell_height,
                ascent,
                face_width,
                cjk_pixel_size,
            )
        };

        #[cfg(target_os = "macos")]
        let (mut coretext, ui_ct_font, cell_width, cell_height, ascent, face_width, cjk_pixel_size) = {
            log::info!(
                "CoreText cache: initializing pixel_size={:.1} family='{}' primary_path={:?} emoji_path={:?} cjk_path={:?}",
                pixel_size,
                params.family_name,
                params.primary_font_path,
                params.emoji_font_path,
                params.cjk_font_path,
            );
            let mut coretext = CoreTextRasterizer::new(
                params.family_name,
                params
                    .primary_font_path
                    .as_ref()
                    .map(|(p, i)| (p.as_str(), *i)),
                params
                    .emoji_font_path
                    .as_ref()
                    .map(|(p, i)| (p.as_str(), *i)),
                params.cjk_font_path.as_ref().map(|(p, i)| (p.as_str(), *i)),
                pixel_size,
            )
            .expect("CoreText init failed");

            let (cell_width, cell_height, ascent, face_width) =
                compute_ct_metrics(coretext.primary_font(FontStyle::Regular));

            let cjk_pixel_size = compute_cjk_pixel_size_ct(
                pixel_size,
                cell_width,
                coretext.primary_font(FontStyle::Regular),
                coretext.cjk_font(),
            );

            // Recreate CJK font at the adjusted size
            if (cjk_pixel_size - pixel_size).abs() > 0.1 {
                coretext.set_cjk_pixel_size(cjk_pixel_size);
            }

            let ui_ct_font = match (&params.ui_font_path, params.ui_pixel_size) {
                (Some((path, idx)), Some(px)) => {
                    crate::shaper::load_ct_font_from_path(path, *idx, px as f64)
                }
                _ => None,
            };

            (
                coretext,
                ui_ct_font,
                cell_width,
                cell_height,
                ascent,
                face_width,
                cjk_pixel_size,
            )
        };

        #[cfg(windows)]
        let (dwrite, cell_width, cell_height, ascent, face_width, cjk_pixel_size) = {
            let mut dwrite = DWriteRasterizer::new(
                params.family_name,
                params
                    .primary_font_path
                    .as_ref()
                    .map(|(p, i)| (p.as_str(), *i)),
                params
                    .emoji_font_path
                    .as_ref()
                    .map(|(p, i)| (p.as_str(), *i)),
                params.cjk_font_path.as_ref().map(|(p, i)| (p.as_str(), *i)),
            )
            .expect("DWrite init failed");

            let (cell_width, cell_height, ascent, face_width) =
                if let Some(face) = dwrite.primary_face(FontStyle::Regular) {
                    compute_dwrite_metrics(face, pixel_size)
                } else {
                    log::warn!("no primary DWrite face, using fallback metrics");
                    (8.0, 16.0, 12.0, 8.0)
                };

            let cjk_pixel_size = compute_cjk_pixel_size_dwrite(
                pixel_size,
                cell_width,
                dwrite.primary_face(FontStyle::Regular),
                dwrite.cjk_face(FontStyle::Regular),
            );

            // Load the UI font. On Windows, prefer the system collection when a
            // family name was configured so simulated bold/italic follow the
            // same fallback rules as the primary terminal font.
            if let Some((ref path, idx)) = params.ui_font_path {
                dwrite.load_ui_font(params.ui_family_name, path, idx);
            }

            (
                dwrite,
                cell_width,
                cell_height,
                ascent,
                face_width,
                cjk_pixel_size,
            )
        };

        // Apply caller-supplied cell metric scaling. We round to whole pixels
        // because grid math elsewhere assumes integer-aligned cells; clamp to
        // a sensible minimum so a misconfigured value can't produce a zero
        // cell.
        let cw_scale = params.cell_width_scale_or_default();
        let ch_scale = params.cell_height_scale_or_default();
        let cell_width = (cell_width * cw_scale).round().max(1.0);
        let cell_height = (cell_height * ch_scale).round().max(1.0);
        // The glyph baseline is measured from the cell top, so it should
        // shift along with the (now larger or smaller) cell height. Keeping
        // the *fraction* of the original cell height that the baseline took
        // up is a reasonable default; users can fine-tune via
        // `adjust-underline-position` etc. if needed.
        let ascent = ascent * ch_scale;
        // CJK pixel size was solved against the unscaled cell width to make
        // "水" advance fill exactly two cells. The advance scales linearly
        // with pixel size, so apply the same horizontal scale to keep CJK
        // glyphs aligned with the (now wider/narrower) two-cell box.
        let cjk_pixel_size = cjk_pixel_size * cw_scale;
        // CoreText caches the CJK font at a specific size; re-set it so the
        // rasterization side actually uses the scaled size.
        #[cfg(target_os = "macos")]
        {
            if (cw_scale - 1.0).abs() > f32::EPSILON {
                coretext.set_cjk_pixel_size(cjk_pixel_size);
            }
        }

        let cache = GlyphCache {
            alpha_packer: ShelfPacker::new(atlas_size),
            color_packer: ShelfPacker::new(atlas_size),
            alpha_pending: Vec::new(),
            color_pending: Vec::new(),
            alpha_pending_clear: false,
            color_pending_clear: false,
            atlas_size,
            max_instances,
            cache: HashMap::new(),
            glyph_id_cache: HashMap::new(),

            #[cfg(target_os = "linux")]
            rasterizer,
            #[cfg(target_os = "linux")]
            font_keys,
            #[cfg(target_os = "linux")]
            font_size,
            #[cfg(target_os = "linux")]
            _ft_library,
            #[cfg(target_os = "linux")]
            ft_face,
            #[cfg(target_os = "linux")]
            emoji_ft_face,
            #[cfg(target_os = "linux")]
            cjk_ft_face,
            #[cfg(target_os = "linux")]
            ui_ft_face,

            #[cfg(target_os = "macos")]
            coretext,
            #[cfg(target_os = "macos")]
            ui_ct_font,

            #[cfg(windows)]
            dwrite,
            #[cfg(windows)]
            dwrite_alpha_pending: Vec::new(),
            #[cfg(windows)]
            dwrite_color_pending: Vec::new(),
            #[cfg(windows)]
            use_d2d_rendering: false,

            font_resolver: params.font_resolver.clone(),
            #[cfg(windows)]
            dwrite_resolver: params.dwrite_resolver.clone(),
            emoji_font_id: params.emoji_font_id,
            cjk_font_id: params.cjk_font_id,
            ui_font_id: params.ui_font_id,
            pixel_size,
            cjk_pixel_size,
            ui_pixel_size: params.ui_pixel_size.unwrap_or(pixel_size),
            cell_width,
            cell_height,
            ascent,
            face_width,
            atlas_needs_clear: false,
        };

        #[cfg(windows)]
        if let Some(ref resolver) = cache.dwrite_resolver {
            let (primary, cjk, emoji) = cache.dwrite.regular_faces();
            resolver.set_known_faces(primary, cjk, emoji);
        }

        cache
    }

    #[cfg(windows)]
    fn cache_windows_glyph(
        &mut self,
        face: &IDWriteFontFace,
        glyph_id: u32,
        pixel_size: f32,
        try_color: bool,
    ) -> Option<GlyphEntry> {
        if self.use_d2d_rendering {
            let measured = self
                .dwrite
                .measure_glyph(face, glyph_id, pixel_size, try_color)?;
            if measured.width == 0 || measured.height == 0 {
                return None;
            }

            if measured.is_color {
                let glyph = self
                    .dwrite
                    .rasterize_glyph(face, glyph_id, pixel_size, true)?;
                if glyph.width == 0 || glyph.height == 0 {
                    return None;
                }
                return cache_rasterized_glyph(
                    glyph,
                    &mut self.alpha_packer,
                    &mut self.color_packer,
                    &mut self.alpha_pending,
                    &mut self.color_pending,
                    self.atlas_size,
                    &mut self.atlas_needs_clear,
                );
            }

            return cache_measured_dwrite_glyph(
                &measured,
                face,
                glyph_id as u16,
                pixel_size,
                &mut self.alpha_packer,
                &mut self.color_packer,
                &mut self.dwrite_alpha_pending,
                &mut self.dwrite_color_pending,
                self.atlas_size,
                &mut self.atlas_needs_clear,
            );
        }

        let glyph = self
            .dwrite
            .rasterize_glyph(face, glyph_id, pixel_size, try_color)?;
        if glyph.width == 0 || glyph.height == 0 {
            return None;
        }
        cache_rasterized_glyph(
            glyph,
            &mut self.alpha_packer,
            &mut self.color_packer,
            &mut self.alpha_pending,
            &mut self.color_pending,
            self.atlas_size,
            &mut self.atlas_needs_clear,
        )
    }

    /// Ensure a glyph for `ch` with the given `style` is in the atlas.
    pub fn ensure_styled_char(&mut self, ch: char, style: FontStyle) -> Option<GlyphEntry> {
        let key = (ch, style);
        if let Some(entry) = self.cache.get(&key) {
            return Some(*entry);
        }

        if ch == ' ' || ch == '\0' || ch.is_control() {
            self.cache.insert(key, GlyphEntry::EMPTY);
            return Some(GlyphEntry::EMPTY);
        }

        // ── Rasterize (platform-specific) ──

        #[cfg(target_os = "linux")]
        let rasterized = {
            let font_key = self.font_keys.get(style);
            let glyph_key = GlyphKey {
                character: ch,
                font_key,
                size: self.font_size,
            };
            let glyph = match self.rasterizer.get_glyph(glyph_key) {
                Ok(g) => g,
                Err(crossfont::Error::MissingGlyph(g)) => g,
                Err(_) => {
                    self.cache.insert(key, GlyphEntry::EMPTY);
                    return Some(GlyphEntry::EMPTY);
                }
            };
            if glyph.width == 0 || glyph.height == 0 {
                self.cache.insert(key, GlyphEntry::EMPTY);
                return Some(GlyphEntry::EMPTY);
            }
            convert_crossfont_glyph(glyph)
        };

        #[cfg(target_os = "macos")]
        let rasterized = {
            // Use the font resolver to determine fallback order.
            let preferred = self.font_resolver.resolve_char(ch);
            let order = resolved_font_order(preferred);

            log::debug!(
                "CoreText cache: ensure_styled_char U+{:04X} '{}' style={:?} preferred={:?}",
                ch as u32,
                ch.escape_unicode(),
                style,
                preferred,
            );

            let mut result: Option<RasterizedGlyph> = None;
            for resolved in &order {
                let (font, _px, try_color) = match resolved {
                    ResolvedFont::Primary => (
                        Some(self.coretext.primary_font(style)),
                        self.pixel_size,
                        false,
                    ),
                    ResolvedFont::Cjk => (self.coretext.cjk_font(), self.cjk_pixel_size, false),
                    ResolvedFont::Emoji => (
                        self.coretext.emoji_font(),
                        self.pixel_size,
                        self.coretext.is_emoji_color(),
                    ),
                };
                if let Some(font) = font {
                    if let Some(glyph) = self.coretext.rasterize_char(ch, style, font, try_color) {
                        if glyph.width > 0 && glyph.height > 0 {
                            log::debug!(
                                "CoreText cache: U+{:04X} rasterized via {:?} ({}x{} color={})",
                                ch as u32,
                                resolved,
                                glyph.width,
                                glyph.height,
                                glyph.is_color,
                            );
                            result = Some(glyph);
                            break;
                        }
                    }
                } else {
                    log::debug!(
                        "CoreText cache: U+{:04X} skipping {:?} (no font loaded)",
                        ch as u32,
                        resolved,
                    );
                }
            }

            match result {
                Some(g) => g,
                None => {
                    log::debug!(
                        "CoreText cache: U+{:04X} '{}' -> EMPTY (no font could rasterize)",
                        ch as u32,
                        ch.escape_unicode(),
                    );
                    self.cache.insert(key, GlyphEntry::EMPTY);
                    return Some(GlyphEntry::EMPTY);
                }
            }
        };

        #[cfg(windows)]
        {
            // Use the font resolver to determine fallback order.
            let preferred = self.font_resolver.resolve_char(ch);
            let order = resolved_font_order(preferred);
            // Build candidates from the resolved order. Each entry is
            // (face, pixel_size, try_color). Done eagerly here so we
            // don't hold an immutable borrow of `self` into the mutable
            // rasterization loop below.
            let candidate = |f: ResolvedFont| -> (Option<IDWriteFontFace>, f32, bool) {
                match f {
                    ResolvedFont::Primary => (
                        self.dwrite.primary_face(style).cloned(),
                        self.pixel_size,
                        false,
                    ),
                    ResolvedFont::Cjk => (
                        self.dwrite.cjk_face(style).cloned(),
                        self.cjk_pixel_size,
                        false,
                    ),
                    ResolvedFont::Emoji => (
                        self.dwrite.emoji_face(style).cloned(),
                        self.pixel_size,
                        self.dwrite.is_emoji_color(),
                    ),
                }
            };
            let candidates: [(Option<IDWriteFontFace>, f32, bool); 3] = [
                candidate(order[0]),
                candidate(order[1]),
                candidate(order[2]),
            ];

            // Helper: try rasterizing a char with a given DWrite face.
            macro_rules! try_dwrite_face {
                ($face:expr, $px:expr, $try_color:expr) => {
                    if let Some(gid) = DWriteRasterizer::char_to_glyph($face, ch) {
                        if let Some(entry) =
                            self.cache_windows_glyph($face, gid as u32, $px, $try_color)
                        {
                            self.cache.insert(key, entry);
                            return Some(entry);
                        }
                    }
                };
            }

            for (face, px, try_color) in &candidates {
                if let Some(face) = face {
                    try_dwrite_face!(face, *px, *try_color);
                }
            }

            if let Some(ref resolver) = self.dwrite_resolver
                && let Some(sys_face) = resolver.get_system_face(ch)
            {
                try_dwrite_face!(&sys_face, self.pixel_size, false);
            }

            self.cache.insert(key, GlyphEntry::EMPTY);
            return Some(GlyphEntry::EMPTY);
        }

        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let entry = cache_rasterized_glyph(
                rasterized,
                &mut self.alpha_packer,
                &mut self.color_packer,
                &mut self.alpha_pending,
                &mut self.color_pending,
                self.atlas_size,
                &mut self.atlas_needs_clear,
            )?;
            self.cache.insert(key, entry);
            Some(entry)
        }
    }

    /// Ensure a glyph by its ID (from text shaping) is in the atlas.
    /// `font_id` selects between primary, CJK, and emoji font faces.
    /// `wide` indicates the glyph may be constrained to a double-width cell.
    pub fn ensure_glyph_id(
        &mut self,
        glyph_id: u32,
        font_id: fontdb::ID,
        style: FontStyle,
        wide: bool,
    ) -> Option<GlyphEntry> {
        let font_class = if Some(font_id) == self.emoji_font_id {
            FontClass::Emoji
        } else if Some(font_id) == self.cjk_font_id {
            FontClass::Cjk
        } else {
            FontClass::Primary
        };
        self.ensure_glyph_id_with_class(glyph_id, font_class, style, wide)
    }

    /// Ensure a shaped UI glyph is in the atlas.
    ///
    /// UI text may intentionally use the same underlying `fontdb::ID` as the
    /// terminal primary font. Route these calls explicitly so terminal glyphs
    /// are never misclassified as `Ui` just because the IDs match.
    pub fn ensure_ui_glyph_id(
        &mut self,
        glyph_id: u32,
        font_id: fontdb::ID,
        style: FontStyle,
        wide: bool,
    ) -> Option<GlyphEntry> {
        let font_class = if Some(font_id) == self.emoji_font_id {
            FontClass::Emoji
        } else if Some(font_id) == self.cjk_font_id {
            FontClass::Cjk
        } else if self.ui_font_id.is_some() && Some(font_id) == self.ui_font_id {
            FontClass::Ui
        } else {
            FontClass::Primary
        };
        self.ensure_glyph_id_with_class(glyph_id, font_class, style, wide)
    }

    fn ensure_glyph_id_with_class(
        &mut self,
        glyph_id: u32,
        font_class: FontClass,
        style: FontStyle,
        wide: bool,
    ) -> Option<GlyphEntry> {
        let key = (glyph_id, font_class, style, wide);
        if let Some(entry) = self.glyph_id_cache.get(&key) {
            return Some(*entry);
        }
        if glyph_id == 0 {
            self.glyph_id_cache.insert(key, GlyphEntry::EMPTY);
            return Some(GlyphEntry::EMPTY);
        }

        // ── Rasterize (platform-specific) ──

        #[cfg(target_os = "linux")]
        let glyph = {
            let (ft_face, px) = match font_class {
                FontClass::Emoji => (self.emoji_ft_face.as_ref(), self.pixel_size),
                FontClass::Cjk => (self.cjk_ft_face.as_ref(), self.cjk_pixel_size),
                FontClass::Ui => (self.ui_ft_face.as_ref(), self.ui_pixel_size),
                FontClass::Primary => (self.ft_face.as_ref(), self.pixel_size),
            };
            rasterize_glyph_id_ft(ft_face, glyph_id, style, px, wide, self.cell_height)?
        };

        #[cfg(target_os = "macos")]
        let glyph = {
            log::debug!(
                "CoreText cache: ensure_glyph_id glyph={} class={:?} style={:?} wide={}",
                glyph_id,
                font_class,
                style,
                wide,
            );

            let (font, _px, try_color) = match font_class {
                FontClass::Emoji => (
                    self.coretext.emoji_font(),
                    self.pixel_size,
                    self.coretext.is_emoji_color(),
                ),
                FontClass::Cjk => (self.coretext.cjk_font(), self.cjk_pixel_size, false),
                FontClass::Ui => (self.ui_ct_font.as_ref(), self.ui_pixel_size, false),
                FontClass::Primary => (
                    Some(self.coretext.primary_font(style)),
                    self.pixel_size,
                    false,
                ),
            };
            match font {
                Some(f) => match self
                    .coretext
                    .rasterize_glyph_id(f, glyph_id, style, try_color)
                {
                    Some(g) => {
                        log::debug!(
                            "CoreText cache: glyph_id={} rasterized via {:?} ({}x{} color={})",
                            glyph_id,
                            font_class,
                            g.width,
                            g.height,
                            g.is_color,
                        );
                        g
                    }
                    None => {
                        log::debug!(
                            "CoreText cache: glyph_id={} -> EMPTY (rasterize returned None for {:?})",
                            glyph_id,
                            font_class,
                        );
                        self.glyph_id_cache.insert(key, GlyphEntry::EMPTY);
                        return Some(GlyphEntry::EMPTY);
                    }
                },
                None => {
                    log::debug!(
                        "CoreText cache: glyph_id={} -> EMPTY (no {:?} font loaded)",
                        glyph_id,
                        font_class,
                    );
                    self.glyph_id_cache.insert(key, GlyphEntry::EMPTY);
                    return Some(GlyphEntry::EMPTY);
                }
            }
        };

        #[cfg(windows)]
        {
            let (face, px, try_color) = match font_class {
                FontClass::Emoji => (
                    self.dwrite.emoji_face(style)?.clone(),
                    self.pixel_size,
                    self.dwrite.is_emoji_color(),
                ),
                FontClass::Cjk => (
                    self.dwrite.cjk_face(style)?.clone(),
                    self.cjk_pixel_size,
                    false,
                ),
                FontClass::Ui => {
                    if let Some(face) = self.dwrite.ui_face(style) {
                        (face.clone(), self.ui_pixel_size, false)
                    } else {
                        self.glyph_id_cache.insert(key, GlyphEntry::EMPTY);
                        return Some(GlyphEntry::EMPTY);
                    }
                }
                FontClass::Primary => (
                    self.dwrite.primary_face(style)?.clone(),
                    self.pixel_size,
                    false,
                ),
            };

            let Some(entry) = self.cache_windows_glyph(&face, glyph_id, px, try_color) else {
                self.glyph_id_cache.insert(key, GlyphEntry::EMPTY);
                return Some(GlyphEntry::EMPTY);
            };
            self.glyph_id_cache.insert(key, entry);
            return Some(entry);
        }

        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            if glyph.width == 0 || glyph.height == 0 {
                self.glyph_id_cache.insert(key, GlyphEntry::EMPTY);
                return Some(GlyphEntry::EMPTY);
            }
            let entry = cache_rasterized_glyph(
                glyph,
                &mut self.alpha_packer,
                &mut self.color_packer,
                &mut self.alpha_pending,
                &mut self.color_pending,
                self.atlas_size,
                &mut self.atlas_needs_clear,
            )?;
            self.glyph_id_cache.insert(key, entry);
            Some(entry)
        }
    }

    /// Ensure a regular-style character is in the atlas.
    pub fn ensure_char(&mut self, ch: char) -> Option<GlyphEntry> {
        self.ensure_styled_char(ch, FontStyle::Regular)
    }

    /// Upload an arbitrary RGBA bitmap into the color atlas.
    pub fn cache_rgba_image(&mut self, width: u32, height: u32, data: &[u8]) -> Option<GlyphEntry> {
        let expected_len = width.checked_mul(height)?.checked_mul(4)? as usize;
        if data.len() != expected_len {
            log::warn!(
                "rejecting RGBA image upload: got {} bytes, expected {expected_len} for {width}x{height}",
                data.len()
            );
            return None;
        }

        cache_rasterized_glyph(
            RasterizedGlyph {
                width,
                height,
                bearing_x: 0.0,
                bearing_y: 0.0,
                is_color: true,
                data: data.to_vec(),
            },
            &mut self.alpha_packer,
            &mut self.color_packer,
            &mut self.alpha_pending,
            &mut self.color_pending,
            self.atlas_size,
            &mut self.atlas_needs_clear,
        )
    }

    /// Drain pending glyph uploads for the GPU backend to consume.
    /// Returns `(alpha_uploads, color_uploads, alpha_needs_clear, color_needs_clear)`.
    pub fn take_pending(&mut self) -> (Vec<PendingUpload>, Vec<PendingUpload>, bool, bool) {
        let alpha_clear = std::mem::take(&mut self.alpha_pending_clear);
        let color_clear = std::mem::take(&mut self.color_pending_clear);
        (
            std::mem::take(&mut self.alpha_pending),
            std::mem::take(&mut self.color_pending),
            alpha_clear,
            color_clear,
        )
    }

    /// Put leftover uploads back at the front of the pending queue so
    /// they are retried on the next `take_pending` / `flush`. GPU
    /// backends call this with whatever the staging buffer could not fit
    /// in a single frame, so glyphs aren't silently dropped on overflow.
    /// Ordering is preserved: the tail goes *before* any new uploads
    /// pushed since the backend drained the queue.
    pub fn restore_pending(
        &mut self,
        mut alpha_tail: Vec<PendingUpload>,
        mut color_tail: Vec<PendingUpload>,
    ) {
        if !alpha_tail.is_empty() {
            alpha_tail.extend(std::mem::take(&mut self.alpha_pending));
            self.alpha_pending = alpha_tail;
        }
        if !color_tail.is_empty() {
            color_tail.extend(std::mem::take(&mut self.color_pending));
            self.color_pending = color_tail;
        }
    }

    /// Drain pending DWrite glyph render commands for the DX D2D backend.
    #[cfg(windows)]
    pub fn take_dwrite_pending(&mut self) -> (Vec<PendingDwriteGlyph>, Vec<PendingDwriteGlyph>) {
        (
            std::mem::take(&mut self.dwrite_alpha_pending),
            std::mem::take(&mut self.dwrite_color_pending),
        )
    }

    /// Enable D2D direct-to-atlas rendering (measure-only, no CPU pixel extraction).
    /// Called by the DX backend at atlas creation time.
    #[cfg(windows)]
    pub fn set_d2d_rendering(&mut self, enabled: bool) {
        self.use_d2d_rendering = enabled;
        log::info!("GlyphCache: D2D direct rendering = {enabled}");
    }

    /// Clear the glyph cache and reset both packers.
    /// No GPU work — the actual texture clear is deferred to next flush.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
        self.glyph_id_cache.clear();
        self.alpha_packer = ShelfPacker::new(self.atlas_size);
        self.color_packer = ShelfPacker::new(self.atlas_size);
        self.alpha_pending.clear();
        self.color_pending.clear();
        #[cfg(windows)]
        {
            self.dwrite_alpha_pending.clear();
            self.dwrite_color_pending.clear();
        }
        self.alpha_pending_clear = true;
        self.color_pending_clear = true;
        log::info!(
            "glyph cache cleared (atlas {}×{})",
            self.atlas_size,
            self.atlas_size
        );
    }

    /// Compute grid dimensions (cols × rows) for the given viewport size.
    pub fn grid_size(&self, viewport_w: f32, viewport_h: f32) -> (u16, u16) {
        let cols = (viewport_w / self.cell_width).floor() as u16;
        let rows = (viewport_h / self.cell_height).floor() as u16;
        (cols.max(1), rows.max(1))
    }
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    use ciri_config::config::CiriConfig;

    fn test_cache(atlas_size: u32) -> GlyphCache {
        let mut config = CiriConfig::default();
        config.render.atlas_size = atlas_size;
        GlyphCache::new(&FontInitParams {
            font_size_pt: config.font.size,
            dpi_scale: 1.0,
            family_name: &config.font.family,
            ui_family_name: None,
            primary_font_path: None,
            emoji_font_path: None,
            emoji_font_id: None,
            cjk_font_path: None,
            cjk_font_id: None,
            ui_font_path: None,
            ui_font_id: None,
            ui_pixel_size: None,
            render_config: &config.render,
            font_resolver: Arc::new(crate::font_resolver::CmapResolver::new(
                (&[], 0),
                None,
                None,
            )),
            #[cfg(windows)]
            dwrite_resolver: None,
            cell_width_scale: None,
            cell_height_scale: None,
        })
    }

    #[test]
    fn clear_cache_resets_packers_and_marks_pending_clear() {
        let mut cache = test_cache(32);
        cache
            .cache
            .insert(('A', FontStyle::Regular), GlyphEntry::EMPTY);
        cache.glyph_id_cache.insert(
            (1, FontClass::Primary, FontStyle::Regular, false),
            GlyphEntry::EMPTY,
        );
        cache.alpha_pending.push(PendingUpload {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
            data: vec![255],
        });
        cache.color_pending.push(PendingUpload {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
            data: vec![255, 0, 0, 255],
        });

        cache.clear_cache();
        let (alpha, color, alpha_clear, color_clear) = cache.take_pending();

        assert!(cache.cache.is_empty());
        assert!(cache.glyph_id_cache.is_empty());
        assert_eq!(cache.alpha_packer.allocate(4, 4), Some((0, 0)));
        assert_eq!(cache.color_packer.allocate(4, 4), Some((0, 0)));
        assert!(alpha.is_empty());
        assert!(color.is_empty());
        assert!(alpha_clear);
        assert!(color_clear);
    }

    #[test]
    fn atlas_full_sets_needs_clear_without_queueing_uploads() {
        let mut cache = test_cache(5);
        let glyph = rasterize::RasterizedGlyph {
            width: 4,
            height: 4,
            bearing_x: 0.0,
            bearing_y: 0.0,
            is_color: false,
            data: vec![255; 16],
        };
        let result = cache_rasterized_glyph(
            glyph,
            &mut cache.alpha_packer,
            &mut cache.color_packer,
            &mut cache.alpha_pending,
            &mut cache.color_pending,
            cache.atlas_size,
            &mut cache.atlas_needs_clear,
        );
        assert!(result.is_some());

        let overflow = rasterize::RasterizedGlyph {
            width: 1,
            height: 1,
            bearing_x: 0.0,
            bearing_y: 0.0,
            is_color: false,
            data: vec![255],
        };
        let result = cache_rasterized_glyph(
            overflow,
            &mut cache.alpha_packer,
            &mut cache.color_packer,
            &mut cache.alpha_pending,
            &mut cache.color_pending,
            cache.atlas_size,
            &mut cache.atlas_needs_clear,
        );
        assert!(result.is_none());
        assert!(cache.atlas_needs_clear);

        let (alpha, color, alpha_clear, color_clear) = cache.take_pending();
        assert_eq!(alpha.len(), 1);
        assert!(color.is_empty());
        assert!(!alpha_clear);
        assert!(!color_clear);
    }

    #[cfg(windows)]
    #[test]
    fn d2d_color_emoji_falls_back_to_cpu_rgba_uploads() {
        let mut cache = test_cache(512);
        cache.set_d2d_rendering(true);

        let Some(entry) = cache.ensure_char('😀') else {
            return;
        };
        if !entry.is_color {
            return;
        }

        let (_alpha, color, _alpha_clear, _color_clear) = cache.take_pending();
        let (_dwrite_alpha, dwrite_color) = cache.take_dwrite_pending();

        assert!(
            !color.is_empty(),
            "color emoji should queue CPU RGBA uploads even when D2D rendering is enabled"
        );
        assert!(
            dwrite_color.is_empty(),
            "color emoji should not use D2D DrawGlyphRun pending queue"
        );
    }
}
