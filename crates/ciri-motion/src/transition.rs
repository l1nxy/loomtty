//! Descriptor for how a single style property should animate between its
//! old and new target values. Either time-based easing or spring physics.

use crate::{EasingCurve, Spring, SpringParams};

/// How a property should animate to a new value.
#[derive(Clone, Copy, Debug)]
pub enum Transition {
    /// Fixed-duration tween with an easing curve.
    Timed {
        duration_secs: f64,
        curve: EasingCurve,
    },
    /// Spring physics. Normalised envelope (from=0, to=1) drives `[0, 1]` progress.
    Spring(SpringParams),
}

impl Transition {
    /// 150ms ease-out cubic — snappy default for hover highlights.
    pub const fn fast() -> Self {
        Self::Timed {
            duration_secs: 0.15,
            curve: EasingCurve::EaseOutCubic,
        }
    }

    /// 250ms ease-out cubic — mid-tempo default for panel fades.
    pub const fn medium() -> Self {
        Self::Timed {
            duration_secs: 0.25,
            curve: EasingCurve::EaseOutCubic,
        }
    }

    /// Evaluate normalised progress at `elapsed` seconds; clamped to `[0, 1]`.
    ///
    /// For springs, progress is derived by evaluating a normalized spring
    /// (from=0 → to=1) at `elapsed` and clamping overshoot. This trades a
    /// small amount of perceived bounce for monotonic, bounded progress.
    pub fn eval(&self, elapsed: f64) -> f32 {
        match self {
            Self::Timed {
                duration_secs,
                curve,
            } => {
                if *duration_secs <= 0.0 {
                    return 1.0;
                }
                let raw = (elapsed / *duration_secs).clamp(0.0, 1.0);
                curve.apply(raw) as f32
            }
            Self::Spring(params) => {
                let s = Spring {
                    from: 0.0,
                    to: 1.0,
                    initial_velocity: 0.0,
                    params: *params,
                };
                s.value_at(elapsed).clamp(0.0, 1.0) as f32
            }
        }
    }

    /// Whether the transition has settled at `elapsed`.
    pub fn is_settled(&self, elapsed: f64) -> bool {
        match self {
            Self::Timed { duration_secs, .. } => elapsed >= *duration_secs,
            Self::Spring(params) => {
                let s = Spring {
                    from: 0.0,
                    to: 1.0,
                    initial_velocity: 0.0,
                    params: *params,
                };
                (1.0 - s.value_at(elapsed)).abs() <= params.epsilon
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timed_endpoints() {
        let t = Transition::Timed {
            duration_secs: 1.0,
            curve: EasingCurve::Linear,
        };
        assert!((t.eval(0.0) - 0.0).abs() < 1e-6);
        assert!((t.eval(1.0) - 1.0).abs() < 1e-6);
        assert!(t.is_settled(1.0));
        assert!(!t.is_settled(0.5));
    }

    #[test]
    fn zero_duration_snaps() {
        let t = Transition::Timed {
            duration_secs: 0.0,
            curve: EasingCurve::Linear,
        };
        assert_eq!(t.eval(0.0), 1.0);
    }

    #[test]
    fn spring_progresses() {
        let t = Transition::Spring(SpringParams::snappy());
        assert!(t.eval(0.001) >= 0.0);
        assert!(t.eval(5.0) > 0.99);
    }
}
