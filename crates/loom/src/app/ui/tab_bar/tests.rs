use super::*;

/// Helper: build a TabBarComponent for tests with the new field set
/// — hides the "give me cell_w / truncated_labels / rect" boilerplate.
fn test_component(
    tabs: Vec<TabEntry>,
    rect: UiRect,
    tab_height: f32,
    tab_gap: f32,
    position: TabBarPosition,
) -> TabBarComponent {
    let truncated_labels = tabs.iter().map(|t| t.label.clone()).collect();
    TabBarComponent {
        tabs,
        tab_height,
        tab_gap,
        position,
        cell_w: 8.0,
        truncated_labels,
        rect,
    }
}

#[test]
fn row_rect_top_of_bar() {
    let bar = UiRect::new(0.0, 0.0, 200.0, 600.0);
    let bar_component = test_component(Vec::new(), bar, 28.0, 0.0, TabBarPosition::Left);
    let r0 = bar_component.row_rect(bar, 0);
    assert_eq!(r0, UiRect::new(0.0, 0.0, 200.0, 28.0));
    let r5 = bar_component.row_rect(bar, 5);
    assert_eq!(r5, UiRect::new(0.0, 140.0, 200.0, 28.0));
}

#[test]
fn separator_gap_snaps_to_even_parity_for_symmetric_hairline() {
    // tab_gap=6, BORDER_THIN=1: free space = 5 (odd). taffy's
    // `justify_center` would render the hairline at 3-above + 1-line +
    // 2-below — visibly off-centre. `separator_gap_h` snaps the gap
    // down by 1 so the per-side padding is equal in integer pixels.
    let bar = UiRect::new(0.0, 0.0, 200.0, 600.0);
    let bar_component = test_component(Vec::new(), bar, 28.0, 6.0, TabBarPosition::Left);
    let gap = bar_component.separator_gap_h();
    assert_eq!(gap, 5.0, "6/1 parity mismatch: gap should snap from 6 to 5");

    // Even-parity case: tab_gap=5, BORDER_THIN=1 → free=4, half=2 → no snap.
    let even = test_component(Vec::new(), bar, 28.0, 5.0, TabBarPosition::Left);
    assert_eq!(even.separator_gap_h(), 5.0);

    // Zero gap → zero (no separator at all).
    let zero = test_component(Vec::new(), bar, 28.0, 0.0, TabBarPosition::Left);
    assert_eq!(zero.separator_gap_h(), 0.0);
}

#[test]
fn row_rect_clips_to_bottom_edge() {
    // Bar height 50, tab_height 28: row 0 = 0..28, row 1 = 28..50
    // (clipped from 28..56 to 28..50 = 22 tall).
    let bar = UiRect::new(0.0, 0.0, 200.0, 50.0);
    let bar_component = test_component(Vec::new(), bar, 28.0, 0.0, TabBarPosition::Left);
    let r1 = bar_component.row_rect(bar, 1);
    assert_eq!(r1, UiRect::new(0.0, 28.0, 200.0, 22.0));
    // Row 2 is fully below the bar — height clamps to 0.
    let r2 = bar_component.row_rect(bar, 2);
    assert!(r2.is_empty());
}

#[test]
fn hit_uses_loom_ui_layout_snapshot() {
    use crate::app::App;
    use loom_config::config::LoomConfig;

    let app = App::new(LoomConfig::default(), "test-session");
    let cx = app.ui_context();
    let bar = UiRect::new(10.0, 20.0, 200.0, 80.0);
    let bar_component = test_component(
        vec![
            TabEntry {
                pane_id: 41,
                label: "one".into(),
                active: true,
            },
            TabEntry {
                pane_id: 42,
                label: "two".into(),
                active: false,
            },
        ],
        bar,
        28.0,
        4.0,
        TabBarPosition::Left,
    );

    assert_eq!(
        bar_component.hit(bar, 12.0, 22.0, &cx),
        Some(UiAction::FocusPaneTab(41))
    );
    assert_eq!(
        bar_component.hit(bar, 12.0, 54.0, &cx),
        Some(UiAction::FocusPaneTab(42))
    );
    assert_eq!(bar_component.hit(bar, 12.0, 50.0, &cx), None);
    assert_eq!(bar_component.hit(bar, 1.0, 1.0, &cx), None);
}

/// End-to-end: with `TabBarPosition::Left`, chrome rect composition must carve
/// out a strip of `config.tabbar.width` on the left, starting below the top bar
/// and ending above the hints bar.
#[test]
fn chrome_rects_place_left_tab_bar_between_top_and_hints() {
    use crate::app::App;
    use crate::app::ui::chrome_rects;
    use loom_config::config::{LoomConfig, StatusBarPosition};

    let mut cfg = LoomConfig::default();
    cfg.tabbar.position = TabBarPosition::Left;
    cfg.statusbar.position = StatusBarPosition::Top;
    let tabbar_w = cfg.tabbar.width;
    let app = App::new(cfg, "test-session");
    let cx = app.ui_context();
    let vw = cx.viewport_w;
    let vh = cx.viewport_h;

    let top_bar_layout = app.top_bar_layout(vw, vh, cx.cell_w, cx.cell_h, cx.ui_shaper);
    let hints_h = app.hints_bar_height();
    let rects = chrome_rects(&app, vw, vh, top_bar_layout.bar_height, hints_h);
    let side = rects.side_tab_bar.expect("left tab bar rect");

    assert!((side.x - 0.0).abs() < 0.01);
    assert!((side.y - top_bar_layout.bar_height).abs() < 0.01);
    assert!((side.w - tabbar_w).abs() < 0.01);
    assert!((side.h - (vh - top_bar_layout.bar_height - hints_h)).abs() < 0.01);

    let (hx, hy, hw, hh) = app.side_tab_bar_rect(vw, vh).unwrap();
    assert!((hx - side.x).abs() < 0.01);
    assert!((hy - side.y).abs() < 0.01);
    assert!((hw - side.w).abs() < 0.01);
    assert!((hh - side.h).abs() < 0.01);
}

/// Step 40 contract: `tab_bar.hit()` returns
/// `Some(UiAction::FocusPaneTab(pane_id))` for clicks inside any tab
/// row (active or inactive), and the `pane_tab_hit_id(pane_id)` value
/// derived from that pane_id is what `App::active_hit_id` captures
/// for `.active()` styling. This test pins down that contract using
/// the same fixture shape as `hit_uses_loom_ui_layout_snapshot`.
#[test]
fn hit_yields_pane_tab_hit_id_for_active_press_capture() {
    use crate::app::App;
    use loom_config::config::LoomConfig;

    let app = App::new(LoomConfig::default(), "test-session");
    let cx = app.ui_context();
    let bar = UiRect::new(10.0, 20.0, 200.0, 80.0);
    let bar_component = test_component(
        vec![
            TabEntry {
                pane_id: 41,
                label: "one".into(),
                active: true,
            },
            TabEntry {
                pane_id: 42,
                label: "two".into(),
                active: false,
            },
        ],
        bar,
        28.0,
        4.0,
        TabBarPosition::Left,
    );

    // Active row click → FocusPaneTab(41) → press captures pane_tab_hit_id(41).
    let hit_active = bar_component.hit(bar, 12.0, 22.0, &cx);
    assert_eq!(hit_active, Some(UiAction::FocusPaneTab(41)));
    if let Some(UiAction::FocusPaneTab(pane_id)) = hit_active {
        assert_eq!(super::pane_tab_hit_id(pane_id), 1_000_000 + 41);
    }

    // Inactive row click → FocusPaneTab(42) → press captures pane_tab_hit_id(42).
    let hit_inactive = bar_component.hit(bar, 12.0, 54.0, &cx);
    assert_eq!(hit_inactive, Some(UiAction::FocusPaneTab(42)));
    if let Some(UiAction::FocusPaneTab(pane_id)) = hit_inactive {
        assert_eq!(super::pane_tab_hit_id(pane_id), 1_000_000 + 42);
    }
}

/// Documents the intended click-routing when the top bar and side
/// bar visually abut. The top bar spans the full window width, so pixels at
/// `y < bar_height` belong to the top bar even over the side bar's x column.
/// The side bar owns pixels at `y >= bar_height` within its x column.
///
/// This test exists to pin down that boundary so a future reordering
/// of `dispatch_ui_click` that "fixes" the apparent overlap by
/// putting the side bar first doesn't silently change the visual
/// contract.
#[test]
fn top_bar_owns_pixels_above_side_bar_for_left_position() {
    use crate::app::App;
    use loom_config::config::{LoomConfig, StatusBarPosition};

    let mut cfg = LoomConfig::default();
    cfg.tabbar.position = TabBarPosition::Left;
    cfg.statusbar.position = StatusBarPosition::Top;
    let app = App::new(cfg, "test-session");
    let cx = app.ui_context();
    let bar_h = app
        .top_bar_layout(
            cx.viewport_w,
            cx.viewport_h,
            cx.cell_w,
            cx.cell_h,
            cx.ui_shaper,
        )
        .bar_height;

    // A click inside the side bar's x column but at y well above
    // bar_height is in *top bar* territory — `hit_test_top_bar`
    // returns true; the side bar's rect does not include this y.
    let mx = 50.0;
    let my_in_top_bar_strip = 5.0;
    assert!(my_in_top_bar_strip < bar_h);
    assert!(app.hit_test_top_bar(mx, my_in_top_bar_strip));
    let (_, by, _, bh) = app.side_tab_bar_rect(cx.viewport_w, cx.viewport_h).unwrap();
    assert!(
        my_in_top_bar_strip < by,
        "side bar must start at or below bar_height"
    );
    assert!(
        !(my_in_top_bar_strip >= by && my_in_top_bar_strip < by + bh),
        "side bar rect must not include top-bar y range",
    );

    // A click inside the side bar's x column at y > bar_height is
    // in *side bar* territory — top bar hit test returns false.
    let my_in_side_bar = by + 20.0;
    assert!(!app.hit_test_top_bar(mx, my_in_side_bar));
    assert!(my_in_side_bar >= by && my_in_side_bar < by + bh);
}

/// `pane_tab_scroll_max` is called every frame from
/// `render_snapshot_hash` and from the mouse-wheel handler, even in
/// side-bar mode. In that mode the top bar has no pane tabs, so the
/// horizontal scroll concept is meaningless — it must return 0 to
/// avoid accumulating stale scroll state when the user later flips
/// back to Integrated.
#[test]
fn pane_tab_scroll_max_is_zero_in_side_bar_mode() {
    use crate::app::App;
    use loom_config::config::LoomConfig;

    let mut cfg = LoomConfig::default();
    cfg.tabbar.position = TabBarPosition::Left;
    let app = App::new(cfg, "test-session");
    assert_eq!(app.pane_tab_scroll_max(), 0.0);

    let mut cfg_right = LoomConfig::default();
    cfg_right.tabbar.position = TabBarPosition::Right;
    let app_right = App::new(cfg_right, "test-session");
    assert_eq!(app_right.pane_tab_scroll_max(), 0.0);
}

/// Regression for the side-tab hover lag: `current_side_tab_hover`
/// must report which Left/Right strip tab the cursor is over. It is
/// folded into both `ui_scene_hash` (chrome cache key) and
/// `render_snapshot_hash` (damage-skip gate); before the fix it had no
/// side-bar counterpart at all, so a hover left both hashes unchanged,
/// `damage_skip` dropped the scheduled redraw, and the highlight only
/// landed on the next unrelated repaint (cursor blink ≈ a few hundred
/// ms later).
#[test]
fn current_side_tab_hover_tracks_cursor_over_side_strip() {
    use crate::app::App;
    use loom_config::config::{LoomConfig, StatusBarPosition};
    use loom_layout::column::ColumnWidth;
    use winit::dpi::PhysicalSize;

    // Top and Bottom drive different side-strip y-origins. Running both
    // also guards the rect lockstep: the production helper derives the
    // rect from `chrome_rects` + `top_bar_layout.bar_height`, while the
    // reference points below come from `side_tab_bar_rect`
    // (`status_bar_height`); if those two ever diverged, the sampled
    // rows would land in the wrong tab and fail here.
    for status in [StatusBarPosition::Top, StatusBarPosition::Bottom] {
        let label = match status {
            StatusBarPosition::Top => "top",
            StatusBarPosition::Bottom => "bottom",
        };

        let mut config = LoomConfig::default();
        config.window.width = 900.0;
        config.window.height = 700.0;
        config.statusbar.position = status;
        config.tabbar.position = TabBarPosition::Left;
        config.tabbar.width = 120.0;
        let mut app = App::new(config, "test-session");
        app.preview_resize(PhysicalSize::new(900, 700));
        app.core
            .workspaces
            .active_mut()
            .add_column_right(1, ColumnWidth::Proportion(0.5));
        app.core
            .workspaces
            .active_mut()
            .add_column_right(2, ColumnWidth::Proportion(0.5));

        // Tabs are emitted one per tile, in `pane_tab_entries` order,
        // which is exactly the row order `TabBarComponent::capture` paints.
        let entries = app.pane_tab_entries();
        assert!(
            entries.len() >= 2,
            "[{label}] fixture should yield at least two side tabs, got {}",
            entries.len()
        );
        let first_pane = entries[0].0;
        let second_pane = entries[1].0;

        let (vw, vh) = app.command_palette_viewport_size();
        let (x, y, w, _h) = app
            .side_tab_bar_rect(vw, vh)
            .expect("left side tab bar rect");
        let mid_x = x + w * 0.5;
        let tab_h = app.core.config.tabbar.tab_height;
        let tab_gap = app.core.config.tabbar.tab_gap;

        // Row 0 → first tab.
        app.last_mouse_pos = Some((mid_x, y + tab_h * 0.5));
        assert_eq!(
            app.current_side_tab_hover(),
            Some(first_pane),
            "[{label}] row 0"
        );

        // Row 1 (past the first tab plus the inter-tab gap) → second tab.
        app.last_mouse_pos = Some((mid_x, y + tab_h + tab_gap + tab_h * 0.5));
        assert_eq!(
            app.current_side_tab_hover(),
            Some(second_pane),
            "[{label}] row 1"
        );

        // Cursor over the terminal area (right of the strip) → no hover.
        app.last_mouse_pos = Some((x + w + 40.0, y + tab_h * 0.5));
        assert_eq!(app.current_side_tab_hover(), None, "[{label}] off-strip");

        // Integrated mode has no dedicated strip → always None.
        app.core.config.tabbar.position = TabBarPosition::Integrated;
        app.last_mouse_pos = Some((mid_x, y + tab_h * 0.5));
        assert_eq!(app.current_side_tab_hover(), None, "[{label}] integrated");
    }
}

#[test]
fn content_origin_x_reflects_tab_bar_position() {
    use crate::app::App;
    use loom_config::config::LoomConfig;

    let mut cfg = LoomConfig::default();
    cfg.tabbar.position = TabBarPosition::Left;
    let tabbar_w = cfg.tabbar.width;
    let app = App::new(cfg, "test-session");
    assert_eq!(app.content_origin_x(), tabbar_w);
    assert_eq!(app.total_chrome_width(), tabbar_w);

    let mut cfg_right = LoomConfig::default();
    cfg_right.tabbar.position = TabBarPosition::Right;
    let app_right = App::new(cfg_right, "test-session");
    // Right: terminal starts at x=0 (tabs are on the right edge).
    assert_eq!(app_right.content_origin_x(), 0.0);
    assert_eq!(
        app_right.total_chrome_width(),
        app_right.core.config.tabbar.width
    );

    let app_int = App::new(LoomConfig::default(), "test-session");
    assert_eq!(app_int.content_origin_x(), 0.0);
    assert_eq!(app_int.total_chrome_width(), 0.0);
}
