use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape, NamedColor};
use ciri_config::config::CiriConfig;
use ciri_config::theme::ThemeConfig;
use glyphon::FontSystem;

use crate::glyph_cache::GlyphAtlas;
use crate::rect::Rect;

fn ansi_color_to_rgba(color: AnsiColor, config: &CiriConfig) -> [f32; 4] {
    match color {
        AnsiColor::Named(named) => named_color_to_rgba(named, config),
        AnsiColor::Spec(rgb) => [rgb.r as f32 / 255.0, rgb.g as f32 / 255.0, rgb.b as f32 / 255.0, 1.0],
        AnsiColor::Indexed(idx) => indexed_color_to_rgba(idx, config),
    }
}

fn named_color_to_rgba(c: NamedColor, config: &CiriConfig) -> [f32; 4] {
    let theme = &config.theme;
    match c {
        NamedColor::Black => ThemeConfig::parse_color(&theme.black),
        NamedColor::Red => ThemeConfig::parse_color(&theme.red),
        NamedColor::Green => ThemeConfig::parse_color(&theme.green),
        NamedColor::Yellow => ThemeConfig::parse_color(&theme.yellow),
        NamedColor::Blue => ThemeConfig::parse_color(&theme.blue),
        NamedColor::Magenta => ThemeConfig::parse_color(&theme.magenta),
        NamedColor::Cyan => ThemeConfig::parse_color(&theme.cyan),
        NamedColor::White => ThemeConfig::parse_color(&theme.white),
        NamedColor::BrightBlack => ThemeConfig::parse_color(&theme.bright_black),
        NamedColor::BrightRed => ThemeConfig::parse_color(&theme.bright_red),
        NamedColor::BrightGreen => ThemeConfig::parse_color(&theme.bright_green),
        NamedColor::BrightYellow => ThemeConfig::parse_color(&theme.bright_yellow),
        NamedColor::BrightBlue => ThemeConfig::parse_color(&theme.bright_blue),
        NamedColor::BrightMagenta => ThemeConfig::parse_color(&theme.bright_magenta),
        NamedColor::BrightCyan => ThemeConfig::parse_color(&theme.bright_cyan),
        NamedColor::BrightWhite | NamedColor::Foreground => ThemeConfig::parse_color(&theme.foreground),
        NamedColor::Background => ThemeConfig::parse_color(&theme.background),
        _ => ThemeConfig::parse_color(&theme.foreground),
    }
}

fn indexed_color_to_rgba(idx: u8, config: &CiriConfig) -> [f32; 4] {
    if idx < 16 {
        return named_color_to_rgba(match idx {
            0 => NamedColor::Black, 1 => NamedColor::Red,
            2 => NamedColor::Green, 3 => NamedColor::Yellow,
            4 => NamedColor::Blue, 5 => NamedColor::Magenta,
            6 => NamedColor::Cyan, 7 => NamedColor::White,
            8 => NamedColor::BrightBlack, 9 => NamedColor::BrightRed,
            10 => NamedColor::BrightGreen, 11 => NamedColor::BrightYellow,
            12 => NamedColor::BrightBlue, 13 => NamedColor::BrightMagenta,
            14 => NamedColor::BrightCyan, 15 => NamedColor::BrightWhite,
            _ => unreachable!(),
        }, config);
    }
    if idx < 232 {
        let idx = idx - 16;
        let r = (idx / 36) % 6;
        let g = (idx / 6) % 6;
        let b = idx % 6;
        let to_f = |v: u8| if v == 0 { 0.0 } else { (55.0 + 40.0 * v as f32) / 255.0 };
        return [to_f(r), to_f(g), to_f(b), 1.0];
    }
    let v = (8 + 10 * (idx - 232) as u32) as f32 / 255.0;
    [v, v, v, 1.0]
}

/// Cached terminal view with positions RELATIVE to the tile's inner origin (0,0).
/// The actual screen offset is applied at render time, NOT baked into the cache.
pub struct TerminalView {
    /// Glyph instances with pixel positions relative to (0, 0).
    pub glyph_instances: Vec<RelativeGlyph>,
    /// Background rects with pixel positions relative to (0, 0).
    pub bg_rects: Vec<Rect>,
    /// Cursor rects relative to (0, 0). One rect for solid/beam/underline, four for hollow outline.
    pub cursor_rects: Vec<Rect>,
}

/// A glyph instance stored with pixel-relative position (not NDC).
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

/// Build rendering data from a terminal. Positions are relative to (0, 0).
pub fn build_terminal_view<T: alacritty_terminal::event::EventListener>(
    term: &Term<T>,
    atlas: &mut GlyphAtlas,
    font_system: &mut FontSystem,
    queue: &wgpu::Queue,
    config: &CiriConfig,
) -> TerminalView {
    let grid = term.grid();
    let cols = grid.columns();
    let total_rows = grid.screen_lines();
    let content = term.renderable_content();

    let cw = atlas.cell_width;
    let ch = atlas.cell_height;
    let baseline_offset = atlas.ascent;
    let default_bg = ThemeConfig::parse_color(&config.theme.background);

    let mut bg_rects = Vec::new();
    let mut glyph_instances = Vec::with_capacity(cols * total_rows / 2);

    for row in 0..total_rows {
        let py = row as f32 * ch;

        for col in 0..cols {
            let point = Point::new(Line(row as i32), Column(col));
            let cell = &grid[point];
            let px = col as f32 * cw;

            if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                continue;
            }

            let is_wide = cell.flags.contains(CellFlags::WIDE_CHAR);
            let bg_width = if is_wide { cw * 2.0 } else { cw };

            let mut fg = ansi_color_to_rgba(cell.fg, config);
            let mut bg = ansi_color_to_rgba(cell.bg, config);

            // BOLD: brighten fg
            if cell.flags.contains(CellFlags::BOLD) {
                fg[0] = (fg[0] * 1.3).min(1.0);
                fg[1] = (fg[1] * 1.3).min(1.0);
                fg[2] = (fg[2] * 1.3).min(1.0);
            }

            // DIM: dim fg
            if cell.flags.contains(CellFlags::DIM) {
                fg[0] *= 0.67;
                fg[1] *= 0.67;
                fg[2] *= 0.67;
            }

            // INVERSE: swap fg and bg
            if cell.flags.contains(CellFlags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }

            if bg != default_bg {
                bg_rects.push(Rect { x: px, y: py, w: bg_width, h: ch, color: bg });
            }

            // HIDDEN: background only, no text or decorations
            if cell.flags.contains(CellFlags::HIDDEN) {
                continue;
            }

            // UNDERLINE
            if cell.flags.contains(CellFlags::UNDERLINE) {
                let uy = py + baseline_offset + 1.0;
                bg_rects.push(Rect { x: px, y: uy, w: bg_width, h: 1.0, color: fg });
            }

            // STRIKETHROUGH
            if cell.flags.contains(CellFlags::STRIKEOUT) {
                let sy = py + ch * 0.5;
                bg_rects.push(Rect { x: px, y: sy, w: bg_width, h: 1.0, color: fg });
            }

            let c = cell.c;
            if c == ' ' || c == '\0' || c.is_control() {
                continue;
            }

            if let Some(entry) = atlas.ensure_char(c, font_system, queue) {
                if entry.width == 0 || entry.height == 0 {
                    continue;
                }

                glyph_instances.push(RelativeGlyph {
                    px: (px + entry.bearing_x as f32).round(),
                    py: (py + baseline_offset - entry.bearing_y as f32).round(),
                    glyph_w: entry.width as f32,
                    glyph_h: entry.height as f32,
                    u0: entry.u0,
                    v0: entry.v0,
                    u1: entry.u1,
                    v1: entry.v1,
                    color: fg,
                });
            }
        }
    }

    let cursor = content.cursor;
    let cursor_line = cursor.point.line.0;
    let cursor_color = ThemeConfig::parse_color(&config.terminal.cursor_color);
    let cursor_visible = cursor.shape != CursorShape::Hidden;
    let cursor_rects = if cursor_visible && cursor_line >= 0 && (cursor_line as usize) < total_rows {
        let cx = cursor.point.column.0 as f32 * cw;
        let cy = cursor_line as f32 * ch;
        let c = [cursor_color[0], cursor_color[1], cursor_color[2], config.terminal.cursor_opacity];
        build_cursor_rects(cursor.shape == CursorShape::HollowBlock, cx, cy, cw, ch, c)
    } else {
        Vec::new()
    };

    TerminalView { glyph_instances, bg_rects, cursor_rects }
}

/// Build cursor rects for all cursor shapes.
/// - Block: filled cell-sized rect
/// - HollowBlock: 4 border rects forming an outline
/// - Beam: thin vertical line on left edge (2px)
/// - Underline: thin horizontal line at bottom (2px)
fn build_cursor_rects_for_shape(shape: u8, cx: f32, cy: f32, cw: f32, ch: f32, color: [f32; 4]) -> Vec<Rect> {
    match shape {
        CURSOR_HOLLOW_BLOCK => {
            let t = 1.0_f32;
            vec![
                Rect { x: cx, y: cy, w: cw, h: t, color },           // top
                Rect { x: cx, y: cy + ch - t, w: cw, h: t, color },  // bottom
                Rect { x: cx, y: cy + t, w: t, h: ch - 2.0 * t, color }, // left
                Rect { x: cx + cw - t, y: cy + t, w: t, h: ch - 2.0 * t, color }, // right
            ]
        }
        CURSOR_BEAM => {
            // Thin vertical bar at left edge of cell
            vec![Rect { x: cx, y: cy, w: 2.0, h: ch, color }]
        }
        CURSOR_UNDERLINE => {
            // Thin horizontal bar at bottom of cell
            vec![Rect { x: cx, y: cy + ch - 2.0, w: cw, h: 2.0, color }]
        }
        _ => {
            // CURSOR_BLOCK and any unknown: filled rect
            vec![Rect { x: cx, y: cy, w: cw, h: ch, color }]
        }
    }
}

/// Legacy helper for build_terminal_view (direct Term access path).
fn build_cursor_rects(hollow: bool, cx: f32, cy: f32, cw: f32, ch: f32, color: [f32; 4]) -> Vec<Rect> {
    if hollow {
        build_cursor_rects_for_shape(CURSOR_HOLLOW_BLOCK, cx, cy, cw, ch, color)
    } else {
        build_cursor_rects_for_shape(0, cx, cy, cw, ch, color) // Block
    }
}

// ─── PackedColor → RGBA resolution (client-side theme mapping) ──────

use ciri_protocol::message::{
    PackedColor, PackedCell, COLOR_NAMED, COLOR_RGB, COLOR_INDEXED,
    FLAG_WIDE_CHAR, FLAG_WIDE_CHAR_SPACER, FLAG_HIDDEN,
    FLAG_BOLD, FLAG_DIM, FLAG_UNDERLINE, FLAG_INVERSE, FLAG_STRIKEOUT,
    CURSOR_HIDDEN, CURSOR_HOLLOW_BLOCK, CURSOR_BEAM, CURSOR_UNDERLINE,
};

fn packed_color_to_rgba(color: PackedColor, config: &CiriConfig) -> [f32; 4] {
    match color.tag {
        COLOR_NAMED => {
            let n = color.b1;
            let theme = &config.theme;
            match n {
                0 => named_color_to_rgba(NamedColor::Black, config),
                1 => named_color_to_rgba(NamedColor::Red, config),
                2 => named_color_to_rgba(NamedColor::Green, config),
                3 => named_color_to_rgba(NamedColor::Yellow, config),
                4 => named_color_to_rgba(NamedColor::Blue, config),
                5 => named_color_to_rgba(NamedColor::Magenta, config),
                6 => named_color_to_rgba(NamedColor::Cyan, config),
                7 => named_color_to_rgba(NamedColor::White, config),
                8 => named_color_to_rgba(NamedColor::BrightBlack, config),
                9 => named_color_to_rgba(NamedColor::BrightRed, config),
                10 => named_color_to_rgba(NamedColor::BrightGreen, config),
                11 => named_color_to_rgba(NamedColor::BrightYellow, config),
                12 => named_color_to_rgba(NamedColor::BrightBlue, config),
                13 => named_color_to_rgba(NamedColor::BrightMagenta, config),
                14 => named_color_to_rgba(NamedColor::BrightCyan, config),
                15 => named_color_to_rgba(NamedColor::BrightWhite, config),
                16 | 27 => ThemeConfig::parse_color(&theme.foreground),
                17 => ThemeConfig::parse_color(&theme.background),
                18 => ThemeConfig::parse_color(&theme.foreground),
                19..=26 => {
                    let base = packed_color_to_rgba(PackedColor::named(n - 19), config);
                    [base[0] * 0.67, base[1] * 0.67, base[2] * 0.67, base[3]]
                }
                28 => {
                    let fg = ThemeConfig::parse_color(&theme.foreground);
                    [fg[0] * 0.67, fg[1] * 0.67, fg[2] * 0.67, fg[3]]
                }
                _ => ThemeConfig::parse_color(&theme.foreground),
            }
        }
        COLOR_RGB => [color.b1 as f32 / 255.0, color.b2 as f32 / 255.0, color.b3 as f32 / 255.0, 1.0],
        COLOR_INDEXED => indexed_color_to_rgba(color.b1, config),
        _ => [1.0, 1.0, 1.0, 1.0],
    }
}

/// Build rendering data from a PackedCell grid (client-side, no alacritty dependency).
///
/// Handles full terminal semantics:
/// - INVERSE: swap fg/bg colors
/// - BOLD: brighten the foreground color
/// - DIM: dim the foreground color (×0.67)
/// - UNDERLINE: draw a 1px line at the cell baseline
/// - STRIKEOUT: draw a 1px line through the middle of the cell
/// - HIDDEN: skip text rendering (SGR 8)
/// - Cursor shapes: Block (filled), HollowBlock (outline), Beam (thin vertical), Underline (thin horizontal)
pub fn build_view_from_grid(
    cells: &[PackedCell],
    cols: u16,
    rows: u16,
    cursor_line: i16,
    cursor_col: u16,
    cursor_shape: u8,
    atlas: &mut GlyphAtlas,
    font_system: &mut FontSystem,
    queue: &wgpu::Queue,
    config: &CiriConfig,
) -> TerminalView {
    let cw = atlas.cell_width;
    let ch = atlas.cell_height;
    let baseline_offset = atlas.ascent;
    let default_bg = ThemeConfig::parse_color(&config.theme.background);

    let mut bg_rects = Vec::new();
    let mut glyph_instances = Vec::with_capacity(cols as usize * rows as usize / 2);

    for row in 0..rows as usize {
        let py = row as f32 * ch;
        for col in 0..cols as usize {
            let idx = row * cols as usize + col;
            if idx >= cells.len() { break; }
            let cell = &cells[idx];
            let px = col as f32 * cw;

            let f = cell.flags_u16();
            if f & FLAG_WIDE_CHAR_SPACER != 0 {
                continue;
            }

            let is_wide = f & FLAG_WIDE_CHAR != 0;
            let bg_width = if is_wide { cw * 2.0 } else { cw };

            // Resolve base colors
            let mut fg = packed_color_to_rgba(cell.fg, config);
            let mut bg = packed_color_to_rgba(cell.bg, config);

            // BOLD: brighten foreground (shift toward white by ~30%)
            if f & FLAG_BOLD != 0 {
                fg[0] = (fg[0] * 1.3).min(1.0);
                fg[1] = (fg[1] * 1.3).min(1.0);
                fg[2] = (fg[2] * 1.3).min(1.0);
            }

            // DIM: dim foreground (×0.67)
            if f & FLAG_DIM != 0 {
                fg[0] *= 0.67;
                fg[1] *= 0.67;
                fg[2] *= 0.67;
            }

            // INVERSE: swap fg and bg
            if f & FLAG_INVERSE != 0 {
                std::mem::swap(&mut fg, &mut bg);
            }

            // Background rect (after INVERSE so swapped bg is used)
            if bg != default_bg {
                bg_rects.push(Rect { x: px, y: py, w: bg_width, h: ch, color: bg });
            }

            // HIDDEN: render background but skip text/decorations
            if f & FLAG_HIDDEN != 0 {
                continue;
            }

            // UNDERLINE: 1px line at cell bottom (baseline + 1)
            if f & FLAG_UNDERLINE != 0 {
                let uy = py + baseline_offset + 1.0;
                bg_rects.push(Rect { x: px, y: uy, w: bg_width, h: 1.0, color: fg });
            }

            // STRIKEOUT: 1px line through middle of cell
            if f & FLAG_STRIKEOUT != 0 {
                let sy = py + ch * 0.5;
                bg_rects.push(Rect { x: px, y: sy, w: bg_width, h: 1.0, color: fg });
            }

            let c = cell.ch();
            if c == ' ' || c == '\0' || c.is_control() {
                continue;
            }

            if let Some(entry) = atlas.ensure_char(c, font_system, queue) {
                if entry.width == 0 || entry.height == 0 {
                    continue;
                }
                glyph_instances.push(RelativeGlyph {
                    px: (px + entry.bearing_x as f32).round(),
                    py: (py + baseline_offset - entry.bearing_y as f32).round(),
                    glyph_w: entry.width as f32,
                    glyph_h: entry.height as f32,
                    u0: entry.u0,
                    v0: entry.v0,
                    u1: entry.u1,
                    v1: entry.v1,
                    color: fg,
                });
            }
        }
    }

    let cursor_color = ThemeConfig::parse_color(&config.terminal.cursor_color);
    let cursor_visible = cursor_shape != CURSOR_HIDDEN;
    let cursor_rects = if cursor_visible && cursor_line >= 0 && (cursor_line as usize) < rows as usize {
        let cx = cursor_col as f32 * cw;
        let cy = cursor_line as f32 * ch;
        let c = [cursor_color[0], cursor_color[1], cursor_color[2], config.terminal.cursor_opacity];
        build_cursor_rects_for_shape(cursor_shape, cx, cy, cw, ch, c)
    } else {
        Vec::new()
    };

    TerminalView { glyph_instances, bg_rects, cursor_rects }
}

