use std::time::{Duration, Instant};

use crate::action::Action;
use crate::keybind::{KeyCombo, KeybindMap};

#[derive(Debug)]
pub enum LeaderState {
    Idle,
    AwaitingAction { entered_at: Instant },
}

pub struct InputHandler {
    pub state: LeaderState,
    pub leader_ctrl_key: String,
    pub keybinds: KeybindMap,
    leader_timeout: Duration,
    double_tap_window: Duration,
    last_leader_press: Option<Instant>,
}

pub enum InputResult {
    /// An action was triggered by leader+key.
    Action(Action),
    /// The leader key was consumed (entering leader mode or double-tap).
    Consumed,
    /// Not a leader-related key; the caller should forward it to the PTY.
    PassThrough,
}

impl InputHandler {
    pub fn new(leader_timeout: Duration, double_tap_window: Duration) -> Self {
        InputHandler {
            state: LeaderState::Idle,
            leader_ctrl_key: "w".to_string(),
            keybinds: KeybindMap::default(),
            leader_timeout,
            double_tap_window,
            last_leader_press: None,
        }
    }

    pub fn check_timeout(&mut self) {
        if let LeaderState::AwaitingAction { entered_at } = &self.state
            && entered_at.elapsed() > self.leader_timeout {
                self.state = LeaderState::Idle;
            }
    }

    /// Process a key event. Only handles leader-related keys.
    /// Returns PassThrough if the caller should forward this key to the PTY.
    pub fn process_key(
        &mut self,
        key_name: &str,
        ctrl: bool,
        shift: bool,
        alt: bool,
        super_key: bool,
    ) -> InputResult {
        self.check_timeout();

        match &self.state {
            LeaderState::Idle => {
                // Only intercept the leader key (Ctrl+Space)
                if ctrl && key_name == self.leader_ctrl_key {
                    if let Some(last) = self.last_leader_press
                        && last.elapsed() < self.double_tap_window {
                            self.last_leader_press = None;
                            return InputResult::Action(Action::SendLeaderKey);
                        }
                    self.last_leader_press = Some(Instant::now());
                    self.state = LeaderState::AwaitingAction {
                        entered_at: Instant::now(),
                    };
                    return InputResult::Consumed;
                }

                // Everything else goes to PTY
                InputResult::PassThrough
            }
            LeaderState::AwaitingAction { .. } => {
                self.state = LeaderState::Idle;

                let combo = KeyCombo::from_modifiers(key_name, ctrl, shift, alt, super_key);

                if let Some(action) = self.keybinds.lookup(&combo) {
                    InputResult::Action(action)
                } else {
                    // Unknown key after leader: consume it (don't send to PTY)
                    InputResult::Consumed
                }
            }
        }
    }

    pub fn is_awaiting_action(&self) -> bool {
        matches!(self.state, LeaderState::AwaitingAction { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Action;
    use std::time::Duration;

    fn test_handler() -> InputHandler {
        InputHandler::new(Duration::from_millis(1000), Duration::from_millis(300))
    }

    #[test]
    fn normal_key_passes_through() {
        let mut h = test_handler();
        assert!(matches!(h.process_key("a", false, false, false, false), InputResult::PassThrough));
    }

    #[test]
    fn leader_key_enters_awaiting() {
        let mut h = test_handler();
        assert!(matches!(h.process_key("w", true, false, false, false), InputResult::Consumed));
        assert!(h.is_awaiting_action());
    }

    #[test]
    fn leader_then_action() {
        let mut h = test_handler();
        h.process_key("w", true, false, false, false);
        match h.process_key("n", false, false, false, false) {
            InputResult::Action(Action::NewColumnRight) => {}
            other => panic!("expected NewColumnRight, got {:?}", matches!(other, InputResult::PassThrough)),
        }
        assert!(!h.is_awaiting_action());
    }

    #[test]
    fn leader_then_unknown_is_consumed() {
        let mut h = test_handler();
        h.process_key("w", true, false, false, false);
        assert!(matches!(h.process_key("z", false, false, false, false), InputResult::Consumed));
    }

    #[test]
    fn leader_timeout_resets() {
        let mut h = test_handler();
        h.process_key("w", true, false, false, false);
        assert!(h.is_awaiting_action());
        // Simulate timeout by setting entered_at to the past
        h.state = LeaderState::AwaitingAction {
            entered_at: std::time::Instant::now() - std::time::Duration::from_secs(2),
        };
        h.check_timeout();
        assert!(!h.is_awaiting_action());
    }

    #[test]
    fn shift_keybinds() {
        let mut h = test_handler();
        h.process_key("w", true, false, false, false);
        match h.process_key("H", false, true, false, false) {
            InputResult::Action(Action::MovePaneLeft) => {}
            _ => panic!("expected MovePaneLeft"),
        }
    }

    #[test]
    fn ctrl_key_not_leader_passes_through() {
        let mut h = test_handler();
        // Ctrl+C should pass through (not leader key)
        assert!(matches!(h.process_key("c", true, false, false, false), InputResult::PassThrough));
    }
}
