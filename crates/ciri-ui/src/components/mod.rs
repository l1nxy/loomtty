//! Higher-level reusable UI components built on top of the `elements`
//! primitives. Each component is stateless config that resolves to a
//! `Div` subtree via an explicit `into_div(&theme)` method — the
//! pattern keeps colour resolution at the point where the theme is in
//! scope (parent's `build_tree` / `Render` impl) without forcing each
//! component to carry its own paint trait.

pub mod banner;

pub use banner::{Banner, Severity};
