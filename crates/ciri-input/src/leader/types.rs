//! Public types for the leader key input system.

use std::time::{Duration, Instant};

use crate::action::Action;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaderSession {
    pub(crate) mode: InputMode,
    pub(crate) entered_at: Instant,
}

impl LeaderSession {
    pub fn new(mode: InputMode) -> Self {
        Self {
            mode,
            entered_at: Instant::now(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyTableState {
    stack: Vec<String>,
}

impl KeyTableState {
    pub fn is_active(&self) -> bool {
        !self.stack.is_empty()
    }

    pub fn current(&self) -> Option<&str> {
        self.stack.last().map(|s| s.as_str())
    }

    pub fn push(&mut self, name: &str, max_depth: usize) -> bool {
        if self.current().is_some_and(|active| active == name) {
            return true;
        }
        if self.stack.len() >= max_depth {
            return false;
        }
        self.stack.push(name.to_string());
        true
    }

    pub fn pop(&mut self) -> Option<String> {
        self.stack.pop()
    }

    pub fn clear(&mut self) {
        self.stack.clear();
    }
}

// ── Session state machine ──

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputSessionState {
    Idle,
    Leader(LeaderSession),
    Locked,
    Unlocking,
}

impl InputSessionState {
    pub fn is_locked(&self) -> bool {
        matches!(self, Self::Locked | Self::Unlocking)
    }

    pub fn is_in_leader(&self) -> bool {
        matches!(self, Self::Leader(_))
    }

    pub fn on_leader_press(&mut self, mode: InputMode) {
        match self {
            Self::Idle => *self = Self::Leader(LeaderSession::new(mode)),
            Self::Locked => *self = Self::Unlocking,
            _ => {}
        }
    }

    pub fn on_leader_release(&mut self) {
        match self {
            Self::Leader(_) => *self = Self::Idle,
            Self::Unlocking => *self = Self::Locked,
            _ => {}
        }
    }

    /// Returns true if escape was consumed (leader was active).
    pub fn on_escape(&mut self) -> bool {
        if matches!(self, Self::Leader(_)) {
            *self = Self::Idle;
            true
        } else {
            false
        }
    }

    pub fn on_action_dispatched(&mut self, action: &Action) {
        let mode = match self {
            Self::Leader(s) => s.mode,
            _ => return,
        };
        match mode {
            InputMode::Prefix => *self = Self::Idle,
            InputMode::Sticky => {
                if action.is_repeatable() {
                    *self = Self::Leader(LeaderSession::new(InputMode::Sticky));
                } else {
                    *self = Self::Idle;
                }
            }
        }
    }

    pub fn on_unmatched_leader(&mut self) {
        if let Self::Leader(s) = self
            && s.mode == InputMode::Prefix
        {
            *self = Self::Idle;
        }
    }

    pub fn on_timeout(&mut self, limit: Duration) {
        if let Self::Leader(s) = self
            && s.mode != InputMode::Sticky
            && s.entered_at.elapsed() >= limit
        {
            *self = Self::Idle;
        }
    }

    /// Returns the deadline at which leader state should expire, if applicable.
    pub fn leader_deadline(&self, limit: Duration) -> Option<Instant> {
        if let Self::Leader(s) = self
            && s.mode != InputMode::Sticky
        {
            Some(s.entered_at + limit)
        } else {
            None
        }
    }

    pub fn on_toggle_lock(&mut self) {
        *self = if self.is_locked() {
            Self::Idle
        } else {
            Self::Locked
        };
    }

    /// Exit leader state if active. No-op otherwise.
    pub fn exit_leader(&mut self) {
        if matches!(self, Self::Leader(_)) {
            *self = Self::Idle;
        }
    }

    /// Resolve an unlock attempt: success → Idle (unlocked), failure → Locked.
    pub fn on_unlock_attempt(&mut self, success: bool) {
        if matches!(self, Self::Unlocking) {
            *self = if success { Self::Idle } else { Self::Locked };
        }
    }
}

// ── Binding resolution (intermediate result) ──

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingResolution {
    Dispatch(Action),
    TextInput,
    Consumed,
    PassThrough,
}

// ── Key event ──

#[derive(Clone, Copy)]
pub(super) struct KeyEvent<'a> {
    pub(super) key_name: &'a str,
    pub(super) ctrl: bool,
    pub(super) shift: bool,
    pub(super) alt: bool,
    pub(super) super_key: bool,
}

impl<'a> KeyEvent<'a> {
    pub(super) fn new(
        key_name: &'a str,
        ctrl: bool,
        shift: bool,
        alt: bool,
        super_key: bool,
    ) -> Self {
        Self {
            key_name,
            ctrl,
            shift,
            alt,
            super_key,
        }
    }
}

// ── Leader key ──

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

    pub fn matches(&self, key_name: &str, ctrl: bool, alt: bool, super_key: bool) -> bool {
        if self.key.is_empty() {
            let solo = |want: bool, name: &str, other1: bool, other2: bool| {
                want && key_name.eq_ignore_ascii_case(name) && !other1 && !other2
            };
            solo(self.alt, "alt", ctrl, super_key)
                || solo(self.ctrl, "control", alt, super_key)
                || solo(self.super_key, "super", ctrl, alt)
                || solo(self.super_key, "meta", ctrl, alt)
        } else {
            key_name.eq_ignore_ascii_case(&self.key)
                && ctrl == self.ctrl
                && alt == self.alt
                && super_key == self.super_key
        }
    }

    pub fn is_bare_modifier(&self) -> bool {
        self.key.is_empty()
    }
}

// ── Output types ──

pub enum InputResult {
    Action(Action),
    Consumed,
    PassThrough,
}

// ── Helpers ──

pub(super) fn is_escape(key_name: &str) -> bool {
    key_name.eq_ignore_ascii_case("escape")
}

pub(super) fn parse_input_mode(input_mode: &str) -> InputMode {
    match input_mode {
        "sticky" => InputMode::Sticky,
        _ => InputMode::Prefix,
    }
}

pub(super) fn disabled_leader_key() -> LeaderKey {
    LeaderKey {
        key: "__disabled__".to_string(),
        ctrl: false,
        alt: false,
        super_key: false,
    }
}
