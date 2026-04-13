//! Terminal view construction and incremental updates.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::Term;
use ciri_config::config::CiriConfig;
use ciri_protocol::message::PackedCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::glyph_cache::GlyphCache;
use crate::rect::Rect;
use crate::shaper::TextShaper;

use super::cell::{CellMetrics, CellProps};
use super::color::ColorTable;
use super::cursor::{cursor_shape_to_protocol, make_cursor_rects};
use super::decoration::{flush_bg_strip, render_cell, render_cell_decorations};
use super::glyph::{
    RelativeGlyph, color_glyph_cell_span, constrain_color_glyph_to_cells,
    constrain_wide_text_glyph, emit_glyph, make_relative_glyph,
};
use super::shaping::{RowLigatureData, precompute_row_shaping};

// ─── Per-row cached data ─────────────────────────────────────────────

/// Per-row cached rendering data for incremental updates.
#[derive(Clone)]
pub struct RowRenderData {
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
    pub row_data: Vec<RowRenderData>,
    /// Per-row cached shaping data for incremental rebuilds.
    pub(crate) row_lig_cache: Vec<RowLigatureData>,
    /// Per-row content version, bumped when a row is re-rendered.
    pub row_epochs: Vec<u64>,
    /// Per-row content hash for scroll/content reuse detection.
    pub row_hashes: Vec<u64>,
    /// Last detected viewport row shift. Positive means content moved downward.
    pub last_scroll_shift: i32,
    /// Cached cell height for row-shifted scene transforms.
    pub cell_height: f32,
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

impl TerminalView {
    pub fn row_count(&self) -> usize {
        self.row_data.len()
    }

    pub fn row_epoch(&self, row: usize) -> u64 {
        self.row_epochs.get(row).copied().unwrap_or(0)
    }

    pub fn row_hash(&self, row: usize) -> u64 {
        self.row_hashes.get(row).copied().unwrap_or(0)
    }

    pub fn row_glyphs(&self, row: usize) -> &[RelativeGlyph] {
        self.row_data
            .get(row)
            .map(|row| row.glyphs.as_slice())
            .unwrap_or(&[])
    }

    pub fn row_color_glyphs(&self, row: usize) -> &[RelativeGlyph] {
        self.row_data
            .get(row)
            .map(|row| row.color_glyphs.as_slice())
            .unwrap_or(&[])
    }

    pub fn row_bg_rects(&self, row: usize) -> &[Rect] {
        self.row_data
            .get(row)
            .map(|row| row.bg_rects.as_slice())
            .unwrap_or(&[])
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
        row_epochs: Vec::new(),
        row_hashes: Vec::new(),
        last_scroll_shift: 0,
        cell_height: m.ch,
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
    let row_hashes = build_row_hash_cache(&params);
    let row_epochs = vec![1; row_data.len()];

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

    let view = TerminalView {
        glyph_instances: Vec::new(),
        color_glyph_instances: Vec::new(),
        bg_rects: Vec::new(),
        cursor_rects,
        scrollbar_rect: None,
        scrollbar_key: None,
        row_data,
        row_lig_cache: row_lig_data,
        row_epochs,
        row_hashes,
        last_scroll_shift: 0,
        cell_height: metrics.ch,
        generation: 1,
    };
    view
}

/// Incrementally update a TerminalView for only the dirty rows.
/// Much cheaper than a full rebuild: typically 1-3 rows vs 67 rows at 4K.
pub fn update_view_from_grid(
    view: &mut TerminalView,
    dirty_rows: &[bool],
    scroll_shift: i32,
    inputs: &PackedViewInputs<'_>,
    atlas: &mut GlyphCache,
) {
    let metrics = CellMetrics::new(atlas, inputs.config);
    let params = inputs.build_params(&metrics);
    let new_row_hashes = build_row_hash_cache(&params);
    let detected_scroll_shift =
        detect_scroll_shift(&view.row_hashes, &new_row_hashes).unwrap_or_default();
    let applied_scroll_shift = normalize_scroll_shift(
        if scroll_shift != 0 {
            scroll_shift
        } else {
            detected_scroll_shift
        },
        params.grid.rows as usize,
    );

    if applied_scroll_shift != 0 {
        rotate_row_caches(view, applied_scroll_shift);
    }

    let rows_rebuilt = update_dirty_rows(
        view,
        dirty_rows,
        &new_row_hashes,
        applied_scroll_shift,
        params.grid,
        &params,
        atlas,
    );

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

    view.row_hashes = new_row_hashes;
    view.last_scroll_shift = applied_scroll_shift;
    // Only bump generation when row content actually changed.  Cursor-only
    // changes (position/shape) are captured via grid.cursor_* in the
    // render snapshot hash, so they still trigger redraws without a
    // generation bump.
    if rows_rebuilt || applied_scroll_shift != 0 {
        view.generation = view.generation.wrapping_add(1);
    }
    view.cell_height = metrics.ch;
    view.glyph_instances.clear();
    view.color_glyph_instances.clear();
    view.bg_rects.clear();
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
                .binary_search_by_key(&col, |(c, _, _, _)| *c)
        {
            let gid = ld.grapheme_glyphs[gi].1;
            let glyph_font_id = ld.grapheme_glyphs[gi].2;
            let display_cols = ld.grapheme_glyphs[gi].3;
            if let Some(entry) =
                atlas.ensure_glyph_id(gid, glyph_font_id, props.style, props.is_wide)
                && entry.width > 0
                && entry.height > 0
            {
                let px = col as f32 * grid.metrics.cw;
                let py = row as f32 * grid.metrics.ch;
                let is_cjk_text_wide =
                    props.is_wide && !entry.is_color && Some(glyph_font_id) == grid.cjk_font_id;
                let g = if entry.is_color && display_cols > 1 {
                    constrain_color_glyph_to_cells(
                        &entry,
                        px,
                        py,
                        grid.metrics,
                        props.fg,
                        display_cols,
                    )
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
                let color_span = color_glyph_cell_span(props.ch, is_wide);
                let g = if entry.is_color && color_span > 1 {
                    constrain_color_glyph_to_cells(
                        &entry,
                        px,
                        py,
                        grid.metrics,
                        props.fg,
                        color_span,
                    )
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

fn build_row_lig_cache(params: &ViewBuildParams<'_>) -> Vec<RowLigatureData> {
    let Some(faces) = params.shaper.face_set() else {
        return Vec::new();
    };

    (0..params.grid.rows as usize)
        .map(|row| precompute_row_shaping(params, row, &faces))
        .collect()
}

fn build_row_hash_cache(params: &ViewBuildParams<'_>) -> Vec<u64> {
    (0..params.grid.rows as usize)
        .map(|row| hash_row(params, row))
        .collect()
}

fn hash_row(params: &ViewBuildParams<'_>, row: usize) -> u64 {
    let mut hasher = DefaultHasher::new();
    let cols = params.grid.cols as usize;
    let start = row.saturating_mul(cols);
    let end = (start + cols).min(params.grid.cells.len());
    let cells = &params.grid.cells[start..end];
    hasher.write(bytemuck::cast_slice(cells));
    for col in 0..cols {
        let cell_idx = start + col;
        if let Some(grapheme) = params.grapheme_map.get(&(cell_idx as u32)) {
            col.hash(&mut hasher);
            grapheme.hash(&mut hasher);
        }
    }
    hasher.finish()
}

fn normalize_scroll_shift(shift: i32, row_count: usize) -> i32 {
    if row_count == 0 {
        return 0;
    }
    shift.clamp(-(row_count as i32 - 1), row_count as i32 - 1)
}

fn detect_scroll_shift(old_hashes: &[u64], new_hashes: &[u64]) -> Option<i32> {
    let row_count = old_hashes.len().min(new_hashes.len());
    if row_count < 2 {
        return None;
    }

    let mut best_shift = 0;
    let mut best_matches = 0usize;
    for shift in -(row_count as i32 - 1)..=(row_count as i32 - 1) {
        if shift == 0 {
            continue;
        }
        let overlap = row_count.saturating_sub(shift.unsigned_abs() as usize);
        if overlap == 0 {
            continue;
        }

        let matches = if shift > 0 {
            (0..overlap)
                .filter(|&row| old_hashes[row] == new_hashes[row + shift as usize])
                .count()
        } else {
            let amount = (-shift) as usize;
            (0..overlap)
                .filter(|&row| old_hashes[row + amount] == new_hashes[row])
                .count()
        };

        if matches == overlap && matches > best_matches {
            best_matches = matches;
            best_shift = shift;
        }
    }

    (best_matches > 0).then_some(best_shift)
}

fn rotate_row_caches(view: &mut TerminalView, scroll_shift: i32) {
    let count = view.row_data.len();
    if count == 0 || scroll_shift == 0 {
        return;
    }

    let amount = scroll_shift.unsigned_abs() as usize;
    if amount >= count {
        return;
    }

    if scroll_shift > 0 {
        view.row_data.rotate_right(amount);
        view.row_lig_cache.rotate_right(amount);
        view.row_epochs.rotate_right(amount);
        view.row_hashes.rotate_right(amount);
    } else {
        view.row_data.rotate_left(amount);
        view.row_lig_cache.rotate_left(amount);
        view.row_epochs.rotate_left(amount);
        view.row_hashes.rotate_left(amount);
    }
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

/// Returns `true` if at least one row was actually rebuilt.
fn update_dirty_rows(
    view: &mut TerminalView,
    _dirty_rows: &[bool],
    new_row_hashes: &[u64],
    scroll_shift: i32,
    grid: PackedGridContext<'_>,
    params: &ViewBuildParams<'_>,
    atlas: &mut GlyphCache,
) -> bool {
    let mut any_rebuilt = false;

    let Some(faces) = params.shaper.face_set() else {
        for row in 0..grid.rows as usize {
            if row >= view.row_data.len() {
                continue;
            }
            let exposed = if scroll_shift > 0 {
                row < scroll_shift as usize
            } else if scroll_shift < 0 {
                row >= grid.rows as usize - (-scroll_shift) as usize
            } else {
                false
            };
            let hash_changed =
                view.row_hashes.get(row).copied() != new_row_hashes.get(row).copied();
            // Only rebuild when content actually changed (hash) or scroll-
            // exposed.  Server dirty flags alone are not sufficient — they
            // fire for cursor-line damage even when no cell content changed.
            if !exposed && !hash_changed {
                continue;
            }
            view.row_data[row] = grid.build_row_data(row, None, atlas);
            if let Some(epoch) = view.row_epochs.get_mut(row) {
                *epoch = epoch.wrapping_add(1);
            }
            any_rebuilt = true;
        }
        return any_rebuilt;
    };

    for row in 0..grid.rows as usize {
        if row >= view.row_data.len() {
            continue;
        }

        let exposed = if scroll_shift > 0 {
            row < scroll_shift as usize
        } else if scroll_shift < 0 {
            row >= grid.rows as usize - (-scroll_shift) as usize
        } else {
            false
        };
        let hash_changed = view.row_hashes.get(row).copied() != new_row_hashes.get(row).copied();
        if !exposed && !hash_changed {
            continue;
        }

        let rebuilt = precompute_row_shaping(params, row, &faces);
        if row < view.row_lig_cache.len() {
            view.row_lig_cache[row] = rebuilt.clone();
        } else {
            view.row_lig_cache.push(rebuilt.clone());
        }

        view.row_data[row] = grid.build_row_data(row, Some(&rebuilt), atlas);
        if let Some(epoch) = view.row_epochs.get_mut(row) {
            *epoch = epoch.wrapping_add(1);
        }
        any_rebuilt = true;
    }
    any_rebuilt
}
