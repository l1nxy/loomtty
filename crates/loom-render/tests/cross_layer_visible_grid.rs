use std::collections::HashMap;

use loom_app::grid::ClientPaneGrid;
use loom_config::config::LoomConfig;
use loom_protocol::message::{
    CURSOR_BLOCK, FLAG_HIDDEN, FLAG_UNDERLINE, FLAG_WIDE_CHAR, FLAG_WIDE_CHAR_SPACER, FullPaneSync,
    GraphemeExtras, HyperlinkExtras, MODE_ALT_SCREEN, MODE_MOUSE_REPORT, PackedCell, PackedColor,
    PaneFrameMeta,
};
use loom_render::glyph_cache::{FontInitParams, GlyphCache};
use loom_render::shaper::TextShaper;
use loom_render::terminal::{ColorTable, PackedViewInputs, build_view_from_grid};

fn styled_cell(ch: char, fg: PackedColor, bg: PackedColor, flags: u16) -> PackedCell {
    let mut cell = PackedCell::with_ch(ch);
    cell.fg = fg;
    cell.bg = bg;
    cell.flags = flags.to_le_bytes();
    cell
}

#[test]
fn protocol_client_render_visible_grid_invariants_stay_aligned() {
    let mut grid = ClientPaneGrid::new(4, 2, 8);
    let scrollback_fg = PackedColor::rgb(0, 255, 255);
    let scrollback_bg = PackedColor::rgb(12, 12, 12);
    let wide_fg = PackedColor::rgb(255, 255, 0);
    let wide_bg = PackedColor::rgb(0, 0, 48);
    let hidden_fg = PackedColor::rgb(255, 0, 0);
    let hidden_bg = PackedColor::rgb(32, 0, 0);
    let plain_fg = PackedColor::rgb(255, 255, 255);
    let plain_bg = PackedColor::rgb(0, 0, 0);

    let scrollback_cell = styled_cell('ß', scrollback_fg, scrollback_bg, FLAG_UNDERLINE);
    let sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 7,
            generation: 99,
            cursor_line: 1,
            cursor_col: 2,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: MODE_ALT_SCREEN | MODE_MOUSE_REPORT,
            received_ack: 0,
            echo_ack: 0,
        },
        cols: 4,
        rows: 2,
        title: "cross-layer".into(),
        scrollback: vec![scrollback_cell; 4],
        scrollback_rows: 1,
        scrollback_replace: false,
        cells: vec![
            styled_cell('好', wide_fg, wide_bg, FLAG_WIDE_CHAR),
            styled_cell(' ', wide_fg, wide_bg, FLAG_WIDE_CHAR_SPACER),
            styled_cell('e', plain_fg, plain_bg, 0),
            PackedCell::default(),
            styled_cell('x', hidden_fg, hidden_bg, FLAG_HIDDEN),
            styled_cell('g', plain_fg, plain_bg, 0),
            styled_cell('o', plain_fg, plain_bg, 0),
            PackedCell::default(),
        ],
        grapheme_extras: GraphemeExtras(vec![(2, "\u{0301}".to_string())]),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: Some("/tmp/loom".into()),
    };

    grid.apply_full_sync_owned(&sync);
    grid.scroll_up(1);

    let visible = grid.visible_cells();
    assert_eq!(visible.len(), 8);
    assert_eq!(
        visible[0], scrollback_cell,
        "scrollback row should remain visible"
    );
    assert_eq!(
        visible[4].ch(),
        '好',
        "wide leading cell should remain visible"
    );
    assert_eq!(visible[4].flags_u16() & FLAG_WIDE_CHAR, FLAG_WIDE_CHAR);
    assert_eq!(
        visible[5].flags_u16() & FLAG_WIDE_CHAR_SPACER,
        FLAG_WIDE_CHAR_SPACER
    );
    assert_eq!(visible[6].ch(), 'e');
    assert_eq!(visible[6].fg, plain_fg);
    assert_eq!(visible[6].bg, plain_bg);
    assert_eq!(visible[6].flags_u16(), 0);
    assert_eq!(visible[7], PackedCell::default());
    assert_eq!(
        grid.viewport_top(),
        0,
        "scrollback viewport should start at the oldest visible row"
    );
    assert_eq!(grid.cursor_line, 1);
    assert_eq!(grid.cursor_col, 2);
    assert_eq!(grid.mode_flags, MODE_ALT_SCREEN | MODE_MOUSE_REPORT);
    assert_eq!(grid.cwd.as_deref(), Some("/tmp/loom"));
    // `GraphemeExtras` indexes are over the *combined* scrollback +
    // viewport buffer the server transmits, not the viewport alone.
    // With 4 cells of scrollback transmitted before the 8 viewport
    // cells, protocol index 2 lands on scrollback cell 2 (`ß`),
    // producing the combined grapheme `ß\u{0301}`. The earlier
    // assertion (`e\u{0301}`) was written against a wrong assumption
    // that indices were viewport-relative.
    assert_eq!(
        grid.grapheme_map.get(&2).map(String::as_str),
        Some("ß\u{0301}"),
        "protocol grapheme extras (combined-buffer index 2) should land on scrollback cell 2"
    );

    let config = LoomConfig::default();
    let shaper = TextShaper::new(&config.font.family);
    let mut atlas = GlyphCache::new(&FontInitParams {
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
    });
    let colors = ColorTable::new(&config);
    let render_graphemes: HashMap<u32, String> =
        [(6, "e\u{0301}".to_string())].into_iter().collect();
    let rendered = build_view_from_grid(
        &mut atlas,
        &PackedViewInputs {
            cells: &visible,
            cols: grid.cols,
            rows: grid.rows,
            cursor_line: 1,
            cursor_col: 2,
            cursor_shape: CURSOR_BLOCK,
            config: &config,
            shaper: &shaper,
            colors: &colors,
            grapheme_map: &render_graphemes,
        },
    );

    let wide_bg_color = colors.resolve_packed(wide_bg);
    let wide_bg_rect = rendered
        .bg_rects
        .iter()
        .find(|rect| rect.color == wide_bg_color)
        .expect("wide cell background should render");
    assert_eq!(wide_bg_rect.w, atlas.cell_width * 2.0);

    assert!(
        rendered
            .bg_rects
            .iter()
            .all(|rect| rect.color != colors.resolve_packed(hidden_fg)),
        "hidden cells should not leak decoration rects into rendering"
    );
    assert_eq!(rendered.cursor_rects.len(), 1);
    let cursor = rendered.cursor_rects[0];
    assert_eq!(cursor.x, atlas.cell_width * 2.0);
    assert_eq!(cursor.w, atlas.cell_width);
    assert!(
        !rendered.glyph_instances.is_empty() || !rendered.color_glyph_instances.is_empty(),
        "visible wide/grapheme cells should produce render glyphs"
    );
}
