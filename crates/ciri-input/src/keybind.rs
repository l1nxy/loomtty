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
        Self {
            bindings: default_bindings(),
        }
    }
}

impl KeybindMap {
    pub fn lookup(&self, combo: &KeyCombo) -> Option<Action> {
        self.bindings.get(combo).cloned()
    }

    /// Default overview-mode keybindings.
    pub fn overview_default() -> Self {
        Self {
            bindings: overview_bindings(),
        }
    }

    /// Build a KeybindMap from defaults, then overlay config-driven bindings.
    pub fn from_config(bindings: &HashMap<String, String>) -> Self {
        Self::with_overrides(Self::default(), bindings, BindingSource::KeybindingConfig)
    }

    /// Build overview KeybindMap from overview defaults, then overlay config.
    pub fn from_overview_config(bindings: &HashMap<String, String>) -> Self {
        Self::with_overrides(
            Self::overview_default(),
            bindings,
            BindingSource::OverviewKeybindingConfig,
        )
    }

    /// Build a KeybindMap from ONLY the given bindings (no defaults).
    pub fn from_config_only(bindings: &HashMap<String, String>) -> Self {
        Self {
            bindings: parse_bindings(bindings, BindingSource::KeybindingConfig),
        }
    }

    fn with_overrides(
        mut map: Self,
        bindings: &HashMap<String, String>,
        source: BindingSource,
    ) -> Self {
        map.bindings.extend(parse_bindings(bindings, source));
        map
    }
}

#[derive(Clone, Copy)]
enum Modifier {
    Shift,
    Alt,
    Ctrl,
    Super,
}

#[derive(Clone, Copy)]
enum BindingSource {
    KeybindingConfig,
    OverviewKeybindingConfig,
}

impl BindingSource {
    fn label(self) -> &'static str {
        match self {
            BindingSource::KeybindingConfig => "keybinding config",
            BindingSource::OverviewKeybindingConfig => "overview keybinding config",
        }
    }
}

fn parse_modifier_prefix(input: &str) -> Option<(Modifier, &str)> {
    for (prefix, modifier) in [
        ("shift+", Modifier::Shift),
        ("alt+", Modifier::Alt),
        ("ctrl+", Modifier::Ctrl),
        ("super+", Modifier::Super),
    ] {
        if input
            .get(..prefix.len())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
        {
            let rest = &input[prefix.len()..];
            return Some((modifier, rest));
        }
    }

    None
}

fn split_modifier_prefixes(mut input: &str) -> (&str, Vec<Modifier>) {
    let mut modifiers = Vec::new();

    while let Some((modifier, rest)) = parse_modifier_prefix(input) {
        modifiers.push(modifier);
        input = rest;
    }

    (input, modifiers)
}

fn apply_modifiers(combo: &mut KeyCombo, modifiers: Vec<Modifier>) {
    for modifier in modifiers {
        match modifier {
            Modifier::Shift => combo.shift = true,
            Modifier::Alt => combo.alt = true,
            Modifier::Ctrl => combo.ctrl = true,
            Modifier::Super => combo.super_key = true,
        }
    }
}

fn parse_bindings(
    bindings: &HashMap<String, String>,
    source: BindingSource,
) -> HashMap<KeyCombo, Action> {
    let mut parsed = HashMap::new();

    for (key_str, action_str) in bindings {
        match Action::from_name(action_str) {
            Some(action) => {
                parsed.insert(KeyCombo::parse(key_str), action);
            }
            None => warn_unknown_action(source, key_str, action_str),
        }
    }

    parsed
}

fn warn_unknown_action(source: BindingSource, key: &str, action: &str) {
    log::warn!(
        "unknown action in {}: {action:?} (key: {key:?})",
        source.label()
    );
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

#[cfg(test)]
mod tests {
    use super::{Action, HashMap, KeyCombo, KeybindMap};

    #[test]
    fn parses_key_combo_with_multiple_modifiers() {
        let combo = KeyCombo::parse("ctrl+alt+shift+super+h");
        assert_eq!(combo.key, "h");
        assert!(combo.ctrl);
        assert!(combo.alt);
        assert!(combo.shift);
        assert!(combo.super_key);
    }

    #[test]
    fn default_bindings_cover_modes_navigation_and_workspace_switching() {
        let map = KeybindMap::default();

        assert_eq!(
            map.lookup(&KeyCombo::new("n")),
            Some(Action::NewColumnRight)
        );
        assert_eq!(
            map.lookup(&KeyCombo::new("r")),
            Some(Action::EnterMode("resize".into()))
        );
        assert_eq!(
            map.lookup(&KeyCombo::with_shift("h")),
            Some(Action::MovePaneLeft)
        );
        assert_eq!(
            map.lookup(&KeyCombo::new("9")),
            Some(Action::SwitchWorkspace(8))
        );
    }

    #[test]
    fn overview_config_overrides_defaults() {
        let mut bindings = HashMap::new();
        bindings.insert("tab".to_string(), "close_pane".to_string());

        let map = KeybindMap::from_overview_config(&bindings);

        assert_eq!(map.lookup(&KeyCombo::new("tab")), Some(Action::ClosePane));
        assert_eq!(
            map.lookup(&KeyCombo::new("escape")),
            Some(Action::ExitOverview)
        );
    }

    #[test]
    fn config_only_ignores_unknown_actions_and_parses_enter_mode() {
        let mut bindings = HashMap::new();
        bindings.insert("ctrl+r".to_string(), "enter_mode:resize".to_string());
        bindings.insert("ctrl+x".to_string(), "not_real".to_string());

        let map = KeybindMap::from_config_only(&bindings);

        assert_eq!(
            map.lookup(&KeyCombo::parse("ctrl+r")),
            Some(Action::EnterMode("resize".into()))
        );
        assert_eq!(map.lookup(&KeyCombo::parse("ctrl+x")), None);
    }

    #[test]
    fn mixed_case_config_bindings_match_lowercase_modifier_prefixes() {
        let mut bindings = HashMap::new();
        bindings.insert("Ctrl+r".to_string(), "enter_mode:resize".to_string());
        bindings.insert("Alt+h".to_string(), "focus_left".to_string());
        bindings.insert("Super+1".to_string(), "switch_workspace_1".to_string());

        let map = KeybindMap::from_config_only(&bindings);

        assert_eq!(
            map.lookup(&KeyCombo::parse("ctrl+r")),
            Some(Action::EnterMode("resize".into()))
        );
        assert_eq!(map.lookup(&KeyCombo::parse("alt+h")), Some(Action::FocusLeft));
        assert_eq!(
            map.lookup(&KeyCombo::parse("super+1")),
            Some(Action::SwitchWorkspace(1))
        );
    }
}
