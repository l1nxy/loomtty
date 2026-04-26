use super::super::types::UiRect;
use ciri_ui::{Color, Div, Styled, div, text};

pub(super) struct SessionLabel<'a> {
    pub(super) text: &'a str,
}

impl<'a> SessionLabel<'a> {
    /// Build the inner positioned Div for this sub-widget. Returns
    /// just the absolute-positioned slot — `TopBarComponent::build_tree`
    /// wraps the viewport once and stitches all sub-widget trees as
    /// siblings, so 4 sub-paints become one walker pass per frame.
    /// Hover styling is declarative: `hit_id(HIT_SESSION)` +
    /// `.hover(|s| s.text_color(fg))` lets the walker switch Text's
    /// inherited colour from `dim` to `fg` on cursor.
    pub(super) fn into_div(
        self,
        rect: UiRect,
        fg: Color,
        dim: Color,
        top_pad: f32,
        cell_h: f32,
    ) -> Div {
        div()
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
            .child(div().w(rect.w).h(cell_h).child(text(self.text)))
    }
}
