use super::super::types::UiRect;
use ciri_ui::{Color, Div, Styled, div, text};

pub(super) struct WorkspaceIndicator<'a> {
    pub(super) label: &'a str,
}

impl<'a> WorkspaceIndicator<'a> {
    /// Inner positioned Div for the workspace slot. Empty rect or
    /// empty label → returns an empty div (the outer wrapper still
    /// places it but it paints nothing). Same state-styling pattern
    /// as `SessionLabel`: hit_id + `.hover()` switches text colour
    /// from `accent` to `fg`; `.active()` adds the press tint and
    /// keeps text at `fg` so press-and-drag-off doesn't flip back.
    pub(super) fn into_div(
        self,
        rect: UiRect,
        fg: Color,
        accent: Color,
        press_bg: Color,
        top_pad: f32,
        cell_h: f32,
    ) -> Div {
        if self.label.is_empty() || rect.is_empty() {
            return div();
        }
        div()
            .absolute()
            .left(rect.x)
            .top(rect.y)
            .w(rect.w)
            .h(rect.h)
            .flex_col()
            .text_color(accent)
            .hit_id(super::HIT_WORKSPACE)
            .hover(|s| s.text_color(fg))
            .active(|s| s.bg(press_bg).text_color(fg))
            .child(div().w(rect.w).h(top_pad))
            .child(div().w(rect.w).h(cell_h).child(text(self.label)))
    }
}
