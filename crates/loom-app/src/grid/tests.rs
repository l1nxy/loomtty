use super::link::{detect_file_path, looks_like_file_path, split_path_line_col};
use super::*;

fn grid_with_line(text: &str) -> ClientPaneGrid {
    let cols = text.chars().count() as u16;
    let mut grid = ClientPaneGrid::new(cols, 1, 0);
    for (i, ch) in text.chars().enumerate() {
        grid.viewport[i] = PackedCell::with_ch(ch);
    }
    grid
}

#[test]
fn word_bounds_select_identifier() {
    let grid = grid_with_line("echo hello_world test");
    assert_eq!(grid.word_bounds_at(7, 0), Some((5, 15)));
}

#[test]
fn word_bounds_select_whitespace_run() {
    let grid = grid_with_line("a   b");
    assert_eq!(grid.word_bounds_at(2, 0), Some((1, 3)));
}

#[test]
fn word_bounds_select_symbol_run() {
    let grid = grid_with_line("foo::bar");
    assert_eq!(grid.word_bounds_at(4, 0), Some((3, 4)));
}

#[test]
fn link_at_detects_https_url() {
    let grid = grid_with_line("go https://example.com/docs now");
    assert_eq!(
        grid.link_at(8, 0),
        Some(LinkMatch {
            url: "https://example.com/docs".to_string(),
            start_col: 3,
            start_row: 0,
            end_col: 26,
            end_row: 0,
        })
    );
}

#[test]
fn link_at_trims_wrapping_punctuation() {
    let grid = grid_with_line("(https://example.com/path).");
    assert_eq!(
        grid.link_at(10, 0),
        Some(LinkMatch {
            url: "https://example.com/path".to_string(),
            start_col: 1,
            start_row: 0,
            end_col: 24,
            end_row: 0,
        })
    );
    assert_eq!(grid.link_at(0, 0), None);
    assert_eq!(grid.link_at(25, 0), None);
}

#[test]
fn link_at_normalizes_www_urls() {
    let grid = grid_with_line("visit www.example.com/test soon");
    assert_eq!(
        grid.link_at(10, 0),
        Some(LinkMatch {
            url: "https://www.example.com/test".to_string(),
            start_col: 6,
            start_row: 0,
            end_col: 25,
            end_row: 0,
        })
    );
}

// ─── Helper: build a grid with scrollback for scroll tests ─────
fn grid_with_scrollback() -> ClientPaneGrid {
    // 4 cols, 2 rows, 10 max_scrollback
    let mut grid = ClientPaneGrid::new(4, 2, 10);
    // Feed 3 full syncs to accumulate scrollback
    for round in 0..3u8 {
        let ch = (b'a' + round) as char;
        let sync = FullPaneSync {
            meta: PaneFrameMeta {
                pane_id: 1,
                generation: round as u64,
                cursor_line: 0,
                cursor_col: 0,
                cursor_shape: CURSOR_BLOCK,
                mode_flags: 0,
                received_ack: 0,
                echo_ack: 0,
            },
            cols: 4,
            rows: 2,
            title: String::new(),
            scrollback: vec![PackedCell::with_ch(ch); 4],
            scrollback_rows: 1,
            scrollback_replace: false,
            cells: vec![PackedCell::with_ch(ch.to_ascii_uppercase()); 8],
            grapheme_extras: loom_protocol::message::GraphemeExtras::new(),
            hyperlink_extras: loom_protocol::message::HyperlinkExtras::new(),
            cwd: None,
        };
        grid.apply_full_sync_owned(&sync);
    }
    // scrollback: ['a'x4, 'b'x4, 'c'x4], viewport: ['C'x4, 'C'x4]
    grid
}

// ─── row() ──────────────────────────────────────────────────────

#[test]
fn row_returns_scrollback_row() {
    let grid = grid_with_scrollback();
    let row = grid.row(0);
    assert_eq!(row.len(), 4);
    assert_eq!(row[0].ch(), 'a');
}

#[test]
fn row_returns_viewport_row() {
    let grid = grid_with_scrollback();
    let sb = grid.scrollback.len(); // 3
    let row = grid.row(sb); // first viewport row
    assert_eq!(row.len(), 4);
    assert_eq!(row[0].ch(), 'C');
}

#[test]
fn row_out_of_bounds_returns_empty() {
    let grid = grid_with_scrollback();
    let row = grid.row(9999);
    assert!(row.is_empty());
}

// ─── viewport_top() ─────────────────────────────────────────────

#[test]
fn viewport_top_no_scroll() {
    let grid = grid_with_scrollback();
    // scroll_offset=0 → top = scrollback.len()
    assert_eq!(grid.viewport_top(), grid.scrollback.len());
}

#[test]
fn viewport_top_scrolled_up() {
    let mut grid = grid_with_scrollback();
    grid.scroll_up(2);
    // scroll_offset=2 → top = scrollback.len() - 2
    assert_eq!(grid.viewport_top(), grid.scrollback.len() - 2);
}

#[test]
fn viewport_top_scrolled_max() {
    let mut grid = grid_with_scrollback();
    grid.scroll_up(100); // clamped to max_scroll_offset = scrollback.len()
    assert_eq!(grid.viewport_top(), 0);
}

// ─── visible_cells() ────────────────────────────────────────────

#[test]
fn visible_cells_no_scroll_returns_viewport_clone() {
    let grid = grid_with_scrollback();
    let cells = grid.visible_cells();
    assert_eq!(cells.len(), 8); // 2 rows * 4 cols
    assert_eq!(cells[0].ch(), 'C');
    assert_eq!(cells[7].ch(), 'C');
}

#[test]
fn visible_cells_scrolled_mixes_scrollback_and_viewport() {
    let mut grid = grid_with_scrollback();
    // scrollback: [row0='a', row1='b', row2='c'], viewport: [row3='C', row4='C']
    grid.scroll_up(1);
    let cells = grid.visible_cells();
    assert_eq!(cells.len(), 8);
    // top row should be last scrollback row ('c')
    assert_eq!(cells[0].ch(), 'c');
    // bottom row should be first viewport row ('C')
    assert_eq!(cells[4].ch(), 'C');
}

#[test]
fn visible_cells_scrolled_to_top() {
    let mut grid = grid_with_scrollback();
    grid.scroll_up(100); // clamp to max=3
    let cells = grid.visible_cells();
    // showing scrollback rows 0 and 1 ('a' and 'b')
    assert_eq!(cells[0].ch(), 'a');
    assert_eq!(cells[4].ch(), 'b');
}

// ─── scroll clamping ────────────────────────────────────────────

#[test]
fn scroll_up_returns_actual_scrolled() {
    let mut grid = grid_with_scrollback(); // 3 scrollback rows
    assert_eq!(grid.scroll_up(2), 2);
    assert_eq!(grid.scroll_up(5), 1); // only 1 more possible
    assert_eq!(grid.scroll_up(1), 0); // already at max
}

#[test]
fn scroll_down_returns_actual_scrolled() {
    let mut grid = grid_with_scrollback();
    grid.scroll_up(3);
    assert_eq!(grid.scroll_down(2), 2);
    assert_eq!(grid.scroll_down(5), 1); // only 1 left
    assert_eq!(grid.scroll_down(1), 0); // already at bottom
}

#[test]
fn scroll_updates_pending_scroll_delta_directionally() {
    let mut grid = grid_with_scrollback();
    assert_eq!(grid.pending_scroll_delta, 0);

    assert_eq!(grid.scroll_up(2), 2);
    assert_eq!(grid.pending_scroll_delta, 2);

    assert_eq!(grid.scroll_down(1), 1);
    assert_eq!(grid.pending_scroll_delta, 1);

    grid.clear_dirty();
    assert_eq!(grid.pending_scroll_delta, 0);
}

#[test]
fn scroll_up_no_scrollback_returns_zero() {
    let mut grid = ClientPaneGrid::new(4, 2, 0);
    assert_eq!(grid.scroll_up(10), 0);
}

#[test]
fn scroll_to_bottom_resets_offset() {
    let mut grid = grid_with_scrollback();
    grid.scroll_up(2);
    assert_eq!(grid.scroll_offset, 2);
    grid.scroll_to_bottom();
    assert_eq!(grid.scroll_offset, 0);
}

// ─── apply_full_sync edge cases ─────────────────────────────────

#[test]
fn full_sync_dimension_change_preserves_scrollback() {
    let mut grid = grid_with_scrollback();
    assert_eq!(grid.scrollback.len(), 3);

    // Resize from 4x2 to 6x3
    let sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 1,
            generation: 10,
            cursor_line: 1,
            cursor_col: 2,
            cursor_shape: CURSOR_BEAM,
            mode_flags: 0,
            received_ack: 0,
            echo_ack: 0,
        },
        cols: 6,
        rows: 3,
        title: "resized".into(),
        scrollback: vec![],
        scrollback_rows: 0,
        scrollback_replace: false,
        cells: vec![PackedCell::with_ch('X'); 18],
        grapheme_extras: loom_protocol::message::GraphemeExtras::new(),
        hyperlink_extras: loom_protocol::message::HyperlinkExtras::new(),
        cwd: None,
    };
    grid.apply_full_sync_owned(&sync);
    assert_eq!(grid.cols, 6);
    assert_eq!(grid.rows, 3);
    assert_eq!(grid.scrollback.len(), 3); // preserved across resize
    assert_eq!(grid.viewport.len(), 18);
    assert_eq!(grid.viewport[0].ch(), 'X');
    assert_eq!(grid.scroll_offset, 0);

    // Old scrollback rows are 4 cols wide; visible_cells() pads to 6
    grid.scroll_up(1);
    let cells = grid.visible_cells();
    assert_eq!(cells.len(), 18); // 3 rows × 6 cols
    // Last scrollback row ('c' × 4) padded to 6 cols
    assert_eq!(cells[0].ch(), 'c');
    assert_eq!(cells[3].ch(), 'c');
    assert_eq!(cells[4].ch(), ' '); // padded
    assert_eq!(cells[5].ch(), ' '); // padded
}

#[test]
fn full_sync_dimension_change_rebases_grapheme_indices() {
    let mut grid = ClientPaneGrid::new(4, 2, 10);
    let mut grapheme_extras = GraphemeExtras::new();
    grapheme_extras.push(8, "\u{0301}");
    let sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 1,
            generation: 10,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            received_ack: 0,
            echo_ack: 0,
        },
        cols: 4,
        rows: 2,
        title: String::new(),
        scrollback: vec![PackedCell::with_ch('s'); 4],
        scrollback_rows: 1,
        scrollback_replace: false,
        cells: vec![PackedCell::with_ch('A'); 8],
        grapheme_extras,
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: None,
    };
    grid.apply_full_sync_owned(&sync);
    assert_eq!(
        grid.grapheme_map.get(&8).map(String::as_str),
        Some("A\u{0301}")
    );

    let resize_sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 1,
            generation: 11,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            received_ack: 0,
            echo_ack: 0,
        },
        cols: 2,
        rows: 2,
        title: String::new(),
        scrollback: vec![],
        scrollback_rows: 0,
        scrollback_replace: false,
        cells: vec![PackedCell::with_ch('B'); 4],
        grapheme_extras: GraphemeExtras(vec![(2, "\u{0301}".to_string())]),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: None,
    };
    grid.apply_full_sync_owned(&resize_sync);

    assert_eq!(
        grid.grapheme_map.get(&6).map(String::as_str),
        Some("B\u{0301}")
    );
    assert!(!grid.grapheme_map.contains_key(&8));
}

#[test]
fn full_sync_short_cells_blanks_remainder() {
    let mut grid = ClientPaneGrid::new(4, 2, 10);
    let sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 1,
            generation: 1,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            received_ack: 0,
            echo_ack: 0,
        },
        cols: 4,
        rows: 2,
        title: String::new(),
        scrollback: vec![],
        scrollback_rows: 0,
        scrollback_replace: false,
        cells: vec![PackedCell::with_ch('A'); 3], // only 3 of 8 cells
        grapheme_extras: GraphemeExtras::new(),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: None,
    };
    grid.apply_full_sync_owned(&sync);
    assert_eq!(grid.viewport[0].ch(), 'A');
    assert_eq!(grid.viewport[2].ch(), 'A');
    assert_eq!(grid.viewport[3].ch(), ' '); // default blank
    assert_eq!(grid.viewport[7].ch(), ' ');
}

#[test]
fn full_sync_scrollback_trimmed_to_max() {
    let mut grid = ClientPaneGrid::new(2, 1, 3); // max 3 scrollback rows
    for i in 0u8..10 {
        let ch = (b'0' + i) as char;
        let sync = FullPaneSync {
            meta: PaneFrameMeta {
                pane_id: 1,
                generation: i as u64,
                cursor_line: 0,
                cursor_col: 0,
                cursor_shape: CURSOR_BLOCK,
                mode_flags: 0,
                received_ack: 0,
                echo_ack: 0,
            },
            cols: 2,
            rows: 1,
            title: String::new(),
            scrollback: vec![PackedCell::with_ch(ch); 2],
            scrollback_rows: 1,
            scrollback_replace: false,
            cells: vec![PackedCell::with_ch('.'); 2],
            grapheme_extras: loom_protocol::message::GraphemeExtras::new(),
            hyperlink_extras: loom_protocol::message::HyperlinkExtras::new(),
            cwd: None,
        };
        grid.apply_full_sync_owned(&sync);
    }
    // Should keep only the last 3 scrollback rows
    assert_eq!(grid.scrollback.len(), 3);
    // Oldest surviving = '7', then '8', then '9'
    assert_eq!(grid.scrollback[0].cells[0].ch(), '7');
    assert_eq!(grid.scrollback[2].cells[0].ch(), '9');
}

#[test]
fn full_sync_same_width_trim_rebases_grapheme_indices() {
    let mut grid = ClientPaneGrid::new(2, 1, 2);

    let initial_sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 1,
            generation: 1,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            received_ack: 0,
            echo_ack: 0,
        },
        cols: 2,
        rows: 1,
        title: String::new(),
        scrollback: vec![PackedCell::with_ch('X'), PackedCell::with_ch('Y')],
        scrollback_rows: 1,
        scrollback_replace: false,
        cells: vec![PackedCell::with_ch('A'), PackedCell::with_ch('B')],
        grapheme_extras: GraphemeExtras(vec![(0, "\u{0301}".to_string())]),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: None,
    };
    grid.apply_full_sync_owned(&initial_sync);
    assert_eq!(grid.scrollback.len(), 1);
    assert_eq!(
        grid.grapheme_map.get(&0).map(String::as_str),
        Some("X\u{0301}")
    );

    let overflow_sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 1,
            generation: 2,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            received_ack: 0,
            echo_ack: 0,
        },
        cols: 2,
        rows: 1,
        title: String::new(),
        scrollback: vec![PackedCell::with_ch('q'), PackedCell::with_ch('r')],
        scrollback_rows: 1,
        scrollback_replace: false,
        cells: vec![PackedCell::with_ch('m'), PackedCell::with_ch('n')],
        grapheme_extras: GraphemeExtras::new(),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: None,
    };
    grid.apply_full_sync_owned(&overflow_sync);
    assert_eq!(grid.scrollback.len(), 2);
    assert_eq!(
        grid.grapheme_map.get(&0).map(String::as_str),
        Some("X\u{0301}")
    );

    let trim_sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 1,
            generation: 3,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            received_ack: 0,
            echo_ack: 0,
        },
        cols: 2,
        rows: 1,
        title: String::new(),
        scrollback: vec![PackedCell::with_ch('m'), PackedCell::with_ch('n')],
        scrollback_rows: 1,
        scrollback_replace: false,
        cells: vec![PackedCell::with_ch('u'), PackedCell::with_ch('v')],
        grapheme_extras: GraphemeExtras::new(),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: None,
    };
    grid.apply_full_sync_owned(&trim_sync);

    assert_eq!(grid.scrollback.len(), 2);
    assert_eq!(grid.scrollback[0].cells[0].ch(), 'q');
    assert_eq!(grid.scrollback[1].cells[0].ch(), 'm');
    assert!(!grid.grapheme_map.contains_key(&0));
}

#[test]
fn full_sync_clamps_scroll_offset() {
    let mut grid = ClientPaneGrid::new(2, 1, 5);
    // Add some scrollback
    for i in 0..4u8 {
        let sync = FullPaneSync {
            meta: PaneFrameMeta {
                pane_id: 1,
                generation: i as u64,
                cursor_line: 0,
                cursor_col: 0,
                cursor_shape: CURSOR_BLOCK,
                mode_flags: 0,
                received_ack: 0,
                echo_ack: 0,
            },
            cols: 2,
            rows: 1,
            title: String::new(),
            scrollback: vec![PackedCell::with_ch('x'); 2],
            scrollback_rows: 1,
            scrollback_replace: false,
            cells: vec![PackedCell::default(); 2],
            grapheme_extras: loom_protocol::message::GraphemeExtras::new(),
            hyperlink_extras: loom_protocol::message::HyperlinkExtras::new(),
            cwd: None,
        };
        grid.apply_full_sync_owned(&sync);
    }
    assert_eq!(grid.scrollback.len(), 4);
    grid.scroll_up(4); // scroll to top
    assert_eq!(grid.scroll_offset, 4);

    // Resize resets scroll_offset to 0, but preserves scrollback
    let sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 1,
            generation: 10,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            received_ack: 0,
            echo_ack: 0,
        },
        cols: 3,
        rows: 1, // dimension change
        title: String::new(),
        scrollback: vec![],
        scrollback_rows: 0,
        scrollback_replace: false,
        cells: vec![PackedCell::default(); 3],
        grapheme_extras: loom_protocol::message::GraphemeExtras::new(),
        hyperlink_extras: loom_protocol::message::HyperlinkExtras::new(),
        cwd: None,
    };
    grid.apply_full_sync_owned(&sync);
    assert_eq!(grid.scroll_offset, 0); // reset by dimension change
    assert_eq!(grid.scrollback.len(), 4); // scrollback preserved
}

#[test]
fn full_sync_append_while_scrolled_preserves_viewport_top() {
    let mut grid = ClientPaneGrid::new(2, 1, 6);
    for ch in ['a', 'b', 'c', 'd'] {
        grid.apply_full_sync_owned(&FullPaneSync {
            meta: PaneFrameMeta {
                pane_id: 1,
                generation: 1,
                cursor_line: 0,
                cursor_col: 0,
                cursor_shape: CURSOR_BLOCK,
                mode_flags: 0,
                received_ack: 0,
                echo_ack: 0,
            },
            cols: 2,
            rows: 1,
            title: String::new(),
            scrollback: vec![PackedCell::with_ch(ch); 2],
            scrollback_rows: 1,
            scrollback_replace: false,
            cells: vec![PackedCell::default(); 2],
            grapheme_extras: GraphemeExtras::new(),
            hyperlink_extras: HyperlinkExtras::new(),
            cwd: None,
        });
    }
    grid.scroll_up(2);
    let top_before = grid.viewport_top();

    grid.apply_full_sync_owned(&FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 1,
            generation: 2,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            received_ack: 0,
            echo_ack: 0,
        },
        cols: 2,
        rows: 1,
        title: String::new(),
        scrollback: vec![PackedCell::with_ch('e'); 2],
        scrollback_rows: 1,
        scrollback_replace: false,
        cells: vec![PackedCell::default(); 2],
        grapheme_extras: GraphemeExtras::new(),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: None,
    });

    assert_eq!(grid.viewport_top(), top_before);
    assert_eq!(grid.scroll_offset, 3);
}

#[test]
fn scrollback_replace_clears_and_repopulates() {
    let mut grid = ClientPaneGrid::new(4, 2, 10);
    // Accumulate 3 scrollback rows: 'a', 'b', 'c'
    for ch in ['a', 'b', 'c'] {
        grid.apply_full_sync_owned(&FullPaneSync {
            meta: PaneFrameMeta {
                pane_id: 1,
                generation: 1,
                cursor_line: 0,
                cursor_col: 0,
                cursor_shape: CURSOR_BLOCK,
                mode_flags: 0,
                received_ack: 0,
                echo_ack: 0,
            },
            cols: 4,
            rows: 2,
            title: String::new(),
            scrollback: vec![PackedCell::with_ch(ch); 4],
            scrollback_rows: 1,
            scrollback_replace: false,
            cells: vec![PackedCell::default(); 8],
            grapheme_extras: loom_protocol::message::GraphemeExtras::new(),
            hyperlink_extras: loom_protocol::message::HyperlinkExtras::new(),
            cwd: None,
        });
    }
    assert_eq!(grid.scrollback.len(), 3);
    assert_eq!(grid.scrollback[0].cells[0].ch(), 'a');

    // Now apply a replace sync with 2 rows: 'X', 'Y'
    grid.apply_full_sync_owned(&FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 1,
            generation: 2,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            received_ack: 0,
            echo_ack: 0,
        },
        cols: 4,
        rows: 2,
        title: String::new(),
        scrollback: vec![
            PackedCell::with_ch('X'),
            PackedCell::with_ch('X'),
            PackedCell::with_ch('X'),
            PackedCell::with_ch('X'),
            PackedCell::with_ch('Y'),
            PackedCell::with_ch('Y'),
            PackedCell::with_ch('Y'),
            PackedCell::with_ch('Y'),
        ],
        scrollback_rows: 2,
        scrollback_replace: true,
        cells: vec![PackedCell::default(); 8],
        grapheme_extras: loom_protocol::message::GraphemeExtras::new(),
        hyperlink_extras: loom_protocol::message::HyperlinkExtras::new(),
        cwd: None,
    });
    // Old scrollback cleared, replaced with 2 new rows
    assert_eq!(grid.scrollback.len(), 2);
    assert_eq!(grid.scrollback[0].cells[0].ch(), 'X');
    assert_eq!(grid.scrollback[1].cells[0].ch(), 'Y');
}

#[test]
fn full_sync_applies_scrollback_grapheme_extras_without_polluting_viewport() {
    let mut grid = ClientPaneGrid::new(2, 1, 5);
    let mut grapheme_extras = GraphemeExtras::new();
    grapheme_extras.push(0, "\u{0301}");
    grapheme_extras.push(2, "\u{0302}");

    grid.apply_full_sync_owned(&FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 1,
            generation: 1,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            received_ack: 0,
            echo_ack: 0,
        },
        cols: 2,
        rows: 1,
        title: String::new(),
        scrollback: vec![PackedCell::with_ch('s'), PackedCell::with_ch('t')],
        scrollback_rows: 1,
        scrollback_replace: false,
        cells: vec![PackedCell::with_ch('V'), PackedCell::with_ch('W')],
        grapheme_extras,
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: None,
    });

    assert_eq!(
        grid.grapheme_map.get(&0).map(String::as_str),
        Some("s\u{0301}")
    );
    assert_eq!(
        grid.grapheme_map.get(&2).map(String::as_str),
        Some("V\u{0302}")
    );
    assert!(!grid.grapheme_map.contains_key(&1));
    assert!(!grid.grapheme_map.contains_key(&3));
}

#[test]
fn full_sync_dimension_change_keeps_scrollback_grapheme_extras() {
    let mut grid = ClientPaneGrid::new(4, 1, 5);
    let mut grapheme_extras = GraphemeExtras::new();
    grapheme_extras.push(0, "\u{0301}");
    grapheme_extras.push(2, "\u{0302}");

    grid.apply_full_sync_owned(&FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 1,
            generation: 1,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            received_ack: 0,
            echo_ack: 0,
        },
        cols: 2,
        rows: 1,
        title: String::new(),
        scrollback: vec![PackedCell::with_ch('s'), PackedCell::with_ch('t')],
        scrollback_rows: 1,
        scrollback_replace: false,
        cells: vec![PackedCell::with_ch('V'), PackedCell::with_ch('W')],
        grapheme_extras,
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: None,
    });

    assert_eq!(
        grid.grapheme_map.get(&0).map(String::as_str),
        Some("s\u{0301}")
    );
    assert_eq!(
        grid.grapheme_map.get(&2).map(String::as_str),
        Some("V\u{0302}")
    );
    assert!(!grid.grapheme_map.contains_key(&1));
    assert!(!grid.grapheme_map.contains_key(&3));
}

// ─── apply_delta edge cases ─────────────────────────────────────

#[test]
fn delta_apply_second_row() {
    let mut grid = ClientPaneGrid::new(5, 3, 0);
    let delta = CellDelta {
        pane_id: 1,
        generation: 1,
        cursor_line: 0,
        cursor_col: 0,
        cursor_shape: 0,
        mode_flags: 0,
        regions: vec![DamageRegion {
            line: 2,
            left: 1,
            right: 3,
            cells: vec![
                PackedCell::with_ch('X'),
                PackedCell::with_ch('Y'),
                PackedCell::with_ch('Z'),
            ],
        }],
    };
    grid.apply_delta(&delta);
    // Row 2 (offset = 2 * 5 = 10), cols 1-3
    assert_eq!(grid.viewport[10].ch(), ' '); // col 0 untouched
    assert_eq!(grid.viewport[11].ch(), 'X');
    assert_eq!(grid.viewport[12].ch(), 'Y');
    assert_eq!(grid.viewport[13].ch(), 'Z');
    assert_eq!(grid.viewport[14].ch(), ' '); // col 4 untouched
}

#[test]
fn delta_out_of_bounds_line_skipped() {
    let mut grid = ClientPaneGrid::new(4, 2, 0);
    let delta = CellDelta {
        pane_id: 1,
        generation: 1,
        cursor_line: 0,
        cursor_col: 0,
        cursor_shape: 0,
        mode_flags: 0,
        regions: vec![DamageRegion {
            line: 5,
            left: 0,
            right: 0, // line 5 doesn't exist in 2-row grid
            cells: vec![PackedCell::with_ch('X')],
        }],
    };
    grid.apply_delta(&delta);
    // Should not panic, viewport unchanged
    assert_eq!(grid.viewport[0].ch(), ' ');
}

#[test]
fn delta_region_clamped_to_cols() {
    let mut grid = ClientPaneGrid::new(3, 1, 0);
    let delta = CellDelta {
        pane_id: 1,
        generation: 1,
        cursor_line: 0,
        cursor_col: 0,
        cursor_shape: 0,
        mode_flags: 0,
        regions: vec![DamageRegion {
            line: 0,
            left: 1,
            right: 9, // right extends way past cols=3
            cells: vec![PackedCell::with_ch('X'); 9],
        }],
    };
    grid.apply_delta(&delta);
    // Only cols 1 and 2 should be written (clamped to cols=3)
    assert_eq!(grid.viewport[0].ch(), ' ');
    assert_eq!(grid.viewport[1].ch(), 'X');
    assert_eq!(grid.viewport[2].ch(), 'X');
}

#[test]
fn delta_sets_mode_flags() {
    let mut grid = ClientPaneGrid::new(4, 1, 0);
    let delta = CellDelta {
        pane_id: 1,
        generation: 1,
        cursor_line: 0,
        cursor_col: 3,
        cursor_shape: CURSOR_BEAM,
        mode_flags: MODE_SHELL_INTEGRATION | MODE_ALT_SCREEN,
        regions: vec![],
    };
    grid.apply_delta(&delta);
    assert_eq!(grid.cursor_col, 3);
    assert_eq!(grid.cursor_shape, CURSOR_BEAM);
    assert!(grid.has_shell_integration);
    assert_eq!(grid.kitty_flags, 0);
}

// ─── text_in_range spanning scrollback + viewport ────────────────

#[test]
fn text_in_range_spans_scrollback_and_viewport() {
    let mut grid = ClientPaneGrid::new(3, 1, 10);
    // Add one scrollback row 'abc', viewport row 'XYZ'
    let sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 1,
            generation: 1,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            received_ack: 0,
            echo_ack: 0,
        },
        cols: 3,
        rows: 1,
        title: String::new(),
        scrollback: vec![
            PackedCell::with_ch('a'),
            PackedCell::with_ch('b'),
            PackedCell::with_ch('c'),
        ],
        scrollback_rows: 1,
        scrollback_replace: false,
        cells: vec![
            PackedCell::with_ch('X'),
            PackedCell::with_ch('Y'),
            PackedCell::with_ch('Z'),
        ],
        grapheme_extras: GraphemeExtras::new(),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: None,
    };
    grid.apply_full_sync_owned(&sync);
    // buffer_row 0 = scrollback 'abc', buffer_row 1 = viewport 'XYZ'
    let text = grid.text_in_range((0, 0), (2, 1));
    assert_eq!(text, "abc\nXYZ");
}

// ─── search spanning scrollback + viewport ──────────────────────

#[test]
fn search_finds_in_scrollback_and_viewport() {
    let mut grid = ClientPaneGrid::new(5, 1, 10);
    let sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 1,
            generation: 1,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            received_ack: 0,
            echo_ack: 0,
        },
        cols: 5,
        rows: 1,
        title: String::new(),
        scrollback: vec![
            PackedCell::with_ch('h'),
            PackedCell::with_ch('e'),
            PackedCell::with_ch('l'),
            PackedCell::with_ch('l'),
            PackedCell::with_ch('o'),
        ],
        scrollback_rows: 1,
        scrollback_replace: false,
        cells: vec![
            PackedCell::with_ch('h'),
            PackedCell::with_ch('e'),
            PackedCell::with_ch('l'),
            PackedCell::with_ch('l'),
            PackedCell::with_ch('o'),
        ],
        grapheme_extras: GraphemeExtras::new(),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: None,
    };
    grid.apply_full_sync_owned(&sync);
    let results = grid.search("hello");
    assert_eq!(results.len(), 2); // found in both scrollback row 0 and viewport row 1
    assert_eq!(results[0].0, 0); // scrollback row
    assert_eq!(results[1].0, 1); // viewport row
}

// ─── word_bounds_at / link_at bounds ────────────────────────────

#[test]
fn word_bounds_at_out_of_bounds_returns_none() {
    let grid = ClientPaneGrid::new(4, 2, 0);
    assert_eq!(grid.word_bounds_at(0, 999), None);
}

#[test]
fn link_at_out_of_bounds_returns_none() {
    let grid = ClientPaneGrid::new(4, 2, 0);
    assert_eq!(grid.link_at(0, 999), None);
}

// ─── file path detection ─────────────────────────────────────────

#[test]
fn link_at_detects_absolute_unix_path() {
    let grid = grid_with_line("error in /usr/src/main.rs found");
    let m = grid.link_at(15, 0).unwrap();
    assert_eq!(m.url, "/usr/src/main.rs");
}

#[test]
fn link_at_detects_relative_path() {
    let grid = grid_with_line("see ./src/lib.rs for details");
    let m = grid.link_at(6, 0).unwrap();
    assert_eq!(m.url, "./src/lib.rs");
}

#[test]
fn link_at_detects_parent_relative_path() {
    let grid = grid_with_line("check ../config.toml now");
    let m = grid.link_at(10, 0).unwrap();
    assert_eq!(m.url, "../config.toml");
}

#[test]
fn link_at_detects_bare_path_with_extension() {
    let grid = grid_with_line("error at src/main.rs:42:10 here");
    let m = grid.link_at(12, 0).unwrap();
    assert_eq!(m.url, "src/main.rs:42:10");
}

#[test]
fn link_at_detects_path_with_line_number() {
    let grid = grid_with_line("warning crates/foo/lib.rs:99 x");
    let m = grid.link_at(15, 0).unwrap();
    assert_eq!(m.url, "crates/foo/lib.rs:99");
}

#[test]
fn link_at_detects_home_path() {
    let grid = grid_with_line("edit ~/docs/notes.md please");
    let m = grid.link_at(10, 0).unwrap();
    assert_eq!(m.url, "~/docs/notes.md");
}

#[test]
fn link_at_detects_windows_path() {
    let grid = grid_with_line(r"open C:\Users\foo\bar.txt now");
    let m = grid.link_at(10, 0).unwrap();
    assert_eq!(m.url, r"C:\Users\foo\bar.txt");
}

#[test]
fn link_at_ignores_bare_word_without_separator() {
    let grid = grid_with_line("just a plain word here");
    assert_eq!(grid.link_at(8, 0), None);
}

#[test]
fn detect_file_path_unit_tests() {
    assert!(detect_file_path("/foo/bar.rs").is_some());
    assert!(detect_file_path("./foo.rs").is_some());
    assert!(detect_file_path("../foo.rs").is_some());
    assert!(detect_file_path("~/foo.rs").is_some());
    assert!(detect_file_path("src/main.rs").is_some());
    assert!(detect_file_path("src/main.rs:42").is_some());
    assert!(detect_file_path("src/main.rs:42:10").is_some());
    assert_eq!(detect_file_path("plainword"), None);
    assert_eq!(detect_file_path("no_slash_here"), None);
}

#[test]
fn split_path_line_col_cases() {
    assert_eq!(split_path_line_col("foo.rs"), ("foo.rs", ""));
    assert_eq!(split_path_line_col("foo.rs:42"), ("foo.rs", ":42"));
    assert_eq!(split_path_line_col("foo.rs:42:10"), ("foo.rs", ":42:10"));
    assert_eq!(split_path_line_col("C:\\foo.rs:42"), ("C:\\foo.rs", ":42"));
}

#[test]
fn word_bounds_on_viewport_row() {
    let mut grid = ClientPaneGrid::new(5, 1, 10);
    let sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 1,
            generation: 1,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            received_ack: 0,
            echo_ack: 0,
        },
        cols: 5,
        rows: 1,
        title: String::new(),
        scrollback: vec![PackedCell::with_ch('x'); 5],
        scrollback_rows: 1,
        scrollback_replace: false,
        cells: vec![
            PackedCell::with_ch('a'),
            PackedCell::with_ch('b'),
            PackedCell::with_ch(' '),
            PackedCell::with_ch('c'),
            PackedCell::with_ch('d'),
        ],
        grapheme_extras: GraphemeExtras::new(),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: None,
    };
    grid.apply_full_sync_owned(&sync);
    // buffer_row 1 = viewport row → "ab cd"
    assert_eq!(grid.word_bounds_at(0, 1), Some((0, 1))); // "ab"
    assert_eq!(grid.word_bounds_at(3, 1), Some((3, 4))); // "cd"
}

// ─── existing tests (unchanged) ─────────────────────────────────

#[test]
fn delta_apply_patches_viewport() {
    let mut grid = ClientPaneGrid::new(10, 2, 0);
    let delta = CellDelta {
        pane_id: 1,
        generation: 1,
        cursor_line: 0,
        cursor_col: 0,
        cursor_shape: CURSOR_BLOCK,
        mode_flags: 0,
        regions: vec![DamageRegion {
            line: 0,
            left: 2,
            right: 4,
            cells: vec![
                PackedCell::with_ch('A'),
                PackedCell::with_ch('B'),
                PackedCell::with_ch('C'),
            ],
        }],
    };
    grid.apply_delta(&delta);
    assert_eq!(grid.viewport[2].ch(), 'A');
    assert_eq!(grid.viewport[3].ch(), 'B');
    assert_eq!(grid.viewport[4].ch(), 'C');
}

#[test]
fn full_sync_populates_viewport_and_scrollback() {
    let mut grid = ClientPaneGrid::new(4, 2, 100);
    let sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 1,
            generation: 1,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            received_ack: 0,
            echo_ack: 0,
        },
        cols: 4,
        rows: 2,
        title: String::new(),
        scrollback: vec![PackedCell::with_ch('S'); 4],
        scrollback_rows: 1,
        scrollback_replace: false,
        cells: vec![
            PackedCell::with_ch('A'),
            PackedCell::with_ch('B'),
            PackedCell::with_ch('C'),
            PackedCell::with_ch('D'),
            PackedCell::with_ch('E'),
            PackedCell::with_ch('F'),
            PackedCell::with_ch('G'),
            PackedCell::with_ch('H'),
        ],
        grapheme_extras: GraphemeExtras::new(),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: None,
    };
    grid.apply_full_sync_owned(&sync);
    assert_eq!(grid.scrollback.len(), 1);
    assert_eq!(grid.viewport[0].ch(), 'A');
    assert_eq!(grid.viewport[7].ch(), 'H');
    assert_eq!(grid.total_lines(), 3); // 1 scrollback + 2 viewport
    assert_eq!(grid.max_scroll_offset(), 1);
}

// ─── reflow tests ──────────────────────────────────────────────────

/// Helper: make a PackedCell with a char and optional WRAPLINE flag.
fn cell_with(ch: char, wrap: bool) -> PackedCell {
    let mut c = PackedCell::with_ch(ch);
    if wrap {
        let f = c.flags_u16() | FLAG_WRAPLINE;
        c.flags = f.to_le_bytes();
    }
    c
}

#[test]
fn reflow_widen_joins_wrapped_rows() {
    // Start with 4-col grid, scrollback has a 8-char logical line wrapped
    // across 2 rows: "ABCD" (wrapped) + "EF  " (not wrapped)
    let mut grid = ClientPaneGrid::new(4, 1, 100);
    grid.scrollback.push_back(ScrollbackRow {
        cells: vec![
            PackedCell::with_ch('A'),
            PackedCell::with_ch('B'),
            PackedCell::with_ch('C'),
            cell_with('D', true), // last cell has WRAPLINE
        ],
        wrapped: true,
    });
    grid.scrollback.push_back(ScrollbackRow {
        cells: vec![
            PackedCell::with_ch('E'),
            PackedCell::with_ch('F'),
            PackedCell::default(),
            PackedCell::default(),
        ],
        wrapped: false,
    });

    // Resize to 8 cols via a sync
    let sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 1,
            generation: 1,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            received_ack: 0,
            echo_ack: 0,
        },
        cols: 8,
        rows: 1,
        title: String::new(),
        scrollback: vec![],
        scrollback_rows: 0,
        scrollback_replace: false,
        cells: vec![PackedCell::default(); 8],
        grapheme_extras: GraphemeExtras::new(),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: None,
    };
    grid.apply_full_sync_owned(&sync);

    // Should reflow to 1 row: "ABCDEF  " (not wrapped)
    assert_eq!(grid.scrollback.len(), 1);
    assert!(!grid.scrollback[0].wrapped);
    assert_eq!(grid.scrollback[0].cells[0].ch(), 'A');
    assert_eq!(grid.scrollback[0].cells[5].ch(), 'F');
}

#[test]
fn reflow_narrow_splits_long_line() {
    // Start with 6-col grid, scrollback has "ABCDEF" (not wrapped)
    let mut grid = ClientPaneGrid::new(6, 1, 100);
    grid.scrollback.push_back(ScrollbackRow {
        cells: vec![
            PackedCell::with_ch('A'),
            PackedCell::with_ch('B'),
            PackedCell::with_ch('C'),
            PackedCell::with_ch('D'),
            PackedCell::with_ch('E'),
            PackedCell::with_ch('F'),
        ],
        wrapped: false,
    });

    // Resize to 3 cols
    let sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 1,
            generation: 1,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            received_ack: 0,
            echo_ack: 0,
        },
        cols: 3,
        rows: 1,
        title: String::new(),
        scrollback: vec![],
        scrollback_rows: 0,
        scrollback_replace: false,
        cells: vec![PackedCell::default(); 3],
        grapheme_extras: GraphemeExtras::new(),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: None,
    };
    grid.apply_full_sync_owned(&sync);

    // Should reflow to 2 rows: "ABC" (wrapped) + "DEF" (not wrapped)
    assert_eq!(grid.scrollback.len(), 2);
    assert!(grid.scrollback[0].wrapped);
    assert!(!grid.scrollback[1].wrapped);
    assert_eq!(grid.scrollback[0].cells[0].ch(), 'A');
    assert_eq!(grid.scrollback[0].cells[2].ch(), 'C');
    assert_eq!(grid.scrollback[1].cells[0].ch(), 'D');
    assert_eq!(grid.scrollback[1].cells[2].ch(), 'F');
}

#[test]
fn reflow_preserves_unwrapped_lines() {
    // Two separate logical lines that don't wrap
    let mut grid = ClientPaneGrid::new(4, 1, 100);
    grid.scrollback.push_back(ScrollbackRow {
        cells: vec![
            PackedCell::with_ch('A'),
            PackedCell::with_ch('B'),
            PackedCell::default(),
            PackedCell::default(),
        ],
        wrapped: false,
    });
    grid.scrollback.push_back(ScrollbackRow {
        cells: vec![
            PackedCell::with_ch('X'),
            PackedCell::with_ch('Y'),
            PackedCell::default(),
            PackedCell::default(),
        ],
        wrapped: false,
    });

    // Resize to 8 cols — should stay as 2 separate rows (not joined)
    let sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 1,
            generation: 1,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            received_ack: 0,
            echo_ack: 0,
        },
        cols: 8,
        rows: 1,
        title: String::new(),
        scrollback: vec![],
        scrollback_rows: 0,
        scrollback_replace: false,
        cells: vec![PackedCell::default(); 8],
        grapheme_extras: GraphemeExtras::new(),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: None,
    };
    grid.apply_full_sync_owned(&sync);

    assert_eq!(grid.scrollback.len(), 2);
    assert_eq!(grid.scrollback[0].cells[0].ch(), 'A');
    assert_eq!(grid.scrollback[1].cells[0].ch(), 'X');
}

// ─── Phase 3: trailing punctuation / port preservation ──────────

#[test]
fn link_at_preserves_port_in_url() {
    let grid = grid_with_line("visit https://example.com:8080/path please");
    let m = grid.link_at(10, 0).unwrap();
    assert_eq!(m.url, "https://example.com:8080/path");
}

#[test]
fn link_at_preserves_trailing_colon_in_url() {
    let grid = grid_with_line("see https://example.com:8080");
    let m = grid.link_at(10, 0).unwrap();
    assert_eq!(m.url, "https://example.com:8080");
}

// ─── Phase 4: bracket-aware boundary detection ──────────────────

#[test]
fn link_at_strips_surrounding_parens() {
    let grid = grid_with_line("(https://example.com)");
    let m = grid.link_at(10, 0).unwrap();
    assert_eq!(m.url, "https://example.com");
}

#[test]
fn link_at_keeps_parens_in_wikipedia_url() {
    let grid = grid_with_line("https://en.wikipedia.org/wiki/Rust_(language)");
    let m = grid.link_at(10, 0).unwrap();
    assert_eq!(m.url, "https://en.wikipedia.org/wiki/Rust_(language)");
}

#[test]
fn link_at_strips_surrounding_brackets() {
    let grid = grid_with_line("[https://example.com]");
    let m = grid.link_at(10, 0).unwrap();
    assert_eq!(m.url, "https://example.com");
}

#[test]
fn link_at_handles_nested_parens() {
    let grid = grid_with_line("(https://example.com/wiki/A_(B))");
    let m = grid.link_at(10, 0).unwrap();
    assert_eq!(m.url, "https://example.com/wiki/A_(B)");
}

// ─── Phase 5: bare filename detection ───────────────────────────

#[test]
fn link_at_detects_cargo_toml() {
    let grid = grid_with_line("see Cargo.toml for details");
    let m = grid.link_at(6, 0).unwrap();
    assert_eq!(m.url, "Cargo.toml");
}

#[test]
fn link_at_detects_main_rs() {
    let grid = grid_with_line("error in main.rs");
    let m = grid.link_at(11, 0).unwrap();
    assert_eq!(m.url, "main.rs");
}

#[test]
fn link_at_does_not_detect_bare_word() {
    let grid = grid_with_line("hello world");
    assert_eq!(grid.link_at(2, 0), None);
}

#[test]
fn link_at_does_not_detect_ambiguous() {
    let grid = grid_with_line("version 2.0");
    assert_eq!(grid.link_at(9, 0), None);
}

// ─── OSC 8 hyperlink detection ─────────────────────────────────

#[test]
fn osc8_link_at_basic() {
    // 10 cols, 1 row, no scrollback
    let mut grid = ClientPaneGrid::new(10, 1, 0);
    // Fill viewport with "click here"
    let text = "click here";
    for (i, ch) in text.chars().enumerate() {
        grid.viewport[i] = PackedCell::with_ch(ch);
    }
    // Set up OSC 8: cells 6..9 ("here") have link_id=1
    grid.hyperlink_map = std::collections::HashMap::from([(1, "https://example.com".to_string())]);
    grid.hyperlink_cell_map = std::collections::HashMap::new();
    for col in 6u32..=9 {
        grid.hyperlink_cell_map.insert(col, 1);
    }

    // Click on col 7 (inside "here") → should return OSC 8 link
    let m = grid.link_at(7, 0).unwrap();
    assert_eq!(m.url, "https://example.com");
    assert_eq!(m.start_col, 6);
    assert_eq!(m.end_col, 9);

    // Click on col 3 (inside "ck ") → no OSC 8 link
    // (may or may not match heuristic, but definitely not OSC 8)
    let no_osc8 = grid.link_at(3, 0);
    // "click" is not a URL, so should be None
    assert_eq!(no_osc8, None);
}

#[test]
fn osc8_link_at_priority_over_heuristic() {
    // Create a grid where the text looks like a URL but OSC 8 points somewhere else
    let text = "https://heuristic.com";
    let cols = text.chars().count() as u16;
    let mut grid = ClientPaneGrid::new(cols, 1, 0);
    for (i, ch) in text.chars().enumerate() {
        grid.viewport[i] = PackedCell::with_ch(ch);
    }
    // OSC 8 says the whole span points to a DIFFERENT URL
    grid.hyperlink_map =
        std::collections::HashMap::from([(42, "https://osc8-wins.example.org".to_string())]);
    grid.hyperlink_cell_map = std::collections::HashMap::new();
    for col in 0..cols as u32 {
        grid.hyperlink_cell_map.insert(col, 42);
    }

    let m = grid.link_at(5, 0).unwrap();
    // OSC 8 should win over heuristic detection
    assert_eq!(m.url, "https://osc8-wins.example.org");
    assert_eq!(m.start_col, 0);
    assert_eq!(m.end_col, cols - 1);
}

#[test]
fn osc8_link_at_scrollback_returns_none() {
    // OSC 8 data is viewport-only; clicking in scrollback should fall through to heuristic
    let mut grid = ClientPaneGrid::new(10, 1, 10);
    // Add a scrollback row
    let sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 1,
            generation: 1,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            received_ack: 0,
            echo_ack: 0,
        },
        cols: 10,
        rows: 1,
        title: String::new(),
        scrollback: vec![PackedCell::with_ch('x'); 10],
        scrollback_rows: 1,
        scrollback_replace: false,
        cells: vec![PackedCell::with_ch('y'); 10],
        grapheme_extras: GraphemeExtras::new(),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: None,
    };
    grid.apply_full_sync_owned(&sync);

    // Even if we manually poke hyperlink_cell_map with index 0, buffer_row 0
    // is scrollback so osc8_link_at should return None
    grid.hyperlink_map = std::collections::HashMap::from([(1, "https://nope.com".to_string())]);
    grid.hyperlink_cell_map.insert(0, 1);

    // buffer_row 0 is scrollback → OSC 8 not applicable, and 'x' is not a URL
    assert_eq!(grid.link_at(0, 0), None);
}

// ─── Trailing colon edge cases ────────────────────────────────

#[test]
fn link_at_strips_trailing_colon_after_url() {
    // "See https://example.com/foo:" — trailing colon is punctuation, strip it
    let grid = grid_with_line("See https://example.com/foo:");
    let m = grid.link_at(10, 0).unwrap();
    assert_eq!(m.url, "https://example.com/foo");
}

#[test]
fn link_at_preserves_port_no_path() {
    // "https://example.com:3000" — colon is part of port, preserve
    let grid = grid_with_line("https://example.com:3000");
    let m = grid.link_at(5, 0).unwrap();
    assert_eq!(m.url, "https://example.com:3000");
}

#[test]
fn link_at_strips_trailing_period() {
    let grid = grid_with_line("Visit https://example.com.");
    let m = grid.link_at(10, 0).unwrap();
    assert_eq!(m.url, "https://example.com");
}

#[test]
fn link_at_strips_trailing_comma() {
    let grid = grid_with_line("https://example.com, then");
    let m = grid.link_at(5, 0).unwrap();
    assert_eq!(m.url, "https://example.com");
}

#[test]
fn link_at_strips_trailing_exclamation() {
    let grid = grid_with_line("Check https://example.com!");
    let m = grid.link_at(10, 0).unwrap();
    assert_eq!(m.url, "https://example.com");
}

#[test]
fn link_at_strips_trailing_semicolon() {
    let grid = grid_with_line("see https://example.com;");
    let m = grid.link_at(10, 0).unwrap();
    assert_eq!(m.url, "https://example.com");
}

// ─── Bracket edge cases ───────────────────────────────────────

#[test]
fn link_at_strips_surrounding_angle_brackets() {
    let grid = grid_with_line("<https://example.com>");
    let m = grid.link_at(10, 0).unwrap();
    assert_eq!(m.url, "https://example.com");
}

#[test]
fn link_at_strips_surrounding_quotes() {
    let grid = grid_with_line("\"https://example.com\"");
    let m = grid.link_at(10, 0).unwrap();
    assert_eq!(m.url, "https://example.com");
}

#[test]
fn link_at_unbalanced_closers_stripped() {
    // ")))" after URL — all excess closers stripped
    let grid = grid_with_line("https://example.com)))");
    let m = grid.link_at(5, 0).unwrap();
    assert_eq!(m.url, "https://example.com");
}

#[test]
fn link_at_balanced_brackets_in_url_preserved() {
    // URL contains balanced square brackets
    let grid = grid_with_line("https://example.com/api?ids[0]=1");
    let m = grid.link_at(5, 0).unwrap();
    assert_eq!(m.url, "https://example.com/api?ids[0]=1");
}

// ─── File path edge cases ─────────────────────────────────────

#[test]
fn link_at_detects_relative_dot_slash() {
    let grid = grid_with_line("edit ./src/main.rs");
    let m = grid.link_at(8, 0).unwrap();
    assert_eq!(m.url, "./src/main.rs");
}

#[test]
fn link_at_detects_parent_relative() {
    let grid = grid_with_line("in ../Cargo.toml");
    let m = grid.link_at(6, 0).unwrap();
    assert_eq!(m.url, "../Cargo.toml");
}

#[test]
fn link_at_detects_home_relative() {
    let grid = grid_with_line("at ~/projects/loom/src/main.rs");
    let m = grid.link_at(8, 0).unwrap();
    assert_eq!(m.url, "~/projects/loom/src/main.rs");
}

#[test]
fn link_at_detects_path_with_line_col() {
    let grid = grid_with_line("error at src/main.rs:42:10");
    let m = grid.link_at(12, 0).unwrap();
    assert_eq!(m.url, "src/main.rs:42:10");
}

#[test]
fn link_at_detects_path_with_line_only() {
    let grid = grid_with_line("src/lib.rs:99");
    let m = grid.link_at(5, 0).unwrap();
    assert_eq!(m.url, "src/lib.rs:99");
}

// ─── Bare filename edge cases ─────────────────────────────────

#[test]
fn link_at_detects_makefile() {
    let grid = grid_with_line("edit Makefile");
    let m = grid.link_at(7, 0).unwrap();
    assert_eq!(m.url, "Makefile");
}

#[test]
fn link_at_detects_dockerfile() {
    let grid = grid_with_line("see Dockerfile");
    let m = grid.link_at(7, 0).unwrap();
    assert_eq!(m.url, "Dockerfile");
}

#[test]
fn link_at_detects_gitignore() {
    let grid = grid_with_line("update .gitignore");
    let m = grid.link_at(10, 0).unwrap();
    assert_eq!(m.url, ".gitignore");
}

#[test]
fn link_at_detects_case_insensitive_makefile() {
    let grid = grid_with_line("edit makefile");
    let m = grid.link_at(7, 0).unwrap();
    assert_eq!(m.url, "makefile");
}

#[test]
fn link_at_rejects_numeric_stem_bare_file() {
    // "2.rs" looks like a bare filename but stem is all digits — reject
    let grid = grid_with_line("I have 2.rs errors");
    assert_eq!(grid.link_at(9, 0), None);
}

#[test]
fn link_at_rejects_version_number() {
    let grid = grid_with_line("version 3.14");
    assert_eq!(grid.link_at(10, 0), None);
}

#[test]
fn link_at_detects_readme() {
    let grid = grid_with_line("see README");
    let m = grid.link_at(6, 0).unwrap();
    assert_eq!(m.url, "README");
}

// ─── URL scheme edge cases ────────────────────────────────────

#[test]
fn link_at_detects_http_url() {
    let grid = grid_with_line("http://example.com");
    let m = grid.link_at(5, 0).unwrap();
    assert_eq!(m.url, "http://example.com");
}

#[test]
fn link_at_detects_www_url() {
    let grid = grid_with_line("visit www.example.com");
    let m = grid.link_at(10, 0).unwrap();
    assert_eq!(m.url, "https://www.example.com");
}

#[test]
fn link_at_rejects_bare_text() {
    let grid = grid_with_line("this is plain text");
    assert_eq!(grid.link_at(5, 0), None);
}

#[test]
fn link_at_url_with_query_and_fragment() {
    let grid = grid_with_line("https://example.com/path?q=1&r=2#section");
    let m = grid.link_at(5, 0).unwrap();
    assert_eq!(m.url, "https://example.com/path?q=1&r=2#section");
}

#[test]
fn link_at_url_with_path_slash_preserved() {
    let grid = grid_with_line("https://example.com/a/b/c");
    let m = grid.link_at(5, 0).unwrap();
    assert_eq!(m.url, "https://example.com/a/b/c");
}

// ─── Delta sync evicts overwritten cells only ─────────────────

#[test]
fn delta_sync_evicts_overwritten_hyperlink_cells() {
    use loom_protocol::message::{
        CURSOR_BLOCK, CellDelta, DamageRegion, FullPaneSync, GraphemeExtras, HyperlinkExtras,
        PaneFrameMeta,
    };
    let mut grid = ClientPaneGrid::new(10, 2, 0);

    // Full sync: cells 0..4 on row 0 have link_id=1
    let mut cells = vec![PackedCell::default(); 20];
    for i in 0..5 {
        cells[i] = PackedCell::with_ch('a');
    }
    let sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 1,
            generation: 1,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            received_ack: 0,
            echo_ack: 0,
        },
        cols: 10,
        rows: 2,
        title: String::new(),
        scrollback: vec![],
        scrollback_rows: 0,
        scrollback_replace: false,
        cells,
        grapheme_extras: GraphemeExtras::new(),
        hyperlink_extras: HyperlinkExtras {
            cell_links: (0..5).map(|i| (i as u32, 1u16)).collect(),
            link_map: vec![(1, "https://linked.example.com".to_string())],
        },
        cwd: None,
    };
    grid.apply_full_sync_owned(&sync);

    // Verify OSC 8 link works
    let m = grid.link_at(2, 0).unwrap();
    assert_eq!(m.url, "https://linked.example.com");

    // Delta: overwrite cells 0..2 on row 0 (overwrites first 3 hyperlinked cells)
    let delta = CellDelta {
        pane_id: 1,
        generation: 1,
        cursor_line: 0,
        cursor_col: 0,
        cursor_shape: CURSOR_BLOCK,
        mode_flags: 0,
        regions: vec![DamageRegion {
            line: 0,
            left: 0,
            right: 2,
            cells: vec![PackedCell::with_ch('z'); 3],
        }],
    };
    grid.apply_delta(&delta);

    // Cells 0..2 were overwritten — their hyperlink entries should be evicted
    assert_eq!(grid.link_at(1, 0), None); // was hyperlinked, now overwritten

    // Cells 3..4 were NOT overwritten — their hyperlink entries should survive
    let m = grid.link_at(3, 0).unwrap();
    assert_eq!(m.url, "https://linked.example.com");
    assert_eq!(m.start_col, 3);
    assert_eq!(m.end_col, 4);
}

// ─── OSC 8 multi-link on same row ─────────────────────────────

#[test]
fn osc8_multiple_links_on_same_row() {
    let text = "foo bar baz";
    let cols = text.chars().count() as u16;
    let mut grid = ClientPaneGrid::new(cols, 1, 0);
    for (i, ch) in text.chars().enumerate() {
        grid.viewport[i] = PackedCell::with_ch(ch);
    }
    // link_id=1 on "foo" (0..2), link_id=2 on "baz" (8..10)
    grid.hyperlink_map = std::collections::HashMap::from([
        (1, "https://link-a.com".to_string()),
        (2, "https://link-b.com".to_string()),
    ]);
    for col in 0..=2u32 {
        grid.hyperlink_cell_map.insert(col, 1);
    }
    for col in 8..=10u32 {
        grid.hyperlink_cell_map.insert(col, 2);
    }

    let m1 = grid.link_at(1, 0).unwrap();
    assert_eq!(m1.url, "https://link-a.com");
    assert_eq!(m1.start_col, 0);
    assert_eq!(m1.end_col, 2);

    let m2 = grid.link_at(9, 0).unwrap();
    assert_eq!(m2.url, "https://link-b.com");
    assert_eq!(m2.start_col, 8);
    assert_eq!(m2.end_col, 10);

    // "bar" has no link
    assert_eq!(grid.link_at(5, 0), None);
}

// ─── CJK / wide-character search ────────────────────────────────

/// Create a grid with wide (CJK) characters properly laid out:
/// each wide char occupies 2 columns (FLAG_WIDE_CHAR + FLAG_WIDE_CHAR_SPACER).
fn grid_with_wide_line(text: &str) -> ClientPaneGrid {
    use unicode_width::UnicodeWidthChar;
    // Calculate total columns needed
    let total_cols: usize = text.chars().map(|c| c.width().unwrap_or(1)).sum();
    let cols = total_cols as u16;
    let mut grid = ClientPaneGrid::new(cols, 1, 0);
    let mut col = 0usize;
    for ch in text.chars() {
        let w = ch.width().unwrap_or(1);
        let mut cell = PackedCell::with_ch(ch);
        if w == 2 {
            cell.flags = FLAG_WIDE_CHAR.to_le_bytes();
        }
        grid.viewport[col] = cell;
        if w == 2 {
            let mut spacer = PackedCell::default();
            spacer.flags = FLAG_WIDE_CHAR_SPACER.to_le_bytes();
            grid.viewport[col + 1] = spacer;
        }
        col += w;
    }
    grid
}

#[test]
fn search_finds_chinese_characters() {
    let grid = grid_with_wide_line("你好世界你好");
    let results = grid.search("你好");
    // "你好" appears at positions 0 and 8 (each CJK char = 2 cols)
    assert_eq!(results.len(), 2);
    assert_eq!(results[0], (0, 0, 3)); // cols 0..3 (2 wide chars × 2 cols)
    assert_eq!(results[1], (0, 8, 11)); // cols 8..11
}

#[test]
fn search_chinese_end_col_includes_wide_char_width() {
    // Verify end_col accounts for the full width of the last CJK character
    let grid = grid_with_wide_line("ab你cd");
    // 'a'=col0, 'b'=col1, '你'=col2+col3, 'c'=col4, 'd'=col5
    let results = grid.search("你");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], (0, 2, 3)); // col 2 (start) to col 3 (end, inclusive)
}

#[test]
fn search_mixed_ascii_and_chinese() {
    let grid = grid_with_wide_line("hello你好world");
    let results = grid.search("你好");
    assert_eq!(results.len(), 1);
    // 'h'=0, 'e'=1, 'l'=2, 'l'=3, 'o'=4, '你'=5+6, '好'=7+8, 'w'=9, ...
    assert_eq!(results[0], (0, 5, 8));
}

#[test]
fn search_chinese_case_insensitive_with_ascii() {
    let grid = grid_with_wide_line("Hello你好World");
    let results = grid.search("hello你好world");
    assert_eq!(results.len(), 1);
}

// ─── URL scanner: URLs glued to surrounding text ────────────────
//
// The scanner locates a scheme anywhere in the line and extends it by
// URL-character class, so text stuck to either side of the URL no
// longer prevents detection or leaks into the opened address.

/// Column of the first char of `needle` in `text` (chars, not bytes).
fn col_of(text: &str, needle: &str) -> u16 {
    let byte = text.find(needle).expect("needle present");
    text[..byte].chars().count() as u16
}

fn url_at(text: &str, needle: &str) -> Option<String> {
    let grid = grid_with_line(text);
    grid.link_at(col_of(text, needle), 0).map(|m| m.url)
}

#[test]
fn link_at_markdown_link_yields_only_the_url() {
    let text = "[docs](https://example.com/guide)";
    assert_eq!(
        url_at(text, "example"),
        Some("https://example.com/guide".into())
    );
    // Hovering the link *text* is not hovering the URL, and the token
    // must not fall through to the file-path heuristic either.
    assert_eq!(url_at(text, "docs"), None);
}

#[test]
fn link_at_markdown_autolink_splits_the_two_urls() {
    // `[https://a](https://a)` — the `]` has no `[` inside the URL, so it
    // ends the first one instead of being swallowed along with the second.
    let text = "[https://example.com](https://example.com)";
    let grid = grid_with_line(text);
    let first = grid.link_at(3, 0).unwrap();
    assert_eq!(first.url, "https://example.com");
    assert_eq!((first.start_col, first.end_col), (1, 19));
    let second = grid.link_at(25, 0).unwrap();
    assert_eq!(second.url, "https://example.com");
    assert_eq!((second.start_col, second.end_col), (22, 40));
    // The `](` between them belongs to neither.
    assert_eq!(grid.link_at(20, 0), None);
    assert_eq!(grid.link_at(21, 0), None);
}

#[test]
fn link_at_url_glued_to_cjk_prose() {
    assert_eq!(
        url_at("请访问https://example.com查看", "example"),
        Some("https://example.com".into())
    );
    assert_eq!(
        url_at("链接：https://example.com", "example"),
        Some("https://example.com".into())
    );
    assert_eq!(
        url_at("详见https://example.com/foo，谢谢", "example"),
        Some("https://example.com/foo".into())
    );
    // Hovering the prose in front of the URL is not a link (and must not
    // become a bogus "file path" containing `://`).
    assert_eq!(url_at("链接：https://example.com", "链接"), None);
}

#[test]
fn link_at_strips_fullwidth_and_typographic_punctuation() {
    for (text, want) in [
        ("https://example.com/path）", "https://example.com/path"),
        ("（https://example.com/path）", "https://example.com/path"),
        ("https://example.com/a。", "https://example.com/a"),
        ("https://example.com/a…", "https://example.com/a"),
        ("→https://example.com/a→", "https://example.com/a"),
        ("“https://example.com/a”", "https://example.com/a"),
    ] {
        assert_eq!(url_at(text, "example"), Some(want.into()), "{text}");
    }
}

#[test]
fn link_at_url_inside_markdown_emphasis_and_code() {
    assert_eq!(
        url_at("**https://example.com/a**", "example"),
        Some("https://example.com/a".into())
    );
    assert_eq!(
        url_at("`https://example.com/a`", "example"),
        Some("https://example.com/a".into())
    );
    assert_eq!(
        url_at("|https://example.com/a|", "example"),
        Some("https://example.com/a".into())
    );
    // `*` and `|` only terminate when the URL was introduced by them.
    assert_eq!(
        url_at("https://example.com/a|b", "example"),
        Some("https://example.com/a|b".into())
    );
}

#[test]
fn link_at_url_after_key_value_prefix() {
    assert_eq!(
        url_at("url=https://example.com/x", "example"),
        Some("https://example.com/x".into())
    );
    assert_eq!(
        url_at("see:https://example.com/a", "example"),
        Some("https://example.com/a".into())
    );
    assert_eq!(
        url_at("\"url\":\"https://example.com/\"", "example"),
        Some("https://example.com/".into())
    );
    assert_eq!(
        url_at("foo(https://example.com/foo)", "example"),
        Some("https://example.com/foo".into())
    );
}

#[test]
fn link_at_bracket_balance_is_tracked_while_extending() {
    assert_eq!(
        url_at("(https://example.com/wiki/Foo_(bar))", "example"),
        Some("https://example.com/wiki/Foo_(bar)".into())
    );
    assert_eq!(
        url_at("https://example.com/a)b", "example"),
        Some("https://example.com/a".into())
    );
    assert_eq!(
        url_at("http://[::1]:8080/x", "::1"),
        Some("http://[::1]:8080/x".into())
    );
    assert_eq!(
        url_at("{https://example.com/a}", "example"),
        Some("https://example.com/a".into())
    );
}

#[test]
fn link_at_quotes_terminate_unless_wrapped_by_other_quote() {
    assert_eq!(
        url_at("https://example.com/a\"b", "example"),
        Some("https://example.com/a".into())
    );
    assert_eq!(
        url_at("\"https://example.com/a?q='x'\"", "example"),
        Some("https://example.com/a?q='x'".into())
    );
    assert_eq!(
        url_at("'https://example.com/a',", "example"),
        Some("https://example.com/a".into())
    );
}

#[test]
fn link_at_url_scheme_is_case_insensitive() {
    assert_eq!(
        url_at("HTTPS://EXAMPLE.COM/x", "EXAMPLE"),
        Some("HTTPS://EXAMPLE.COM/x".into())
    );
    assert_eq!(
        url_at("WWW.EXAMPLE.COM/x", "EXAMPLE"),
        Some("https://WWW.EXAMPLE.COM/x".into())
    );
}

#[test]
fn link_at_www_needs_a_token_boundary() {
    assert_eq!(url_at("wwww.example.com", "example"), None);
    assert_eq!(url_at("foo.www.example.com", "example"), None);
}

#[test]
fn link_at_bare_scheme_is_not_a_link() {
    assert_eq!(url_at("https://", "https"), None);
    assert_eq!(url_at("https://.", "https"), None);
    assert_eq!(url_at("https://)", "https"), None);
}

#[test]
fn link_at_hovering_gap_between_words_is_not_a_link() {
    // Previously the token expansion from a space swallowed both
    // neighbours and offered "see main.rs" as a path.
    let grid = grid_with_line("see main.rs");
    assert_eq!(grid.link_at(3, 0), None);
    assert_eq!(grid.link_at(6, 0).map(|m| m.url), Some("main.rs".into()));
}

#[test]
fn link_at_token_with_scheme_is_never_a_file_path() {
    // `detect_file_path` used to accept anything with a `/` and an
    // extension-looking tail; that made unopenable pseudo-paths out of
    // URL-bearing tokens.
    assert_eq!(detect_file_path("链接：https://example.com"), None);
    assert_eq!(detect_file_path("foo://bar/baz.rs"), None);
}

// ─── URL scanner: soft-wrapped rows ─────────────────────────────

/// Build a `cols`-wide grid whose rows hold `lines` (each padded with
/// spaces to `cols`); rows listed in `wrapped` get `FLAG_WRAPLINE` on
/// their last cell, i.e. they soft-continue onto the next row.
fn grid_with_rows(cols: u16, lines: &[&str], wrapped: &[usize]) -> ClientPaneGrid {
    let mut grid = ClientPaneGrid::new(cols, lines.len() as u16, 0);
    for (r, line) in lines.iter().enumerate() {
        for (c, ch) in line.chars().enumerate() {
            grid.viewport[r * cols as usize + c] = PackedCell::with_ch(ch);
        }
        if wrapped.contains(&r) {
            let last = &mut grid.viewport[r * cols as usize + cols as usize - 1];
            last.flags = (last.flags_u16() | FLAG_WRAPLINE).to_le_bytes();
        }
    }
    grid
}

#[test]
fn link_at_joins_soft_wrapped_rows_into_one_url() {
    // 20 cols: "https://example.com/" fills row 0 exactly and wraps.
    let grid = grid_with_rows(20, &["https://example.com/", "very/long/file.rs ok"], &[0]);
    let want = "https://example.com/very/long/file.rs";
    // Hovering the first row…
    let m = grid.link_at(5, 0).unwrap();
    assert_eq!(m.url, want);
    assert_eq!((m.start_col, m.start_row), (0, 0));
    assert_eq!((m.end_col, m.end_row), (16, 1));
    // …and the continuation on the second row give the same span.
    assert_eq!(grid.link_at(3, 1), Some(m));
    // Past the URL on the second row: nothing.
    assert_eq!(grid.link_at(18, 1), None);
}

#[test]
fn link_at_does_not_join_hard_line_breaks() {
    // Same layout, but row 0 is not flagged as wrapped: the second row is
    // an unrelated line and must not be appended to the URL.
    let grid = grid_with_rows(20, &["https://example.com/", "very/long/file.rs ok"], &[]);
    assert_eq!(
        grid.link_at(5, 0).map(|m| m.url),
        Some("https://example.com/".into())
    );
    assert_eq!(
        grid.link_at(3, 1).map(|m| m.url),
        Some("very/long/file.rs".into()) // file-path fallback, this row only
    );
}

#[test]
fn link_at_wrapped_url_hovered_from_middle_row() {
    let grid = grid_with_rows(10, &["go https:/", "/example.c", "om/x now  "], &[0, 1]);
    let m = grid.link_at(4, 1).unwrap();
    assert_eq!(m.url, "https://example.com/x");
    assert_eq!((m.start_col, m.start_row), (3, 0));
    assert_eq!((m.end_col, m.end_row), (3, 2));
    // Hovering the trailing prose on the last row is outside the link.
    assert_eq!(grid.link_at(6, 2), None);
}

// ─── File-path scanner: paths glued to surrounding text ─────────

#[test]
fn link_at_path_inside_markdown_code_and_emphasis() {
    assert_eq!(url_at("`src/main.rs`", "main"), Some("src/main.rs".into()));
    assert_eq!(
        url_at("**src/main.rs**", "main"),
        Some("src/main.rs".into())
    );
    // Markdown link to a file: hovering the target yields just the path.
    let grid = grid_with_line("[src/main.rs](src/main.rs)");
    let m = grid.link_at(5, 0).unwrap();
    assert_eq!(m.url, "src/main.rs");
    assert_eq!((m.start_col, m.end_col), (1, 11));
    let m = grid.link_at(18, 0).unwrap();
    assert_eq!(m.url, "src/main.rs");
    assert_eq!((m.start_col, m.end_col), (14, 24));
    assert_eq!(
        url_at("[docs](docs/guide.md)", "guide"),
        Some("docs/guide.md".into())
    );
}

#[test]
fn link_at_path_glued_to_cjk_prose() {
    for (text, needle, want) in [
        ("请看src/main.rs文件", "main", "src/main.rs"),
        ("文件：src/main.rs", "main", "src/main.rs"),
        ("修改了src/main.rs，然后", "main", "src/main.rs"),
        ("src/main.rs。", "main", "src/main.rs"),
        ("（src/main.rs）", "main", "src/main.rs"),
        ("看C:\\Users\\x\\y.rs", "y.rs", "C:\\Users\\x\\y.rs"),
        ("see main.rs文件", "main", "main.rs"),
    ] {
        assert_eq!(url_at(text, needle), Some(want.into()), "{text}");
    }
}

#[test]
fn link_at_keeps_cjk_that_is_part_of_the_path() {
    for text in [
        "桌面/main.rs",
        "src/中文.md",
        "中文.md",
        "C:\\Users\\linxy\\桌面\\test.rs",
        "docs/说明/readme.md",
    ] {
        let grid = grid_with_line(text);
        let m = grid.link_at(0, 0).unwrap_or_else(|| panic!("{text}"));
        assert_eq!(m.url, text, "{text}");
    }
}

#[test]
fn link_at_msvc_and_tsc_location_suffix() {
    assert_eq!(
        url_at("Program.cs(12,5): error CS1002", "Program"),
        Some("Program.cs:12:5".into())
    );
    assert_eq!(
        url_at("src/app.ts(3,7): error TS2304", "app"),
        Some("src/app.ts:3:7".into())
    );
    assert_eq!(
        url_at("foo.cpp(9): warning", "foo"),
        Some("foo.cpp:9".into())
    );
    // A balanced pair that is not a location stays part of the name.
    assert_eq!(url_at("open foo (1).txt", "1"), None); // space breaks the token…
    assert_eq!(url_at("open foo(1).txt", "foo"), Some("foo(1).txt".into())); // …but this is one file
}

#[test]
fn link_at_path_after_key_value_and_shell_operators() {
    assert_eq!(
        url_at("--file=src/main.rs", "main"),
        Some("src/main.rs".into())
    );
    assert_eq!(
        url_at("path='src/main.rs'", "main"),
        Some("src/main.rs".into())
    );
    assert_eq!(
        url_at("src/main.rs|grep x", "main"),
        Some("src/main.rs".into())
    );
    assert_eq!(
        url_at("cat src/main.rs>out", "main"),
        Some("src/main.rs".into())
    );
    assert_eq!(url_at("cat src/main.rs>out", "out"), None);
}

#[test]
fn link_at_quoted_path_with_spaces() {
    let text = "\"C:\\Program Files\\x\\y.rs\"";
    let grid = grid_with_line(text);
    // Hovering the space inside the quotes still yields the whole path.
    let m = grid.link_at(11, 0).unwrap();
    assert_eq!(m.url, "C:\\Program Files\\x\\y.rs");
    assert_eq!((m.start_col, m.end_col), (1, 23));
    // The quotes themselves are not part of the link.
    assert_eq!(grid.link_at(0, 0), None);
    assert_eq!(grid.link_at(24, 0), None);

    assert_eq!(
        url_at("'/home/u/my dir/a.rs'", "a.rs"),
        Some("/home/u/my dir/a.rs".into())
    );
    // Prose in quotes is not a path, and does not stop the bare token
    // inside from being found.
    assert_eq!(
        url_at("echo \"hello src/main.rs world\"", "main"),
        Some("src/main.rs".into())
    );
    assert_eq!(url_at("echo \"hello src/main.rs world\"", "hello"), None);
}

#[test]
fn link_at_bare_filename_extension_coverage() {
    for name in [
        "go.mod",
        ".env",
        "main.cs",
        "foo.vue",
        "app.dart",
        "index.mjs",
        ".gitattributes",
    ] {
        assert_eq!(
            url_at(name, name.trim_start_matches('.')),
            Some(name.into()),
            "{name}"
        );
    }
    // Prose with a dot is still not a file.
    for text in [
        "e.g. it",
        "i.e. that",
        "9 a.m. sharp",
        "v1.2.3",
        "Ph.D student",
    ] {
        let grid = grid_with_line(text);
        for col in 0..text.chars().count() as u16 {
            assert_eq!(grid.link_at(col, 0), None, "{text} @ {col}");
        }
    }
}

#[test]
fn link_at_bracketed_path_variants() {
    assert_eq!(
        url_at("at src/main.rs:12:5)", "main"),
        Some("src/main.rs:12:5".into())
    );
    assert_eq!(
        url_at("(src/main.rs:12:5)", "main"),
        Some("src/main.rs:12:5".into())
    );
    assert_eq!(
        url_at("src/main.rs:12:5:", "main"),
        Some("src/main.rs:12:5".into())
    );
    assert_eq!(url_at("src/[id].tsx", "id"), Some("src/[id].tsx".into()));
    assert_eq!(
        url_at("include(src/main.rs)", "main"),
        Some("src/main.rs".into())
    );
    // Hovering an unmatched bracket itself is not a link.
    assert_eq!(url_at("[text](src/main.rs)", "]"), None);
}

#[test]
fn link_at_joins_soft_wrapped_rows_for_paths_too() {
    let grid = grid_with_rows(
        20,
        &[
            "error --> crates/loo",
            "m-app/src/grid/link.",
            "rs:12:5 here        ",
        ],
        &[0, 1],
    );
    let m = grid.link_at(12, 0).unwrap();
    assert_eq!(m.url, "crates/loom-app/src/grid/link.rs:12:5");
    assert_eq!((m.start_col, m.start_row), (10, 0));
    assert_eq!((m.end_col, m.end_row), (6, 2));
    assert_eq!(grid.link_at(3, 1), Some(m));
}

#[test]
fn looks_like_file_path_matches_detector() {
    assert!(looks_like_file_path("Cargo.toml"));
    assert!(looks_like_file_path("src/main.rs:12:5"));
    assert!(looks_like_file_path("C:\\x\\y.rs"));
    assert!(!looks_like_file_path("https://example.com/a.rs"));
    assert!(!looks_like_file_path("hello"));
}
