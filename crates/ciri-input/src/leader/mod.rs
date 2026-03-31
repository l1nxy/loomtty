//! Leader key input handling: state machine, modes, and key tables.

mod handler;
pub mod types;

pub use handler::InputHandler;
pub use types::{InputMode, InputResult, InputSessionState, LeaderKey};

#[cfg(test)]
mod tests;
