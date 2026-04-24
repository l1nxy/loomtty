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
//! screen x=0 — the vertical variant computes tab rows from the rect handed in
//! by chrome composition. This keeps the component fully rect-local without
//! caring which side it lives on.

use ciri_config::config::TabBarPosition;

use super::layout::{UiElement, UiRect};
use super::text_layout;
use super::tokens;
use super::types::{UiAction, UiContext, UiScene};
use crate::app::ciri_ui_bridge::paint_ui_tree;
use crate::app::App;
use ciri_ui::{div, text, Layer, Styled};

const HIT_TAB_BASE: u64 = 1_000_000;

fn pane_tab_hit_id(pane_id: u64) -> u64 {
    HIT_TAB_BASE + pane_id
}

fn pane_id_from_hit_id(hit_id: Option<u64>) -> Option<u64> {
    hit_id
        .filter(|id| *id >= HIT_TAB_BASE)
        .map(|id| id - HIT_TAB_BASE)
}

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

    fn build_hit_tree(&self, rect: UiRect, cx: &UiContext<'_>) -> ciri_ui::Div {
        let mut rows = div().w(rect.w).h(rect.h).flex_col();
        for (idx, tab) in self.tabs.iter().enumerate() {
            let row = self.row_rect(rect, idx);
            if row.is_empty() {
                break;
            }
            if idx > 0 && self.tab_gap > 0.0 {
                rows = rows.child(div().w(rect.w).h(self.tab_gap));
            }
            rows = rows.child(
                div()
                    .w(rect.w)
                    .h(row.h)
                    .hit_id(pane_tab_hit_id(tab.pane_id))
                    .cursor_pointer(),
            );
        }

        div().w(cx.viewport_w).h(cx.viewport_h).child(
            rows.in_layer(Layer::Chrome)
                .absolute()
                .left(rect.x)
                .top(rect.y)
                .w(rect.w)
                .h(rect.h),
        )
    }

    fn build_tree(&self, rect: UiRect, cx: &UiContext<'_>) -> ciri_ui::Div {
        let bar_bg = cx.theme.statusbar_bg;
        let fg = cx.theme.on_surface;
        let dim = cx.theme.on_surface_muted;
        let accent = cx.theme.accent;
        let sep = tokens::tint(dim, tokens::ALPHA_SEPARATOR);
        let indicator_w = tokens::BORDER_THICK;

        // Inner edge = the edge of the bar that touches the terminal.
        // For Left: inner edge is on the right (rect.right() - 1). For
        // Right: inner edge is on the left (rect.x). The separator and
        // active indicator both anchor to this edge so the bar looks
        // "attached" to the terminal on either side.
        // Label padding — indent from the outer edge (away from the
        // terminal) so the indicator strip sits flush against the text.
        let label_pad = cx.cell_w * 0.5;
        let sep_inset_x = tokens::SPACE_1;
        let content_w = (rect.w - tokens::BORDER_THIN).max(0.0);
        let label_budget = (rect.w - indicator_w - cx.cell_w).max(0.0);

        let mut rows = div().w(content_w).h(rect.h).flex_col();
        for (idx, tab) in self.tabs.iter().enumerate() {
            let row = self.row_rect(rect, idx);
            if row.is_empty() {
                break;
            }
            let hovered = self.hovered_tab == Some(tab.pane_id);

            if idx > 0 && self.tab_gap > 0.0 {
                rows = rows.child(
                    div()
                        .w(content_w)
                        .h(self.tab_gap)
                        .flex_col()
                        .justify_center()
                        .child(
                            div()
                                .w((content_w - sep_inset_x * 2.0).max(0.0))
                                .h(tokens::BORDER_THIN)
                                .bg(sep),
                        ),
                );
            }

            // Active / hover background tint — mirrors integrated variant.
            let bg_alpha = if tab.active {
                Some(tokens::ALPHA_TAB_ACTIVE_BG)
            } else if hovered {
                Some(tokens::ALPHA_HOVER_BG)
            } else {
                None
            };
            // Label. Truncate with ellipsis so shaped text never overflows
            // the row, regardless of whether the UI font is monospaced.
            let label_color = if tab.active || hovered { fg } else { dim };
            let truncated = text_layout::truncate_with_ellipsis(cx, &tab.label, label_budget);
            let mut row_el = div().w(content_w).h(row.h).flex_row().items_center();
            if let Some(a) = bg_alpha {
                row_el = row_el.bg(tokens::tint(accent, a));
            }

            let indicator =
                div()
                    .w(indicator_w)
                    .h(row.h)
                    .bg(if tab.active { accent } else { [0.0; 4] });
            let label = div()
                .w((content_w - indicator_w).max(0.0))
                .h(row.h)
                .flex_row()
                .items_center()
                .pl(label_pad)
                .pr(label_pad)
                .child(text(truncated).color(label_color));

            row_el = match self.position {
                TabBarPosition::Left | TabBarPosition::Integrated => {
                    row_el.child(label).child(indicator)
                }
                TabBarPosition::Right => row_el.child(indicator).child(label),
            };
            rows = rows.child(row_el);
        }

        let sep_line = div().w(tokens::BORDER_THIN).h(rect.h).bg(sep);
        let bar = match self.position {
            TabBarPosition::Left | TabBarPosition::Integrated => {
                div().flex_row().child(rows).child(sep_line)
            }
            TabBarPosition::Right => div().flex_row().child(sep_line).child(rows),
        };
        let root = div().w(cx.viewport_w).h(cx.viewport_h).child(
            bar.in_layer(Layer::Chrome)
                .absolute()
                .left(rect.x)
                .top(rect.y)
                .w(rect.w)
                .h(rect.h)
                .bg(bar_bg),
        );

        root
    }
}

impl UiElement for TabBarComponent {
    fn paint(&self, rect: UiRect, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        if rect.is_empty() {
            return;
        }
        let root = self.build_tree(rect, cx);
        paint_ui_tree(&root, cx, scene);
    }

    fn hit(&self, rect: UiRect, mx: f32, my: f32, cx: &UiContext<'_>) -> Option<UiAction> {
        if !rect.contains(mx, my) {
            return None;
        }
        let root = self.build_hit_tree(rect, cx);
        let mut shaper = ciri_ui::NullShaper;
        let out = ciri_ui::paint_tree_with_layout(
            &root,
            cx.theme,
            [cx.viewport_w, cx.viewport_h],
            1.0,
            &mut shaper,
        );
        pane_id_from_hit_id(out.layout.hit_test(mx, my).and_then(|node| node.hit_id))
            .map(UiAction::FocusPaneTab)
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
