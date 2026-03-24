use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::action::Action;
use crate::keybind::{KeyCombo, KeybindMap};

// ── Public types ──

/// Input mode determines how keybindings are activated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// tmux-style: press leader combo → one action → back to normal.
    Prefix,
    /// zellij-style when leader is a bare modifier (e.g. "alt"):
    ///   leader bindings are promoted to direct_bindings at init (app layer).
    /// With a combo leader (e.g. "ctrl+w"):
    ///   repeatable actions keep leader active, oneshot actions exit.
    Sticky,
}

/// State machine for the input handler.
#[derive(Debug)]
pub enum State {
    /// Normal — keys go to the terminal.
    Idle,
    /// Leader was pressed, awaiting action key (prefix / sticky-combo only).
    AwaitingAction { entered_at: Instant },
    /// Inside a named mode (e.g. "resize", "scroll").
    InMode { name: String },
    /// Locked — all keys pass through. Only leader+unlock exits.
    Locked,
    /// Leader pressed while locked, awaiting the unlock key.
    AwaitingUnlock,
}

pub enum InputResult {
    /// A keybinding matched; the caller should execute this action.
    Action(Action),
    /// Key was consumed by the input system (don't forward to PTY).
    Consumed,
    /// Not handled; the caller should forward to the PTY.
    PassThrough,
}

/// Parsed leader key specification (e.g. "ctrl+w", "alt", "ctrl+space").
#[derive(Debug, Clone)]
pub struct LeaderKey {
    /// Key name (e.g. "w", "space"). Empty for bare modifier leaders.
    pub key: String,
    pub ctrl: bool,
    pub alt: bool,
    pub super_key: bool,
}

impl LeaderKey {
    pub fn parse(s: &str) -> Self {
        let mut ctrl = false;
        let mut alt = false;
        let mut super_key = false;
        let mut key = String::new();
        for part in s.split('+') {
            match part.to_lowercase().as_str() {
                "ctrl" | "control" => ctrl = true,
                "alt" => alt = true,
                "super" | "meta" | "win" => super_key = true,
                other => key = other.to_string(),
            }
        }
        LeaderKey { key, ctrl, alt, super_key }
    }

    /// Does this key event match the leader key?
    pub fn matches(&self, key_name: &str, ctrl: bool, alt: bool, super_key: bool) -> bool {
        if self.key.is_empty() {
            // Bare modifier (e.g. "alt"): match when that modifier key is pressed alone.
            let solo = |want: bool, name: &str, other1: bool, other2: bool| {
                want && key_name.eq_ignore_ascii_case(name) && !other1 && !other2
            };
            solo(self.alt, "alt", ctrl, super_key)
                || solo(self.ctrl, "control", alt, super_key)
                || solo(self.super_key, "super", ctrl, alt)
                || solo(self.super_key, "meta", ctrl, alt)
        } else {
            // Combo (e.g. "ctrl+w"): exact match on key + modifiers.
            key_name.eq_ignore_ascii_case(&self.key)
                && ctrl == self.ctrl
                && alt == self.alt
                && super_key == self.super_key
        }
    }

    /// Is this a bare modifier leader (empty key part)?
    pub fn is_bare_modifier(&self) -> bool {
        self.key.is_empty()
    }
}

// ── InputHandler ──

pub struct InputHandler {
    pub state: State,
    pub leader_key: LeaderKey,
    pub input_mode: InputMode,

    /// Bindings behind the leader key (prefix/sticky-combo mode).
    pub keybinds: KeybindMap,
    /// Bindings that fire directly in Idle — no leader needed (e.g. Ctrl+R, Alt+H).
    pub direct_keybinds: KeybindMap,
    /// Named mode tables (e.g. "resize" → {h: decrease, l: increase}).
    pub mode_keybinds: HashMap<String, KeybindMap>,

    leader_timeout: Duration,
    double_tap_window: Duration,
    last_leader_press: Option<Instant>,
}

impl InputHandler {
    pub fn new(leader_timeout: Duration, double_tap_window: Duration) -> Self {
        Self {
            state: State::Idle,
            leader_key: LeaderKey::parse("ctrl+space"),
            input_mode: InputMode::Prefix,
            keybinds: KeybindMap::default(),
            direct_keybinds: KeybindMap { bindings: HashMap::new() },
            mode_keybinds: HashMap::new(),
            leader_timeout,
            double_tap_window,
            last_leader_press: None,
        }
    }

    // ── Public API ──

    /// Process a key-press event.
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
            State::Idle => self.process_idle(key_name, ctrl, shift, alt, super_key),
            State::AwaitingAction { .. } => {
                self.process_awaiting_action(key_name, ctrl, shift, alt, super_key)
            }
            State::InMode { .. } => {
                self.process_in_mode(key_name, ctrl, shift, alt, super_key)
            }
            State::Locked => self.process_locked(key_name, ctrl, alt, super_key),
            State::AwaitingUnlock => {
                self.process_awaiting_unlock(key_name, ctrl, shift, alt, super_key)
            }
        }
    }

    /// Process a key-release event.
    /// For bare-modifier leaders: releases AwaitingAction → Idle, AwaitingUnlock → Locked.
    pub fn process_key_release(&mut self, key_name: &str) {
        if !self.leader_key.is_bare_modifier() {
            return;
        }
        if !self.is_leader_release(key_name) {
            return;
        }
        match self.state {
            State::AwaitingAction { .. } => self.state = State::Idle,
            State::AwaitingUnlock => self.state = State::Locked,
            _ => {}
        }
    }

    pub fn toggle_lock(&mut self) {
        self.state = if self.is_locked() { State::Idle } else { State::Locked };
    }

    pub fn is_awaiting_action(&self) -> bool {
        matches!(self.state, State::AwaitingAction { .. } | State::InMode { .. })
    }

    pub fn is_locked(&self) -> bool {
        matches!(self.state, State::Locked | State::AwaitingUnlock)
    }

    pub fn current_mode_name(&self) -> Option<&str> {
        if let State::InMode { name, .. } = &self.state { Some(name) } else { None }
    }

    // ── State handlers ──

    fn process_idle(
        &mut self,
        key_name: &str,
        ctrl: bool,
        shift: bool,
        alt: bool,
        super_key: bool,
    ) -> InputResult {
        // 1. Direct bindings (Ctrl+R → resize, Alt+H → focus_left, etc.)
        let combo = KeyCombo::from_modifiers(key_name, ctrl, shift, alt, super_key);
        if let Some(action) = self.direct_keybinds.lookup(&combo) {
            return self.dispatch_immediate(action);
        }

        // 2. Leader key (prefix/sticky-combo mode only)
        if self.leader_key.matches(key_name, ctrl, alt, super_key) {
            if self.detect_double_tap() {
                return InputResult::Action(Action::SendLeaderKey);
            }
            self.state = State::AwaitingAction { entered_at: Instant::now() };
            return InputResult::Consumed;
        }

        InputResult::PassThrough
    }

    fn process_awaiting_action(
        &mut self,
        key_name: &str,
        ctrl: bool,
        shift: bool,
        alt: bool,
        super_key: bool,
    ) -> InputResult {
        if is_escape(key_name) {
            self.state = State::Idle;
            return InputResult::Consumed;
        }

        let combo = self.combo_stripping_leader(key_name, ctrl, shift, alt, super_key);

        if let Some(action) = self.keybinds.lookup(&combo) {
            // EnterMode → transition directly to InMode
            if let Action::EnterMode(ref name) = action {
                return self.try_enter_mode(name);
            }
            self.transition_after_action(&action);
            return InputResult::Action(action);
        }

        // No match: prefix exits, sticky stays
        if self.input_mode == InputMode::Prefix {
            self.state = State::Idle;
        }
        InputResult::Consumed
    }

    fn process_in_mode(
        &mut self,
        key_name: &str,
        ctrl: bool,
        shift: bool,
        alt: bool,
        super_key: bool,
    ) -> InputResult {
        if is_escape(key_name) {
            self.state = State::Idle;
            return InputResult::Consumed;
        }

        let combo = self.combo_stripping_leader(key_name, ctrl, shift, alt, super_key);

        // Clone name to avoid borrow conflict
        let name = match &self.state {
            State::InMode { name, .. } => name.clone(),
            _ => unreachable!(),
        };
        if let Some(table) = self.mode_keybinds.get(&name) {
            if let Some(action) = table.lookup(&combo) {
                return InputResult::Action(action);
            }
        }
        InputResult::Consumed
    }

    fn process_locked(
        &mut self,
        key_name: &str,
        ctrl: bool,
        alt: bool,
        super_key: bool,
    ) -> InputResult {
        // Only intercept the leader key to begin unlock sequence
        if self.leader_key.matches(key_name, ctrl, alt, super_key) {
            self.state = State::AwaitingUnlock;
            return InputResult::Consumed;
        }
        // Check direct bindings for toggle_lock (e.g. Ctrl+G)
        let combo = KeyCombo::from_modifiers(key_name, ctrl, false, alt, super_key);
        if let Some(Action::ToggleLock) = self.direct_keybinds.lookup(&combo) {
            self.state = State::Idle;
            return InputResult::Consumed;
        }
        InputResult::PassThrough
    }

    fn process_awaiting_unlock(
        &mut self,
        key_name: &str,
        ctrl: bool,
        shift: bool,
        alt: bool,
        super_key: bool,
    ) -> InputResult {
        let combo = self.combo_stripping_leader(key_name, ctrl, shift, alt, super_key);
        if let Some(Action::ToggleLock) = self.keybinds.lookup(&combo) {
            self.state = State::Idle;
            return InputResult::Consumed;
        }
        self.state = State::Locked;
        InputResult::PassThrough
    }

    // ── Helpers ──

    /// Dispatch an action from direct_bindings (Idle state).
    /// Handles EnterMode and ToggleLock specially.
    fn dispatch_immediate(&mut self, action: Action) -> InputResult {
        if let Action::EnterMode(ref name) = action {
            return self.try_enter_mode(name);
        }
        if matches!(action, Action::ToggleLock) {
            self.toggle_lock();
            return InputResult::Consumed;
        }
        InputResult::Action(action)
    }

    fn try_enter_mode(&mut self, name: &str) -> InputResult {
        if self.mode_keybinds.contains_key(name) {
            self.state = State::InMode { name: name.to_string() };
            InputResult::Consumed
        } else {
            log::warn!("enter_mode: unknown mode {:?}", name);
            self.state = State::Idle;
            InputResult::Consumed
        }
    }

    /// After executing an action in AwaitingAction, decide next state.
    fn transition_after_action(&mut self, action: &Action) {
        match self.input_mode {
            InputMode::Prefix => self.state = State::Idle,
            InputMode::Sticky => {
                if action.is_repeatable() {
                    self.state = State::AwaitingAction { entered_at: Instant::now() };
                } else {
                    self.state = State::Idle;
                }
            }
        }
    }

    /// Build a KeyCombo, stripping the leader modifier for bare-modifier leaders.
    fn combo_stripping_leader(
        &self,
        key_name: &str,
        ctrl: bool,
        shift: bool,
        alt: bool,
        super_key: bool,
    ) -> KeyCombo {
        let mut combo = KeyCombo::from_modifiers(key_name, ctrl, shift, alt, super_key);
        if self.leader_key.is_bare_modifier() {
            if self.leader_key.alt { combo.alt = false; }
            if self.leader_key.ctrl { combo.ctrl = false; }
            if self.leader_key.super_key { combo.super_key = false; }
        }
        combo
    }

    fn is_leader_release(&self, key_name: &str) -> bool {
        (self.leader_key.alt && key_name.eq_ignore_ascii_case("alt"))
            || (self.leader_key.ctrl && key_name.eq_ignore_ascii_case("control"))
            || (self.leader_key.super_key
                && (key_name.eq_ignore_ascii_case("super")
                    || key_name.eq_ignore_ascii_case("meta")))
    }

    fn detect_double_tap(&mut self) -> bool {
        if let Some(last) = self.last_leader_press {
            if last.elapsed() < self.double_tap_window {
                self.last_leader_press = None;
                return true;
            }
        }
        self.last_leader_press = Some(Instant::now());
        false
    }

    fn check_timeout(&mut self) {
        if self.input_mode == InputMode::Sticky {
            return; // Sticky mode never times out
        }
        if let State::AwaitingAction { entered_at } = &self.state {
            if entered_at.elapsed() > self.leader_timeout {
                self.state = State::Idle;
            }
        }
    }
}

fn is_escape(key_name: &str) -> bool {
    key_name.eq_ignore_ascii_case("escape")
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::time::Duration;

    fn prefix_handler() -> InputHandler {
        let mut h = InputHandler::new(Duration::from_millis(1000), Duration::from_millis(300));
        h.leader_key = LeaderKey::parse("ctrl+w");
        h
    }

    fn sticky_handler() -> InputHandler {
        let mut h = prefix_handler();
        h.input_mode = InputMode::Sticky;
        h
    }

    fn handler_with_modes() -> InputHandler {
        let mut h = sticky_handler();
        h.keybinds
            .bindings
            .insert(KeyCombo::new("r"), Action::EnterMode("resize".into()));
        h.keybinds
            .bindings
            .insert(KeyCombo::new("g"), Action::ToggleLock);

        let mut resize = KeybindMap { bindings: HashMap::new() };
        resize.bindings.insert(KeyCombo::new("h"), Action::ColumnWidthDecrease);
        resize.bindings.insert(KeyCombo::new("l"), Action::ColumnWidthIncrease);
        h.mode_keybinds.insert("resize".into(), resize);
        h
    }

    // ── Prefix mode ──

    #[test]
    fn normal_key_passes_through() {
        let mut h = prefix_handler();
        assert!(matches!(h.process_key("a", false, false, false, false), InputResult::PassThrough));
    }

    #[test]
    fn leader_enters_awaiting() {
        let mut h = prefix_handler();
        assert!(matches!(
            h.process_key("w", true, false, false, false),
            InputResult::Consumed
        ));
        assert!(h.is_awaiting_action());
    }

    #[test]
    fn leader_then_action() {
        let mut h = prefix_handler();
        h.process_key("w", true, false, false, false);
        assert!(matches!(
            h.process_key("n", false, false, false, false),
            InputResult::Action(Action::NewColumnRight)
        ));
        assert!(!h.is_awaiting_action());
    }

    #[test]
    fn leader_then_unknown_consumed_and_exits() {
        let mut h = prefix_handler();
        h.process_key("w", true, false, false, false);
        assert!(matches!(h.process_key("z", false, false, false, false), InputResult::Consumed));
        assert!(!h.is_awaiting_action()); // prefix exits on unknown
    }

    #[test]
    fn leader_timeout_resets() {
        let mut h = prefix_handler();
        h.process_key("w", true, false, false, false);
        h.state = State::AwaitingAction {
            entered_at: Instant::now() - Duration::from_secs(2),
        };
        h.check_timeout();
        assert!(!h.is_awaiting_action());
    }

    #[test]
    fn shift_keybinds() {
        let mut h = prefix_handler();
        h.process_key("w", true, false, false, false);
        assert!(matches!(
            h.process_key("H", false, true, false, false),
            InputResult::Action(Action::MovePaneLeft)
        ));
    }

    #[test]
    fn prefix_esc_exits() {
        let mut h = prefix_handler();
        h.process_key("w", true, false, false, false);
        assert!(matches!(h.process_key("escape", false, false, false, false), InputResult::Consumed));
        assert!(!h.is_awaiting_action());
    }

    // ── Sticky mode ──

    #[test]
    fn sticky_repeatable_stays_in_leader() {
        let mut h = sticky_handler();
        h.process_key("w", true, false, false, false);
        assert!(matches!(
            h.process_key("h", false, false, false, false),
            InputResult::Action(Action::FocusLeft)
        ));
        assert!(h.is_awaiting_action()); // stays
        assert!(matches!(
            h.process_key("l", false, false, false, false),
            InputResult::Action(Action::FocusRight)
        ));
        assert!(h.is_awaiting_action()); // still stays
    }

    #[test]
    fn sticky_oneshot_exits() {
        let mut h = sticky_handler();
        h.process_key("w", true, false, false, false);
        assert!(matches!(
            h.process_key("n", false, false, false, false),
            InputResult::Action(Action::NewColumnRight)
        ));
        assert!(!h.is_awaiting_action());
    }

    #[test]
    fn sticky_unknown_stays() {
        let mut h = sticky_handler();
        h.process_key("w", true, false, false, false);
        assert!(matches!(h.process_key("z", false, false, false, false), InputResult::Consumed));
        assert!(h.is_awaiting_action()); // stays, unlike prefix
    }

    #[test]
    fn sticky_no_timeout() {
        let mut h = sticky_handler();
        h.state = State::AwaitingAction {
            entered_at: Instant::now() - Duration::from_secs(100),
        };
        h.check_timeout();
        assert!(h.is_awaiting_action());
    }

    // ── Leader key parsing ──

    #[test]
    fn parse_leader_key_formats() {
        let k = LeaderKey::parse("ctrl+w");
        assert!(k.ctrl && !k.alt && k.key == "w");
        let k = LeaderKey::parse("alt");
        assert!(!k.ctrl && k.alt && k.key.is_empty() && k.is_bare_modifier());
        let k = LeaderKey::parse("ctrl+space");
        assert!(k.ctrl && k.key == "space" && !k.is_bare_modifier());
    }

    // ── Bare modifier leader (Alt) ──

    #[test]
    fn alt_leader_enters_awaiting() {
        let mut h = prefix_handler();
        h.leader_key = LeaderKey::parse("alt");
        assert!(matches!(
            h.process_key("Alt", false, false, true, false),
            InputResult::Consumed
        ));
        assert!(h.is_awaiting_action());
    }

    #[test]
    fn alt_held_strips_modifier() {
        let mut h = prefix_handler();
        h.leader_key = LeaderKey::parse("alt");
        h.process_key("Alt", false, false, true, false);
        // Alt still held → alt=true, but should be stripped
        assert!(matches!(
            h.process_key("h", false, false, true, false),
            InputResult::Action(Action::FocusLeft)
        ));
    }

    #[test]
    fn alt_release_exits_awaiting() {
        let mut h = sticky_handler();
        h.leader_key = LeaderKey::parse("alt");
        h.process_key("Alt", false, false, true, false);
        h.process_key_release("Alt");
        assert!(!h.is_awaiting_action());
    }

    #[test]
    fn ctrl_w_release_does_not_affect_leader() {
        let mut h = sticky_handler();
        h.process_key("w", true, false, false, false);
        h.process_key_release("w");
        assert!(h.is_awaiting_action()); // unaffected
    }

    // ── Named modes (key tables) ──

    #[test]
    fn enter_mode_from_leader() {
        let mut h = handler_with_modes();
        h.process_key("w", true, false, false, false);
        assert!(matches!(h.process_key("r", false, false, false, false), InputResult::Consumed));
        assert_eq!(h.current_mode_name(), Some("resize"));
    }

    #[test]
    fn mode_dispatches_and_stays() {
        let mut h = handler_with_modes();
        h.process_key("w", true, false, false, false);
        h.process_key("r", false, false, false, false);
        assert!(matches!(
            h.process_key("h", false, false, false, false),
            InputResult::Action(Action::ColumnWidthDecrease)
        ));
        assert_eq!(h.current_mode_name(), Some("resize"));
    }

    #[test]
    fn mode_esc_exits() {
        let mut h = handler_with_modes();
        h.process_key("w", true, false, false, false);
        h.process_key("r", false, false, false, false);
        h.process_key("escape", false, false, false, false);
        assert!(!h.is_awaiting_action());
    }

    #[test]
    fn mode_unknown_key_consumed() {
        let mut h = handler_with_modes();
        h.process_key("w", true, false, false, false);
        h.process_key("r", false, false, false, false);
        assert!(matches!(h.process_key("z", false, false, false, false), InputResult::Consumed));
        assert_eq!(h.current_mode_name(), Some("resize"));
    }

    #[test]
    fn alt_release_does_not_exit_mode() {
        let mut h = handler_with_modes();
        h.leader_key = LeaderKey::parse("alt");
        h.process_key("Alt", false, false, true, false);
        h.process_key("r", false, false, true, false);
        h.process_key_release("Alt");
        assert_eq!(h.current_mode_name(), Some("resize")); // stays
    }

    // ── Locked mode ──

    #[test]
    fn toggle_lock() {
        let mut h = handler_with_modes();
        h.toggle_lock();
        assert!(h.is_locked());
        h.toggle_lock();
        assert!(!h.is_locked());
    }

    #[test]
    fn locked_passes_through() {
        let mut h = handler_with_modes();
        h.toggle_lock();
        assert!(matches!(h.process_key("a", false, false, false, false), InputResult::PassThrough));
        assert!(h.is_locked());
    }

    #[test]
    fn locked_leader_then_unlock() {
        let mut h = handler_with_modes();
        h.toggle_lock();
        h.process_key("w", true, false, false, false); // leader → AwaitingUnlock
        assert!(matches!(h.state, State::AwaitingUnlock));
        h.process_key("g", false, false, false, false); // unlock key
        assert!(!h.is_locked());
    }

    #[test]
    fn locked_leader_then_wrong_key_stays_locked() {
        let mut h = handler_with_modes();
        h.toggle_lock();
        h.process_key("w", true, false, false, false);
        assert!(matches!(
            h.process_key("z", false, false, false, false),
            InputResult::PassThrough
        ));
        assert!(matches!(h.state, State::Locked));
    }

    #[test]
    fn locked_alt_release_cancels_unlock() {
        let mut h = handler_with_modes();
        h.leader_key = LeaderKey::parse("alt");
        h.toggle_lock();
        h.process_key("Alt", false, false, true, false);
        h.process_key_release("Alt");
        assert!(matches!(h.state, State::Locked));
    }

    // ── Direct bindings ──

    #[test]
    fn direct_binding_enters_mode() {
        let mut h = handler_with_modes();
        h.direct_keybinds
            .bindings
            .insert(KeyCombo::parse("ctrl+r"), Action::EnterMode("resize".into()));
        assert!(matches!(
            h.process_key("r", true, false, false, false),
            InputResult::Consumed
        ));
        assert_eq!(h.current_mode_name(), Some("resize"));
    }

    #[test]
    fn direct_binding_toggle_lock() {
        let mut h = handler_with_modes();
        h.direct_keybinds
            .bindings
            .insert(KeyCombo::parse("ctrl+g"), Action::ToggleLock);
        h.process_key("g", true, false, false, false);
        assert!(h.is_locked());
    }

    #[test]
    fn direct_binding_fires_action() {
        let mut h = prefix_handler();
        h.direct_keybinds
            .bindings
            .insert(KeyCombo::parse("alt+h"), Action::FocusLeft);
        assert!(matches!(
            h.process_key("h", false, false, true, false),
            InputResult::Action(Action::FocusLeft)
        ));
        assert!(!h.is_awaiting_action()); // stays Idle
    }

    #[test]
    fn locked_direct_unlock() {
        let mut h = handler_with_modes();
        h.direct_keybinds
            .bindings
            .insert(KeyCombo::parse("ctrl+g"), Action::ToggleLock);
        h.toggle_lock();
        // Ctrl+G should unlock directly (via direct binding in Locked state)
        h.process_key("g", true, false, false, false);
        assert!(!h.is_locked());
    }
}
