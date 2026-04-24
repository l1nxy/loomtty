//! Tree-based layout primitives for composable UI chrome.
//!
//! # Model
//!
//! Every piece of chrome (top bar, tab bar, status bar, …) implements
//! [`UiElement`]. Containers ([`Linear`], [`Border`]) own a set of children
//! and know how to split an available [`UiRect`] between them. Positions
//! are **never stored** on the elements themselves — they are computed
//! lazily at paint time by walking the tree, mirroring the approach niri
//! takes with `LayoutElement::render(location)` in
//! `niri/src/layout/scrolling.rs:2376`.
//!
//! The trait is deliberately minimal:
//! - [`UiElement::size_hint`] — how much room the element wants along the
//!   parent's main axis.
//! - [`UiElement::paint`] — draw inside a rect handed in by the parent.
//! - [`UiElement::hit`] — map a mouse position inside the rect to an action.
//!
//! Elements are free to ignore `hit` (default `None`) if they are purely
//! decorative.
//!
//! This module intentionally contains **no product-specific logic**. It
//! is a generic geometry + composition layer; concrete widgets live
//! alongside their feature code (e.g. `ui/top_bar/`).
//!
//! # File layout
//!
//! - `rect.rs` — [`UiRect`] geometry primitive
//! - `offset.rs` — [`Offset`] additive paint-time shift + `UiRect::offset_by`
//! - `hint.rs` — [`Axis`] / [`SizeHint`]
//! - `element.rs` — [`UiElement`] trait
//! - `spacer.rs` — [`Spacer`] no-op filler
//! - `linear.rs` — [`Linear`] one-axis container
//! - `border.rs` — [`Border`] dock container + [`BorderSlots`]
//! - `test_support.rs` — shared probes for `*_tests.rs`

mod border;
mod element;
mod hint;
mod linear;
mod offset;
mod rect;
mod spacer;

#[cfg(test)]
mod test_support;

#[allow(unused_imports)]
pub(crate) use border::Border;
pub(crate) use element::UiElement;
pub(crate) use hint::{Axis, SizeHint};
pub(crate) use linear::Linear;
pub(crate) use rect::UiRect;
pub(crate) use spacer::Spacer;
// `BorderSlots` is the return type of `Border::layout` and `Offset` is
// the return type of `UiElement::render_offset` — kept re-exported for
// API completeness, but external callers usually access them via type
// inference rather than by name.
#[allow(unused_imports)]
pub(crate) use border::BorderSlots;
#[allow(unused_imports)]
pub(crate) use offset::Offset;
