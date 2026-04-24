use ciri_ui::{Layer, Styled, div, text};
use unicode_width::UnicodeWidthStr;

use super::types::{UiContext, UiScene};
use crate::app::ciri_ui_adapter::paint_ui_tree;

pub(crate) struct ImePreeditComponent {
    pub(crate) text: String,
    pub(crate) base_x: f32,
    pub(crate) base_y: f32,
    pub(crate) cursor_cols: Option<usize>,
}

impl ImePreeditComponent {
    pub(crate) fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let text_width = UnicodeWidthStr::width(self.text.as_str()) as f32 * cx.cell_w;
        let box_w = text_width + 4.0;

        let panel = div()
            .in_layer(Layer::Overlay)
            .absolute()
            .left(self.base_x)
            .top(self.base_y)
            .w(box_w)
            .h(cx.cell_h + 2.0)
            .bg([0.15, 0.15, 0.25, 0.95])
            .child(
                div()
                    .absolute()
                    .left(2.0)
                    .top(1.0)
                    .w(text_width.max(0.0))
                    .h(cx.cell_h)
                    .child(text(self.text.clone()).color([1.0, 1.0, 1.0, 1.0])),
            );
        let mut root = div().w(cx.viewport_w).h(cx.viewport_h).child(panel).child(
            div()
                .in_layer(Layer::Overlay)
                .absolute()
                .left(self.base_x)
                .top(self.base_y + cx.cell_h)
                .w(box_w)
                .h(2.0)
                .bg([0.5, 0.7, 1.0, 0.9]),
        );

        if let Some(cursor_cols) = self.cursor_cols {
            root = root.child(
                div()
                    .in_layer(Layer::Overlay)
                    .absolute()
                    .left(self.base_x + 2.0 + cursor_cols as f32 * cx.cell_w)
                    .top(self.base_y + 1.0)
                    .w(2.0)
                    .h(cx.cell_h)
                    .bg([1.0, 1.0, 1.0, 0.8]),
            );
        }

        paint_ui_tree(&root, cx, scene);
    }
}
