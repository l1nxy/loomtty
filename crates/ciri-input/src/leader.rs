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

#[derive(Clone, Copy)]
struct KeyEvent<'a> {
    key_name: &'a str,
    ctrl: bool,
    shift: bool,
    alt: bool,
    super_key: bool,
}

impl<'a> KeyEvent<'a> {
    fn new(key_name: &'a str, ctrl: bool, shift: bool, alt: bool, super_key: bool) -> Self {
        Self {
            key_name,
            ctrl,
            shift,
            alt,
            super_key,
        }
    }
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
        let mut key = String::new();
        let mut leader = Self {
            key: String::new(),
            ctrl: false,
            alt: false,
            super_key: false,
        };

        for part in s.split('+') {
            match part.to_lowercase().as_str() {
                "ctrl" | "control" => leader.ctrl = true,
                "alt" => leader.alt = true,
                "super" | "meta" | "win" => leader.super_key = true,
                other => key = other.to_string(),
            }
        }

        leader.key = key;
        leader
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
            direct_keybinds: KeybindMap {
                bindings: HashMap::new(),
            },
            mode_keybinds: HashMap::new(),
            leader_timeout,
            double_tap_window,
            last_leader_press: None,
        }
    }

    /// Reload all keybindings from config values.
    /// Handles mode keybinds, direct keybinds, and bare-modifier promotion.
    pub fn reload_bindings(
        &mut self,
        leader: &str,
        input_mode: &str,
        bindings: &HashMap<String, String>,
        modes: &HashMap<String, HashMap<String, String>>,
        direct_bindings: &HashMap<String, String>,
    ) {
        self.keybinds = KeybindMap::from_config(bindings);
        self.leader_key = LeaderKey::parse(leader);
        self.input_mode = parse_input_mode(input_mode);
        self.mode_keybinds = load_mode_keybinds(modes);
        self.direct_keybinds = KeybindMap::from_config_only(direct_bindings);

        if self.uses_promoted_bare_modifier_leader() {
            self.promote_leader_bindings_to_direct();
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
        let event = KeyEvent::new(key_name, ctrl, shift, alt, super_key);

        match self.state {
            State::Idle => self.process_idle(event),
            State::AwaitingAction { .. } => self.process_awaiting_action(event),
            State::InMode { .. } => self.process_in_mode(event),
            State::Locked => self.process_locked(event),
            State::AwaitingUnlock => self.process_awaiting_unlock(event),
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
        self.state = if self.is_locked() {
            State::Idle
        } else {
            State::Locked
        };
    }

    pub fn is_awaiting_action(&self) -> bool {
        matches!(
            self.state,
            State::AwaitingAction { .. } | State::InMode { .. }
        )
    }

    pub fn is_locked(&self) -> bool {
        matches!(self.state, State::Locked | State::AwaitingUnlock)
    }

    pub fn current_mode_name(&self) -> Option<&str> {
        if let State::InMode { name, .. } = &self.state {
            Some(name)
        } else {
            None
        }
    }

    // ── State handlers ──

    fn process_idle(&mut self, event: KeyEvent<'_>) -> InputResult {
        // 1. Direct bindings (Ctrl+R → resize, Alt+H → focus_left, etc.)
        let combo = self.event_combo(event);
        if let Some(action) = self.direct_keybinds.lookup(&combo) {
            return self.dispatch_immediate(action);
        }

        // 2. Leader key (prefix/sticky-combo mode only)
        if self.is_leader_press(event) {
            if self.detect_double_tap() {
                return InputResult::Action(Action::SendLeaderKey);
            }
            self.enter_awaiting_action();
            return InputResult::Consumed;
        }

        InputResult::PassThrough
    }

    fn process_awaiting_action(&mut self, event: KeyEvent<'_>) -> InputResult {
        if self.consume_escape(event.key_name) {
            return InputResult::Consumed;
        }

        let combo = self.combo_stripping_leader(event);

        if let Some(action) = self.keybinds.lookup(&combo) {
            if let Some(result) = self.try_dispatch_mode_entry(&action) {
                return result;
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

    fn process_in_mode(&mut self, event: KeyEvent<'_>) -> InputResult {
        if self.consume_escape(event.key_name) {
            return InputResult::Consumed;
        }

        let combo = self.combo_stripping_leader(event);
        self.lookup_mode_action(&combo)
            .map(InputResult::Action)
            .unwrap_or(InputResult::Consumed)
    }

    fn process_locked(&mut self, event: KeyEvent<'_>) -> InputResult {
        // Only intercept the leader key to begin unlock sequence
        if self.is_leader_press(event) {
            self.state = State::AwaitingUnlock;
            return InputResult::Consumed;
        }
        // Check direct bindings for toggle_lock (e.g. Ctrl+G)
        let combo = KeyCombo::from_modifiers(
            event.key_name,
            event.ctrl,
            false,
            event.alt,
            event.super_key,
        );
        if let Some(Action::ToggleLock) = self.direct_keybinds.lookup(&combo) {
            self.state = State::Idle;
            return InputResult::Consumed;
        }
        InputResult::PassThrough
    }

    fn process_awaiting_unlock(&mut self, event: KeyEvent<'_>) -> InputResult {
        let combo = self.combo_stripping_leader(event);
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
        if let Some(result) = self.try_dispatch_mode_entry(&action) {
            return result;
        }
        if matches!(action, Action::ToggleLock) {
            self.toggle_lock();
            return InputResult::Consumed;
        }
        InputResult::Action(action)
    }

    fn try_enter_mode(&mut self, name: &str) -> InputResult {
        if self.mode_keybinds.contains_key(name) {
            self.state = State::InMode {
                name: name.to_string(),
            };
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
                    self.enter_awaiting_action();
                } else {
                    self.state = State::Idle;
                }
            }
        }
    }

    /// Build a KeyCombo, stripping the leader modifier for bare-modifier leaders.
    fn combo_stripping_leader(&self, event: KeyEvent<'_>) -> KeyCombo {
        let mut combo = self.event_combo(event);
        self.strip_leader_modifier(&mut combo);
        combo
    }

    fn event_combo(&self, event: KeyEvent<'_>) -> KeyCombo {
        KeyCombo::from_modifiers(
            event.key_name,
            event.ctrl,
            event.shift,
            event.alt,
            event.super_key,
        )
    }

    fn is_leader_press(&self, event: KeyEvent<'_>) -> bool {
        self.leader_key
            .matches(event.key_name, event.ctrl, event.alt, event.super_key)
    }

    fn enter_awaiting_action(&mut self) {
        self.state = State::AwaitingAction {
            entered_at: Instant::now(),
        };
    }

    fn is_leader_release(&self, key_name: &str) -> bool {
        (self.leader_key.alt && key_name.eq_ignore_ascii_case("alt"))
            || (self.leader_key.ctrl && key_name.eq_ignore_ascii_case("control"))
            || (self.leader_key.super_key
                && (key_name.eq_ignore_ascii_case("super")
                    || key_name.eq_ignore_ascii_case("meta")))
    }

    fn detect_double_tap(&mut self) -> bool {
        if let Some(last) = self.last_leader_press
            && last.elapsed() < self.double_tap_window
        {
            self.last_leader_press = None;
            return true;
        }
        self.last_leader_press = Some(Instant::now());
        false
    }

    fn check_timeout(&mut self) {
        if self.input_mode == InputMode::Sticky {
            return; // Sticky mode never times out
        }
        if let State::AwaitingAction { entered_at } = &self.state
            && entered_at.elapsed() > self.leader_timeout
        {
            self.state = State::Idle;
        }
    }

    fn consume_escape(&mut self, key_name: &str) -> bool {
        if !is_escape(key_name) {
            return false;
        }

        self.state = State::Idle;
        true
    }

    fn try_dispatch_mode_entry(&mut self, action: &Action) -> Option<InputResult> {
        match action {
            Action::EnterMode(name) => Some(self.try_enter_mode(name)),
            _ => None,
        }
    }

    fn lookup_mode_action(&self, combo: &KeyCombo) -> Option<Action> {
        let State::InMode { name } = &self.state else {
            return None;
        };

        self.mode_keybinds
            .get(name)
            .and_then(|table| table.lookup(combo))
    }

    fn strip_leader_modifier(&self, combo: &mut KeyCombo) {
        if !self.leader_key.is_bare_modifier() {
            return;
        }

        if self.leader_key.alt {
            combo.alt = false;
        }
        if self.leader_key.ctrl {
            combo.ctrl = false;
        }
        if self.leader_key.super_key {
            combo.super_key = false;
        }
    }

    /// Whether this handler has promoted leader bindings to direct (bare-modifier sticky mode).
    pub fn uses_bare_modifier_promotion(&self) -> bool {
        // After promotion, leader_key is disabled — check if input_mode is sticky
        // and the original config was a bare modifier. We detect this by checking
        // if the leader key was disabled (the sentinel value after promotion).
        self.leader_key.key == "__disabled__"
    }

    fn uses_promoted_bare_modifier_leader(&self) -> bool {
        self.input_mode == InputMode::Sticky && self.leader_key.is_bare_modifier()
    }

    fn promote_leader_bindings_to_direct(&mut self) {
        for (combo, action) in &self.keybinds.bindings {
            let direct_combo = self.promoted_direct_combo(combo);
            self.direct_keybinds
                .bindings
                .entry(direct_combo)
                .or_insert_with(|| action.clone());
        }

        self.leader_key = disabled_leader_key();
    }

    fn promoted_direct_combo(&self, combo: &KeyCombo) -> KeyCombo {
        let mut direct_combo = combo.clone();

        if self.leader_key.alt {
            direct_combo.alt = true;
        }
        if self.leader_key.ctrl {
            direct_combo.ctrl = true;
        }
        if self.leader_key.super_key {
            direct_combo.super_key = true;
        }

        direct_combo
    }
}

fn is_escape(key_name: &str) -> bool {
    key_name.eq_ignore_ascii_case("escape")
}

fn parse_input_mode(input_mode: &str) -> InputMode {
    match input_mode {
        "sticky" => InputMode::Sticky,
        _ => InputMode::Prefix,
    }
}

fn load_mode_keybinds(
    modes: &HashMap<String, HashMap<String, String>>,
) -> HashMap<String, KeybindMap> {
    modes
        .iter()
        .map(|(name, bindings)| (name.clone(), KeybindMap::from_config_only(bindings)))
        .collect()
}

fn disabled_leader_key() -> LeaderKey {
    LeaderKey {
        key: "__disabled__".to_string(),
        ctrl: false,
        alt: false,
        super_key: false,
    }
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

        let mut resize = KeybindMap {
            bindings: HashMap::new(),
        };
        resize
            .bindings
            .insert(KeyCombo::new("h"), Action::ColumnWidthDecrease);
        resize
            .bindings
            .insert(KeyCombo::new("l"), Action::ColumnWidthIncrease);
        h.mode_keybinds.insert("resize".into(), resize);
        h
    }

    // ── Prefix mode ──

    #[test]
    fn normal_key_passes_through() {
        let mut h = prefix_handler();
        assert!(matches!(
            h.process_key("a", false, false, false, false),
            InputResult::PassThrough
        ));
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
        assert!(matches!(
            h.process_key("z", false, false, false, false),
            InputResult::Consumed
        ));
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
        assert!(matches!(
            h.process_key("escape", false, false, false, false),
            InputResult::Consumed
        ));
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
        assert!(matches!(
            h.process_key("z", false, false, false, false),
            InputResult::Consumed
        ));
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
        assert!(matches!(
            h.process_key("r", false, false, false, false),
            InputResult::Consumed
        ));
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
        assert!(matches!(
            h.process_key("z", false, false, false, false),
            InputResult::Consumed
        ));
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
        assert!(matches!(
            h.process_key("a", false, false, false, false),
            InputResult::PassThrough
        ));
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
        h.direct_keybinds.bindings.insert(
            KeyCombo::parse("ctrl+r"),
            Action::EnterMode("resize".into()),
        );
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
