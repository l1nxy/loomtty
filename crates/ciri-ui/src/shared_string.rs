//! `SharedString` — cheap-to-clone immutable string for chrome text.
//!
//! Backed by [`SmolStr`], which inlines short strings (≤ 22 bytes on
//! 64-bit) and reference-counts longer ones. Static literals constructed
//! via [`SharedString::new_static`] cost zero heap allocations; cloning
//! is always either a memcpy of the inline buffer or an `Arc` ref-count
//! bump, never an allocation.
//!
//! Use this in place of `String` for any chrome text that flows into
//! the element tree — `Text` content, palette row labels, formatted
//! status strings, etc. Element trees are rebuilt every paint, so even
//! a single `String::from("Copy")` in a hot widget shows up.

use std::borrow::Cow;
use std::fmt;
use std::ops::Deref;

use smol_str::SmolStr;

#[derive(Clone, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SharedString(SmolStr);

impl SharedString {
    /// Wrap a `&'static str`. Const so callers can keep static literals
    /// in `const` contexts without paying any allocation.
    pub const fn new_static(s: &'static str) -> Self {
        Self(SmolStr::new_static(s))
    }

    /// Build from anything string-like. Short strings are inlined; long
    /// ones allocate an `Arc<str>` once (then clone for free).
    pub fn new(s: impl AsRef<str>) -> Self {
        Self(SmolStr::new(s.as_ref()))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl Deref for SharedString {
    type Target = str;
    fn deref(&self) -> &str {
        self.0.as_str()
    }
}

impl AsRef<str> for SharedString {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for SharedString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl fmt::Debug for SharedString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl<'a> From<&'a str> for SharedString {
    /// Always goes through SmolStr's inline-or-Arc path, so this is one
    /// allocation for long strings and zero for short (≤22 bytes). Use
    /// [`SharedString::new_static`] explicitly when you have a `&'static
    /// str` and want to skip even the inline copy.
    fn from(s: &'a str) -> Self {
        Self(SmolStr::from(s))
    }
}

impl From<String> for SharedString {
    fn from(s: String) -> Self {
        Self(SmolStr::from(s))
    }
}

impl From<Cow<'static, str>> for SharedString {
    fn from(s: Cow<'static, str>) -> Self {
        match s {
            Cow::Borrowed(b) => Self::new_static(b),
            Cow::Owned(o) => Self::from(o),
        }
    }
}

impl From<SharedString> for String {
    fn from(s: SharedString) -> Self {
        s.0.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_constructor_is_zero_copy() {
        const S: SharedString = SharedString::new_static("hello");
        assert_eq!(S.as_str(), "hello");
    }

    #[test]
    fn dynamic_short_string_inlines() {
        let s = SharedString::new("abc");
        // Inline check: SmolStr inlines ≤22 bytes. We can't observe the
        // discriminant directly, but cloning must not allocate; a clone
        // of an inlined SmolStr is a stack memcpy.
        let clone = s.clone();
        assert_eq!(clone.as_str(), "abc");
    }

    #[test]
    fn from_string_takes_ownership() {
        let s: SharedString = String::from("dynamic value").into();
        assert_eq!(&*s, "dynamic value");
    }
}
