//! Additive paint-time offset for layered animations.
//!
//! `Offset` and [`super::UiElement::render_offset`] set up niri's
//! additive-offset animation pattern *for the chrome tree* (top bar,
//! side tab bar, hints bar, modals once they migrate). Concrete chrome
//! animations layered on top of this hook will land in follow-ups.
//!
//! **Out of scope for this layer**: the column-width animation glitch
//! surfaced in commit 06cf83d ("skip column-width animation in overview
//! mode to prevent panel resize glitch"). That animation lives in
//! `ciri-layout::workspace` (column geometry of terminal panes), not in
//! this chrome tree. Properly fixing it requires the same additive
//! pattern *inside* `Column`/`Tile`: keep the column at its target width
//! in layout state, expose an animation `render_offset()` per tile, and
//! have the renderer add it at paint time. That is a parallel refactor
//! of the layout crate, not a ciri/app change. See `niri/src/layout/
//! scrolling.rs:2376` for the working reference implementation.

use super::rect::UiRect;

/// Additive shift applied to an element's paint rect *without* touching
/// its layout rect. Used to layer animations on top of the computed
/// layout — mirrors niri's `render_offset()` pattern
/// (see `niri/src/layout/tile.rs:532–545`).
///
/// The rule is: **paint applies the offset, hit does not**. That way,
/// mid-animation a tab can slide visually from its old position to its
/// new one, but clicks go to where the tab logically *is*, not where it
/// happens to be on-screen this frame. This is the same isolation niri
/// uses for interactive-move offsets vs. window focus hit-testing.
///
/// Offsets compose by simple sum: a child's `render_offset` is added to
/// the slot its parent produced from layout; the parent may in turn sit
/// inside a slot that its own parent offset. Each level adds its own
/// contribution — never multiplies, never replaces.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Offset {
    pub dx: f32,
    pub dy: f32,
}

impl Offset {
    pub const ZERO: Offset = Offset { dx: 0.0, dy: 0.0 };

    #[allow(dead_code)] // constructor helper for future callers
    pub fn new(dx: f32, dy: f32) -> Self {
        Self { dx, dy }
    }

    #[inline]
    pub fn is_zero(&self) -> bool {
        self.dx == 0.0 && self.dy == 0.0
    }
}

impl UiRect {
    /// Shift the rect by `offset`. Does not resize.
    pub fn offset_by(self, o: Offset) -> UiRect {
        UiRect::new(self.x + o.dx, self.y + o.dy, self.w, self.h)
    }
}

#[cfg(test)]
#[path = "offset_tests.rs"]
mod tests;
