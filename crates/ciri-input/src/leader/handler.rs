//! InputHandler: state machine for leader key, modes, and key tables.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::action::Action;
use crate::keybind::{BindingMode, BindingSet, KeyCombo, KeybindMap};

use super::types::*;

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

    // ── Unified binding system ──
    /// Unified binding set (populated via `set_binding_set`).
    pub binding_set: BindingSet,
    /// Stack of active named key tables (e.g. ["resize"]).
    key_table_stack: Vec<String>,
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
            binding_set: BindingSet::new(),
            key_table_stack: Vec::new(),
        }
    }

    /// Reload all keybindings from config values.
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
            State::Locked => self.process_locked_legacy(event),
            State::AwaitingUnlock => self.process_awaiting_unlock_legacy(event),
        }
    }

    /// Process a key-release event.
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

    // ── Unified key processing API ──

    pub fn has_active_table(&self) -> bool {
        !self.key_table_stack.is_empty()
    }

    pub fn active_table_name(&self) -> Option<&str> {
        self.key_table_stack.last().map(|s| s.as_str())
    }

    const MAX_TABLE_DEPTH: usize = 4;

    pub fn activate_key_table(&mut self, name: &str) {
        if self.key_table_stack.len() < Self::MAX_TABLE_DEPTH {
            self.key_table_stack.push(name.to_string());
        } else {
            log::warn!(
                "key table stack depth limit ({}) reached, ignoring activate_key_table({name:?})",
                Self::MAX_TABLE_DEPTH
            );
        }
    }

    pub fn deactivate_key_table(&mut self) {
        self.key_table_stack.pop();
    }

    pub fn deactivate_all_key_tables(&mut self) {
        self.key_table_stack.clear();
    }

    pub fn set_binding_set(&mut self, set: BindingSet) {
        self.binding_set = set;
    }

    /// Unified key processing that uses BindingSet + BindingMode.
    pub fn process_key_event(
        &mut self,
        key_name: &str,
        ctrl: bool,
        shift: bool,
        alt: bool,
        super_key: bool,
        app_mode: BindingMode,
    ) -> InputResult {
        self.check_timeout();
        let event = KeyEvent::new(key_name, ctrl, shift, alt, super_key);

        let mut mode = app_mode;
        if matches!(self.state, State::AwaitingAction { .. }) {
            mode |= BindingMode::LEADER;
        }
        if !self.key_table_stack.is_empty() {
            mode |= BindingMode::KEY_TABLE;
        }

        if matches!(self.state, State::AwaitingUnlock) {
            return self.process_awaiting_unlock(event);
        }
        if mode.contains(BindingMode::LOCKED) {
            return self.process_locked(event, mode);
        }
        if mode.contains(BindingMode::PASTE_CONFIRM) {
            return self.lookup_binding(event, mode);
        }

        if self.is_leader_press(event) && !matches!(self.state, State::AwaitingAction { .. }) {
            if self.detect_double_tap() {
                return InputResult::Action(Action::SendLeaderKey);
            }
            self.enter_awaiting_action();
            return InputResult::Consumed;
        }

        if is_escape(event.key_name) {
            if matches!(self.state, State::AwaitingAction { .. }) {
                self.state = State::Idle;
                return InputResult::Consumed;
            }
            if !self.key_table_stack.is_empty() {
                self.key_table_stack.pop();
                return InputResult::Consumed;
            }
        }

        self.lookup_binding(event, mode)
    }

    fn process_locked(&mut self, event: KeyEvent<'_>, mode: BindingMode) -> InputResult {
        if self.is_leader_press(event) {
            self.state = State::AwaitingUnlock;
            return InputResult::Consumed;
        }
        let combo = self.event_combo(event);
        if let Some(binding) = self.binding_set.lookup(mode, None, &combo)
            && matches!(binding.action, Action::ToggleLock)
        {
            return InputResult::Action(Action::ToggleLock);
        }
        InputResult::PassThrough
    }

    fn process_awaiting_unlock(&mut self, event: KeyEvent<'_>) -> InputResult {
        let combo = self.combo_stripping_leader(event);
        if let Some(binding) = self.binding_set.lookup(BindingMode::LEADER, None, &combo)
            && matches!(binding.action, Action::ToggleLock)
        {
            self.state = State::Idle;
            return InputResult::Action(Action::ToggleLock);
        }
        self.state = State::Idle;
        InputResult::PassThrough
    }

    fn lookup_binding(&mut self, event: KeyEvent<'_>, mode: BindingMode) -> InputResult {
        let combo = if mode.contains(BindingMode::LEADER) {
            self.combo_stripping_leader(event)
        } else {
            self.event_combo(event)
        };

        let active_table = self.key_table_stack.last().map(|s| s.as_str());
        if let Some(binding) = self.binding_set.lookup(mode, active_table, &combo) {
            let action = binding.action.clone();
            return self.dispatch_action(action, mode);
        }

        if mode.intersects(BindingMode::SEARCH | BindingMode::PALETTE) {
            return InputResult::Action(Action::TextInput);
        }

        if mode.contains(BindingMode::LEADER) {
            if self.input_mode == InputMode::Prefix {
                self.state = State::Idle;
            }
            return InputResult::Consumed;
        }

        InputResult::PassThrough
    }

    fn dispatch_action(&mut self, action: Action, mode: BindingMode) -> InputResult {
        match &action {
            Action::ActivateKeyTable(name) => {
                self.activate_key_table(name);
                self.state = State::Idle;
                return InputResult::Consumed;
            }
            Action::DeactivateKeyTable => {
                self.key_table_stack.pop();
                return InputResult::Consumed;
            }
            Action::EnterMode(name) => {
                self.activate_key_table(name);
                self.state = State::Idle;
                return InputResult::Consumed;
            }
            Action::ToggleLock => {
                self.state = State::Idle;
                return InputResult::Action(action);
            }
            _ => {}
        }

        if mode.contains(BindingMode::LEADER) {
            self.transition_after_action(&action);
        }

        InputResult::Action(action)
    }

    // ── State handlers (legacy) ──

    fn process_idle(&mut self, event: KeyEvent<'_>) -> InputResult {
        let combo = self.event_combo(event);
        if let Some(action) = self.direct_keybinds.lookup(&combo) {
            return self.dispatch_immediate(action);
        }

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

    fn process_locked_legacy(&mut self, event: KeyEvent<'_>) -> InputResult {
        if self.is_leader_press(event) {
            self.state = State::AwaitingUnlock;
            return InputResult::Consumed;
        }
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

    fn process_awaiting_unlock_legacy(&mut self, event: KeyEvent<'_>) -> InputResult {
        let combo = self.combo_stripping_leader(event);
        if let Some(Action::ToggleLock) = self.keybinds.lookup(&combo) {
            self.state = State::Idle;
            return InputResult::Consumed;
        }
        self.state = State::Locked;
        InputResult::PassThrough
    }

    // ── Helpers ──

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

    pub(super) fn check_timeout(&mut self) {
        if self.input_mode == InputMode::Sticky {
            return;
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

    pub fn uses_bare_modifier_promotion(&self) -> bool {
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

fn load_mode_keybinds(
    modes: &HashMap<String, HashMap<String, String>>,
) -> HashMap<String, KeybindMap> {
    modes
        .iter()
        .map(|(name, bindings)| (name.clone(), KeybindMap::from_config_only(bindings)))
        .collect()
}
