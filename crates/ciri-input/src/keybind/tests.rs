use super::{Binding, BindingMode, BindingSet, KeyCombo, KeybindMap};
use crate::action::Action;
use std::collections::HashMap;

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

    assert!(binding.matches(BindingMode::OVERVIEW, None, &combo));
    assert!(binding.matches(BindingMode::NORMAL | BindingMode::OVERVIEW, None, &combo));
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

    assert!(binding.matches(BindingMode::NORMAL, None, &combo));
    assert!(!binding.matches(BindingMode::SEARCH, None, &combo));
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

    assert!(binding.matches(BindingMode::KEY_TABLE, Some("resize"), &combo));
    assert!(!binding.matches(BindingMode::KEY_TABLE, Some("scroll"), &combo));
    assert!(!binding.matches(BindingMode::NORMAL, None, &combo));
}

// ── BindingSet lookup tests ──

#[test]
fn binding_set_first_match_wins() {
    let mut set = BindingSet::new();
    set.push(Binding::in_mode(
        KeyCombo::new("h"),
        Action::ClosePane,
        BindingMode::OVERVIEW,
    ));
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
    set.push(Binding::in_mode(
        KeyCombo::new("h"),
        Action::FocusLeft,
        BindingMode::OVERVIEW,
    ));
    set.push(Binding::in_mode(
        KeyCombo::new("h"),
        Action::FocusLeft,
        BindingMode::LEADER,
    ));
    set.push(Binding::in_table(
        KeyCombo::new("h"),
        Action::ColumnWidthDecrease,
        "resize",
    ));

    let combo = KeyCombo::new("h");

    let r = set.lookup(BindingMode::OVERVIEW, None, &combo);
    assert_eq!(r.map(|b| &b.action), Some(&Action::FocusLeft));

    let r = set.lookup(BindingMode::KEY_TABLE, Some("resize"), &combo);
    assert_eq!(r.map(|b| &b.action), Some(&Action::ColumnWidthDecrease));

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
    let paste_confirm = HashMap::from([
        ("enter".to_string(), "confirm_paste".to_string()),
        ("escape".to_string(), "dismiss_paste_confirm".to_string()),
    ]);

    let set = BindingSet::from_legacy(&leader, &direct, &modes, &overview, &search, &palette, &paste_confirm);
    assert!(!set.is_empty());

    let combo = KeyCombo::parse("ctrl+g");
    let r = set.lookup(BindingMode::NORMAL, None, &combo);
    assert_eq!(r.map(|b| &b.action), Some(&Action::ToggleLock));

    let combo = KeyCombo::new("n");
    let r = set.lookup(BindingMode::LEADER, None, &combo);
    assert_eq!(r.map(|b| &b.action), Some(&Action::NewColumnRight));

    let combo = KeyCombo::new("escape");
    let r = set.lookup(BindingMode::OVERVIEW, None, &combo);
    assert_eq!(r.map(|b| &b.action), Some(&Action::ExitOverview));

    let combo = KeyCombo::new("h");
    let r = set.lookup(BindingMode::KEY_TABLE, Some("resize"), &combo);
    assert_eq!(r.map(|b| &b.action), Some(&Action::ColumnWidthDecrease));

    let combo = KeyCombo::new("escape");
    let r = set.lookup(BindingMode::SEARCH, None, &combo);
    assert_eq!(r.map(|b| &b.action), Some(&Action::CloseSearch));

    let combo = KeyCombo::new("escape");
    let r = set.lookup(BindingMode::PALETTE, None, &combo);
    assert_eq!(r.map(|b| &b.action), Some(&Action::CloseCommandPalette));

    let combo = KeyCombo::new("enter");
    let r = set.lookup(BindingMode::PASTE_CONFIRM, None, &combo);
    assert_eq!(r.map(|b| &b.action), Some(&Action::ConfirmPaste));

    let combo = KeyCombo::new("escape");
    let r = set.lookup(BindingMode::PASTE_CONFIRM, None, &combo);
    assert_eq!(r.map(|b| &b.action), Some(&Action::DismissPasteConfirm));
}
