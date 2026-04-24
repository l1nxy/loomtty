//! Tree-based layout primitives for composable UI chrome.
//!
//! # Model
//!
//! Parent components hand explicit [`UiRect`] slots to children; positions are
//! **never stored** on the elements themselves.
//!
//! This module intentionally contains **no product-specific logic**. It
//! is a generic geometry + composition layer; concrete widgets live
//! alongside their feature code (e.g. `ui/top_bar/`).
//!
//! # File layout
//!
//! - `rect.rs` — [`UiRect`] geometry primitive

mod rect;

pub(crate) use rect::UiRect;
