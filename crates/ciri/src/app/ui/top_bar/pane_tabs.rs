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
pub(super) struct PaneTabsElement<'a> {
    pub(super) tabs: &'a [PaneTabLayout],
    pub(super) hovered_tab: Option<u64>,
    pub(super) scroll: f32,
    pub(super) scroll_max: f32,
}

impl<'a> PaneTabsElement<'a> {
    /// Produce the absolute-positioned children for the pane-tabs
    /// region (per-tab bg, separators, active indicator, labels, fade
    /// gradients). Returns a `Vec<Div>` so caller (`TopBarComponent`)
    /// can push each as a flat sibling of the unified viewport root —
    /// avoids wrapping in a zero-sized parent that the walker would
    /// skip, and keeps absolute positions relative to the same
    /// positioning context as all the other top-bar children.
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
        let text_y = rect.y + padding * 0.5;

        let tabs_start_x = rect.x;
        let tabs_end_x = rect.right();
        let indicator_thickness = tokens::BORDER_THICK;
        // The active-tab accent strip sits on the bar edge that touches
        // the terminal viewport — opposite side from the bar's outer
        // edge. For a `Top` status bar, that is the bottom of `rect`;
        // for a `Bottom` status bar, the top.
        let indicator_y = match cx.config.statusbar.position {
            StatusBarPosition::Top => rect.bottom() - indicator_thickness,
            StatusBarPosition::Bottom => rect.y,
        };
        let separator_inset = tokens::SPACE_1;

        let mut children: Vec<Div> = Vec::new();
        for tab in self.tabs {
            let hovered = self.hovered_tab == Some(tab.pane_id);
            let visible_left = tab.x.max(tabs_start_x);
            let visible_right = (tab.x + tab.w).min(tabs_end_x);
            let visible_w = (visible_right - visible_left).max(0.0);
            if visible_w <= 0.0 {
                continue;
            }

            // Active / hover background tint — same vocabulary as the
            // side `tab_bar`, so integrated and side tabs read as the
            // same component rotated onto a different axis.
            let bg_alpha = if tab.active {
                Some(tokens::ALPHA_TAB_ACTIVE_BG)
            } else if hovered {
                Some(tokens::ALPHA_HOVER_BG)
            } else {
                None
            };
            if let Some(a) = bg_alpha {
                children.push(abs_rect(
                    visible_left,
                    rect.y,
                    visible_w,
                    rect.h,
                    tokens::tint(accent, a),
                ));
            }

            // Separator on the leading edge (skipped if scrolled off-screen).
            if tab.x > tabs_start_x - tokens::BORDER_THIN && tab.x < tabs_end_x {
                children.push(abs_rect(
                    tab.x - tokens::BORDER_THIN * 0.5,
                    rect.y + separator_inset,
                    tokens::BORDER_THIN,
                    rect.h - separator_inset * 2.0,
                    separator_color,
                ));
            }
            // Active-tab indicator (thin accent strip on top or bottom).
            if tab.active {
                children.push(abs_rect(
                    visible_left,
                    indicator_y,
                    visible_w,
                    indicator_thickness,
                    accent,
                ));
            }
            // Clipped label. Truncate with ellipsis so shaped text never
            // overflows the tab slot on proportional UI fonts. One cell of
            // padding on each side keeps the text off the separator edge.
            let color = if tab.active || hovered { fg } else { dim };
            let pad = cx.cell_w * 0.5;
            let label_left = (tab.x + pad).max(tabs_start_x);
            let label_right = (tab.x + tab.w - pad).min(tabs_end_x);
            let label_budget = (label_right - label_left).max(0.0);
            if label_budget > 0.0 {
                let truncated = text_layout::truncate_with_ellipsis(cx, &tab.label, label_budget);
                if !truncated.is_empty() {
                    children.push(abs_text(label_left, text_y, truncated, color));
                }
            }
        }

        // Fade gradients at the scrollable edges.
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

// Layer is inherited from the parent wrapper that TopBarComponent
// places at `Layer::Chrome`, so per-rect / per-text `.in_layer()`
// is redundant after sub-widget unification.
fn abs_rect(x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) -> Div {
    div()
        .absolute()
        .left(x)
        .top(y)
        .w(w)
        .h(h)
        .bg(color)
}

fn abs_text(x: f32, y: f32, content: String, color: [f32; 4]) -> Div {
    div()
        .absolute()
        .left(x)
        .top(y)
        .child(text(content).color(color))
}
