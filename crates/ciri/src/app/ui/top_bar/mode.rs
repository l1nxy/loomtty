use super::super::tokens::SEGMENT_PAD_X;
use super::super::types::UiRect;
use ciri_ui::color::contrast_on;
use ciri_ui::{Div, Styled, div, text};

pub(super) struct ModeIndicator<'a> {
    pub(super) label: &'a str,
    pub(super) color: [f32; 4],
}

impl<'a> ModeIndicator<'a> {
    /// Lualine-style right-most section: a flat rectangle filled with
    /// the per-mode colour (already varies between NORMAL / LEADER /
    /// BROADCAST). Text colour is `contrast_on(mode_color)` so a
    /// saturated mode hue stays readable on either light or dark
    /// themes. Mode is display-only — no `hit_id`, no press state.
    pub(super) fn into_div(self, slot: UiRect) -> Div {
        if slot.is_empty() {
            return div();
        }
        let fg = contrast_on(self.color);
        div()
            .absolute()
            .left(slot.x)
            .top(slot.y)
            .w(slot.w)
            .h(slot.h)
            .bg(self.color)
            .text_color(fg)
            .flex_row()
            .items_center()
            .pl(SEGMENT_PAD_X)
            .pr(SEGMENT_PAD_X)
            .child(text(self.label))
    }
}
