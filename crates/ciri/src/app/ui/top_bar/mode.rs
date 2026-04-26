use super::super::types::UiRect;
use ciri_ui::{Div, Styled, div, text};

pub(super) struct ModeIndicator<'a> {
    pub(super) label: &'a str,
    pub(super) color: [f32; 4],
}

impl<'a> ModeIndicator<'a> {
    /// Inner positioned Div for the mode slot. Empty rect → empty div
    /// (no hover, no hit_id; mode label is display-only).
    pub(super) fn into_div(self, rect: UiRect, top_pad: f32, cell_h: f32) -> Div {
        if rect.is_empty() {
            return div();
        }
        div()
            .absolute()
            .left(rect.x)
            .top(rect.y)
            .w(rect.w)
            .h(rect.h)
            .flex_col()
            .child(div().w(rect.w).h(top_pad))
            .child(div().w(rect.w).h(cell_h).child(text(self.label).color(self.color)))
    }
}
