use super::super::layout::{UiElement, UiRect};
use super::super::types::{UiContext, UiScene};
use crate::app::ciri_ui_bridge::paint_ui_tree;
use ciri_ui::{Div, Layer, Styled, div, text};

pub(super) struct SessionLabel<'a> {
    pub(super) text: &'a str,
    pub(super) hovered: bool,
}

impl<'a> SessionLabel<'a> {
    fn build_tree(&self, rect: UiRect, cx: &UiContext<'_>) -> Div {
        let fg = cx.theme.on_surface;
        let dim = cx.theme.on_surface_muted;
        let color = if self.hovered { fg } else { dim };
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
                .child(text(self.text).color(color)),
        )
    }
}

impl<'a> UiElement for SessionLabel<'a> {
    fn paint(&self, rect: UiRect, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let root = self.build_tree(rect, cx);
        paint_ui_tree(&root, cx, scene);
    }
}
