//! Glyph positioning and RelativeGlyph type.

use crate::glyph_cache::{GlyphCache, GlyphEntry};
use unicode_width::UnicodeWidthChar;

use super::cell::{CellMetrics, CellProps};

/// A glyph instance stored with pixel-relative position (not NDC).
/// NDC conversion happens at render time when the tile's screen offset is known.
#[derive(Clone, Copy)]
pub struct RelativeGlyph {
    pub px: f32,
    pub py: f32,
    pub glyph_w: f32,
    pub glyph_h: f32,
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
    pub color: [f32; 4],
}

/// Create a `RelativeGlyph` positioned by bearing offsets.
/// Applies face_width centering compensation when cell_width > face_width (from rounding).
#[inline]
pub(super) fn make_relative_glyph(
    entry: &GlyphEntry,
    px: f32,
    py: f32,
    m: &CellMetrics,
    color: [f32; 4],
) -> RelativeGlyph {
    // Center glyphs when cell is wider than face advance (due to rounding)
    let x_center = ((m.cw - m.face_width) / 2.0).round();
    RelativeGlyph {
        px: px + entry.bearing_x + x_center,
        py: py + m.baseline - entry.bearing_y,
        glyph_w: entry.width as f32,
        glyph_h: entry.height as f32,
        u0: entry.u0,
        v0: entry.v0,
        u1: entry.u1,
        v1: entry.v1,
        color,
    }
}

/// Fit a color glyph into `cols` terminal cells, preserving aspect ratio, centered.
#[inline]
pub(super) fn constrain_color_glyph_to_cells(
    entry: &GlyphEntry,
    px: f32,
    py: f32,
    m: &CellMetrics,
    color: [f32; 4],
    cols: usize,
) -> RelativeGlyph {
    let gw = entry.width as f32;
    let gh = entry.height as f32;
    let target_w = m.cw * cols.max(1) as f32;
    let target_h = m.ch;
    // Fit within target, preserving aspect ratio
    let scale = (target_w / gw).min(target_h / gh);
    let final_w = gw * scale;
    let final_h = gh * scale;
    // Center within the double-width cell
    let offset_x = (target_w - final_w) * 0.5;
    let offset_y = (target_h - final_h) * 0.5;
    RelativeGlyph {
        px: (px + offset_x).round(),
        py: (py + offset_y).round(),
        glyph_w: final_w,
        glyph_h: final_h,
        u0: entry.u0,
        v0: entry.v0,
        u1: entry.u1,
        v1: entry.v1,
        color,
    }
}

/// Compute the display cell span for a color glyph.
#[inline]
pub(super) fn color_glyph_cell_span(ch: char, is_wide: bool) -> usize {
    let flagged = if is_wide { 2 } else { 1 };
    let unicode_w = UnicodeWidthChar::width(ch).unwrap_or(0);
    let span = unicode_w.max(flagged).max(1);
    // Log interesting width classifications (CJK/emoji)
    if !ch.is_ascii() && (unicode_w >= 2 || is_wide) {
        log::debug!(
            "width calc: U+{:04X} '{}' unicode_width={} is_wide={} -> span={}",
            ch as u32,
            ch.escape_unicode(),
            unicode_w,
            is_wide,
            span,
        );
    }
    span
}

/// Place a wide CJK text glyph in a double-width cell using bearing positioning.
/// The CJK font is already size-adjusted (via ic_width scaling) so the glyph
/// naturally fills the double-width cell; no centering needed.
#[inline]
pub(super) fn constrain_wide_text_glyph(
    entry: &GlyphEntry,
    px: f32,
    py: f32,
    m: &CellMetrics,
    color: [f32; 4],
) -> RelativeGlyph {
    let gw = entry.width as f32;
    let target_w = m.cw * 2.0;
    let final_w = gw.min(target_w);
    RelativeGlyph {
        px: px + entry.bearing_x,
        py: py + m.baseline - entry.bearing_y,
        glyph_w: final_w,
        glyph_h: entry.height as f32,
        u0: entry.u0,
        v0: entry.v0,
        u1: entry.u1,
        v1: entry.v1,
        color,
    }
}

/// Emit a single glyph for a character at (col, row).
pub(super) fn emit_glyph(
    col: usize,
    row: usize,
    cell: &CellProps,
    m: &CellMetrics,
    atlas: &mut GlyphCache,
    glyphs: &mut Vec<RelativeGlyph>,
    color_glyphs: &mut Vec<RelativeGlyph>,
) {
    if let Some(entry) = atlas.ensure_styled_char(cell.ch, cell.style) {
        if entry.width == 0 || entry.height == 0 {
            return;
        }
        let px = col as f32 * m.cw;
        let py = row as f32 * m.ch;
        let color_span = color_glyph_cell_span(cell.ch, cell.is_wide);
        let g = if entry.is_color && color_span > 1 {
            constrain_color_glyph_to_cells(&entry, px, py, m, cell.fg, color_span)
        } else {
            make_relative_glyph(&entry, px, py, m, cell.fg)
        };
        if entry.is_color {
            color_glyphs.push(g);
        } else {
            glyphs.push(g);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_metrics() -> CellMetrics {
        CellMetrics {
            cw: 10.0,
            ch: 20.0,
            baseline: 16.0,
            face_width: 10.0,
            default_bg: [0.0, 0.0, 0.0, 1.0],
        }
    }

    fn test_entry() -> GlyphEntry {
        GlyphEntry {
            u0: 0.0,
            v0: 0.0,
            u1: 0.1,
            v1: 0.2,
            width: 8,
            height: 16,
            bearing_x: 1.0,
            bearing_y: 14.0,
            is_color: false,
        }
    }

    #[test]
    fn make_relative_glyph_applies_bearing_offset() {
        let m = test_metrics();
        let entry = test_entry();
        let g = make_relative_glyph(&entry, 100.0, 200.0, &m, [1.0; 4]);

        // px = 100 + bearing_x(1.0) + center(0.0 since cw==face_width)
        assert_eq!(g.px, 101.0);
        // py = 200 + baseline(16.0) - bearing_y(14.0)
        assert_eq!(g.py, 202.0);
        assert_eq!(g.glyph_w, 8.0);
        assert_eq!(g.glyph_h, 16.0);
    }

    #[test]
    fn make_relative_glyph_centers_when_cell_wider_than_face() {
        let mut m = test_metrics();
        m.cw = 12.0; // 2px wider than face_width
        m.face_width = 10.0;
        let entry = test_entry();
        let g = make_relative_glyph(&entry, 0.0, 0.0, &m, [1.0; 4]);

        // center offset = round((12-10)/2) = round(1.0) = 1.0
        assert_eq!(g.px, entry.bearing_x + 1.0);
    }

    #[test]
    fn constrain_wide_glyph_fits_to_double_cell() {
        let m = test_metrics();
        // A 32x32 emoji glyph fitting into 2*10 x 20 cell
        let entry = GlyphEntry {
            width: 32,
            height: 32,
            bearing_x: 0.0,
            bearing_y: 0.0,
            is_color: true,
            ..test_entry()
        };
        let g = constrain_color_glyph_to_cells(&entry, 0.0, 0.0, &m, [1.0; 4], 2);

        // target_w = 20, target_h = 20. scale = min(20/32, 20/32) = 0.625
        // final_w = 32 * 0.625 = 20, final_h = 32 * 0.625 = 20
        assert!((g.glyph_w - 20.0).abs() < 1e-3);
        assert!((g.glyph_h - 20.0).abs() < 1e-3);
    }

    #[test]
    fn constrain_wide_glyph_preserves_aspect_ratio() {
        let m = test_metrics();
        // Wide glyph (40x20): aspect 2:1
        let entry = GlyphEntry {
            width: 40,
            height: 20,
            is_color: true,
            ..test_entry()
        };
        let g = constrain_color_glyph_to_cells(&entry, 0.0, 0.0, &m, [1.0; 4], 2);

        let original_ratio = 40.0 / 20.0;
        let rendered_ratio = g.glyph_w / g.glyph_h;
        assert!(
            (original_ratio - rendered_ratio).abs() < 0.01,
            "aspect ratio changed: {original_ratio} vs {rendered_ratio}"
        );
    }

    #[test]
    fn constrain_wide_glyph_centers_in_double_cell() {
        let m = test_metrics();
        // Small square glyph
        let entry = GlyphEntry {
            width: 10,
            height: 10,
            is_color: true,
            ..test_entry()
        };
        let g = constrain_color_glyph_to_cells(&entry, 0.0, 0.0, &m, [1.0; 4], 2);

        // target = 2*cw x ch = 20x20. Scale = min(20/10, 20/10) = 2.0, final = 20x20
        // Centered: offset_x = (20-20)*0.5 = 0, offset_y = (20-20)*0.5 = 0
        assert!((g.glyph_w - 20.0).abs() < 0.01);
        assert_eq!(g.px, 0.0);
        assert_eq!(g.py, 0.0);
    }

    #[test]
    fn constrain_wide_text_glyph_uses_bearing_not_centering() {
        let m = test_metrics();
        let entry = GlyphEntry {
            width: 18,
            height: 16,
            bearing_x: 1.0,
            bearing_y: 14.0,
            is_color: false,
            ..test_entry()
        };
        let g = constrain_wide_text_glyph(&entry, 50.0, 100.0, &m, [1.0; 4]);

        // Uses bearing positioning, not centering
        assert_eq!(g.px, 50.0 + 1.0); // px + bearing_x
        assert_eq!(g.py, 100.0 + 16.0 - 14.0); // py + baseline - bearing_y
        // Width clamped to 2*cw = 20, glyph is 18 so stays 18
        assert_eq!(g.glyph_w, 18.0);
    }

    #[test]
    fn constrain_wide_text_glyph_clamps_overwide() {
        let m = test_metrics();
        // Glyph wider than 2*cw
        let entry = GlyphEntry {
            width: 30, // wider than 2*10=20
            height: 16,
            ..test_entry()
        };
        let g = constrain_wide_text_glyph(&entry, 0.0, 0.0, &m, [1.0; 4]);

        // Should be clamped to target_w = 2*10 = 20
        assert_eq!(g.glyph_w, 20.0);
    }

    #[test]
    fn color_glyph_cell_span_uses_unicode_width_for_emoji() {
        assert_eq!(color_glyph_cell_span('😀', false), 2);
        assert_eq!(color_glyph_cell_span('A', false), 1);
        assert_eq!(color_glyph_cell_span('中', true), 2);
    }

    #[test]
    fn glyph_entry_empty_is_zero() {
        let e = GlyphEntry::EMPTY;
        assert_eq!(e.width, 0);
        assert_eq!(e.height, 0);
        assert_eq!(e.u0, 0.0);
        assert_eq!(e.v0, 0.0);
        assert_eq!(e.u1, 0.0);
        assert_eq!(e.v1, 0.0);
        assert!(!e.is_color);
    }
}
