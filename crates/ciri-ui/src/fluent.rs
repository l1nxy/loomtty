//! `FluentBuilder` — chainable conditional helpers for element builders.
//!
//! Builders that return `Self` from every method (e.g. [`crate::Div`])
//! play poorly with `if` blocks. Without these helpers, every
//! conditional chain breaks into a `let mut` rebind:
//!
//! ```ignore
//! let mut row = div().w(rect.w);
//! if active { row = row.bg(theme.accent); }
//! if let Some(label) = label { row = row.child(label); }
//! ```
//!
//! With them, the chain stays intact:
//!
//! ```ignore
//! div().w(rect.w)
//!     .when(active, |d| d.bg(theme.accent))
//!     .when_some(label, |d, l| d.child(l))
//! ```
//!
//! The blanket impl is gated on [`IntoElement`] so the methods don't
//! shadow `Iterator::map` and similar on unrelated types — same pattern
//! GPUI uses to keep the surface scoped to UI builders.

use crate::element::IntoElement;

pub trait FluentBuilder: Sized {
    /// Apply `then(self)` if `cond` is true; otherwise return `self`
    /// unchanged. Useful for state-conditional styles.
    #[inline]
    fn when(self, cond: bool, then: impl FnOnce(Self) -> Self) -> Self {
        if cond {
            then(self)
        } else {
            self
        }
    }

    /// Apply `then(self, value)` if `option` is `Some(value)`; otherwise
    /// return `self` unchanged. Useful for optional children / styles
    /// that only exist sometimes.
    #[inline]
    fn when_some<T>(self, option: Option<T>, then: impl FnOnce(Self, T) -> Self) -> Self {
        match option {
            Some(v) => then(self, v),
            None => self,
        }
    }

    /// Apply `then(self)` if `option` is `None`; otherwise return `self`
    /// unchanged. The mirror of [`when_some`] for fall-through cases.
    #[inline]
    fn when_none<T>(self, option: &Option<T>, then: impl FnOnce(Self) -> Self) -> Self {
        if option.is_none() {
            then(self)
        } else {
            self
        }
    }

    /// Pipe `self` through an arbitrary closure. Lets a builder branch
    /// or transform at a point in the chain without breaking it. Returns
    /// whatever the closure returns.
    #[inline]
    fn map<U>(self, f: impl FnOnce(Self) -> U) -> U {
        f(self)
    }
}

impl<T: IntoElement> FluentBuilder for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::div;
    use crate::styled::Styled;

    #[test]
    fn when_true_applies() {
        let d = div().w(100.0).when(true, |d| d.h(50.0));
        assert!((d.style_ref().height.is_some()));
    }

    #[test]
    fn when_false_skips() {
        let d = div().w(100.0).when(false, |d| d.h(50.0));
        assert!(d.style_ref().height.is_none());
    }

    #[test]
    fn when_some_unwraps() {
        let val: Option<f32> = Some(50.0);
        let d = div().w(100.0).when_some(val, |d, h| d.h(h));
        assert!(d.style_ref().height.is_some());
    }

    #[test]
    fn when_none_branch() {
        let val: Option<f32> = None;
        let d = div().w(100.0).when_none(&val, |d| d.h(50.0));
        assert!(d.style_ref().height.is_some());
    }
}
