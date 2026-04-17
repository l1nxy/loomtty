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
    /// Spring physics. Progress is the normalised *decay envelope*, not the
    /// raw damped-oscillator position, so `eval()` is monotonic even for
    /// underdamped presets that overshoot in value space. Visual "bounce"
    /// comes from applying the transition to values whose own dynamics
    /// express it (e.g. a translate that snaps past and returns in a
    /// timeline), not from this scalar progress.
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

    /// Evaluate normalised progress at `elapsed` seconds; always in `[0, 1]`
    /// and **monotonically non-decreasing**.
    ///
    /// For springs, progress tracks the decay envelope `1 − exp(−β·t)`
    /// rather than the raw oscillator position. That avoids the
    /// "hit target, undershoot, return" regression underdamped presets
    /// would otherwise exhibit when sampled by this API.
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
                spring_envelope_progress(*params, elapsed) as f32
            }
        }
    }

    /// Whether the transition has settled at `elapsed`.
    ///
    /// For springs this checks both that the position has reached `to`
    /// within `epsilon` **and** that the oscillator's velocity has decayed
    /// below `epsilon`. Under-damped springs cross their target long before
    /// settling; a position-only check would retire bouncy animations on
    /// the first crossing and drop the rest of the motion.
    pub fn is_settled(&self, elapsed: f64) -> bool {
        match self {
            Self::Timed { duration_secs, .. } => elapsed >= *duration_secs,
            Self::Spring(params) => {
                // Matches the envelope used by `eval()` — once the envelope
                // has decayed below epsilon the spring cannot produce
                // perceptibly different values.
                let beta = params.damping / (2.0 * params.mass);
                if beta <= 0.0 {
                    return false;
                }
                let envelope = (-beta * elapsed).exp();
                if envelope > params.epsilon {
                    return false;
                }
                // Belt-and-braces: confirm the oscillator itself is also at
                // rest so we never strand motion that an overshoot'd leave
                // outstanding. `velocity_at` is exact.
                let s = Spring {
                    from: 0.0,
                    to: 1.0,
                    initial_velocity: 0.0,
                    params: *params,
                };
                let pos_settled = (1.0 - s.value_at(elapsed)).abs() <= params.epsilon;
                let vel_settled = s.velocity_at(elapsed).abs() <= params.epsilon;
                pos_settled && vel_settled
            }
        }
    }
}

/// Decay envelope for a spring with `from=0, to=1, v0=0`, mapped into
/// `[0, 1]` progress space. Stateless, monotonic, and clamped — safe to
/// sample at arbitrary `elapsed` without tracking history.
fn spring_envelope_progress(params: SpringParams, elapsed: f64) -> f64 {
    if elapsed <= 0.0 {
        return 0.0;
    }
    let beta = params.damping / (2.0 * params.mass);
    if beta <= 0.0 {
        // Degenerate: no decay → permanently stalled at 0. Treat as instant
        // to avoid callers wedging forever.
        return 1.0;
    }
    (1.0 - (-beta * elapsed).exp()).clamp(0.0, 1.0)
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

    /// Regression: every underdamped preset must produce monotonically
    /// non-decreasing progress. Before the envelope-based rewrite, a
    /// `Spring` transition could hit 1.0 on overshoot, briefly stay there
    /// while clamped, then regress as the oscillator undershot.
    #[test]
    fn spring_progress_is_monotonic_for_all_presets() {
        let presets = [
            ("snappy", SpringParams::snappy()),
            ("comfortable", SpringParams::comfortable()),
            ("smooth", SpringParams::smooth()),
            ("gentle", SpringParams::gentle()),
            ("bouncy", SpringParams::bouncy()),
        ];
        for (name, params) in presets {
            let t = Transition::Spring(params);
            let mut prev = 0.0_f32;
            for i in 0..=600 {
                let elapsed = f64::from(i) * 0.005;
                let v = t.eval(elapsed);
                assert!(v >= 0.0 && v <= 1.0, "{name}: out of range at {elapsed}: {v}");
                assert!(
                    v + 1e-6 >= prev,
                    "{name}: regressed at {elapsed}: {prev} -> {v}"
                );
                prev = v;
            }
        }
    }

    /// Regression: bouncy springs cross their target long before they
    /// settle. A position-only `is_settled` check would terminate the
    /// animation on that first crossing; the real spring must still be
    /// considered in flight while its velocity is non-trivial.
    #[test]
    fn bouncy_spring_not_settled_at_first_target_crossing() {
        let params = SpringParams::bouncy();
        let t = Transition::Spring(params);
        let s = Spring {
            from: 0.0,
            to: 1.0,
            initial_velocity: 0.0,
            params,
        };
        // Find the first `elapsed` where the raw spring reaches 1.0
        // (i.e. the crossing where the position-only check would have
        // tripped). Scan at 1ms granularity.
        let mut crossing = None;
        for i in 1..=3000 {
            let elapsed = f64::from(i) * 0.001;
            if s.value_at(elapsed) >= 1.0 {
                crossing = Some(elapsed);
                break;
            }
        }
        let crossing = crossing.expect("bouncy spring must overshoot within 3s");
        assert!(
            !t.is_settled(crossing),
            "bouncy spring wrongly reported settled at position crossing t={crossing}"
        );
        // After long enough, it must settle.
        assert!(t.is_settled(5.0));
    }

    #[test]
    fn zero_duration_is_settled_at_zero() {
        let t = Transition::Timed {
            duration_secs: 0.0,
            curve: EasingCurve::Linear,
        };
        assert!(t.is_settled(0.0));
    }
}
