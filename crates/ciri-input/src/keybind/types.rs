//! Core types: BindingMode bitflags and KeyCombo.

use super::parse::{apply_modifiers, split_modifier_prefixes};

// ── BindingMode bitflags ──

/// Bitflags representing the current application input context.
/// Each binding declares which mode flags must / must-not be active for it to match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BindingMode(u32);

impl BindingMode {
    pub const EMPTY: Self = Self(0);
    pub const NORMAL: Self = Self(1 << 0);
    pub const LEADER: Self = Self(1 << 1);
    pub const OVERVIEW: Self = Self(1 << 2);
    pub const SEARCH: Self = Self(1 << 3);
    pub const PALETTE: Self = Self(1 << 4);
    pub const LOCKED: Self = Self(1 << 5);
    pub const PASTE_CONFIRM: Self = Self(1 << 6);
    pub const KEY_TABLE: Self = Self(1 << 7);

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub const fn intersects(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for BindingMode {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for BindingMode {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

// ── KeyCombo ──

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyCombo {
    pub key: String,
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub super_key: bool,
}

impl KeyCombo {
    pub fn new(key: &str) -> Self {
        Self::from_modifiers(key, false, false, false, false)
    }

    pub fn with_shift(key: &str) -> Self {
        Self::from_modifiers(key, false, true, false, false)
    }

    /// Parse a combo string like "ctrl+alt+shift+h" into a KeyCombo.
    pub fn parse(s: &str) -> Self {
        let (key_part, modifiers) = split_modifier_prefixes(s);
        let mut combo = Self::new(key_part);
        apply_modifiers(&mut combo, modifiers);
        combo
    }

    /// Format this combo as a human-readable string (e.g. "alt+?", "ctrl+g").
    pub fn display(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("ctrl");
        }
        if self.alt {
            parts.push("alt");
        }
        if self.super_key {
            parts.push("super");
        }
        if self.shift {
            parts.push("shift");
        }
        parts.push(&self.key);
        parts.join("+")
    }

    /// Build a KeyCombo from runtime modifier state + key name.
    pub fn from_modifiers(key: &str, ctrl: bool, shift: bool, alt: bool, super_key: bool) -> Self {
        KeyCombo {
            key: key.to_lowercase(),
            shift,
            alt,
            ctrl,
            super_key,
        }
    }
}
