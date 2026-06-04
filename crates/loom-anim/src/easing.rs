//! Easing curves for time-based animations.

use crate::bezier::CubicBezier;

/// Easing curve for time-based animations.
#[derive(Debug, Clone, Copy)]
pub enum EasingCurve {
    Linear,
    EaseOutQuad,
    EaseOutCubic,
    EaseOutExpo,
    CubicBezier(CubicBezier),
}

impl EasingCurve {
    pub fn apply(self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::EaseOutQuad => {
                let inv = 1.0 - t;
                1.0 - inv * inv
            }
            Self::EaseOutCubic => 1.0 - (1.0 - t).powi(3),
            Self::EaseOutExpo => {
                if t >= 1.0 {
                    1.0
                } else {
                    1.0 - 2.0_f64.powf(-10.0 * t)
                }
            }
            Self::CubicBezier(b) => b.y(t),
        }
    }
}

// Allow comparing curves for effect dedup
impl PartialEq for EasingCurve {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Linear, Self::Linear)
            | (Self::EaseOutQuad, Self::EaseOutQuad)
            | (Self::EaseOutCubic, Self::EaseOutCubic)
            | (Self::EaseOutExpo, Self::EaseOutExpo) => true,
            (Self::CubicBezier(a), Self::CubicBezier(b)) => {
                a.control_points() == b.control_points()
            }
            _ => false,
        }
    }
}

impl Eq for EasingCurve {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_identity() {
        for i in 0..=10 {
            let t = i as f64 / 10.0;
            assert!((EasingCurve::Linear.apply(t) - t).abs() < 1e-10);
        }
    }

    #[test]
    fn ease_out_quad_starts_fast() {
        assert!(EasingCurve::EaseOutQuad.apply(0.25) > 0.25);
    }

    #[test]
    fn ease_out_cubic_starts_fast() {
        assert!(EasingCurve::EaseOutCubic.apply(0.25) > 0.5);
    }

    #[test]
    fn ease_out_expo_starts_fast() {
        assert!(EasingCurve::EaseOutExpo.apply(0.25) > 0.5);
    }

    #[test]
    fn all_curves_clamp() {
        for curve in [
            EasingCurve::Linear,
            EasingCurve::EaseOutQuad,
            EasingCurve::EaseOutCubic,
            EasingCurve::EaseOutExpo,
        ] {
            assert!((curve.apply(0.0) - 0.0).abs() < 1e-10);
            assert!((curve.apply(1.0) - 1.0).abs() < 1e-10);
            assert!((curve.apply(-0.5) - 0.0).abs() < 1e-10);
            assert!((curve.apply(1.5) - 1.0).abs() < 1e-10);
        }
    }
}
