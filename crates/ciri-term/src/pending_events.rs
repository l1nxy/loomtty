//! Drainable pending events: clipboard, bell, command completion.

use std::time::Duration;

/// Accumulates events that the server drains each tick.
pub(crate) struct PendingEvents {
    pub clipboard: Vec<String>,
    pub bell: bool,
    pub command_completion: Option<Duration>,
}

impl PendingEvents {
    pub fn new() -> Self {
        Self {
            clipboard: Vec::new(),
            bell: false,
            command_completion: None,
        }
    }

    pub fn drain_clipboard(&mut self) -> Vec<String> {
        std::mem::take(&mut self.clipboard)
    }

    pub fn drain_bell(&mut self) -> bool {
        std::mem::take(&mut self.bell)
    }

    pub fn drain_command_completion(&mut self) -> Option<Duration> {
        self.command_completion.take()
    }
}
