use super::*;
use crate::glyph_cache::{FontInitParams, GlyphCache};
use crate::rect::Rect;
use crate::shaper::TextShaper;
use ciri_config::config::CiriConfig;
use ciri_protocol::message::{
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

fn test_config() -> CiriConfig {
    CiriConfig::default()
}

fn test_shaper(config: &CiriConfig) -> TextShaper {
    TextShaper::new(&config.font.family)
}

fn test_atlas(config: &CiriConfig, shaper: &TextShaper) -> GlyphCache {
    GlyphCache::new(&FontInitParams {
        font_size_pt: config.font.size,
        dpi_scale: 1.0,
        family_name: &config.font.family,
        primary_font_path: shaper.primary_font_path(),
        emoji_font_path: shaper.emoji_font_path(),
        emoji_font_id: shaper.emoji_font_id(),
        cjk_font_path: shaper.cjk_font_path(),
        cjk_font_id: shaper.cjk_font_id(),
        render_config: &config.render,
    })
}

fn test_color_table(config: &CiriConfig) -> ColorTable {
    ColorTable::new(config)
}

fn test_view_inputs<'a>(
    frame: TestFrame<'a>,
    shaper: &'a TextShaper,
    config: &'a CiriConfig,
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
