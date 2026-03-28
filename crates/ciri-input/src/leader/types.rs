//! Public types for the leader key input system.

use std::time::Instant;

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
pub(super) struct KeyEvent<'a> {
    pub(super) key_name: &'a str,
    pub(super) ctrl: bool,
    pub(super) shift: bool,
    pub(super) alt: bool,
    pub(super) super_key: bool,
}

impl<'a> KeyEvent<'a> {
    pub(super) fn new(key_name: &'a str, ctrl: bool, shift: bool, alt: bool, super_key: bool) -> Self {
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
