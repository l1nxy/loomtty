//! `AnimProp<T>` — a single animated property slot.
//!
//! Holds the currently displayed value plus an optional in-flight animation.
//! Used by `ciri-ui`'s `AnimatedStyle` to interpolate between old and new
//! style-property targets over time.

use crate::{Lerp, Transition};

/// One property's in-flight animation state.
#[derive(Clone, Copy, Debug)]
pub struct InFlight<T: Lerp> {
    pub from: T,
    pub to: T,
    pub transition: Transition,
    pub elapsed: f64,
}

/// A property that can be animated between values.
#[derive(Clone, Copy, Debug)]
pub struct AnimProp<T: Lerp> {
    current: T,
    anim: Option<InFlight<T>>,
}

impl<T: Lerp> AnimProp<T> {
    pub fn new(v: T) -> Self {
        Self {
            current: v,
            anim: None,
        }
    }

    /// Currently displayed value. Interpolated if an animation is in flight.
    pub fn current(&self) -> T {
        self.current
    }

    /// Target value, if an animation is in flight; otherwise `current()`.
    pub fn target(&self) -> T {
        match self.anim {
            Some(a) => a.to,
            None => self.current,
        }
    }

    /// True iff an animation is in flight.
    pub fn is_animating(&self) -> bool {
        self.anim.is_some()
    }

    /// Jump immediately to `v` — cancels any in-flight animation.
    pub fn jump_to(&mut self, v: T) {
        self.current = v;
        self.anim = None;
    }

    /// Start animating from the current displayed value to `target` using
    /// `transition`. If `target == target()` this is a no-op.
    ///
    /// A transition that is already settled at `elapsed = 0` (e.g. a
    /// `Timed` with `duration_secs == 0.0`, or a `Spring` with
    /// effectively-instant parameters) snaps synchronously: `current` is
    /// updated in place and no in-flight animation is recorded. This
    /// matches the caller's obvious intent ("no animation") and avoids a
    /// stale frame plus a sticky `is_animating()` flag until the next tick.
    pub fn animate_to(&mut self, target: T, transition: Transition) {
        if self.target() == target {
            return;
        }
        if transition.is_settled(0.0) {
            self.jump_to(target);
            return;
        }
        self.anim = Some(InFlight {
            from: self.current,
            to: target,
            transition,
            elapsed: 0.0,
        });
    }

    /// If `new_target` differs from the current `target()`, smoothly retarget
    /// the in-flight animation (or start one) with `transition`. Common
    /// entry point for CSS-style "auto-transition on style diff".
    pub fn diff_and_animate(&mut self, new_target: T, transition: Transition) {
        if self.target() != new_target {
            self.animate_to(new_target, transition);
        }
    }

    /// Advance the animation by `dt` seconds. Returns true if the property
    /// is still animating after this step.
    pub fn advance(&mut self, dt: f64) -> bool {
        let Some(mut a) = self.anim else {
            return false;
        };
        a.elapsed += dt;
        let t = a.transition.eval(a.elapsed);
        self.current = T::lerp(a.from, a.to, t);
        if a.transition.is_settled(a.elapsed) {
            self.current = a.to;
            self.anim = None;
            false
        } else {
            self.anim = Some(a);
            true
        }
    }
}

impl<T: Lerp + Default> Default for AnimProp<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EasingCurve;

    fn linear(dur: f64) -> Transition {
        Transition::Timed {
            duration_secs: dur,
            curve: EasingCurve::Linear,
        }
    }

    #[test]
    fn jump_cancels_animation() {
        let mut p = AnimProp::new(0.0_f32);
        p.animate_to(10.0, linear(1.0));
        assert!(p.is_animating());
        p.jump_to(5.0);
        assert!(!p.is_animating());
        assert_eq!(p.current(), 5.0);
    }

    #[test]
    fn advance_interpolates() {
        let mut p = AnimProp::new(0.0_f32);
        p.animate_to(10.0, linear(1.0));
        assert!(p.advance(0.5));
        assert!((p.current() - 5.0).abs() < 1e-3);
        assert!(!p.advance(0.6));
        assert!((p.current() - 10.0).abs() < 1e-6);
    }

    #[test]
    fn diff_retarget_mid_flight() {
        let mut p = AnimProp::new(0.0_f32);
        p.animate_to(10.0, linear(1.0));
        assert!(p.advance(0.5));
        let mid = p.current();
        // Mid-flight retarget — expected: new animation from current -> new_target
        p.diff_and_animate(20.0, linear(1.0));
        assert_eq!(p.target(), 20.0);
        // Halfway through the retarget: value has moved from `mid` toward 20.
        assert!(p.advance(0.5));
        assert!(
            p.current() > mid && p.current() < 20.0,
            "mid={mid} current={}",
            p.current()
        );
        // Finishing completes the second animation.
        assert!(!p.advance(0.6));
        assert!((p.current() - 20.0).abs() < 1e-3);
    }

    #[test]
    fn no_op_when_target_matches() {
        let mut p = AnimProp::new(7.0_f32);
        p.animate_to(7.0, linear(1.0));
        assert!(!p.is_animating());
    }

    #[test]
    fn zero_duration_snaps_synchronously() {
        let mut p = AnimProp::new(0.0_f32);
        p.animate_to(10.0, linear(0.0));
        // Caller's intent with a zero-duration transition is "no animation":
        // current must update immediately, and is_animating must stay false.
        assert_eq!(p.current(), 10.0);
        assert!(!p.is_animating());
        assert_eq!(p.target(), 10.0);
    }
}
