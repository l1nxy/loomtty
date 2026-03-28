//! Cursor shape rendering.

use alacritty_terminal::vte::ansi::CursorShape;
use ciri_config::config::CiriConfig;
use ciri_config::theme::ThemeConfig;
use ciri_protocol::message::{
    CURSOR_BEAM, CURSOR_BLOCK, CURSOR_HIDDEN, CURSOR_HOLLOW_BLOCK, CURSOR_UNDERLINE,
};

use crate::rect::Rect;

use super::cell::CellMetrics;

/// Build cursor rects for the given cursor shape.
pub(super) fn build_cursor_rects(
    shape: u8,
    cx: f32,
    cy: f32,
    cw: f32,
    ch: f32,
    color: [f32; 4],
) -> Vec<Rect> {
    match shape {
        CURSOR_HIDDEN => Vec::new(),
        CURSOR_HOLLOW_BLOCK => {
            // Four 1px border lines forming a hollow rectangle
            let t = 1.0;
            vec![
                Rect {
                    x: cx,
                    y: cy,
                    w: cw,
                    h: t,
                    color,
                }, // top
                Rect {
                    x: cx,
                    y: cy + ch - t,
                    w: cw,
                    h: t,
                    color,
                }, // bottom
                Rect {
                    x: cx,
                    y: cy + t,
                    w: t,
                    h: ch - 2.0 * t,
                    color,
                }, // left
                Rect {
                    x: cx + cw - t,
                    y: cy + t,
                    w: t,
                    h: ch - 2.0 * t,
                    color,
                }, // right
            ]
        }
        CURSOR_BEAM => vec![Rect {
            x: cx,
            y: cy,
            w: 2.0,
            h: ch,
            color,
        }],
        CURSOR_UNDERLINE => vec![Rect {
            x: cx,
            y: cy + ch - 2.0,
            w: cw,
            h: 2.0,
            color,
        }],
        _ => vec![Rect {
            x: cx,
            y: cy,
            w: cw,
            h: ch,
            color,
        }], // solid block
    }
}

/// Map alacritty `CursorShape` to protocol cursor shape constant.
pub(super) fn cursor_shape_to_protocol(shape: CursorShape) -> u8 {
    match shape {
        CursorShape::Block => CURSOR_BLOCK,
        CursorShape::Underline => CURSOR_UNDERLINE,
        CursorShape::Beam => CURSOR_BEAM,
        CursorShape::Hidden => CURSOR_HIDDEN,
        CursorShape::HollowBlock => CURSOR_HOLLOW_BLOCK,
    }
}

/// Compute cursor rects from shape, position, and config.
pub(super) fn make_cursor_rects(
    shape: u8,
    line: i32,
    col: usize,
    total_rows: usize,
    m: &CellMetrics,
    config: &CiriConfig,
) -> Vec<Rect> {
    if shape == CURSOR_HIDDEN || line < 0 || (line as usize) >= total_rows {
        return Vec::new();
    }
    let cursor_color = ThemeConfig::parse_color(&config.terminal.cursor_color);
    let color = [
        cursor_color[0],
        cursor_color[1],
        cursor_color[2],
        config.terminal.cursor_opacity,
    ];
    build_cursor_rects(
        shape,
        col as f32 * m.cw,
        line as f32 * m.ch,
        m.cw,
        m.ch,
        color,
    )
}
