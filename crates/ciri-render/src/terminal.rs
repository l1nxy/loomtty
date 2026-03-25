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

use crate::glyph_cache::{FontStyle, GlyphCache, GlyphEntry};
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

struct CellRenderer<'a> {
    row: usize,
    metrics: &'a CellMetrics,
    atlas: &'a mut GlyphCache,
    bg_rects: &'a mut Vec<Rect>,
    glyphs: &'a mut Vec<RelativeGlyph>,
    color_glyphs: &'a mut Vec<RelativeGlyph>,
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
    pub scrollbar_key: Option<(usize, usize, u16, u32, u32, u8)>,
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
fn render_cell(col: usize, cell: &CellProps, renderer: &mut CellRenderer<'_>) {
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
        let uy = py + renderer.metrics.baseline + 1.0;
        emit_underline_rects(
            renderer.bg_rects,
            cell.underline,
            px,
            uy,
            bg_width,
            cell.fg,
            renderer.metrics.cw,
        );
    }

    // Strikethrough: 1px line through vertical center
    if cell.is_strikeout {
        renderer.bg_rects.push(Rect {
            x: px,
            y: py + renderer.metrics.ch * 0.5,
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
    if let Some(entry) = renderer.atlas.ensure_styled_char(c, cell.style) {
        if entry.width == 0 || entry.height == 0 {
            return;
        }
        let glyph = if cell.is_wide && entry.is_color {
            constrain_wide_glyph(&entry, px, py, renderer.metrics, cell.fg)
        } else {
            make_relative_glyph(&entry, px, py, renderer.metrics.baseline, cell.fg)
        };
        if entry.is_color {
            renderer.color_glyphs.push(glyph);
        } else {
            renderer.glyphs.push(glyph);
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

                let mut renderer = CellRenderer {
                    row,
                    metrics: &m,
                    atlas,
                    bg_rects: &mut bg_rects,
                    glyphs: &mut glyphs,
                    color_glyphs: &mut color_glyphs,
                };
                render_cell(col, &props, &mut renderer);
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
#[derive(Clone)]
struct RowLigatureData {
    /// True for columns that are continuations of a ligature (should skip normal rendering).
    skip_cols: Vec<bool>,
    /// Ligature glyphs to render: (col, glyph_id, font_id, style, fg_color).
    ligature_glyphs: Vec<(usize, u32, fontdb::ID, FontStyle, [f32; 4])>,
    /// Pre-shaped grapheme clusters: (col, glyph_id, font_id).
    grapheme_glyphs: Vec<(usize, u32, fontdb::ID)>,
    /// Per-char shaped glyph IDs for all-through-shaping path: (col, glyph_id, font_id, is_wide).
    char_glyphs: Vec<(usize, u32, fontdb::ID, bool)>,
}

/// Render a single row of cells into per-row buffers.
fn render_single_row(
    grid: PackedGridContext<'_>,
    row: usize,
    lig: Option<&RowLigatureData>,
    atlas: &mut GlyphCache,
) -> RowRenderData {
    let mut glyphs = Vec::new();
    let mut color_glyphs = Vec::new();
    let mut bg_rects = Vec::new();

    let mut strip_color: Option<[f32; 4]> = None;
    let mut strip_start: usize = 0;

    for col in 0..grid.cols as usize {
        let idx = row * grid.cols as usize + col;
        if idx >= grid.cells.len() {
            break;
        }

        let Some(props) = CellProps::from_packed_cell_fast(&grid.cells[idx], grid.colors) else {
            continue;
        };

        if props.bg != grid.metrics.default_bg {
            if let Some(sc) = strip_color {
                if sc != props.bg {
                    flush_bg_strip(&mut bg_rects, sc, strip_start, col, row, grid.metrics);
                    strip_color = Some(props.bg);
                    strip_start = col;
                }
            } else {
                strip_color = Some(props.bg);
                strip_start = col;
            }
        } else if let Some(sc) = strip_color.take() {
            flush_bg_strip(&mut bg_rects, sc, strip_start, col, row, grid.metrics);
        }

        render_cell_decorations(row, col, &props, grid.metrics, &mut bg_rects);

        if props.is_hidden || props.ch == ' ' || props.ch == '\0' || props.ch.is_control() {
            continue;
        }

        if let Some(ld) = lig
            && col < ld.skip_cols.len()
            && ld.skip_cols[col]
        {
            continue;
        }

        if let Some(ld) = lig
            && let Ok(gi) = ld
                .grapheme_glyphs
                .binary_search_by_key(&col, |(c, _, _)| *c)
        {
            let gid = ld.grapheme_glyphs[gi].1;
            let glyph_font_id = ld.grapheme_glyphs[gi].2;
            if let Some(entry) =
                atlas.ensure_glyph_id(gid, glyph_font_id, props.style, props.is_wide)
                && entry.width > 0
                && entry.height > 0
            {
                let px = col as f32 * grid.metrics.cw;
                let py = row as f32 * grid.metrics.ch;
                let is_cjk_text_wide =
                    props.is_wide && !entry.is_color && Some(glyph_font_id) == grid.cjk_font_id;
                let g = if props.is_wide && entry.is_color {
                    constrain_wide_glyph(&entry, px, py, grid.metrics, props.fg)
                } else if is_cjk_text_wide {
                    constrain_wide_text_glyph(&entry, px, py, grid.metrics, props.fg)
                } else {
                    make_relative_glyph(&entry, px, py, grid.metrics.baseline, props.fg)
                };
                if entry.is_color {
                    color_glyphs.push(g);
                } else {
                    glyphs.push(g);
                }
                continue;
            }
            // Rasterization failed — fall through to emit_glyph
            // so the base character is still visible.
        }

        // Try single-char shaping path (glyph-ID based, all-through-shaping)
        if let Some(ld) = lig
            && let Ok(ci) = ld.char_glyphs.binary_search_by_key(&col, |(c, _, _, _)| *c)
        {
            let (_, gid, font_id, is_wide) = ld.char_glyphs[ci];
            if let Some(entry) = atlas.ensure_glyph_id(gid, font_id, props.style, is_wide)
                && entry.width > 0
                && entry.height > 0
            {
                let px = col as f32 * grid.metrics.cw;
                let py = row as f32 * grid.metrics.ch;
                let is_cjk_text_wide =
                    is_wide && !entry.is_color && Some(font_id) == grid.cjk_font_id;
                let g = if is_wide && entry.is_color {
                    constrain_wide_glyph(&entry, px, py, grid.metrics, props.fg)
                } else if is_cjk_text_wide {
                    constrain_wide_text_glyph(&entry, px, py, grid.metrics, props.fg)
                } else {
                    make_relative_glyph(&entry, px, py, grid.metrics.baseline, props.fg)
                };
                if entry.is_color {
                    color_glyphs.push(g);
                } else {
                    glyphs.push(g);
                }
                continue;
            }
        }

        // Fallback: crossfont character-based path
        emit_glyph(
            col,
            row,
            &props,
            grid.metrics,
            atlas,
            &mut glyphs,
            &mut color_glyphs,
        );
    }
    if let Some(sc) = strip_color {
        flush_bg_strip(
            &mut bg_rects,
            sc,
            strip_start,
            grid.cols as usize,
            row,
            grid.metrics,
        );
    }

    if let Some(ld) = lig {
        for &(col, glyph_id, font_id, style, fg) in &ld.ligature_glyphs {
            if let Some(entry) = atlas.ensure_glyph_id(glyph_id, font_id, style, false) {
                if entry.width == 0 || entry.height == 0 {
                    continue;
                }
                let px = col as f32 * grid.metrics.cw;
                let py = row as f32 * grid.metrics.ch;
                let g = make_relative_glyph(&entry, px, py, grid.metrics.baseline, fg);
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

#[derive(Clone, Copy)]
struct PackedGridContext<'a> {
    cells: &'a [PackedCell],
    cols: u16,
    rows: u16,
    cjk_font_id: Option<fontdb::ID>,
    metrics: &'a CellMetrics,
    colors: &'a ColorTable,
}

struct ViewBuildParams<'a> {
    grid: PackedGridContext<'a>,
    cursor_line: i16,
    cursor_col: u16,
    cursor_shape: u8,
    config: &'a CiriConfig,
    shaper: &'a TextShaper,
    grapheme_map: &'a std::collections::HashMap<u32, String>,
}

pub struct PackedViewInputs<'a> {
    pub cells: &'a [PackedCell],
    pub cols: u16,
    pub rows: u16,
    pub cursor_line: i16,
    pub cursor_col: u16,
    pub cursor_shape: u8,
    pub config: &'a CiriConfig,
    pub shaper: &'a TextShaper,
    pub colors: &'a ColorTable,
    pub grapheme_map: &'a std::collections::HashMap<u32, String>,
}

impl<'a> PackedViewInputs<'a> {
    fn build_params(&'a self, metrics: &'a CellMetrics) -> ViewBuildParams<'a> {
        ViewBuildParams {
            grid: PackedGridContext {
                cells: self.cells,
                cols: self.cols,
                rows: self.rows,
                cjk_font_id: self.shaper.cjk_font_id(),
                metrics,
                colors: self.colors,
            },
            cursor_line: self.cursor_line,
            cursor_col: self.cursor_col,
            cursor_shape: self.cursor_shape,
            config: self.config,
            shaper: self.shaper,
            grapheme_map: self.grapheme_map,
        }
    }
}

impl<'a> PackedGridContext<'a> {
    fn build_row_data(
        self,
        row: usize,
        row_lig_data: Option<&RowLigatureData>,
        atlas: &mut GlyphCache,
    ) -> RowRenderData {
        render_single_row(self, row, row_lig_data, atlas)
    }
}

fn build_row_lig_cache(params: &ViewBuildParams<'_>) -> Vec<RowLigatureData> {
    let Some(fid) = params.shaper.primary_font_id() else {
        return Vec::new();
    };
    let Some(face) = params.shaper.create_face(fid) else {
        return Vec::new();
    };

    (0..params.grid.rows as usize)
        .map(|row| precompute_row_shaping(params, row, fid, &face))
        .collect()
}

fn precompute_dirty_row_shaping(
    params: &ViewBuildParams<'_>,
    dirty_rows: &[bool],
) -> Vec<Option<RowLigatureData>> {
    let Some(fid) = params.shaper.primary_font_id() else {
        return Vec::new();
    };
    let Some(face) = params.shaper.create_face(fid) else {
        return Vec::new();
    };

    dirty_rows
        .iter()
        .enumerate()
        .take(params.grid.rows as usize)
        .map(|(row, dirty)| dirty.then(|| precompute_row_shaping(params, row, fid, &face)))
        .collect()
}

fn build_row_render_cache(
    grid: PackedGridContext<'_>,
    row_lig_data: &[RowLigatureData],
    atlas: &mut GlyphCache,
) -> Vec<RowRenderData> {
    let mut row_data = Vec::with_capacity(grid.rows as usize);
    for row in 0..grid.rows as usize {
        row_data.push(grid.build_row_data(row, row_lig_data.get(row), atlas));
    }
    row_data
}

fn update_dirty_rows(
    view: &mut TerminalView,
    dirty_rows: &[bool],
    grid: PackedGridContext<'_>,
    atlas: &mut GlyphCache,
    rebuilt_row_lig_cache: &[Option<RowLigatureData>],
) {
    for (row, &dirty) in dirty_rows.iter().enumerate().take(grid.rows as usize) {
        if !dirty || row >= view.row_data.len() {
            continue;
        }

        if let Some(Some(rebuilt)) = rebuilt_row_lig_cache.get(row) {
            if row < view.row_lig_cache.len() {
                view.row_lig_cache[row] = rebuilt.clone();
            } else {
                view.row_lig_cache.push(rebuilt.clone());
            }
        }

        view.row_data[row] = grid.build_row_data(
            row,
            rebuilt_row_lig_cache.get(row).and_then(Option::as_ref),
            atlas,
        );
    }
}

/// Build rendering data from a `PackedCell` grid (client-side path, full rebuild).
///
/// The `shaper` is passed separately from `atlas` to allow simultaneous
/// immutable shaper access (for Face creation) and mutable atlas access
/// (for glyph caching).
pub fn build_view_from_grid(atlas: &mut GlyphCache, inputs: &PackedViewInputs<'_>) -> TerminalView {
    let metrics = CellMetrics::new(atlas, inputs.config);
    let params = inputs.build_params(&metrics);
    let row_lig_data = build_row_lig_cache(&params);
    let row_data = build_row_render_cache(params.grid, &row_lig_data, atlas);

    let cursor_rects = make_cursor_rects(
        params.cursor_shape,
        params.cursor_line as i32,
        params.cursor_col as usize,
        params.grid.rows as usize,
        params.grid.metrics,
        params.config,
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
    inputs: &PackedViewInputs<'_>,
    atlas: &mut GlyphCache,
) {
    let metrics = CellMetrics::new(atlas, inputs.config);
    let params = inputs.build_params(&metrics);
    let rebuilt_row_lig_cache = precompute_dirty_row_shaping(&params, dirty_rows);
    update_dirty_rows(view, dirty_rows, params.grid, atlas, &rebuilt_row_lig_cache);

    // Rebuild cursor
    view.cursor_rects = make_cursor_rects(
        params.cursor_shape,
        params.cursor_line as i32,
        params.cursor_col as usize,
        params.grid.rows as usize,
        params.grid.metrics,
        params.config,
    );

    // Bump generation and re-flatten
    view.generation = view.generation.wrapping_add(1);
    flatten_view(view);
}

// ─── Text shaping integration ───────────────────────────────────────

/// Pre-compute all ligature/grapheme shaping data for a single row.
/// Uses a pre-created Face to avoid per-row Face::from_slice overhead.
fn precompute_row_shaping(
    params: &ViewBuildParams<'_>,
    row: usize,
    fid: fontdb::ID,
    face: &rustybuzz::Face,
) -> RowLigatureData {
    #[allow(clippy::too_many_arguments)]
    fn flush_ligature_run(
        shaper: &TextShaper,
        face: &rustybuzz::Face,
        fid: fontdb::ID,
        cols_usize: usize,
        run_start: Option<usize>,
        run_text: &str,
        run_style: FontStyle,
        run_fg: [f32; 4],
        skip_cols: &mut [bool],
        ligature_glyphs: &mut Vec<(usize, u32, fontdb::ID, FontStyle, [f32; 4])>,
    ) {
        if let Some(start) = run_start
            && run_text.len() >= 2
        {
            for lig in shaper.detect_ligatures_with_face(run_text, face, fid) {
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
    }

    let cols_usize = params.grid.cols as usize;
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
            if idx < params.grid.cells.len() {
                CellProps::from_packed_cell_fast(&params.grid.cells[idx], params.grid.colors)
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
            flush_ligature_run(
                params.shaper,
                face,
                fid,
                cols_usize,
                run_start,
                &run_text,
                run_style,
                run_fg,
                &mut skip_cols,
                &mut ligature_glyphs,
            );
            run_start = Some(col);
            run_text.clear();
            run_text.push(props.ch);
            run_style = props.style;
            run_fg = props.fg;
        } else {
            flush_ligature_run(
                params.shaper,
                face,
                fid,
                cols_usize,
                run_start,
                &run_text,
                run_style,
                run_fg,
                &mut skip_cols,
                &mut ligature_glyphs,
            );
            run_start = None;
            run_text.clear();
        }
    }

    // ── Detect grapheme clusters ──
    for col in 0..cols_usize {
        let idx = row * cols_usize + col;
        if idx >= params.grid.cells.len() {
            break;
        }
        let Some(props) =
            CellProps::from_packed_cell_fast(&params.grid.cells[idx], params.grid.colors)
        else {
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
        let (cluster_str, consumed_cols) =
            if let Some(full_grapheme) = params.grapheme_map.get(&(idx as u32)) {
                // Server sent the full grapheme — use it directly.
                // Wide char spacers are already skipped by render_single_row.
                (full_grapheme.clone(), 0usize)
            } else if is_regional_indicator(props.ch) {
                // Regional Indicator: pair with the next cell if it's also an RI
                let mut s = String::from(props.ch);
                let next_col = col + 1;
                if next_col < cols_usize {
                    let li = row * cols_usize + next_col;
                    if li < params.grid.cells.len() {
                        let next_ch = params.grid.cells[li].ch();
                        if is_regional_indicator(next_ch) {
                            s.push(next_ch);
                        }
                    }
                }
                let consumed = s.chars().count() - 1; // cells consumed after base
                (s, consumed)
            } else {
                // Look ahead for combining/modifier characters in adjacent cells
                let mut s = String::from(props.ch);
                let mut look = col + if props.is_wide { 2 } else { 1 };
                let mut consumed = 0usize;
                while look < cols_usize {
                    let li = row * cols_usize + look;
                    if li >= params.grid.cells.len() {
                        break;
                    }
                    let next_ch = params.grid.cells[li].ch();
                    if is_combining_or_modifier(next_ch) {
                        s.push(next_ch);
                        consumed += 1;
                        look += 1;
                    } else {
                        break;
                    }
                }
                (s, consumed)
            };

        if cluster_str.graphemes(true).count() == 1
            && cluster_str.chars().count() > 1
            && let Some((gid, fid)) = params
                .shaper
                .shape_grapheme_with_fallback(&cluster_str, face)
        {
            grapheme_glyphs.push((col, gid, fid));
            // Mark consumed cells so they aren't rendered independently
            let start = col + if props.is_wide { 2 } else { 1 };
            for k in 0..consumed_cols {
                let c = start + k;
                if c < cols_usize {
                    skip_cols[c] = true;
                }
            }
        }
    }

    // ── Single-char shaping for all remaining characters ──
    let mut char_glyphs = Vec::new();
    for (col, should_skip) in skip_cols.iter().enumerate().take(cols_usize) {
        if *should_skip {
            continue;
        }
        // Skip columns already handled by grapheme shaping
        if grapheme_glyphs
            .binary_search_by_key(&col, |(c, _, _)| *c)
            .is_ok()
        {
            continue;
        }
        // Skip columns handled by ligatures
        if ligature_glyphs.iter().any(|(c, _, _, _, _)| *c == col) {
            continue;
        }
        let idx = row * cols_usize + col;
        if idx >= params.grid.cells.len() {
            break;
        }
        let Some(props) =
            CellProps::from_packed_cell_fast(&params.grid.cells[idx], params.grid.colors)
        else {
            continue;
        };
        if props.is_hidden || props.ch == ' ' || props.ch == '\0' || props.ch.is_control() {
            continue;
        }
        if let Some((gid, fid)) = params.shaper.shape_char_with_fallback(props.ch, face) {
            char_glyphs.push((col, gid, fid, props.is_wide));
        }
    }

    RowLigatureData {
        skip_cols,
        ligature_glyphs,
        grapheme_glyphs,
        char_glyphs,
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
        // Only constrain color emoji in wide cells; text glyphs use bearing positioning
        let g = if cell.is_wide && entry.is_color {
            constrain_wide_glyph(&entry, px, py, m, cell.fg)
        } else {
            make_relative_glyph(&entry, px, py, m.baseline, cell.fg)
        };
        if entry.is_color {
            color_glyphs.push(g);
        } else {
            glyphs.push(g);
        }
    }
}

/// Create a `RelativeGlyph` positioned by bearing offsets.
#[inline]
fn make_relative_glyph(
    entry: &GlyphEntry,
    px: f32,
    py: f32,
    baseline: f32,
    color: [f32; 4],
) -> RelativeGlyph {
    RelativeGlyph {
        px: (px + entry.bearing_x).round(),
        py: (py + baseline - entry.bearing_y).round(),
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
fn constrain_wide_glyph(
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

/// Place a wide text glyph inside a double-width cell without enlarging height.
/// Keeps baseline-aligned vertical metrics and only constrains horizontal width.
#[inline]
fn constrain_wide_text_glyph(
    entry: &GlyphEntry,
    px: f32,
    py: f32,
    m: &CellMetrics,
    color: [f32; 4],
) -> RelativeGlyph {
    let gw = entry.width as f32;
    let target_w = m.cw * 2.0;
    let final_w = gw.min(target_w);
    let offset_x = (target_w - final_w) * 0.5;
    RelativeGlyph {
        px: (px + offset_x).round(),
        py: (py + m.baseline - entry.bearing_y).round(),
        glyph_w: final_w,
        glyph_h: entry.height as f32,
        u0: entry.u0,
        v0: entry.v0,
        u1: entry.u1,
        v1: entry.v1,
        color,
    }
}

/// Check if a character is a Unicode combining character, ZWJ, or variation selector.
/// Regional Indicator Symbols are NOT included — they are base characters that pair
/// only with other regional indicators (handled separately in grapheme detection).
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
    )
}

/// Regional Indicator Symbols: U+1F1E6 ('🇦') to U+1F1FF ('🇿').
/// Two adjacent RIs form a single flag emoji grapheme cluster.
fn is_regional_indicator(c: char) -> bool {
    ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)
}

/// Visual scrollbar width in pixels.
pub const SCROLLBAR_WIDTH: f32 = 4.0;
/// Margin between scrollbar and pane edge in pixels.
pub const SCROLLBAR_MARGIN: f32 = 2.0;

/// Build a scrollbar rect for a pane with scrollback.
/// Returns `None` if scrollback is empty (nothing to scroll).
/// Visual state of the scrollbar thumb.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollbarState {
    Idle,
    Hovered,
    Pressed,
}

pub fn build_scrollbar(
    scroll_offset: usize,
    total_lines: usize,
    visible_rows: u16,
    pane_width: f32,
    pane_height: f32,
    state: ScrollbarState,
    config: &CiriConfig,
) -> Option<Rect> {
    let visible = visible_rows as usize;
    if total_lines <= visible {
        return None;
    }

    let thumb_height = scrollbar_thumb_height(visible, total_lines, pane_height);
    let y = scrollbar_thumb_y(
        scroll_offset,
        total_lines,
        visible,
        pane_height,
        thumb_height,
    );
    let [r, g, b, _] = scrollbar_thumb_color(state, config);

    Some(Rect {
        x: pane_width - SCROLLBAR_WIDTH - SCROLLBAR_MARGIN,
        y,
        w: SCROLLBAR_WIDTH,
        h: thumb_height,
        color: [r, g, b, scrollbar_thumb_alpha(state)],
    })
}

fn scrollbar_thumb_height(visible: usize, total_lines: usize, pane_height: f32) -> f32 {
    let ratio = visible as f32 / total_lines as f32;
    (ratio * pane_height).max(10.0)
}

fn scrollbar_thumb_y(
    scroll_offset: usize,
    total_lines: usize,
    visible: usize,
    pane_height: f32,
    thumb_height: f32,
) -> f32 {
    let max_offset = total_lines - visible;
    let position_ratio = if max_offset == 0 {
        1.0
    } else {
        1.0 - (scroll_offset as f32 / max_offset as f32)
    };
    position_ratio * (pane_height - thumb_height)
}

fn scrollbar_thumb_color(state: ScrollbarState, config: &CiriConfig) -> [f32; 4] {
    match state {
        ScrollbarState::Idle => ThemeConfig::parse_color(&config.theme.bright_black),
        ScrollbarState::Hovered | ScrollbarState::Pressed => {
            ThemeConfig::parse_color(&config.theme.foreground)
        }
    }
}

fn scrollbar_thumb_alpha(state: ScrollbarState) -> f32 {
    match state {
        ScrollbarState::Idle => 0.4,
        ScrollbarState::Hovered => 0.45,
        ScrollbarState::Pressed => 0.6,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use ciri_config::config::CiriConfig;
    use ciri_protocol::message::{
        CURSOR_BEAM, CURSOR_BLOCK, CURSOR_HIDDEN, CURSOR_HOLLOW_BLOCK, CURSOR_UNDERLINE,
        FLAG_HIDDEN, FLAG_STRIKEOUT, FLAG_UNDERLINE, FLAG_WIDE_CHAR, FLAG_WIDE_CHAR_SPACER,
        PackedCell, PackedColor,
    };
    use std::collections::HashMap;

    fn assert_rect_lists_match(left: &[Rect], right: &[Rect]) {
        assert_eq!(left.len(), right.len());
        for (left, right) in left.iter().zip(right.iter()) {
            assert_eq!(left.x, right.x);
            assert_eq!(left.y, right.y);
            assert_eq!(left.w, right.w);
            assert_eq!(left.h, right.h);
            assert_eq!(left.color, right.color);
        }
    }

    fn assert_relative_glyph_lists_match(left: &[RelativeGlyph], right: &[RelativeGlyph]) {
        assert_eq!(left.len(), right.len());
        for (left, right) in left.iter().zip(right.iter()) {
            assert_eq!(left.px, right.px);
            assert_eq!(left.py, right.py);
            assert_eq!(left.glyph_w, right.glyph_w);
            assert_eq!(left.glyph_h, right.glyph_h);
            assert_eq!(left.color, right.color);
        }
    }

    fn test_config() -> CiriConfig {
        CiriConfig::default()
    }

    fn test_shaper(config: &CiriConfig) -> TextShaper {
        TextShaper::new(&config.font.family)
    }

    fn test_atlas(config: &CiriConfig, shaper: &TextShaper) -> GlyphCache {
        GlyphCache::new(
            config.font.size,
            1.0,
            &config.font.family,
            shaper.primary_font_path(),
            shaper.emoji_font_path(),
            shaper.emoji_font_id(),
            shaper.cjk_font_path(),
            shaper.cjk_font_id(),
            &config.render,
        )
    }

    fn test_color_table(config: &CiriConfig) -> ColorTable {
        ColorTable::new(config)
    }

    #[allow(clippy::too_many_arguments)]
    fn test_view_inputs<'a>(
        cells: &'a [PackedCell],
        cols: u16,
        rows: u16,
        cursor_line: i16,
        cursor_col: u16,
        cursor_shape: u8,
        shaper: &'a TextShaper,
        config: &'a CiriConfig,
        ct: &'a ColorTable,
        grapheme_map: &'a HashMap<u32, String>,
    ) -> PackedViewInputs<'a> {
        PackedViewInputs {
            cells,
            cols,
            rows,
            cursor_line,
            cursor_col,
            cursor_shape,
            config,
            shaper,
            colors: ct,
            grapheme_map,
        }
    }

    fn grid_with_size(cols: usize, rows: usize) -> Vec<PackedCell> {
        vec![PackedCell::default(); cols * rows]
    }

    fn styled_cell(ch: char, fg: PackedColor, bg: PackedColor, flags: u16) -> PackedCell {
        let mut cell = PackedCell::with_ch(ch);
        cell.fg = fg;
        cell.bg = bg;
        cell.flags = flags.to_le_bytes();
        cell
    }

    #[test]
    fn packed_hidden_and_wide_spacer_cells_do_not_render_text_or_decorations() {
        let config = test_config();
        let shaper = test_shaper(&config);
        let mut atlas = test_atlas(&config, &shaper);
        let ct = test_color_table(&config);
        let graphemes = HashMap::new();
        let mut cells = grid_with_size(3, 1);
        cells[0] = styled_cell(
            'A',
            PackedColor::rgb(255, 0, 0),
            PackedColor::rgb(0, 0, 32),
            FLAG_HIDDEN | FLAG_UNDERLINE | FLAG_STRIKEOUT,
        );
        cells[1] = styled_cell(
            '好',
            PackedColor::rgb(0, 255, 0),
            PackedColor::rgb(32, 0, 0),
            FLAG_WIDE_CHAR,
        );
        cells[2] = styled_cell(
            ' ',
            PackedColor::rgb(0, 0, 255),
            PackedColor::rgb(0, 32, 0),
            FLAG_WIDE_CHAR_SPACER,
        );
        let params = test_view_inputs(
            &cells,
            3,
            1,
            -1,
            0,
            CURSOR_HIDDEN,
            &shaper,
            &config,
            &ct,
            &graphemes,
        );

        let view = build_view_from_grid(&mut atlas, &params);

        let wide_background = view
            .bg_rects
            .iter()
            .find(|rect| rect.color == ct.resolve_packed(cells[1].bg))
            .expect("wide visible cell background should render");
        assert_eq!(wide_background.w, atlas.cell_width * 2.0);
        assert!(
            view.bg_rects
                .iter()
                .all(|rect| rect.color != ct.resolve_packed(cells[0].fg)),
            "hidden cells should not emit underline/strikeout rects"
        );
        assert!(
            !view.glyph_instances.is_empty() || !view.color_glyph_instances.is_empty(),
            "visible wide cell should still render a glyph"
        );
    }

    #[test]
    fn packed_cursor_shapes_map_to_expected_rect_geometry() {
        let config = test_config();
        let shaper = test_shaper(&config);
        let mut atlas = test_atlas(&config, &shaper);
        let ct = test_color_table(&config);
        let graphemes = HashMap::new();
        let cells = grid_with_size(2, 2);
        let cw = atlas.cell_width;
        let ch = atlas.cell_height;
        let block_params = test_view_inputs(
            &cells,
            2,
            2,
            1,
            1,
            CURSOR_BLOCK,
            &shaper,
            &config,
            &ct,
            &graphemes,
        );
        let block = build_view_from_grid(&mut atlas, &block_params);
        assert_eq!(block.cursor_rects.len(), 1);
        assert_eq!(block.cursor_rects[0].x, cw);
        assert_eq!(block.cursor_rects[0].y, ch);
        assert_eq!(block.cursor_rects[0].w, cw);
        assert_eq!(block.cursor_rects[0].h, ch);

        let beam_params = test_view_inputs(
            &cells,
            2,
            2,
            0,
            1,
            CURSOR_BEAM,
            &shaper,
            &config,
            &ct,
            &graphemes,
        );
        let beam = build_view_from_grid(&mut atlas, &beam_params);
        assert_eq!(beam.cursor_rects.len(), 1);
        assert_eq!(beam.cursor_rects[0].w, 2.0);
        assert_eq!(beam.cursor_rects[0].h, ch);

        let underline_params = test_view_inputs(
            &cells,
            2,
            2,
            0,
            0,
            CURSOR_UNDERLINE,
            &shaper,
            &config,
            &ct,
            &graphemes,
        );
        let underline = build_view_from_grid(&mut atlas, &underline_params);
        assert_eq!(underline.cursor_rects.len(), 1);
        assert_eq!(underline.cursor_rects[0].y, ch - 2.0);
        assert_eq!(underline.cursor_rects[0].h, 2.0);

        let hollow_params = test_view_inputs(
            &cells,
            2,
            2,
            1,
            0,
            CURSOR_HOLLOW_BLOCK,
            &shaper,
            &config,
            &ct,
            &graphemes,
        );
        let hollow = build_view_from_grid(&mut atlas, &hollow_params);
        assert_eq!(hollow.cursor_rects.len(), 4);
    }

    #[test]
    fn incremental_update_matches_full_rebuild_for_same_final_grid() {
        let config = test_config();
        let shaper = test_shaper(&config);
        let ct = test_color_table(&config);
        let graphemes = HashMap::new();
        let mut initial_cells = grid_with_size(4, 2);
        initial_cells[0] = styled_cell(
            'a',
            PackedColor::rgb(255, 255, 255),
            PackedColor::rgb(16, 16, 16),
            0,
        );
        initial_cells[1] = styled_cell(
            'b',
            PackedColor::rgb(200, 0, 0),
            PackedColor::rgb(16, 16, 16),
            FLAG_UNDERLINE,
        );

        let mut full_cells = initial_cells.clone();
        full_cells[0] = styled_cell(
            '中',
            PackedColor::rgb(255, 255, 0),
            PackedColor::rgb(0, 0, 48),
            FLAG_WIDE_CHAR,
        );
        full_cells[1] = styled_cell(
            ' ',
            PackedColor::rgb(255, 255, 0),
            PackedColor::rgb(0, 0, 48),
            FLAG_WIDE_CHAR_SPACER,
        );
        full_cells[5] = styled_cell(
            'x',
            PackedColor::rgb(0, 255, 255),
            PackedColor::rgb(48, 0, 0),
            FLAG_STRIKEOUT,
        );

        let mut atlas_for_incremental = test_atlas(&config, &shaper);
        let initial_params = test_view_inputs(
            &initial_cells,
            4,
            2,
            0,
            1,
            CURSOR_BLOCK,
            &shaper,
            &config,
            &ct,
            &graphemes,
        );
        let mut incremental = build_view_from_grid(&mut atlas_for_incremental, &initial_params);
        let updated_params = test_view_inputs(
            &full_cells,
            4,
            2,
            1,
            2,
            CURSOR_UNDERLINE,
            &shaper,
            &config,
            &ct,
            &graphemes,
        );
        update_view_from_grid(
            &mut incremental,
            &[true, true],
            &updated_params,
            &mut atlas_for_incremental,
        );

        let mut atlas_for_full = test_atlas(&config, &shaper);
        let rebuilt_params = test_view_inputs(
            &full_cells,
            4,
            2,
            1,
            2,
            CURSOR_UNDERLINE,
            &shaper,
            &config,
            &ct,
            &graphemes,
        );
        let rebuilt = build_view_from_grid(&mut atlas_for_full, &rebuilt_params);

        assert_rect_lists_match(&incremental.bg_rects, &rebuilt.bg_rects);
        assert_rect_lists_match(&incremental.cursor_rects, &rebuilt.cursor_rects);
        assert_relative_glyph_lists_match(&incremental.glyph_instances, &rebuilt.glyph_instances);
        assert_relative_glyph_lists_match(
            &incremental.color_glyph_instances,
            &rebuilt.color_glyph_instances,
        );
    }

    #[test]
    fn incremental_update_only_recomputes_dirty_row_shaping() {
        let config = test_config();
        let shaper = test_shaper(&config);
        let ct = test_color_table(&config);
        let graphemes = HashMap::new();
        let initial_cells = vec![
            PackedCell::with_ch('f'),
            PackedCell::with_ch('i'),
            PackedCell::default(),
            PackedCell::default(),
            PackedCell::with_ch('a'),
            PackedCell::with_ch('b'),
            PackedCell::default(),
            PackedCell::default(),
        ];

        let mut atlas = test_atlas(&config, &shaper);
        let initial_params = test_view_inputs(
            &initial_cells,
            4,
            2,
            -1,
            0,
            CURSOR_HIDDEN,
            &shaper,
            &config,
            &ct,
            &graphemes,
        );
        let mut view = build_view_from_grid(&mut atlas, &initial_params);
        let original_clean_row = view.row_lig_cache[1].clone();

        let updated_cells = vec![
            PackedCell::with_ch('o'),
            PackedCell::with_ch('f'),
            PackedCell::with_ch('f'),
            PackedCell::with_ch('i'),
            PackedCell::with_ch('a'),
            PackedCell::with_ch('b'),
            PackedCell::default(),
            PackedCell::default(),
        ];
        let updated_params = test_view_inputs(
            &updated_cells,
            4,
            2,
            -1,
            0,
            CURSOR_HIDDEN,
            &shaper,
            &config,
            &ct,
            &graphemes,
        );

        update_view_from_grid(&mut view, &[true, false], &updated_params, &mut atlas);

        assert_eq!(
            view.row_lig_cache[1].skip_cols,
            original_clean_row.skip_cols
        );
        assert_eq!(
            view.row_lig_cache[1].ligature_glyphs,
            original_clean_row.ligature_glyphs
        );
        assert_eq!(
            view.row_lig_cache[1].grapheme_glyphs,
            original_clean_row.grapheme_glyphs
        );
        assert_eq!(
            view.row_lig_cache[1].char_glyphs,
            original_clean_row.char_glyphs
        );
    }

    #[test]
    fn scrollbar_hidden_without_scrollback() {
        let config = test_config();
        assert!(build_scrollbar(0, 4, 4, 120.0, 80.0, ScrollbarState::Idle, &config).is_none());
    }

    #[test]
    fn scrollbar_thumb_geometry_tracks_scroll_extent() {
        let config = test_config();
        let rect = build_scrollbar(3, 10, 4, 120.0, 100.0, ScrollbarState::Idle, &config)
            .expect("scrollback should produce a scrollbar");
        assert_eq!(rect.x, 114.0);
        assert_eq!(rect.w, SCROLLBAR_WIDTH);
        assert_eq!(rect.h, 40.0);
        assert_eq!(rect.y, 30.0);
    }

    #[test]
    fn scrollbar_thumb_visual_state_changes_color_and_alpha() {
        let config = test_config();
        let idle = build_scrollbar(0, 10, 4, 120.0, 100.0, ScrollbarState::Idle, &config)
            .expect("scrollback should produce a scrollbar");
        let hovered = build_scrollbar(0, 10, 4, 120.0, 100.0, ScrollbarState::Hovered, &config)
            .expect("scrollback should produce a scrollbar");
        let pressed = build_scrollbar(0, 10, 4, 120.0, 100.0, ScrollbarState::Pressed, &config)
            .expect("scrollback should produce a scrollbar");

        assert_eq!(idle.x, hovered.x);
        assert_eq!(idle.y, hovered.y);
        assert_eq!(idle.w, hovered.w);
        assert_eq!(idle.h, hovered.h);
        assert_eq!(hovered.color[..3], pressed.color[..3]);
        assert_eq!(idle.color[3], 0.4);
        assert_eq!(hovered.color[3], 0.45);
        assert_eq!(pressed.color[3], 0.6);
    }
}
