use crate::action::Action;
use std::collections::HashMap;

// ── BindingMode bitflags ──

/// Bitflags representing the current application input context.
/// Each binding declares which mode flags must / must-not be active for it to match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BindingMode(u16);

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

// ── Binding ──

/// A single keybinding with mode-awareness.
#[derive(Debug, Clone)]
pub struct Binding {
    pub combo: KeyCombo,
    pub action: Action,
    /// Mode flags that MUST all be active for this binding to match.
    pub mode: BindingMode,
    /// Mode flags that must NOT be active.
    pub notmode: BindingMode,
    /// If non-empty, only matches when this named key table is at the top of the stack.
    pub key_table: String,
}

impl Binding {
    /// Create a binding that is active in any mode (no mode requirement).
    pub fn global(combo: KeyCombo, action: Action) -> Self {
        Self {
            combo,
            action,
            mode: BindingMode::EMPTY,
            notmode: BindingMode::EMPTY,
            key_table: String::new(),
        }
    }

    /// Create a binding scoped to a specific mode.
    pub fn in_mode(combo: KeyCombo, action: Action, mode: BindingMode) -> Self {
        Self {
            combo,
            action,
            mode,
            notmode: BindingMode::EMPTY,
            key_table: String::new(),
        }
    }

    /// Create a binding scoped to a named key table.
    pub fn in_table(combo: KeyCombo, action: Action, table: &str) -> Self {
        Self {
            combo,
            action,
            mode: BindingMode::KEY_TABLE,
            notmode: BindingMode::EMPTY,
            key_table: table.to_string(),
        }
    }

    /// Check whether this binding matches the current context.
    pub fn matches(
        &self,
        current_mode: BindingMode,
        active_table: Option<&str>,
        combo: &KeyCombo,
    ) -> bool {
        self.combo == *combo
            && current_mode.contains(self.mode)
            && !current_mode.intersects(self.notmode)
            && (self.key_table.is_empty() || active_table == Some(self.key_table.as_str()))
    }
}

// ── BindingSet ──

/// Ordered collection of bindings. First-match-wins lookup.
/// User bindings are inserted before defaults so they take priority.
#[derive(Debug, Clone, Default)]
pub struct BindingSet {
    bindings: Vec<Binding>,
}

impl BindingSet {
    pub fn new() -> Self {
        Self {
            bindings: Vec::new(),
        }
    }

    /// Add a binding. Later bindings have lower priority (first-match-wins).
    pub fn push(&mut self, binding: Binding) {
        self.bindings.push(binding);
    }

    /// Prepend a binding (gives it highest priority).
    pub fn push_front(&mut self, binding: Binding) {
        self.bindings.insert(0, binding);
    }

    /// Append all bindings from another set (lower priority).
    pub fn extend(&mut self, other: BindingSet) {
        self.bindings.extend(other.bindings);
    }

    /// Look up the first matching binding for the given context.
    pub fn lookup(
        &self,
        current_mode: BindingMode,
        active_table: Option<&str>,
        combo: &KeyCombo,
    ) -> Option<&Binding> {
        self.bindings
            .iter()
            .find(|b| b.matches(current_mode, active_table, combo))
    }

    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Build a BindingSet from legacy config maps (backward compatibility).
    ///
    /// Converts old-style keybinding maps into mode-qualified bindings:
    /// - `leader_bindings` → mode: LEADER
    /// - `direct_bindings` → mode: NORMAL (no mode requirement except not in text-input modes)
    /// - `mode_keybinds` → key_table-qualified bindings
    /// - `overview_bindings` → mode: OVERVIEW
    /// - `search_bindings` → mode: SEARCH
    /// - `palette_bindings` → mode: PALETTE
    pub fn from_legacy(
        leader_bindings: &KeybindMap,
        direct_bindings: &KeybindMap,
        mode_keybinds: &HashMap<String, KeybindMap>,
        overview_bindings: &KeybindMap,
        search_bindings: &HashMap<String, String>,
        palette_bindings: &HashMap<String, String>,
    ) -> Self {
        let mut set = BindingSet::new();

        // Direct bindings: active in NORMAL, not in text-input overlays
        for (combo, action) in &direct_bindings.bindings {
            set.push(Binding {
                combo: combo.clone(),
                action: action.clone(),
                mode: BindingMode::EMPTY,
                notmode: BindingMode::SEARCH | BindingMode::PALETTE | BindingMode::PASTE_CONFIRM,
                key_table: String::new(),
            });
        }

        // Leader bindings: active when leader is pressed
        for (combo, action) in &leader_bindings.bindings {
            set.push(Binding::in_mode(
                combo.clone(),
                action.clone(),
                BindingMode::LEADER,
            ));
        }

        // Mode/key-table bindings
        for (table_name, table_map) in mode_keybinds {
            for (combo, action) in &table_map.bindings {
                set.push(Binding::in_table(
                    combo.clone(),
                    action.clone(),
                    table_name,
                ));
            }
        }

        // Overview bindings
        for (combo, action) in &overview_bindings.bindings {
            set.push(Binding::in_mode(
                combo.clone(),
                action.clone(),
                BindingMode::OVERVIEW,
            ));
        }

        // Search bindings
        let search_parsed = parse_bindings(search_bindings, BindingSource::KeybindingConfig);
        for (combo, action) in search_parsed {
            set.push(Binding::in_mode(combo, action, BindingMode::SEARCH));
        }

        // Palette bindings
        let palette_parsed = parse_bindings(palette_bindings, BindingSource::KeybindingConfig);
        for (combo, action) in palette_parsed {
            set.push(Binding::in_mode(combo, action, BindingMode::PALETTE));
        }

        set
    }
}

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
    use super::{Action, Binding, BindingMode, BindingSet, HashMap, KeyCombo, KeybindMap};

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

    // ── BindingMode tests ──

    #[test]
    fn binding_mode_contains_and_intersects() {
        let mode = BindingMode::NORMAL | BindingMode::OVERVIEW;
        assert!(mode.contains(BindingMode::NORMAL));
        assert!(mode.contains(BindingMode::OVERVIEW));
        assert!(mode.contains(BindingMode::NORMAL | BindingMode::OVERVIEW));
        assert!(!mode.contains(BindingMode::SEARCH));
        assert!(mode.intersects(BindingMode::OVERVIEW));
        assert!(!mode.intersects(BindingMode::SEARCH));
        assert!(!BindingMode::EMPTY.intersects(BindingMode::NORMAL));
    }

    // ── Binding matching tests ──

    #[test]
    fn binding_matches_exact_mode() {
        let binding = Binding::in_mode(
            KeyCombo::new("h"),
            Action::FocusLeft,
            BindingMode::OVERVIEW,
        );
        let combo = KeyCombo::new("h");

        // Matches when OVERVIEW is active
        assert!(binding.matches(BindingMode::OVERVIEW, None, &combo));
        // Also matches when OVERVIEW + NORMAL both active
        assert!(binding.matches(BindingMode::NORMAL | BindingMode::OVERVIEW, None, &combo));
        // Does NOT match when only NORMAL is active
        assert!(!binding.matches(BindingMode::NORMAL, None, &combo));
    }

    #[test]
    fn binding_notmode_blocks_match() {
        let binding = Binding {
            combo: KeyCombo::parse("ctrl+shift+c"),
            action: Action::ClipboardCopy,
            mode: BindingMode::EMPTY,
            notmode: BindingMode::SEARCH | BindingMode::PALETTE,
            key_table: String::new(),
        };
        let combo = KeyCombo::parse("ctrl+shift+c");

        // Matches in normal mode
        assert!(binding.matches(BindingMode::NORMAL, None, &combo));
        // Blocked in search mode
        assert!(!binding.matches(BindingMode::SEARCH, None, &combo));
        // Blocked in palette mode
        assert!(!binding.matches(BindingMode::PALETTE, None, &combo));
    }

    #[test]
    fn binding_key_table_filter() {
        let binding = Binding::in_table(
            KeyCombo::new("h"),
            Action::ColumnWidthDecrease,
            "resize",
        );
        let combo = KeyCombo::new("h");

        // Matches when resize table is active
        assert!(binding.matches(BindingMode::KEY_TABLE, Some("resize"), &combo));
        // Does NOT match when scroll table is active
        assert!(!binding.matches(BindingMode::KEY_TABLE, Some("scroll"), &combo));
        // Does NOT match when no table active
        assert!(!binding.matches(BindingMode::NORMAL, None, &combo));
    }

    // ── BindingSet lookup tests ──

    #[test]
    fn binding_set_first_match_wins() {
        let mut set = BindingSet::new();
        // User override: "h" in overview → ClosePane
        set.push(Binding::in_mode(
            KeyCombo::new("h"),
            Action::ClosePane,
            BindingMode::OVERVIEW,
        ));
        // Default: "h" in overview → FocusLeft
        set.push(Binding::in_mode(
            KeyCombo::new("h"),
            Action::FocusLeft,
            BindingMode::OVERVIEW,
        ));

        let combo = KeyCombo::new("h");
        let result = set.lookup(BindingMode::OVERVIEW, None, &combo);
        assert_eq!(result.map(|b| &b.action), Some(&Action::ClosePane));
    }

    #[test]
    fn binding_set_same_combo_different_modes() {
        let mut set = BindingSet::new();
        // "h" in OVERVIEW → FocusLeft
        set.push(Binding::in_mode(
            KeyCombo::new("h"),
            Action::FocusLeft,
            BindingMode::OVERVIEW,
        ));
        // "h" in LEADER → FocusLeft (different context)
        set.push(Binding::in_mode(
            KeyCombo::new("h"),
            Action::FocusLeft,
            BindingMode::LEADER,
        ));
        // "h" in resize table → ColumnWidthDecrease
        set.push(Binding::in_table(
            KeyCombo::new("h"),
            Action::ColumnWidthDecrease,
            "resize",
        ));

        let combo = KeyCombo::new("h");

        // In overview: FocusLeft
        let r = set.lookup(BindingMode::OVERVIEW, None, &combo);
        assert_eq!(r.map(|b| &b.action), Some(&Action::FocusLeft));

        // In resize table: ColumnWidthDecrease
        let r = set.lookup(BindingMode::KEY_TABLE, Some("resize"), &combo);
        assert_eq!(r.map(|b| &b.action), Some(&Action::ColumnWidthDecrease));

        // In normal mode: no match
        let r = set.lookup(BindingMode::NORMAL, None, &combo);
        assert!(r.is_none());
    }

    #[test]
    fn binding_set_from_legacy_round_trip() {
        let leader = KeybindMap::default();
        let direct = KeybindMap::from_config_only(&HashMap::from([
            ("ctrl+g".to_string(), "toggle_lock".to_string()),
        ]));
        let overview = KeybindMap::overview_default();
        let modes = HashMap::from([(
            "resize".to_string(),
            KeybindMap::from_config_only(&HashMap::from([
                ("h".to_string(), "column_width_decrease".to_string()),
            ])),
        )]);
        let search = HashMap::from([
            ("escape".to_string(), "close_search".to_string()),
        ]);
        let palette = HashMap::from([
            ("escape".to_string(), "close_command_palette".to_string()),
        ]);

        let set = BindingSet::from_legacy(&leader, &direct, &modes, &overview, &search, &palette);
        assert!(!set.is_empty());

        // Direct binding works in NORMAL
        let combo = KeyCombo::parse("ctrl+g");
        let r = set.lookup(BindingMode::NORMAL, None, &combo);
        assert_eq!(r.map(|b| &b.action), Some(&Action::ToggleLock));

        // Leader binding works in LEADER
        let combo = KeyCombo::new("n");
        let r = set.lookup(BindingMode::LEADER, None, &combo);
        assert_eq!(r.map(|b| &b.action), Some(&Action::NewColumnRight));

        // Overview binding works in OVERVIEW
        let combo = KeyCombo::new("escape");
        let r = set.lookup(BindingMode::OVERVIEW, None, &combo);
        assert_eq!(r.map(|b| &b.action), Some(&Action::ExitOverview));

        // Resize table works
        let combo = KeyCombo::new("h");
        let r = set.lookup(BindingMode::KEY_TABLE, Some("resize"), &combo);
        assert_eq!(r.map(|b| &b.action), Some(&Action::ColumnWidthDecrease));

        // Search binding works
        let combo = KeyCombo::new("escape");
        let r = set.lookup(BindingMode::SEARCH, None, &combo);
        assert_eq!(r.map(|b| &b.action), Some(&Action::CloseSearch));

        // Palette binding works
        let combo = KeyCombo::new("escape");
        let r = set.lookup(BindingMode::PALETTE, None, &combo);
        assert_eq!(r.map(|b| &b.action), Some(&Action::CloseCommandPalette));
    }
}
