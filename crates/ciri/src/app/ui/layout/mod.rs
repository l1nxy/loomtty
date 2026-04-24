//! Tree-based layout primitives for composable UI chrome.
//!
//! # Model
//!
//! Every piece of chrome (top bar, tab bar, status bar, …) implements
//! [`UiElement`]. Parent components hand explicit [`UiRect`] slots to children;
//! positions are **never stored** on the elements themselves.
//!
//! The trait is deliberately minimal:
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
//! - `element.rs` — [`UiElement`] trait

mod element;
mod rect;

pub(crate) use element::UiElement;
pub(crate) use rect::UiRect;
