//! CPU glyph cache — rasterizes and caches terminal text glyphs.
//!
//! Two logical atlas layers:
//! - **Alpha** (`R8`): monochrome text glyphs
//! - **Color** (`RGBA`): color emoji
//!
//! The GPU upload/rendering is handled by the backend's `GlyphAtlasGpu`.

use ciri_config::config::RenderConfig;
use cosmic_text::FontSystem;
use cosmic_text::fontdb;
use std::collections::HashMap;
use swash::scale::{Render, ScaleContext, Source, StrikeWith, image::Content};
use swash::zeno::Format;

// ─── Font style ──────────────────────────────────────────────────────

/// Font style for glyph cache lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontStyle {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

impl FontStyle {
    /// Select font style from bold/italic flags.
    pub fn from_bold_italic(bold: bool, italic: bool) -> Self {
        match (bold, italic) {
            (true, true) => Self::BoldItalic,
            (true, false) => Self::Bold,
            (false, true) => Self::Italic,
            (false, false) => Self::Regular,
        }
    }
}

// ─── Glyph entry ─────────────────────────────────────────────────────

/// UV coordinates and metrics of a cached glyph in the atlas.
#[derive(Debug, Clone, Copy)]
pub struct GlyphEntry {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
    pub width: u16,
    pub height: u16,
    pub bearing_x: i16,
    pub bearing_y: i16,
    /// True if this glyph was rasterized as RGBA color (emoji).
    pub is_color: bool,
}

impl GlyphEntry {
    pub const EMPTY: Self = GlyphEntry {
        u0: 0.0,
        v0: 0.0,
        u1: 0.0,
        v1: 0.0,
        width: 0,
        height: 0,
        bearing_x: 0,
        bearing_y: 0,
        is_color: false,
    };
}

// ─── Shelf-based atlas packer ────────────────────────────────────────

/// Simple shelf-based 2D rectangle packer for atlas allocation.
/// Allocates left-to-right, top-to-bottom in horizontal shelves.
pub(crate) struct ShelfPacker {
    shelf_y: u32,
    shelf_height: u32,
    cursor_x: u32,
    size: u32,
}

impl ShelfPacker {
    pub(crate) fn new(size: u32) -> Self {
        ShelfPacker {
            shelf_y: 0,
            shelf_height: 0,
            cursor_x: 0,
            size,
        }
    }

    /// Try to allocate a `w×h` region. Returns `(x, y)` origin or `None` if full.
    pub(crate) fn allocate(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w > self.size || h > self.size {
            return None;
        }
        // Wrap to next shelf if current row is too narrow
        if self.cursor_x + w > self.size {
            self.shelf_y += self.shelf_height;
            self.shelf_height = 0;
            self.cursor_x = 0;
        }
        if self.shelf_y + h > self.size {
            return None; // atlas full
        }
        let (x, y) = (self.cursor_x, self.shelf_y);
        self.cursor_x += w;
        self.shelf_height = self.shelf_height.max(h);
        Some((x, y))
    }
}

// ─── Pending upload ──────────────────────────────────────────────────

/// Queued glyph pixel data, flushed to GPU at frame start by the backend.
pub struct PendingUpload {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
}

// ─── Per-instance data ───────────────────────────────────────────────

/// Per-instance data for instanced glyph rendering.
/// `pos`/`size` are in pixel coordinates; the vertex shader converts to NDC.
#[repr(C)]
#[derive(Copy, Clone, bytemuck_derive::Pod, bytemuck_derive::Zeroable)]
pub struct GlyphInstance {
    pub pos: [f32; 2],     // pixel position (top-left of glyph quad)
    pub size: [f32; 2],    // pixel size
    pub uv_pos: [f32; 2],  // atlas UV top-left
    pub uv_size: [f32; 2], // atlas UV size
    pub color: [f32; 4],   // RGBA color
}

/// Draw range clipped to a scissor rect.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScissoredRange {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub start: usize,
    pub end: usize,
}

// ─── Glyph cache (CPU) ──────────────────────────────────────────────

/// CPU-side glyph cache: rasterization, packing, and caching.
/// GPU upload/rendering is delegated to backend's `GlyphAtlasGpu`.
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
    glyph_id_cache: HashMap<(u32, fontdb::ID, FontStyle), GlyphEntry>,
    scale_context: ScaleContext,
    font_chains: HashMap<FontStyle, Vec<fontdb::ID>>,
    font_size: f32,
    alpha_buf: Vec<u8>,
    // Public metrics
    pub cell_width: f32,
    pub cell_height: f32,
    pub ascent: f32,
    pub atlas_needs_clear: bool,
}

impl GlyphCache {
    /// Create a new GlyphCache. Returns `(cache, primary_font_id)`.
    pub fn new(
        font_system: &mut FontSystem,
        font_size_pt: f32,
        dpi_scale: f64,
        family_name: &str,
        render_config: &RenderConfig,
    ) -> (Self, Option<fontdb::ID>) {
        let atlas_size = render_config.atlas_size;
        let max_instances = render_config.max_glyph_instances;

        // Convert point size to pixels: pt × (96 × scale) / 72
        let font_size = font_size_pt * (96.0 * dpi_scale as f32) / 72.0;

        // ── Font setup ──
        let base_chain = build_fallback_chain(font_system, family_name);
        let mut font_chains = HashMap::new();
        font_chains.insert(FontStyle::Regular, base_chain.clone());
        font_chains.insert(
            FontStyle::Bold,
            build_style_chain(font_system, &base_chain, FontStyle::Bold),
        );
        font_chains.insert(
            FontStyle::Italic,
            build_style_chain(font_system, &base_chain, FontStyle::Italic),
        );
        font_chains.insert(
            FontStyle::BoldItalic,
            build_style_chain(font_system, &base_chain, FontStyle::BoldItalic),
        );

        // ── Cell metrics from primary font ──
        let (cell_width, cell_height, ascent) =
            compute_cell_metrics(font_system, base_chain.first().copied(), font_size);

        let primary_font_id = base_chain.first().copied();

        (
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
                scale_context: ScaleContext::new(),
                font_chains,
                font_size,
                cell_width,
                cell_height,
                ascent,
                atlas_needs_clear: false,
                alpha_buf: Vec::new(),
            },
            primary_font_id,
        )
    }

    /// Ensure a glyph for `ch` with the given `style` is in the atlas.
    pub fn ensure_styled_char(
        &mut self,
        ch: char,
        style: FontStyle,
        font_system: &mut FontSystem,
    ) -> Option<GlyphEntry> {
        let key = (ch, style);
        if let Some(entry) = self.cache.get(&key) {
            return Some(*entry);
        }

        if ch == ' ' || ch == '\0' || ch.is_control() {
            self.cache.insert(key, GlyphEntry::EMPTY);
            return Some(GlyphEntry::EMPTY);
        }

        let font_ids = self
            .font_chains
            .get(&style)
            .or_else(|| self.font_chains.get(&FontStyle::Regular))?;
        let (font_id, glyph_id) = resolve_glyph(font_system, font_ids, ch)?;

        let image = rasterize_glyph(
            &mut self.scale_context,
            font_system,
            font_id,
            glyph_id,
            self.font_size,
            style,
        )?;

        let w = image.placement.width;
        let h = image.placement.height;
        if w == 0 || h == 0 {
            self.cache.insert(key, GlyphEntry::EMPTY);
            return Some(GlyphEntry::EMPTY);
        }

        let entry = self.upload_rasterized(image)?;
        self.cache.insert(key, entry);
        Some(entry)
    }

    /// Ensure a glyph by its ID (from text shaping) is in the atlas.
    pub fn ensure_glyph_id(
        &mut self,
        glyph_id: u32,
        font_id: fontdb::ID,
        style: FontStyle,
        font_system: &mut FontSystem,
    ) -> Option<GlyphEntry> {
        let key = (glyph_id, font_id, style);
        if let Some(entry) = self.glyph_id_cache.get(&key) {
            return Some(*entry);
        }
        if glyph_id == 0 {
            self.glyph_id_cache.insert(key, GlyphEntry::EMPTY);
            return Some(GlyphEntry::EMPTY);
        }

        let image = rasterize_glyph(
            &mut self.scale_context,
            font_system,
            font_id,
            glyph_id as u16,
            self.font_size,
            style,
        )?;

        let w = image.placement.width;
        let h = image.placement.height;
        if w == 0 || h == 0 {
            self.glyph_id_cache.insert(key, GlyphEntry::EMPTY);
            return Some(GlyphEntry::EMPTY);
        }

        let entry = self.upload_rasterized(image)?;
        self.glyph_id_cache.insert(key, entry);
        Some(entry)
    }

    /// Allocate atlas space, queue pixel data for upload, return the entry.
    /// Moves `image.data` instead of cloning to avoid extra allocations.
    fn upload_rasterized(
        &mut self,
        image: swash::scale::image::Image,
    ) -> Option<GlyphEntry> {
        let w = image.placement.width;
        let h = image.placement.height;
        let is_color = matches!(image.content, Content::Color);
        if is_color {
            let (ax, ay) = match self.color_packer.allocate(w, h) {
                Some(pos) => pos,
                None => {
                    log::warn!("color atlas full, flagging for clear");
                    self.atlas_needs_clear = true;
                    return None;
                }
            };
            let entry = make_glyph_entry(ax, ay, w, h, &image, self.atlas_size, true);
            self.color_pending.push(PendingUpload {
                x: ax, y: ay, w, h,
                data: image.data, // move, not clone
            });
            Some(entry)
        } else {
            to_alpha_into(&image.data, w, h, &mut self.alpha_buf);
            let (ax, ay) = match self.alpha_packer.allocate(w, h) {
                Some(pos) => pos,
                None => {
                    log::warn!("alpha atlas full, flagging for clear");
                    self.atlas_needs_clear = true;
                    return None;
                }
            };
            let entry = make_glyph_entry(ax, ay, w, h, &image, self.atlas_size, false);
            self.alpha_pending.push(PendingUpload {
                x: ax, y: ay, w, h,
                data: std::mem::take(&mut self.alpha_buf), // move, not clone
            });
            Some(entry)
        }
    }

    /// Ensure a regular-style character is in the atlas.
    pub fn ensure_char(
        &mut self,
        ch: char,
        font_system: &mut FontSystem,
    ) -> Option<GlyphEntry> {
        self.ensure_styled_char(ch, FontStyle::Regular, font_system)
    }

    /// Drain pending glyph uploads for the GPU backend to consume.
    /// Returns `(alpha_uploads, color_uploads, alpha_needs_clear, color_needs_clear)`.
    pub fn take_pending(
        &mut self,
    ) -> (Vec<PendingUpload>, Vec<PendingUpload>, bool, bool) {
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

// ─── Helpers ─────────────────────────────────────────────────────────

/// Compute cell width, height, and ascent from the primary font.
fn compute_cell_metrics(
    font_system: &mut FontSystem,
    primary_id: Option<fontdb::ID>,
    font_size: f32,
) -> (f32, f32, f32) {
    let fallback = (font_size * 0.6, font_size * 1.2, font_size * 1.2 * 0.8);

    let Some(fid) = primary_id else {
        return fallback;
    };
    let Some(font) = font_system.get_font(fid) else {
        return fallback;
    };

    let swash_font = font.as_swash();
    let metrics = swash_font.metrics(&[]);
    let scale = font_size / metrics.units_per_em as f32;
    let ascent = (metrics.ascent * scale).ceil();
    let descent = (metrics.descent * scale).ceil();
    let height = (ascent + descent).ceil();

    let glyph_id = swash_font.charmap().map('M');
    let advance = swash_font.glyph_metrics(&[]).advance_width(glyph_id) * scale;
    let cw = advance.ceil();
    let ch = height.max(font_size * 1.2);
    let safe_ascent = ascent.min(ch);

    log::info!(
        "font metrics: ascent={ascent:.1} descent={descent:.1} height={height:.1} cw={cw:.1} ch={ch:.1}"
    );
    (cw, ch, safe_ascent)
}

/// Find which font in the fallback chain contains `ch`, returning (font_id, glyph_id).
fn resolve_glyph(
    font_system: &mut FontSystem,
    font_ids: &[fontdb::ID],
    ch: char,
) -> Option<(fontdb::ID, u16)> {
    for &fid in font_ids {
        if let Some(font) = font_system.get_font(fid) {
            let gid = font.as_swash().charmap().map(ch);
            if gid != 0 {
                return Some((fid, gid));
            }
        }
    }
    None
}

/// Rasterize a glyph with optional synthetic bold/italic.
fn rasterize_glyph(
    scale_ctx: &mut ScaleContext,
    font_system: &mut FontSystem,
    font_id: fontdb::ID,
    glyph_id: u16,
    font_size: f32,
    style: FontStyle,
) -> Option<swash::scale::image::Image> {
    let font = font_system.get_font(font_id)?;
    let swash_font = font.as_swash();

    let need_synth_bold = matches!(style, FontStyle::Bold | FontStyle::BoldItalic) && {
        let db = font_system.db();
        db.face(font_id).is_some_and(|f| f.weight.0 < 600)
    };
    let need_synth_italic = matches!(style, FontStyle::Italic | FontStyle::BoldItalic) && {
        let db = font_system.db();
        db.face(font_id)
            .is_some_and(|f| f.style == fontdb::Style::Normal)
    };

    let mut scaler = scale_ctx
        .builder(swash_font)
        .size(font_size)
        .hint(true)
        .build();

    let color_image = {
        let mut r = Render::new(&[
            Source::ColorBitmap(StrikeWith::BestFit),
            Source::ColorOutline(0),
        ]);
        r.format(Format::Subpixel)
            .offset(swash::zeno::Vector::new(0.0, 0.0));
        r.render(&mut scaler, glyph_id)
    };

    color_image.or_else(|| {
        let italic_transform = need_synth_italic.then_some(swash::zeno::Transform {
            xx: 1.0,
            yx: 0.0,
            xy: 0.2125,
            yy: 1.0,
            x: 0.0,
            y: 0.0,
        });
        let embolden = if need_synth_bold {
            0.02 * font_size
        } else {
            0.0
        };

        let mut r = Render::new(&[Source::Outline]);
        r.format(Format::Alpha)
            .offset(swash::zeno::Vector::new(0.0, 0.0))
            .transform(italic_transform)
            .embolden(embolden);
        r.render(&mut scaler, glyph_id)
    })
}

/// Convert rasterized pixel data to single-channel alpha into an existing buffer.
fn to_alpha_into(data: &[u8], w: u32, h: u32, buf: &mut Vec<u8>) {
    let expected_alpha = (w * h) as usize;
    let expected_rgba = (w * h * 4) as usize;

    buf.clear();
    if data.len() == expected_alpha {
        buf.extend_from_slice(data);
    } else if data.len() == expected_rgba {
        buf.reserve(expected_alpha);
        buf.extend(data.iter().skip(3).step_by(4).copied());
    } else {
        buf.reserve(expected_alpha);
        buf.extend(
            data.chunks(3)
                .map(|rgb| ((rgb[0] as u16 + rgb[1] as u16 + rgb[2] as u16) / 3) as u8),
        );
    }
}

/// Convert rasterized pixel data to single-channel alpha (allocating variant for tests).
#[cfg(test)]
fn to_alpha(data: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut buf = Vec::new();
    to_alpha_into(data, w, h, &mut buf);
    buf
}

/// Build a `GlyphEntry` from atlas coordinates and image placement.
fn make_glyph_entry(
    ax: u32,
    ay: u32,
    w: u32,
    h: u32,
    image: &swash::scale::image::Image,
    atlas_size: u32,
    is_color: bool,
) -> GlyphEntry {
    let s = atlas_size as f32;
    GlyphEntry {
        u0: ax as f32 / s,
        v0: ay as f32 / s,
        u1: (ax + w) as f32 / s,
        v1: (ay + h) as f32 / s,
        width: w as u16,
        height: h as u16,
        bearing_x: image.placement.left as i16,
        bearing_y: image.placement.top as i16,
        is_color,
    }
}

// ─── Font fallback chains ────────────────────────────────────────────

/// Build font fallback chain using fontconfig (Unix) or simple scan (Windows).
fn build_fallback_chain(font_system: &mut FontSystem, family_name: &str) -> Vec<fontdb::ID> {
    let mut font_ids = Vec::new();

    #[cfg(unix)]
    {
        use fontconfig::{Fontconfig, Pattern};
        use std::ffi::CString;

        if let Some(fc) = Fontconfig::new() {
            let db = font_system.db();
            let family_lower = family_name.to_ascii_lowercase();
            for face in db.faces() {
                for family in &face.families {
                    if family.0.eq_ignore_ascii_case(family_name)
                        || family.0.to_ascii_lowercase().contains(&family_lower)
                    {
                        if !font_ids.contains(&face.id) {
                            font_ids.push(face.id);
                        }
                        break;
                    }
                }
            }

            let mut pat = Pattern::new(&fc);
            if let Ok(fam) = CString::new(family_name) {
                pat.add_string(c"family", &fam);
            }
            let sorted = pat.sort_fonts(false);

            let db = font_system.db();
            let mut path_to_ids: HashMap<(String, u32), fontdb::ID> = HashMap::new();
            for face in db.faces() {
                if let fontdb::Source::File(ref path) = face.source {
                    path_to_ids.insert((path.to_string_lossy().to_string(), face.index), face.id);
                }
            }

            for fc_font in sorted.iter() {
                let Some(fc_path_raw) = fc_font.filename() else {
                    continue;
                };
                let fc_path = fc_path_raw.replace("\\", "");
                let fc_index = fc_font.face_index().unwrap_or(0) as u32;
                if let Some(&id) = path_to_ids.get(&(fc_path, fc_index))
                    && !font_ids.contains(&id)
                {
                    font_ids.push(id);
                }
            }

            for face in db.faces() {
                if !font_ids.contains(&face.id) {
                    font_ids.push(face.id);
                }
            }
        }
    }

    if font_ids.is_empty() {
        let db = font_system.db();
        let mut primary = None;
        let mut first_mono = None;
        let family_lower = family_name.to_ascii_lowercase();
        for face in db.faces() {
            if first_mono.is_none() && face.monospaced {
                first_mono = Some(face.id);
            }
            for family in &face.families {
                if family.0.eq_ignore_ascii_case(family_name)
                    || (family_name != "monospace"
                        && family.0.to_ascii_lowercase().contains(&family_lower))
                {
                    primary = Some(face.id);
                }
            }
        }
        if let Some(id) = primary.or(first_mono) {
            font_ids.push(id);
        }
        for face in db.faces() {
            if !font_ids.contains(&face.id) {
                font_ids.push(face.id);
            }
        }
    }

    if let Some(&first) = font_ids.first() {
        let db = font_system.db();
        if let Some(face) = db.face(first) {
            let name = face.families.first().map(|f| f.0.as_str()).unwrap_or("?");
            log::info!("primary font: {name} (monospaced={})", face.monospaced);
        }
    }
    log::info!("font fallback chain: {} fonts total", font_ids.len());
    font_ids
}

/// Build a style-specific fallback chain.
fn build_style_chain(
    font_system: &mut FontSystem,
    base_chain: &[fontdb::ID],
    style: FontStyle,
) -> Vec<fontdb::ID> {
    if matches!(style, FontStyle::Regular) {
        return base_chain.to_vec();
    }

    let want_bold = matches!(style, FontStyle::Bold | FontStyle::BoldItalic);
    let want_italic = matches!(style, FontStyle::Italic | FontStyle::BoldItalic);

    let db = font_system.db();
    let primary_family = base_chain
        .first()
        .and_then(|id| db.face(*id))
        .and_then(|f| f.families.first())
        .map(|f| f.0.clone())
        .unwrap_or_default();

    let mut style_ids: Vec<fontdb::ID> = base_chain
        .iter()
        .filter(|&&fid| {
            db.face(fid).is_some_and(|face| {
                let is_bold = face.weight.0 >= 600;
                let is_italic = face.style != fontdb::Style::Normal;
                let family_match = face.families.iter().any(|f| f.0 == primary_family);
                family_match && is_bold == want_bold && is_italic == want_italic
            })
        })
        .copied()
        .collect();

    if !style_ids.is_empty() {
        for &fid in base_chain {
            if !style_ids.contains(&fid) {
                style_ids.push(fid);
            }
        }
        let name = db
            .face(style_ids[0])
            .and_then(|f| f.families.first())
            .map(|f| f.0.as_str())
            .unwrap_or("?");
        log::info!("font style {:?}: using {name}", style);
        return style_ids;
    }

    log::info!("font style {:?}: no variant found, using regular", style);
    base_chain.to_vec()
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelf_packer_basic() {
        let mut p = ShelfPacker::new(100);
        assert_eq!(p.allocate(10, 10), Some((0, 0)));
        assert_eq!(p.allocate(10, 10), Some((10, 0)));
    }

    #[test]
    fn shelf_packer_wrap() {
        let mut p = ShelfPacker::new(100);
        for _ in 0..10 {
            assert!(p.allocate(10, 20).is_some());
        }
        assert_eq!(p.allocate(10, 15), Some((0, 20)));
    }

    #[test]
    fn shelf_packer_full() {
        let mut p = ShelfPacker::new(20);
        assert!(p.allocate(20, 20).is_some());
        assert!(p.allocate(1, 1).is_none());
    }

    #[test]
    fn shelf_packer_oversized() {
        let mut p = ShelfPacker::new(10);
        assert!(p.allocate(11, 5).is_none());
        assert!(p.allocate(5, 11).is_none());
    }

    #[test]
    fn font_style_hash_distinct() {
        let mut map = HashMap::new();
        map.insert(('A', FontStyle::Regular), 1);
        map.insert(('A', FontStyle::Bold), 2);
        map.insert(('A', FontStyle::Italic), 3);
        assert_eq!(map.len(), 3);
        assert_eq!(map[&('A', FontStyle::Bold)], 2);
    }

    #[test]
    fn font_style_from_bold_italic() {
        assert_eq!(
            FontStyle::from_bold_italic(false, false),
            FontStyle::Regular
        );
        assert_eq!(FontStyle::from_bold_italic(true, false), FontStyle::Bold);
        assert_eq!(FontStyle::from_bold_italic(false, true), FontStyle::Italic);
        assert_eq!(
            FontStyle::from_bold_italic(true, true),
            FontStyle::BoldItalic
        );
    }

    #[test]
    fn to_alpha_passthrough() {
        let data = vec![100, 200, 50, 255];
        assert_eq!(to_alpha(&data, 2, 2), data);
    }

    #[test]
    fn to_alpha_from_rgba() {
        let data = vec![10, 20, 30, 128];
        assert_eq!(to_alpha(&data, 1, 1), vec![128]);
    }
}
