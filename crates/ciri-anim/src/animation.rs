use crate::spring::Spring;

/// Manages the view offset animation for horizontal scrolling.
#[derive(Debug)]
pub enum ViewOffset {
    /// No animation, static position.
    Static(f64),
    /// Animating via spring physics.
    Animating(Spring),
    /// User is gesturing (trackpad scroll).
    Gesture(f64),
}

impl Default for ViewOffset {
    fn default() -> Self {
        Self::new()
    }
}

impl ViewOffset {
    pub fn new() -> Self {
        ViewOffset::Static(0.0)
    }

    pub fn value(&self) -> f64 {
        match self {
            ViewOffset::Static(v) => *v,
            ViewOffset::Animating(s) => s.position,
            ViewOffset::Gesture(v) => *v,
        }
    }

    /// The target value (final destination of animation, or current value if static).
    pub fn target(&self) -> f64 {
        match self {
            ViewOffset::Static(v) => *v,
            ViewOffset::Animating(s) => s.target,
            ViewOffset::Gesture(v) => *v,
        }
    }

    /// Start animating to a target position.
    /// `omega`: angular frequency (8-15 recommended for smooth UI).
    pub fn animate_to(&mut self, target: f64, omega: f64) {
        let current = self.value();
        let mut spring = Spring::with_omega(omega);
        spring.position = current;
        spring.velocity = match self {
            ViewOffset::Animating(s) => s.velocity,
            _ => 0.0,
        };
        spring.set_target(target);
        *self = ViewOffset::Animating(spring);
    }

    /// Jump to a position without animation.
    pub fn jump_to(&mut self, value: f64) {
        *self = ViewOffset::Static(value);
    }

    /// Advance the animation by dt seconds. Returns true if still animating.
    pub fn advance(&mut self, dt: f64) -> bool {
        match self {
            ViewOffset::Animating(spring) => {
                spring.advance(dt);
                spring.settle();
                if spring.is_at_rest() {
                    let final_pos = spring.position;
                    *self = ViewOffset::Static(final_pos);
                    false
                } else {
                    true
                }
            }
            _ => false,
        }
    }

    pub fn is_animating(&self) -> bool {
        matches!(self, ViewOffset::Animating(_))
    }

    /// Begin a gesture (trackpad scroll).
    pub fn begin_gesture(&mut self) {
        let v = self.value();
        *self = ViewOffset::Gesture(v);
    }

    /// Update gesture position.
    pub fn update_gesture(&mut self, delta: f64) {
        if let ViewOffset::Gesture(v) = self {
            *v = (*v - delta).max(0.0);
        }
    }

    /// End gesture and snap to nearest target.
    pub fn end_gesture(&mut self, target: f64, stiffness: f64) {
        self.animate_to(target, stiffness);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_value() {
        let v = ViewOffset::new();
        assert_eq!(v.value(), 0.0);
        assert!(!v.is_animating());
    }

    #[test]
    fn animate_to_converges() {
        let mut v = ViewOffset::new();
        v.animate_to(100.0, 12.0);
        assert!(v.is_animating());
        for _ in 0..300 {
            v.advance(1.0 / 60.0);
        }
        assert!(!v.is_animating());
        assert!((v.value() - 100.0).abs() < 1.0);
    }

    #[test]
    fn jump_to() {
        let mut v = ViewOffset::new();
        v.jump_to(50.0);
        assert_eq!(v.value(), 50.0);
        assert!(!v.is_animating());
    }

    #[test]
    fn gesture_cycle() {
        let mut v = ViewOffset::new();
        v.jump_to(100.0);
        v.begin_gesture();
        v.update_gesture(-20.0); // scroll right
        assert!((v.value() - 120.0).abs() < 1.0);
        v.end_gesture(100.0, 400.0);
        assert!(v.is_animating());
    }

    #[test]
    fn animate_preserves_velocity() {
        let mut v = ViewOffset::new();
        v.animate_to(100.0, 12.0);
        v.advance(0.05);
        let mid = v.value();
        assert!(mid > 0.0 && mid < 100.0);
        // Re-target while in flight
        v.animate_to(200.0, 12.0);
        assert!(v.is_animating());
    }
}
