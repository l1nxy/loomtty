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
    h.check_timeout();
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
