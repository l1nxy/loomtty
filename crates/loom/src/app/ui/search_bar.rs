use loom_layout::geometry::Rect as GeoRect;
use loom_ui::{Div, IntoElement, Render, RenderCtx, Styled, deferred, div, text};

use super::types::{UiContext, UiScene};
use crate::app::loom_ui_adapter::paint_element_tree;

pub(crate) struct SearchBarComponent {
    pub(crate) query: String,
    pub(crate) matches_len: usize,
    pub(crate) current_match_idx: usize,
    pub(crate) pane_rect: GeoRect,
    /// Border width / padding / cell height frozen at construction so
    /// the `Render` impl reads from `self` instead of re-borrowing
    /// `UiContext`. Constructed by `App::search_bar_component` which
    /// already has the host metrics in scope.
    pub(crate) border_w: f32,
    pub(crate) padding: f32,
    pub(crate) cell_h: f32,
}

impl SearchBarComponent {
    pub(crate) fn paint(&mut self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let render_cx = RenderCtx {
            theme: cx.theme,
            viewport: [cx.viewport_w, cx.viewport_h],
            scale: 1.0,
        };
        // 9th production usage of `loom_ui::Render`. SearchBar holds
        // every metric it needs (cell_h, border_w, padding) on `self`
        // — no host shaper required at paint time.
        let root = <Self as Render>::render(self, &render_cx).into_element();
        paint_element_tree(&root, cx, scene);
    }

    fn build_tree(&self, cx: &RenderCtx<'_>) -> Div {
        let bar_height = self.cell_h + 4.0;
        let bar_y = self.pane_rect.y + self.pane_rect.h - self.border_w - bar_height;
        let bar_x = self.pane_rect.x + self.border_w;
        let bar_w = self.pane_rect.w - self.border_w * 2.0;

        let match_info = if self.matches_len == 0 {
            if self.query.is_empty() {
                String::new()
            } else {
                " [no matches]".to_string()
            }
        } else {
            format!(" [{}/{}]", self.current_match_idx + 1, self.matches_len)
        };
        let bar_text = format!(" Search: {}{}", self.query, match_info);

        let bar = div()
            .absolute()
            .left(bar_x)
            .top(bar_y)
            .w(bar_w)
            .h(bar_height)
            .bg([0.15, 0.15, 0.2, 0.95])
            .child(
                div()
                    .absolute()
                    .left(self.padding)
                    .top(2.0)
                    .w((bar_w - self.padding * 2.0).max(0.0))
                    .h(self.cell_h)
                    .child(text(bar_text).color([1.0, 1.0, 1.0, 1.0])),
            );
        // `deferred()` keeps the bar above pane content emitted earlier
        // in the host'\''s scene merge — replaces `Layer::Overlay`.
        div()
            .w(cx.viewport[0])
            .h(cx.viewport[1])
            .child(deferred(bar))
    }
}

impl Render for SearchBarComponent {
    fn render(&mut self, cx: &RenderCtx<'_>) -> impl IntoElement {
        self.build_tree(cx)
    }
}
