use super::super::tokens::SEGMENT_PAD_X;
use super::super::types::UiRect;
use loom_ui::color::scale_rgb;
use loom_ui::{Color, Div, Styled, div, text};

pub(super) struct WorkspaceIndicator<'a> {
    pub(super) label: &'a str,
}

impl<'a> WorkspaceIndicator<'a> {
    /// Lualine-style mid-section: a `surface_elevated` rectangle with
    /// `accent` text. Same flat geometry as `SessionLabel` — fills
    /// the slot edge-to-edge, no rounding — but a quieter colour so
    /// it doesn't fight the bright session block on the far left or
    /// the per-mode block on the far right.
    pub(super) fn into_div(self, slot: UiRect, accent: Color, surface_elevated: Color) -> Div {
        if self.label.is_empty() || slot.is_empty() {
            return div();
        }
        let press_bg = scale_rgb(surface_elevated, 0.85);
        div()
            .absolute()
            .left(slot.x)
            .top(slot.y)
            .w(slot.w)
            .h(slot.h)
            .bg(surface_elevated)
            .text_color(accent)
            .flex_row()
            .items_center()
            .pl(SEGMENT_PAD_X)
            .pr(SEGMENT_PAD_X)
            .hit_id(super::HIT_WORKSPACE)
            .cursor_pointer()
            .active(|s| s.bg(press_bg))
            .child(text(self.label))
    }
}
