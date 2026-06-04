use super::*;
use crate::glyph_cache::{FontInitParams, GlyphCache};
use crate::rect::Rect;
use crate::shaper::TextShaper;
use loom_config::config::LoomConfig;
use loom_protocol::message::{
    CURSOR_BEAM, CURSOR_BLOCK, CURSOR_HIDDEN, CURSOR_HOLLOW_BLOCK, CURSOR_UNDERLINE, FLAG_HIDDEN,
    FLAG_STRIKEOUT, FLAG_UNDERLINE, FLAG_WIDE_CHAR, FLAG_WIDE_CHAR_SPACER, PackedCell, PackedColor,
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

fn flattened_bg_rects(view: &TerminalView) -> Vec<Rect> {
    let mut rects = Vec::new();
    for row in 0..view.row_count() {
        rects.extend_from_slice(view.row_bg_rects(row));
    }
    rects
}

fn flattened_glyphs(view: &TerminalView) -> Vec<RelativeGlyph> {
    let mut glyphs = Vec::new();
    for row in 0..view.row_count() {
        glyphs.extend_from_slice(view.row_glyphs(row));
    }
    glyphs
}

fn flattened_color_glyphs(view: &TerminalView) -> Vec<RelativeGlyph> {
    let mut glyphs = Vec::new();
    for row in 0..view.row_count() {
        glyphs.extend_from_slice(view.row_color_glyphs(row));
    }
    glyphs
}

fn test_config() -> LoomConfig {
    LoomConfig::default()
}

fn test_shaper(config: &LoomConfig) -> TextShaper {
    TextShaper::new(&config.font.family)
}

fn test_atlas(config: &LoomConfig, shaper: &TextShaper) -> GlyphCache {
    GlyphCache::new(&FontInitParams {
        font_size_pt: config.font.size,
        dpi_scale: 1.0,
        family_name: &config.font.family,
        ui_family_name: None,
        primary_font_path: shaper.primary_font_path(),
        emoji_font_path: shaper.emoji_font_path(),
        emoji_font_id: shaper.emoji_font_id(),
        cjk_font_path: shaper.cjk_font_path(),
        cjk_font_id: shaper.cjk_font_id(),
        ui_font_path: None,
        ui_font_id: None,
        ui_pixel_size: None,
        render_config: &config.render,
        font_resolver: shaper.font_resolver(),
        #[cfg(windows)]
        dwrite_resolver: shaper.dwrite_resolver(),
        cell_width_scale: None,
        cell_height_scale: None,
    })
}

fn test_color_table(config: &LoomConfig) -> ColorTable {
    ColorTable::new(config)
}

fn test_view_inputs<'a>(
    frame: TestFrame<'a>,
    shaper: &'a TextShaper,
    config: &'a LoomConfig,
    ct: &'a ColorTable,
    grapheme_map: &'a HashMap<u32, String>,
) -> PackedViewInputs<'a> {
    PackedViewInputs {
        cells: frame.cells,
        cols: frame.cols,
        rows: frame.rows,
        cursor_line: frame.cursor_line,
        cursor_col: frame.cursor_col,
        cursor_shape: frame.cursor_shape,
        config,
        shaper,
        colors: ct,
        grapheme_map,
    }
}

struct TestFrame<'a> {
    cells: &'a [PackedCell],
    cols: u16,
    rows: u16,
    cursor_line: i16,
    cursor_col: u16,
    cursor_shape: u8,
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
        TestFrame {
            cells: &cells,
            cols: 3,
            rows: 1,
            cursor_line: -1,
            cursor_col: 0,
            cursor_shape: CURSOR_HIDDEN,
        },
        &shaper,
        &config,
        &ct,
        &graphemes,
    );

    let view = build_view_from_grid(&mut atlas, &params);

    let bg_rects = flattened_bg_rects(&view);
    let glyphs = flattened_glyphs(&view);
    let color_glyphs = flattened_color_glyphs(&view);

    let wide_background = bg_rects
        .iter()
        .find(|rect| rect.color == ct.resolve_packed(cells[1].bg))
        .expect("wide visible cell background should render");
    assert_eq!(wide_background.w, atlas.cell_width * 2.0);
    assert!(
        bg_rects
            .iter()
            .all(|rect| rect.color != ct.resolve_packed(cells[0].fg)),
        "hidden cells should not emit underline/strikeout rects"
    );
    assert!(
        !glyphs.is_empty() || !color_glyphs.is_empty(),
        "visible wide cell should still render a glyph"
    );
}

#[test]
fn cursor_on_wide_spacer_anchors_to_leading_cell() {
    let config = test_config();
    let shaper = test_shaper(&config);
    let mut atlas = test_atlas(&config, &shaper);
    let ct = test_color_table(&config);
    let graphemes = HashMap::new();
    let mut cells = grid_with_size(2, 1);
    cells[0] = styled_cell(
        '好',
        PackedColor::rgb(255, 255, 255),
        PackedColor::rgb(0, 0, 0),
        FLAG_WIDE_CHAR,
    );
    cells[1] = styled_cell(
        ' ',
        PackedColor::rgb(255, 255, 255),
        PackedColor::rgb(0, 0, 0),
        FLAG_WIDE_CHAR_SPACER,
    );

    let lead_params = test_view_inputs(
        TestFrame {
            cells: &cells,
            cols: 2,
            rows: 1,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
        },
        &shaper,
        &config,
        &ct,
        &graphemes,
    );
    let lead_view = build_view_from_grid(&mut atlas, &lead_params);
    assert_eq!(lead_view.cursor_rects.len(), 1);
    let lead_rect = lead_view.cursor_rects[0];
    assert_eq!(lead_rect.x, 0.0);
    assert_eq!(lead_rect.w, atlas.cell_width);

    let spacer_params = test_view_inputs(
        TestFrame {
            cells: &cells,
            cols: 2,
            rows: 1,
            cursor_line: 0,
            cursor_col: 1,
            cursor_shape: CURSOR_BLOCK,
        },
        &shaper,
        &config,
        &ct,
        &graphemes,
    );
    let spacer_view = build_view_from_grid(&mut atlas, &spacer_params);
    assert_eq!(spacer_view.cursor_rects.len(), 1);
    assert_eq!(spacer_view.cursor_rects[0].x, 0.0);
    assert_eq!(spacer_view.cursor_rects[0].y, lead_rect.y);
    assert_eq!(spacer_view.cursor_rects[0].w, atlas.cell_width * 2.0);
    assert_eq!(spacer_view.cursor_rects[0].h, lead_rect.h);
    assert_eq!(spacer_view.cursor_rects[0].color, lead_rect.color);
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
        TestFrame {
            cells: &cells,
            cols: 2,
            rows: 2,
            cursor_line: 1,
            cursor_col: 1,
            cursor_shape: CURSOR_BLOCK,
        },
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
        TestFrame {
            cells: &cells,
            cols: 2,
            rows: 2,
            cursor_line: 0,
            cursor_col: 1,
            cursor_shape: CURSOR_BEAM,
        },
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
        TestFrame {
            cells: &cells,
            cols: 2,
            rows: 2,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_UNDERLINE,
        },
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
        TestFrame {
            cells: &cells,
            cols: 2,
            rows: 2,
            cursor_line: 1,
            cursor_col: 0,
            cursor_shape: CURSOR_HOLLOW_BLOCK,
        },
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
        TestFrame {
            cells: &initial_cells,
            cols: 4,
            rows: 2,
            cursor_line: 0,
            cursor_col: 1,
            cursor_shape: CURSOR_BLOCK,
        },
        &shaper,
        &config,
        &ct,
        &graphemes,
    );
    let mut incremental = build_view_from_grid(&mut atlas_for_incremental, &initial_params);
    let updated_params = test_view_inputs(
        TestFrame {
            cells: &full_cells,
            cols: 4,
            rows: 2,
            cursor_line: 1,
            cursor_col: 2,
            cursor_shape: CURSOR_UNDERLINE,
        },
        &shaper,
        &config,
        &ct,
        &graphemes,
    );
    update_view_from_grid(
        &mut incremental,
        &[true, true],
        0,
        &updated_params,
        &mut atlas_for_incremental,
    );

    let mut atlas_for_full = test_atlas(&config, &shaper);
    let rebuilt_params = test_view_inputs(
        TestFrame {
            cells: &full_cells,
            cols: 4,
            rows: 2,
            cursor_line: 1,
            cursor_col: 2,
            cursor_shape: CURSOR_UNDERLINE,
        },
        &shaper,
        &config,
        &ct,
        &graphemes,
    );
    let rebuilt = build_view_from_grid(&mut atlas_for_full, &rebuilt_params);

    let incremental_bg = flattened_bg_rects(&incremental);
    let rebuilt_bg = flattened_bg_rects(&rebuilt);
    let incremental_glyphs = flattened_glyphs(&incremental);
    let rebuilt_glyphs = flattened_glyphs(&rebuilt);
    let incremental_color_glyphs = flattened_color_glyphs(&incremental);
    let rebuilt_color_glyphs = flattened_color_glyphs(&rebuilt);

    assert_rect_lists_match(&incremental_bg, &rebuilt_bg);
    assert_rect_lists_match(&incremental.cursor_rects, &rebuilt.cursor_rects);
    assert_relative_glyph_lists_match(&incremental_glyphs, &rebuilt_glyphs);
    assert_relative_glyph_lists_match(&incremental_color_glyphs, &rebuilt_color_glyphs);
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
        TestFrame {
            cells: &initial_cells,
            cols: 4,
            rows: 2,
            cursor_line: -1,
            cursor_col: 0,
            cursor_shape: CURSOR_HIDDEN,
        },
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
        TestFrame {
            cells: &updated_cells,
            cols: 4,
            rows: 2,
            cursor_line: -1,
            cursor_col: 0,
            cursor_shape: CURSOR_HIDDEN,
        },
        &shaper,
        &config,
        &ct,
        &graphemes,
    );

    update_view_from_grid(&mut view, &[true, false], 0, &updated_params, &mut atlas);

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
fn scroll_shift_rotates_row_cache_and_rebuilds_only_exposed_rows() {
    let config = test_config();
    let shaper = test_shaper(&config);
    let ct = test_color_table(&config);
    let graphemes = HashMap::new();

    let initial_cells = vec![
        PackedCell::with_ch('A'),
        PackedCell::default(),
        PackedCell::default(),
        PackedCell::with_ch('B'),
        PackedCell::default(),
        PackedCell::default(),
        PackedCell::with_ch('C'),
        PackedCell::default(),
        PackedCell::default(),
    ];
    let initial_params = test_view_inputs(
        TestFrame {
            cells: &initial_cells,
            cols: 3,
            rows: 3,
            cursor_line: -1,
            cursor_col: 0,
            cursor_shape: CURSOR_HIDDEN,
        },
        &shaper,
        &config,
        &ct,
        &graphemes,
    );
    let mut atlas = test_atlas(&config, &shaper);
    let mut view = build_view_from_grid(&mut atlas, &initial_params);
    let original_epochs = view.row_epochs.clone();
    let original_hashes = view.row_hashes.clone();

    let scrolled_cells = vec![
        PackedCell::with_ch('B'),
        PackedCell::default(),
        PackedCell::default(),
        PackedCell::with_ch('C'),
        PackedCell::default(),
        PackedCell::default(),
        PackedCell::with_ch('D'),
        PackedCell::default(),
        PackedCell::default(),
    ];
    let scrolled_params = test_view_inputs(
        TestFrame {
            cells: &scrolled_cells,
            cols: 3,
            rows: 3,
            cursor_line: -1,
            cursor_col: 0,
            cursor_shape: CURSOR_HIDDEN,
        },
        &shaper,
        &config,
        &ct,
        &graphemes,
    );

    update_view_from_grid(
        &mut view,
        &[false, false, false],
        -1,
        &scrolled_params,
        &mut atlas,
    );

    assert_eq!(view.last_scroll_shift, -1);
    assert_eq!(view.row_hash(0), original_hashes[1]);
    assert_eq!(view.row_hash(1), original_hashes[2]);
    assert_eq!(view.row_epoch(0), original_epochs[1]);
    assert_eq!(view.row_epoch(1), original_epochs[2]);
    assert!(view.row_epoch(2) > original_epochs[0]);

    let glyphs = flattened_glyphs(&view);
    assert_eq!(glyphs.len(), 3);
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

/// Regression: when a row of box-drawing cells sits on a solid bg colour,
/// the merged bg strip used to be appended to `bg_rects` *after* the line
/// rects and paint right over them. The renderer must emit the strip (or
/// per-cell bg) BEFORE the box-drawing geometry so the line stays visible.
#[test]
fn box_drawing_lines_stay_above_colored_bg_strip() {
    let config = test_config();
    let shaper = test_shaper(&config);
    let mut atlas = test_atlas(&config, &shaper);
    let ct = test_color_table(&config);
    let graphemes = HashMap::new();

    let bg = PackedColor::rgb(64, 0, 0);
    let fg = PackedColor::rgb(255, 255, 255);
    let mut cells = grid_with_size(3, 1);
    cells[0] = styled_cell('\u{2500}', fg, bg, 0);
    cells[1] = styled_cell('\u{2500}', fg, bg, 0);
    cells[2] = styled_cell('\u{2500}', fg, bg, 0);

    let params = test_view_inputs(
        TestFrame {
            cells: &cells,
            cols: 3,
            rows: 1,
            cursor_line: -1,
            cursor_col: 0,
            cursor_shape: CURSOR_HIDDEN,
        },
        &shaper,
        &config,
        &ct,
        &graphemes,
    );
    let view = build_view_from_grid(&mut atlas, &params);

    let bg_rects: Vec<_> = (0..view.row_count())
        .flat_map(|r| view.row_bg_rects(r).iter().copied())
        .collect();

    let bg_color = ct.resolve_packed(bg);
    let fg_color = ct.resolve_packed(fg);

    // Every line rect (the fg-coloured short-height rects emitted by
    // box_drawing) must NOT have any later rect that fully overlaps it
    // and is bg-coloured — otherwise the renderer would paint over it.
    let line_indices: Vec<usize> = bg_rects
        .iter()
        .enumerate()
        .filter(|(_, r)| r.color == fg_color && r.h < atlas.cell_height)
        .map(|(i, _)| i)
        .collect();
    assert!(
        !line_indices.is_empty(),
        "expected box-drawing line rects to be emitted"
    );
    fn covers(outer: &Rect, inner: &Rect) -> bool {
        outer.x <= inner.x
            && outer.y <= inner.y
            && outer.x + outer.w >= inner.x + inner.w
            && outer.y + outer.h >= inner.y + inner.h
    }
    for &li in &line_indices {
        let line = bg_rects[li];
        for (j, later) in bg_rects.iter().enumerate().skip(li + 1) {
            if later.color == bg_color && covers(later, &line) {
                panic!(
                    "line rect at index {li} (x={} y={} w={} h={}) is covered by a later \
                     bg rect at index {j} (x={} y={} w={} h={}) — strip flush ordering bug",
                    line.x, line.y, line.w, line.h, later.x, later.y, later.w, later.h
                );
            }
        }
    }
}
