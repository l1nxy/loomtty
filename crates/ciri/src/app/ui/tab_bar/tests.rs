use super::*;

#[test]
fn row_rect_top_of_bar() {
    let bar = UiRect::new(0.0, 0.0, 200.0, 600.0);
    let bar_component = TabBarComponent {
        tabs: Vec::new(),
        hovered_tab: None,
        bar_width: 200.0,
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
        bar_width: 200.0,
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

/// End-to-end: with `TabBarPosition::Left`, `Border::layout` must
/// carve out a strip of `config.tabbar.width` on the left, starting
/// below the top bar and ending above the hints bar. Cross-checks
/// the `side_tab_bar_rect` helper against the actual `Border`
/// arithmetic.
#[test]
fn border_places_left_tab_bar_between_top_and_hints() {
    use crate::app::App;
    use crate::app::ui::HintsBarComponent;
    use crate::app::ui::TopBarComponent;
    use crate::app::ui::layout::Border;
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
    let top_bar = TopBarComponent::capture(&app, top_bar_layout, &cx);
    let hints_bar = HintsBarComponent::capture(&app, &cx);
    let tab_bar = TabBarComponent::capture(&app, &cx);

    let chrome = Border::new()
        .top(top_bar)
        .bottom(hints_bar)
        .left(tab_bar);
    let slots = chrome.layout(UiRect::new(0.0, 0.0, vw, vh), &cx);

    // Left slot: x=0, y=top_bar_h, w=tabbar_w, h = vh - chrome_h
    assert!((slots.left.x - 0.0).abs() < 0.01);
    assert!((slots.left.y - top_bar_layout.bar_height).abs() < 0.01);
    assert!((slots.left.w - tabbar_w).abs() < 0.01);
    let expected_left_h = vh - app.total_chrome_height();
    assert!((slots.left.h - expected_left_h).abs() < 0.01);

    // The helper must match exactly — hit-test uses it, paint uses
    // Border::layout, they must agree to avoid dead zones.
    let (hx, hy, hw, hh) = app.side_tab_bar_rect(vw, vh).unwrap();
    assert!((hx - slots.left.x).abs() < 0.01);
    assert!((hy - slots.left.y).abs() < 0.01);
    assert!((hw - slots.left.w).abs() < 0.01);
    assert!((hh - slots.left.h).abs() < 0.01);

    // Center (terminal) must be: x = tabbar_w, w = vw - tabbar_w.
    assert!((slots.center.x - tabbar_w).abs() < 0.01);
    assert!((slots.center.w - (vw - tabbar_w)).abs() < 0.01);
}

/// Documents the intended click-routing when the top bar and side
/// bar *visually* abut. `Border` resolves `top` before `left`, so
/// pixels at `y < bar_height` are painted by the top bar across the
/// full window width, even over the side bar's x column. Clicks in
/// that strip therefore belong to the top bar. The side bar owns
/// pixels at `y >= bar_height` within its x column.
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
    let bar_h = app.top_bar_layout(cx.viewport_w, cx.viewport_h, cx.cell_w, cx.cell_h, cx.ui_shaper).bar_height;

    // A click inside the side bar's x column but at y well above
    // bar_height is in *top bar* territory — `hit_test_top_bar`
    // returns true; the side bar's rect does not include this y.
    let mx = 50.0;
    let my_in_top_bar_strip = 5.0;
    assert!(my_in_top_bar_strip < bar_h);
    assert!(app.hit_test_top_bar(mx, my_in_top_bar_strip));
    let (_, by, _, bh) = app.side_tab_bar_rect(cx.viewport_w, cx.viewport_h).unwrap();
    assert!(my_in_top_bar_strip < by, "side bar must start at or below bar_height");
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
    assert_eq!(app_right.total_chrome_width(), app_right.core.config.tabbar.width);

    let app_int = App::new(CiriConfig::default(), "test-session");
    assert_eq!(app_int.content_origin_x(), 0.0);
    assert_eq!(app_int.total_chrome_width(), 0.0);
}
