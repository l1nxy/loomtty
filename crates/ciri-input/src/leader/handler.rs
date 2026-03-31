//! InputHandler: state machine for leader key, modes, and key tables.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::action::Action;
use crate::keybind::{BindingMode, BindingSet, KeyCombo, KeybindMap};

use super::types::*;

pub struct InputHandler {
    pub session: InputSessionState,
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
        if matches!(self.session, InputSessionState::Leader(_)) {
            self.session = InputSessionState::Idle;
        } else if matches!(self.session, InputSessionState::Unlocking) {
            self.session = InputSessionState::Locked;
        }
    }

    pub fn toggle_lock(&mut self) {
        self.session = if self.is_locked() {
            InputSessionState::Idle
        } else {
            InputSessionState::Locked
        };
    }

    pub fn is_awaiting_action(&self) -> bool {
        matches!(self.session, InputSessionState::Leader(_)) || self.has_active_table()
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
        let context = AppInputContext::new(app_mode);

        if let Some(result) = self.handle_session_gate(event, context) {
            return result;
        }

        if let Some(result) = self.handle_leader_press(event) {
            return result;
        }

        if let Some(result) = self.handle_escape(event.key_name) {
            return result;
        }

        let resolution = self.resolve_binding(event, context);
        self.apply_resolution(resolution, context)
    }

    fn handle_session_gate(
        &mut self,
        event: KeyEvent<'_>,
        context: AppInputContext,
    ) -> Option<InputResult> {
        if matches!(self.session, InputSessionState::Unlocking) {
            return Some(self.process_unlocking(event));
        }

        if context.mode.contains(BindingMode::LOCKED) {
            return Some(self.process_locked(event, context));
        }

        if context.mode.contains(BindingMode::PASTE_CONFIRM) {
            let resolution = self.resolve_binding(event, context);
            return Some(self.apply_resolution(resolution, context));
        }

        None
    }

    fn handle_leader_press(&mut self, event: KeyEvent<'_>) -> Option<InputResult> {
        if self.is_leader_press(event) && !matches!(self.session, InputSessionState::Leader(_)) {
            if self.detect_double_tap() {
                return Some(InputResult::Action(Action::SendLeaderKey));
            }
            self.session = InputSessionState::Leader(LeaderSession::new(self.session_policy()));
            return Some(InputResult::Consumed);
        }
        None
    }

    fn handle_escape(&mut self, key_name: &str) -> Option<InputResult> {
        if !is_escape(key_name) {
            return None;
        }

        if matches!(self.session, InputSessionState::Leader(_)) {
            self.session = InputSessionState::Idle;
            return Some(InputResult::Consumed);
        }

        if self.has_active_table() {
            self.deactivate_key_table();
            return Some(InputResult::Consumed);
        }

        None
    }

    fn resolve_binding(&self, event: KeyEvent<'_>, context: AppInputContext) -> BindingResolution {
        let runtime_mode = self.runtime_mode(context.mode);
        let combo = if matches!(self.session, InputSessionState::Leader(_)) {
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

        if matches!(self.session, InputSessionState::Leader(_)) {
            return BindingResolution::Consumed;
        }

        BindingResolution::PassThrough
    }

    fn apply_resolution(
        &mut self,
        resolution: BindingResolution,
        context: AppInputContext,
    ) -> InputResult {
        match resolution {
            BindingResolution::Dispatch(action) => self.dispatch_action(action, context),
            BindingResolution::TextInput => InputResult::Action(Action::TextInput),
            BindingResolution::Consumed => {
                self.finish_unmatched_leader();
                InputResult::Consumed
            }
            BindingResolution::PassThrough => InputResult::PassThrough,
        }
    }

    fn dispatch_action(&mut self, action: Action, context: AppInputContext) -> InputResult {
        match &action {
            Action::ActivateKeyTable(name) | Action::EnterMode(name) => {
                self.activate_key_table(name);
                self.session = InputSessionState::Idle;
                InputResult::Consumed
            }
            Action::DeactivateKeyTable => {
                self.deactivate_key_table();
                InputResult::Consumed
            }
            Action::ToggleLock => {
                self.session = InputSessionState::Idle;
                InputResult::Action(action)
            }
            _ => {
                self.after_dispatched_action(&action, context);
                InputResult::Action(action)
            }
        }
    }

    fn after_dispatched_action(&mut self, action: &Action, _context: AppInputContext) {
        let Some(session) = self.session.leader_session().cloned() else {
            return;
        };

        match session.policy {
            SessionPolicy::OneShot => self.session = InputSessionState::Idle,
            SessionPolicy::Sticky => {
                if action.is_repeatable() {
                    self.session = InputSessionState::Leader(LeaderSession::new(SessionPolicy::Sticky));
                } else {
                    self.session = InputSessionState::Idle;
                }
            }
        }
    }

    fn finish_unmatched_leader(&mut self) {
        if matches!(self.session, InputSessionState::Leader(_))
            && matches!(self.input_mode, InputMode::Prefix)
        {
            self.session = InputSessionState::Idle;
        }
    }

    fn process_locked(&mut self, event: KeyEvent<'_>, context: AppInputContext) -> InputResult {
        if self.is_leader_press(event) {
            self.session = InputSessionState::Unlocking;
            return InputResult::Consumed;
        }

        let combo = self.event_combo(event);
        if let Some(binding) = self.binding_set.lookup(context.mode, None, &combo)
            && matches!(binding.action, Action::ToggleLock)
        {
            self.session = InputSessionState::Idle;
            return InputResult::Action(Action::ToggleLock);
        }

        InputResult::PassThrough
    }

    fn process_unlocking(&mut self, event: KeyEvent<'_>) -> InputResult {
        let combo = self.combo_stripping_leader(event);
        if let Some(binding) = self.binding_set.lookup(BindingMode::LEADER, None, &combo)
            && matches!(binding.action, Action::ToggleLock)
        {
            self.session = InputSessionState::Idle;
            return InputResult::Action(Action::ToggleLock);
        }
        self.session = InputSessionState::Idle;
        InputResult::PassThrough
    }

    fn runtime_mode(&self, app_mode: BindingMode) -> BindingMode {
        let mut mode = app_mode;
        if matches!(self.session, InputSessionState::Leader(_)) {
            mode |= BindingMode::LEADER;
        }
        if self.has_active_table() {
            mode |= BindingMode::KEY_TABLE;
        }
        mode
    }

    fn session_policy(&self) -> SessionPolicy {
        match self.input_mode {
            InputMode::Prefix => SessionPolicy::OneShot,
            InputMode::Sticky => SessionPolicy::Sticky,
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
        if self.input_mode == InputMode::Sticky || self.has_active_table() {
            return;
        }
        let Some(session) = self.session.leader_session() else {
            return;
        };
        if session.entered_at.elapsed() > self.leader_timeout {
            self.session = InputSessionState::Idle;
        }
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
