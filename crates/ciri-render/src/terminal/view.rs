//! Terminal view construction and incremental updates.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::Term;
use ciri_config::config::CiriConfig;
use ciri_protocol::message::PackedCell;

use crate::glyph_cache::GlyphCache;
use crate::rect::Rect;
use crate::shaper::TextShaper;

use super::cell::{CellMetrics, CellProps};
use super::color::ColorTable;
use super::cursor::{cursor_shape_to_protocol, make_cursor_rects};
use super::decoration::{flush_bg_strip, render_cell, render_cell_decorations};
use super::glyph::{
    RelativeGlyph, constrain_wide_glyph, constrain_wide_text_glyph, emit_glyph, make_relative_glyph,
};
use super::shaping::{RowLigatureData, precompute_row_shaping};

// ─── Per-row cached data ─────────────────────────────────────────────

/// Per-row cached rendering data for incremental updates.
pub(super) struct RowRenderData {
    glyphs: Vec<RelativeGlyph>,
    color_glyphs: Vec<RelativeGlyph>,
    bg_rects: Vec<Rect>,
}

pub(super) struct CellRenderer<'a> {
    pub(super) row: usize,
    pub(super) metrics: &'a CellMetrics,
    pub(super) atlas: &'a mut GlyphCache,
    pub(super) bg_rects: &'a mut Vec<Rect>,
    pub(super) glyphs: &'a mut Vec<RelativeGlyph>,
    pub(super) color_glyphs: &'a mut Vec<RelativeGlyph>,
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
    pub(super) row_data: Vec<RowRenderData>,
    /// Per-row cached shaping data for incremental rebuilds.
    pub(super) row_lig_cache: Vec<RowLigatureData>,
    /// Monotonically increasing generation counter. Bumped on every build/update.
    pub generation: u64,
}

// ─── Internal grid context types ─────────────────────────────────────

#[derive(Clone, Copy)]
pub(super) struct PackedGridContext<'a> {
    pub(super) cells: &'a [PackedCell],
    pub(super) cols: u16,
    pub(super) rows: u16,
    pub(super) cjk_font_id: Option<fontdb::ID>,
    pub(super) metrics: &'a CellMetrics,
    pub(super) colors: &'a ColorTable,
}

pub(super) struct ViewBuildParams<'a> {
    pub(super) grid: PackedGridContext<'a>,
    pub(super) cursor_line: i16,
    pub(super) cursor_col: u16,
    pub(super) cursor_shape: u8,
    pub(super) config: &'a CiriConfig,
    pub(super) shaper: &'a TextShaper,
    pub(super) grapheme_map: &'a std::collections::HashMap<u32, String>,
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

// ─── Server-side path ────────────────────────────────────────────────

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
        super::cursor::CursorCellContext {
            row_width: cols,
            cell_flags: None,
        },
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

// ─── Client-side path ────────────────────────────────────────────────

/// Build rendering data from a `PackedCell` grid (client-side path, full rebuild).
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
        super::cursor::CursorCellContext {
            row_width: params.grid.cols as usize,
            cell_flags: Some(
                &params
                    .grid
                    .cells
                    .iter()
                    .map(PackedCell::flags_u16)
                    .collect::<Vec<_>>(),
            ),
        },
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
    let cell_flags = params
        .grid
        .cells
        .iter()
        .map(PackedCell::flags_u16)
        .collect::<Vec<_>>();
    view.cursor_rects = make_cursor_rects(
        params.cursor_shape,
        params.cursor_line as i32,
        params.cursor_col as usize,
        params.grid.rows as usize,
        super::cursor::CursorCellContext {
            row_width: params.grid.cols as usize,
            cell_flags: Some(&cell_flags),
        },
        params.grid.metrics,
        params.config,
    );

    // Bump generation and re-flatten
    view.generation = view.generation.wrapping_add(1);
    flatten_view(view);
}

// ─── Row rendering ───────────────────────────────────────────────────

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
                    make_relative_glyph(&entry, px, py, grid.metrics, props.fg)
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
                    make_relative_glyph(&entry, px, py, grid.metrics, props.fg)
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
                let g = make_relative_glyph(&entry, px, py, grid.metrics, fg);
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

// ─── Helpers ─────────────────────────────────────────────────────────

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
