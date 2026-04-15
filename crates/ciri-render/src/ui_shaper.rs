//! UI text shaper — used only by UI chrome (palette, tab bar, status bar, …),
//! never by the terminal grid. Lets UI fonts be proportional (Inter, SF Pro,
//! Segoe UI) while the terminal keeps its monospaced grid.
//!
//! Platform backends:
//!   - Linux / Windows: `rustybuzz` (HarfBuzz port).
//!   - macOS: CoreText via `core_text::font::CTFont::get_glyphs_for_characters`
//!     and advances from `CTFontGetAdvancesForGlyphs`.
//!
//! A small LRU keeps the last N shaped runs so painting the same label each
//! frame doesn't re-shape. UI text has very high repeat rate.
//!
//! What callers get back (`UiShapedGlyph`):
//!   - `glyph_id` + `font_id` suitable for `GlyphCache::ensure_glyph_id`,
//!   - `x_advance` (pixels) for pen advance,
//!   - `cluster` (byte offset into the original `&str`) for hit-test /
//!     truncation decisions,
//!   - `x_offset` / `y_offset` (pixels) — rustybuzz's positioning output.

use std::collections::VecDeque;
use std::sync::Arc;

#[cfg(target_os = "macos")]
use core_text::font::CTFont;

/// Resolve a UI font family name to a (font file path, face index, fontdb ID).
/// Used by the app startup path to hand `ui_font_path` / `ui_font_id` into
/// both `UiTextShaper::new` and `FontInitParams`. Returns `None` if the
/// family can't be found on the system.
///
/// When `family` is empty, queries the platform's default sans-serif font
/// via `fontdb::Family::SansSerif` (fontconfig `sans-serif` on Linux,
/// system default on macOS/Windows).
pub fn resolve_ui_font(family: &str) -> Option<(String, u32, fontdb::ID)> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    let families: Vec<fontdb::Family<'_>> = if family.is_empty() {
        vec![fontdb::Family::SansSerif]
    } else {
        vec![
            fontdb::Family::Name(family),
            fontdb::Family::SansSerif,
        ]
    };

    let query = fontdb::Query {
        families: &families,
        ..Default::default()
    };
    let id = db.query(&query)?;
    let face = db.face(id)?;
    match &face.source {
        fontdb::Source::File(path) => {
            Some((path.to_string_lossy().to_string(), face.index, id))
        }
        _ => None,
    }
}

/// Shaped glyph produced by [`UiTextShaper::shape`].
#[derive(Debug, Clone, Copy)]
pub struct UiShapedGlyph {
    pub glyph_id: u32,
    pub font_id: fontdb::ID,
    pub x_advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
    /// Byte offset into the original `&str` this glyph starts at. Used for
    /// hit-testing and prefix truncation.
    pub cluster: u32,
}

/// Owned font data; the face is rebuilt on each shape() call (cheap — table
/// pointers only, no rasterization). Avoids the lifetime-erasing transmute
/// the previous design needed to keep a `Face<'static>` alongside its bytes.
#[cfg(not(target_os = "macos"))]
struct UiFace {
    data: Arc<Vec<u8>>,
    index: u32,
    units_per_em: f32,
    ascent: f32,
    descent: f32,
    line_gap: f32,
}

#[cfg(not(target_os = "macos"))]
impl UiFace {
    fn new(data: Arc<Vec<u8>>, index: u32) -> Option<Self> {
        let face = rustybuzz::Face::from_slice(&data, index)?;
        Some(UiFace {
            units_per_em: face.units_per_em() as f32,
            ascent: face.ascender() as f32,
            descent: face.descender() as f32,
            line_gap: face.line_gap() as f32,
            data,
            index,
        })
    }

    fn borrow_face(&self) -> rustybuzz::Face<'_> {
        // The cached metrics above guarantee the bytes already parsed once;
        // unwrap is safe because the same data parsed in `new`.
        rustybuzz::Face::from_slice(&self.data, self.index)
            .expect("UiFace data parsed once in new(), should reparse")
    }
}

/// Small fixed-size LRU used by [`UiTextShaper`] to cache shape results.
struct ShapeLru {
    cap: usize,
    order: VecDeque<String>,
    map: std::collections::HashMap<String, Vec<UiShapedGlyph>>,
}

impl ShapeLru {
    fn new(cap: usize) -> Self {
        Self {
            cap,
            order: VecDeque::with_capacity(cap),
            map: std::collections::HashMap::with_capacity(cap),
        }
    }

    fn get(&mut self, key: &str) -> Option<&Vec<UiShapedGlyph>> {
        if self.map.contains_key(key) {
            // Move to back (MRU).
            if let Some(pos) = self.order.iter().position(|k| k == key) {
                let k = self.order.remove(pos).unwrap();
                self.order.push_back(k);
            }
            return self.map.get(key);
        }
        None
    }

    fn put(&mut self, key: String, value: Vec<UiShapedGlyph>) {
        if self.map.contains_key(&key) {
            // Duplicate insert: refresh value and promote to MRU so callers
            // that re-shape don't surprise the eviction order.
            if let Some(pos) = self.order.iter().position(|k| k == &key) {
                let k = self.order.remove(pos).unwrap();
                self.order.push_back(k);
            }
            self.map.insert(key, value);
            return;
        }
        if self.order.len() == self.cap
            && let Some(oldest) = self.order.pop_front()
        {
            self.map.remove(&oldest);
        }
        self.map.insert(key.clone(), value);
        self.order.push_back(key);
    }
}

/// UI text shaper — see module-level docs.
pub struct UiTextShaper {
    #[cfg(not(target_os = "macos"))]
    face: Option<UiFace>,
    #[cfg(target_os = "macos")]
    font: Option<CTFont>,
    #[cfg(target_os = "macos")]
    #[allow(dead_code)]
    units_per_em: f32,
    #[cfg(target_os = "macos")]
    ct_ascent: f32,
    #[cfg(target_os = "macos")]
    ct_descent: f32,
    #[cfg(target_os = "macos")]
    ct_leading: f32,
    font_id: Option<fontdb::ID>,
    pixel_size: f32,
    cache: ShapeLru,
    /// Fallback x_advance used when no face is loaded. Comes from the
    /// terminal cell_width so text still lays out roughly right.
    fallback_advance: f32,
    /// Fallback line height (terminal cell_height) for the same reason.
    fallback_line_height: f32,
}

impl UiTextShaper {
    /// Capacity of the shape-result LRU. 256 is enough for the small handful
    /// of repeated UI strings we paint per frame (palette rows, tab titles,
    /// status segments) without blowing memory on rarely-seen strings.
    const CACHE_CAP: usize = 256;

    /// Build a shaper. Pass `font_path` from `TextShaper::primary_font_path()`
    /// or equivalent for the UI family, and the fallback advance/line height
    /// to use when no face is loaded (terminal cell metrics are fine).
    pub fn new(
        font_path: Option<(String, u32)>,
        font_id: Option<fontdb::ID>,
        pixel_size: f32,
        fallback_advance: f32,
        fallback_line_height: f32,
    ) -> Self {
        #[cfg(not(target_os = "macos"))]
        let face = font_path.as_ref().and_then(|(path, idx)| {
            let data = std::fs::read(path).ok()?;
            UiFace::new(Arc::new(data), *idx)
        });

        #[cfg(target_os = "macos")]
        let (font, upem, ct_asc, ct_desc, ct_lead) = match font_path.as_ref() {
            Some((path, idx)) => {
                let f = crate::shaper::load_ct_font_from_path(path, *idx, pixel_size as f64);
                if let Some(f) = f {
                    let upem = f.units_per_em() as f32;
                    // CoreText returns metrics in points (same as pixel_size in our setup).
                    let asc = f.ascent() as f32;
                    let desc = f.descent() as f32;
                    let lead = f.leading() as f32;
                    (Some(f), upem, asc, desc, lead)
                } else {
                    (None, 1000.0, 0.0, 0.0, 0.0)
                }
            }
            None => (None, 1000.0, 0.0, 0.0, 0.0),
        };

        #[cfg(not(target_os = "macos"))]
        let _ = font_path;

        Self {
            #[cfg(not(target_os = "macos"))]
            face,
            #[cfg(target_os = "macos")]
            font,
            #[cfg(target_os = "macos")]
            units_per_em: upem,
            #[cfg(target_os = "macos")]
            ct_ascent: ct_asc,
            #[cfg(target_os = "macos")]
            ct_descent: ct_desc,
            #[cfg(target_os = "macos")]
            ct_leading: ct_lead,
            font_id,
            pixel_size,
            cache: ShapeLru::new(Self::CACHE_CAP),
            fallback_advance,
            fallback_line_height,
        }
    }

    pub fn pixel_size(&self) -> f32 {
        self.pixel_size
    }

    pub fn font_id(&self) -> Option<fontdb::ID> {
        self.font_id
    }

    pub fn has_face(&self) -> bool {
        #[cfg(not(target_os = "macos"))]
        {
            self.face.is_some()
        }
        #[cfg(target_os = "macos")]
        {
            self.font.is_some()
        }
    }

    /// Total x-advance of `text` in pixels.
    pub fn measure(&mut self, text: &str) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        // Sum from cached result without cloning.
        if let Some(cached) = self.cache.get(text) {
            return cached.iter().map(|g| g.x_advance).sum();
        }
        let shaped = self.shape_uncached(text);
        let sum: f32 = shaped.iter().map(|g| g.x_advance).sum();
        self.cache.put(text.to_string(), shaped);
        sum
    }

    /// Shape `text`. Returns cached result when possible.
    pub fn shape(&mut self, text: &str) -> Vec<UiShapedGlyph> {
        if text.is_empty() {
            return Vec::new();
        }
        if let Some(cached) = self.cache.get(text) {
            return cached.clone();
        }
        let shaped = self.shape_uncached(text);
        self.cache.put(text.to_string(), shaped.clone());
        shaped
    }

    fn shape_uncached(&self, text: &str) -> Vec<UiShapedGlyph> {
        #[cfg(not(target_os = "macos"))]
        {
            if let Some(face) = self.face.as_ref()
                && let Some(fid) = self.font_id
            {
                return self.shape_rustybuzz(face, fid, text);
            }
        }
        #[cfg(target_os = "macos")]
        {
            if let Some(font) = self.font.as_ref()
                && let Some(fid) = self.font_id
            {
                return self.shape_coretext(font, fid, text);
            }
        }
        self.shape_fallback(text)
    }

    /// Fallback when no face is loaded: pretend each char takes
    /// `fallback_advance * unicode_width(ch)`. Glyph id is 0 so callers
    /// emit nothing from the atlas; they'll still advance the pen correctly.
    fn shape_fallback(&self, text: &str) -> Vec<UiShapedGlyph> {
        use unicode_width::UnicodeWidthChar;
        let mut out = Vec::with_capacity(text.len());
        let mut byte = 0usize;
        for ch in text.chars() {
            let w = UnicodeWidthChar::width(ch).unwrap_or(0).max(1) as f32;
            out.push(UiShapedGlyph {
                glyph_id: 0,
                font_id: fontdb::ID::dummy(),
                x_advance: self.fallback_advance * w,
                x_offset: 0.0,
                y_offset: 0.0,
                cluster: byte as u32,
            });
            byte += ch.len_utf8();
        }
        out
    }

    #[cfg(not(target_os = "macos"))]
    fn shape_rustybuzz(
        &self,
        face: &UiFace,
        font_id: fontdb::ID,
        text: &str,
    ) -> Vec<UiShapedGlyph> {
        let rb_face = face.borrow_face();
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(text);
        let output = rustybuzz::shape(&rb_face, &[], buffer);
        let infos = output.glyph_infos();
        let positions = output.glyph_positions();
        let scale = self.pixel_size / face.units_per_em.max(1.0);
        let mut out = Vec::with_capacity(infos.len());
        for (info, pos) in infos.iter().zip(positions.iter()) {
            out.push(UiShapedGlyph {
                glyph_id: info.glyph_id,
                font_id,
                x_advance: pos.x_advance as f32 * scale,
                x_offset: pos.x_offset as f32 * scale,
                y_offset: pos.y_offset as f32 * scale,
                cluster: info.cluster,
            });
        }
        out
    }

    #[cfg(target_os = "macos")]
    fn shape_coretext(
        &self,
        font: &CTFont,
        font_id: fontdb::ID,
        text: &str,
    ) -> Vec<UiShapedGlyph> {
        use core_foundation::attributed_string::CFMutableAttributedString;
        use core_foundation::base::{CFRange, TCFType};
        use core_foundation::string::CFString;
        use core_text::line::CTLine;
        use core_text::string_attributes::kCTFontAttributeName;

        if text.is_empty() {
            return Vec::new();
        }
        let cf_string = CFString::new(text);
        let mut attr_string = CFMutableAttributedString::new();
        attr_string.replace_str(&cf_string, CFRange::init(0, 0));
        let len = attr_string.char_len();
        unsafe {
            attr_string.set_attribute(CFRange::init(0, len), kCTFontAttributeName, font);
        }
        let line = CTLine::new_with_attributed_string(attr_string.as_concrete_TypeRef());

        // UTF-16 index → UTF-8 byte offset map (1-to-1 for BMP, 2-to-1 for
        // supplementary plane).
        let mut utf16_to_byte: Vec<usize> = Vec::with_capacity(text.len());
        for (b, c) in text.char_indices() {
            for _ in 0..c.len_utf16() {
                utf16_to_byte.push(b);
            }
        }

        // Note on units: CoreText `CTFont::new(size)` returns positions in
        // user-space units equal to the creation size. We pass `pixel_size`
        // (already pre-multiplied by dpi_scale) as the size, so positions
        // and run-typographic-bounds come back in device pixels — no
        // `pixel_size / units_per_em` scaling needed (that's specific to the
        // rustybuzz path, which works in raw font units).
        let mut out = Vec::new();
        for run in line.glyph_runs().iter() {
            let glyphs = run.glyphs();
            let positions = run.positions();
            let indices = run.string_indices();
            let n = glyphs.len();
            // Total typesetting width of the run; needed to compute the last
            // glyph's advance without falling back to the (possibly substituted
            // out) primary font's advance table.
            let run_total_w = unsafe {
                let r = core_foundation::base::CFRange::init(0, n as core_foundation::base::CFIndex);
                CTRunGetTypographicBounds(
                    run.as_concrete_TypeRef(),
                    r,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                ) as f32
            };
            for i in 0..n {
                let idx16 = indices[i] as usize;
                let advance = if i + 1 < n {
                    positions[i + 1].x as f32 - positions[i].x as f32
                } else {
                    // Last glyph: width = run total − x of last glyph.
                    run_total_w - positions[i].x as f32
                };
                let cluster = utf16_to_byte.get(idx16).copied().unwrap_or(text.len());
                out.push(UiShapedGlyph {
                    glyph_id: glyphs[i] as u32,
                    font_id,
                    x_advance: advance,
                    x_offset: 0.0,
                    y_offset: 0.0,
                    cluster: cluster as u32,
                });
            }
        }
        out
    }

    /// Line height in pixels (ascent − descent + line gap). If no face is
    /// loaded, returns the fallback value.
    pub fn line_height(&self) -> f32 {
        #[cfg(not(target_os = "macos"))]
        {
            if let Some(face) = self.face.as_ref() {
                let scale = self.pixel_size / face.units_per_em.max(1.0);
                return (face.ascent - face.descent + face.line_gap) * scale;
            }
        }
        #[cfg(target_os = "macos")]
        {
            if self.font.is_some() {
                return self.ct_ascent + self.ct_descent + self.ct_leading;
            }
        }
        self.fallback_line_height
    }

    /// Ascent in pixels. Used to position the baseline inside a row.
    pub fn ascent(&self) -> f32 {
        #[cfg(not(target_os = "macos"))]
        {
            if let Some(face) = self.face.as_ref() {
                let scale = self.pixel_size / face.units_per_em.max(1.0);
                return face.ascent * scale;
            }
        }
        #[cfg(target_os = "macos")]
        {
            if self.font.is_some() {
                return self.ct_ascent;
            }
        }
        self.fallback_line_height * 0.8
    }

    /// Given a byte offset into the original text, return the pen-x position
    /// at that boundary. Used for cursor positioning inside text inputs.
    pub fn pen_x_at_byte(&mut self, text: &str, byte_offset: usize) -> f32 {
        let shaped = self.shape(text);
        let mut pen = 0.0_f32;
        for g in &shaped {
            if (g.cluster as usize) >= byte_offset {
                break;
            }
            pen += g.x_advance;
        }
        pen
    }

    /// Longest prefix (in bytes) that fits inside `max_w`. Returns the byte
    /// length of the prefix and its pixel width. Ideal for truncating labels
    /// with an ellipsis appended.
    pub fn prefix_fit(&mut self, text: &str, max_w: f32) -> (usize, f32) {
        let shaped = self.shape(text);
        let mut pen = 0.0_f32;
        let mut last_cluster = 0usize;
        for g in &shaped {
            if pen + g.x_advance > max_w {
                return (last_cluster, pen);
            }
            pen += g.x_advance;
            last_cluster = g.cluster as usize + glyph_byte_len(text, g.cluster as usize);
        }
        (text.len(), pen)
    }
}

/// Length in bytes of the char that starts at `byte` in `text`. Safe: falls
/// back to 1 if the offset lands mid-char (shouldn't happen for a cluster
/// emitted by the shaper).
fn glyph_byte_len(text: &str, byte: usize) -> usize {
    text[byte..]
        .chars()
        .next()
        .map(|c| c.len_utf8())
        .unwrap_or(1)
}

// ─── CoreText FFI (only bits the shaper needs) ───────────────────────

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn CTRunGetTypographicBounds(
        run: core_text::run::CTRunRef,
        range: core_foundation::base::CFRange,
        ascent: *mut core_graphics::base::CGFloat,
        descent: *mut core_graphics::base::CGFloat,
        leading: *mut core_graphics::base::CGFloat,
    ) -> f64;
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measure_empty_is_zero() {
        let mut s = UiTextShaper::new(None, None, 12.0, 8.0, 16.0);
        assert_eq!(s.measure(""), 0.0);
    }

    #[test]
    fn fallback_measure_matches_sum_of_advances() {
        let mut s = UiTextShaper::new(None, None, 12.0, 8.0, 16.0);
        let m = s.measure("abc");
        let shaped = s.shape("abc");
        let sum: f32 = shaped.iter().map(|g| g.x_advance).sum();
        assert!((m - sum).abs() < 0.001);
    }

    #[test]
    fn fallback_prefix_fit_respects_max_width() {
        let mut s = UiTextShaper::new(None, None, 12.0, 10.0, 16.0);
        // 3 chars * 10 = 30 px; cap at 25 → fit 2 chars.
        let (bytes, w) = s.prefix_fit("abcd", 25.0);
        assert_eq!(bytes, 2);
        assert!((w - 20.0).abs() < 0.001);
    }

    #[test]
    fn lru_evicts_oldest() {
        let mut lru = ShapeLru::new(2);
        lru.put("a".into(), vec![]);
        lru.put("b".into(), vec![]);
        lru.put("c".into(), vec![]);
        assert!(lru.get("a").is_none());
        assert!(lru.get("b").is_some());
        assert!(lru.get("c").is_some());
    }

    #[test]
    fn lru_touches_on_get() {
        let mut lru = ShapeLru::new(2);
        lru.put("a".into(), vec![]);
        lru.put("b".into(), vec![]);
        let _ = lru.get("a");
        lru.put("c".into(), vec![]);
        assert!(lru.get("a").is_some(), "a was touched, should survive");
        assert!(lru.get("b").is_none(), "b is LRU, should be evicted");
    }
}
