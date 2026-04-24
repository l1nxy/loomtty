use super::*;

#[test]
fn row_rect_top_of_bar() {
    let bar = UiRect::new(0.0, 0.0, 200.0, 600.0);
    let bar_component = TabBarComponent {
        tabs: Vec::new(),
        hovered_tab: None,
        tab_height: 28.0,
        tab_gap: 0.0,
        position: TabBarPosition::Left,
    };
    let r0 = bar_component.row_rect(bar, 0);
    assert_eq!(r0, UiRect::new(0.0, 0.0, 200.0, 28.0));
    let r5 = bar_component.row_rect(bar, 5);
    assert_eq!(r5, UiRect::new(0.0, 140.0, 200.0, 28.0));
}

#[test]
fn row_rect_clips_to_bottom_edge() {
    // Bar height 50, tab_height 28: row 0 = 0..28, row 1 = 28..50
    // (clipped from 28..56 to 28..50 = 22 tall).
    let bar = UiRect::new(0.0, 0.0, 200.0, 50.0);
    let bar_component = TabBarComponent {
        tabs: Vec::new(),
        hovered_tab: None,
        tab_height: 28.0,
        tab_gap: 0.0,
        position: TabBarPosition::Left,
    };
    let r1 = bar_component.row_rect(bar, 1);
    assert_eq!(r1, UiRect::new(0.0, 28.0, 200.0, 22.0));
    // Row 2 is fully below the bar — height clamps to 0.
    let r2 = bar_component.row_rect(bar, 2);
    assert!(r2.is_empty());
}

#[test]
fn hit_uses_ciri_ui_layout_snapshot() {
    use crate::app::App;
    use ciri_config::config::CiriConfig;

    let app = App::new(CiriConfig::default(), "test-session");
    let cx = app.ui_context();
    let bar = UiRect::new(10.0, 20.0, 200.0, 80.0);
    let bar_component = TabBarComponent {
        tabs: vec![
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
        hovered_tab: None,
        tab_height: 28.0,
        tab_gap: 4.0,
        position: TabBarPosition::Left,
    };

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
    use crate::app::ui::chrome_rects;
    use crate::app::App;
    use ciri_config::config::{CiriConfig, StatusBarPosition};

    let mut cfg = CiriConfig::default();
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
    use ciri_config::config::{CiriConfig, StatusBarPosition};

    let mut cfg = CiriConfig::default();
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
    use ciri_config::config::CiriConfig;

    let mut cfg = CiriConfig::default();
    cfg.tabbar.position = TabBarPosition::Left;
    let app = App::new(cfg, "test-session");
    assert_eq!(app.pane_tab_scroll_max(), 0.0);

    let mut cfg_right = CiriConfig::default();
    cfg_right.tabbar.position = TabBarPosition::Right;
    let app_right = App::new(cfg_right, "test-session");
    assert_eq!(app_right.pane_tab_scroll_max(), 0.0);
}

#[test]
fn content_origin_x_reflects_tab_bar_position() {
    use crate::app::App;
    use ciri_config::config::CiriConfig;

    let mut cfg = CiriConfig::default();
    cfg.tabbar.position = TabBarPosition::Left;
    let tabbar_w = cfg.tabbar.width;
    let app = App::new(cfg, "test-session");
    assert_eq!(app.content_origin_x(), tabbar_w);
    assert_eq!(app.total_chrome_width(), tabbar_w);

    let mut cfg_right = CiriConfig::default();
    cfg_right.tabbar.position = TabBarPosition::Right;
    let app_right = App::new(cfg_right, "test-session");
    // Right: terminal starts at x=0 (tabs are on the right edge).
    assert_eq!(app_right.content_origin_x(), 0.0);
    assert_eq!(
        app_right.total_chrome_width(),
        app_right.core.config.tabbar.width
    );

    let app_int = App::new(CiriConfig::default(), "test-session");
    assert_eq!(app_int.content_origin_x(), 0.0);
    assert_eq!(app_int.total_chrome_width(), 0.0);
}
