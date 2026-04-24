use super::super::types::{UiAction, UiContext, UiScene};
use super::rect::UiRect;

/// A node in the UI tree.
///
/// Implementors are responsible for drawing *inside* a rect handed in by
/// the parent and for mapping clicks inside that rect to actions. They
/// must not reach for `cx.viewport_w` / `cx.viewport_h` to position
/// themselves — those dimensions describe the window, not the slot this
/// element was given.
pub(crate) trait UiElement {
    /// Draw into `rect`. Implementors must not draw outside `rect`.
    fn paint(&self, rect: UiRect, cx: &UiContext<'_>, scene: &mut UiScene<'_>);

    /// Map a mouse click at `(mx, my)` (in screen coords) to an action.
    /// The default ignores clicks — override only for interactive elements.
    ///
    /// `rect` is the element's layout rect.
    #[allow(dead_code)]
    fn hit(&self, rect: UiRect, mx: f32, my: f32, cx: &UiContext<'_>) -> Option<UiAction> {
        let _ = (rect, mx, my, cx);
        None
    }
}
