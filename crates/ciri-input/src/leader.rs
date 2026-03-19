use std::time::{Duration, Instant};

use crate::action::Action;
use crate::keybind::{KeyCombo, KeybindMap};

#[derive(Debug)]
pub enum LeaderState {
    Idle,
    AwaitingAction { entered_at: Instant },
}

/// Parsed leader key specification.
#[derive(Debug, Clone)]
pub struct LeaderKey {
    /// The key name to match (e.g. "w", "space"), or empty if matching a bare modifier.
    pub key: String,
    /// Required modifiers.
    pub ctrl: bool,
    pub alt: bool,
    pub super_key: bool,
}

impl LeaderKey {
    /// Parse a leader key string like "ctrl+w", "alt", "ctrl+space", "super+a".
    pub fn parse(s: &str) -> Self {
        let parts: Vec<&str> = s.split('+').collect();
        let mut ctrl = false;
        let mut alt = false;
        let mut super_key = false;
        let mut key = String::new();

        for part in &parts {
            match part.to_lowercase().as_str() {
                "ctrl" | "control" => ctrl = true,
                "alt" => alt = true,
                "super" | "meta" | "win" => super_key = true,
                other => key = other.to_string(),
            }
        }

        // If the entire string is just a modifier (e.g. "alt"), key stays empty
        // and we match when that modifier is pressed alone.
        LeaderKey { key, ctrl, alt, super_key }
    }

    /// Check if a key event matches this leader key.
    pub fn matches(&self, key_name: &str, ctrl: bool, alt: bool, super_key: bool) -> bool {
        if self.key.is_empty() {
            // Bare modifier key (e.g. "alt"): match when that modifier's own
            // key name appears (winit sends "Alt" as the key).
            let is_mod_key = key_name.eq_ignore_ascii_case("alt")
                || key_name.eq_ignore_ascii_case("control")
                || key_name.eq_ignore_ascii_case("super")
                || key_name.eq_ignore_ascii_case("meta");
            if !is_mod_key {
                return false;
            }
            // Check that the right modifier flag is expected
            if self.alt && key_name.eq_ignore_ascii_case("alt") { return true; }
            if self.ctrl && key_name.eq_ignore_ascii_case("control") { return true; }
            if self.super_key && (key_name.eq_ignore_ascii_case("super") || key_name.eq_ignore_ascii_case("meta")) { return true; }
            return false;
        }

        // Regular key + modifier combo (e.g. "ctrl+w")
        key_name == self.key
            && ctrl == self.ctrl
            && alt == self.alt
            && super_key == self.super_key
    }
}

pub struct InputHandler {
    pub state: LeaderState,
    pub leader_key: LeaderKey,
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
            leader_key: LeaderKey::parse("ctrl+w"),
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
                if self.leader_key.matches(key_name, ctrl, alt, super_key) {
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
                log::debug!("leader combo: key={:?} shift={} ctrl={} alt={} super={}", key_name, shift, ctrl, alt, super_key);

                if let Some(action) = self.keybinds.lookup(&combo) {
                    log::debug!("leader matched action: {:?}", action);
                    InputResult::Action(action)
                } else {
                    log::debug!("leader: no match for combo {:?}", combo);
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
        let mut h = InputHandler::new(Duration::from_millis(1000), Duration::from_millis(300));
        h.leader_key = LeaderKey::parse("ctrl+w");
        h
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
        assert!(matches!(h.process_key("c", true, false, false, false), InputResult::PassThrough));
    }

    #[test]
    fn alt_leader_key() {
        let mut h = test_handler();
        h.leader_key = LeaderKey::parse("alt");
        // Alt key press should enter leader mode
        assert!(matches!(h.process_key("Alt", false, false, true, false), InputResult::Consumed));
        assert!(h.is_awaiting_action());
        // Then pressing 'n' should trigger action
        match h.process_key("n", false, false, false, false) {
            InputResult::Action(Action::NewColumnRight) => {}
            _ => panic!("expected NewColumnRight"),
        }
    }

    #[test]
    fn parse_leader_key_formats() {
        let k = LeaderKey::parse("ctrl+w");
        assert!(k.ctrl && !k.alt && k.key == "w");

        let k = LeaderKey::parse("alt");
        assert!(!k.ctrl && k.alt && k.key.is_empty());

        let k = LeaderKey::parse("ctrl+space");
        assert!(k.ctrl && k.key == "space");

        let k = LeaderKey::parse("super+a");
        assert!(k.super_key && k.key == "a");
    }
}
