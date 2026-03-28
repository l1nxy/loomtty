//! Keybinding system: combos, mode-aware bindings, and legacy maps.

mod binding;
mod map;
mod parse;
mod types;

pub use binding::{Binding, BindingSet};
pub use map::KeybindMap;
pub use types::{BindingMode, KeyCombo};

#[cfg(test)]
mod tests;
