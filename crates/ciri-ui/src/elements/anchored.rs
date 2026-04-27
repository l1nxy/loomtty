//! `Anchored` — viewport-aware popover positioning.
//!
//! Wraps a single child and pins it to an anchor point + corner. After
//! Taffy lays the child out, the walker's drain pass computes the
//! child's screen position by combining the anchor with the child's
//! measured size, edge-flipping when the preferred corner would push
//! the child off the viewport. The result keeps the cursor / anchor
//! at one of the child's corners — far more aesthetic than clamping
//! the child back into view (which can leave the cursor inside the
//! child rather than at its edge).
//!
//! Modelled on GPUI's `anchored` element. Z-order semantics are
//! identical to [`crate::elements::Deferred`] — the child paints
//! after every non-deferred sibling, on the same queue, sorted by
//! priority. Anchored is essentially "deferred + reposition".

use crate::element::{AnchorPlacement, AnchorCorner, AnyElement, Element, IntoElement, PaintCtx};
use smallvec::SmallVec;

/// Build an anchored wrapper around `child` at the given anchor point,
/// with a preferred attachment corner. The child is positioned so that
/// `corner` of the child sits at `point`. If that placement would
/// push the child off the viewport, the drain edge-flips to the
/// opposite corner along the offending axis.
pub fn anchored(child: impl IntoElement, point: [f32; 2], corner: AnchorCorner) -> Anchored {
    Anchored {
        child: SmallVec::from_buf([AnyElement::new(child.into_element())]),
        placement: AnchorPlacement { point, corner },
        priority: 0,
    }
}

/// Element wrapper whose child paint is queued + re-positioned after
/// the main walk. See module docs for semantics.
pub struct Anchored {
    child: SmallVec<[AnyElement; 1]>,
    placement: AnchorPlacement,
    priority: u32,
}

impl Anchored {
    /// Set the deferred-paint priority. Same shape as
    /// [`crate::elements::Deferred::priority`] — higher draws later
    /// (on top). Default `0`.
    pub fn priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }
}

impl IntoElement for Anchored {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Anchored {
    fn taffy_style(&self) -> taffy::Style {
        // Layout-transparent like Deferred — the child sizes itself
        // from its own style. Anchored doesn't need taffy bounds; it
        // computes paint position from the child's measured size at
        // drain time.
        taffy::Style::default()
    }

    fn children(&self) -> &[AnyElement] {
        &self.child
    }

    fn type_id(&self) -> &'static str {
        "ciri.anchored"
    }

    fn is_deferred(&self) -> bool {
        true
    }

    fn deferred_priority(&self) -> u32 {
        self.priority
    }

    fn anchor_placement(&self) -> Option<AnchorPlacement> {
        Some(self.placement)
    }

    fn paint(&self, _cx: &mut PaintCtx<'_>) {
        // No own primitives — child paints during drain, repositioned.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::div;

    #[test]
    fn factory_starts_at_priority_zero_with_top_left() {
        let a = anchored(div(), [10.0, 20.0], AnchorCorner::TopLeft);
        assert!(a.is_deferred());
        assert_eq!(a.deferred_priority(), 0);
        assert_eq!(
            a.anchor_placement(),
            Some(AnchorPlacement {
                point: [10.0, 20.0],
                corner: AnchorCorner::TopLeft,
            })
        );
    }

    #[test]
    fn priority_builder_overrides() {
        let a = anchored(div(), [0.0, 0.0], AnchorCorner::TopLeft).priority(5);
        assert_eq!(a.deferred_priority(), 5);
    }

    #[test]
    fn child_count_is_exactly_one() {
        let a = anchored(div().child("inside"), [0.0, 0.0], AnchorCorner::TopLeft);
        assert_eq!(a.children().len(), 1);
    }
}
