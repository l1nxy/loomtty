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

use super::text_layout;
use super::tokens;
use super::types::{UiAction, UiContext, UiRect, UiScene, ui_hit_id};
use crate::app::App;
use crate::app::ciri_ui_adapter::paint_element_tree;
use ciri_ui::{IntoElement, Layer, Render, RenderCtx, Styled, div, text};

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
    /// Height of one tab row, from `config.tabbar.tab_height`.
    tab_height: f32,
    /// Vertical gap between adjacent tabs, from `config.tabbar.tab_gap`.
    tab_gap: f32,
    /// Which side the bar sits on. Used so the active-tab accent strip
    /// and the outer separator line anchor to the bar edge that touches
    /// the terminal — i.e. the *inner* edge — making the bar look
    /// visually "attached" to the terminal for both Left and Right.
    position: TabBarPosition,
    /// Cell width frozen at `capture()` so `build_tree` only needs
    /// `RenderCtx`. Same pattern as the other widgets migrated to
    /// `Render`.
    cell_w: f32,
    /// Pre-truncated label per tab — truncation depends on the
    /// chrome layout `rect.w` (known at capture time) and
    /// `cell_w` (also known), so doing it here keeps `build_tree`
    /// off the host shaper.
    truncated_labels: Vec<String>,
    /// Layout rect from the host's chrome layout, captured once so
    /// the `Render` trait method can read it without an extra
    /// parameter (same pattern as `HintsBarComponent`).
    rect: UiRect,
}

impl TabBarComponent {
    pub fn capture(app: &App, cx: &UiContext<'_>, rect: UiRect) -> Self {
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

        // Pre-truncate each label using the same `label_budget` math
        // `build_tree` would have done at paint time. Pulling it into
        // `capture` keeps `build_tree` off the host shaper so the
        // `Render` impl can run from a minimal `RenderCtx`.
        let indicator_w = tokens::BORDER_THICK;
        let label_budget = (rect.w - indicator_w - cx.cell_w).max(0.0);
        let truncated_labels: Vec<String> = tabs
            .iter()
            .map(|tab| text_layout::truncate_with_ellipsis(cx, &tab.label, label_budget))
            .collect();

        Self {
            tabs,
            tab_height: app.core.config.tabbar.tab_height,
            tab_gap: app.core.config.tabbar.tab_gap,
            position: app.core.config.tabbar.position,
            cell_w: cx.cell_w,
            truncated_labels,
            rect,
        }
    }

    fn render_cx<'a>(cx: &'a UiContext<'_>) -> RenderCtx<'a> {
        RenderCtx {
            theme: cx.theme,
            viewport: [cx.viewport_w, cx.viewport_h],
            scale: 1.0,
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

    fn build_tree(&self, cx: &RenderCtx<'_>) -> ciri_ui::Div {
        let rect = self.rect;
        let bar_bg = cx.theme.statusbar_bg;
        let fg = cx.theme.on_surface;
        let dim = cx.theme.on_surface_muted;
        let accent = cx.theme.accent;
        let sep = tokens::tint(dim, tokens::ALPHA_SEPARATOR);
        let indicator_w = tokens::BORDER_THICK;

        let label_pad = self.cell_w * 0.5;
        let sep_inset_x = tokens::SPACE_1;
        let content_w = (rect.w - tokens::BORDER_THIN).max(0.0);
        // Hover bg + text-color refinements applied declaratively via
        // `.hit_id` + `.hover()` on each row. Inactive non-hovered:
        // dim text, no bg. Active: accent strip + tinted bg, fg text
        // (no hover refinement — active wins). Inactive hovered: hover
        // tint bg + fg text via the refinement, propagated to the
        // descendant Text by `text_color_override_with_state`.
        let active_bg = tokens::tint(accent, tokens::ALPHA_TAB_ACTIVE_BG);
        let hover_bg = tokens::tint(accent, tokens::ALPHA_HOVER_BG);

        let mut rows = div().w(content_w).h(rect.h).flex_col();
        for (idx, tab) in self.tabs.iter().enumerate() {
            let row = self.row_rect(rect, idx);
            if row.is_empty() {
                break;
            }

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

            let truncated = self
                .truncated_labels
                .get(idx)
                .cloned()
                .unwrap_or_default();

            let mut row_el = div().w(content_w).h(row.h).flex_row().items_center();
            if tab.active {
                // Active row: accent-tinted bg, fg text. Set
                // `text_color(fg)` on the row Div so the descendant
                // Text inherits it — needed because Text's own
                // `.color()` would otherwise override (codex Q3 from
                // Step 20 review).
                row_el = row_el.bg(active_bg).text_color(fg);
            } else {
                // Inactive row: dim text base, hover refinement
                // switches to fg text + tinted bg. The Text child
                // intentionally has NO `.color()` so it inherits
                // whichever `text_color` the walker resolves —
                // base (dim) at rest, refinement (fg) on hover via
                // the Step 16 refinement-aware inheritance.
                row_el = row_el
                    .text_color(dim)
                    .hit_id(pane_tab_hit_id(tab.pane_id))
                    .cursor_pointer()
                    .hover(|s| s.bg(hover_bg).text_color(fg));
            }

            let indicator =
                div()
                    .w(indicator_w)
                    .h(row.h)
                    .bg(if tab.active { accent } else { [0.0; 4] });
            // Text deliberately has no `.color(...)` — the row Div
            // sets `text_color` (active = fg directly; inactive = dim
            // with hover refinement to fg) and the walker propagates.
            let label = div()
                .w((content_w - indicator_w).max(0.0))
                .h(row.h)
                .flex_row()
                .items_center()
                .pl(label_pad)
                .pr(label_pad)
                .child(text(truncated));

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
        let root = div().w(cx.viewport[0]).h(cx.viewport[1]).child(
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

impl TabBarComponent {
    pub(crate) fn paint(&mut self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        if self.rect.is_empty() {
            return;
        }
        let render_cx = Self::render_cx(cx);
        // Sixth production usage of `ciri_ui::Render`. The chrome
        // layout `rect` was captured in `Self::capture(app, cx, rect)`
        // and the active label color is text-color-inheritance from
        // the row Div via the Step 16 refinement support.
        let root = <Self as Render>::render(self, &render_cx).into_element();
        paint_element_tree(&root, cx, scene);
    }
}

impl Render for TabBarComponent {
    fn render(&mut self, cx: &RenderCtx<'_>) -> impl IntoElement {
        self.build_tree(cx)
    }
}

impl TabBarComponent {

    pub(crate) fn hit(
        &self,
        rect: UiRect,
        mx: f32,
        my: f32,
        cx: &UiContext<'_>,
    ) -> Option<UiAction> {
        if !rect.contains(mx, my) {
            return None;
        }
        let root = self.build_hit_tree(rect, cx);
        pane_id_from_hit_id(ui_hit_id(&root, cx, mx, my)).map(UiAction::FocusPaneTab)
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
