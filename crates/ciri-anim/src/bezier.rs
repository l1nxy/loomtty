/// Cubic Bézier curve for custom easing.
///
/// Implements the CSS `cubic-bezier(x1, y1, x2, y2)` function.
/// Based on libadwaita's implementation (LGPL-2.1-or-later).
#[derive(Debug, Clone, Copy)]
pub struct CubicBezier {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

impl CubicBezier {
    pub fn new(x1: f64, y1: f64, x2: f64, y2: f64) -> Self {
        // Clamp x values to [0,1] per CSS spec to ensure monotonic x(t)
        Self {
            x1: x1.clamp(0.0, 1.0),
            y1,
            x2: x2.clamp(0.0, 1.0),
            y2,
        }
    }

    /// Returns control points as bits for equality comparison.
    pub fn control_points(&self) -> [u64; 4] {
        [
            self.x1.to_bits(),
            self.y1.to_bits(),
            self.x2.to_bits(),
            self.y2.to_bits(),
        ]
    }

    /// Evaluate the curve: given progress `x` in [0, 1], return eased value `y`.
    pub fn y(&self, x: f64) -> f64 {
        if x <= f64::EPSILON {
            return 0.0;
        }
        if 1.0 - f64::EPSILON <= x {
            return 1.0;
        }
        self.y_for_t(self.t_for_x(x))
    }

    fn x_for_t(&self, t: f64) -> f64 {
        let omt = 1.0 - t;
        3.0 * omt * omt * t * self.x1 + 3.0 * omt * t * t * self.x2 + t * t * t
    }

    fn y_for_t(&self, t: f64) -> f64 {
        let omt = 1.0 - t;
        3.0 * omt * omt * t * self.y1 + 3.0 * omt * t * t * self.y2 + t * t * t
    }

    /// Binary search to find `t` such that `x_for_t(t) ≈ x`.
    fn t_for_x(&self, x: f64) -> f64 {
        let mut lo = 0.0;
        let mut hi = 1.0;

        for _ in 0..=30 {
            let mid = (lo + hi) / 2.0;
            if x < self.x_for_t(mid) {
                hi = mid;
            } else {
                lo = mid;
            }
        }

        (lo + hi) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints() {
        let b = CubicBezier::new(0.25, 0.1, 0.25, 1.0);
        assert!((b.y(0.0) - 0.0).abs() < 1e-10);
        assert!((b.y(1.0) - 1.0).abs() < 1e-10);
    }

    #[test]
    fn linear_bezier() {
        // cubic-bezier(0, 0, 1, 1) ≈ linear
        let b = CubicBezier::new(0.0, 0.0, 1.0, 1.0);
        for i in 0..=10 {
            let x = i as f64 / 10.0;
            assert!((b.y(x) - x).abs() < 0.01, "x={x}, y={}", b.y(x));
        }
    }

    #[test]
    fn ease_out_shape() {
        // CSS ease-out: cubic-bezier(0, 0, 0.58, 1)
        let b = CubicBezier::new(0.0, 0.0, 0.58, 1.0);
        // Should be above linear at midpoint (starts fast)
        assert!(b.y(0.5) > 0.5);
    }

    #[test]
    fn monotonic() {
        let b = CubicBezier::new(0.4, 0.0, 0.2, 1.0);
        let mut prev = 0.0;
        for i in 1..=100 {
            let x = i as f64 / 100.0;
            let y = b.y(x);
            assert!(y >= prev - 1e-10, "non-monotonic at x={x}");
            prev = y;
        }
    }
}
