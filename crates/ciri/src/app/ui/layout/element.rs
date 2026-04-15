use super::super::types::{UiAction, UiContext, UiScene};
use super::hint::{Axis, SizeHint};
use super::offset::Offset;
use super::rect::UiRect;

/// A node in the UI tree.
///
/// Implementors are responsible for drawing *inside* a rect handed in by
/// the parent and for mapping clicks inside that rect to actions. They
/// must not reach for `cx.viewport_w` / `cx.viewport_h` to position
/// themselves — those dimensions describe the window, not the slot this
/// element was given.
pub(crate) trait UiElement {
    /// Size this element wants along `axis`. The parent combines the
    /// hints of its children to slice its own rect.
    fn size_hint(&self, axis: Axis, cx: &UiContext<'_>) -> SizeHint;

    /// Draw into `rect`. Implementors must not draw outside `rect`.
    fn paint(&self, rect: UiRect, cx: &UiContext<'_>, scene: &mut UiScene<'_>);

    /// Map a mouse click at `(mx, my)` (in screen coords) to an action.
    /// The default ignores clicks — override only for interactive elements.
    ///
    /// `rect` is the element's *layout* rect (no `render_offset` applied).
    /// This is deliberate: mid-animation clicks always go to where the
    /// element logically sits, not to the interpolated visual position.
    #[allow(dead_code)]
    fn hit(
        &self,
        rect: UiRect,
        mx: f32,
        my: f32,
        cx: &UiContext<'_>,
    ) -> Option<UiAction> {
        let _ = (rect, mx, my, cx);
        None
    }

    /// Per-frame paint-time offset applied on top of the layout rect.
    /// Default is zero — only animated elements override.
    ///
    /// See the doc on [`Offset`] for the composition rule. Parents must
    /// apply *child* offsets in their own `paint` (containers do this
    /// automatically for their children; leaf elements only need to
    /// override this method if they want to be offset by their parent).
    fn render_offset(&self) -> Offset {
        Offset::ZERO
    }
}
