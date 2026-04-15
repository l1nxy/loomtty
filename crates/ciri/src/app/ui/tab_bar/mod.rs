//! Vertical side tab bar (`TabBarPosition::Left` / `::Right`).
//!
//! When `config.tabbar.position != Integrated`, pane tabs are extracted
//! out of the status bar and painted as a dedicated vertical strip on
//! the chosen side of the terminal viewport. This module owns the
//! vertical-layout variant; the horizontal (integrated) variant lives
//! in `top_bar/pane_tabs.rs` as `PaneTabsElement`.
//!
//! # Coordinate system
//!
//! Unlike the integrated tab strip — whose per-tab `x`/`w` are baked at
//! snapshot time by `App::pane_tab_layouts` assuming the bar starts at
//! screen x=0 — the vertical variant computes tab rows *inside* `paint`
//! from the rect handed in by the parent `Border`. This keeps the
//! component fully rect-local and suitable for `Border::left` or
//! `Border::right` placement without caring which side it lives on.

mod truncate;

use ciri_config::config::TabBarPosition;
use ciri_config::theme::ThemeConfig;

use self::truncate::truncate_to_cols;
use super::builder::UiBuilder;
use super::layout::{Axis, SizeHint, UiElement, UiRect};
use super::tokens;
use super::types::{UiAction, UiContext, UiScene};
use crate::app::App;

/// Snapshot of a single tab: what pane it represents, its label, and
/// whether it is currently active.
#[derive(Debug, Clone)]
pub(crate) struct TabEntry {
    pub pane_id: u64,
    pub label: String,
    pub active: bool,
}

pub(crate) struct TabBarComponent {
    tabs: Vec<TabEntry>,
    hovered_tab: Option<u64>,
    /// Fixed width of the bar, from `config.tabbar.width`.
    bar_width: f32,
    /// Height of one tab row, from `config.tabbar.tab_height`.
    tab_height: f32,
    /// Vertical gap between adjacent tabs, from `config.tabbar.tab_gap`.
    tab_gap: f32,
    /// Which side the bar sits on. Used so the active-tab accent strip
    /// and the outer separator line anchor to the bar edge that touches
    /// the terminal — i.e. the *inner* edge — making the bar look
    /// visually "attached" to the terminal for both Left and Right.
    position: TabBarPosition,
}

impl TabBarComponent {
    pub fn capture(app: &App, _cx: &UiContext<'_>) -> Self {
        let active_pane_id = app.core.workspaces.active().active_pane_id();
        let tabs: Vec<TabEntry> = app
            .pane_tab_entries()
            .into_iter()
            .enumerate()
            .map(|(idx, (pane_id, title))| TabEntry {
                pane_id,
                label: app.format_pane_tab_label(idx, &title),
                active: Some(pane_id) == active_pane_id,
            })
            .collect();
        Self {
            tabs,
            hovered_tab: app.core.hovered_pane_tab,
            bar_width: app.core.config.tabbar.width,
            tab_height: app.core.config.tabbar.tab_height,
            tab_gap: app.core.config.tabbar.tab_gap,
            position: app.core.config.tabbar.position,
        }
    }

    /// Row rect for the nth tab inside the bar's rect. Clipped at the
    /// bottom if the bar would overflow (callers should skip empty rects).
    fn row_rect(&self, rect: UiRect, idx: usize) -> UiRect {
        let stride = self.tab_height + self.tab_gap;
        let y = rect.y + idx as f32 * stride;
        let max_bottom = rect.bottom();
        let h = (self.tab_height).min((max_bottom - y).max(0.0));
        UiRect::new(rect.x, y, rect.w, h)
    }
}

impl UiElement for TabBarComponent {
    fn size_hint(&self, axis: Axis, _cx: &UiContext<'_>) -> SizeHint {
        match axis {
            // Fixed horizontal width — this is what makes the bar a
            // dedicated side strip rather than stealing from the terminal.
            Axis::Horizontal => SizeHint::Fixed(self.bar_width),
            // Fills the height that Border gave us (minus what top/bottom
            // edges already consumed).
            Axis::Vertical => SizeHint::Fill,
        }
    }

    fn paint(&self, rect: UiRect, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        if rect.is_empty() {
            return;
        }
        let bar_bg = ThemeConfig::parse_color(&cx.config.theme.statusbar_background);
        let fg = ThemeConfig::parse_color(&cx.config.theme.foreground);
        let dim = ThemeConfig::parse_color(&cx.config.theme.statusbar_dim);
        let accent = ThemeConfig::parse_color(&cx.config.theme.accent);
        let sep = tokens::tint(dim, tokens::ALPHA_SEPARATOR);
        let indicator_w = tokens::BORDER_THICK;

        // Absolute-coord builder anchored at the bar's top-left.
        let mut ui = UiBuilder::new_horizontal(
            rect.x, rect.y, rect.w, cx.cell_h, 0.0, 0.0, 0.0, false, cx, scene,
        );

        // Inner edge = the edge of the bar that touches the terminal.
        // For Left: inner edge is on the right (rect.right() - 1). For
        // Right: inner edge is on the left (rect.x). The separator and
        // active indicator both anchor to this edge so the bar looks
        // "attached" to the terminal on either side.
        let inner_sep_x = match self.position {
            TabBarPosition::Left => rect.right() - tokens::BORDER_THIN,
            TabBarPosition::Right => rect.x,
            // Unreachable in paint — `TabBarComponent` is only wired in
            // for the two side positions.
            TabBarPosition::Integrated => rect.right() - tokens::BORDER_THIN,
        };
        let indicator_x = match self.position {
            TabBarPosition::Left => rect.right() - indicator_w,
            TabBarPosition::Right => rect.x,
            TabBarPosition::Integrated => rect.right() - indicator_w,
        };
        // Label padding — indent from the outer edge (away from the
        // terminal) so the indicator strip sits flush against the text.
        let label_x_base = match self.position {
            TabBarPosition::Left => rect.x + cx.cell_w * 0.5,
            TabBarPosition::Right => rect.x + indicator_w + cx.cell_w * 0.5,
            TabBarPosition::Integrated => rect.x + cx.cell_w * 0.5,
        };

        // Bar background + outer separator on the inner edge.
        ui.abs_rect(rect.x, rect.y, rect.w, rect.h, bar_bg);
        ui.abs_rect(inner_sep_x, rect.y, tokens::BORDER_THIN, rect.h, sep);

        // Shared vocabulary with integrated tabs in `top_bar::pane_tabs`:
        //   • hairline separator between adjacent rows (SPACE_1 horizontal inset)
        //   • active row gets a subtle accent-tint bg + accent indicator strip
        //   • hovered row gets a lighter accent-tint bg
        //   • inactive rows: just dim text on the bar bg
        // The only axis-specific differences are orientation: side bar
        // draws the separator as a horizontal hairline, integrated draws
        // it vertical; side indicator is vertical, integrated horizontal.
        let sep_inset_x = tokens::SPACE_1;
        for (idx, tab) in self.tabs.iter().enumerate() {
            let row = self.row_rect(rect, idx);
            if row.is_empty() {
                break;
            }
            let hovered = self.hovered_tab == Some(tab.pane_id);

            // Active / hover background tint — mirrors integrated variant.
            let bg_alpha = if tab.active {
                Some(tokens::ALPHA_TAB_ACTIVE_BG)
            } else if hovered {
                Some(tokens::ALPHA_HOVER_BG)
            } else {
                None
            };
            if let Some(a) = bg_alpha {
                ui.abs_rect(row.x, row.y, row.w, row.h, tokens::tint(accent, a));
            }

            // Inter-row separator on the leading edge of every row except
            // the first, inset on both sides like the integrated variant's
            // vertical separator.
            if idx > 0 {
                ui.abs_rect(
                    row.x + sep_inset_x,
                    row.y - tokens::BORDER_THIN * 0.5,
                    row.w - sep_inset_x * 2.0,
                    tokens::BORDER_THIN,
                    sep,
                );
            }

            // Active-tab accent strip on the inner edge.
            if tab.active {
                ui.abs_rect(indicator_x, row.y, indicator_w, row.h, accent);
            }

            // Label.
            let label_color = if tab.active || hovered { fg } else { dim };
            let text_y = row.y + (row.h - cx.cell_h) * 0.5;
            // Truncate to fit in the row (subtract the indicator + one
            // cell of padding on each side of the text).
            let budget_cols =
                (((row.w - indicator_w - cx.cell_w).max(0.0)) / cx.cell_w) as usize;
            let truncated = truncate_to_cols(&tab.label, budget_cols);
            ui.abs_text(&truncated, label_x_base, text_y, label_color);
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
        for (idx, tab) in self.tabs.iter().enumerate() {
            let row = self.row_rect(rect, idx);
            if row.is_empty() {
                break;
            }
            if row.contains(mx, my) {
                return Some(UiAction::FocusPaneTab(tab.pane_id));
            }
        }
        None
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
