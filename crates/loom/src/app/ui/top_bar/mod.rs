//! Top bar chrome — session label, pane tabs, workspace indicator, mode badge.
//!
//! # Structure
//!
//! `TopBarComponent` is the outer "panel" that owns:
//! - Global decorations (bar background, separator line, leader/broadcast strip)
//! - A row of four explicit slots that lay out horizontally:
//!   - [`session_label::SessionLabel`] — fixed width, clicks open the session palette
//!   - [`pane_tabs::PaneTabsElement`] — fill, scrollable list of pane tabs
//!   - [`workspace::WorkspaceIndicator`] — fixed width (or zero), clicks cycle workspace
//!   - [`mode::ModeIndicator`] — fixed width, clicks toggle overview
//!
//! Each sub-element is told exactly what rect to draw into; none of them look
//! at `cx.viewport_w` to decide its own slot.
//!
//! # Compatibility
//!
//! `TopBarComponent` also exposes a full-bar click helper for call-sites that
//! just want "the top bar" without manually building a rect.

mod mode;
mod pane_tabs;
mod session_label;
pub(crate) mod usage;
mod workspace;

use loom_config::config::{StatusBarPosition, StatusBarSegmentKind};

use self::mode::ModeIndicator;
use self::pane_tabs::PaneTabsElement;
use self::session_label::SessionLabel;
use self::usage::UsageSegment;
use self::workspace::WorkspaceIndicator;
use super::text_layout;
use super::tokens;
use super::types::{UiAction, UiContext, UiRect, UiScene, UiTopBarHit, ui_hit_id};
use crate::app::loom_ui_adapter::paint_element_tree;
use crate::app::top_bar::{PaneTabLayout, SegmentMeasure, TopBarLayout};
use crate::app::usage::UsageSnapshot;
use crate::app::App;
use loom_ui::{Div, Styled, div};

pub(super) const HIT_SESSION: u64 = 1;
pub(super) const HIT_WORKSPACE: u64 = 2;
const HIT_MODE: u64 = 3;
const HIT_TAB_BASE: u64 = 1_000_000;

// The pane-tab hit-id base must sit far above every singleton chrome
// hit_id so `App::active_hit_id` and the chrome cache hash can't
// conflate a pane-tab press with a session/workspace/mode press. With
// `HIT_TAB_BASE = 1_000_000`, the gap leaves room for ~999_996 unique
// chrome hit_ids and trillions of pane ids before any overlap.
const _: () = {
    assert!(HIT_TAB_BASE > HIT_SESSION);
    assert!(HIT_TAB_BASE > HIT_WORKSPACE);
    assert!(HIT_TAB_BASE > HIT_MODE);
};

pub(super) fn pane_tab_hit_id(pane_id: u64) -> u64 {
    HIT_TAB_BASE + pane_id
}

fn top_bar_hit_from_id(hit_id: Option<u64>) -> UiTopBarHit {
    match hit_id {
        Some(HIT_SESSION) => UiTopBarHit::Session,
        Some(HIT_WORKSPACE) => UiTopBarHit::Workspace,
        Some(HIT_MODE) => UiTopBarHit::Mode,
        Some(id) if id >= HIT_TAB_BASE => UiTopBarHit::PaneTab(id - HIT_TAB_BASE),
        _ => UiTopBarHit::Background,
    }
}

pub(crate) struct TopBarComponent {
    pub layout: TopBarLayout,
    /// Configured segments in render order. Drives row_slots,
    /// build_row_children, and build_hit_tree.
    segments: Vec<StatusBarSegmentKind>,
    session_text: String,
    workspace_label: String,
    mode_label: String,
    mode_color: [f32; 4],
    pane_tabs: Vec<PaneTabLayout>,
    usage_snapshot: UsageSnapshot,
    /// Pre-computed label string shown by the usage segment. Cached so
    /// `row_slots` can measure without re-running formatting.
    usage_label: String,
    is_leader: bool,
    is_broadcast: bool,
    is_overview: bool,
    tab_scroll: f32,
    tab_scroll_max: f32,
    /// When false, tabs are rendered by a dedicated side tab bar
    /// (`TabBarComponent`) and the middle slot is left empty. Keeps
    /// session/workspace/mode positions stable regardless of tab placement.
    show_integrated_tabs: bool,
}

/// Per-frame slot rects in render order. The named accessors are kept
/// for backward-compatible test code; new callers iterate `ordered`.
#[derive(Debug, Clone)]
pub(super) struct TopBarRowSlots {
    pub session: UiRect,
    pub pane_tabs: UiRect,
    pub workspace: UiRect,
    pub mode: UiRect,
    pub usage: UiRect,
    /// Ordered list of (kind, rect) — render iteration source. Empty
    /// rects are filtered out so consumers can blindly iterate.
    pub ordered: Vec<(StatusBarSegmentKind, UiRect)>,
}

impl TopBarRowSlots {
    fn empty(rect: UiRect) -> Self {
        let zero = UiRect::new(rect.x, rect.y, 0.0, rect.h);
        Self {
            session: zero,
            pane_tabs: zero,
            workspace: zero,
            mode: zero,
            usage: zero,
            ordered: Vec::new(),
        }
    }

    fn slot_for(&self, kind: StatusBarSegmentKind) -> UiRect {
        match kind {
            StatusBarSegmentKind::SessionLabel => self.session,
            StatusBarSegmentKind::PaneTabs => self.pane_tabs,
            StatusBarSegmentKind::Workspace => self.workspace,
            StatusBarSegmentKind::Mode => self.mode,
            StatusBarSegmentKind::Usage => self.usage,
        }
    }
}

impl TopBarComponent {
    pub fn capture(app: &App, layout: TopBarLayout, cx: &UiContext<'_>) -> Self {
        let (mode_label, mode_color) = app.current_mode_label();
        let workspace_label = app.workspace_indicator_label();
        let segments = app.core.config.statusbar.effective_segments();
        let show_integrated_tabs = matches!(
            app.core.config.tabbar.position,
            loom_config::config::TabBarPosition::Integrated,
        ) && segments.contains(&StatusBarSegmentKind::PaneTabs);
        // Tab snapshot is only needed when we draw them inline. Saves
        // a Vec allocation + label cloning for side-bar configs.
        let pane_tabs = if show_integrated_tabs {
            app.pane_tab_layouts(cx.cell_w, layout.tabs_area_px, cx.ui_shaper)
        } else {
            Vec::new()
        };
        let usage_snapshot = app.usage_snapshot();
        let usage_label = usage::format_label(&usage_snapshot);
        Self {
            layout,
            segments,
            session_text: app.session_display_name(),
            workspace_label,
            mode_label,
            mode_color,
            pane_tabs,
            usage_snapshot,
            usage_label,
            is_leader: app.core.input.is_awaiting_action(),
            is_broadcast: app.core.broadcast_mode,
            is_overview: app.core.overview.active,
            tab_scroll: app.pane_tab_scroll,
            tab_scroll_max: app.pane_tab_scroll_max(),
            show_integrated_tabs,
        }
    }

    fn measure_segment(&self, kind: StatusBarSegmentKind, cx: &UiContext<'_>) -> SegmentMeasure {
        match kind {
            StatusBarSegmentKind::SessionLabel => SegmentMeasure::Fixed(
                crate::app::top_bar::segment_slot_width(text_layout::measure(cx, &self.session_text)),
            ),
            StatusBarSegmentKind::PaneTabs => SegmentMeasure::Fill,
            StatusBarSegmentKind::Workspace => {
                if self.workspace_label.is_empty() {
                    SegmentMeasure::Fixed(0.0)
                } else {
                    SegmentMeasure::Fixed(crate::app::top_bar::segment_slot_width(
                        text_layout::measure(cx, &self.workspace_label),
                    ))
                }
            }
            StatusBarSegmentKind::Mode => SegmentMeasure::Fixed(
                crate::app::top_bar::segment_slot_width(text_layout::measure(cx, &self.mode_label)),
            ),
            StatusBarSegmentKind::Usage => SegmentMeasure::Fixed(
                crate::app::top_bar::segment_slot_width(text_layout::measure(cx, &self.usage_label)),
            ),
        }
    }

    /// The rect this bar occupies, derived from `UiContext` + config.
    /// Kept as a helper so full-bar click/hit helpers can ask "where am I?"
    /// without duplicating the math.
    ///
    /// NB (pre-existing, inherited from full-bar paint/click helpers):
    /// `cx.viewport_w` here comes from `App::ui_context()` →
    /// `command_palette_viewport_size()`, which is the renderer surface size.
    /// During a resize event this can briefly differ from the `vw` passed to
    /// `App::build_ui`; production paint uses the explicit chrome rects built
    /// from that same `vw`, while this helper is used by hit-test paths that
    /// operate from the current UI context.
    pub(super) fn bar_rect(&self, cx: &UiContext<'_>) -> UiRect {
        UiRect::new(
            0.0,
            self.layout.bar_y,
            cx.viewport_w,
            self.layout.bar_height,
        )
    }

    /// Compute the inner row slots shared by paint and tests.
    pub(super) fn row_slots(&self, rect: UiRect, cx: &UiContext<'_>) -> TopBarRowSlots {
        if self.segments.is_empty() {
            return TopBarRowSlots::empty(rect);
        }
        // Each capsule slot = measured text width + capsule padding
        // (`segment_slot_width` in `crate::app::top_bar`). App-side
        // `top_bar_layout` measures from a `UiTextShaper` directly while
        // the row uses `text_layout::measure(cx, ...)`; both paths agree
        // on width, so `pane_tabs_area_px` and the rendered Fill slot
        // stay in lockstep.
        let measures: Vec<(StatusBarSegmentKind, SegmentMeasure)> = self
            .segments
            .iter()
            .map(|k| (*k, self.measure_segment(*k, cx)))
            .collect();
        let widths = crate::app::top_bar::cap_segments(rect.w, &measures);

        let mut slots = TopBarRowSlots::empty(rect);
        let mut x = rect.x;
        for ((kind, _), w) in measures.iter().zip(widths.iter().copied()) {
            let slot = UiRect::new(x, rect.y, w, rect.h);
            slots.ordered.push((*kind, slot));
            match kind {
                StatusBarSegmentKind::SessionLabel => slots.session = slot,
                StatusBarSegmentKind::PaneTabs => slots.pane_tabs = slot,
                StatusBarSegmentKind::Workspace => slots.workspace = slot,
                StatusBarSegmentKind::Mode => slots.mode = slot,
                StatusBarSegmentKind::Usage => slots.usage = slot,
            }
            x += w;
        }
        slots
    }

    /// Produce the absolute-positioned children for the row's segments.
    /// Iterates `slots.ordered` so config-driven segment order (and any
    /// future additions) flow through one dispatch site.
    fn build_row_children(&self, rect: UiRect, cx: &UiContext<'_>) -> Vec<Div> {
        let slots = self.row_slots(rect, cx);
        let accent = cx.theme.accent;
        let on_accent = cx.theme.on_accent;
        let surface_elevated = cx.theme.surface_elevated;

        let mut children: Vec<Div> = Vec::new();
        for (kind, slot) in &slots.ordered {
            match kind {
                StatusBarSegmentKind::SessionLabel => children.push(
                    SessionLabel {
                        text: &self.session_text,
                    }
                    .into_div(*slot, accent, on_accent),
                ),
                StatusBarSegmentKind::PaneTabs => {
                    if self.show_integrated_tabs {
                        children.extend(
                            PaneTabsElement {
                                tabs: &self.pane_tabs,
                                scroll: self.tab_scroll,
                                scroll_max: self.tab_scroll_max,
                            }
                            .into_children(*slot, cx),
                        );
                    }
                }
                StatusBarSegmentKind::Workspace => children.push(
                    WorkspaceIndicator {
                        label: &self.workspace_label,
                    }
                    .into_div(*slot, accent, surface_elevated),
                ),
                StatusBarSegmentKind::Mode => children.push(
                    ModeIndicator {
                        label: &self.mode_label,
                        color: self.mode_color,
                    }
                    .into_div(*slot),
                ),
                StatusBarSegmentKind::Usage => children.push(
                    UsageSegment {
                        snapshot: &self.usage_snapshot,
                    }
                    .into_div(*slot, accent, surface_elevated),
                ),
            }
        }

        children
    }

    fn build_hit_tree(&self, rect: UiRect, cx: &UiContext<'_>) -> Div {
        let slots = self.row_slots(rect, cx);
        let mut row = div()
            .absolute()
            .left(rect.x)
            .top(rect.y)
            .w(rect.w)
            .h(rect.h)
            .flex_row();

        for (kind, slot) in &slots.ordered {
            match kind {
                StatusBarSegmentKind::SessionLabel => {
                    if slot.w > 0.0 {
                        row = row.child(
                            div()
                                .w(slot.w)
                                .h(rect.h)
                                .hit_id(HIT_SESSION)
                                .cursor_pointer(),
                        );
                    }
                }
                StatusBarSegmentKind::PaneTabs => {
                    let mut tabs_slot = div().w(slot.w).h(rect.h);
                    if self.show_integrated_tabs {
                        let tabs_start_x = slot.x;
                        let tabs_end_x = slot.right().max(tabs_start_x);
                        let mut cursor_x = tabs_start_x;
                        for tab in &self.pane_tabs {
                            let visible_left = tab.x.max(tabs_start_x);
                            let visible_right = (tab.x + tab.w).min(tabs_end_x);
                            let visible_w = (visible_right - visible_left).max(0.0);
                            if visible_w <= 0.0 {
                                continue;
                            }
                            let gap = (visible_left - cursor_x).max(0.0);
                            if gap > 0.0 {
                                tabs_slot = tabs_slot.child(div().w(gap).h(rect.h));
                            }
                            tabs_slot = tabs_slot.child(
                                div()
                                    .w(visible_w)
                                    .h(rect.h)
                                    .hit_id(pane_tab_hit_id(tab.pane_id))
                                    .cursor_pointer(),
                            );
                            cursor_x = visible_right;
                        }
                    }
                    row = row.child(tabs_slot);
                }
                StatusBarSegmentKind::Workspace => {
                    if slot.w > 0.0 {
                        row = row.child(
                            div()
                                .w(slot.w)
                                .h(rect.h)
                                .hit_id(HIT_WORKSPACE)
                                .cursor_pointer(),
                        );
                    }
                }
                StatusBarSegmentKind::Mode => {
                    row = row.child(
                        div()
                            .w(slot.w)
                            .h(rect.h)
                            .hit_id(HIT_MODE)
                            .cursor_pointer(),
                    );
                }
                StatusBarSegmentKind::Usage => {
                    // Display-only — emit a sized but hit-id-less div so
                    // it occupies the slot without intercepting clicks.
                    if slot.w > 0.0 {
                        row = row.child(div().w(slot.w).h(rect.h));
                    }
                }
            }
        }

        // Suppress unused-method-warning while the Usage hit_test
        // routing is still display-only.
        let _ = slots.slot_for(StatusBarSegmentKind::Usage);

        div().w(cx.viewport_w).h(cx.viewport_h).child(row)
    }

    fn hit_test_in_rect(
        &self,
        rect: UiRect,
        mx: f32,
        my: f32,
        cx: &UiContext<'_>,
    ) -> Option<UiTopBarHit> {
        if !rect.contains(mx, my) {
            return None;
        }
        let root = self.build_hit_tree(rect, cx);
        Some(top_bar_hit_from_id(ui_hit_id(&root, cx, mx, my)))
    }

    pub(super) fn hit_test(&self, mx: f32, my: f32, cx: &UiContext<'_>) -> Option<UiTopBarHit> {
        let rect = self.bar_rect(cx);
        self.hit_test_in_rect(rect, mx, my, cx)
    }

    /// Inner chrome trees (bar bg + separator line + optional leader/
    /// broadcast/overview band). Returns a Vec of absolute-positioned
    /// children — no viewport wrapper, no layer override; caller
    /// composes them as siblings inside the unified TopBar tree.
    fn build_chrome_inner(&self, rect: UiRect, cx: &UiContext<'_>) -> Vec<Div> {
        let bar_bg = cx.theme.statusbar_bg;
        let dim = cx.theme.on_surface_muted;
        let accent = cx.theme.accent;
        let broadcast_color = cx.theme.broadcast;
        let sep_color = tokens::tint(dim, tokens::ALPHA_SEPARATOR);

        let sep_y = match cx.config.statusbar.position {
            StatusBarPosition::Top => rect.h - tokens::BORDER_THIN,
            StatusBarPosition::Bottom => 0.0,
        };
        let mut children = vec![
            div()
                .absolute()
                .left(rect.x)
                .top(rect.y)
                .w(rect.w)
                .h(rect.h)
                .bg(bar_bg)
                .child(
                    div()
                        .absolute()
                        .left(0.0)
                        .top(sep_y)
                        .w(rect.w)
                        .h(tokens::BORDER_THIN)
                        .bg(sep_color),
                ),
        ];

        if self.is_leader || self.is_broadcast || self.is_overview {
            let indicator_h = cx.cell_h * cx.config.statusbar.leader_indicator_ratio;
            let indicator_color = if self.is_broadcast {
                broadcast_color
            } else {
                accent
            };
            let band_y = match cx.config.statusbar.position {
                StatusBarPosition::Top => rect.bottom(),
                StatusBarPosition::Bottom => rect.y - indicator_h,
            };
            children.push(
                div()
                    .absolute()
                    .left(rect.x)
                    .top(band_y)
                    .w(rect.w)
                    .h(indicator_h)
                    .bg(indicator_color),
            );
        }

        children
    }
}

impl TopBarComponent {
    pub(crate) fn paint(&self, rect: UiRect, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        // Single unified TopBar tree: one viewport wrapper, every chrome
        // + row sub-piece as a flat absolute-positioned sibling. One
        // walker pass per frame instead of 5 (chrome + 4 sub-widget
        // paints). All children are themselves `.absolute()` with
        // viewport-relative coords, so the viewport root stays the only
        // positioning ancestor — no double offsets, no zero-sized
        // intermediate wrappers.
        let mut root = div().w(cx.viewport_w).h(cx.viewport_h);
        for chrome_child in self.build_chrome_inner(rect, cx) {
            root = root.child(chrome_child);
        }
        for row_child in self.build_row_children(rect, cx) {
            root = root.child(row_child);
        }
        paint_element_tree(&root, cx, scene);
    }

    fn hit(&self, rect: UiRect, mx: f32, my: f32, cx: &UiContext<'_>) -> Option<UiAction> {
        match self.hit_test_in_rect(rect, mx, my, cx)? {
            UiTopBarHit::Session => Some(UiAction::OpenSessionPalette),
            UiTopBarHit::Workspace => Some(UiAction::CycleWorkspace),
            UiTopBarHit::Mode => Some(UiAction::ToggleOverview),
            UiTopBarHit::PaneTab(id) => Some(UiAction::FocusPaneTab(id)),
            UiTopBarHit::Background => None,
        }
    }
    pub(crate) fn click(&self, mx: f32, my: f32, cx: &UiContext<'_>) -> Option<UiAction> {
        let rect = self.bar_rect(cx);
        self.hit(rect, mx, my, cx)
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
