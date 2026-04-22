//! Global animation ticker.
//!
//! The UI layer advances every `AnimProp` it owns each frame; `Ticker`
//! tracks whether **any** property is still animating so the host app can
//! decide whether to schedule another redraw.
//!
//! Hooked into the existing frame path via `App::advance_animations`:
//! `is_animating |= ticker.pending();` before scheduling the next tick.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Thread-safe counter of currently-animating properties.
///
/// Increment via `wake()` when a property starts animating (in
/// `AnimProp::animate_to`); decrement via `sleep()` when it settles.
#[derive(Default, Debug)]
pub struct Ticker {
    active: AtomicUsize,
    frame: AtomicUsize,
}

impl Ticker {
    pub const fn new() -> Self {
        Self {
            active: AtomicUsize::new(0),
            frame: AtomicUsize::new(0),
        }
    }

    /// Register that one more property is animating.
    pub fn wake(&self) {
        self.active.fetch_add(1, Ordering::Relaxed);
    }

    /// Register that one property settled.
    ///
    /// Uses `fetch_update` so an imbalanced `sleep()` can never wrap to
    /// `usize::MAX` even transiently — another thread calling `is_animating()`
    /// between a naive `fetch_sub` and a compensating `store(0)` would
    /// briefly see a huge active count and mistakenly keep requesting
    /// redraws.
    pub fn sleep(&self) {
        let result = self.active.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |v| if v == 0 { None } else { Some(v - 1) },
        );
        if result.is_err() {
            // Imbalanced wake/sleep — a programming error. Surface in
            // release logs so it doesn't stay invisible.
            log::warn!("ciri-motion: Ticker::sleep called while active == 0");
        }
    }

    /// Monotonically bumped each frame that any property advanced. Consumers
    /// can fold this into UI-state hashes to invalidate cached scenes.
    pub fn bump_frame(&self) -> usize {
        self.frame.fetch_add(1, Ordering::Relaxed).wrapping_add(1)
    }

    pub fn frame(&self) -> usize {
        self.frame.load(Ordering::Relaxed)
    }

    /// True iff any registered property is still animating.
    pub fn is_animating(&self) -> bool {
        self.active.load(Ordering::Relaxed) > 0
    }

    /// Count of in-flight animations (diagnostic).
    pub fn active(&self) -> usize {
        self.active.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_sleep_balance() {
        let t = Ticker::new();
        assert!(!t.is_animating());
        t.wake();
        t.wake();
        assert_eq!(t.active(), 2);
        assert!(t.is_animating());
        t.sleep();
        t.sleep();
        assert!(!t.is_animating());
    }

    #[test]
    fn frame_is_monotonic() {
        let t = Ticker::new();
        let a = t.bump_frame();
        let b = t.bump_frame();
        assert!(b > a);
    }
}
