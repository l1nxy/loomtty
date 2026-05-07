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

const TOTAL_W: f32 = 96.0;
const ROW_H: f32 = 24.0;
const BTN_W: f32 = 24.0;
const VALUE_W: f32 = TOTAL_W - BTN_W * 2.0;

pub struct NumberField {
    dec_hit_id: u64,
    inc_hit_id: u64,
    value: SharedString,
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
        }
    }

    /// Set the value display string. Caller is responsible for
    /// formatting (decimal places, units, etc.).
    pub fn value(mut self, value: impl Into<SharedString>) -> Self {
        self.value = value.into();
        self
    }

    pub fn into_div(self, theme: &ResolvedTheme) -> Div {
        let outer_bg = theme.surface_sunken;
        let border_color = theme.border;
        let fg = theme.on_surface;

        let dec_btn = div()
            .w(BTN_W)
            .h(ROW_H)
            .flex_row()
            .items_center()
            .justify_center()
            .hit_id(self.dec_hit_id)
            .child(text("−").color(fg));

        let value_cell = div()
            .w(VALUE_W)
            .h(ROW_H)
            .flex_row()
            .items_center()
            .justify_center()
            .child(text(self.value).color(fg));

        let inc_btn = div()
            .w(BTN_W)
            .h(ROW_H)
            .flex_row()
            .items_center()
            .justify_center()
            .hit_id(self.inc_hit_id)
            .child(text("+").color(fg));

        div()
            .flex_row()
            .items_center()
            .w(TOTAL_W)
            .h(ROW_H)
            .bg(outer_bg)
            .border(1.0, border_color)
            .rounded(theme.radius.sm)
            .child(dec_btn)
            .child(value_cell)
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
