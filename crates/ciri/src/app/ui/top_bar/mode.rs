use super::super::layout::{Axis, SizeHint, UiElement, UiRect};
use super::super::types::{UiContext, UiScene};
use crate::app::ciri_ui_bridge::paint_ui_tree;
use ciri_ui::{Div, Layer, Styled, div, text};

pub(super) struct ModeIndicator<'a> {
    pub(super) label: &'a str,
    pub(super) color: [f32; 4],
    pub(super) width: f32,
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

impl<'a> UiElement for ModeIndicator<'a> {
    fn size_hint(&self, axis: Axis, _cx: &UiContext<'_>) -> SizeHint {
        match axis {
            Axis::Horizontal => SizeHint::Fixed(self.width),
            Axis::Vertical => SizeHint::Fill,
        }
    }

    fn paint(&self, rect: UiRect, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let root = self.build_tree(rect, cx);
        paint_ui_tree(&root, cx, scene);
    }
}
