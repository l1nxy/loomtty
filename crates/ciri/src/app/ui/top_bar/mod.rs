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
use crate::app::ciri_ui_bridge::paint_ui_tree;
use crate::app::top_bar::{PaneTabLayout, TopBarLayout};
use crate::app::{App, TopBarHoverRegion};
use ciri_ui::{Div, Layer, Styled, div};

const HIT_SESSION: u64 = 1;
const HIT_WORKSPACE: u64 = 2;
const HIT_MODE: u64 = 3;
const HIT_TAB_BASE: u64 = 1_000_000;

fn pane_tab_hit_id(pane_id: u64) -> u64 {
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
    hovered_region: Option<TopBarHoverRegion>,
    hovered_pane_tab: Option<u64>,
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
            session_text: format!(" {}  ", app.session_display_name()),
            workspace_label,
            mode_label,
            mode_color,
            pane_tabs,
            hovered_region: app.core.hovered_top_bar_region,
            hovered_pane_tab: app.core.hovered_pane_tab,
            is_leader: app.core.input.is_awaiting_action(),
            is_broadcast: app.core.broadcast_mode,
            is_overview: app.core.overview.active,
            tab_scroll: app.core.pane_tab_scroll,
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
        // Widths of fixed zones. Measured via `text_layout` so proportional UI
        // fonts get their real advance. The App-side `top_bar_layout` measures
        // the same way, keeping `tabs_area_px` in lockstep with these slots.
        let session_w = self.layout.session_w;
        let workspace_w = if self.workspace_label.is_empty() {
            0.0
        } else {
            text_layout::measure(cx, &self.workspace_label)
        };
        let mode_w = text_layout::measure(cx, &self.mode_label);
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

    fn paint_row(&self, rect: UiRect, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let slots = self.row_slots(rect, cx);
        SessionLabel {
            text: &self.session_text,
            hovered: self.hovered_region == Some(TopBarHoverRegion::Session),
        }
        .paint(slots.session, cx, scene);

        if self.show_integrated_tabs {
            PaneTabsElement {
                tabs: &self.pane_tabs,
                hovered_tab: self.hovered_pane_tab,
                scroll: self.tab_scroll,
                scroll_max: self.tab_scroll_max,
            }
            .paint(slots.pane_tabs, cx, scene);
        }

        WorkspaceIndicator {
            label: &self.workspace_label,
            hovered: self.hovered_region == Some(TopBarHoverRegion::Workspace),
        }
        .paint(slots.workspace, cx, scene);

        ModeIndicator {
            label: &self.mode_label,
            color: self.mode_color,
        }
        .paint(slots.mode, cx, scene);
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
            .in_layer(Layer::Chrome)
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

    fn build_chrome_tree(&self, rect: UiRect, cx: &UiContext<'_>) -> Div {
        let bar_bg = cx.theme.statusbar_bg;
        let dim = cx.theme.on_surface_muted;
        let accent = cx.theme.accent;
        let broadcast_color = cx.theme.broadcast;
        let sep_color = tokens::tint(dim, tokens::ALPHA_SEPARATOR);

        let sep_y = match cx.config.statusbar.position {
            StatusBarPosition::Top => rect.h - tokens::BORDER_THIN,
            StatusBarPosition::Bottom => 0.0,
        };
        let mut root = div().w(cx.viewport_w).h(cx.viewport_h).child(
            div()
                .in_layer(Layer::Chrome)
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
        );

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
            root = root.child(
                div()
                    .in_layer(Layer::Chrome)
                    .absolute()
                    .left(rect.x)
                    .top(band_y)
                    .w(rect.w)
                    .h(indicator_h)
                    .bg(indicator_color),
            );
        }

        root
    }
}

impl TopBarComponent {
    pub(crate) fn paint(&self, rect: UiRect, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let chrome = self.build_chrome_tree(rect, cx);
        paint_ui_tree(&chrome, cx, scene);

        // --- inner row (session | tabs | workspace | mode) ---
        self.paint_row(rect, cx, scene);
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
