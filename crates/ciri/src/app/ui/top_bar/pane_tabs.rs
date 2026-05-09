use ciri_config::config::StatusBarPosition;

use super::super::text_layout;
use super::super::tokens;
use super::super::types::{UiContext, UiRect};
use crate::app::top_bar::PaneTabLayout;
use ciri_ui::{Div, Styled, div, text};

/// Element-relative scrollable tab list.
///
/// Holds *only* element-local state: a borrowed slice of pre-computed
/// tab layouts plus scroll bookkeeping. All geometry (separator height,
/// active-indicator y, fade gradient bounds) is derived from the `rect`
/// handed to `paint` — there is no captured `bar_y` or `bar_height`.
/// This keeps the element's drawing strictly inside `rect`, matching what the
/// side-tab layout in `super::super::tab_bar` relies on.
///
/// Hover is now declarative: each non-active tab wrapper carries a
/// `.hit_id(pane_tab_hit_id(...))` and `.hover(|s| s.bg(hover_bg).text_color(fg))`,
/// so the walker switches the descendant Text's inherited colour from
/// `dim` to `fg` whenever the cursor sits on that hit_id (refinement-aware
/// inheritance from Step 16 makes the text_color refinement reach the
/// child Text). No stored `hovered_tab` state on the widget.
pub(super) struct PaneTabsElement<'a> {
    pub(super) tabs: &'a [PaneTabLayout],
    pub(super) scroll: f32,
    pub(super) scroll_max: f32,
}

impl<'a> PaneTabsElement<'a> {
    /// Produce the absolute-positioned children for the pane-tabs
    /// region. Each tab is a single Div wrapper at
    /// `(visible_left, rect.y, visible_w, rect.h)` with hit_id +
    /// declarative hover. Separators sit between tabs as flat
    /// siblings; the active indicator and the label text are
    /// children of the per-tab wrapper (positioned wrapper-relative)
    /// so the wrapper's `text_color` refinement propagates to the
    /// label via the walker's inheritance machinery.
    pub(super) fn into_children(self, rect: UiRect, cx: &UiContext<'_>) -> Vec<Div> {
        if rect.is_empty() {
            return Vec::new();
        }
        let bar_bg = cx.theme.statusbar_bg;
        let fg = cx.theme.on_surface;
        let dim = cx.theme.on_surface_muted;
        let accent = cx.theme.accent;
        let separator_color = tokens::tint(dim, tokens::ALPHA_SEPARATOR);
        let padding = cx
            .config
            .statusbar
            .height_padding
            .unwrap_or(cx.cell_h * cx.config.statusbar.padding_ratio);
        let label_top_in_wrapper = padding * 0.5;

        let tabs_start_x = rect.x;
        let tabs_end_x = rect.right();
        let indicator_thickness = tokens::BORDER_THICK;
        // The active-tab accent strip sits on the bar edge that touches
        // the terminal viewport — opposite side from the bar's outer
        // edge. For a `Top` status bar, that is the bottom of `rect`;
        // for a `Bottom` status bar, the top.
        let indicator_top_in_wrapper = match cx.config.statusbar.position {
            StatusBarPosition::Top => rect.h - indicator_thickness,
            StatusBarPosition::Bottom => 0.0,
        };
        let separator_inset = tokens::SPACE_1;

        let active_bg = tokens::tint(accent, tokens::ALPHA_TAB_ACTIVE_BG);
        // Hover / press go neutral (chrome `element_hover` /
        // `element_active`) — preset-independent. Active tab keeps the
        // accent tint above for the focused-pane selection cue.
        let hover_bg = cx.theme.element_hover;
        let press_bg = cx.theme.element_active;
        // Tab corners: round the edge that faces away from the bar's
        // baseline so the tab reads as "lifted off the bar" — top
        // corners when the bar sits at the top of the viewport, bottom
        // corners when the bar sits at the bottom.
        let tab_radius = cx.theme.radius.sm;

        let mut children: Vec<Div> = Vec::new();
        for (idx, tab) in self.tabs.iter().enumerate() {
            let visible_left = tab.x.max(tabs_start_x);
            let visible_right = (tab.x + tab.w).min(tabs_end_x);
            let visible_w = (visible_right - visible_left).max(0.0);
            if visible_w <= 0.0 {
                continue;
            }

            // Separator on the leading edge — flat sibling between
            // tabs. Skipped for the first tab so the hairline doesn't
            // bleed onto the session section's solid bg, and skipped
            // when scrolled off-screen.
            if idx > 0 && tab.x > tabs_start_x - tokens::BORDER_THIN && tab.x < tabs_end_x {
                children.push(abs_rect(
                    tab.x - tokens::BORDER_THIN * 0.5,
                    rect.y + separator_inset,
                    tokens::BORDER_THIN,
                    rect.h - separator_inset * 2.0,
                    separator_color,
                ));
            }

            // Per-tab wrapper. Active gets the active bg + fg text;
            // inactive gets dim text + a hover refinement that flips
            // bg + text_color when the cursor sits on this hit_id.
            // The wrapper covers the visible tab area so the framework
            // sees a real positioned element under the cursor.
            let mut tab_wrapper = div()
                .absolute()
                .left(visible_left)
                .top(rect.y)
                .w(visible_w)
                .h(rect.h)
                .hit_id(super::pane_tab_hit_id(tab.pane_id))
                .cursor_pointer();
            tab_wrapper = match cx.config.statusbar.position {
                StatusBarPosition::Top => tab_wrapper.rounded_t(tab_radius),
                StatusBarPosition::Bottom => tab_wrapper.rounded_b(tab_radius),
            };
            if tab.active {
                // The currently-focused tab also responds to press —
                // a slight darken so re-clicking the active tab still
                // gives feedback (handy for users who muscle-mash).
                tab_wrapper = tab_wrapper
                    .bg(active_bg)
                    .text_color(fg)
                    .active(|s| s.bg(press_bg));
            } else {
                tab_wrapper = tab_wrapper
                    .text_color(dim)
                    .hover(|s| s.bg(hover_bg).text_color(fg))
                    // Press wins over hover (CSS `:active` semantics):
                    // the user feels the click commit even if the
                    // cursor drifts during the press.
                    .active(|s| s.bg(press_bg).text_color(fg));
            }

            // Active indicator — child of the wrapper, so its y is
            // wrapper-relative.
            if tab.active {
                tab_wrapper = tab_wrapper.child(
                    div()
                        .absolute()
                        .left(0.0)
                        .top(indicator_top_in_wrapper)
                        .w(visible_w)
                        .h(indicator_thickness)
                        .bg(accent),
                );
            }

            // Label — child of the wrapper. Compute the label slot
            // in viewport coords (same clipping math as before), then
            // convert left to wrapper-relative via `- visible_left`.
            // The Text node has no `.color()` — it inherits via the
            // wrapper's `text_color` (and refinement on hover).
            let pad = cx.cell_w * 0.5;
            let label_left = (tab.x + pad).max(tabs_start_x);
            let label_right = (tab.x + tab.w - pad).min(tabs_end_x);
            let label_budget = (label_right - label_left).max(0.0);
            if label_budget > 0.0 {
                let truncated = text_layout::truncate_with_ellipsis(cx, &tab.label, label_budget);
                if !truncated.is_empty() {
                    tab_wrapper = tab_wrapper.child(
                        div()
                            .absolute()
                            .left(label_left - visible_left)
                            .top(label_top_in_wrapper)
                            .child(text(truncated)),
                    );
                }
            }

            children.push(tab_wrapper);
        }

        // Fade gradients at the scrollable edges — flat siblings,
        // outside any tab wrapper, so they always paint on top.
        let fade_w = (cx.cell_w * 3.0).min(rect.w * 0.25);
        if fade_w > 0.0 {
            if self.scroll > 0.5 {
                for i in 0..4 {
                    let alpha = 0.22 * (1.0 - i as f32 / 4.0);
                    children.push(abs_rect(
                        tabs_start_x + i as f32 * (fade_w / 4.0),
                        rect.y,
                        fade_w / 4.0 + 1.0,
                        rect.h,
                        [bar_bg[0], bar_bg[1], bar_bg[2], alpha],
                    ));
                }
            }
            if self.scroll < self.scroll_max - 0.5 {
                for i in 0..4 {
                    let alpha = 0.22 * (i as f32 + 1.0) / 4.0;
                    children.push(abs_rect(
                        tabs_end_x - fade_w + i as f32 * (fade_w / 4.0),
                        rect.y,
                        fade_w / 4.0 + 1.0,
                        rect.h,
                        [bar_bg[0], bar_bg[1], bar_bg[2], alpha],
                    ));
                }
            }
        }

        children
    }
}

fn abs_rect(x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) -> Div {
    div()
        .absolute()
        .left(x)
        .top(y)
        .w(w)
        .h(h)
        .bg(color)
}
