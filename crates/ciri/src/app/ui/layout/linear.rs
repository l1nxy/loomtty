use super::super::types::{UiAction, UiContext, UiScene};
use super::element::UiElement;
use super::hint::{Axis, SizeHint};
use super::rect::UiRect;

/// A stack of children along one axis.
///
/// Each child carries a [`SizeHint`]. `Linear` distributes its rect by:
/// 1. Subtracting the sum of `Fixed` hints from the main-axis length.
/// 2. Splitting the remainder evenly between `Fill` children.
///
/// Cross-axis is always the parent rect's cross-axis.
pub(crate) struct Linear<'a> {
    axis: Axis,
    children: Vec<Box<dyn UiElement + 'a>>,
}

impl<'a> Linear<'a> {
    pub fn new(axis: Axis) -> Self {
        Self {
            axis,
            children: Vec::new(),
        }
    }

    pub fn push<E: UiElement + 'a>(mut self, child: E) -> Self {
        self.children.push(Box::new(child));
        self
    }

    /// Compute the per-child *paint* rect: layout slot + the child's
    /// own `render_offset`. Production `paint` calls this; tests can
    /// also call it without needing a `UiScene`/`GlyphCache`. Sharing
    /// the helper means a regression in offset wiring fails the unit
    /// test, not just an integration test that may not exist.
    pub(crate) fn child_paint_rects(
        &self,
        rect: UiRect,
        cx: &UiContext<'_>,
    ) -> Vec<UiRect> {
        let slots = self.layout(rect, cx);
        self.children
            .iter()
            .zip(slots)
            .map(|(child, slot)| {
                let off = child.render_offset();
                if off.is_zero() {
                    slot
                } else {
                    slot.offset_by(off)
                }
            })
            .collect()
    }

    /// Compute a rect per child given the parent rect.
    /// Returns a vector the same length as `children`.
    ///
    /// `pub(crate)` so sibling tests (e.g. `ui::top_bar::tests`) can
    /// exercise the slot-allocation logic on real rows without going
    /// through `paint`/`hit` (which would require a glyph cache).
    pub(crate) fn layout(&self, rect: UiRect, cx: &UiContext<'_>) -> Vec<UiRect> {
        let main_total = match self.axis {
            Axis::Horizontal => rect.w,
            Axis::Vertical => rect.h,
        };

        // Sum fixed; count fills.
        let mut fixed_sum = 0.0_f32;
        let mut fill_count = 0_u32;
        let mut hints = Vec::with_capacity(self.children.len());
        for child in &self.children {
            let h = child.size_hint(self.axis, cx);
            match h {
                SizeHint::Fixed(px) => fixed_sum += px.max(0.0),
                SizeHint::Fill => fill_count += 1,
            }
            hints.push(h);
        }

        let fill_budget = (main_total - fixed_sum).max(0.0);
        let fill_each = if fill_count > 0 {
            fill_budget / fill_count as f32
        } else {
            0.0
        };

        let mut out = Vec::with_capacity(self.children.len());
        let mut remaining = rect;
        for h in hints {
            let slice = match h {
                SizeHint::Fixed(px) => px.max(0.0),
                SizeHint::Fill => fill_each,
            };
            let (taken, rest) = match self.axis {
                Axis::Horizontal => remaining.split_left(slice),
                Axis::Vertical => remaining.split_top(slice),
            };
            out.push(taken);
            remaining = rest;
        }
        out
    }
}

impl<'a> UiElement for Linear<'a> {
    fn size_hint(&self, axis: Axis, cx: &UiContext<'_>) -> SizeHint {
        if axis == self.axis {
            // Sum along main axis. Any Fill child propagates Fill upward.
            let mut sum = 0.0;
            for child in &self.children {
                match child.size_hint(axis, cx) {
                    SizeHint::Fixed(px) => sum += px,
                    SizeHint::Fill => return SizeHint::Fill,
                }
            }
            SizeHint::Fixed(sum)
        } else {
            // Cross axis: take the max of fixed hints; Fill wins over Fixed.
            let mut max_fixed = 0.0_f32;
            let mut any_fill = false;
            for child in &self.children {
                match child.size_hint(axis, cx) {
                    SizeHint::Fixed(px) => max_fixed = max_fixed.max(px),
                    SizeHint::Fill => any_fill = true,
                }
            }
            if any_fill {
                SizeHint::Fill
            } else {
                SizeHint::Fixed(max_fixed)
            }
        }
    }

    fn paint(&self, rect: UiRect, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let rects = self.child_paint_rects(rect, cx);
        for (child, paint_rect) in self.children.iter().zip(rects) {
            if paint_rect.is_empty() {
                continue;
            }
            child.paint(paint_rect, cx, scene);
        }
    }

    fn hit(
        &self,
        rect: UiRect,
        mx: f32,
        my: f32,
        cx: &UiContext<'_>,
    ) -> Option<UiAction> {
        if !rect.contains(mx, my) {
            return None;
        }
        // `render_offset` intentionally does *not* enter hit testing:
        // clicks go to where the element logically sits in the layout,
        // not where it happens to be painted mid-animation. See the doc
        // on `Offset`.
        let slots = self.layout(rect, cx);
        for (child, slot) in self.children.iter().zip(slots) {
            if slot.contains(mx, my)
                && let Some(action) = child.hit(slot, mx, my, cx)
            {
                return Some(action);
            }
        }
        None
    }
}

#[cfg(test)]
#[path = "linear_tests.rs"]
mod tests;
