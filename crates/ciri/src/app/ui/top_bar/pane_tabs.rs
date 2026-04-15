use ciri_config::config::StatusBarPosition;
use ciri_config::theme::ThemeConfig;

use super::super::builder::UiBuilder;
use super::super::layout::{Axis, SizeHint, UiElement, UiRect};
use super::super::tokens;
use super::super::types::{UiAction, UiContext, UiScene};
use crate::app::top_bar::PaneTabLayout;

/// Element-relative scrollable tab list.
///
/// Holds *only* element-local state: a borrowed slice of pre-computed
/// tab layouts plus scroll bookkeeping. All geometry (separator height,
/// active-indicator y, fade gradient bounds) is derived from the `rect`
/// handed to `paint` — there is no captured `bar_y` or `bar_height`.
/// This keeps the element's drawing strictly inside `rect`, which is
/// the contract of `UiElement` (and what the side-tab layout in
/// `super::super::tab_bar` relies on).
pub(super) struct PaneTabsElement<'a> {
    pub(super) tabs: &'a [PaneTabLayout],
    pub(super) hovered_tab: Option<u64>,
    pub(super) scroll: f32,
    pub(super) scroll_max: f32,
}

impl<'a> UiElement for PaneTabsElement<'a> {
    fn size_hint(&self, _axis: Axis, _cx: &UiContext<'_>) -> SizeHint {
        // Tabs fill whatever horizontal space is left between the fixed
        // zones; vertically they stretch with the parent bar rect.
        SizeHint::Fill
    }

    fn paint(&self, rect: UiRect, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        if rect.is_empty() {
            return;
        }
        let bar_bg = ThemeConfig::parse_color(&cx.config.theme.statusbar_background);
        let fg = ThemeConfig::parse_color(&cx.config.theme.foreground);
        let dim = ThemeConfig::parse_color(&cx.config.theme.statusbar_dim);
        let accent = ThemeConfig::parse_color(&cx.config.theme.accent);
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

        let mut ui = UiBuilder::new_horizontal(
            rect.x, text_y, rect.w, cx.cell_h, 0.0, 0.0, 0.0, false, cx, scene,
        );

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
                ui.abs_rect(
                    visible_left,
                    rect.y,
                    visible_w,
                    rect.h,
                    tokens::tint(accent, a),
                );
            }

            // Separator on the leading edge (skipped if scrolled off-screen).
            if tab.x > tabs_start_x - tokens::BORDER_THIN && tab.x < tabs_end_x {
                ui.abs_rect(
                    tab.x - tokens::BORDER_THIN * 0.5,
                    rect.y + separator_inset,
                    tokens::BORDER_THIN,
                    rect.h - separator_inset * 2.0,
                    separator_color,
                );
            }
            // Active-tab indicator (thin accent strip on top or bottom).
            if tab.active {
                ui.abs_rect(
                    visible_left,
                    indicator_y,
                    visible_w,
                    indicator_thickness,
                    accent,
                );
            }
            // Clipped label.
            let color = if tab.active || hovered { fg } else { dim };
            if let Some((label, label_x)) =
                clip_tab_label(&tab.label, tab.x, tab.w, cx.cell_w, tabs_start_x, tabs_end_x)
            {
                ui.abs_text(&label, label_x, text_y, color);
            }
        }

        // Fade gradients at the scrollable edges.
        let fade_w = (cx.cell_w * 3.0).min(rect.w * 0.25);
        if fade_w > 0.0 {
            if self.scroll > 0.5 {
                for i in 0..4 {
                    let alpha = 0.22 * (1.0 - i as f32 / 4.0);
                    ui.abs_rect(
                        tabs_start_x + i as f32 * (fade_w / 4.0),
                        rect.y,
                        fade_w / 4.0 + 1.0,
                        rect.h,
                        [bar_bg[0], bar_bg[1], bar_bg[2], alpha],
                    );
                }
            }
            if self.scroll < self.scroll_max - 0.5 {
                for i in 0..4 {
                    let alpha = 0.22 * (i as f32 + 1.0) / 4.0;
                    ui.abs_rect(
                        tabs_end_x - fade_w + i as f32 * (fade_w / 4.0),
                        rect.y,
                        fade_w / 4.0 + 1.0,
                        rect.h,
                        [bar_bg[0], bar_bg[1], bar_bg[2], alpha],
                    );
                }
            }
        }
    }

    fn hit(
        &self,
        rect: UiRect,
        mx: f32,
        my: f32,
        _cx: &UiContext<'_>,
    ) -> Option<UiAction> {
        if !rect.contains(mx, my) {
            return None;
        }
        let tabs_start_x = rect.x;
        let tabs_end_x = rect.right();
        for tab in self.tabs {
            let visible_left = tab.x.max(tabs_start_x);
            let visible_right = (tab.x + tab.w).min(tabs_end_x);
            if mx >= visible_left && mx <= visible_right {
                return Some(UiAction::FocusPaneTab(tab.pane_id));
            }
        }
        None
    }
}

fn clip_tab_label(
    label: &str,
    tab_x: f32,
    tab_w: f32,
    cw: f32,
    tabs_start_x: f32,
    tabs_end_x: f32,
) -> Option<(String, f32)> {
    use unicode_width::UnicodeWidthChar;

    let visible_left = tab_x.max(tabs_start_x);
    let visible_right = (tab_x + tab_w).min(tabs_end_x);
    if visible_right <= visible_left {
        return None;
    }
    let skip_cols = ((visible_left - tab_x) / cw).floor().max(0.0) as usize;
    let visible_cols = ((visible_right - visible_left) / cw).floor().max(0.0) as usize;

    let mut col = 0usize;
    let mut clipped = String::new();
    let mut clip_start_col = skip_cols;
    for ch in label.chars() {
        let char_w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if col + char_w > skip_cols + visible_cols {
            break;
        }
        if col >= skip_cols {
            if clipped.is_empty() {
                // Record the actual column where we start clipping.
                // For wide chars straddling the boundary, this may be > skip_cols.
                clip_start_col = col;
            }
            clipped.push(ch);
        }
        col += char_w;
    }
    if clipped.is_empty() {
        None
    } else {
        // Shift draw position right if a wide char was partially skipped.
        let overhang = (clip_start_col - skip_cols) as f32 * cw;
        Some((clipped, visible_left + overhang))
    }
}
