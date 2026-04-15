//! Top bar chrome — session label, pane tabs, workspace indicator, mode badge.
//!
//! # Structure
//!
//! `TopBarComponent` is the outer "panel" that owns:
//! - Global decorations (bar background, separator line, leader/broadcast strip)
//! - A [`Linear`] row of four sub-elements that lay out horizontally:
//!   - [`session_label::SessionLabel`] — fixed width, clicks open the session palette
//!   - [`pane_tabs::PaneTabsElement`] — fill, scrollable list of pane tabs
//!   - [`workspace::WorkspaceIndicator`] — fixed width (or zero), clicks cycle workspace
//!   - [`mode::ModeIndicator`] — fixed width, clicks toggle overview
//!
//! Each sub-element implements [`UiElement`] and is told exactly what rect
//! to draw into; none of them look at `cx.viewport_w`. This is a direct
//! translation of niri's `LayoutElement::render(location)` idea
//! (`niri/src/layout/mod.rs:128`) to the chrome layer.
//!
//! # Compatibility
//!
//! `TopBarComponent` also still implements [`UiComponent`] — this keeps
//! the existing `ui/mod.rs` dispatch code working while we transition.
//! The `UiComponent` impl just builds the full-width rect from
//! `cx.viewport_w` and delegates to `UiElement::paint`/`::hit`.

mod mode;
mod pane_tabs;
mod session_label;
mod workspace;

use ciri_config::config::StatusBarPosition;
use ciri_config::theme::ThemeConfig;
use unicode_width::UnicodeWidthStr;

use self::mode::ModeIndicator;
use self::pane_tabs::PaneTabsElement;
use self::session_label::SessionLabel;
use self::workspace::WorkspaceIndicator;
use super::builder::UiBuilder;
use super::layout::{Axis, Linear, SizeHint, Spacer, UiElement, UiRect};
use super::types::{UiAction, UiComponent, UiContext, UiScene, UiTopBarHit};
use crate::app::top_bar::{PaneTabLayout, TopBarLayout};
use crate::app::{App, TopBarHoverRegion};

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
    /// (`TabBarComponent`) and the middle slot of this bar's row is a
    /// no-op `Spacer` instead of `PaneTabsElement`. Keeps session/
    /// workspace/mode positions stable regardless of tab placement.
    show_integrated_tabs: bool,
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
            app.pane_tab_layouts(cx.cell_w, layout.tabs_area_px)
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
    /// Kept as a helper so the `UiComponent` impl and legacy `hit_test`
    /// can both ask "where am I?" without duplicating the math.
    ///
    /// NB (pre-existing, inherited from `UiComponent::paint`/`::click`):
    /// `cx.viewport_w` here comes from `App::ui_context()` →
    /// `command_palette_viewport_size()`, which is the renderer surface
    /// size. During a resize event this can briefly differ from the `vw`
    /// passed to `App::build_ui`. The discrepancy resolves naturally when
    /// `UiComponent` is removed and all paint/hit goes through
    /// `UiElement::paint(rect)` / `::hit(rect)` with the rect that
    /// `Border::layout` produced from the same `vw`.
    pub(super) fn bar_rect(&self, cx: &UiContext<'_>) -> UiRect {
        UiRect::new(0.0, self.layout.bar_y, cx.viewport_w, self.layout.bar_height)
    }

    /// Build the inner [`Linear`] row. Extracted so both paint and hit
    /// see the exact same child layout.
    ///
    /// The returned `Linear` borrows from `self`; no per-frame heap
    /// allocation for the tab snapshot or label strings.
    pub(super) fn build_row<'a>(&'a self, cx: &UiContext<'_>) -> Linear<'a> {
        // Widths of the right-side fixed zones — matches the pre-split math
        // in `top_bar_layout()`. Use `UnicodeWidthStr::width()` everywhere
        // (matching the layout side) so multibyte mode/workspace labels
        // align correctly with `tabs_area_px`.
        let session_w = self.layout.session_w;
        let workspace_w = if self.workspace_label.is_empty() {
            0.0
        } else {
            UnicodeWidthStr::width(self.workspace_label.as_str()) as f32 * cx.cell_w
        };
        let mode_w = UnicodeWidthStr::width(self.mode_label.as_str()) as f32 * cx.cell_w;

        // Sub-elements borrow from `self` — no per-frame heap allocation
        // for the tab snapshot or label strings. The `'a` lifetime ties
        // every child to `&self`, so the returned `Linear` is short-lived
        // and discarded after the paint/hit call returns.
        //
        // When `show_integrated_tabs == false`, the middle slot becomes
        // a `Spacer` so the right-side items stay right-anchored and the
        // bar background still covers the full width. The actual tabs
        // are painted by a sibling `TabBarComponent` in the `Border`.
        let row = Linear::new(Axis::Horizontal).push(SessionLabel {
            text: &self.session_text,
            hovered: self.hovered_region == Some(TopBarHoverRegion::Session),
            width: session_w,
        });
        let row = if self.show_integrated_tabs {
            row.push(PaneTabsElement {
                tabs: &self.pane_tabs,
                hovered_tab: self.hovered_pane_tab,
                scroll: self.tab_scroll,
                scroll_max: self.tab_scroll_max,
            })
        } else {
            row.push(Spacer)
        };
        row.push(WorkspaceIndicator {
            label: &self.workspace_label,
            hovered: self.hovered_region == Some(TopBarHoverRegion::Workspace),
            width: workspace_w,
        })
        .push(ModeIndicator {
            label: &self.mode_label,
            color: self.mode_color,
            width: mode_w,
        })
    }

    pub(super) fn hit_test(&self, mx: f32, my: f32, cx: &UiContext<'_>) -> Option<UiTopBarHit> {
        let rect = self.bar_rect(cx);
        if !rect.contains(mx, my) {
            return None;
        }
        // The Linear decides which child owns which slice; we ask it.
        // UiElement::hit returns a `UiAction` though — map back to the
        // `UiTopBarHit` enum expected by the legacy hover dispatcher.
        let row = self.build_row(cx);
        match row.hit(rect, mx, my, cx) {
            Some(UiAction::OpenSessionPalette) => Some(UiTopBarHit::Session),
            Some(UiAction::CycleWorkspace) => Some(UiTopBarHit::Workspace),
            Some(UiAction::ToggleOverview) => Some(UiTopBarHit::Mode),
            Some(UiAction::FocusPaneTab(id)) => Some(UiTopBarHit::PaneTab(id)),
            // Any other action would be a programming error here; the top
            // bar only produces the four actions above.
            _ => Some(UiTopBarHit::Background),
        }
    }
}

impl UiElement for TopBarComponent {
    fn size_hint(&self, axis: Axis, _cx: &UiContext<'_>) -> SizeHint {
        match axis {
            Axis::Vertical => SizeHint::Fixed(self.layout.bar_height),
            Axis::Horizontal => SizeHint::Fill,
        }
    }

    fn paint(&self, rect: UiRect, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let bar_bg = ThemeConfig::parse_color(&cx.config.theme.statusbar_background);
        let dim = ThemeConfig::parse_color(&cx.config.theme.statusbar_dim);
        let accent = ThemeConfig::parse_color(&cx.config.theme.accent);
        let broadcast_color = ThemeConfig::parse_color(&cx.config.theme.mode_broadcast);
        let sep_color = [dim[0], dim[1], dim[2], 0.25];

        // --- global decorations: bar background + separator strip ---
        {
            // We just need a no-op UiBuilder long enough to emit absolute
            // rects via `abs_rect`. (The builder's cursor is not used here.)
            let mut ui = UiBuilder::new_horizontal(
                rect.x, rect.y, rect.w, cx.cell_h, 0.0, 0.0, 0.0, false, cx, scene,
            );
            ui.abs_rect(rect.x, rect.y, rect.w, rect.h, bar_bg);
            let sep_y = match cx.config.statusbar.position {
                StatusBarPosition::Top => rect.bottom() - 1.0,
                StatusBarPosition::Bottom => rect.y,
            };
            ui.abs_rect(rect.x, sep_y, rect.w, 1.0, sep_color);
        }

        // --- inner row (session | tabs | workspace | mode) ---
        self.build_row(cx).paint(rect, cx, scene);

        // --- leader / broadcast / overview band (painted above or below) ---
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
            let mut ui = UiBuilder::new_horizontal(
                rect.x, rect.y, rect.w, cx.cell_h, 0.0, 0.0, 0.0, false, cx, scene,
            );
            ui.abs_rect(rect.x, band_y, rect.w, indicator_h, indicator_color);
        }
    }

    fn hit(
        &self,
        rect: UiRect,
        mx: f32,
        my: f32,
        cx: &UiContext<'_>,
    ) -> Option<UiAction> {
        if !rect.contains(mx, my) {
            return None;
        }
        // If no child claims the click, we still consume it as "Background"
        // at the dispatcher level — but UiElement::hit returns UiAction,
        // and there is no `Background` action. Return None so the caller
        // can treat "top-bar-hit but no action" itself.
        self.build_row(cx).hit(rect, mx, my, cx)
    }
}

impl UiComponent for TopBarComponent {
    fn click(&self, mx: f32, my: f32, cx: &UiContext<'_>) -> Option<UiAction> {
        let rect = self.bar_rect(cx);
        <Self as UiElement>::hit(self, rect, mx, my, cx)
    }

    fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let rect = self.bar_rect(cx);
        <Self as UiElement>::paint(self, rect, cx, scene);
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
