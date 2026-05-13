//! NumberField — `[ − value + ]` stepper.
//!
//! Stateless config: the consumer formats the value (so int / float
//! settings keep their own precision), supplies a hit_id for each
//! button, and dispatches the click in its own `hit_test → action`
//! handler. The component itself does no math — it just paints the
//! row and tags the dec / inc click regions.
//!
//! ```ignore
//! use ciri_ui::NumberField;
//!
//! const HIT_FONT_SIZE_DEC: u64 = 1100;
//! const HIT_FONT_SIZE_INC: u64 = 1101;
//!
//! NumberField::new(HIT_FONT_SIZE_DEC, HIT_FONT_SIZE_INC)
//!     .value(format!("{}", config.font_size))
//!     .into_div(cx.theme)
//! ```
//!
//! Width is fixed (96 px) so multiple NumberFields stack into a
//! settings page without per-row width math; if a future row needs
//! wider value text, expose `width()` on the builder.

use crate::elements::{Div, div, text};
use crate::shared_string::SharedString;
use crate::styled::Styled;
use crate::theme::ResolvedTheme;

const TOTAL_W: f32 = 144.0;
// Tracks Dropdown::ROW_H so the two button-class controls share a
// vertical baseline when they appear in the same settings row.
const ROW_H: f32 = 48.0;
// Square buttons (48 × 48) — matches the row height so the dec/inc
// regions read as proper press targets, not narrow strips.
const BTN_W: f32 = 48.0;
const HAIRLINE: f32 = 1.0;

pub struct NumberField {
    dec_hit_id: u64,
    inc_hit_id: u64,
    value: SharedString,
    width: f32,
}

impl NumberField {
    /// Build a NumberField with hit ids for the decrement and
    /// increment buttons. The parent panel maps these back to typed
    /// actions in its click handler.
    pub fn new(dec_hit_id: u64, inc_hit_id: u64) -> Self {
        Self {
            dec_hit_id,
            inc_hit_id,
            value: SharedString::default(),
            width: TOTAL_W,
        }
    }

    /// Set the value display string. Caller is responsible for
    /// formatting (decimal places, units, etc.).
    pub fn value(mut self, value: impl Into<SharedString>) -> Self {
        self.value = value.into();
        self
    }

    /// Override the overall stepper width (defaults to 144 px). The
    /// dec / inc buttons stay 40 × 40; the value cell absorbs any
    /// extra width. Settings rows use this to keep every control
    /// column the same width so the right edges align.
    pub fn width(mut self, width: f32) -> Self {
        self.width = width.max(BTN_W * 2.0 + HAIRLINE * 2.0);
        self
    }

    pub fn into_div(self, theme: &ResolvedTheme) -> Div {
        let outer_bg = theme.surface_sunken;
        let border_color = theme.border;
        let fg = theme.on_surface;
        let hover_bg = theme.element_hover;
        let press_bg = theme.element_active;

        // Value cell expands to fill whatever's left after the two
        // 40×40 buttons + two hairline dividers — keeps the stepper
        // stretchable for unified-width settings rows without changing
        // the dec/inc hit-area sizes.
        let value_w = (self.width - BTN_W * 2.0 - HAIRLINE * 2.0).max(0.0);

        // Vertical hairline between segments — gives the dec / value /
        // inc tri-cell the obvious "stepper buttons" silhouette so the
        // hit areas are visually discoverable, not just clickable rect
        // regions hiding inside an otherwise undivided box.
        let hairline = || div().w(HAIRLINE).h(ROW_H).bg(border_color);

        let dec_btn = div()
            .w(BTN_W)
            .h(ROW_H)
            .flex_none()
            .flex_row()
            .items_center()
            .justify_center()
            .hit_id(self.dec_hit_id)
            .cursor_pointer()
            .hover(|s| s.bg(hover_bg))
            .active(|s| s.bg(press_bg))
            .child(text("−").color(fg));

        let value_cell = div()
            .w(value_w)
            .h(ROW_H)
            .flex_none()
            .flex_row()
            .items_center()
            .justify_center()
            .child(text(self.value).color(fg));

        let inc_btn = div()
            .w(BTN_W)
            .h(ROW_H)
            .flex_none()
            .flex_row()
            .items_center()
            .justify_center()
            .hit_id(self.inc_hit_id)
            .cursor_pointer()
            .hover(|s| s.bg(hover_bg))
            .active(|s| s.bg(press_bg))
            .child(text("+").color(fg));

        div()
            .flex_none()
            .flex_row()
            .items_center()
            .w(self.width)
            .h(ROW_H)
            .bg(outer_bg)
            .border(1.0, border_color)
            .rounded(theme.radius.sm)
            .child(dec_btn)
            .child(hairline())
            .child(value_cell)
            .child(hairline())
            .child(inc_btn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ResolvedTheme;

    /// Build with no `.value()` call — must not panic on the
    /// `Into<SharedString>` path. Documents the builder default.
    #[test]
    fn default_value_does_not_panic() {
        let theme = ResolvedTheme::default();
        let _ = NumberField::new(1, 2).into_div(&theme);
    }

    /// Build with both buttons and a value — exercises the full
    /// fluent chain.
    #[test]
    fn value_builder_sets_field() {
        let nf = NumberField::new(10, 20).value("42");
        assert_eq!(nf.value.as_ref(), "42");
        assert_eq!(nf.dec_hit_id, 10);
        assert_eq!(nf.inc_hit_id, 20);
    }
}
