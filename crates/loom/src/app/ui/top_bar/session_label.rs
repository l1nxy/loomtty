use super::super::tokens::SEGMENT_PAD_X;
use super::super::types::UiRect;
use loom_ui::color::scale_rgb;
use loom_ui::{Color, Div, Styled, div, text};

pub(super) struct SessionLabel<'a> {
    pub(super) text: &'a str,
}

impl<'a> SessionLabel<'a> {
    /// Lualine-style left-most section: a solid `accent` rectangle
    /// that fills the slot edge-to-edge, with `on_accent` text. No
    /// rounding, no inset — the section is the whole slot. Press
    /// darkens the fill so the user feels the click commit.
    pub(super) fn into_div(self, slot: UiRect, accent: Color, on_accent: Color) -> Div {
        if slot.is_empty() {
            return div();
        }
        let press_bg = scale_rgb(accent, 0.8);
        div()
            .absolute()
            .left(slot.x)
            .top(slot.y)
            .w(slot.w)
            .h(slot.h)
            .bg(accent)
            .text_color(on_accent)
            .flex_row()
            .items_center()
            .pl(SEGMENT_PAD_X)
            .pr(SEGMENT_PAD_X)
            .hit_id(super::HIT_SESSION)
            .cursor_pointer()
            .active(|s| s.bg(press_bg))
            .child(text(self.text))
    }
}
