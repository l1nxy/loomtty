use super::super::layout::{Axis, SizeHint, UiElement, UiRect};
use super::super::types::{UiContext, UiScene};
use crate::app::ciri_ui_bridge::paint_ui_tree;
use ciri_ui::{Layer, Styled, div, text};

pub(super) struct SessionLabel<'a> {
    pub(super) text: &'a str,
    pub(super) hovered: bool,
    pub(super) width: f32,
}

impl<'a> UiElement for SessionLabel<'a> {
    fn size_hint(&self, axis: Axis, _cx: &UiContext<'_>) -> SizeHint {
        match axis {
            Axis::Horizontal => SizeHint::Fixed(self.width),
            Axis::Vertical => SizeHint::Fill,
        }
    }

    fn paint(&self, rect: UiRect, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let fg = cx.theme.on_surface;
        let dim = cx.theme.on_surface_muted;
        let color = if self.hovered { fg } else { dim };
        let padding = cx
            .config
            .statusbar
            .height_padding
            .unwrap_or(cx.cell_h * cx.config.statusbar.padding_ratio);
        let text_y = rect.y + padding * 0.5;

        let root = div().w(cx.viewport_w).h(cx.viewport_h).child(
            div()
                .in_layer(Layer::Chrome)
                .w(rect.w)
                .h(cx.cell_h)
                .translate(rect.x, text_y)
                .child(text(self.text).color(color)),
        );
        paint_ui_tree(&root, cx, scene);
    }
}
