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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_contains_point_inside() {
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        assert!(r.contains(10.0, 20.0)); // top-left corner
        assert!(r.contains(50.0, 40.0)); // center
        assert!(r.contains(109.0, 69.0)); // near bottom-right
    }

    #[test]
    fn rect_contains_excludes_right_and_bottom_edge() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(!r.contains(10.0, 5.0)); // right edge excluded
        assert!(!r.contains(5.0, 10.0)); // bottom edge excluded
        assert!(!r.contains(-1.0, 5.0)); // left of rect
        assert!(!r.contains(5.0, -1.0)); // above rect
    }

    #[test]
    fn rect_intersects_overlapping() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 10.0, 10.0);
        assert!(a.intersects(&b));
        assert!(b.intersects(&a));
    }

    #[test]
    fn rect_intersects_touching_edges_not_overlapping() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(10.0, 0.0, 10.0, 10.0); // touching right edge
        assert!(!a.intersects(&b));
    }

    #[test]
    fn rect_intersects_completely_separate() {
        let a = Rect::new(0.0, 0.0, 5.0, 5.0);
        let b = Rect::new(100.0, 100.0, 5.0, 5.0);
        assert!(!a.intersects(&b));
    }

    #[test]
    fn rect_intersection_partial_overlap() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 10.0, 10.0);
        let i = a.intersection(&b).unwrap();
        assert_eq!(i.x, 5.0);
        assert_eq!(i.y, 5.0);
        assert_eq!(i.w, 5.0);
        assert_eq!(i.h, 5.0);
    }

    #[test]
    fn rect_intersection_contained() {
        let outer = Rect::new(0.0, 0.0, 100.0, 100.0);
        let inner = Rect::new(10.0, 10.0, 20.0, 20.0);
        let i = outer.intersection(&inner).unwrap();
        assert_eq!(i, inner);
    }

    #[test]
    fn rect_intersection_no_overlap_returns_none() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(20.0, 20.0, 10.0, 10.0);
        assert!(a.intersection(&b).is_none());
    }

    #[test]
    fn rect_intersection_touching_edge_returns_none() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(10.0, 0.0, 10.0, 10.0);
        assert!(a.intersection(&b).is_none());
    }
}
