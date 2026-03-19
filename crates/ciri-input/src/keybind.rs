use crate::action::Action;
use std::collections::HashMap;

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
        KeyCombo { key: key.to_lowercase(), shift: false, alt: false, ctrl: false, super_key: false }
    }
    pub fn with_shift(key: &str) -> Self {
        KeyCombo { key: key.to_lowercase(), shift: true, alt: false, ctrl: false, super_key: false }
    }

    /// Parse a combo string like "ctrl+alt+shift+h" into a KeyCombo.
    pub fn parse(s: &str) -> Self {
        let mut shift = false;
        let mut alt = false;
        let mut ctrl = false;
        let mut super_key = false;
        let mut key_part = s;

        // Strip modifier prefixes in any order
        loop {
            if let Some(rest) = key_part.strip_prefix("shift+") {
                shift = true;
                key_part = rest;
            } else if let Some(rest) = key_part.strip_prefix("alt+") {
                alt = true;
                key_part = rest;
            } else if let Some(rest) = key_part.strip_prefix("ctrl+") {
                ctrl = true;
                key_part = rest;
            } else if let Some(rest) = key_part.strip_prefix("super+") {
                super_key = true;
                key_part = rest;
            } else {
                break;
            }
        }

        KeyCombo {
            key: key_part.to_lowercase(),
            shift,
            alt,
            ctrl,
            super_key,
        }
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

pub struct KeybindMap {
    pub bindings: HashMap<KeyCombo, Action>,
}

impl Default for KeybindMap {
    fn default() -> Self {
        let mut b = HashMap::new();

        // Pane lifecycle
        b.insert(KeyCombo::new("n"), Action::NewColumnRight);   // new column in this workspace
        b.insert(KeyCombo::new("d"), Action::NewWorkspaceBelow);  // new workspace below
        b.insert(KeyCombo::new("x"), Action::ClosePane);

        // Navigation: h/l = within workspace, j/k = between workspaces
        b.insert(KeyCombo::new("h"), Action::FocusLeft);
        b.insert(KeyCombo::new("l"), Action::FocusRight);
        b.insert(KeyCombo::new("j"), Action::FocusDown);        // next workspace
        b.insert(KeyCombo::new("k"), Action::FocusUp);          // prev workspace
        b.insert(KeyCombo::new("left"), Action::FocusLeft);
        b.insert(KeyCombo::new("right"), Action::FocusRight);
        b.insert(KeyCombo::new("up"), Action::FocusUp);
        b.insert(KeyCombo::new("down"), Action::FocusDown);

        // Move column within workspace
        b.insert(KeyCombo::with_shift("h"), Action::MovePaneLeft);
        b.insert(KeyCombo::with_shift("l"), Action::MovePaneRight);

        // Column width: cycle through presets
        b.insert(KeyCombo::new("r"), Action::CyclePresetWidth);
        b.insert(KeyCombo::with_shift("r"), Action::CyclePresetWidthReverse);
        b.insert(KeyCombo::new("f"), Action::ColumnWidthFull);

        // Incremental resize against the nearest split
        b.insert(KeyCombo::new("["), Action::ColumnWidthDecrease);
        b.insert(KeyCombo::new("]"), Action::ColumnWidthIncrease);

        // Equalize active column with its right neighbor
        b.insert(KeyCombo::new("="), Action::EqualizeAdjacentColumns);

        // Consume/Expel tiles within columns
        b.insert(KeyCombo::new("c"), Action::ConsumeIntoColumn);
        b.insert(KeyCombo::new("v"), Action::ExpelFromColumn);

        // Broadcast mode
        b.insert(KeyCombo::new("b"), Action::ToggleBroadcast);

        // Workspace switching by number
        for i in 1u8..=9 {
            b.insert(KeyCombo::new(&format!("{i}")), Action::SwitchWorkspace((i - 1) as usize));
        }

        // Overview
        b.insert(KeyCombo::new("o"), Action::ToggleOverview);
        b.insert(KeyCombo::new("tab"), Action::ToggleOverview);

        KeybindMap { bindings: b }
    }
}

impl KeybindMap {
    pub fn lookup(&self, combo: &KeyCombo) -> Option<Action> {
        self.bindings.get(combo).copied()
    }

    /// Default overview-mode keybindings.
    pub fn overview_default() -> Self {
        let mut b = HashMap::new();
        b.insert(KeyCombo::new("h"), Action::FocusLeft);
        b.insert(KeyCombo::new("l"), Action::FocusRight);
        b.insert(KeyCombo::new("j"), Action::FocusDown);
        b.insert(KeyCombo::new("k"), Action::FocusUp);
        b.insert(KeyCombo::new("left"), Action::FocusLeft);
        b.insert(KeyCombo::new("right"), Action::FocusRight);
        b.insert(KeyCombo::new("up"), Action::FocusUp);
        b.insert(KeyCombo::new("down"), Action::FocusDown);
        b.insert(KeyCombo::new("x"), Action::ClosePane);
        b.insert(KeyCombo::new("n"), Action::NewColumnRight);
        b.insert(KeyCombo::new("escape"), Action::ExitOverview);
        b.insert(KeyCombo::new("enter"), Action::ExitOverview);
        b.insert(KeyCombo::new("o"), Action::ExitOverview);
        b.insert(KeyCombo::new("tab"), Action::ExitOverview);
        KeybindMap { bindings: b }
    }

    /// Build a KeybindMap from defaults, then overlay config-driven bindings.
    pub fn from_config(bindings: &HashMap<String, String>) -> Self {
        // Start with defaults
        let mut map = Self::default();
        // Overlay user config
        for (key_str, action_str) in bindings {
            if let Some(action) = Action::from_name(action_str) {
                map.bindings.insert(KeyCombo::parse(key_str), action);
            } else {
                log::warn!("unknown action in keybinding config: {action_str:?} (key: {key_str:?})");
            }
        }
        map
    }

    /// Build overview KeybindMap from overview defaults, then overlay config.
    pub fn from_overview_config(bindings: &HashMap<String, String>) -> Self {
        let mut map = Self::overview_default();
        for (key_str, action_str) in bindings {
            if let Some(action) = Action::from_name(action_str) {
                map.bindings.insert(KeyCombo::parse(key_str), action);
            } else {
                log::warn!("unknown action in overview keybinding config: {action_str:?} (key: {key_str:?})");
            }
        }
        map
    }

    /// Build a KeybindMap from ONLY the given bindings (no defaults).
    pub fn from_config_only(bindings: &HashMap<String, String>) -> Self {
        let mut b = HashMap::new();
        for (key_str, action_str) in bindings {
            if let Some(action) = Action::from_name(action_str) {
                b.insert(KeyCombo::parse(key_str), action);
            } else {
                log::warn!("unknown action in keybinding config: {action_str:?} (key: {key_str:?})");
            }
        }
        KeybindMap { bindings: b }
    }
}
