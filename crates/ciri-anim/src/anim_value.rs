use crate::easing::EasingCurve;
use crate::spring::{Spring, SpringParams};

/// Animation mode: spring physics, time-based easing, or deceleration.
#[derive(Debug, Clone)]
enum AnimMode {
    /// Spring physics (supports all damping regimes).
    Spring {
        spring: Spring,
        elapsed: f64,
        /// Cached clamped_duration (computed once on creation).
        clamped_dur: Option<f64>,
    },
    /// Time-based easing (fixed duration, predictable).
    Easing {
        from: f64,
        to: f64,
        elapsed: f64,
        duration: f64,
        curve: EasingCurve,
    },
    /// Deceleration (inertial scrolling after gesture release).
    Deceleration {
        from: f64,
        velocity: f64,
        decel_rate: f64,
        elapsed: f64,
        duration: f64,
    },
}

/// Gesture tracking state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GestureState {
    /// No gesture active.
    None,
    /// User is actively dragging.
    Active,
}

/// A single animated scalar value.
///
/// Unified primitive for all animations in the system. Supports:
/// - Spring physics (for view scrolling, focus transitions, zoom)
/// - Easing curves (for pane open/close, flash effects)
/// - Deceleration (for inertial scrolling after gesture release)
/// - Gesture tracking (for trackpad scrolling)
#[derive(Debug, Clone)]
pub struct AnimValue {
    position: f64,
    velocity: f64,
    target: f64,
    mode: Option<AnimMode>,
    gesture: GestureState,
}

impl AnimValue {
    /// Create an idle value at the given position.
    pub fn new(initial: f64) -> Self {
        Self {
            position: initial,
            velocity: 0.0,
            target: initial,
            mode: None,
            gesture: GestureState::None,
        }
    }

    /// Create a spring-based animation starting at `from`, targeting `to`.
    pub fn spring(from: f64, to: f64, params: SpringParams) -> Self {
        let spring = Spring {
            from,
            to,
            initial_velocity: 0.0,
            params,
        };
        let clamped_dur = spring.clamped_duration();
        Self {
            position: from,
            velocity: 0.0,
            target: to,
            mode: Some(AnimMode::Spring {
                spring,
                elapsed: 0.0,
                clamped_dur,
            }),
            gesture: GestureState::None,
        }
    }

    /// Create a time-based easing animation.
    pub fn eased(from: f64, to: f64, duration_secs: f64, curve: EasingCurve) -> Self {
        Self {
            position: from,
            velocity: 0.0,
            target: to,
            mode: Some(AnimMode::Easing {
                from,
                to,
                elapsed: 0.0,
                duration: duration_secs,
                curve,
            }),
            gesture: GestureState::None,
        }
    }

    // ── Getters ──

    pub fn value(&self) -> f64 {
        self.position
    }

    /// Returns a value clamped at the target after first reaching it.
    /// Useful for bouncy springs where you don't want overshoot in rendering.
    /// Also clamps intermediate values to stay between from and to.
    pub fn clamped_value(&self) -> f64 {
        if let Some(AnimMode::Spring {
            spring,
            elapsed,
            clamped_dur,
        }) = &self.mode
        {
            if let Some(cd) = clamped_dur {
                if *elapsed >= *cd {
                    return self.target;
                }
            }
            // Clamp to [min(from,to), max(from,to)] to prevent overshoot artifacts
            let lo = spring.from.min(spring.to);
            let hi = spring.from.max(spring.to);
            return self.position.clamp(lo, hi);
        }
        self.position
    }

    pub fn target(&self) -> f64 {
        self.target
    }

    pub fn is_animating(&self) -> bool {
        self.mode.is_some()
    }

    pub fn is_gesture(&self) -> bool {
        self.gesture == GestureState::Active
    }

    // ── Spring transitions ──

    /// Start a spring animation to the target value.
    /// Preserves current velocity for smooth interruption.
    pub fn animate_to(&mut self, target: f64, params: SpringParams) {
        let spring = Spring {
            from: self.position,
            to: target,
            initial_velocity: self.velocity,
            params,
        };
        let clamped_dur = spring.clamped_duration();
        self.target = target;
        self.mode = Some(AnimMode::Spring {
            spring,
            elapsed: 0.0,
            clamped_dur,
        });
        self.gesture = GestureState::None;
    }

    /// Jump to a value instantly, canceling any animation.
    pub fn jump_to(&mut self, value: f64) {
        self.position = value;
        self.velocity = 0.0;
        self.target = value;
        self.mode = None;
        self.gesture = GestureState::None;
    }

    // ── Easing transitions ──

    /// Start an easing animation from current value to target.
    pub fn ease_to(&mut self, target: f64, duration_secs: f64, curve: EasingCurve) {
        let from = self.position;
        self.target = target;
        self.mode = Some(AnimMode::Easing {
            from,
            to: target,
            elapsed: 0.0,
            duration: duration_secs,
            curve,
        });
        self.gesture = GestureState::None;
    }

    // ── Deceleration ──

    /// Start a deceleration animation from current position with initial velocity.
    /// Used for inertial scrolling after gesture release.
    ///
    /// `decel_rate`: deceleration rate (e.g. 0.998), must be in (0, 1).
    /// `threshold`: velocity threshold to stop (e.g. 0.001).
    pub fn decelerate(&mut self, velocity: f64, decel_rate: f64, threshold: f64) {
        let from = self.position;

        let (duration, to) = if velocity == 0.0 {
            (0.0, from)
        } else {
            let coeff = 1000.0 * decel_rate.ln();
            let dur = ((-coeff * threshold / velocity.abs()).ln() / coeff).max(0.0);
            let to = from - velocity / coeff;
            (dur, to)
        };

        self.target = to;
        self.mode = Some(AnimMode::Deceleration {
            from,
            velocity,
            decel_rate,
            elapsed: 0.0,
            duration,
        });
        self.gesture = GestureState::None;
    }

    // ── Gesture support ──

    /// Begin a gesture, capturing current animated position.
    pub fn begin_gesture(&mut self) {
        // position is already up-to-date from last advance() tick
        self.velocity = 0.0;
        self.target = self.position;
        self.mode = None;
        self.gesture = GestureState::Active;
    }

    /// Update gesture position (clamped to >= 0).
    pub fn update_gesture(&mut self, delta: f64) {
        if self.gesture == GestureState::Active {
            self.position = (self.position - delta).max(0.0);
            self.target = self.position;
        }
    }

    /// Update gesture position without clamping (allows negative).
    pub fn update_gesture_unclamped(&mut self, delta: f64) {
        if self.gesture == GestureState::Active {
            self.position -= delta;
            self.target = self.position;
        }
    }

    /// End gesture, spring to target position.
    pub fn end_gesture(&mut self, target: f64, params: SpringParams) {
        self.gesture = GestureState::None;
        self.animate_to(target, params);
    }

    // ── Tick ──

    /// Advance the animation by `dt` seconds. Returns true if still running.
    pub fn advance(&mut self, dt: f64) -> bool {
        if dt <= 0.0 {
            return self.mode.is_some();
        }

        let finished = match &mut self.mode {
            None => return false,
            Some(AnimMode::Spring { spring, elapsed, .. }) => {
                *elapsed += dt;
                let t = *elapsed;

                self.position = spring.value_at(t);
                self.velocity = spring.velocity_at(t);

                // Check convergence
                let dur = spring.duration();
                t >= dur
            }
            Some(AnimMode::Easing {
                from,
                to,
                elapsed,
                duration,
                curve,
            }) => {
                *elapsed += dt;
                let t = if *duration > 0.0 {
                    (*elapsed / *duration).min(1.0)
                } else {
                    1.0
                };
                let eased = curve.apply(t);
                self.position = *from + (*to - *from) * eased;
                t >= 1.0
            }
            Some(AnimMode::Deceleration {
                from,
                velocity,
                decel_rate,
                elapsed,
                duration,
            }) => {
                *elapsed += dt;
                let t = *elapsed;
                let coeff = 1000.0 * decel_rate.ln();
                self.position = *from + (decel_rate.powf(1000.0 * t) - 1.0) / coeff * *velocity;
                t >= *duration
            }
        };

        if finished {
            self.position = self.target;
            self.velocity = 0.0;
            self.mode = None;
        }

        !finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_idle() {
        let v = AnimValue::new(42.0);
        assert_eq!(v.value(), 42.0);
        assert_eq!(v.target(), 42.0);
        assert!(!v.is_animating());
    }

    #[test]
    fn spring_converges() {
        let mut v = AnimValue::spring(0.0, 100.0, SpringParams::snappy());
        assert!(v.is_animating());
        for _ in 0..300 {
            v.advance(1.0 / 60.0);
        }
        assert!(!v.is_animating());
        assert!((v.value() - 100.0).abs() < 0.5);
    }

    #[test]
    fn spring_interrupt_preserves_velocity() {
        let mut v = AnimValue::new(0.0);
        v.animate_to(100.0, SpringParams::snappy());
        v.advance(0.05);
        let mid = v.value();
        assert!(mid > 0.0 && mid < 100.0);

        // Re-target while in flight
        v.animate_to(200.0, SpringParams::snappy());
        assert!(v.is_animating());
        assert_eq!(v.target(), 200.0);
    }

    #[test]
    fn bouncy_spring_overshoots() {
        let mut v = AnimValue::spring(0.0, 100.0, SpringParams::bouncy());
        let mut max = 0.0f64;
        for _ in 0..300 {
            v.advance(1.0 / 60.0);
            max = max.max(v.value());
        }
        assert!(max > 100.5, "bouncy should overshoot: max={max}");
    }

    #[test]
    fn clamped_value_stops_at_target() {
        let mut v = AnimValue::spring(0.0, 100.0, SpringParams::bouncy());
        // Advance past clamped duration
        for _ in 0..300 {
            v.advance(1.0 / 60.0);
            let clamped = v.clamped_value();
            // clamped_value should never exceed target for positive direction
            assert!(
                clamped <= 100.01,
                "clamped_value should stop at target: {clamped}"
            );
        }
    }

    #[test]
    fn eased_completes_in_duration() {
        let mut v = AnimValue::eased(0.0, 1.0, 0.2, EasingCurve::Linear);
        assert!(v.is_animating());

        for _ in 0..10 {
            v.advance(1.0 / 60.0);
        }
        assert!(v.is_animating());
        assert!(v.value() > 0.5);

        for _ in 0..5 {
            v.advance(1.0 / 60.0);
        }
        assert!(!v.is_animating());
        assert_eq!(v.value(), 1.0);
    }

    #[test]
    fn ease_out_cubic_starts_fast() {
        let mut v = AnimValue::eased(0.0, 1.0, 1.0, EasingCurve::EaseOutCubic);
        v.advance(0.25);
        assert!(v.value() > 0.5);
    }

    #[test]
    fn jump_to_cancels_animation() {
        let mut v = AnimValue::spring(0.0, 100.0, SpringParams::snappy());
        v.advance(0.01);
        v.jump_to(50.0);
        assert!(!v.is_animating());
        assert_eq!(v.value(), 50.0);
    }

    #[test]
    fn gesture_cycle() {
        let mut v = AnimValue::new(100.0);
        v.begin_gesture();
        assert!(v.is_gesture());

        v.update_gesture(-20.0);
        assert!((v.value() - 120.0).abs() < 0.01);

        v.end_gesture(100.0, SpringParams::snappy());
        assert!(v.is_animating());
        assert!(!v.is_gesture());

        for _ in 0..300 {
            v.advance(1.0 / 60.0);
        }
        assert!(!v.is_animating());
        assert!((v.value() - 100.0).abs() < 1.0);
    }

    #[test]
    fn gesture_unclamped_allows_negative() {
        let mut v = AnimValue::new(0.0);
        v.begin_gesture();
        v.update_gesture_unclamped(10.0);
        assert_eq!(v.value(), -10.0);
    }

    #[test]
    fn ease_to_from_current() {
        let mut v = AnimValue::new(5.0);
        v.ease_to(10.0, 0.1, EasingCurve::Linear);
        assert!(v.is_animating());
        assert_eq!(v.target(), 10.0);

        for _ in 0..10 {
            v.advance(1.0 / 60.0);
        }
        assert!(!v.is_animating());
        assert_eq!(v.value(), 10.0);
    }

    #[test]
    fn advance_zero_dt_is_noop() {
        let mut v = AnimValue::spring(0.0, 100.0, SpringParams::snappy());
        let pos_before = v.value();
        v.advance(0.0);
        assert_eq!(v.value(), pos_before);
        assert!(v.is_animating());
    }

    #[test]
    fn deceleration_basic() {
        let mut v = AnimValue::new(0.0);
        v.decelerate(100.0, 0.998, 0.001);
        assert!(v.is_animating());

        // Should move away from starting position
        v.advance(0.01);
        assert!(
            (v.value() - 0.0).abs() > 0.001,
            "should move: val={}",
            v.value()
        );

        // Should eventually stop
        for _ in 0..600 {
            v.advance(1.0 / 60.0);
        }
        assert!(!v.is_animating());
    }

    #[test]
    fn deceleration_zero_velocity() {
        let mut v = AnimValue::new(50.0);
        v.decelerate(0.0, 0.998, 0.001);
        // Zero velocity = instant completion
        v.advance(0.01);
        assert!(!v.is_animating());
        assert_eq!(v.value(), 50.0);
    }

    #[test]
    fn from_omega_backward_compat() {
        let params = SpringParams::from_omega(12.0, 0.1);
        let mut v = AnimValue::spring(0.0, 100.0, params);
        for _ in 0..300 {
            v.advance(1.0 / 60.0);
        }
        assert!(!v.is_animating());
        assert!((v.value() - 100.0).abs() < 0.5);
    }
}
