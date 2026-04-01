//! CPU glyph cache — rasterizes and caches terminal text glyphs.
//!
//! Two atlas layers:
//! - **Text** (`R8Unorm`): grayscale alpha mask via crossfont
//! - **Color** (`Rgba8Srgb`): color emoji via crossfont
//!
//! The GPU upload/rendering is handled by the backend's `GlyphAtlasGpu`.

mod atlas;
mod cjk;
mod metrics;
mod rasterize;
pub mod types;

pub use atlas::PendingUpload;
pub(crate) use atlas::ShelfPacker;
pub use types::{FontStyle, GlyphEntry, GlyphInstance, ScissoredRange};

use cjk::compute_cjk_pixel_size;
use metrics::compute_ft_metrics;
use rasterize::{
    RasterizedGlyph, cache_rasterized_glyph, convert_crossfont_glyph, rasterize_glyph_id_ft,
};
use types::{FontClass, FontKeySet};

use ciri_config::config::RenderConfig;
use crossfont::{FontDesc, GlyphKey, Rasterize, Rasterizer, Size, Slant, Style, Weight};
use freetype::Library as FtLibrary;
use std::collections::HashMap;

// ─── Font init params ───────────────────────────────────────────────

/// Parameters for initializing the glyph cache and font pipeline.
pub struct FontInitParams<'a> {
    pub font_size_pt: f32,
    pub dpi_scale: f64,
    pub family_name: &'a str,
    pub primary_font_path: Option<(String, u32)>,
    pub emoji_font_path: Option<(String, u32)>,
    pub emoji_font_id: Option<fontdb::ID>,
    pub cjk_font_path: Option<(String, u32)>,
    pub cjk_font_id: Option<fontdb::ID>,
    pub render_config: &'a RenderConfig,
}

// ─── Glyph cache (CPU) ──────────────────────────────────────────────

/// CPU-side glyph cache: rasterization, packing, and caching.
/// GPU upload/rendering is delegated to backend's `GlyphAtlasGpu`.
///
/// Uses crossfont for character-based rendering and a thin FreeType path
/// for glyph-ID rendering (ligatures from text shaping).
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
    // Crossfont rasterizer (character-based rendering)
    rasterizer: Rasterizer,
    font_keys: FontKeySet,
    font_size: Size,
    // Thin FreeType path (glyph-ID rendering for shaped glyphs)
    // Keep library alive — ft_face borrows from it.
    _ft_library: FtLibrary,
    ft_face: Option<freetype::Face>,
    emoji_ft_face: Option<freetype::Face>,
    emoji_font_id: Option<fontdb::ID>,
    cjk_ft_face: Option<freetype::Face>,
    cjk_font_id: Option<fontdb::ID>,
    ft_pixel_size: f32,
    /// CJK font pixel size, adjusted so that "水" advance matches 2 * cell_width.
    cjk_pixel_size: f32,
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

        // ── Crossfont setup ──
        let mut rasterizer = Rasterizer::new().expect("crossfont init failed");
        // Convert point size to pixels: pt × (96 × scale) / 72, then use from_px
        let pixel_size = params.font_size_pt * (96.0 * params.dpi_scale as f32) / 72.0;
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

        // Force crossfont char size initialization by rasterizing a probe glyph.
        // crossfont sets FreeType char size lazily in get_glyph(), so metrics()
        // returns zeros if called before any glyph has been rasterized.
        let _ = rasterizer.get_glyph(GlyphKey {
            character: 'M',
            font_key: regular_key,
            size: font_size,
        });
        // Keep crossfont metrics as fallback only.
        let crossfont_metrics = rasterizer
            .metrics(regular_key, font_size)
            .expect("failed to get font metrics");

        // ── Thin FreeType path for glyph-ID rendering ──
        let ft_library = FtLibrary::init().expect("FreeType init failed");
        let ft_pixel_size = params.font_size_pt * (96.0 * params.dpi_scale as f32) / 72.0;
        let mut ft_face =
            params.primary_font_path.clone().and_then(|(path, index)| {
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
        let emoji_ft_face =
            params.emoji_font_path.clone().and_then(|(path, index)| {
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
        let cjk_ft_face = params.cjk_font_path.clone().and_then(|(path, index)| {
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

        // ── Compute metrics from FreeType directly ──
        let (cell_width, cell_height, ascent, face_width) = if let Some(ref mut face) = ft_face {
            compute_ft_metrics(face, ft_pixel_size)
        } else {
            // Fallback: use crossfont metrics (legacy behavior)
            let cw = (crossfont_metrics.average_advance as f32).ceil();
            let ch = (crossfont_metrics.line_height as f32).ceil();
            let asc = (crossfont_metrics.line_height as f32 + crossfont_metrics.descent)
                .ceil()
                .min(ch);
            (cw, ch, asc, cw)
        };

        // ── CJK font size adjustment (ghostty approach) ──
        let cjk_pixel_size = compute_cjk_pixel_size(
            ft_pixel_size,
            cell_width,
            ft_face.as_ref(),
            cjk_ft_face.as_ref(),
        );

        GlyphCache {
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
            rasterizer,
            font_keys: FontKeySet {
                regular: regular_key,
                bold: bold_key,
                italic: italic_key,
                bold_italic: bold_italic_key,
            },
            font_size,
            _ft_library: ft_library,
            ft_face,
            emoji_ft_face,
            emoji_font_id: params.emoji_font_id,
            cjk_ft_face,
            cjk_font_id: params.cjk_font_id,
            ft_pixel_size,
            cjk_pixel_size,
            cell_width,
            cell_height,
            ascent,
            face_width,
            atlas_needs_clear: false,
        }
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

        let w = glyph.width as u32;
        let h = glyph.height as u32;
        if w == 0 || h == 0 {
            self.cache.insert(key, GlyphEntry::EMPTY);
            return Some(GlyphEntry::EMPTY);
        }

        let rasterized = convert_crossfont_glyph(glyph);
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

    /// Ensure a glyph by its ID (from text shaping) is in the atlas.
    /// Uses the thin FreeType path since crossfont only accepts characters.
    /// `font_id` selects between primary, CJK, and emoji font faces.
    /// `wide` indicates the glyph may be constrained to a double-width cell (disables hinting).
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
        let key = (glyph_id, font_class, style, wide);
        if let Some(entry) = self.glyph_id_cache.get(&key) {
            return Some(*entry);
        }
        if glyph_id == 0 {
            self.glyph_id_cache.insert(key, GlyphEntry::EMPTY);
            return Some(GlyphEntry::EMPTY);
        }

        let (ft_face, pixel_size) = match font_class {
            FontClass::Emoji => (self.emoji_ft_face.as_ref(), self.ft_pixel_size),
            FontClass::Cjk => (self.cjk_ft_face.as_ref(), self.cjk_pixel_size),
            FontClass::Primary => (self.ft_face.as_ref(), self.ft_pixel_size),
        };
        let glyph =
            rasterize_glyph_id_ft(ft_face, glyph_id, style, pixel_size, wide, self.cell_height)?;

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

    /// Clear the glyph cache and reset both packers.
    /// No GPU work — the actual texture clear is deferred to next flush.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
        self.glyph_id_cache.clear();
        self.alpha_packer = ShelfPacker::new(self.atlas_size);
        self.color_packer = ShelfPacker::new(self.atlas_size);
        self.alpha_pending.clear();
        self.color_pending.clear();
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
    use super::*;
    use ciri_config::config::CiriConfig;

    fn test_cache(atlas_size: u32) -> GlyphCache {
        let mut config = CiriConfig::default();
        config.render.atlas_size = atlas_size;
        GlyphCache::new(&FontInitParams {
            font_size_pt: config.font.size,
            dpi_scale: 1.0,
            family_name: &config.font.family,
            primary_font_path: None,
            emoji_font_path: None,
            emoji_font_id: None,
            cjk_font_path: None,
            cjk_font_id: None,
            render_config: &config.render,
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
        let mut cache = test_cache(4);
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
}
