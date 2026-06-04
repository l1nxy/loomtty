//! Dropdown trigger — current selection + chevron, opens a popup.
//!
//! Trigger only. The popup itself reuses the existing `context_menu`
//! widget anchored below the trigger; the parent panel manages
//! open/closed state and renders the menu when open. Settings rows
//! that need a single-select enum (theme preset, leader key, etc.)
//! pair this trigger with a `context_menu` popup driven from their
//! local state.
//!
//! ```ignore
//! use loom_ui::Dropdown;
//!
//! const HIT_THEME_PRESET: u64 = 1200;
//!
//! Dropdown::new(HIT_THEME_PRESET)
//!     .value(config.theme.preset.clone())
//!     .width(180.0)
//!     .into_div(cx.theme)
//! ```

use crate::elements::{Div, div, text};
use crate::shared_string::SharedString;
use crate::styled::Styled;
use crate::theme::ResolvedTheme;

const DEFAULT_W: f32 = 160.0;
// Row height tracks NumberField::ROW_H so dropdowns and steppers
// share a control-class baseline and align vertically when they
// share a settings row. 48 px is a generous "primary action button"
// height — the trigger reads as a proper button (not a list cell)
// and the value text gets vertical breathing room.
const ROW_H: f32 = 48.0;
const PAD_X: f32 = 12.0;

pub struct Dropdown {
    hit_id: u64,
    value: SharedString,
    width: f32,
}

impl Dropdown {
    /// Build a Dropdown trigger with a hit id for the open / toggle
    /// click. The parent panel maps this id to "open the popup for
    /// this dropdown" in its action handler.
    pub fn new(hit_id: u64) -> Self {
        Self {
            hit_id,
            value: SharedString::default(),
            width: DEFAULT_W,
        }
    }

    /// Set the displayed value (current selection).
    pub fn value(mut self, value: impl Into<SharedString>) -> Self {
        self.value = value.into();
        self
    }

    /// Override the trigger width (defaults to 160 px).
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn into_div(self, theme: &ResolvedTheme) -> Div {
        // Reserve room for the chevron + its left gap so the value
        // text never collides with it. The value text gets the
        // remaining width; loom-ui doesn't auto-truncate yet so
        // callers should pre-truncate / size the dropdown for the
        // longest expected option.
        let chevron_col_w = ROW_H;
        let value_col_w = (self.width - PAD_X * 2.0 - chevron_col_w).max(0.0);
        div()
            .w(self.width)
            .h(ROW_H)
            // Pin the trigger size against flex shrink in the parent —
            // settings rows use justify_between() and would otherwise
            // collapse the dropdown if its content was narrower than
            // expected, leaving only a sliver of clickable area.
            .flex_none()
            .flex_row()
            .items_center()
            .justify_between()
            .px(PAD_X)
            .bg(theme.surface_sunken)
            .border(1.0, theme.border)
            .rounded(theme.radius.sm)
            .hit_id(self.hit_id)
            .cursor_pointer()
            .hover(|s| s.bg(theme.element_hover))
            .active(|s| s.bg(theme.element_active))
            // Value column reserves explicit width so glyphs can't
            // overrun the chevron's space at long preset names.
            .child(
                div()
                    .w(value_col_w)
                    .h(ROW_H)
                    .flex_row()
                    .items_center()
                    .child(text(self.value).color(theme.on_surface)),
            )
            // U+25BE BLACK DOWN-POINTING SMALL TRIANGLE — works in any
            // monospace fallback chain, no SVG icon needed for v1.
            .child(
                div()
                    .w(chevron_col_w)
                    .h(ROW_H)
                    .flex_row()
                    .items_center()
                    .justify_end()
                    .child(text("▾").color(theme.on_surface_muted)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ResolvedTheme;

    #[test]
    fn default_width_is_documented_constant() {
        let d = Dropdown::new(1);
        assert_eq!(d.width, DEFAULT_W);
    }

    #[test]
    fn width_builder_overrides_default() {
        let d = Dropdown::new(1).width(240.0);
        assert_eq!(d.width, 240.0);
    }

    #[test]
    fn value_builder_sets_field() {
        let d = Dropdown::new(1).value("dracula");
        assert_eq!(d.value.as_ref(), "dracula");
    }

    /// Build with no value — must not panic on `Into<SharedString>`.
    #[test]
    fn default_value_does_not_panic() {
        let theme = ResolvedTheme::default();
        let _ = Dropdown::new(1).into_div(&theme);
    }
}
