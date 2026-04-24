use ciri_ui::{Layer, Styled, div, text};
use unicode_width::UnicodeWidthStr;

use super::types::{UiContext, UiScene};
use crate::app::ciri_ui_bridge::paint_ui_tree;

pub(crate) fn paint_ime_preedit(
    preedit_text: &str,
    base_x: f32,
    base_y: f32,
    cursor_cols: Option<usize>,
    cx: &UiContext<'_>,
    scene: &mut UiScene<'_>,
) {
    let text_width = UnicodeWidthStr::width(preedit_text) as f32 * cx.cell_w;
    let box_w = text_width + 4.0;

    let panel = div()
        .in_layer(Layer::Overlay)
        .absolute()
        .left(base_x)
        .top(base_y)
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
                .child(text(preedit_text.to_string()).color([1.0, 1.0, 1.0, 1.0])),
        );
    let mut root = div().w(cx.viewport_w).h(cx.viewport_h).child(panel).child(
        div()
            .in_layer(Layer::Overlay)
            .absolute()
            .left(base_x)
            .top(base_y + cx.cell_h)
            .w(box_w)
            .h(2.0)
            .bg([0.5, 0.7, 1.0, 0.9]),
    );

    if let Some(cursor_cols) = cursor_cols {
        root = root.child(
            div()
                .in_layer(Layer::Overlay)
                .absolute()
                .left(base_x + 2.0 + cursor_cols as f32 * cx.cell_w)
                .top(base_y + 1.0)
                .w(2.0)
                .h(cx.cell_h)
                .bg([1.0, 1.0, 1.0, 0.8]),
        );
    }

    paint_ui_tree(&root, cx, scene);
}
