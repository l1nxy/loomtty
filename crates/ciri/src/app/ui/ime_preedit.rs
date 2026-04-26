use ciri_ui::{Div, IntoElement, Layer, Render, RenderCtx, Styled, div, text};
use unicode_width::UnicodeWidthStr;

use super::types::{UiContext, UiScene};
use crate::app::ciri_ui_adapter::paint_element_tree;

pub(crate) struct ImePreeditComponent {
    pub(crate) text: String,
    pub(crate) base_x: f32,
    pub(crate) base_y: f32,
    pub(crate) cursor_cols: Option<usize>,
    /// Cell metrics frozen at construction so the `Render` impl reads
    /// from `self`. Caller fills these in (the `App` helper that
    /// constructs the component already has cell metrics in scope).
    pub(crate) cell_w: f32,
    pub(crate) cell_h: f32,
}

impl ImePreeditComponent {
    pub(crate) fn paint(&mut self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let render_cx = RenderCtx {
            theme: cx.theme,
            viewport: [cx.viewport_w, cx.viewport_h],
            scale: 1.0,
        };
        // 10th production usage of `ciri_ui::Render`. Cell metrics are
        // frozen on the component so `build_tree` runs from a minimal
        // `RenderCtx`.
        let root = <Self as Render>::render(self, &render_cx).into_element();
        paint_element_tree(&root, cx, scene);
    }

    fn build_tree(&self, cx: &RenderCtx<'_>) -> Div {
        let text_width = UnicodeWidthStr::width(self.text.as_str()) as f32 * self.cell_w;
        let box_w = text_width + 4.0;

        let panel = div()
            .in_layer(Layer::Overlay)
            .absolute()
            .left(self.base_x)
            .top(self.base_y)
            .w(box_w)
            .h(self.cell_h + 2.0)
            .bg([0.15, 0.15, 0.25, 0.95])
            .child(
                div()
                    .absolute()
                    .left(2.0)
                    .top(1.0)
                    .w(text_width.max(0.0))
                    .h(self.cell_h)
                    .child(text(self.text.clone()).color([1.0, 1.0, 1.0, 1.0])),
            );
        let mut root = div().w(cx.viewport[0]).h(cx.viewport[1]).child(panel).child(
            div()
                .in_layer(Layer::Overlay)
                .absolute()
                .left(self.base_x)
                .top(self.base_y + self.cell_h)
                .w(box_w)
                .h(2.0)
                .bg([0.5, 0.7, 1.0, 0.9]),
        );

        if let Some(cursor_cols) = self.cursor_cols {
            root = root.child(
                div()
                    .in_layer(Layer::Overlay)
                    .absolute()
                    .left(self.base_x + 2.0 + cursor_cols as f32 * self.cell_w)
                    .top(self.base_y + 1.0)
                    .w(2.0)
                    .h(self.cell_h)
                    .bg([1.0, 1.0, 1.0, 0.8]),
            );
        }
        root
    }
}

impl Render for ImePreeditComponent {
    fn render(&mut self, cx: &RenderCtx<'_>) -> impl IntoElement {
        self.build_tree(cx)
    }
}
