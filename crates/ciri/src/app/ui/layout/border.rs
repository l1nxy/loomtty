use super::super::types::{UiAction, UiContext, UiScene};
use super::element::UiElement;
use super::hint::{Axis, SizeHint};
use super::rect::UiRect;

/// A "dock" container: zero or more fixed-size edges (top/bottom/left/right)
/// wrapping a single filler `center`.
///
/// Edges that are `None` contribute zero size. The center receives whatever
/// rect is left over after the edges have been subtracted. Edges are
/// resolved in the order top → bottom → left → right, so horizontal edges
/// span the full width and vertical edges only span the remaining height
/// (matches the CSS "dock panel" layout most users expect).
#[allow(dead_code)] // edges wired up incrementally in later steps
pub(crate) struct Border<'a> {
    pub top: Option<Box<dyn UiElement + 'a>>,
    pub bottom: Option<Box<dyn UiElement + 'a>>,
    pub left: Option<Box<dyn UiElement + 'a>>,
    pub right: Option<Box<dyn UiElement + 'a>>,
    pub center: Option<Box<dyn UiElement + 'a>>,
}

#[allow(dead_code)]
impl<'a> Border<'a> {
    pub fn new() -> Self {
        Self {
            top: None,
            bottom: None,
            left: None,
            right: None,
            center: None,
        }
    }

    pub fn top<E: UiElement + 'a>(mut self, e: E) -> Self {
        self.top = Some(Box::new(e));
        self
    }
    pub fn bottom<E: UiElement + 'a>(mut self, e: E) -> Self {
        self.bottom = Some(Box::new(e));
        self
    }
    #[allow(dead_code)]
    pub fn left<E: UiElement + 'a>(mut self, e: E) -> Self {
        self.left = Some(Box::new(e));
        self
    }
    #[allow(dead_code)]
    pub fn right<E: UiElement + 'a>(mut self, e: E) -> Self {
        self.right = Some(Box::new(e));
        self
    }
    #[allow(dead_code)] // wired in step 4 (terminal as center child)
    pub fn center<E: UiElement + 'a>(mut self, e: E) -> Self {
        self.center = Some(Box::new(e));
        self
    }

    /// Per-child paint rect (layout slot + each child's `render_offset`),
    /// shared between production `paint` and unit tests so that a
    /// regression in offset wiring fails the unit test, not just an
    /// integration test that may not exist. See [`super::Linear::child_paint_rects`]
    /// for the rationale.
    pub(super) fn child_paint_slots(&self, rect: UiRect, cx: &UiContext<'_>) -> BorderSlots {
        let slots = self.layout(rect, cx);
        let apply = |maybe: Option<&Box<dyn UiElement + 'a>>, slot: UiRect| {
            if let Some(e) = maybe {
                let off = e.render_offset();
                if off.is_zero() {
                    slot
                } else {
                    slot.offset_by(off)
                }
            } else {
                slot
            }
        };
        BorderSlots {
            top: apply(self.top.as_ref(), slots.top),
            bottom: apply(self.bottom.as_ref(), slots.bottom),
            left: apply(self.left.as_ref(), slots.left),
            right: apply(self.right.as_ref(), slots.right),
            center: apply(self.center.as_ref(), slots.center),
        }
    }

    /// Resolve the rect tree into `(top, bottom, left, right, center)`
    /// slots. Absent edges get `UiRect::ZERO` rects. `center` is always
    /// produced even if empty — consumers check `is_empty()`.
    ///
    /// Public so callers that need to know "where does the terminal go"
    /// can query the center rect without painting.
    pub fn layout(&self, rect: UiRect, cx: &UiContext<'_>) -> BorderSlots {
        let main_h = |e: &Box<dyn UiElement + 'a>| match e.size_hint(Axis::Vertical, cx) {
            SizeHint::Fixed(px) => px.max(0.0),
            SizeHint::Fill => 0.0, // edges are not allowed to Fill
        };
        let main_w = |e: &Box<dyn UiElement + 'a>| match e.size_hint(Axis::Horizontal, cx) {
            SizeHint::Fixed(px) => px.max(0.0),
            SizeHint::Fill => 0.0,
        };

        let mut remaining = rect;

        let top_rect = if let Some(e) = &self.top {
            let (top, rest) = remaining.split_top(main_h(e));
            remaining = rest;
            top
        } else {
            UiRect::ZERO
        };

        let bottom_rect = if let Some(e) = &self.bottom {
            let (rest, bot) = remaining.split_bottom(main_h(e));
            remaining = rest;
            bot
        } else {
            UiRect::ZERO
        };

        let left_rect = if let Some(e) = &self.left {
            let (left, rest) = remaining.split_left(main_w(e));
            remaining = rest;
            left
        } else {
            UiRect::ZERO
        };

        let right_rect = if let Some(e) = &self.right {
            let (rest, right) = remaining.split_right(main_w(e));
            remaining = rest;
            right
        } else {
            UiRect::ZERO
        };

        BorderSlots {
            top: top_rect,
            bottom: bottom_rect,
            left: left_rect,
            right: right_rect,
            center: remaining,
        }
    }
}

/// Result of [`Border::layout`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct BorderSlots {
    pub top: UiRect,
    pub bottom: UiRect,
    #[allow(dead_code)]
    pub left: UiRect,
    #[allow(dead_code)]
    pub right: UiRect,
    pub center: UiRect,
}

impl<'a> UiElement for Border<'a> {
    fn size_hint(&self, _axis: Axis, _cx: &UiContext<'_>) -> SizeHint {
        // Border always wants to fill whatever its parent gives it —
        // the whole point is wrapping a center area.
        SizeHint::Fill
    }

    fn paint(&self, rect: UiRect, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let slots = self.child_paint_slots(rect, cx);
        for (maybe_child, paint_rect) in [
            (self.top.as_ref(), slots.top),
            (self.bottom.as_ref(), slots.bottom),
            (self.left.as_ref(), slots.left),
            (self.right.as_ref(), slots.right),
            (self.center.as_ref(), slots.center),
        ] {
            if let Some(e) = maybe_child
                && !paint_rect.is_empty()
            {
                e.paint(paint_rect, cx, scene);
            }
        }
    }

    fn hit(&self, rect: UiRect, mx: f32, my: f32, cx: &UiContext<'_>) -> Option<UiAction> {
        if !rect.contains(mx, my) {
            return None;
        }
        let slots = self.layout(rect, cx);
        // Check edges first (they have higher z), then center.
        let candidates = [
            (self.top.as_ref(), slots.top),
            (self.bottom.as_ref(), slots.bottom),
            (self.left.as_ref(), slots.left),
            (self.right.as_ref(), slots.right),
            (self.center.as_ref(), slots.center),
        ];
        for (maybe_child, slot) in candidates {
            if let Some(child) = maybe_child
                && slot.contains(mx, my)
                && let Some(action) = child.hit(slot, mx, my, cx)
            {
                return Some(action);
            }
        }
        None
    }
}

#[cfg(test)]
#[path = "border_tests.rs"]
mod tests;
