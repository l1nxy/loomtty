use super::super::types::{UiContext, UiRect, UiScene};
use crate::app::ciri_ui_adapter::paint_element_tree;
use ciri_ui::{Div, Layer, Styled, div, text};

pub(super) struct WorkspaceIndicator<'a> {
    pub(super) label: &'a str,
}

impl<'a> WorkspaceIndicator<'a> {
    /// Same declarative-hover treatment as `SessionLabel`: the outer
    /// wrapper carries `hit_id(HIT_WORKSPACE)` and a
    /// `.hover(|s| s.text_color(fg))` refinement, so the descendant
    /// Text inherits `accent` at rest and `fg` on hover. No bool
    /// parameter — the framework handles it per frame.
    fn build_tree(&self, rect: UiRect, cx: &UiContext<'_>) -> Div {
        if self.label.is_empty() || rect.is_empty() {
            return div().w(cx.viewport_w).h(cx.viewport_h);
        }
        let fg = cx.theme.on_surface;
        let accent = cx.theme.accent;
        let padding = cx
            .config
            .statusbar
            .height_padding
            .unwrap_or(cx.cell_h * cx.config.statusbar.padding_ratio);
        let top_pad = padding * 0.5;

        // Same wrapper-with-padding pattern as `SessionLabel`: outer
        // hit_id span matches the full slot rect to align with the
        // click hit-zone tree, inner text row positioned via a top
        // spacer to preserve the original visual y.
        div().w(cx.viewport_w).h(cx.viewport_h).child(
            div()
                .in_layer(Layer::Chrome)
                .absolute()
                .left(rect.x)
                .top(rect.y)
                .w(rect.w)
                .h(rect.h)
                .flex_col()
                .text_color(accent)
                .hit_id(super::HIT_WORKSPACE)
                .hover(|s| s.text_color(fg))
                .child(div().w(rect.w).h(top_pad))
                .child(div().w(rect.w).h(cx.cell_h).child(text(self.label))),
        )
    }
}

impl<'a> WorkspaceIndicator<'a> {
    pub(super) fn paint(&self, rect: UiRect, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let root = self.build_tree(rect, cx);
        paint_element_tree(&root, cx, scene);
    }
}
