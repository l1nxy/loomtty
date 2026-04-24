use super::super::types::{UiContext, UiRect, UiScene};
use crate::app::ciri_ui_adapter::paint_element_tree;
use ciri_ui::{Div, Layer, Styled, div, text};

pub(super) struct ModeIndicator<'a> {
    pub(super) label: &'a str,
    pub(super) color: [f32; 4],
}

impl<'a> ModeIndicator<'a> {
    fn build_tree(&self, rect: UiRect, cx: &UiContext<'_>) -> Div {
        if rect.is_empty() {
            return div().w(cx.viewport_w).h(cx.viewport_h);
        }
        let padding = cx
            .config
            .statusbar
            .height_padding
            .unwrap_or(cx.cell_h * cx.config.statusbar.padding_ratio);
        let text_y = rect.y + padding * 0.5;

        div().w(cx.viewport_w).h(cx.viewport_h).child(
            div()
                .in_layer(Layer::Chrome)
                .absolute()
                .left(rect.x)
                .top(text_y)
                .w(rect.w)
                .h(cx.cell_h)
                .child(text(self.label).color(self.color)),
        )
    }
}

impl<'a> ModeIndicator<'a> {
    pub(super) fn paint(&self, rect: UiRect, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let root = self.build_tree(rect, cx);
        paint_element_tree(&root, cx, scene);
    }
}
