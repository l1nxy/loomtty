//! KeybindMap: legacy HashMap-based keybinding storage.

use std::collections::HashMap;

use crate::action::Action;

use super::parse::{parse_bindings, parse_overview_bindings};
use super::types::KeyCombo;

pub struct KeybindMap {
    pub bindings: HashMap<KeyCombo, Action>,
}

impl Default for KeybindMap {
    fn default() -> Self {
        Self {
            bindings: default_bindings(),
        }
    }
}

impl KeybindMap {
    pub fn lookup(&self, combo: &KeyCombo) -> Option<Action> {
        self.bindings.get(combo).cloned()
    }

    /// Find the shortest key display string bound to a given action name.
    pub fn find_key_for_action(&self, action_name: &str) -> Option<String> {
        let target = Action::from_name(action_name)?;
        self.bindings
            .iter()
            .filter(|(_, a)| **a == target)
            .map(|(combo, _)| combo.display())
            .min_by_key(|s| s.len())
    }

    /// Default overview-mode keybindings.
    pub fn overview_default() -> Self {
        Self {
            bindings: overview_bindings(),
        }
    }

    /// Build a KeybindMap from defaults, then overlay config-driven bindings.
    pub fn from_config(bindings: &HashMap<String, String>) -> Self {
        let mut map = Self::default();
        map.bindings.extend(parse_bindings(bindings));
        map
    }

    /// Build overview KeybindMap from overview defaults, then overlay config.
    pub fn from_overview_config(bindings: &HashMap<String, String>) -> Self {
        let mut map = Self::overview_default();
        map.bindings.extend(parse_overview_bindings(bindings));
        map
    }

    /// Build a KeybindMap from ONLY the given bindings (no defaults).
    pub fn from_config_only(bindings: &HashMap<String, String>) -> Self {
        Self {
            bindings: parse_bindings(bindings),
        }
    }
}

fn default_bindings() -> HashMap<KeyCombo, Action> {
    let mut bindings = HashMap::new();

    bind_pairs(
        &mut bindings,
        [
            ("n", Action::NewColumnRight),
            ("d", Action::NewWorkspaceBelow),
            ("x", Action::ClosePane),
            ("h", Action::FocusLeft),
            ("l", Action::FocusRight),
            ("j", Action::FocusDown),
            ("k", Action::FocusUp),
            ("left", Action::FocusLeft),
            ("right", Action::FocusRight),
            ("up", Action::FocusUp),
            ("down", Action::FocusDown),
            ("r", Action::EnterMode("resize".into())),
            ("s", Action::EnterMode("scroll".into())),
            ("m", Action::EnterMode("move".into())),
            ("f", Action::ColumnWidthFull),
            ("c", Action::ConsumeIntoColumn),
            ("e", Action::ExpelFromColumn),
            ("b", Action::ToggleBroadcast),
            ("o", Action::ToggleOverview),
            ("tab", Action::ToggleOverview),
            ("p", Action::ToggleCommandPalette),
            ("g", Action::ToggleLock),
            ("q", Action::Detach),
        ],
    );

    bindings.insert(KeyCombo::with_shift("h"), Action::MovePaneLeft);
    bindings.insert(KeyCombo::with_shift("l"), Action::MovePaneRight);

    for i in 1u8..=9 {
        bindings.insert(
            KeyCombo::new(&i.to_string()),
            Action::SwitchWorkspace((i - 1) as usize),
        );
    }

    bindings
}

fn overview_bindings() -> HashMap<KeyCombo, Action> {
    let mut bindings = HashMap::new();
    bind_pairs(
        &mut bindings,
        [
            ("h", Action::FocusLeft),
            ("l", Action::FocusRight),
            ("j", Action::FocusDown),
            ("k", Action::FocusUp),
            ("left", Action::FocusLeft),
            ("right", Action::FocusRight),
            ("up", Action::FocusUp),
            ("down", Action::FocusDown),
            ("x", Action::ClosePane),
            ("n", Action::NewColumnRight),
            ("escape", Action::ExitOverview),
            ("enter", Action::ExitOverview),
            ("o", Action::ExitOverview),
            ("tab", Action::ExitOverview),
        ],
    );
    bindings
}

fn bind_pairs<const N: usize>(
    bindings: &mut HashMap<KeyCombo, Action>,
    pairs: [(&str, Action); N],
) {
    for (key, action) in pairs {
        bindings.insert(KeyCombo::new(key), action);
    }
}
