use super::*;
use crate::app::ui::layout::Border;
use crate::app::ui::HintsBarComponent;
use crate::app::App;
use ciri_config::config::{CiriConfig, StatusBarPosition};

fn make_cx(app: &App) -> UiContext<'_> {
    app.ui_context()
}

/// Verifies the load-bearing invariant that `PaneTabsElement` actually
/// receives a slot one cell **wider** than the tab visibility window
/// (`tabs_area_px`) when the row is laid out by `Linear`. This is what
/// produces the breathing-room gap between the right-most tab and the
/// workspace label.
///
/// The test exercises three independent properties:
/// 1. `Linear::layout`'s Fill arithmetic (the diff vs `tabs_area_px`).
/// 2. The Fill slot's *absolute width* against a synthetic bar rect of
///    known width — this guards against the case where both
///    `tabs_area_px` and the Fill slot would be wrong by the same
///    systematic amount, since this assertion is computed from the
///    bar rect width minus the *captured* fixed-zone widths only.
/// 3. The slot adjacency invariant: the four slots cover the bar
///    rect exactly, no overlap, no gap.
#[test]
fn pane_tabs_element_slot_is_one_cell_wider_than_tabs_area_px() {
    let app = App::new(CiriConfig::default(), "test-session");
    let cx = make_cx(&app);
    let layout = app.top_bar_layout(
        cx.viewport_w,
        cx.viewport_h,
        cx.cell_w,
        cx.cell_h,
        cx.ui_shaper,
    );
    let component = TopBarComponent::capture(&app, layout, &cx);

    // Use a *synthetic* bar rect of a hand-picked width so the
    // expected Fill slot can be cross-checked independently.
    let synthetic_w = 1234.0_f32;
    let bar_rect = UiRect::new(0.0, 0.0, synthetic_w, layout.bar_height);

    let row = component.build_row(&cx);
    let slots = row.layout(bar_rect, &cx);

    // Row is [SessionLabel(Fixed), PaneTabsElement(Fill),
    //        WorkspaceIndicator(Fixed), ModeIndicator(Fixed)].
    assert_eq!(slots.len(), 4, "row should have exactly four children");
    let session_slot = slots[0];
    let pane_tabs_slot = slots[1];
    let workspace_slot = slots[2];
    let mode_slot = slots[3];

    // (1) Slot adjacency — the four slots must tile the bar rect
    // exactly with no gap and no overlap.
    assert!((session_slot.x - bar_rect.x).abs() < 0.01);
    assert!(
        (pane_tabs_slot.x - session_slot.right()).abs() < 0.01,
        "PaneTabs slot must start where SessionLabel ends",
    );
    assert!(
        (workspace_slot.x - pane_tabs_slot.right()).abs() < 0.01,
        "Workspace slot must start where PaneTabs ends",
    );
    assert!(
        (mode_slot.x - workspace_slot.right()).abs() < 0.01,
        "Mode slot must start where Workspace ends",
    );
    assert!(
        (mode_slot.right() - bar_rect.right()).abs() < 0.01,
        "Mode slot must reach the bar's right edge",
    );

    // (2) Independent absolute-width check on the Fill slot. The
    // expected width is computed against the synthetic bar rect
    // width — independent of the `tabs_area_px` formula in
    // `top_bar_layout`. If `Linear::layout`'s Fill computation drifted
    // (e.g. allocated zero or twice the budget), this would fail
    // even if `tabs_area_px` had drifted by the same amount.
    let expected_fill_w = synthetic_w - session_slot.w - workspace_slot.w - mode_slot.w;
    assert!(
        (pane_tabs_slot.w - expected_fill_w).abs() < 0.01,
        "Fill slot width ({}) must equal bar_w - sum(fixed slots) ({expected_fill_w})",
        pane_tabs_slot.w,
    );

    // (3) The actual one-cell-gap invariant the test is named for.
    // Re-run the row layout against the *real* `viewport_w`-derived
    // bar rect — the same rect production code uses — and compare
    // the Fill slot to the `tabs_area_px` produced by `top_bar_layout`.
    // This cross-checks the *formula* in `top_bar_layout` against the
    // *Fill arithmetic* in `Linear`, which assertion (2) above has
    // already independently proved correct. A future refactor that
    // removes the `- cw` term from `top_bar_layout::tabs_area_px`
    // (or otherwise breaks the gap) would fail here. This is *not*
    // tautological: the right-hand side `tabs_area_px` and the
    // left-hand side `Fill slot width` come from two different code
    // paths (the captured layout formula vs. live `Linear::layout`).
    let cw = cx.cell_w;
    let real_bar_rect = component.bar_rect(&cx);
    let real_slots = component.build_row(&cx).layout(real_bar_rect, &cx);
    let real_pane_tabs_slot = real_slots[1];
    let gap_diff = real_pane_tabs_slot.w - layout.tabs_area_px;
    assert!(
        (gap_diff - cw).abs() < 0.01,
        "Fill slot ({}) must be exactly one cell ({cw}) wider than tabs_area_px ({}); got diff={gap_diff}. \
         A change to either the Linear Fill arithmetic OR the top_bar_layout formula would trip this.",
        real_pane_tabs_slot.w,
        layout.tabs_area_px,
    );
}

#[test]
fn hit_test_uses_ciri_ui_layout_snapshot() {
    let app = App::new(CiriConfig::default(), "test-session");
    let cx = make_cx(&app);
    let layout = app.top_bar_layout(
        cx.viewport_w,
        cx.viewport_h,
        cx.cell_w,
        cx.cell_h,
        cx.ui_shaper,
    );
    let component = TopBarComponent::capture(&app, layout, &cx);
    let rect = component.bar_rect(&cx);

    assert_eq!(
        component.hit_test(rect.x + 4.0, rect.y + 2.0, &cx),
        Some(UiTopBarHit::Session)
    );
    assert_eq!(
        component.hit_test(rect.right() - 2.0, rect.y + 2.0, &cx),
        Some(UiTopBarHit::Mode)
    );
    assert_eq!(
        component.hit_test(rect.x + layout.session_w + 2.0, rect.y + 2.0, &cx),
        Some(UiTopBarHit::Background)
    );
    assert_eq!(
        component.hit_test(rect.x + 4.0, rect.bottom() + 2.0, &cx),
        None
    );
}

/// End-to-end check that wiring TopBar + HintsBar through the
/// `Border` chrome tree reproduces the legacy bar y-positions for
/// `StatusBarPosition::Top`. Operates on `Border::layout` output so
/// no glyph cache or GPU init is needed.
#[test]
fn border_chrome_places_top_bar_and_hints_bar_at_legacy_y_for_top_position() {
    let mut cfg = CiriConfig::default();
    cfg.statusbar.position = StatusBarPosition::Top;
    let app = App::new(cfg, "test-session");
    let cx = make_cx(&app);
    let vw = cx.viewport_w;
    let vh = cx.viewport_h;
    let top_bar_layout = app.top_bar_layout(vw, vh, cx.cell_w, cx.cell_h, cx.ui_shaper);
    let top_bar = TopBarComponent::capture(&app, top_bar_layout, &cx);
    let hints_bar = HintsBarComponent::capture(&app, &cx);

    let chrome = Border::new().top(top_bar).bottom(hints_bar);
    let slots = chrome.layout(UiRect::new(0.0, 0.0, vw, vh), &cx);

    // Top slot = bar_y(Top) = 0, height = bar_height.
    assert!((slots.top.y - 0.0).abs() < 0.01);
    assert!((slots.top.h - top_bar_layout.bar_height).abs() < 0.01);
    // Bottom slot = vh - hints_height; same as legacy hints_bar_y(vh).
    let hints_h = app.hints_bar_height();
    assert!((slots.bottom.y - (vh - hints_h)).abs() < 0.01);
    assert!((slots.bottom.h - hints_h).abs() < 0.01);
    // Center is what's left for the terminal.
    assert!((slots.center.y - top_bar_layout.bar_height).abs() < 0.01);
    assert!((slots.center.h - (vh - top_bar_layout.bar_height - hints_h)).abs() < 0.01);
}

/// Same end-to-end check for `StatusBarPosition::Bottom`, where
/// HintsBar sits *above* TopBar in a vertical Linear inside the
/// bottom edge.
#[test]
fn border_chrome_places_top_bar_and_hints_bar_at_legacy_y_for_bottom_position() {
    let mut cfg = CiriConfig::default();
    cfg.statusbar.position = StatusBarPosition::Bottom;
    let app = App::new(cfg, "test-session");
    let cx = make_cx(&app);
    let vw = cx.viewport_w;
    let vh = cx.viewport_h;
    let top_bar_layout = app.top_bar_layout(vw, vh, cx.cell_w, cx.cell_h, cx.ui_shaper);
    let top_bar = TopBarComponent::capture(&app, top_bar_layout, &cx);
    let hints_bar = HintsBarComponent::capture(&app, &cx);
    let bar_h = top_bar_layout.bar_height;
    let hints_h = app.hints_bar_height();

    let inner = Linear::new(Axis::Vertical).push(hints_bar).push(top_bar);
    let chrome = Border::new().bottom(inner);
    let slots = chrome.layout(UiRect::new(0.0, 0.0, vw, vh), &cx);

    // Whole stacked chrome occupies the bottom (hints_h + bar_h) strip.
    let expected_bottom_y = vh - (hints_h + bar_h);
    assert!((slots.bottom.y - expected_bottom_y).abs() < 0.01);
    assert!((slots.bottom.h - (hints_h + bar_h)).abs() < 0.01);
    // No top slot — Top edge is empty in this config.
    assert!(slots.top.is_empty());
    // Center occupies the upper part of the screen.
    assert!((slots.center.y - 0.0).abs() < 0.01);
    assert!((slots.center.h - expected_bottom_y).abs() < 0.01);

    // And inside the bottom strip, the inner Linear places hints_bar
    // *above* top_bar — matching the legacy `hints_bar_y(vh) =
    // vh - status_bar_height - hints_bar_height` formula.
    // Re-derive the inner row from the bottom slot to verify.
    let inner_row = Linear::new(Axis::Vertical)
        .push(HintsBarComponent::capture(&app, &cx))
        .push(TopBarComponent::capture(&app, top_bar_layout, &cx));
    let inner_slots = inner_row.layout(slots.bottom, &cx);
    // slot[0] = HintsBar (top of bottom strip)
    assert!((inner_slots[0].y - expected_bottom_y).abs() < 0.01);
    assert!((inner_slots[0].h - hints_h).abs() < 0.01);
    // slot[1] = TopBar (bottom of bottom strip = legacy status_bar_y)
    assert!((inner_slots[1].y - (vh - bar_h)).abs() < 0.01);
    assert!((inner_slots[1].h - bar_h).abs() < 0.01);
}
