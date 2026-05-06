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
mod workspace;

use ciri_config::config::StatusBarPosition;

use self::mode::ModeIndicator;
use self::pane_tabs::PaneTabsElement;
use self::session_label::SessionLabel;
use self::workspace::WorkspaceIndicator;
use super::text_layout;
use super::tokens;
use super::types::{UiAction, UiContext, UiRect, UiScene, UiTopBarHit, ui_hit_id};
use crate::app::ciri_ui_adapter::paint_element_tree;
use crate::app::top_bar::{PaneTabLayout, TopBarLayout};
use crate::app::App;
use ciri_ui::{Div, Styled, div};

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
    session_text: String,
    workspace_label: String,
    mode_label: String,
    mode_color: [f32; 4],
    pane_tabs: Vec<PaneTabLayout>,
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

#[derive(Debug, Clone, Copy)]
pub(super) struct TopBarRowSlots {
    pub session: UiRect,
    pub pane_tabs: UiRect,
    pub workspace: UiRect,
    pub mode: UiRect,
}

impl TopBarComponent {
    pub fn capture(app: &App, layout: TopBarLayout, cx: &UiContext<'_>) -> Self {
        let (mode_label, mode_color) = app.current_mode_label();
        let workspace_label = app.workspace_indicator_label();
        let show_integrated_tabs = matches!(
            app.core.config.tabbar.position,
            ciri_config::config::TabBarPosition::Integrated,
        );
        // Tab snapshot is only needed when we draw them inline. Saves
        // a Vec allocation + label cloning for side-bar configs.
        let pane_tabs = if show_integrated_tabs {
            app.pane_tab_layouts(cx.cell_w, layout.tabs_area_px, cx.ui_shaper)
        } else {
            Vec::new()
        };
        Self {
            layout,
            session_text: app.session_display_name(),
            workspace_label,
            mode_label,
            mode_color,
            pane_tabs,
            is_leader: app.core.input.is_awaiting_action(),
            is_broadcast: app.core.broadcast_mode,
            is_overview: app.core.overview.active,
            tab_scroll: app.pane_tab_scroll,
            tab_scroll_max: app.pane_tab_scroll_max(),
            show_integrated_tabs,
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
        // Widths of fixed zones. Each capsule slot = measured text width
        // + capsule padding + visual gap budget (`segment_slot_width` in
        // `crate::app::top_bar`). The App-side `top_bar_layout` wraps
        // its measurements through the same helpers, including the
        // mode > session > workspace overflow cap, so the fill slot
        // (= bar_w - sum(fixed)) and the captured `tabs_area_px` stay
        // in lockstep — see `pane_tabs_element_slot_is_one_cell_wider…`.
        let session_w =
            crate::app::top_bar::segment_slot_width(text_layout::measure(cx, &self.session_text));
        let workspace_w = if self.workspace_label.is_empty() {
            0.0
        } else {
            crate::app::top_bar::segment_slot_width(text_layout::measure(cx, &self.workspace_label))
        };
        let mode_w =
            crate::app::top_bar::segment_slot_width(text_layout::measure(cx, &self.mode_label));
        let (session_w, workspace_w, mode_w) =
            crate::app::top_bar::cap_fixed_section_widths(rect.w, session_w, workspace_w, mode_w);
        let fixed_w = session_w + workspace_w + mode_w;
        let pane_tabs_w = (rect.w - fixed_w).max(0.0);

        let session = UiRect::new(rect.x, rect.y, session_w, rect.h);
        let pane_tabs = UiRect::new(session.right(), rect.y, pane_tabs_w, rect.h);
        let workspace = UiRect::new(pane_tabs.right(), rect.y, workspace_w, rect.h);
        let mode = UiRect::new(workspace.right(), rect.y, mode_w, rect.h);

        TopBarRowSlots {
            session,
            pane_tabs,
            workspace,
            mode,
        }
    }

    /// Produce the absolute-positioned children for the row's 4 sub-
    /// widgets. Returns a flat `Vec<Div>` so caller can push each as
    /// a sibling of the viewport root — avoids a row-level wrapper
    /// that would shift child absolute coords by `(rect.x, rect.y)`
    /// (codex Q: double-offset bug from Step 25 review).
    fn build_row_children(&self, rect: UiRect, cx: &UiContext<'_>) -> Vec<Div> {
        let slots = self.row_slots(rect, cx);
        let accent = cx.theme.accent;
        let on_accent = cx.theme.on_accent;
        let surface_elevated = cx.theme.surface_elevated;

        let mut children: Vec<Div> = Vec::new();
        children.push(
            SessionLabel {
                text: &self.session_text,
            }
            .into_div(slots.session, accent, on_accent),
        );

        if self.show_integrated_tabs {
            // PaneTabsElement returns its own Vec<Div> of absolute
            // tab/label/fade children — one rounded pill per tab.
            children.extend(
                PaneTabsElement {
                    tabs: &self.pane_tabs,
                    scroll: self.tab_scroll,
                    scroll_max: self.tab_scroll_max,
                }
                .into_children(slots.pane_tabs, cx),
            );
        }

        children.push(
            WorkspaceIndicator {
                label: &self.workspace_label,
            }
            .into_div(slots.workspace, accent, surface_elevated),
        );

        children.push(
            ModeIndicator {
                label: &self.mode_label,
                color: self.mode_color,
            }
            .into_div(slots.mode),
        );

        children
    }

    fn build_hit_tree(&self, rect: UiRect, cx: &UiContext<'_>) -> Div {
        let slots = self.row_slots(rect, cx);
        let workspace_w = slots.workspace.w;
        let mode_w = slots.mode.w;
        let mut tabs_slot = div().flex_1().h(rect.h);
        let tabs_start_x = slots.pane_tabs.x;
        let tabs_end_x = slots.pane_tabs.right().max(tabs_start_x);
        let mut cursor_x = tabs_start_x;

        if self.show_integrated_tabs {
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

        let mut row = div()
            .absolute()
            .left(rect.x)
            .top(rect.y)
            .w(rect.w)
            .h(rect.h)
            .flex_row()
            .child(
                div()
                    .w(slots.session.w)
                    .h(rect.h)
                    .hit_id(HIT_SESSION)
                    .cursor_pointer(),
            )
            .child(tabs_slot);
        if workspace_w > 0.0 {
            row = row.child(
                div()
                    .w(workspace_w)
                    .h(rect.h)
                    .hit_id(HIT_WORKSPACE)
                    .cursor_pointer(),
            );
        }
        row = row.child(div().w(mode_w).h(rect.h).hit_id(HIT_MODE).cursor_pointer());

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
