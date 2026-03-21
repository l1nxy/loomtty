//! CPU glyph cache — rasterizes and caches terminal text glyphs.
//!
//! Two atlas layers:
//! - **Text** (`R8Unorm`): grayscale alpha mask via crossfont
//! - **Color** (`Rgba8Srgb`): color emoji via crossfont
//!
//! The GPU upload/rendering is handled by the backend's `GlyphAtlasGpu`.

use ciri_config::config::RenderConfig;
use crossfont::{
    BitmapBuffer, FontDesc, FontKey, GlyphKey, Rasterize, Rasterizer, Size, Slant, Style, Weight,
};
use freetype::face::LoadFlag;
use freetype::Library as FtLibrary;
use std::collections::HashMap;

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

// ─── Font key set ────────────────────────────────────────────────────

/// The 4 crossfont FontKeys for regular/bold/italic/bold_italic.
struct FontKeySet {
    regular: FontKey,
    bold: FontKey,
    italic: FontKey,
    bold_italic: FontKey,
}

impl FontKeySet {
    fn get(&self, style: FontStyle) -> FontKey {
        match style {
            FontStyle::Regular => self.regular,
            FontStyle::Bold => self.bold,
            FontStyle::Italic => self.italic,
            FontStyle::BoldItalic => self.bold_italic,
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
    glyph_id_cache: HashMap<(u32, FontStyle), GlyphEntry>,
    // Crossfont rasterizer (character-based rendering)
    rasterizer: Rasterizer,
    font_keys: FontKeySet,
    font_size: Size,
    // Thin FreeType path (glyph-ID rendering for shaped glyphs)
    // Keep library alive — ft_face borrows from it.
    #[allow(dead_code)]
    ft_library: FtLibrary,
    ft_face: Option<freetype::Face>,
    ft_pixel_size: f32,
    // Public metrics
    pub cell_width: f32,
    pub cell_height: f32,
    pub ascent: f32,
    pub atlas_needs_clear: bool,
}

impl GlyphCache {
    /// Create a new self-contained GlyphCache.
    ///
    /// `primary_font_path` is the file path + face index for the thin FreeType
    /// path used by `ensure_glyph_id()`. Obtained from `TextShaper::primary_font_path()`.
    pub fn new(
        font_size_pt: f32,
        dpi_scale: f64,
        family_name: &str,
        primary_font_path: Option<(String, u32)>,
        render_config: &RenderConfig,
    ) -> Self {
        let atlas_size = render_config.atlas_size;
        let max_instances = render_config.max_glyph_instances;

        // ── Crossfont setup ──
        let mut rasterizer = Rasterizer::new().expect("crossfont init failed");
        // Convert point size to pixels: pt × (96 × scale) / 72, then use from_px
        let pixel_size = font_size_pt * (96.0 * dpi_scale as f32) / 72.0;
        let font_size = Size::from_px(pixel_size);

        let regular_desc = FontDesc::new(
            family_name,
            Style::Description { slant: Slant::Normal, weight: Weight::Normal },
        );
        let regular_key = rasterizer
            .load_font(&regular_desc, font_size)
            .or_else(|e| {
                log::warn!("font '{}' not found ({:?}), trying monospace fallback", family_name, e);
                rasterizer.load_font(
                    &FontDesc::new("monospace", Style::Description { slant: Slant::Normal, weight: Weight::Normal }),
                    font_size,
                )
            })
            .expect("no usable font found (neither configured nor monospace fallback)");

        let bold_key = rasterizer
            .load_font(
                &FontDesc::new(family_name, Style::Description { slant: Slant::Normal, weight: Weight::Bold }),
                font_size,
            )
            .unwrap_or(regular_key);

        let italic_key = rasterizer
            .load_font(
                &FontDesc::new(family_name, Style::Description { slant: Slant::Italic, weight: Weight::Normal }),
                font_size,
            )
            .unwrap_or(regular_key);

        let bold_italic_key = rasterizer
            .load_font(
                &FontDesc::new(family_name, Style::Description { slant: Slant::Italic, weight: Weight::Bold }),
                font_size,
            )
            .unwrap_or(regular_key);

        // ── Metrics from crossfont ──
        // Force char size initialization by rasterizing a probe glyph first.
        // crossfont sets FreeType char size lazily in get_glyph(), so metrics()
        // returns zeros if called before any glyph has been rasterized.
        let _ = rasterizer.get_glyph(GlyphKey {
            character: 'M',
            font_key: regular_key,
            size: font_size,
        });
        let metrics = rasterizer
            .metrics(regular_key, font_size)
            .expect("failed to get font metrics");
        let cell_width = (metrics.average_advance as f32).ceil();
        let cell_height = (metrics.line_height as f32).ceil();
        let ascent = (metrics.line_height as f32 + metrics.descent).ceil();
        let safe_ascent = ascent.min(cell_height);

        log::info!(
            "font metrics: ascent={safe_ascent:.1} descent={:.1} line_height={:.1} cw={cell_width:.1} ch={cell_height:.1}",
            metrics.descent,
            metrics.line_height,
        );

        // ── Thin FreeType path for glyph-ID rendering ──
        let ft_library = FtLibrary::init().expect("FreeType init failed");
        let ft_pixel_size = font_size_pt * (96.0 * dpi_scale as f32) / 72.0;
        let ft_face = primary_font_path.and_then(|(path, index)| {
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
            ft_library,
            ft_face,
            ft_pixel_size,
            cell_width,
            cell_height,
            ascent: safe_ascent,
            atlas_needs_clear: false,
        }
    }

    /// Ensure a glyph for `ch` with the given `style` is in the atlas.
    pub fn ensure_styled_char(
        &mut self,
        ch: char,
        style: FontStyle,
    ) -> Option<GlyphEntry> {
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
        let entry = self.upload_rasterized(rasterized)?;
        self.cache.insert(key, entry);
        Some(entry)
    }

    /// Ensure a glyph by its ID (from text shaping) is in the atlas.
    /// Uses the thin FreeType path since crossfont only accepts characters.
    pub fn ensure_glyph_id(
        &mut self,
        glyph_id: u32,
        _font_id: fontdb::ID,
        style: FontStyle,
    ) -> Option<GlyphEntry> {
        let key = (glyph_id, style);
        if let Some(entry) = self.glyph_id_cache.get(&key) {
            return Some(*entry);
        }
        if glyph_id == 0 {
            self.glyph_id_cache.insert(key, GlyphEntry::EMPTY);
            return Some(GlyphEntry::EMPTY);
        }

        let glyph = self.rasterize_glyph_id_ft(glyph_id, style)?;

        if glyph.width == 0 || glyph.height == 0 {
            self.glyph_id_cache.insert(key, GlyphEntry::EMPTY);
            return Some(GlyphEntry::EMPTY);
        }

        let entry = self.upload_rasterized(glyph)?;
        self.glyph_id_cache.insert(key, entry);
        Some(entry)
    }

    /// Rasterize a glyph by ID using the thin FreeType path.
    /// Tries color bitmap first (for emoji), then falls back to grayscale outline.
    fn rasterize_glyph_id_ft(&self, glyph_id: u32, style: FontStyle) -> Option<RasterizedGlyph> {
        let ft_face = self.ft_face.as_ref()?;

        ft_face
            .set_char_size(0, (self.ft_pixel_size * 64.0) as isize, 72, 72)
            .ok()?;

        // Try color bitmap first (CBDT/sbix emoji)
        let color_flags = LoadFlag::COLOR | LoadFlag::RENDER;
        if ft_face.load_glyph(glyph_id, color_flags).is_ok() {
            let glyph = ft_face.glyph();
            let bitmap = glyph.bitmap();
            let is_bgra = matches!(bitmap.pixel_mode(), Ok(freetype::bitmap::PixelMode::Bgra));
            if is_bgra && bitmap.width() > 0 && bitmap.rows() > 0 {
                let w = bitmap.width() as u32;
                let h = bitmap.rows() as u32;
                let pitch = bitmap.pitch().unsigned_abs() as u32;
                let raw = bitmap.buffer();
                // Convert BGRA → RGBA
                let mut data = Vec::with_capacity((w * h * 4) as usize);
                for row in 0..h {
                    let start = (row * pitch) as usize;
                    for x in 0..w as usize {
                        let offset = start + x * 4;
                        if offset + 3 < raw.len() {
                            data.push(raw[offset + 2]); // R
                            data.push(raw[offset + 1]); // G
                            data.push(raw[offset]);     // B
                            data.push(raw[offset + 3]); // A
                        }
                    }
                }
                // Scale color bitmap to cell size if needed
                let target_h = self.cell_height as u32;
                if h != target_h && target_h > 0 {
                    let scale = target_h as f32 / h as f32;
                    let new_w = (w as f32 * scale).round() as u32;
                    let scaled = downsample_rgba(&data, w, h, new_w, target_h);
                    return Some(RasterizedGlyph {
                        width: new_w,
                        height: target_h,
                        bearing_x: (glyph.bitmap_left() as f32 * scale).round() as i16,
                        bearing_y: (glyph.bitmap_top() as f32 * scale).round() as i16,
                        is_color: true,
                        data: scaled,
                    });
                }
                return Some(RasterizedGlyph {
                    width: w,
                    height: h,
                    bearing_x: glyph.bitmap_left() as i16,
                    bearing_y: glyph.bitmap_top() as i16,
                    is_color: true,
                    data,
                });
            }
        }

        // Grayscale outline path
        let load_flags = LoadFlag::TARGET_LIGHT;
        ft_face.load_glyph(glyph_id, load_flags).ok()?;
        let glyph = ft_face.glyph();

        // Synthetic bold/italic transformations
        let need_synth_bold = matches!(style, FontStyle::Bold | FontStyle::BoldItalic);
        let need_synth_italic = matches!(style, FontStyle::Italic | FontStyle::BoldItalic);
        unsafe {
            let slot = (*ft_face.raw()).glyph;
            if (*slot).format == freetype::ffi::FT_GLYPH_FORMAT_OUTLINE {
                let outline = &mut (*slot).outline;
                if need_synth_bold {
                    let font_height = (*(*ft_face.raw()).size).metrics.height as f64;
                    let amount = (font_height * 64.0 / 2048.0).ceil() as i64;
                    freetype::ffi::FT_Outline_Embolden(outline, amount);
                }
                if need_synth_italic {
                    let matrix = freetype::ffi::FT_Matrix {
                        xx: 0x10000,
                        xy: (0.2125 * 65536.0) as i64,
                        yx: 0,
                        yy: 0x10000,
                    };
                    freetype::ffi::FT_Outline_Transform(outline, &matrix);
                }
            }
        }

        glyph.render_glyph(freetype::RenderMode::Normal).ok()?;

        let bitmap = glyph.bitmap();
        let w = bitmap.width() as u32;
        let h = bitmap.rows() as u32;
        if w == 0 || h == 0 {
            return None;
        }

        let pitch = bitmap.pitch().unsigned_abs() as u32;
        let raw = bitmap.buffer();
        let mut data = Vec::with_capacity((w * h) as usize);
        for row in 0..h {
            let start = (row * pitch) as usize;
            let end = start + w as usize;
            if end <= raw.len() {
                data.extend_from_slice(&raw[start..end]);
            }
        }

        Some(RasterizedGlyph {
            width: w,
            height: h,
            bearing_x: glyph.bitmap_left() as i16,
            bearing_y: glyph.bitmap_top() as i16,
            is_color: false,
            data,
        })
    }

    /// Allocate atlas space, queue pixel data for upload, return the entry.
    fn upload_rasterized(&mut self, glyph: RasterizedGlyph) -> Option<GlyphEntry> {
        let w = glyph.width;
        let h = glyph.height;
        if glyph.is_color {
            let (ax, ay) = match self.color_packer.allocate(w, h) {
                Some(pos) => pos,
                None => {
                    log::warn!("color atlas full, flagging for clear");
                    self.atlas_needs_clear = true;
                    return None;
                }
            };
            let entry = make_glyph_entry(
                ax, ay, w, h, glyph.bearing_x, glyph.bearing_y,
                self.atlas_size, true,
            );
            self.color_pending.push(PendingUpload {
                x: ax, y: ay, w, h,
                data: glyph.data,
            });
            Some(entry)
        } else {
            let (ax, ay) = match self.alpha_packer.allocate(w, h) {
                Some(pos) => pos,
                None => {
                    log::warn!("alpha atlas full, flagging for clear");
                    self.atlas_needs_clear = true;
                    return None;
                }
            };
            let entry = make_glyph_entry(
                ax, ay, w, h, glyph.bearing_x, glyph.bearing_y,
                self.atlas_size, false,
            );
            self.alpha_pending.push(PendingUpload {
                x: ax, y: ay, w, h,
                data: glyph.data,
            });
            Some(entry)
        }
    }

    /// Ensure a regular-style character is in the atlas.
    pub fn ensure_char(&mut self, ch: char) -> Option<GlyphEntry> {
        self.ensure_styled_char(ch, FontStyle::Regular)
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

/// Nearest-neighbor downscale of RGBA bitmap data.
fn downsample_rgba(src: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
    let mut out = vec![0u8; (dst_w * dst_h * 4) as usize];
    for dy in 0..dst_h {
        let sy = (dy as f32 * src_h as f32 / dst_h as f32) as u32;
        for dx in 0..dst_w {
            let sx = (dx as f32 * src_w as f32 / dst_w as f32) as u32;
            let si = ((sy * src_w + sx) * 4) as usize;
            let di = ((dy * dst_w + dx) * 4) as usize;
            if si + 3 < src.len() {
                out[di..di + 4].copy_from_slice(&src[si..si + 4]);
            }
        }
    }
    out
}

/// Convert a crossfont `RasterizedGlyph` to our internal format.
///
/// - `BitmapBuffer::Rgb` → single-channel alpha: `(R + G + B) / 3`
/// - `BitmapBuffer::Rgba` → color emoji: keep as-is
fn convert_crossfont_glyph(glyph: crossfont::RasterizedGlyph) -> RasterizedGlyph {
    let w = glyph.width as u32;
    let h = glyph.height as u32;

    match glyph.buffer {
        BitmapBuffer::Rgb(rgb_data) => {
            // Collapse RGB to single-channel alpha: (R + G + B) / 3
            let alpha_data: Vec<u8> = rgb_data
                .chunks(3)
                .map(|rgb| ((rgb[0] as u16 + rgb[1] as u16 + rgb[2] as u16) / 3) as u8)
                .collect();
            RasterizedGlyph {
                width: w,
                height: h,
                bearing_x: glyph.left as i16,
                bearing_y: glyph.top as i16,
                is_color: false,
                data: alpha_data,
            }
        }
        BitmapBuffer::Rgba(rgba_data) => {
            RasterizedGlyph {
                width: w,
                height: h,
                bearing_x: glyph.left as i16,
                bearing_y: glyph.top as i16,
                is_color: true,
                data: rgba_data,
            }
        }
    }
}

/// Backend-agnostic rasterized glyph data.
struct RasterizedGlyph {
    width: u32,
    height: u32,
    bearing_x: i16,
    bearing_y: i16,
    is_color: bool,
    data: Vec<u8>,
}

/// Build a `GlyphEntry` from atlas coordinates.
fn make_glyph_entry(
    ax: u32,
    ay: u32,
    w: u32,
    h: u32,
    bearing_x: i16,
    bearing_y: i16,
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
        bearing_x,
        bearing_y,
        is_color,
    }
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
    fn rgb_to_alpha_conversion() {
        // Grayscale (R=G=B): result should equal any channel
        let glyph = crossfont::RasterizedGlyph {
            character: 'A',
            width: 1,
            height: 1,
            top: 0,
            left: 0,
            advance: (0, 0),
            buffer: BitmapBuffer::Rgb(vec![128, 128, 128]),
        };
        let converted = convert_crossfont_glyph(glyph);
        assert!(!converted.is_color);
        assert_eq!(converted.data, vec![128]);
    }

    #[test]
    fn rgba_passthrough() {
        let glyph = crossfont::RasterizedGlyph {
            character: '😀',
            width: 1,
            height: 1,
            top: 0,
            left: 0,
            advance: (0, 0),
            buffer: BitmapBuffer::Rgba(vec![255, 0, 0, 128]),
        };
        let converted = convert_crossfont_glyph(glyph);
        assert!(converted.is_color);
        assert_eq!(converted.data, vec![255, 0, 0, 128]);
    }
}
