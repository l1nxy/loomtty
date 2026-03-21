/// A rectangle in pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Rect { x, y, w, h }
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.w
            && self.x + self.w > other.x
            && self.y < other.y + other.h
            && self.y + self.h > other.y
    }

    /// Returns the intersection of two rectangles, or `None` if they don't overlap.
    pub fn intersection(&self, clip: &Rect) -> Option<Rect> {
        let x0 = self.x.max(clip.x);
        let y0 = self.y.max(clip.y);
        let x1 = (self.x + self.w).min(clip.x + clip.w);
        let y1 = (self.y + self.h).min(clip.y + clip.h);
        if x0 < x1 && y0 < y1 {
            Some(Rect {
                x: x0,
                y: y0,
                w: x1 - x0,
                h: y1 - y0,
            })
        } else {
            None
        }
    }
}

/// Size of the viewport in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewSize {
    pub width: f32,
    pub height: f32,
}
