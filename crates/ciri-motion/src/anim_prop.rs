//! `AnimProp<T>` — a single animated property slot.
//!
//! Holds the currently displayed value, an optional in-flight animation,
//! and an optional weak reference to a shared [`Ticker`]. When present,
//! the ticker is notified on every state transition so the host can
//! decide whether another frame is needed without walking the element
//! tree to poll `is_animating()` per-node.
//!
//! Used by `ciri-ui`'s `AnimatedStyle` to interpolate between old and new
//! style-property targets over time.

use std::sync::Weak;

use crate::{Lerp, Ticker, Transition};

/// One property's in-flight animation state.
#[derive(Clone, Copy, Debug)]
pub struct InFlight<T: Lerp> {
    pub from: T,
    pub to: T,
    pub transition: Transition,
    pub elapsed: f64,
}

/// A property that can be animated between values.
///
/// Not `Copy` (the `Weak<Ticker>` isn't) but [`Clone`]. The counter is
/// balanced per-prop: each prop calls `wake` at most once at a time and
/// the matching `sleep` is guaranteed by either a state transition or
/// `Drop`. Cloning a prop that is **currently animating** issues an
/// extra `wake` on the shared ticker so the clone's eventual `Drop` /
/// settle has a matching `sleep` — otherwise one drop could drive the
/// ticker to zero while the other clone is still in flight, and the
/// host would stop scheduling redraws.
#[derive(Debug, Default)]
pub struct AnimProp<T: Lerp> {
    current: T,
    anim: Option<InFlight<T>>,
    ticker: Option<Weak<Ticker>>,
}

impl<T: Lerp> Clone for AnimProp<T> {
    fn clone(&self) -> Self {
        let cloned = Self {
            current: self.current,
            anim: self.anim,
            ticker: self.ticker.clone(),
        };
        if cloned.anim.is_some() {
            // Balance the Drop-side sleep the clone is now promising.
            // If the upgrade fails (host already tore down the ticker),
            // there's nothing to balance — matches the original's
            // best-effort wake_ticker behaviour.
            cloned.wake_ticker();
        }
        cloned
    }
}

impl<T: Lerp> AnimProp<T> {
    pub fn new(v: T) -> Self {
        Self {
            current: v,
            anim: None,
            ticker: None,
        }
    }

    /// Attach a shared ticker. Returns self for builder-style chaining.
    ///
    /// When a ticker is attached, every `animate_to` / `advance` /
    /// `jump_to` call that changes `is_animating()` calls `wake` or
    /// `sleep` on it, so `ticker.is_animating()` is a correct global
    /// redraw predicate.
    pub fn with_ticker(mut self, ticker: &std::sync::Arc<Ticker>) -> Self {
        self.ticker = Some(std::sync::Arc::downgrade(ticker));
        self
    }

    /// Attach (or replace) the ticker on an existing prop.
    pub fn set_ticker(&mut self, ticker: &std::sync::Arc<Ticker>) {
        // Sleep the old ticker if we were mid-animation on it, so its
        // counter doesn't leak when the tickers switch.
        if self.anim.is_some() {
            self.sleep_ticker();
        }
        self.ticker = Some(std::sync::Arc::downgrade(ticker));
        if self.anim.is_some() {
            self.wake_ticker();
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
        let was_animating = self.anim.is_some();
        self.current = v;
        self.anim = None;
        if was_animating {
            self.sleep_ticker();
        }
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
    ///
    /// Note on `Transition::Spring`: progress here tracks the
    /// *decay envelope* (`1 − exp(−β·t)`), not the raw damped-oscillator
    /// position, so even underdamped presets (`SpringParams::bouncy()`)
    /// interpolate monotonically from `from` to `to` in value space.
    /// Visual overshoot must come from the values themselves (e.g. a
    /// translate target sequenced past the rest point and back) — the
    /// scalar progress delivered by this API does not overshoot 1.0.
    pub fn animate_to(&mut self, target: T, transition: Transition) {
        if self.target() == target {
            return;
        }
        if transition.is_settled(0.0) {
            self.jump_to(target);
            return;
        }
        let was_animating = self.anim.is_some();
        // Preserve `elapsed` when retargeting a spring with identical
        // params so a hover that toggles on/off rapidly doesn't restart
        // the decay envelope from zero on every toggle. `from` still
        // updates to the current displayed value, so visual continuity
        // holds. Timed transitions always reset — a tween retarget that
        // inherited elapsed would skip part of its easing curve.
        let elapsed = match (self.anim, transition) {
            (Some(prev), Transition::Spring(new_params))
                if matches!(prev.transition, Transition::Spring(p) if p == new_params) =>
            {
                prev.elapsed
            }
            _ => 0.0,
        };
        self.anim = Some(InFlight {
            from: self.current,
            to: target,
            transition,
            elapsed,
        });
        if !was_animating {
            self.wake_ticker();
        }
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
    ///
    /// Negative `dt` is clamped to zero — a debugger-paused frame or a
    /// clock-skew glitch must not roll `elapsed` backwards (which would
    /// snap the displayed value back toward `from`).
    pub fn advance(&mut self, dt: f64) -> bool {
        let dt = dt.max(0.0);
        let Some(mut a) = self.anim else {
            return false;
        };
        a.elapsed += dt;
        let t = a.transition.eval(a.elapsed);
        self.current = T::lerp(a.from, a.to, t);
        // Bump the ticker's frame counter so any `ui_scene_hash` that
        // folds it in invalidates on animated frames. Callers that don't
        // care pay nothing (`frame()` is never read).
        self.bump_ticker_frame();
        if a.transition.is_settled(a.elapsed) {
            self.current = a.to;
            self.anim = None;
            self.sleep_ticker();
            false
        } else {
            self.anim = Some(a);
            true
        }
    }

    fn wake_ticker(&self) {
        if let Some(w) = &self.ticker
            && let Some(t) = w.upgrade()
        {
            t.wake();
        }
    }

    fn sleep_ticker(&self) {
        if let Some(w) = &self.ticker
            && let Some(t) = w.upgrade()
        {
            t.sleep();
        }
    }

    fn bump_ticker_frame(&self) {
        if let Some(w) = &self.ticker
            && let Some(t) = w.upgrade()
        {
            t.bump_frame();
        }
    }
}

impl<T: Lerp> Drop for AnimProp<T> {
    /// If we were animating when dropped, balance the ticker counter. A
    /// sleep without a matching wake is benign (the ticker saturates at
    /// zero), but leaking a wake would freeze `is_animating()` high and
    /// force permanent redraws.
    fn drop(&mut self) {
        if self.anim.is_some() {
            self.sleep_ticker();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EasingCurve;
    use std::sync::Arc;

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

    /// Regression for Codex P2: without ticker wake-up on animate_to, the
    /// host's redraw predicate would never observe a newly-started
    /// animation, so transitions freeze after the first frame.
    #[test]
    fn animate_to_wakes_ticker() {
        let ticker = Arc::new(Ticker::new());
        let mut p = AnimProp::new(0.0_f32).with_ticker(&ticker);
        assert_eq!(ticker.active(), 0);
        assert!(!ticker.is_animating());

        p.animate_to(10.0, linear(1.0));
        assert_eq!(ticker.active(), 1);
        assert!(ticker.is_animating());
    }

    #[test]
    fn advance_to_settle_sleeps_ticker() {
        let ticker = Arc::new(Ticker::new());
        let mut p = AnimProp::new(0.0_f32).with_ticker(&ticker);
        p.animate_to(10.0, linear(1.0));
        assert_eq!(ticker.active(), 1);
        assert!(p.advance(0.5));
        assert_eq!(ticker.active(), 1, "mid-flight should keep wake");
        assert!(!p.advance(0.6));
        assert_eq!(ticker.active(), 0);
        assert!(!ticker.is_animating());
    }

    #[test]
    fn jump_to_mid_flight_sleeps_ticker() {
        let ticker = Arc::new(Ticker::new());
        let mut p = AnimProp::new(0.0_f32).with_ticker(&ticker);
        p.animate_to(10.0, linear(1.0));
        p.jump_to(7.0);
        assert_eq!(ticker.active(), 0);
    }

    /// Zero-duration animate_to takes the synchronous snap path. It must
    /// not leave the ticker held awake.
    #[test]
    fn zero_duration_animate_does_not_leak_ticker_wake() {
        let ticker = Arc::new(Ticker::new());
        let mut p = AnimProp::new(0.0_f32).with_ticker(&ticker);
        p.animate_to(10.0, linear(0.0));
        assert_eq!(ticker.active(), 0);
    }

    /// Retargeting mid-flight (is_animating stays true) must not double-wake.
    #[test]
    fn retarget_keeps_ticker_balanced() {
        let ticker = Arc::new(Ticker::new());
        let mut p = AnimProp::new(0.0_f32).with_ticker(&ticker);
        p.animate_to(10.0, linear(1.0));
        assert_eq!(ticker.active(), 1);
        p.diff_and_animate(20.0, linear(1.0));
        assert_eq!(ticker.active(), 1, "retarget must not increment");
    }

    /// Dropping a prop mid-animation must release its ticker wake.
    #[test]
    fn drop_mid_flight_sleeps_ticker() {
        let ticker = Arc::new(Ticker::new());
        {
            let mut p = AnimProp::new(0.0_f32).with_ticker(&ticker);
            p.animate_to(10.0, linear(1.0));
            assert_eq!(ticker.active(), 1);
        }
        assert_eq!(ticker.active(), 0);
    }

    /// Regression for Codex P2: cloning an in-flight AnimProp must not
    /// leave the ticker counter unbalanced. If the naive `#[derive]`
    /// shallow-clone is in effect, the second drop drives the counter
    /// to zero while a live clone is still animating — the host stops
    /// scheduling redraws and the other clone later underflows.
    #[test]
    fn clone_mid_flight_wakes_ticker_once_per_clone() {
        let ticker = Arc::new(Ticker::new());
        let mut p = AnimProp::new(0.0_f32).with_ticker(&ticker);
        p.animate_to(10.0, linear(1.0));
        assert_eq!(ticker.active(), 1);
        let q = p.clone();
        assert_eq!(ticker.active(), 2, "clone of animating prop must wake");
        drop(p);
        assert_eq!(ticker.active(), 1, "one drop, one sleep");
        assert!(ticker.is_animating(), "clone still animating");
        drop(q);
        assert_eq!(ticker.active(), 0);
    }

    /// A clone of a settled prop must not wake the ticker.
    #[test]
    fn clone_of_idle_does_not_wake() {
        let ticker = Arc::new(Ticker::new());
        let p = AnimProp::new(0.0_f32).with_ticker(&ticker);
        let _q = p.clone();
        assert_eq!(ticker.active(), 0);
    }

    /// A negative `dt` (clock skew, paused debugger) must not roll
    /// elapsed backwards and snap the value toward `from`.
    #[test]
    fn negative_dt_is_clamped() {
        let mut p = AnimProp::new(0.0_f32);
        p.animate_to(10.0, linear(1.0));
        assert!(p.advance(0.5));
        let mid = p.current();
        assert!(p.advance(-5.0));
        // Value held; negative dt didn't rewind the animation.
        assert_eq!(p.current(), mid);
    }

    /// `advance` bumps the shared ticker's frame counter on every frame
    /// where it actually moves a value, so `ui_scene_hash` consumers can
    /// fold `ticker.frame()` in to invalidate cached scenes during
    /// animation. Settled / idle props must not bump.
    #[test]
    fn advance_bumps_ticker_frame_while_animating() {
        let ticker = Arc::new(Ticker::new());
        let mut p = AnimProp::new(0.0_f32).with_ticker(&ticker);
        let before = ticker.frame();

        // Idle prop must not bump.
        assert!(!p.advance(0.016));
        assert_eq!(ticker.frame(), before, "idle advance must not bump frame");

        p.animate_to(10.0, linear(1.0));
        assert!(p.advance(0.1));
        let after_tick = ticker.frame();
        assert!(after_tick > before, "mid-flight advance bumps frame");

        // Settle: one more bump on the step that settles, then none.
        assert!(!p.advance(2.0));
        let after_settle = ticker.frame();
        assert!(after_settle > after_tick);

        // Post-settle advance stays quiet — cache hits must resume.
        assert!(!p.advance(0.016));
        assert_eq!(ticker.frame(), after_settle);
    }
}
