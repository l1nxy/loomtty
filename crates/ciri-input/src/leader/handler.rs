//! InputHandler: state machine for leader key, modes, and key tables.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::action::Action;
use crate::keybind::{BindingMode, BindingSet, KeyCombo, KeybindMap};

use super::types::*;

pub struct InputHandler {
    pub(crate) session: InputSessionState,
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

    /// Unified binding set (populated via `set_binding_set`).
    pub binding_set: BindingSet,
    key_tables: KeyTableState,
}

impl InputHandler {
    const MAX_TABLE_DEPTH: usize = 4;

    pub fn new(leader_timeout: Duration, double_tap_window: Duration) -> Self {
        Self {
            session: InputSessionState::Idle,
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
            key_tables: KeyTableState::default(),
        }
    }

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

    pub fn process_key(
        &mut self,
        key_name: &str,
        ctrl: bool,
        shift: bool,
        alt: bool,
        super_key: bool,
    ) -> InputResult {
        self.process_key_event(key_name, ctrl, shift, alt, super_key, BindingMode::EMPTY)
    }

    pub fn process_key_release(&mut self, key_name: &str) {
        if !self.leader_key.is_bare_modifier() || !self.is_leader_release(key_name) {
            return;
        }
        self.session.on_leader_release();
    }

    pub fn toggle_lock(&mut self) {
        self.session.on_toggle_lock();
    }

    pub fn is_awaiting_action(&self) -> bool {
        self.session.is_in_leader() || self.has_active_table()
    }

    pub fn is_locked(&self) -> bool {
        self.session.is_locked()
    }

    pub fn current_mode_name(&self) -> Option<&str> {
        self.active_table_name()
    }

    pub fn has_active_table(&self) -> bool {
        self.key_tables.is_active()
    }

    pub fn active_table_name(&self) -> Option<&str> {
        self.key_tables.current()
    }

    pub fn activate_key_table(&mut self, name: &str) {
        if !self.key_tables.push(name, Self::MAX_TABLE_DEPTH) {
            log::warn!(
                "key table stack depth limit ({}) reached, ignoring activate_key_table({name:?})",
                Self::MAX_TABLE_DEPTH
            );
        }
    }

    pub fn deactivate_key_table(&mut self) {
        self.key_tables.pop();
    }

    pub fn deactivate_all_key_tables(&mut self) {
        self.key_tables.clear();
    }

    pub fn set_binding_set(&mut self, set: BindingSet) {
        self.binding_set = set;
    }

    // ── Orchestration ──

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

        if let Some(result) = self.handle_session_gate(event, app_mode) {
            return result;
        }

        if let Some(result) = self.handle_leader_press(event) {
            return result;
        }

        if let Some(result) = self.handle_escape(event.key_name) {
            return result;
        }

        let resolution = self.resolve_binding(event, app_mode);
        self.apply_resolution(resolution)
    }

    fn handle_session_gate(
        &mut self,
        event: KeyEvent<'_>,
        app_mode: BindingMode,
    ) -> Option<InputResult> {
        if matches!(self.session, InputSessionState::Unlocking) {
            return Some(self.process_unlocking(event));
        }

        if app_mode.contains(BindingMode::LOCKED) {
            return Some(self.process_locked(event, app_mode));
        }

        if app_mode.contains(BindingMode::PASTE_CONFIRM) {
            let resolution = self.resolve_binding(event, app_mode);
            return Some(self.apply_resolution(resolution));
        }

        None
    }

    fn handle_leader_press(&mut self, event: KeyEvent<'_>) -> Option<InputResult> {
        if !self.is_leader_press(event) || self.session.is_in_leader() {
            return None;
        }
        if self.detect_double_tap() {
            return Some(InputResult::Action(Action::SendLeaderKey));
        }
        self.session.on_leader_press(self.input_mode);
        Some(InputResult::Consumed)
    }

    fn handle_escape(&mut self, key_name: &str) -> Option<InputResult> {
        if !is_escape(key_name) {
            return None;
        }

        if self.session.on_escape() {
            return Some(InputResult::Consumed);
        }

        if self.has_active_table() {
            self.deactivate_key_table();
            return Some(InputResult::Consumed);
        }

        None
    }

    // ── Binding resolution (pure) ──

    fn resolve_binding(&self, event: KeyEvent<'_>, app_mode: BindingMode) -> BindingResolution {
        let runtime_mode = self.runtime_mode(app_mode);
        let combo = if self.session.is_in_leader() {
            self.combo_stripping_leader(event)
        } else {
            self.event_combo(event)
        };
        let active_table = self.active_table_name();

        if let Some(binding) = self.binding_set.lookup(runtime_mode, active_table, &combo) {
            return BindingResolution::Dispatch(binding.action.clone());
        }

        if runtime_mode.intersects(BindingMode::SEARCH | BindingMode::PALETTE) {
            return BindingResolution::TextInput;
        }

        if self.session.is_in_leader() {
            return BindingResolution::Consumed;
        }

        BindingResolution::PassThrough
    }

    // ── Apply resolution (effectful) ──

    fn apply_resolution(&mut self, resolution: BindingResolution) -> InputResult {
        match resolution {
            BindingResolution::Dispatch(action) => self.dispatch_action(action),
            BindingResolution::TextInput => InputResult::Action(Action::TextInput),
            BindingResolution::Consumed => {
                self.session.on_unmatched_leader();
                InputResult::Consumed
            }
            BindingResolution::PassThrough => InputResult::PassThrough,
        }
    }

    fn dispatch_action(&mut self, action: Action) -> InputResult {
        match &action {
            Action::ActivateKeyTable(name) | Action::EnterMode(name) => {
                self.activate_key_table(name);
                self.session.exit_leader();
                InputResult::Consumed
            }
            Action::DeactivateKeyTable => {
                self.deactivate_key_table();
                InputResult::Consumed
            }
            Action::ToggleLock => {
                self.session.exit_leader();
                InputResult::Action(action)
            }
            _ => {
                self.session.on_action_dispatched(&action);
                InputResult::Action(action)
            }
        }
    }

    // ── Locked state handlers ──

    fn process_locked(&mut self, event: KeyEvent<'_>, app_mode: BindingMode) -> InputResult {
        if self.is_leader_press(event) {
            self.session.on_leader_press(self.input_mode);
            return InputResult::Consumed;
        }

        let combo = self.event_combo(event);
        if let Some(binding) = self.binding_set.lookup(app_mode, None, &combo)
            && matches!(binding.action, Action::ToggleLock)
        {
            // Don't change state — the caller will call toggle_lock().
            return InputResult::Action(Action::ToggleLock);
        }

        InputResult::PassThrough
    }

    fn process_unlocking(&mut self, event: KeyEvent<'_>) -> InputResult {
        let combo = self.combo_stripping_leader(event);
        if let Some(binding) = self.binding_set.lookup(BindingMode::LEADER, None, &combo)
            && matches!(binding.action, Action::ToggleLock)
        {
            self.session.on_unlock_attempt(true);
            return InputResult::Consumed;
        }
        self.session.on_unlock_attempt(false);
        InputResult::PassThrough
    }

    // ── Helpers ──

    fn runtime_mode(&self, app_mode: BindingMode) -> BindingMode {
        let mut mode = app_mode;
        if self.session.is_in_leader() {
            mode |= BindingMode::LEADER;
        }
        if self.has_active_table() {
            mode |= BindingMode::KEY_TABLE;
        }
        mode
    }

    fn check_timeout(&mut self) {
        if self.has_active_table() {
            return;
        }
        self.session.on_timeout(self.leader_timeout);
    }

    /// Proactive timeout check for the event loop.
    /// Idempotent and safe to call at any time — internally re-checks
    /// elapsed time, so no external timing guard is required.
    pub fn poll_timeout(&mut self) {
        self.check_timeout();
    }

    /// Returns the instant at which leader state should expire.
    /// The event loop can use this to schedule a wake-up for proactive timeout.
    pub fn leader_deadline(&self) -> Option<std::time::Instant> {
        if self.has_active_table() {
            return None;
        }
        self.session.leader_deadline(self.leader_timeout)
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
