//! Terminal cell → GPU rendering primitives.
//!
//! Two entry points serve different contexts:
//! - [`build_terminal_view`]: reads directly from alacritty's `Term` (server-side)
//! - [`build_view_from_grid`]: reads from [`PackedCell`] grid received over the wire (client-side)
//!
//! Both extract per-cell properties into [`CellProps`], then share a single rendering
//! path for backgrounds, text glyphs, decorations (underline/strikeout), and cursor.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape, NamedColor};
use ciri_config::config::CiriConfig;
use ciri_config::theme::ThemeConfig;
use unicode_segmentation::UnicodeSegmentation;

use crate::glyph_cache::{FontStyle, GlyphCache};
use crate::rect::Rect;
use crate::shaper::TextShaper;
use ciri_protocol::message::{
    COLOR_INDEXED, COLOR_NAMED, COLOR_RGB, CURSOR_BEAM, CURSOR_BLOCK, CURSOR_HIDDEN,
    CURSOR_HOLLOW_BLOCK, CURSOR_UNDERLINE, FLAG_BOLD, FLAG_DIM, FLAG_HIDDEN, FLAG_INVERSE,
    FLAG_ITALIC, FLAG_STRIKEOUT, FLAG_UNDERLINE, FLAG_UNDERLINE_CURLY, FLAG_UNDERLINE_DASHED,
    FLAG_UNDERLINE_DOTTED, FLAG_UNDERLINE_DOUBLE, FLAG_UNDERLINE_STYLE_MASK, FLAG_WIDE_CHAR,
    FLAG_WIDE_CHAR_SPACER, PackedCell, PackedColor,
};

// ─── Common types ────────────────────────────────────────────────────

/// Underline decoration style, abstracted from source-specific flag formats.
#[derive(Clone, Copy, PartialEq, Eq)]
enum UnderlineStyle {
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

/// Cell properties extracted from either alacritty `Term` or protocol `PackedCell`.
/// Colors are pre-processed: bold-brighten, dim, and inverse already applied.
struct CellProps {
    ch: char,
    fg: [f32; 4],
    bg: [f32; 4],
    style: FontStyle,
    is_wide: bool,
    is_hidden: bool,
    underline: UnderlineStyle,
    is_strikeout: bool,
}

/// Pre-computed cell layout values from the glyph atlas.
struct CellMetrics {
    cw: f32,       // cell width in pixels
    ch: f32,       // cell height in pixels
    baseline: f32, // font ascent (baseline offset from cell top)
    default_bg: [f32; 4],
}

impl CellMetrics {
    fn new(atlas: &GlyphCache, config: &CiriConfig) -> Self {
        CellMetrics {
            cw: atlas.cell_width,
            ch: atlas.cell_height,
            baseline: atlas.ascent,
            default_bg: ThemeConfig::parse_color(&config.theme.background),
        }
    }
}

/// Pre-computed color lookup table to avoid per-cell hex string parsing.
pub struct ColorTable {
    named: [[f32; 4]; 16],
    foreground: [f32; 4],
    background: [f32; 4],
    dim_foreground: [f32; 4],
    dim_colors: [[f32; 4]; 8],
}

impl ColorTable {
    pub fn new(config: &CiriConfig) -> Self {
        let theme = &config.theme;
        let fg = ThemeConfig::parse_color(&theme.foreground);
        let named = [
            ThemeConfig::parse_color(&theme.black),
            ThemeConfig::parse_color(&theme.red),
            ThemeConfig::parse_color(&theme.green),
            ThemeConfig::parse_color(&theme.yellow),
            ThemeConfig::parse_color(&theme.blue),
            ThemeConfig::parse_color(&theme.magenta),
            ThemeConfig::parse_color(&theme.cyan),
            ThemeConfig::parse_color(&theme.white),
            ThemeConfig::parse_color(&theme.bright_black),
            ThemeConfig::parse_color(&theme.bright_red),
            ThemeConfig::parse_color(&theme.bright_green),
            ThemeConfig::parse_color(&theme.bright_yellow),
            ThemeConfig::parse_color(&theme.bright_blue),
            ThemeConfig::parse_color(&theme.bright_magenta),
            ThemeConfig::parse_color(&theme.bright_cyan),
            ThemeConfig::parse_color(&theme.foreground), // bright_white = foreground
        ];
        let mut dim_colors = [[0.0f32; 4]; 8];
        for i in 0..8 {
            dim_colors[i] = [
                named[i][0] * 0.67,
                named[i][1] * 0.67,
                named[i][2] * 0.67,
                named[i][3],
            ];
        }
        ColorTable {
            named,
            foreground: fg,
            background: ThemeConfig::parse_color(&theme.background),
            dim_foreground: [fg[0] * 0.67, fg[1] * 0.67, fg[2] * 0.67, fg[3]],
            dim_colors,
        }
    }

    fn resolve_packed(&self, color: PackedColor) -> [f32; 4] {
        match color.tag {
            COLOR_NAMED => {
                let n = color.b1;
                match n {
                    0..=15 => self.named[n as usize],
                    16 | 27 => self.foreground,
                    17 => self.background,
                    18 => self.foreground,
                    19..=26 => self.dim_colors[(n - 19) as usize],
                    28 => self.dim_foreground,
                    _ => self.foreground,
                }
            }
            COLOR_RGB => [
                color.b1 as f32 / 255.0,
                color.b2 as f32 / 255.0,
                color.b3 as f32 / 255.0,
                1.0,
            ],
            COLOR_INDEXED => indexed_color_to_rgba_table(color.b1, self),
            _ => [1.0, 1.0, 1.0, 1.0],
        }
    }
}

/// Resolve 256-color index using pre-computed color table.
fn indexed_color_to_rgba_table(idx: u8, ct: &ColorTable) -> [f32; 4] {
    if idx < 16 {
        return ct.named[idx as usize];
    }
    if idx < 232 {
        let i = idx - 16;
        let r = (i / 36) % 6;
        let g = (i / 6) % 6;
        let b = i % 6;
        let to_f = |v: u8| {
            if v == 0 {
                0.0
            } else {
                (55.0 + 40.0 * v as f32) / 255.0
            }
        };
        return [to_f(r), to_f(g), to_f(b), 1.0];
    }
    let v = (8 + 10 * (idx - 232) as u32) as f32 / 255.0;
    [v, v, v, 1.0]
}

/// Per-row cached rendering data for incremental updates.
struct RowRenderData {
    glyphs: Vec<RelativeGlyph>,
    color_glyphs: Vec<RelativeGlyph>,
    bg_rects: Vec<Rect>,
}

/// Cached terminal view with positions RELATIVE to the tile's inner origin (0,0).
/// The actual screen offset is applied at render time, NOT baked into the cache.
pub struct TerminalView {
    /// Regular text glyph instances (alpha atlas).
    pub glyph_instances: Vec<RelativeGlyph>,
    /// Color emoji glyph instances (RGBA atlas).
    pub color_glyph_instances: Vec<RelativeGlyph>,
    /// Background rects with pixel positions relative to (0, 0).
    pub bg_rects: Vec<Rect>,
    /// Cursor rects relative to (0, 0).
    pub cursor_rects: Vec<Rect>,
    /// Scrollbar rect (if any), relative to the pane.
    pub scrollbar_rect: Option<Rect>,
    /// Cached scrollbar key: (scroll_offset, total_lines, rows, pane_w_bits, pane_h_bits).
    /// Avoids redundant scrollbar recomputation when parameters haven't changed.
    pub scrollbar_key: Option<(usize, usize, u16, u32, u32)>,
    /// Per-row cached rendering data for incremental rebuilds.
    row_data: Vec<RowRenderData>,
    /// Per-row cached shaping data for incremental rebuilds.
    row_lig_cache: Vec<RowLigatureData>,
    /// Monotonically increasing generation counter. Bumped on every build/update.
    pub generation: u64,
}

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

// ─── Cell property extraction ────────────────────────────────────────

impl CellProps {
    /// Extract from an alacritty terminal cell. Returns `None` for wide-char spacers.
    fn from_term_cell(
        cell: &alacritty_terminal::term::cell::Cell,
        config: &CiriConfig,
    ) -> Option<Self> {
        if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
            return None;
        }

        let is_bold = cell.flags.contains(CellFlags::BOLD);
        let is_italic = cell.flags.contains(CellFlags::ITALIC);
        let mut fg = ansi_color_to_rgba(cell.fg, config);
        let mut bg = ansi_color_to_rgba(cell.bg, config);
        apply_color_modifiers(
            &mut fg,
            &mut bg,
            is_bold,
            cell.flags.contains(CellFlags::DIM),
            cell.flags.contains(CellFlags::INVERSE),
        );

        Some(CellProps {
            ch: cell.c,
            fg,
            bg,
            style: FontStyle::from_bold_italic(is_bold, is_italic),
            is_wide: cell.flags.contains(CellFlags::WIDE_CHAR),
            is_hidden: cell.flags.contains(CellFlags::HIDDEN),
            underline: UnderlineStyle::from_term_flags(cell.flags),
            is_strikeout: cell.flags.contains(CellFlags::STRIKEOUT),
        })
    }

    /// Extract from a protocol `PackedCell` using pre-computed color table.
    fn from_packed_cell_fast(cell: &PackedCell, ct: &ColorTable) -> Option<Self> {
        let f = cell.flags_u16();
        if f & FLAG_WIDE_CHAR_SPACER != 0 {
            return None;
        }

        let is_bold = f & FLAG_BOLD != 0;
        let is_italic = f & FLAG_ITALIC != 0;
        let mut fg = ct.resolve_packed(cell.fg);
        let mut bg = ct.resolve_packed(cell.bg);
        apply_color_modifiers(
            &mut fg,
            &mut bg,
            is_bold,
            f & FLAG_DIM != 0,
            f & FLAG_INVERSE != 0,
        );

        let underline = if f & FLAG_UNDERLINE != 0 {
            UnderlineStyle::from_packed_flags(f & FLAG_UNDERLINE_STYLE_MASK)
        } else {
            UnderlineStyle::None
        };

        Some(CellProps {
            ch: cell.ch(),
            fg,
            bg,
            style: FontStyle::from_bold_italic(is_bold, is_italic),
            is_wide: f & FLAG_WIDE_CHAR != 0,
            is_hidden: f & FLAG_HIDDEN != 0,
            underline,
            is_strikeout: f & FLAG_STRIKEOUT != 0,
        })
    }
}

/// Apply bold-brighten, dim, and inverse color modifications in-place.
fn apply_color_modifiers(
    fg: &mut [f32; 4],
    bg: &mut [f32; 4],
    bold: bool,
    dim: bool,
    inverse: bool,
) {
    if bold {
        for c in &mut fg[..3] {
            *c = (*c * 1.3).min(1.0);
        }
    }
    if dim {
        for c in &mut fg[..3] {
            *c *= 0.67;
        }
    }
    if inverse {
        std::mem::swap(fg, bg);
    }
}

impl UnderlineStyle {
    /// Detect underline style from alacritty's `CellFlags`.
    fn from_term_flags(flags: CellFlags) -> Self {
        if !flags.contains(CellFlags::ALL_UNDERLINES) {
            Self::None
        } else if flags.contains(CellFlags::DOUBLE_UNDERLINE) {
            Self::Double
        } else if flags.contains(CellFlags::UNDERCURL) {
            Self::Curly
        } else if flags.contains(CellFlags::DOTTED_UNDERLINE) {
            Self::Dotted
        } else if flags.contains(CellFlags::DASHED_UNDERLINE) {
            Self::Dashed
        } else {
            Self::Single
        }
    }

    /// Decode underline style from packed protocol flags (masked bits).
    fn from_packed_flags(style_bits: u16) -> Self {
        match style_bits {
            FLAG_UNDERLINE_DOUBLE => Self::Double,
            FLAG_UNDERLINE_CURLY => Self::Curly,
            FLAG_UNDERLINE_DOTTED => Self::Dotted,
            FLAG_UNDERLINE_DASHED => Self::Dashed,
            _ => Self::Single,
        }
    }
}

// ─── Core rendering (shared by both paths) ───────────────────────────

/// Render a single cell: emit decorations and glyph (NOT background — handled by strip merger).
fn render_cell(
    row: usize,
    col: usize,
    cell: &CellProps,
    m: &CellMetrics,
    atlas: &mut GlyphCache,
    bg_rects: &mut Vec<Rect>,
    glyphs: &mut Vec<RelativeGlyph>,
    color_glyphs: &mut Vec<RelativeGlyph>,
) {
    let px = col as f32 * m.cw;
    let py = row as f32 * m.ch;
    let bg_width = if cell.is_wide { m.cw * 2.0 } else { m.cw };

    // Hidden cells: no text or decorations (background handled by strip merger)
    if cell.is_hidden {
        return;
    }

    // Underline decoration
    if cell.underline != UnderlineStyle::None {
        let uy = py + m.baseline + 1.0;
        emit_underline_rects(bg_rects, cell.underline, px, uy, bg_width, cell.fg, m.cw);
    }

    // Strikethrough: 1px line through vertical center
    if cell.is_strikeout {
        bg_rects.push(Rect {
            x: px,
            y: py + m.ch * 0.5,
            w: bg_width,
            h: 1.0,
            color: cell.fg,
        });
    }

    // Skip whitespace / control chars (no glyph to render)
    let c = cell.ch;
    if c == ' ' || c == '\0' || c.is_control() {
        return;
    }

    // Rasterize and cache the glyph, then emit a rendering instance
    if let Some(entry) = atlas.ensure_styled_char(c, cell.style) {
        if entry.width == 0 || entry.height == 0 {
            return;
        }
        let glyph = RelativeGlyph {
            px: (px + entry.bearing_x as f32).round(),
            py: (py + m.baseline - entry.bearing_y as f32).round(),
            glyph_w: entry.width as f32,
            glyph_h: entry.height as f32,
            u0: entry.u0,
            v0: entry.v0,
            u1: entry.u1,
            v1: entry.v1,
            color: cell.fg,
        };
        if entry.is_color {
            color_glyphs.push(glyph);
        } else {
            glyphs.push(glyph);
        }
    }
}

/// Emit underline decoration rects into `bg_rects`.
fn emit_underline_rects(
    rects: &mut Vec<Rect>,
    style: UnderlineStyle,
    px: f32,
    uy: f32,
    width: f32,
    color: [f32; 4],
    cell_width: f32,
) {
    match style {
        UnderlineStyle::None => {}
        UnderlineStyle::Single => {
            rects.push(Rect {
                x: px,
                y: uy,
                w: width,
                h: 1.0,
                color,
            });
        }
        UnderlineStyle::Double => {
            // Two 1px lines with 1px gap
            rects.push(Rect {
                x: px,
                y: uy,
                w: width,
                h: 1.0,
                color,
            });
            rects.push(Rect {
                x: px,
                y: uy + 2.0,
                w: width,
                h: 1.0,
                color,
            });
        }
        UnderlineStyle::Curly => {
            // Approximate sine wave with 2px-wide rect segments
            let wave_len = cell_width.max(8.0);
            let segments = (width / 2.0).ceil() as usize;
            for i in 0..segments {
                let x = px + i as f32 * 2.0;
                let y_off = (i as f32 / wave_len * std::f32::consts::TAU).sin() * 1.5;
                let w = 2.0_f32.min(width - i as f32 * 2.0);
                if w > 0.0 {
                    rects.push(Rect {
                        x,
                        y: uy + y_off,
                        w,
                        h: 1.0,
                        color,
                    });
                }
            }
        }
        UnderlineStyle::Dotted => {
            emit_dashed_line(rects, px, uy, width, color, 2.0, 2.0);
        }
        UnderlineStyle::Dashed => {
            emit_dashed_line(rects, px, uy, width, color, 4.0, 2.0);
        }
    }
}

/// Emit a dashed/dotted horizontal line as a series of small rects.
fn emit_dashed_line(
    rects: &mut Vec<Rect>,
    px: f32,
    y: f32,
    total_width: f32,
    color: [f32; 4],
    dash_len: f32,
    gap_len: f32,
) {
    let end = px + total_width;
    let mut x = px;
    while x < end {
        let w = dash_len.min(end - x);
        rects.push(Rect {
            x,
            y,
            w,
            h: 1.0,
            color,
        });
        x += dash_len + gap_len;
    }
}

/// Build cursor rects for the given cursor shape.
fn build_cursor_rects(shape: u8, cx: f32, cy: f32, cw: f32, ch: f32, color: [f32; 4]) -> Vec<Rect> {
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
fn cursor_shape_to_protocol(shape: CursorShape) -> u8 {
    match shape {
        CursorShape::Block => CURSOR_BLOCK,
        CursorShape::Underline => CURSOR_UNDERLINE,
        CursorShape::Beam => CURSOR_BEAM,
        CursorShape::Hidden => CURSOR_HIDDEN,
        CursorShape::HollowBlock => CURSOR_HOLLOW_BLOCK,
    }
}

/// Compute cursor rects from shape, position, and config.
fn make_cursor_rects(
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

// ─── Public API ──────────────────────────────────────────────────────

/// Build rendering data from an alacritty `Term` (server-side path).
pub fn build_terminal_view<T: alacritty_terminal::event::EventListener>(
    term: &Term<T>,
    atlas: &mut GlyphCache,
    config: &CiriConfig,
) -> TerminalView {
    let m = CellMetrics::new(atlas, config);
    let grid = term.grid();
    let cols = grid.columns();
    let total_rows = grid.screen_lines();

    let mut bg_rects = Vec::new();
    let mut glyphs = Vec::with_capacity(cols * total_rows / 2);
    let mut color_glyphs = Vec::new();

    for row in 0..total_rows {
        let mut strip_color: Option<[f32; 4]> = None;
        let mut strip_start: usize = 0;

        for col in 0..cols {
            let cell = &grid[Point::new(Line(row as i32), Column(col))];
            if let Some(props) = CellProps::from_term_cell(cell, config) {
                // Merge adjacent same-color bg cells into strips
                if props.bg != m.default_bg {
                    if let Some(sc) = strip_color {
                        if sc != props.bg {
                            flush_bg_strip(&mut bg_rects, sc, strip_start, col, row, &m);
                            strip_color = Some(props.bg);
                            strip_start = col;
                        }
                    } else {
                        strip_color = Some(props.bg);
                        strip_start = col;
                    }
                } else if let Some(sc) = strip_color.take() {
                    flush_bg_strip(&mut bg_rects, sc, strip_start, col, row, &m);
                }

                render_cell(
                    row,
                    col,
                    &props,
                    &m,
                    atlas,
                    &mut bg_rects,
                    &mut glyphs,
                    &mut color_glyphs,
                );
            }
        }
        if let Some(sc) = strip_color {
            flush_bg_strip(&mut bg_rects, sc, strip_start, cols, row, &m);
        }
    }

    // Cursor (read from renderable_content which has the resolved cursor state)
    let cursor = term.renderable_content().cursor;
    let cursor_rects = make_cursor_rects(
        cursor_shape_to_protocol(cursor.shape),
        cursor.point.line.0,
        cursor.point.column.0,
        total_rows,
        &m,
        config,
    );

    TerminalView {
        glyph_instances: glyphs,
        color_glyph_instances: color_glyphs,
        bg_rects,
        cursor_rects,
        scrollbar_rect: None,
        scrollbar_key: None,
        row_data: Vec::new(),
        row_lig_cache: Vec::new(),
        generation: 0,
    }
}

/// Pre-computed ligature info for a single row.
struct RowLigatureData {
    /// True for columns that are continuations of a ligature (should skip normal rendering).
    skip_cols: Vec<bool>,
    /// Ligature glyphs to render: (col, glyph_id, font_id, style, fg_color).
    ligature_glyphs: Vec<(usize, u32, fontdb::ID, FontStyle, [f32; 4])>,
    /// Pre-shaped grapheme clusters: (col, glyph_id).
    grapheme_glyphs: Vec<(usize, u32)>,
}

/// Render a single row of cells into per-row buffers.
fn render_single_row(
    cells: &[PackedCell],
    row: usize,
    cols: u16,
    lig: Option<&RowLigatureData>,
    primary_font_id: Option<fontdb::ID>,
    m: &CellMetrics,
    ct: &ColorTable,
    atlas: &mut GlyphCache,
) -> RowRenderData {
    let mut glyphs = Vec::new();
    let mut color_glyphs = Vec::new();
    let mut bg_rects = Vec::new();

    let mut strip_color: Option<[f32; 4]> = None;
    let mut strip_start: usize = 0;

    for col in 0..cols as usize {
        let idx = row * cols as usize + col;
        if idx >= cells.len() {
            break;
        }

        let Some(props) = CellProps::from_packed_cell_fast(&cells[idx], ct) else {
            continue;
        };

        if props.bg != m.default_bg {
            if let Some(sc) = strip_color {
                if sc != props.bg {
                    flush_bg_strip(&mut bg_rects, sc, strip_start, col, row, m);
                    strip_color = Some(props.bg);
                    strip_start = col;
                }
            } else {
                strip_color = Some(props.bg);
                strip_start = col;
            }
        } else if let Some(sc) = strip_color.take() {
            flush_bg_strip(&mut bg_rects, sc, strip_start, col, row, m);
        }

        render_cell_decorations(row, col, &props, m, &mut bg_rects);

        if props.is_hidden || props.ch == ' ' || props.ch == '\0' || props.ch.is_control() {
            continue;
        }

        if let Some(ld) = lig {
            if col < ld.skip_cols.len() && ld.skip_cols[col] {
                continue;
            }
        }

        if let Some(ld) = lig {
            if let Ok(gi) = ld.grapheme_glyphs.binary_search_by_key(&col, |(c, _)| *c) {
                let gid = ld.grapheme_glyphs[gi].1;
                if let Some(fid) = primary_font_id {
                    if let Some(entry) =
                        atlas.ensure_glyph_id(gid, fid, props.style)
                    {
                        if entry.width > 0 && entry.height > 0 {
                            let px = col as f32 * m.cw;
                            let py = row as f32 * m.ch;
                            let g = RelativeGlyph {
                                px: (px + entry.bearing_x as f32).round(),
                                py: (py + m.baseline - entry.bearing_y as f32).round(),
                                glyph_w: entry.width as f32,
                                glyph_h: entry.height as f32,
                                u0: entry.u0,
                                v0: entry.v0,
                                u1: entry.u1,
                                v1: entry.v1,
                                color: props.fg,
                            };
                            if entry.is_color {
                                color_glyphs.push(g);
                            } else {
                                glyphs.push(g);
                            }
                        }
                    }
                }
                continue;
            }
        }

        emit_glyph(
            col,
            row,
            &props,
            m,
            atlas,
            &mut glyphs,
            &mut color_glyphs,
        );
    }
    if let Some(sc) = strip_color {
        flush_bg_strip(&mut bg_rects, sc, strip_start, cols as usize, row, m);
    }

    if let Some(ld) = lig {
        for &(col, glyph_id, font_id, style, fg) in &ld.ligature_glyphs {
            if let Some(entry) = atlas.ensure_glyph_id(glyph_id, font_id, style) {
                if entry.width == 0 || entry.height == 0 {
                    continue;
                }
                let px = col as f32 * m.cw;
                let py = row as f32 * m.ch;
                let g = RelativeGlyph {
                    px: (px + entry.bearing_x as f32).round(),
                    py: (py + m.baseline - entry.bearing_y as f32).round(),
                    glyph_w: entry.width as f32,
                    glyph_h: entry.height as f32,
                    u0: entry.u0,
                    v0: entry.v0,
                    u1: entry.u1,
                    v1: entry.v1,
                    color: fg,
                };
                if entry.is_color {
                    color_glyphs.push(g);
                } else {
                    glyphs.push(g);
                }
            }
        }
    }

    RowRenderData {
        glyphs,
        color_glyphs,
        bg_rects,
    }
}

/// Flatten per-row cached data into the flat TerminalView vecs.
fn flatten_view(view: &mut TerminalView) {
    view.glyph_instances.clear();
    view.color_glyph_instances.clear();
    view.bg_rects.clear();
    for rd in &view.row_data {
        view.glyph_instances.extend_from_slice(&rd.glyphs);
        view.color_glyph_instances
            .extend_from_slice(&rd.color_glyphs);
        view.bg_rects.extend_from_slice(&rd.bg_rects);
    }
}

/// Build rendering data from a `PackedCell` grid (client-side path, full rebuild).
///
/// The `shaper` is passed separately from `atlas` to allow simultaneous
/// immutable shaper access (for Face creation) and mutable atlas access
/// (for glyph caching).
pub fn build_view_from_grid(
    cells: &[PackedCell],
    cols: u16,
    rows: u16,
    cursor_line: i16,
    cursor_col: u16,
    cursor_shape: u8,
    atlas: &mut GlyphCache,
    shaper: &TextShaper,
    config: &CiriConfig,
    ct: &ColorTable,
    grapheme_map: &std::collections::HashMap<u32, String>,
) -> TerminalView {
    let m = CellMetrics::new(atlas, config);
    let primary_font_id = shaper.primary_font_id();

    // ─── Phase 1: Pre-compute all shaping data (immutable shaper access) ───
    let row_lig_data: Vec<RowLigatureData> = if let Some(fid) = primary_font_id {
        if let Some(face) = shaper.create_face(fid) {
            (0..rows as usize)
                .map(|row| precompute_row_shaping(cells, row, cols, ct, shaper, fid, &face, grapheme_map))
                .collect()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    // ─── Phase 2: Render all rows into per-row buffers ───
    let mut row_data = Vec::with_capacity(rows as usize);
    for row in 0..rows as usize {
        let lig = row_lig_data.get(row);
        row_data.push(render_single_row(
            cells,
            row,
            cols,
            lig,
            primary_font_id,
            &m,
            ct,
            atlas,
        ));
    }

    // ─── Phase 3: Flatten into contiguous vecs ───
    let cursor_rects = make_cursor_rects(
        cursor_shape,
        cursor_line as i32,
        cursor_col as usize,
        rows as usize,
        &m,
        config,
    );

    let mut view = TerminalView {
        glyph_instances: Vec::new(),
        color_glyph_instances: Vec::new(),
        bg_rects: Vec::new(),
        cursor_rects,
        scrollbar_rect: None,
        scrollbar_key: None,
        row_data,
        row_lig_cache: row_lig_data,
        generation: 1,
    };
    flatten_view(&mut view);
    view
}

/// Incrementally update a TerminalView for only the dirty rows.
/// Much cheaper than a full rebuild: typically 1-3 rows vs 67 rows at 4K.
pub fn update_view_from_grid(
    view: &mut TerminalView,
    dirty_rows: &[bool],
    cells: &[PackedCell],
    cols: u16,
    rows: u16,
    cursor_line: i16,
    cursor_col: u16,
    cursor_shape: u8,
    atlas: &mut GlyphCache,
    shaper: &TextShaper,
    config: &CiriConfig,
    ct: &ColorTable,
    grapheme_map: &std::collections::HashMap<u32, String>,
) {
    let m = CellMetrics::new(atlas, config);
    let primary_font_id = shaper.primary_font_id();
    let nrows = rows as usize;

    // Re-shape only dirty rows (immutable shaper access)
    if let Some(fid) = primary_font_id {
        if let Some(face) = shaper.create_face(fid) {
            for (row, &dirty) in dirty_rows.iter().enumerate().take(nrows) {
                if dirty && row < view.row_lig_cache.len() {
                    view.row_lig_cache[row] =
                        precompute_row_shaping(cells, row, cols, ct, shaper, fid, &face, grapheme_map);
                }
            }
        }
    }

    // Re-render only dirty rows (mutable atlas access)
    for (row, &dirty) in dirty_rows.iter().enumerate().take(nrows) {
        if dirty && row < view.row_data.len() {
            let lig = view.row_lig_cache.get(row);
            view.row_data[row] = render_single_row(
                cells,
                row,
                cols,
                lig,
                primary_font_id,
                &m,
                ct,
                atlas,
            );
        }
    }

    // Rebuild cursor
    view.cursor_rects = make_cursor_rects(
        cursor_shape,
        cursor_line as i32,
        cursor_col as usize,
        nrows,
        &m,
        config,
    );

    // Bump generation and re-flatten
    view.generation = view.generation.wrapping_add(1);
    flatten_view(view);
}

// ─── Text shaping integration ───────────────────────────────────────

/// Pre-compute all ligature/grapheme shaping data for a single row.
/// Uses a pre-created Face to avoid per-row Face::from_slice overhead.
fn precompute_row_shaping(
    cells: &[PackedCell],
    row: usize,
    cols: u16,
    ct: &ColorTable,
    shaper: &TextShaper,
    fid: fontdb::ID,
    face: &rustybuzz::Face,
    grapheme_map: &std::collections::HashMap<u32, String>,
) -> RowLigatureData {
    let cols_usize = cols as usize;
    let mut skip_cols = vec![false; cols_usize];
    let mut ligature_glyphs = Vec::new();
    let mut grapheme_glyphs = Vec::new();

    // ── Detect ligatures via text shaping ──
    let mut run_start = None;
    let mut run_text = String::new();
    let mut run_style = FontStyle::Regular;
    let mut run_fg = [1.0f32; 4];

    for col in 0..=cols_usize {
        let cell_info = if col < cols_usize {
            let idx = row * cols_usize + col;
            if idx < cells.len() {
                CellProps::from_packed_cell_fast(&cells[idx], ct)
                    .filter(|p| !p.is_hidden && p.ch != ' ' && p.ch != '\0' && !p.ch.is_control())
            } else {
                None
            }
        } else {
            None
        };

        if let Some(props) = &cell_info {
            if run_start.is_some() && props.style == run_style {
                run_text.push(props.ch);
                continue;
            }
            // Flush previous run
            if run_start.is_some() && run_text.len() >= 2 {
                let start = run_start.unwrap();
                for lig in shaper.detect_ligatures_with_face(&run_text, face, fid) {
                    for k in 1..lig.char_count {
                        let c = start + lig.start_col + k;
                        if c < cols_usize {
                            skip_cols[c] = true;
                        }
                    }
                    ligature_glyphs.push((
                        start + lig.start_col,
                        lig.glyph_id,
                        lig.font_id,
                        run_style,
                        run_fg,
                    ));
                }
            }
            run_start = Some(col);
            run_text.clear();
            run_text.push(props.ch);
            run_style = props.style;
            run_fg = props.fg;
        } else {
            if run_start.is_some() && run_text.len() >= 2 {
                let start = run_start.unwrap();
                for lig in shaper.detect_ligatures_with_face(&run_text, face, fid) {
                    for k in 1..lig.char_count {
                        let c = start + lig.start_col + k;
                        if c < cols_usize {
                            skip_cols[c] = true;
                        }
                    }
                    ligature_glyphs.push((
                        start + lig.start_col,
                        lig.glyph_id,
                        lig.font_id,
                        run_style,
                        run_fg,
                    ));
                }
            }
            run_start = None;
            run_text.clear();
        }
    }

    // ── Detect grapheme clusters ──
    for col in 0..cols_usize {
        let idx = row * cols_usize + col;
        if idx >= cells.len() {
            break;
        }
        let Some(props) = CellProps::from_packed_cell_fast(&cells[idx], ct) else {
            continue;
        };
        if props.is_hidden || props.ch == ' ' || props.ch == '\0' || props.ch.is_control() {
            continue;
        }
        if skip_cols[col] {
            continue;
        }

        // Check if the grapheme extras map has multi-codepoint data for this cell
        // (e.g. flag emoji with zerowidth combiners sent by the server)
        let cluster_str = if let Some(full_grapheme) = grapheme_map.get(&(idx as u32)) {
            full_grapheme.clone()
        } else {
            // Look ahead for combining/modifier characters in adjacent cells
            let mut s = String::from(props.ch);
            let mut look = col + if props.is_wide { 2 } else { 1 };
            while look < cols_usize {
                let li = row * cols_usize + look;
                if li >= cells.len() {
                    break;
                }
                let next_ch = cells[li].ch();
                if is_combining_or_modifier(next_ch) {
                    s.push(next_ch);
                    look += 1;
                } else {
                    break;
                }
            }
            s
        };

        if cluster_str.graphemes(true).count() == 1 && cluster_str.chars().count() > 1 {
            if let Some(gid) = shaper.shape_grapheme_with_face(&cluster_str, face) {
                grapheme_glyphs.push((col, gid));
            }
        }
    }

    RowLigatureData {
        skip_cols,
        ligature_glyphs,
        grapheme_glyphs,
    }
}

/// Render decorations (underline, strikeout) for a cell. Background is handled by strip merger.
fn render_cell_decorations(
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
            py + m.baseline + 1.0,
            bg_width,
            cell.fg,
            m.cw,
        );
    }
    if cell.is_strikeout {
        bg_rects.push(Rect {
            x: px,
            y: py + m.ch * 0.5,
            w: bg_width,
            h: 1.0,
            color: cell.fg,
        });
    }
}

/// Flush a pending background strip as a single rect.
/// Merges adjacent cells with the same background color into one rect per run,
/// reducing rect count from `cols` to typically 1–5 per row.
#[inline]
fn flush_bg_strip(
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

/// Emit a single glyph for a character at (col, row).
fn emit_glyph(
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
        let g = RelativeGlyph {
            px: (px + entry.bearing_x as f32).round(),
            py: (py + m.baseline - entry.bearing_y as f32).round(),
            glyph_w: entry.width as f32,
            glyph_h: entry.height as f32,
            u0: entry.u0,
            v0: entry.v0,
            u1: entry.u1,
            v1: entry.v1,
            color: cell.fg,
        };
        if entry.is_color {
            color_glyphs.push(g);
        } else {
            glyphs.push(g);
        }
    }
}

/// Check if a character is a Unicode combining character, ZWJ, or variation selector.
fn is_combining_or_modifier(c: char) -> bool {
    matches!(c,
        '\u{200D}'          // Zero Width Joiner
        | '\u{FE0E}'..='\u{FE0F}'  // Variation Selectors
        | '\u{0300}'..='\u{036F}'   // Combining Diacritical Marks
        | '\u{20D0}'..='\u{20FF}'   // Combining Marks for Symbols
        | '\u{1AB0}'..='\u{1AFF}'   // Combining Diacritical Marks Extended
        | '\u{1DC0}'..='\u{1DFF}'   // Combining Diacritical Marks Supplement
        | '\u{FE20}'..='\u{FE2F}'   // Combining Half Marks
        | '\u{E0100}'..='\u{E01EF}' // Variation Selectors Supplement
        | '\u{1F3FB}'..='\u{1F3FF}' // Emoji skin tone modifiers
        | '\u{1F1E0}'..='\u{1F1FF}' // Regional Indicator Symbols
    )
}

/// Visual scrollbar width in pixels.
pub const SCROLLBAR_WIDTH: f32 = 4.0;
/// Margin between scrollbar and pane edge in pixels.
pub const SCROLLBAR_MARGIN: f32 = 2.0;

/// Build a scrollbar rect for a pane with scrollback.
/// Returns `None` if scrollback is empty (nothing to scroll).
pub fn build_scrollbar(
    scroll_offset: usize,
    total_lines: usize,
    visible_rows: u16,
    pane_width: f32,
    pane_height: f32,
    config: &CiriConfig,
) -> Option<Rect> {
    let visible = visible_rows as usize;
    if total_lines <= visible {
        return None;
    }

    let scrollbar_width = SCROLLBAR_WIDTH;
    let scrollbar_margin = SCROLLBAR_MARGIN;

    // Thumb size proportional to visible/total ratio
    let ratio = visible as f32 / total_lines as f32;
    let thumb_height = (ratio * pane_height).max(10.0);

    // Thumb position: offset=0 → bottom, max → top
    let max_offset = total_lines - visible;
    let position_ratio = if max_offset > 0 {
        1.0 - (scroll_offset as f32 / max_offset as f32)
    } else {
        1.0
    };

    let scrollbar_color = ThemeConfig::parse_color(&config.theme.bright_black);
    Some(Rect {
        x: pane_width - scrollbar_width - scrollbar_margin,
        y: position_ratio * (pane_height - thumb_height),
        w: scrollbar_width,
        h: thumb_height,
        color: [
            scrollbar_color[0],
            scrollbar_color[1],
            scrollbar_color[2],
            0.4,
        ],
    })
}

// ─── Color resolution ────────────────────────────────────────────────

/// Resolve alacritty `AnsiColor` to RGBA.
fn ansi_color_to_rgba(color: AnsiColor, config: &CiriConfig) -> [f32; 4] {
    match color {
        AnsiColor::Named(named) => named_color_to_rgba(named, config),
        AnsiColor::Spec(rgb) => [
            rgb.r as f32 / 255.0,
            rgb.g as f32 / 255.0,
            rgb.b as f32 / 255.0,
            1.0,
        ],
        AnsiColor::Indexed(idx) => indexed_color_to_rgba(idx, config),
    }
}

/// Map named color index (0–15) to alacritty `NamedColor`.
fn named_color_from_index(idx: u8) -> NamedColor {
    match idx {
        0 => NamedColor::Black,
        1 => NamedColor::Red,
        2 => NamedColor::Green,
        3 => NamedColor::Yellow,
        4 => NamedColor::Blue,
        5 => NamedColor::Magenta,
        6 => NamedColor::Cyan,
        7 => NamedColor::White,
        8 => NamedColor::BrightBlack,
        9 => NamedColor::BrightRed,
        10 => NamedColor::BrightGreen,
        11 => NamedColor::BrightYellow,
        12 => NamedColor::BrightBlue,
        13 => NamedColor::BrightMagenta,
        14 => NamedColor::BrightCyan,
        15 => NamedColor::BrightWhite,
        _ => NamedColor::Foreground,
    }
}

/// Resolve alacritty `NamedColor` to RGBA using the theme config.
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
        NamedColor::BrightWhite | NamedColor::Foreground => {
            ThemeConfig::parse_color(&theme.foreground)
        }
        NamedColor::Background => ThemeConfig::parse_color(&theme.background),
        _ => ThemeConfig::parse_color(&theme.foreground),
    }
}

/// Resolve 256-color index to RGBA.
/// 0–15: named colors, 16–231: 6×6×6 RGB cube, 232–255: grayscale ramp.
fn indexed_color_to_rgba(idx: u8, config: &CiriConfig) -> [f32; 4] {
    if idx < 16 {
        return named_color_to_rgba(named_color_from_index(idx), config);
    }
    if idx < 232 {
        // 6×6×6 color cube: index = 16 + 36*r + 6*g + b
        let i = idx - 16;
        let r = (i / 36) % 6;
        let g = (i / 6) % 6;
        let b = i % 6;
        let to_f = |v: u8| {
            if v == 0 {
                0.0
            } else {
                (55.0 + 40.0 * v as f32) / 255.0
            }
        };
        return [to_f(r), to_f(g), to_f(b), 1.0];
    }
    // Grayscale ramp: 232–255 → 8, 18, 28, ..., 238
    let v = (8 + 10 * (idx - 232) as u32) as f32 / 255.0;
    [v, v, v, 1.0]
}
