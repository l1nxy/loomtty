use super::*;
use crate::action::Action;
use crate::keybind::{Binding, BindingMode, BindingSet, KeyCombo};
use crate::leader::types::{KeyTableState, LeaderSession};
use std::time::{Duration, Instant};

fn prefix_handler() -> InputHandler {
    let mut h = InputHandler::new(Duration::from_millis(1000), Duration::from_millis(300));
    h.leader_key = LeaderKey::parse("ctrl+w");
    h.set_binding_set(make_binding_set());
    h
}

fn sticky_handler() -> InputHandler {
    let mut h = prefix_handler();
    h.input_mode = InputMode::Sticky;
    h
}

fn make_binding_set() -> BindingSet {
    let mut set = BindingSet::new();

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
        KeyCombo::with_shift("h"),
        Action::MovePaneLeft,
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
        KeyCombo::new("m"),
        Action::EnterMode("move".into()),
        BindingMode::LEADER,
    ));
    set.push(Binding::in_mode(
        KeyCombo::new("g"),
        Action::ToggleLock,
        BindingMode::LEADER,
    ));

    set.push(Binding {
        combo: KeyCombo::parse("ctrl+g"),
        action: Action::ToggleLock,
        mode: BindingMode::EMPTY,
        notmode: BindingMode::SEARCH | BindingMode::PALETTE,
        key_table: String::new(),
    });
    set.push(Binding {
        combo: KeyCombo::parse("alt+h"),
        action: Action::FocusLeft,
        mode: BindingMode::EMPTY,
        notmode: BindingMode::SEARCH | BindingMode::PALETTE,
        key_table: String::new(),
    });
    set.push(Binding {
        combo: KeyCombo::parse("ctrl+r"),
        action: Action::EnterMode("resize".into()),
        mode: BindingMode::EMPTY,
        notmode: BindingMode::SEARCH | BindingMode::PALETTE,
        key_table: String::new(),
    });

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
    set.push(Binding::in_table(
        KeyCombo::new("h"),
        Action::MovePaneLeft,
        "move",
    ));
    set.push(Binding::in_table(
        KeyCombo::new("l"),
        Action::MovePaneRight,
        "move",
    ));

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

    set
}

// ── Basic handler tests ──

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
    assert!(!h.is_awaiting_action());
}

#[test]
fn leader_timeout_resets() {
    let mut h = prefix_handler();
    h.process_key("w", true, false, false, false);
    h.session = InputSessionState::Leader(LeaderSession {
        mode: InputMode::Prefix,
        entered_at: Instant::now() - Duration::from_secs(2),
    });
    h.poll_timeout();
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

#[test]
fn sticky_repeatable_stays_in_leader() {
    let mut h = sticky_handler();
    h.process_key("w", true, false, false, false);
    assert!(matches!(
        h.process_key("h", false, false, false, false),
        InputResult::Action(Action::FocusLeft)
    ));
    assert!(h.is_awaiting_action());
    assert!(matches!(
        h.process_key("l", false, false, false, false),
        InputResult::Action(Action::FocusRight)
    ));
    assert!(h.is_awaiting_action());
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
    assert!(h.is_awaiting_action());
}

#[test]
fn sticky_no_timeout() {
    let mut h = sticky_handler();
    h.session = InputSessionState::Leader(LeaderSession {
        mode: InputMode::Sticky,
        entered_at: Instant::now() - Duration::from_secs(100),
    });
    h.poll_timeout();
    assert!(h.is_awaiting_action());
}

#[test]
fn parse_leader_key_formats() {
    let k = LeaderKey::parse("ctrl+w");
    assert!(k.ctrl && !k.alt && k.key == "w");
    let k = LeaderKey::parse("alt");
    assert!(!k.ctrl && k.alt && k.key.is_empty() && k.is_bare_modifier());
    let k = LeaderKey::parse("ctrl+space");
    assert!(k.ctrl && k.key == "space" && !k.is_bare_modifier());
}

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

/// Sticky + bare-modifier (`alt`) leader promotes every leader binding
/// to an `alt+<key>` *direct* chord (`reload_bindings` →
/// `promote_leader_bindings_to_direct`). Verifies the real config path:
/// `Alt+/` fires `ToggleHelp` in a single press with no prior Alt tap,
/// and Alt alone does NOT arm a leader session (the leader is disabled).
#[test]
fn sticky_alt_leader_promotes_slash_to_direct_toggle_help() {
    use std::collections::HashMap;

    let mut h = InputHandler::new(Duration::from_millis(1000), Duration::from_millis(300));
    let mut bindings = HashMap::new();
    bindings.insert("/".to_string(), "toggle_help".to_string());
    bindings.insert("n".to_string(), "new_column_right".to_string());
    let modes: HashMap<String, HashMap<String, String>> = HashMap::new();
    let direct: HashMap<String, String> = HashMap::new();
    h.reload_bindings("alt", "sticky", &bindings, &modes, &direct);

    let empty: HashMap<String, String> = HashMap::new();
    let set = BindingSet::from_legacy(
        &h.keybinds,
        &h.direct_keybinds,
        &h.mode_keybinds,
        &crate::keybind::KeybindMap::from_overview_config(&empty),
        &empty,
        &empty,
        &empty,
    );
    h.set_binding_set(set);

    // Alt alone must NOT enter a leader session — promotion disables the
    // leader key, so the bare modifier is a no-op (passes through).
    let r = h.process_key("Alt", false, false, true, false);
    assert!(matches!(r, InputResult::PassThrough));
    assert!(!h.is_awaiting_action());

    // Alt+/ in a single press dispatches ToggleHelp directly.
    let r = h.process_key("/", false, false, true, false);
    assert!(matches!(r, InputResult::Action(Action::ToggleHelp)));
}

#[test]
fn alt_held_strips_modifier() {
    let mut h = prefix_handler();
    h.leader_key = LeaderKey::parse("alt");
    h.process_key("Alt", false, false, true, false);
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
    assert!(h.is_awaiting_action());
}

#[test]
fn enter_mode_from_leader() {
    let mut h = sticky_handler();
    h.process_key("w", true, false, false, false);
    assert!(matches!(
        h.process_key("r", false, false, false, false),
        InputResult::Consumed
    ));
    assert_eq!(h.current_mode_name(), Some("resize"));
}

#[test]
fn mode_dispatches_and_stays() {
    let mut h = sticky_handler();
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
    let mut h = sticky_handler();
    h.process_key("w", true, false, false, false);
    h.process_key("r", false, false, false, false);
    h.process_key("escape", false, false, false, false);
    assert!(!h.is_awaiting_action());
    assert_eq!(h.current_mode_name(), None);
}

#[test]
fn mode_unknown_key_passes_through() {
    let mut h = sticky_handler();
    h.process_key("w", true, false, false, false);
    h.process_key("r", false, false, false, false);
    assert!(matches!(
        h.process_key("z", false, false, false, false),
        InputResult::PassThrough
    ));
    assert_eq!(h.current_mode_name(), Some("resize"));
}

#[test]
fn alt_leader_can_enter_mode() {
    let mut h = sticky_handler();
    h.leader_key = LeaderKey::parse("alt");
    h.process_key("Alt", false, false, true, false);
    h.process_key("r", false, false, true, false);
    h.process_key_release("Alt");
    assert_eq!(h.current_mode_name(), Some("resize"));
}

#[test]
fn toggle_lock() {
    let mut h = prefix_handler();
    h.toggle_lock();
    assert!(h.is_locked());
    h.toggle_lock();
    assert!(!h.is_locked());
}

#[test]
fn locked_passes_through() {
    let mut h = prefix_handler();
    h.toggle_lock();
    assert!(matches!(
        h.process_key("a", false, false, false, false),
        InputResult::PassThrough
    ));
}

#[test]
fn locked_leader_then_unlock() {
    let mut h = sticky_handler();
    h.toggle_lock();
    let app_mode = BindingMode::LOCKED;
    let r = h.process_key_event("w", true, false, false, false, app_mode);
    assert!(matches!(r, InputResult::Consumed));
    assert!(matches!(h.session, InputSessionState::Unlocking));
    // Successful unlock: handled internally, returns Consumed.
    let r = h.process_key_event("g", false, false, false, false, BindingMode::EMPTY);
    assert!(matches!(r, InputResult::Consumed));
    assert!(!h.is_locked());
}

#[test]
fn locked_leader_then_wrong_key_returns_to_locked() {
    let mut h = sticky_handler();
    h.toggle_lock();
    h.process_key_event("w", true, false, false, false, BindingMode::LOCKED);
    assert!(matches!(
        h.process_key_event("z", false, false, false, false, BindingMode::EMPTY),
        InputResult::PassThrough
    ));
    assert!(h.is_locked());
    assert!(matches!(h.session, InputSessionState::Locked));
}

#[test]
fn locked_alt_release_cancels_unlock() {
    let mut h = sticky_handler();
    h.leader_key = LeaderKey::parse("alt");
    h.toggle_lock();
    h.process_key_event("Alt", false, false, true, false, BindingMode::LOCKED);
    h.process_key_release("Alt");
    assert!(matches!(h.session, InputSessionState::Locked));
}

#[test]
fn direct_binding_enters_mode() {
    let mut h = prefix_handler();
    assert!(matches!(
        h.process_key("r", true, false, false, false),
        InputResult::Consumed
    ));
    assert_eq!(h.current_mode_name(), Some("resize"));
}

#[test]
fn direct_binding_toggle_lock() {
    let mut h = prefix_handler();
    assert!(matches!(
        h.process_key("g", true, false, false, false),
        InputResult::Action(Action::ToggleLock)
    ));
}

#[test]
fn direct_binding_fires_action() {
    let mut h = prefix_handler();
    assert!(matches!(
        h.process_key("h", false, false, true, false),
        InputResult::Action(Action::FocusLeft)
    ));
}

#[test]
fn locked_direct_toggle_lock() {
    let mut h = prefix_handler();
    h.toggle_lock();
    let r = h.process_key_event("g", true, false, false, false, BindingMode::LOCKED);
    assert!(matches!(r, InputResult::Action(Action::ToggleLock)));
    // State is still Locked — the caller is responsible for calling toggle_lock().
    h.toggle_lock();
    assert!(!h.is_locked());
}

// ── Unified pipeline tests ──

#[test]
fn unified_normal_passthrough() {
    let mut h = prefix_handler();
    let r = h.process_key_event("a", false, false, false, false, BindingMode::EMPTY);
    assert!(matches!(r, InputResult::PassThrough));
}

#[test]
fn unified_leader_then_action() {
    let mut h = prefix_handler();
    let r = h.process_key_event("w", true, false, false, false, BindingMode::EMPTY);
    assert!(matches!(r, InputResult::Consumed));
    assert!(h.is_awaiting_action());
    let r = h.process_key_event("n", false, false, false, false, BindingMode::EMPTY);
    assert!(matches!(r, InputResult::Action(Action::NewColumnRight)));
}

#[test]
fn unified_leader_escape_exits() {
    let mut h = prefix_handler();
    h.process_key_event("w", true, false, false, false, BindingMode::EMPTY);
    let r = h.process_key_event("escape", false, false, false, false, BindingMode::EMPTY);
    assert!(matches!(r, InputResult::Consumed));
    assert!(!h.is_awaiting_action());
}

#[test]
fn unified_enter_mode_pushes_key_table() {
    let mut h = prefix_handler();
    h.process_key_event("w", true, false, false, false, BindingMode::EMPTY);
    h.process_key_event("r", false, false, false, false, BindingMode::EMPTY);
    assert!(h.has_active_table());
    assert_eq!(h.active_table_name(), Some("resize"));
}

#[test]
fn unified_enter_mode_does_not_duplicate_same_key_table() {
    let mut h = prefix_handler();
    h.process_key_event("w", true, false, false, false, BindingMode::EMPTY);
    h.process_key_event("m", false, false, false, false, BindingMode::EMPTY);
    assert_eq!(h.active_table_name(), Some("move"));

    h.process_key_event("w", true, false, false, false, BindingMode::EMPTY);
    h.process_key_event("m", false, false, false, false, BindingMode::EMPTY);
    h.process_key_event("w", true, false, false, false, BindingMode::EMPTY);
    h.process_key_event("m", false, false, false, false, BindingMode::EMPTY);
    h.process_key_event("w", true, false, false, false, BindingMode::EMPTY);
    h.process_key_event("m", false, false, false, false, BindingMode::EMPTY);

    assert_eq!(h.active_table_name(), Some("move"));

    let r = h.process_key_event("h", false, false, false, false, BindingMode::EMPTY);
    assert!(matches!(r, InputResult::Action(Action::MovePaneLeft)));

    h.process_key_event("escape", false, false, false, false, BindingMode::EMPTY);
    assert!(!h.has_active_table());
}

#[test]
fn unified_key_table_dispatches_action() {
    let mut h = prefix_handler();
    h.process_key_event("w", true, false, false, false, BindingMode::EMPTY);
    h.process_key_event("r", false, false, false, false, BindingMode::EMPTY);
    let r = h.process_key_event("h", false, false, false, false, BindingMode::EMPTY);
    assert!(matches!(
        r,
        InputResult::Action(Action::ColumnWidthDecrease)
    ));
    assert!(h.has_active_table());
}

#[test]
fn unified_palette_escape_deactivates_key_table_before_overlay_binding() {
    let mut h = prefix_handler();
    h.process_key_event("w", true, false, false, false, BindingMode::EMPTY);
    h.process_key_event("r", false, false, false, false, BindingMode::EMPTY);
    assert_eq!(h.active_table_name(), Some("resize"));

    let r = h.process_key_event("escape", false, false, false, false, BindingMode::PALETTE);

    assert!(matches!(r, InputResult::Consumed));
    assert!(!h.has_active_table());
}

#[test]
fn unified_search_escape_deactivates_key_table_before_overlay_binding() {
    let mut h = prefix_handler();
    h.process_key_event("w", true, false, false, false, BindingMode::EMPTY);
    h.process_key_event("r", false, false, false, false, BindingMode::EMPTY);
    assert_eq!(h.active_table_name(), Some("resize"));

    let r = h.process_key_event("escape", false, false, false, false, BindingMode::SEARCH);

    assert!(matches!(r, InputResult::Consumed));
    assert!(!h.has_active_table());
}

#[test]
fn unified_escape_pops_key_table() {
    let mut h = prefix_handler();
    h.process_key_event("w", true, false, false, false, BindingMode::EMPTY);
    h.process_key_event("r", false, false, false, false, BindingMode::EMPTY);
    assert!(h.has_active_table());
    h.process_key_event("escape", false, false, false, false, BindingMode::EMPTY);
    assert!(!h.has_active_table());
}

#[test]
fn unified_overview_binding() {
    let mut h = prefix_handler();
    let r = h.process_key_event("h", false, false, false, false, BindingMode::OVERVIEW);
    assert!(matches!(r, InputResult::Action(Action::FocusLeft)));
}

#[test]
fn unified_overview_escape() {
    let mut h = prefix_handler();
    let r = h.process_key_event("escape", false, false, false, false, BindingMode::OVERVIEW);
    assert!(matches!(r, InputResult::Action(Action::ExitOverview)));
}

#[test]
fn unified_overview_leader_toggle() {
    let mut h = prefix_handler();
    h.process_key_event("w", true, false, false, false, BindingMode::OVERVIEW);
    let r = h.process_key_event("o", false, false, false, false, BindingMode::OVERVIEW);
    assert!(matches!(r, InputResult::Action(Action::ToggleOverview)));
}

#[test]
fn unified_search_control_keys() {
    let mut h = prefix_handler();
    let r = h.process_key_event("escape", false, false, false, false, BindingMode::SEARCH);
    assert!(matches!(r, InputResult::Action(Action::CloseSearch)));

    let r = h.process_key_event("enter", false, false, false, false, BindingMode::SEARCH);
    assert!(matches!(r, InputResult::Action(Action::SearchNextMatch)));
}

#[test]
fn unified_search_text_input_fallback() {
    let mut h = prefix_handler();
    let r = h.process_key_event("a", false, false, false, false, BindingMode::SEARCH);
    assert!(matches!(r, InputResult::Action(Action::TextInput)));
}

#[test]
fn unified_palette_navigation() {
    let mut h = prefix_handler();
    let r = h.process_key_event("up", false, false, false, false, BindingMode::PALETTE);
    assert!(matches!(r, InputResult::Action(Action::PaletteUp)));

    let r = h.process_key_event("down", false, false, false, false, BindingMode::PALETTE);
    assert!(matches!(r, InputResult::Action(Action::PaletteDown)));

    let r = h.process_key_event("enter", false, false, false, false, BindingMode::PALETTE);
    assert!(matches!(r, InputResult::Action(Action::PaletteConfirm)));
}

#[test]
fn unified_palette_text_input_fallback() {
    let mut h = prefix_handler();
    let r = h.process_key_event("a", false, false, false, false, BindingMode::PALETTE);
    assert!(matches!(r, InputResult::Action(Action::TextInput)));
}

#[test]
fn unified_sticky_mode_repeatable_stays() {
    let mut h = sticky_handler();
    h.process_key_event("w", true, false, false, false, BindingMode::EMPTY);
    let r = h.process_key_event("h", false, false, false, false, BindingMode::EMPTY);
    assert!(matches!(r, InputResult::Action(Action::FocusLeft)));
    assert!(h.is_awaiting_action());
}

#[test]
fn unified_sticky_mode_oneshot_exits() {
    let mut h = sticky_handler();
    h.process_key_event("w", true, false, false, false, BindingMode::EMPTY);
    let r = h.process_key_event("n", false, false, false, false, BindingMode::EMPTY);
    assert!(matches!(r, InputResult::Action(Action::NewColumnRight)));
    assert!(!h.is_awaiting_action());
}

#[test]
fn unified_locked_passthrough() {
    let mut h = prefix_handler();
    h.toggle_lock();
    let r = h.process_key_event("a", false, false, false, false, BindingMode::LOCKED);
    assert!(matches!(r, InputResult::PassThrough));
}

#[test]
fn unified_locked_leader_then_unlock() {
    let mut h = prefix_handler();
    h.toggle_lock();
    h.process_key_event("w", true, false, false, false, BindingMode::LOCKED);
    let r = h.process_key_event("g", false, false, false, false, BindingMode::EMPTY);
    assert!(matches!(r, InputResult::Consumed));
    assert!(!h.is_locked());
}

#[test]
fn unified_locked_direct_toggle_lock() {
    let mut h = prefix_handler();
    h.toggle_lock();
    let r = h.process_key_event("g", true, false, false, false, BindingMode::LOCKED);
    assert!(matches!(r, InputResult::Action(Action::ToggleLock)));
}

// ── InputSessionState transition unit tests ──

#[test]
fn session_on_leader_press_from_idle() {
    let mut s = InputSessionState::Idle;
    s.on_leader_press(InputMode::Prefix);
    assert!(s.is_in_leader());
}

#[test]
fn session_on_leader_press_from_locked() {
    let mut s = InputSessionState::Locked;
    s.on_leader_press(InputMode::Prefix);
    assert!(matches!(s, InputSessionState::Unlocking));
}

#[test]
fn session_on_leader_press_noop_in_leader() {
    let mut s = InputSessionState::Leader(LeaderSession::new(InputMode::Prefix));
    s.on_leader_press(InputMode::Sticky);
    // Should stay in leader with original mode
    match &s {
        InputSessionState::Leader(session) => assert_eq!(session.mode, InputMode::Prefix),
        _ => panic!("expected Leader"),
    }
}

#[test]
fn session_on_leader_release_exits_leader() {
    let mut s = InputSessionState::Leader(LeaderSession::new(InputMode::Prefix));
    s.on_leader_release();
    assert!(matches!(s, InputSessionState::Idle));
}

#[test]
fn session_on_leader_release_unlocking_returns_locked() {
    let mut s = InputSessionState::Unlocking;
    s.on_leader_release();
    assert!(matches!(s, InputSessionState::Locked));
}

#[test]
fn session_on_leader_release_noop_from_idle() {
    let mut s = InputSessionState::Idle;
    s.on_leader_release();
    assert!(matches!(s, InputSessionState::Idle));
}

#[test]
fn session_on_escape_exits_leader() {
    let mut s = InputSessionState::Leader(LeaderSession::new(InputMode::Prefix));
    assert!(s.on_escape());
    assert!(matches!(s, InputSessionState::Idle));
}

#[test]
fn session_on_escape_noop_when_idle() {
    let mut s = InputSessionState::Idle;
    assert!(!s.on_escape());
}

#[test]
fn session_on_escape_noop_when_locked() {
    let mut s = InputSessionState::Locked;
    assert!(!s.on_escape());
}

#[test]
fn session_on_action_dispatched_prefix_exits() {
    let mut s = InputSessionState::Leader(LeaderSession::new(InputMode::Prefix));
    s.on_action_dispatched(&Action::FocusLeft);
    assert!(matches!(s, InputSessionState::Idle));
}

#[test]
fn session_on_action_dispatched_sticky_repeatable_stays() {
    let mut s = InputSessionState::Leader(LeaderSession::new(InputMode::Sticky));
    s.on_action_dispatched(&Action::FocusLeft);
    assert!(s.is_in_leader());
}

#[test]
fn session_on_action_dispatched_sticky_non_repeatable_exits() {
    let mut s = InputSessionState::Leader(LeaderSession::new(InputMode::Sticky));
    s.on_action_dispatched(&Action::NewColumnRight);
    assert!(matches!(s, InputSessionState::Idle));
}

#[test]
fn session_on_action_dispatched_noop_when_idle() {
    let mut s = InputSessionState::Idle;
    s.on_action_dispatched(&Action::FocusLeft);
    assert!(matches!(s, InputSessionState::Idle));
}

#[test]
fn session_on_unmatched_leader_prefix_exits() {
    let mut s = InputSessionState::Leader(LeaderSession::new(InputMode::Prefix));
    s.on_unmatched_leader();
    assert!(matches!(s, InputSessionState::Idle));
}

#[test]
fn session_on_unmatched_leader_sticky_stays() {
    let mut s = InputSessionState::Leader(LeaderSession::new(InputMode::Sticky));
    s.on_unmatched_leader();
    assert!(s.is_in_leader());
}

#[test]
fn session_on_timeout_expired_exits() {
    let mut s = InputSessionState::Leader(LeaderSession {
        mode: InputMode::Prefix,
        entered_at: Instant::now() - Duration::from_secs(2),
    });
    s.on_timeout(Duration::from_secs(1));
    assert!(matches!(s, InputSessionState::Idle));
}

#[test]
fn session_on_timeout_not_expired_stays() {
    let mut s = InputSessionState::Leader(LeaderSession::new(InputMode::Prefix));
    s.on_timeout(Duration::from_secs(100));
    assert!(s.is_in_leader());
}

#[test]
fn session_on_timeout_sticky_never_expires() {
    let mut s = InputSessionState::Leader(LeaderSession {
        mode: InputMode::Sticky,
        entered_at: Instant::now() - Duration::from_secs(1000),
    });
    s.on_timeout(Duration::from_secs(1));
    assert!(s.is_in_leader());
}

#[test]
fn session_on_toggle_lock_idle_to_locked() {
    let mut s = InputSessionState::Idle;
    s.on_toggle_lock();
    assert!(matches!(s, InputSessionState::Locked));
}

#[test]
fn session_on_toggle_lock_locked_to_idle() {
    let mut s = InputSessionState::Locked;
    s.on_toggle_lock();
    assert!(matches!(s, InputSessionState::Idle));
}

#[test]
fn session_on_toggle_lock_from_unlocking() {
    let mut s = InputSessionState::Unlocking;
    s.on_toggle_lock();
    // Unlocking is_locked() == true, so toggle → Idle
    assert!(matches!(s, InputSessionState::Idle));
}

#[test]
fn session_on_toggle_lock_from_leader() {
    let mut s = InputSessionState::Leader(LeaderSession::new(InputMode::Prefix));
    s.on_toggle_lock();
    // Leader is_locked() == false, so toggle → Locked
    assert!(matches!(s, InputSessionState::Locked));
}

#[test]
fn session_exit_leader_from_leader() {
    let mut s = InputSessionState::Leader(LeaderSession::new(InputMode::Prefix));
    s.exit_leader();
    assert!(matches!(s, InputSessionState::Idle));
}

#[test]
fn session_exit_leader_noop_from_idle() {
    let mut s = InputSessionState::Idle;
    s.exit_leader();
    assert!(matches!(s, InputSessionState::Idle));
}

#[test]
fn session_exit_leader_noop_from_locked() {
    let mut s = InputSessionState::Locked;
    s.exit_leader();
    assert!(matches!(s, InputSessionState::Locked));
}

#[test]
fn session_on_unlock_attempt_success() {
    let mut s = InputSessionState::Unlocking;
    s.on_unlock_attempt(true);
    assert!(matches!(s, InputSessionState::Idle));
}

#[test]
fn session_on_unlock_attempt_failure() {
    let mut s = InputSessionState::Unlocking;
    s.on_unlock_attempt(false);
    assert!(matches!(s, InputSessionState::Locked));
}

// ── leader_deadline tests (proactive timer support) ──

#[test]
fn leader_deadline_returns_some_in_prefix_leader() {
    let mut h = prefix_handler();
    assert!(h.leader_deadline().is_none());
    let before = Instant::now();
    h.process_key("w", true, false, false, false);
    let after = Instant::now();
    let deadline = h.leader_deadline().unwrap();
    let timeout = Duration::from_millis(1000);
    // entered_at is captured between `before` and `after`,
    // so deadline must be in [before + timeout, after + timeout].
    assert!(deadline >= before + timeout);
    assert!(deadline <= after + timeout);
}

#[test]
fn leader_deadline_returns_none_when_idle() {
    let h = prefix_handler();
    assert!(h.leader_deadline().is_none());
}

#[test]
fn leader_deadline_returns_none_in_sticky_mode() {
    let mut h = sticky_handler();
    h.process_key("w", true, false, false, false);
    assert!(h.is_awaiting_action());
    assert!(h.leader_deadline().is_none());
}

#[test]
fn leader_deadline_returns_none_when_locked() {
    let mut h = prefix_handler();
    h.toggle_lock();
    assert!(h.leader_deadline().is_none());
}

#[test]
fn leader_deadline_returns_none_with_active_key_table() {
    let mut h = prefix_handler();
    // Enter key table via normal flow: leader → mode action → exit_leader + activate table.
    // After this, session is Idle (exit_leader was called) and table is active.
    h.process_key("w", true, false, false, false);
    h.process_key("r", false, false, false, false);
    assert!(h.has_active_table());
    assert!(!h.session.is_in_leader());
    assert!(h.leader_deadline().is_none());
}

#[test]
fn leader_deadline_returns_none_with_active_key_table_guard() {
    let mut h = prefix_handler();
    // Inject a state where both leader AND key table are active.
    // This can't happen through the normal API (enter_mode calls exit_leader),
    // but tests the has_active_table() guard in handler.leader_deadline().
    h.activate_key_table("resize");
    h.session = InputSessionState::Leader(LeaderSession {
        mode: InputMode::Prefix,
        entered_at: Instant::now(),
    });
    assert!(h.has_active_table());
    assert!(h.session.is_in_leader());
    // The key table guard should suppress the deadline even though leader is active.
    assert!(h.leader_deadline().is_none());
}

#[test]
fn leader_deadline_cleared_after_action_in_prefix() {
    let mut h = prefix_handler();
    h.process_key("w", true, false, false, false);
    assert!(h.leader_deadline().is_some());
    h.process_key("n", false, false, false, false); // action → exits leader
    assert!(h.leader_deadline().is_none());
}

#[test]
fn leader_deadline_cleared_after_escape() {
    let mut h = prefix_handler();
    h.process_key("w", true, false, false, false);
    assert!(h.leader_deadline().is_some());
    h.process_key("escape", false, false, false, false);
    assert!(h.leader_deadline().is_none());
}

#[test]
fn leader_deadline_cleared_after_unmatched_key_in_prefix() {
    let mut h = prefix_handler();
    h.process_key("w", true, false, false, false);
    assert!(h.leader_deadline().is_some());
    h.process_key("z", false, false, false, false); // unmatched → exits in prefix
    assert!(h.leader_deadline().is_none());
}

#[test]
fn leader_deadline_none_after_sticky_repeatable_dispatch() {
    let mut h = sticky_handler();
    h.process_key("w", true, false, false, false);
    // Dispatch a repeatable action — sticky refreshes the LeaderSession
    let r = h.process_key("h", false, false, false, false);
    assert!(matches!(r, InputResult::Action(Action::FocusLeft)));
    assert!(h.is_awaiting_action());
    // Sticky mode: deadline must still be None after session refresh
    assert!(h.leader_deadline().is_none());
}

#[test]
fn leader_deadline_persists_after_unmatched_key_in_sticky() {
    let mut h = sticky_handler();
    h.process_key("w", true, false, false, false);
    h.process_key("z", false, false, false, false); // unmatched → stays in sticky
    assert!(h.is_awaiting_action());
    // Sticky never has a deadline
    assert!(h.leader_deadline().is_none());
}

#[test]
fn leader_deadline_reflects_entered_at_timestamp() {
    let mut h = prefix_handler();
    // Manually set a leader session entered 500ms ago
    let entered_at = Instant::now() - Duration::from_millis(500);
    h.session = InputSessionState::Leader(LeaderSession {
        mode: InputMode::Prefix,
        entered_at,
    });
    let deadline = h.leader_deadline().unwrap();
    // Deadline must be exactly entered_at + timeout (1000ms)
    assert_eq!(deadline, entered_at + Duration::from_millis(1000));
}

#[test]
fn leader_deadline_in_the_past_when_already_expired() {
    let mut h = prefix_handler();
    h.session = InputSessionState::Leader(LeaderSession {
        mode: InputMode::Prefix,
        entered_at: Instant::now() - Duration::from_secs(5),
    });
    let deadline = h.leader_deadline().unwrap();
    // Deadline is in the past — the event loop should fire immediately
    assert!(deadline < Instant::now());
}

#[test]
fn check_timeout_clears_expired_leader_proactively() {
    let mut h = prefix_handler();
    h.session = InputSessionState::Leader(LeaderSession {
        mode: InputMode::Prefix,
        entered_at: Instant::now() - Duration::from_secs(2),
    });
    assert!(h.is_awaiting_action());
    // Simulate event loop calling check_timeout on wake
    h.poll_timeout();
    assert!(!h.is_awaiting_action());
    assert!(h.leader_deadline().is_none());
}

#[test]
fn check_timeout_does_not_clear_fresh_leader() {
    let mut h = prefix_handler();
    h.process_key("w", true, false, false, false);
    assert!(h.is_awaiting_action());
    h.poll_timeout();
    // Not expired yet — should still be in leader
    assert!(h.is_awaiting_action());
    assert!(h.leader_deadline().is_some());
}

#[test]
fn check_timeout_skips_key_table() {
    let mut h = prefix_handler();
    h.activate_key_table("resize");
    assert!(h.has_active_table());
    // Even if we fake an expired leader session, check_timeout skips it
    // because a key table is active.
    h.session = InputSessionState::Leader(LeaderSession {
        mode: InputMode::Prefix,
        entered_at: Instant::now() - Duration::from_secs(100),
    });
    h.poll_timeout();
    // Leader session untouched because key table takes priority
    assert!(h.session.is_in_leader());
}

#[test]
fn check_timeout_does_not_affect_sticky() {
    let mut h = sticky_handler();
    h.session = InputSessionState::Leader(LeaderSession {
        mode: InputMode::Sticky,
        entered_at: Instant::now() - Duration::from_secs(999),
    });
    h.poll_timeout();
    assert!(h.is_awaiting_action());
}

#[test]
fn process_key_event_triggers_timeout_before_processing() {
    let mut h = prefix_handler();
    // Simulate: leader was pressed long ago, then user presses 'a'
    h.session = InputSessionState::Leader(LeaderSession {
        mode: InputMode::Prefix,
        entered_at: Instant::now() - Duration::from_secs(2),
    });
    // 'a' should be processed as PassThrough because check_timeout
    // fires at the start of process_key_event and clears the leader.
    let r = h.process_key("a", false, false, false, false);
    assert!(matches!(r, InputResult::PassThrough));
    assert!(!h.is_awaiting_action());
}

#[test]
fn leader_deadline_none_during_unlocking() {
    let mut h = prefix_handler();
    h.toggle_lock();
    h.process_key_event("w", true, false, false, false, BindingMode::LOCKED);
    assert!(matches!(h.session, InputSessionState::Unlocking));
    assert!(h.leader_deadline().is_none());
}

// ── InputSessionState::leader_deadline unit tests ──

#[test]
fn session_leader_deadline_prefix_returns_correct_instant() {
    let entered = Instant::now() - Duration::from_millis(200);
    let s = InputSessionState::Leader(LeaderSession {
        mode: InputMode::Prefix,
        entered_at: entered,
    });
    let limit = Duration::from_millis(1000);
    let deadline = s.leader_deadline(limit).unwrap();
    assert_eq!(deadline, entered + limit);
}

#[test]
fn session_leader_deadline_sticky_returns_none() {
    let s = InputSessionState::Leader(LeaderSession {
        mode: InputMode::Sticky,
        entered_at: Instant::now(),
    });
    assert!(s.leader_deadline(Duration::from_secs(1)).is_none());
}

#[test]
fn session_leader_deadline_idle_returns_none() {
    let s = InputSessionState::Idle;
    assert!(s.leader_deadline(Duration::from_secs(1)).is_none());
}

#[test]
fn session_leader_deadline_locked_returns_none() {
    let s = InputSessionState::Locked;
    assert!(s.leader_deadline(Duration::from_secs(1)).is_none());
}

#[test]
fn session_leader_deadline_unlocking_returns_none() {
    let s = InputSessionState::Unlocking;
    assert!(s.leader_deadline(Duration::from_secs(1)).is_none());
}

// ── KeyTableState unit tests ──

#[test]
fn key_table_push_overflow() {
    let mut kt = KeyTableState::default();
    assert!(kt.push("a", 2));
    assert!(kt.push("b", 2));
    assert!(!kt.push("c", 2));
    assert_eq!(kt.current(), Some("b"));
}

#[test]
fn key_table_push_dedup() {
    let mut kt = KeyTableState::default();
    assert!(kt.push("a", 4));
    assert!(kt.push("a", 4)); // duplicate, returns true but doesn't push
    kt.pop();
    assert!(!kt.is_active()); // only one was pushed
}
