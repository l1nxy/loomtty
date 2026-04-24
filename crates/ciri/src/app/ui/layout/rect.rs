/// A pixel-aligned axis-aligned rectangle used to describe layout slots.
///
/// Kept separate from `ciri_render::rect::Rect` (which is a coloured GPU
/// quad) because this type has no colour and no side-effects — it is pure
/// geometry passed down the tree at paint time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct UiRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl UiRect {
    #[allow(dead_code)]
    pub const ZERO: UiRect = UiRect {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    };

    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    #[inline]
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    #[inline]
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    #[inline]
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    /// Slice a strip off the left edge; returns (taken, remaining).
    /// If `width >= self.w`, `remaining` is empty.
    pub fn split_left(self, width: f32) -> (UiRect, UiRect) {
        let w = width.clamp(0.0, self.w);
        (
            UiRect::new(self.x, self.y, w, self.h),
            UiRect::new(self.x + w, self.y, self.w - w, self.h),
        )
    }

    /// Slice a strip off the right edge; returns (remaining, taken).
    #[allow(dead_code)]
    pub fn split_right(self, width: f32) -> (UiRect, UiRect) {
        let w = width.clamp(0.0, self.w);
        (
            UiRect::new(self.x, self.y, self.w - w, self.h),
            UiRect::new(self.x + self.w - w, self.y, w, self.h),
        )
    }

    /// Slice a strip off the top edge; returns (taken, remaining).
    pub fn split_top(self, height: f32) -> (UiRect, UiRect) {
        let h = height.clamp(0.0, self.h);
        (
            UiRect::new(self.x, self.y, self.w, h),
            UiRect::new(self.x, self.y + h, self.w, self.h - h),
        )
    }

    /// Slice a strip off the bottom edge; returns (remaining, taken).
    #[allow(dead_code)]
    pub fn split_bottom(self, height: f32) -> (UiRect, UiRect) {
        let h = height.clamp(0.0, self.h);
        (
            UiRect::new(self.x, self.y, self.w, self.h - h),
            UiRect::new(self.x, self.y + self.h - h, self.w, h),
        )
    }
}

#[cfg(test)]
#[path = "rect_tests.rs"]
mod tests;
