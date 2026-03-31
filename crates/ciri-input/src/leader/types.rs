//! Public types for the leader key input system.

use std::time::Instant;

use crate::action::Action;
use crate::keybind::BindingMode;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPolicy {
    OneShot,
    Sticky,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaderSession {
    pub policy: SessionPolicy,
    pub entered_at: Instant,
}

impl LeaderSession {
    pub fn new(policy: SessionPolicy) -> Self {
        Self {
            policy,
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

    pub fn leader_session(&self) -> Option<&LeaderSession> {
        match self {
            Self::Leader(session) => Some(session),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputDispatch {
    Action,
    TextInput,
    Consumed,
    PassThrough,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingResolution {
    Dispatch(Action),
    TextInput,
    Consumed,
    PassThrough,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppInputContext {
    pub mode: BindingMode,
}

impl AppInputContext {
    pub fn new(mode: BindingMode) -> Self {
        Self { mode }
    }
}

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

pub enum InputResult {
    Action(Action),
    Consumed,
    PassThrough,
}

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
