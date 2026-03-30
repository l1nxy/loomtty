use super::*;
use crate::action::Action;
use crate::keybind::{Binding, BindingMode, BindingSet, KeyCombo, KeybindMap};
use std::collections::HashMap;
use std::time::{Duration, Instant};

fn prefix_handler() -> InputHandler {
    let mut h = InputHandler::new(Duration::from_millis(1000), Duration::from_millis(300));
    h.leader_key = LeaderKey::parse("ctrl+w");
    h
}

fn sticky_handler() -> InputHandler {
    let mut h = prefix_handler();
    h.input_mode = InputMode::Sticky;
    h
}

fn handler_with_modes() -> InputHandler {
    let mut h = sticky_handler();
    h.keybinds
        .bindings
        .insert(KeyCombo::new("r"), Action::EnterMode("resize".into()));
    h.keybinds
        .bindings
        .insert(KeyCombo::new("g"), Action::ToggleLock);

    let mut resize = KeybindMap {
        bindings: HashMap::new(),
    };
    resize
        .bindings
        .insert(KeyCombo::new("h"), Action::ColumnWidthDecrease);
    resize
        .bindings
        .insert(KeyCombo::new("l"), Action::ColumnWidthIncrease);
    h.mode_keybinds.insert("resize".into(), resize);
    h
}

// ── Prefix mode ──

#[test]
fn normal_key_passes_through() {
    let mut h = prefix_handler();
    assert!(matches!(
        h.process_key("a", false, false, false, false),
        InputResult::PassThrough
    ));
}

#[test]
fn leader_enters_awaiting() {
    let mut h = prefix_handler();
    assert!(matches!(
        h.process_key("w", true, false, false, false),
        InputResult::Consumed
    ));
    assert!(h.is_awaiting_action());
}

#[test]
fn leader_then_action() {
    let mut h = prefix_handler();
    h.process_key("w", true, false, false, false);
    assert!(matches!(
        h.process_key("n", false, false, false, false),
        InputResult::Action(Action::NewColumnRight)
    ));
    assert!(!h.is_awaiting_action());
}

#[test]
fn leader_then_unknown_consumed_and_exits() {
    let mut h = prefix_handler();
    h.process_key("w", true, false, false, false);
    assert!(matches!(
        h.process_key("z", false, false, false, false),
        InputResult::Consumed
    ));
    assert!(!h.is_awaiting_action()); // prefix exits on unknown
}

#[test]
fn leader_timeout_resets() {
    let mut h = prefix_handler();
    h.process_key("w", true, false, false, false);
    h.state = State::AwaitingAction {
        entered_at: Instant::now() - Duration::from_secs(2),
    };
    h.check_timeout();
    assert!(!h.is_awaiting_action());
}

#[test]
fn shift_keybinds() {
    let mut h = prefix_handler();
    h.process_key("w", true, false, false, false);
    assert!(matches!(
        h.process_key("H", false, true, false, false),
        InputResult::Action(Action::MovePaneLeft)
    ));
}

#[test]
fn prefix_esc_exits() {
    let mut h = prefix_handler();
    h.process_key("w", true, false, false, false);
    assert!(matches!(
        h.process_key("escape", false, false, false, false),
        InputResult::Consumed
    ));
    assert!(!h.is_awaiting_action());
}

// ── Sticky mode ──

#[test]
fn sticky_repeatable_stays_in_leader() {
    let mut h = sticky_handler();
    h.process_key("w", true, false, false, false);
    assert!(matches!(
        h.process_key("h", false, false, false, false),
        InputResult::Action(Action::FocusLeft)
    ));
    assert!(h.is_awaiting_action()); // stays
    assert!(matches!(
        h.process_key("l", false, false, false, false),
        InputResult::Action(Action::FocusRight)
    ));
    assert!(h.is_awaiting_action()); // still stays
}

#[test]
fn sticky_oneshot_exits() {
    let mut h = sticky_handler();
    h.process_key("w", true, false, false, false);
    assert!(matches!(
        h.process_key("n", false, false, false, false),
        InputResult::Action(Action::NewColumnRight)
    ));
    assert!(!h.is_awaiting_action());
}

#[test]
fn sticky_unknown_stays() {
    let mut h = sticky_handler();
    h.process_key("w", true, false, false, false);
    assert!(matches!(
        h.process_key("z", false, false, false, false),
        InputResult::Consumed
    ));
    assert!(h.is_awaiting_action()); // stays, unlike prefix
}

#[test]
fn sticky_no_timeout() {
    let mut h = sticky_handler();
    h.state = State::AwaitingAction {
        entered_at: Instant::now() - Duration::from_secs(100),
    };
    h.check_timeout();
    assert!(h.is_awaiting_action());
}

// ── Leader key parsing ──

#[test]
fn parse_leader_key_formats() {
    let k = LeaderKey::parse("ctrl+w");
    assert!(k.ctrl && !k.alt && k.key == "w");
    let k = LeaderKey::parse("alt");
    assert!(!k.ctrl && k.alt && k.key.is_empty() && k.is_bare_modifier());
    let k = LeaderKey::parse("ctrl+space");
    assert!(k.ctrl && k.key == "space" && !k.is_bare_modifier());
}

// ── Bare modifier leader (Alt) ──

#[test]
fn alt_leader_enters_awaiting() {
    let mut h = prefix_handler();
    h.leader_key = LeaderKey::parse("alt");
    assert!(matches!(
        h.process_key("Alt", false, false, true, false),
        InputResult::Consumed
    ));
    assert!(h.is_awaiting_action());
}

#[test]
fn alt_held_strips_modifier() {
    let mut h = prefix_handler();
    h.leader_key = LeaderKey::parse("alt");
    h.process_key("Alt", false, false, true, false);
    // Alt still held → alt=true, but should be stripped
    assert!(matches!(
        h.process_key("h", false, false, true, false),
        InputResult::Action(Action::FocusLeft)
    ));
}

#[test]
fn alt_release_exits_awaiting() {
    let mut h = sticky_handler();
    h.leader_key = LeaderKey::parse("alt");
    h.process_key("Alt", false, false, true, false);
    h.process_key_release("Alt");
    assert!(!h.is_awaiting_action());
}

#[test]
fn ctrl_w_release_does_not_affect_leader() {
    let mut h = sticky_handler();
    h.process_key("w", true, false, false, false);
    h.process_key_release("w");
    assert!(h.is_awaiting_action()); // unaffected
}

// ── Named modes (key tables) ──

#[test]
fn enter_mode_from_leader() {
    let mut h = handler_with_modes();
    h.process_key("w", true, false, false, false);
    assert!(matches!(
        h.process_key("r", false, false, false, false),
        InputResult::Consumed
    ));
    assert_eq!(h.current_mode_name(), Some("resize"));
}

#[test]
fn mode_dispatches_and_stays() {
    let mut h = handler_with_modes();
    h.process_key("w", true, false, false, false);
    h.process_key("r", false, false, false, false);
    assert!(matches!(
        h.process_key("h", false, false, false, false),
        InputResult::Action(Action::ColumnWidthDecrease)
    ));
    assert_eq!(h.current_mode_name(), Some("resize"));
}

#[test]
fn mode_esc_exits() {
    let mut h = handler_with_modes();
    h.process_key("w", true, false, false, false);
    h.process_key("r", false, false, false, false);
    h.process_key("escape", false, false, false, false);
    assert!(!h.is_awaiting_action());
}

#[test]
fn mode_unknown_key_consumed() {
    let mut h = handler_with_modes();
    h.process_key("w", true, false, false, false);
    h.process_key("r", false, false, false, false);
    assert!(matches!(
        h.process_key("z", false, false, false, false),
        InputResult::Consumed
    ));
    assert_eq!(h.current_mode_name(), Some("resize"));
}

#[test]
fn alt_release_does_not_exit_mode() {
    let mut h = handler_with_modes();
    h.leader_key = LeaderKey::parse("alt");
    h.process_key("Alt", false, false, true, false);
    h.process_key("r", false, false, true, false);
    h.process_key_release("Alt");
    assert_eq!(h.current_mode_name(), Some("resize")); // stays
}

// ── Locked mode ──

#[test]
fn toggle_lock() {
    let mut h = handler_with_modes();
    h.toggle_lock();
    assert!(h.is_locked());
    h.toggle_lock();
    assert!(!h.is_locked());
}

#[test]
fn locked_passes_through() {
    let mut h = handler_with_modes();
    h.toggle_lock();
    assert!(matches!(
        h.process_key("a", false, false, false, false),
        InputResult::PassThrough
    ));
    assert!(h.is_locked());
}

#[test]
fn locked_leader_then_unlock() {
    let mut h = handler_with_modes();
    h.toggle_lock();
    h.process_key("w", true, false, false, false); // leader → AwaitingUnlock
    assert!(matches!(h.state, State::AwaitingUnlock));
    h.process_key("g", false, false, false, false); // unlock key
    assert!(!h.is_locked());
}

#[test]
fn locked_leader_then_wrong_key_stays_locked() {
    let mut h = handler_with_modes();
    h.toggle_lock();
    h.process_key("w", true, false, false, false);
    assert!(matches!(
        h.process_key("z", false, false, false, false),
        InputResult::PassThrough
    ));
    assert!(matches!(h.state, State::Locked));
}

#[test]
fn locked_alt_release_cancels_unlock() {
    let mut h = handler_with_modes();
    h.leader_key = LeaderKey::parse("alt");
    h.toggle_lock();
    h.process_key("Alt", false, false, true, false);
    h.process_key_release("Alt");
    assert!(matches!(h.state, State::Locked));
}

// ── Direct bindings ──

#[test]
fn direct_binding_enters_mode() {
    let mut h = handler_with_modes();
    h.direct_keybinds.bindings.insert(
        KeyCombo::parse("ctrl+r"),
        Action::EnterMode("resize".into()),
    );
    assert!(matches!(
        h.process_key("r", true, false, false, false),
        InputResult::Consumed
    ));
    assert_eq!(h.current_mode_name(), Some("resize"));
}

#[test]
fn direct_binding_toggle_lock() {
    let mut h = handler_with_modes();
    h.direct_keybinds
        .bindings
        .insert(KeyCombo::parse("ctrl+g"), Action::ToggleLock);
    h.process_key("g", true, false, false, false);
    assert!(h.is_locked());
}

#[test]
fn direct_binding_fires_action() {
    let mut h = prefix_handler();
    h.direct_keybinds
        .bindings
        .insert(KeyCombo::parse("alt+h"), Action::FocusLeft);
    assert!(matches!(
        h.process_key("h", false, false, true, false),
        InputResult::Action(Action::FocusLeft)
    ));
    assert!(!h.is_awaiting_action()); // stays Idle
}

#[test]
fn locked_direct_unlock() {
    let mut h = handler_with_modes();
    h.direct_keybinds
        .bindings
        .insert(KeyCombo::parse("ctrl+g"), Action::ToggleLock);
    h.toggle_lock();
    // Ctrl+G should unlock directly (via direct binding in Locked state)
    h.process_key("g", true, false, false, false);
    assert!(!h.is_locked());
}

// ══════════════════════════════════════════════
//  process_key_event tests
// ══════════════════════════════════════════════

/// Helper: build a handler with BindingSet populated for testing.
fn make_test_handler() -> InputHandler {
    let mut h = InputHandler::new(Duration::from_millis(1000), Duration::from_millis(300));
    h.leader_key = LeaderKey::parse("ctrl+w");
    h.input_mode = InputMode::Prefix;

    let mut set = BindingSet::new();

    // Leader bindings (mode: LEADER)
    set.push(Binding::in_mode(
        KeyCombo::new("n"),
        Action::NewColumnRight,
        BindingMode::LEADER,
    ));
    set.push(Binding::in_mode(
        KeyCombo::new("h"),
        Action::FocusLeft,
        BindingMode::LEADER,
    ));
    set.push(Binding::in_mode(
        KeyCombo::new("l"),
        Action::FocusRight,
        BindingMode::LEADER,
    ));
    set.push(Binding::in_mode(
        KeyCombo::new("o"),
        Action::ToggleOverview,
        BindingMode::LEADER,
    ));
    set.push(Binding::in_mode(
        KeyCombo::new("r"),
        Action::EnterMode("resize".into()),
        BindingMode::LEADER,
    ));
    set.push(Binding::in_mode(
        KeyCombo::new("g"),
        Action::ToggleLock,
        BindingMode::LEADER,
    ));

    // Direct bindings (no mode requirement, but not in text-input overlays)
    set.push(Binding {
        combo: KeyCombo::parse("ctrl+g"),
        action: Action::ToggleLock,
        mode: BindingMode::EMPTY,
        notmode: BindingMode::SEARCH | BindingMode::PALETTE,
        key_table: String::new(),
    });

    // Resize key table
    set.push(Binding::in_table(
        KeyCombo::new("h"),
        Action::ColumnWidthDecrease,
        "resize",
    ));
    set.push(Binding::in_table(
        KeyCombo::new("l"),
        Action::ColumnWidthIncrease,
        "resize",
    ));

    // Overview bindings
    set.push(Binding::in_mode(
        KeyCombo::new("h"),
        Action::FocusLeft,
        BindingMode::OVERVIEW,
    ));
    set.push(Binding::in_mode(
        KeyCombo::new("escape"),
        Action::ExitOverview,
        BindingMode::OVERVIEW,
    ));

    // Search bindings
    set.push(Binding::in_mode(
        KeyCombo::new("escape"),
        Action::CloseSearch,
        BindingMode::SEARCH,
    ));
    set.push(Binding::in_mode(
        KeyCombo::new("enter"),
        Action::SearchNextMatch,
        BindingMode::SEARCH,
    ));

    // Palette bindings
    set.push(Binding::in_mode(
        KeyCombo::new("escape"),
        Action::CloseCommandPalette,
        BindingMode::PALETTE,
    ));
    set.push(Binding::in_mode(
        KeyCombo::new("up"),
        Action::PaletteUp,
        BindingMode::PALETTE,
    ));
    set.push(Binding::in_mode(
        KeyCombo::new("down"),
        Action::PaletteDown,
        BindingMode::PALETTE,
    ));
    set.push(Binding::in_mode(
        KeyCombo::new("enter"),
        Action::PaletteConfirm,
        BindingMode::PALETTE,
    ));

    h.binding_set = set;
    h
}

#[test]
fn unified_normal_passthrough() {
    let mut h = make_test_handler();
    let r = h.process_key_event("a", false, false, false, false, BindingMode::EMPTY);
    assert!(matches!(r, InputResult::PassThrough));
}

#[test]
fn unified_leader_then_action() {
    let mut h = make_test_handler();
    // Press leader
    let r = h.process_key_event("w", true, false, false, false, BindingMode::EMPTY);
    assert!(matches!(r, InputResult::Consumed));
    assert!(h.is_awaiting_action());
    // Press action key
    let r = h.process_key_event("n", false, false, false, false, BindingMode::EMPTY);
    assert!(matches!(r, InputResult::Action(Action::NewColumnRight)));
}

#[test]
fn unified_leader_escape_exits() {
    let mut h = make_test_handler();
    h.process_key_event("w", true, false, false, false, BindingMode::EMPTY);
    let r = h.process_key_event("escape", false, false, false, false, BindingMode::EMPTY);
    assert!(matches!(r, InputResult::Consumed));
    assert!(!h.is_awaiting_action());
}

#[test]
fn unified_enter_mode_pushes_key_table() {
    let mut h = make_test_handler();
    h.process_key_event("w", true, false, false, false, BindingMode::EMPTY);
    h.process_key_event("r", false, false, false, false, BindingMode::EMPTY);
    assert!(h.has_active_table());
    assert_eq!(h.active_table_name(), Some("resize"));
}

#[test]
fn unified_key_table_dispatches_action() {
    let mut h = make_test_handler();
    // Enter resize table
    h.process_key_event("w", true, false, false, false, BindingMode::EMPTY);
    h.process_key_event("r", false, false, false, false, BindingMode::EMPTY);
    // Press "h" in resize table
    let r = h.process_key_event("h", false, false, false, false, BindingMode::EMPTY);
    assert!(matches!(
        r,
        InputResult::Action(Action::ColumnWidthDecrease)
    ));
    // Still in resize table
    assert!(h.has_active_table());
}

#[test]
fn unified_escape_pops_key_table() {
    let mut h = make_test_handler();
    h.process_key_event("w", true, false, false, false, BindingMode::EMPTY);
    h.process_key_event("r", false, false, false, false, BindingMode::EMPTY);
    assert!(h.has_active_table());
    h.process_key_event("escape", false, false, false, false, BindingMode::EMPTY);
    assert!(!h.has_active_table());
}

#[test]
fn unified_overview_binding() {
    let mut h = make_test_handler();
    let r = h.process_key_event("h", false, false, false, false, BindingMode::OVERVIEW);
    assert!(matches!(r, InputResult::Action(Action::FocusLeft)));
}

#[test]
fn unified_overview_escape() {
    let mut h = make_test_handler();
    let r = h.process_key_event("escape", false, false, false, false, BindingMode::OVERVIEW);
    assert!(matches!(r, InputResult::Action(Action::ExitOverview)));
}

#[test]
fn unified_overview_leader_toggle() {
    let mut h = make_test_handler();
    // In overview mode, press leader then "o" → should toggle overview
    h.process_key_event("w", true, false, false, false, BindingMode::OVERVIEW);
    let r = h.process_key_event("o", false, false, false, false, BindingMode::OVERVIEW);
    assert!(matches!(r, InputResult::Action(Action::ToggleOverview)));
}

#[test]
fn unified_search_control_keys() {
    let mut h = make_test_handler();
    let r = h.process_key_event("escape", false, false, false, false, BindingMode::SEARCH);
    assert!(matches!(r, InputResult::Action(Action::CloseSearch)));

    let r = h.process_key_event("enter", false, false, false, false, BindingMode::SEARCH);
    assert!(matches!(r, InputResult::Action(Action::SearchNextMatch)));
}

#[test]
fn unified_search_text_input_fallback() {
    let mut h = make_test_handler();
    // Unmatched key in SEARCH mode → TextInput
    let r = h.process_key_event("a", false, false, false, false, BindingMode::SEARCH);
    assert!(matches!(r, InputResult::Action(Action::TextInput)));
}

#[test]
fn unified_palette_navigation() {
    let mut h = make_test_handler();
    let r = h.process_key_event("up", false, false, false, false, BindingMode::PALETTE);
    assert!(matches!(r, InputResult::Action(Action::PaletteUp)));

    let r = h.process_key_event("down", false, false, false, false, BindingMode::PALETTE);
    assert!(matches!(r, InputResult::Action(Action::PaletteDown)));

    let r = h.process_key_event("enter", false, false, false, false, BindingMode::PALETTE);
    assert!(matches!(r, InputResult::Action(Action::PaletteConfirm)));
}

#[test]
fn unified_palette_text_input_fallback() {
    let mut h = make_test_handler();
    let r = h.process_key_event("x", false, false, false, false, BindingMode::PALETTE);
    assert!(matches!(r, InputResult::Action(Action::TextInput)));
}

#[test]
fn unified_locked_passthrough() {
    let mut h = make_test_handler();
    let r = h.process_key_event("a", false, false, false, false, BindingMode::LOCKED);
    assert!(matches!(r, InputResult::PassThrough));
}

#[test]
fn unified_locked_direct_toggle_lock() {
    let mut h = make_test_handler();
    let r = h.process_key_event("g", true, false, false, false, BindingMode::LOCKED);
    assert!(matches!(r, InputResult::Action(Action::ToggleLock)));
}

#[test]
fn unified_locked_leader_then_unlock() {
    let mut h = make_test_handler();
    // Press leader while locked
    h.process_key_event("w", true, false, false, false, BindingMode::LOCKED);
    assert!(matches!(h.state, State::AwaitingUnlock));
    // Press "g" to unlock
    let r = h.process_key_event("g", false, false, false, false, BindingMode::LOCKED);
    assert!(matches!(r, InputResult::Action(Action::ToggleLock)));
    assert!(!matches!(h.state, State::AwaitingUnlock));
}

#[test]
fn unified_sticky_mode_repeatable_stays() {
    let mut h = make_test_handler();
    h.input_mode = InputMode::Sticky;
    h.process_key_event("w", true, false, false, false, BindingMode::EMPTY);
    let r = h.process_key_event("h", false, false, false, false, BindingMode::EMPTY);
    assert!(matches!(r, InputResult::Action(Action::FocusLeft)));
    assert!(h.is_awaiting_action()); // stays in leader
}

#[test]
fn unified_sticky_mode_oneshot_exits() {
    let mut h = make_test_handler();
    h.input_mode = InputMode::Sticky;
    h.process_key_event("w", true, false, false, false, BindingMode::EMPTY);
    let r = h.process_key_event("n", false, false, false, false, BindingMode::EMPTY);
    assert!(matches!(r, InputResult::Action(Action::NewColumnRight)));
    assert!(!h.is_awaiting_action()); // exits
}
