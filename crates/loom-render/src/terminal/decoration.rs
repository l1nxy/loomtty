//! Underline, strikeout, and background strip rendering.

use crate::rect::Rect;

use super::cell::{CellMetrics, CellProps, UnderlineStyle};

/// Render a single cell: emit decorations and glyph (NOT background — handled by strip merger).
pub(super) fn render_cell(
    col: usize,
    cell: &CellProps,
    renderer: &mut super::view::CellRenderer<'_>,
) {
    let px = col as f32 * renderer.metrics.cw;
    let py = renderer.row as f32 * renderer.metrics.ch;
    let bg_width = if cell.is_wide {
        renderer.metrics.cw * 2.0
    } else {
        renderer.metrics.cw
    };

    // Hidden cells: no text or decorations (background handled by strip merger)
    if cell.is_hidden {
        return;
    }

    // Underline decoration
    if cell.underline != UnderlineStyle::None {
        let uy = py + renderer.metrics.baseline + 1.0 + renderer.metrics.underline_offset;
        emit_underline_rects(
            renderer.bg_rects,
            cell.underline,
            px,
            uy,
            bg_width,
            cell.fg,
            renderer.metrics.cw,
            renderer.metrics.underline_thickness,
        );
    }

    // Strikethrough: line through vertical center, configurable offset & thickness
    if cell.is_strikeout {
        let h = renderer.metrics.strikethrough_thickness;
        renderer.bg_rects.push(Rect {
            x: px,
            y: py + renderer.metrics.ch * 0.5 + renderer.metrics.strikethrough_offset
                - (h - 1.0) * 0.5,
            w: bg_width,
            h,
            color: cell.fg,
        });
    }

    // Skip whitespace / control chars (no glyph to render)
    let c = cell.ch;
    if c == ' ' || c == '\0' || c.is_control() {
        return;
    }

    // Box-drawing / block-element codepoints render via geometric rects
    // pinned to the cell, never via the font glyph — that's how WT and
    // Ghostty avoid the seam-brightening / corner-misalignment issues.
    if super::box_drawing::is_in_range(c)
        && super::box_drawing::emit(
            c,
            px,
            py,
            renderer.metrics.cw,
            renderer.metrics.ch,
            cell.fg,
            renderer.bg_rects,
        )
    {
        return;
    }

    // Rasterize and cache the glyph, then emit a rendering instance
    if let Some(entry) = renderer.atlas.ensure_styled_char(c, cell.style) {
        if entry.width == 0 || entry.height == 0 {
            return;
        }
        let color_span = super::glyph::color_glyph_cell_span(cell.ch, cell.is_wide).max(2);
        let glyph = if entry.is_color {
            super::glyph::constrain_color_glyph_to_cells(
                &entry,
                px,
                py,
                renderer.metrics,
                cell.fg,
                color_span,
            )
        } else {
            super::glyph::make_relative_glyph(&entry, px, py, renderer.metrics, cell.fg)
        };
        if entry.is_color {
            renderer.color_glyphs.push(glyph);
        } else {
            renderer.glyphs.push(glyph);
        }
    }
}

/// Emit underline decoration rects into `bg_rects`. `thickness` is the
/// resolved underline pixel height (≥ 1).
#[allow(clippy::too_many_arguments)] // Clippy 1.94: decoration emission keeps geometry/color scalars explicit.
pub(super) fn emit_underline_rects(
    rects: &mut Vec<Rect>,
    style: UnderlineStyle,
    px: f32,
    uy: f32,
    width: f32,
    color: [f32; 4],
    cell_width: f32,
    thickness: f32,
) {
    match style {
        UnderlineStyle::None => {}
        UnderlineStyle::Single => {
            rects.push(Rect {
                x: px,
                y: uy,
                w: width,
                h: thickness,
                color,
            });
        }
        UnderlineStyle::Double => {
            // Two `thickness`-tall lines with `thickness`-tall gap
            rects.push(Rect {
                x: px,
                y: uy,
                w: width,
                h: thickness,
                color,
            });
            rects.push(Rect {
                x: px,
                y: uy + 2.0 * thickness,
                w: width,
                h: thickness,
                color,
            });
        }
        UnderlineStyle::Curly => {
            // Approximate sine wave with 2px-wide rect segments. Thickness
            // controls dash height; amplitude scales with it so the wave
            // remains visible at heavier weights.
            let wave_len = cell_width.max(8.0);
            let segments = (width / 2.0).ceil() as usize;
            let amplitude = 1.5_f32.max(thickness);
            for i in 0..segments {
                let x = px + i as f32 * 2.0;
                let y_off = (i as f32 / wave_len * std::f32::consts::TAU).sin() * amplitude;
                let w = 2.0_f32.min(width - i as f32 * 2.0);
                if w > 0.0 {
                    rects.push(Rect {
                        x,
                        y: uy + y_off,
                        w,
                        h: thickness,
                        color,
                    });
                }
            }
        }
        UnderlineStyle::Dotted => {
            emit_dashed_line(rects, px, uy, width, color, 2.0, 2.0, thickness);
        }
        UnderlineStyle::Dashed => {
            emit_dashed_line(rects, px, uy, width, color, 4.0, 2.0, thickness);
        }
    }
}

/// Emit a dashed/dotted horizontal line as a series of small rects.
#[allow(clippy::too_many_arguments)] // Clippy 1.94: small geometry helper mirrors underline parameters directly.
fn emit_dashed_line(
    rects: &mut Vec<Rect>,
    px: f32,
    y: f32,
    total_width: f32,
    color: [f32; 4],
    dash_len: f32,
    gap_len: f32,
    thickness: f32,
) {
    let end = px + total_width;
    let mut x = px;
    while x < end {
        let w = dash_len.min(end - x);
        rects.push(Rect {
            x,
            y,
            w,
            h: thickness,
            color,
        });
        x += dash_len + gap_len;
    }
}

/// Render decorations (underline, strikeout) for a cell. Background is handled by strip merger.
pub(super) fn render_cell_decorations(
    row: usize,
    col: usize,
    cell: &CellProps,
    m: &CellMetrics,
    bg_rects: &mut Vec<Rect>,
) {
    if cell.is_hidden {
        return;
    }
    let px = col as f32 * m.cw;
    let py = row as f32 * m.ch;
    let bg_width = if cell.is_wide { m.cw * 2.0 } else { m.cw };
    if cell.underline != UnderlineStyle::None {
        emit_underline_rects(
            bg_rects,
            cell.underline,
            px,
            py + m.baseline + 1.0 + m.underline_offset,
            bg_width,
            cell.fg,
            m.cw,
            m.underline_thickness,
        );
    }
    if cell.is_strikeout {
        let h = m.strikethrough_thickness;
        bg_rects.push(Rect {
            x: px,
            y: py + m.ch * 0.5 + m.strikethrough_offset - (h - 1.0) * 0.5,
            w: bg_width,
            h,
            color: cell.fg,
        });
    }
}

/// Flush a pending background strip as a single rect.
/// Merges adjacent cells with the same background color into one rect per run,
/// reducing rect count from `cols` to typically 1–5 per row.
#[inline]
pub(super) fn flush_bg_strip(
    bg_rects: &mut Vec<Rect>,
    color: [f32; 4],
    start_col: usize,
    end_col: usize,
    row: usize,
    m: &CellMetrics,
) {
    let x = start_col as f32 * m.cw;
    let w = (end_col - start_col) as f32 * m.cw;
    bg_rects.push(Rect {
        x,
        y: row as f32 * m.ch,
        w,
        h: m.ch,
        color,
    });
}
