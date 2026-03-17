use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape, NamedColor};
use ciri_config::config::CiriConfig;
use ciri_config::theme::ThemeConfig;
use glyphon::FontSystem;

use crate::glyph_cache::{GlyphAtlas, GlyphInstance};
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
    /// Cursor rect relative to (0, 0).
    pub cursor_rect: Option<Rect>,
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
    let baseline_offset = ch * 0.8;
    let default_bg = ThemeConfig::parse_color(&config.theme.background);

    let mut bg_rects = Vec::new();
    let mut glyph_instances = Vec::with_capacity(cols * total_rows / 2);

    for row in 0..total_rows {
        let py = row as f32 * ch;

        for col in 0..cols {
            let point = Point::new(Line(row as i32), Column(col));
            let cell = &grid[point];
            let px = col as f32 * cw;

            // Skip spacer cells (second cell of a wide char)
            if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                continue;
            }

            let is_wide = cell.flags.contains(CellFlags::WIDE_CHAR);
            let bg_width = if is_wide { cw * 2.0 } else { cw };

            let bg = ansi_color_to_rgba(cell.bg, config);
            if bg != default_bg {
                bg_rects.push(Rect { x: px, y: py, w: bg_width, h: ch, color: bg });
            }

            let c = cell.c;
            if c == ' ' || c == '\0' || c.is_control() {
                continue;
            }

            let fg = ansi_color_to_rgba(cell.fg, config);

            if let Some(entry) = atlas.ensure_char(c, font_system, queue) {
                if entry.width == 0 || entry.height == 0 {
                    continue;
                }

                glyph_instances.push(RelativeGlyph {
                    px: px + entry.bearing_x as f32,
                    py: py + baseline_offset - entry.bearing_y as f32,
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
    // Respect cursor visibility: programs hide it with \x1b[?25l during
    // multi-line redraws (e.g. cargo progress bars) to avoid flicker.
    let cursor_visible = cursor.shape != CursorShape::Hidden;
    let cursor_rect = if cursor_visible && cursor_line >= 0 && (cursor_line as usize) < total_rows {
        Some(Rect {
            x: cursor.point.column.0 as f32 * cw,
            y: cursor_line as f32 * ch,
            w: cw,
            h: ch,
            color: [cursor_color[0], cursor_color[1], cursor_color[2], config.terminal.cursor_opacity],
        })
    } else {
        None
    };

    TerminalView { glyph_instances, bg_rects, cursor_rect }
}

