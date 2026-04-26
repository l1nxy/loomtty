use super::super::types::{UiContext, UiRect, UiScene};
use crate::app::ciri_ui_adapter::paint_element_tree;
use ciri_ui::{Div, Layer, Styled, div, text};

pub(super) struct SessionLabel<'a> {
    pub(super) text: &'a str,
}

impl<'a> SessionLabel<'a> {
    /// Build the element tree. Hover styling is now declarative: the
    /// outer wrapper carries `hit_id(HIT_SESSION)` and a
    /// `.hover(|s| s.text_color(fg))` refinement, so the walker
    /// switches the descendant Text's inherited colour from `dim` to
    /// `fg` whenever the cursor sits on that hit_id. No `hovered: bool`
    /// parameter needed — the framework figures it out per frame.
    fn build_tree(&self, rect: UiRect, cx: &UiContext<'_>) -> Div {
        let fg = cx.theme.on_surface;
        let dim = cx.theme.on_surface_muted;
        let padding = cx
            .config
            .statusbar
            .height_padding
            .unwrap_or(cx.cell_h * cx.config.statusbar.padding_ratio);
        let top_pad = padding * 0.5;

        // Outer hover-detection wrapper covers the full slot rect so
        // its `hit_id(HIT_SESSION)` aligns with the click hit-zone tree
        // (`build_hit_tree` uses `rect.h` for the same id). The inner
        // text row stays at the original visual y via a top padding
        // strip — this preserves the painted text position from when
        // the hit_id wrapper was sized to cell_h.
        div().w(cx.viewport_w).h(cx.viewport_h).child(
            div()
                .in_layer(Layer::Chrome)
                .absolute()
                .left(rect.x)
                .top(rect.y)
                .w(rect.w)
                .h(rect.h)
                .flex_col()
                .text_color(dim)
                .hit_id(super::HIT_SESSION)
                .hover(|s| s.text_color(fg))
                .child(div().w(rect.w).h(top_pad))
                .child(div().w(rect.w).h(cx.cell_h).child(text(self.text))),
        )
    }
}

impl<'a> SessionLabel<'a> {
    pub(super) fn paint(&self, rect: UiRect, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let root = self.build_tree(rect, cx);
        paint_element_tree(&root, cx, scene);
    }
}
