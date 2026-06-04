//! `Deferred` — escape-hatch z-order primitive.
//!
//! Wraps a single child whose paint the walker delays until the main
//! tree walk completes. Deferred children are then painted in
//! ascending [`Element::deferred_priority`] order, so they sit on top
//! of non-deferred siblings regardless of tree position.
//!
//! Use this for popovers, modals, tooltips and overlays that should
//! float above the chrome they are syntactically nested within. It
//! replaces the fixed `Layer` enum's role for ad-hoc z-ordering: a
//! deferred subtree wins over normal paint without the host having
//! to pre-classify the widget into a coarse semantic layer.
//!
//! Layout still happens in-place — Taffy lays out the child as an
//! ordinary descendant of the deferred wrapper. Only paint is moved.
//! That matches GPUI's `deferred(child)` element, on which this is
//! modelled.

use crate::element::{AnyElement, Element, IntoElement, PaintCtx};
use smallvec::SmallVec;

/// Build a deferred wrapper around `child`. Default priority `0`; chain
/// `.priority(n)` to raise it above other deferred siblings.
pub fn deferred(child: impl IntoElement) -> Deferred {
    Deferred {
        child: SmallVec::from_buf([AnyElement::new(child.into_element())]),
        priority: 0,
    }
}

/// Element wrapper whose child paints after the rest of the frame.
/// See module docs for semantics.
pub struct Deferred {
    /// Single-child storage. `SmallVec<[…; 1]>` keeps the child inline
    /// so creating a deferred wrapper costs no heap allocation beyond
    /// the arena slot the child already occupies.
    child: SmallVec<[AnyElement; 1]>,
    priority: u32,
}

impl Deferred {
    /// Set the deferred-paint priority. Higher values draw on top of
    /// lower-priority deferred siblings (drained later).
    pub fn priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }
}

impl IntoElement for Deferred {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Deferred {
    fn taffy_style(&self) -> taffy::Style {
        // Wrapper is layout-transparent: child decides its own size and
        // position via its own style. Default `Style` lets the child's
        // `position: absolute` (or whatever) flow through unchanged.
        taffy::Style::default()
    }

    fn children(&self) -> &[AnyElement] {
        &self.child
    }

    fn type_id(&self) -> &'static str {
        "loom.deferred"
    }

    fn is_deferred(&self) -> bool {
        true
    }

    fn deferred_priority(&self) -> u32 {
        self.priority
    }

    fn paint(&self, _cx: &mut PaintCtx<'_>) {
        // No own primitives; the child paints during drain.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::div;

    #[test]
    fn factory_starts_at_priority_zero() {
        let d = deferred(div());
        assert!(d.is_deferred());
        assert_eq!(d.deferred_priority(), 0);
    }

    #[test]
    fn priority_builder_overrides() {
        let d = deferred(div()).priority(7);
        assert_eq!(d.deferred_priority(), 7);
    }

    #[test]
    fn child_count_is_exactly_one() {
        let d = deferred(div().child("inside"));
        assert_eq!(d.children().len(), 1);
    }
}
