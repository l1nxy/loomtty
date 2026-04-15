use super::super::types::{UiContext, UiScene};
use super::element::UiElement;
use super::hint::{Axis, SizeHint};
use super::rect::UiRect;

/// A no-op placeholder that fills whatever slot its parent gives it
/// without painting anything.
///
/// Useful when a `Linear` child slot needs to reserve space but the
/// actual content lives elsewhere (e.g. the top bar's tab area when
/// tabs have been extracted to a dedicated side bar). Keeping the slot
/// present — instead of just dropping the child — preserves the
/// positions of the sibling slots so that e.g. the mode badge stays
/// right-anchored regardless of whether tabs are integrated.
pub(crate) struct Spacer;

impl UiElement for Spacer {
    fn size_hint(&self, _axis: Axis, _cx: &UiContext<'_>) -> SizeHint {
        SizeHint::Fill
    }
    fn paint(&self, _rect: UiRect, _cx: &UiContext<'_>, _scene: &mut UiScene<'_>) {}
}
