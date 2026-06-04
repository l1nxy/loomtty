//! Switch — boolean toggle control.
//!
//! Stateless config: the `checked` value comes from the consumer's
//! data model and the click is dispatched by the consumer's
//! `hit_test → action` mapping (matching how palette / context_menu
//! handle clicks today; loom-ui's Element-level click walker is not
//! yet wired into the host dispatcher, see `Styled::on_click` doc).
//!
//! ```ignore
//! use loom_ui::Switch;
//!
//! const HIT_OVERVIEW_AT_STARTUP: u64 = 1001;
//!
//! Switch::new(HIT_OVERVIEW_AT_STARTUP)
//!     .checked(config.overview_on_startup)
//!     .into_div(cx.theme)
//! ```
//!
//! The parent panel pairs the `hit_id` with an action in its
//! `click(mx, my, cx) -> Option<Action>` handler.

use crate::elements::{Div, div};
use crate::styled::Styled;
use crate::theme::ResolvedTheme;

/// Logical track / thumb metrics. Picked to sit comfortably inside
/// the 48 px settings-panel control row — visually weighty enough to
/// match Dropdown / NumberField triggers without ballooning into a
/// "toggle the size of a button" caricature. Aspect ~1.86 matches
/// the iOS / Material switch family.
const TRACK_W: f32 = 52.0;
const TRACK_H: f32 = 28.0;
const THUMB_SIZE: f32 = 22.0;
const INNER_PAD: f32 = (TRACK_H - THUMB_SIZE) / 2.0;

pub struct Switch {
    hit_id: u64,
    checked: bool,
}

impl Switch {
    /// Build a Switch with the given hit-test id (used by the parent
    /// panel to map the click back to a typed action).
    pub fn new(hit_id: u64) -> Self {
        Self {
            hit_id,
            checked: false,
        }
    }

    /// Set the on/off state. Defaults to `false` (off).
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// Resolve to a `Div` subtree. The track holds the hit_id; the
    /// thumb is a child positioned absolutely so the track's bg fills
    /// the rounded shape and the thumb floats over it.
    pub fn into_div(self, theme: &ResolvedTheme) -> Div {
        let track_bg = if self.checked {
            theme.accent
        } else {
            theme.element_hover
        };
        let thumb_color = if self.checked {
            theme.on_accent
        } else {
            theme.on_surface_muted
        };
        let thumb_x = if self.checked {
            TRACK_W - THUMB_SIZE - INNER_PAD
        } else {
            INNER_PAD
        };

        div()
            .w(TRACK_W)
            .h(TRACK_H)
            .bg(track_bg)
            .rounded(TRACK_H / 2.0)
            .hit_id(self.hit_id)
            .child(
                div()
                    .absolute()
                    .left(thumb_x)
                    .top(INNER_PAD)
                    .w(THUMB_SIZE)
                    .h(THUMB_SIZE)
                    .bg(thumb_color)
                    .rounded(THUMB_SIZE / 2.0),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ResolvedTheme;

    /// A Switch's track bg must change with `checked`. Catches anyone
    /// dropping the conditional in `into_div` — without this assertion
    /// a copy-paste error could leave the toggle visually inert.
    ///
    /// Tested by inspecting the resolved style on the produced `Div`:
    /// can't see paint output from a unit test, but the bg field is
    /// exposed via the `Styled::style` accessor.
    #[test]
    fn checked_state_changes_track_bg() {
        let theme = ResolvedTheme::default();
        let on = Switch::new(1).checked(true).into_div(&theme);
        let off = Switch::new(1).checked(false).into_div(&theme);
        let on_bg = on.style_ref().background;
        let off_bg = off.style_ref().background;
        assert_ne!(
            on_bg, off_bg,
            "track bg must differ between checked / unchecked"
        );
    }

    /// Default `checked` is `false`. Pin so adding new builder fields
    /// can't accidentally flip the resting state.
    #[test]
    fn default_checked_is_false() {
        let theme = ResolvedTheme::default();
        let s = Switch::new(1).into_div(&theme);
        assert_eq!(s.style_ref().background, Some(theme.element_hover));
    }
}
