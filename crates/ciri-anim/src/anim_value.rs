use crate::easing;
use crate::spring::Spring;

/// Easing curve for time-based animations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EasingCurve {
    Linear,
    EaseOutCubic,
    EaseOutExpo,
}

impl EasingCurve {
    pub fn apply(self, t: f64) -> f64 {
        match self {
            EasingCurve::Linear => easing::linear(t),
            EasingCurve::EaseOutCubic => easing::ease_out_cubic(t),
            EasingCurve::EaseOutExpo => easing::ease_out_expo(t),
        }
    }
}

/// Animation mode: spring physics or time-based easing.
#[derive(Debug, Clone)]
enum AnimMode {
    /// Critically damped spring (interruptible, momentum-aware).
    Spring(Spring),
    /// Time-based easing (fixed duration, predictable).
    Easing {
        from: f64,
        to: f64,
        elapsed: f64,
        duration: f64,
        curve: EasingCurve,
    },
}

/// A single animated scalar value.
///
/// Unified primitive for all animations in the system. Supports:
/// - Spring physics (for view scrolling, focus transitions, zoom)
/// - Easing curves (for pane open/close, flash effects)
/// - Gesture tracking (for trackpad scrolling)
#[derive(Debug, Clone)]
pub struct AnimValue {
    position: f64,
    velocity: f64,
    target: f64,
    mode: Option<AnimMode>,
    in_gesture: bool,
}

impl AnimValue {
    /// Create an idle value at the given position.
    pub fn new(initial: f64) -> Self {
        Self {
            position: initial,
            velocity: 0.0,
            target: initial,
            mode: None,
            in_gesture: false,
        }
    }

    /// Create a spring-based animation starting at `from`, targeting `to`.
    pub fn spring(from: f64, to: f64, omega: f64, epsilon: f64) -> Self {
        let mut spring = Spring::new(omega, epsilon);
        spring.position = from;
        spring.set_target(to);
        Self {
            position: from,
            velocity: 0.0,
            target: to,
            mode: Some(AnimMode::Spring(spring)),
            in_gesture: false,
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
            in_gesture: false,
        }
    }

    // ── Getters ──

    pub fn value(&self) -> f64 {
        self.position
    }

    pub fn target(&self) -> f64 {
        self.target
    }

    pub fn is_animating(&self) -> bool {
        self.mode.is_some()
    }

    pub fn is_gesture(&self) -> bool {
        self.in_gesture
    }

    // ── Spring transitions ──

    /// Start a spring animation to the target value.
    /// Preserves current velocity for smooth interruption.
    pub fn animate_to(&mut self, target: f64, omega: f64, epsilon: f64) {
        let mut spring = Spring::new(omega, epsilon);
        spring.position = self.position;
        spring.velocity = self.velocity;
        spring.set_target(target);
        self.target = target;
        self.mode = Some(AnimMode::Spring(spring));
        self.in_gesture = false;
    }

    /// Jump to a value instantly, canceling any animation.
    pub fn jump_to(&mut self, value: f64) {
        self.position = value;
        self.velocity = 0.0;
        self.target = value;
        self.mode = None;
        self.in_gesture = false;
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
        self.in_gesture = false;
    }

    // ── Gesture support ──

    /// Begin a gesture, capturing current animated position.
    pub fn begin_gesture(&mut self) {
        self.position = self.value();
        self.velocity = 0.0;
        self.target = self.position;
        self.mode = None;
        self.in_gesture = true;
    }

    /// Update gesture position (clamped to >= 0).
    pub fn update_gesture(&mut self, delta: f64) {
        if self.in_gesture {
            self.position = (self.position - delta).max(0.0);
            self.target = self.position;
        }
    }

    /// Update gesture position without clamping (allows negative).
    pub fn update_gesture_unclamped(&mut self, delta: f64) {
        if self.in_gesture {
            self.position -= delta;
            self.target = self.position;
        }
    }

    /// End gesture, spring to target position.
    pub fn end_gesture(&mut self, target: f64, omega: f64, epsilon: f64) {
        self.in_gesture = false;
        self.animate_to(target, omega, epsilon);
    }

    // ── Tick ──

    /// Advance the animation by `dt` seconds. Returns true if still running.
    pub fn advance(&mut self, dt: f64) -> bool {
        if dt <= 0.0 {
            return self.mode.is_some();
        }

        let finished = match &mut self.mode {
            None => return false,
            Some(AnimMode::Spring(spring)) => {
                spring.advance(dt);
                spring.settle();
                self.position = spring.position;
                self.velocity = spring.velocity;
                spring.is_at_rest()
            }
            Some(AnimMode::Easing {
                from,
                to,
                elapsed,
                duration,
                curve,
            }) => {
                *elapsed += dt;
                let t = (*elapsed / *duration).min(1.0);
                let eased = curve.apply(t);
                self.position = *from + (*to - *from) * eased;
                t >= 1.0
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
        let mut v = AnimValue::spring(0.0, 100.0, 12.0, 0.1);
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
        v.animate_to(100.0, 12.0, 0.1);
        v.advance(0.05);
        let mid = v.value();
        assert!(mid > 0.0 && mid < 100.0);

        // Re-target while in flight
        v.animate_to(200.0, 12.0, 0.1);
        assert!(v.is_animating());
        assert_eq!(v.target(), 200.0);
    }

    #[test]
    fn eased_completes_in_duration() {
        let mut v = AnimValue::eased(0.0, 1.0, 0.2, EasingCurve::Linear);
        assert!(v.is_animating());

        // 10 frames at 60fps = ~167ms, should not be done
        for _ in 0..10 {
            v.advance(1.0 / 60.0);
        }
        assert!(v.is_animating());
        assert!(v.value() > 0.5);

        // 5 more frames = ~250ms, should be done
        for _ in 0..5 {
            v.advance(1.0 / 60.0);
        }
        assert!(!v.is_animating());
        assert_eq!(v.value(), 1.0);
    }

    #[test]
    fn ease_out_cubic_starts_fast() {
        let mut v = AnimValue::eased(0.0, 1.0, 1.0, EasingCurve::EaseOutCubic);
        v.advance(0.25); // 25% of duration
        // ease_out_cubic at t=0.25 ≈ 0.578 — starts fast
        assert!(v.value() > 0.5);
    }

    #[test]
    fn jump_to_cancels_animation() {
        let mut v = AnimValue::spring(0.0, 100.0, 12.0, 0.1);
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

        v.update_gesture(-20.0); // scroll right
        assert!((v.value() - 120.0).abs() < 0.01);

        v.end_gesture(100.0, 12.0, 0.1);
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
        let mut v = AnimValue::spring(0.0, 100.0, 12.0, 0.1);
        let pos_before = v.value();
        v.advance(0.0);
        assert_eq!(v.value(), pos_before);
        assert!(v.is_animating());
    }
}
