//! Glyph positioning and RelativeGlyph type.

use crate::glyph_cache::{GlyphCache, GlyphEntry};

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

/// Fit a glyph into a double-width cell, preserving aspect ratio, centered.
#[inline]
pub(super) fn constrain_wide_glyph(
    entry: &GlyphEntry,
    px: f32,
    py: f32,
    m: &CellMetrics,
    color: [f32; 4],
) -> RelativeGlyph {
    let gw = entry.width as f32;
    let gh = entry.height as f32;
    let target_w = m.cw * 2.0;
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
        // Only constrain color emoji in wide cells; text glyphs use bearing positioning
        let g = if cell.is_wide && entry.is_color {
            constrain_wide_glyph(&entry, px, py, m, cell.fg)
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
