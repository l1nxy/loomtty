use super::*;
use legacy::{
    ctrl_mapping_for_legacy_key, encode_legacy_ascii_text_key, legacy_csi_special,
    legacy_csi_tilde, legacy_f1_f4, legacy_modifier_value,
};
use winit::event::ElementState;
use winit::keyboard::{Key, KeyCode, KeyLocation, NamedKey, PhysicalKey};

// ── Test event construction ─────────────────────────────────────────

/// Build a test KeyEvent for encoding tests.
///
/// winit's `KeyEvent` has a private `platform_specific` field we can't set
/// via the public API. We use `addr_of_mut!` to write only the public fields
/// into a `MaybeUninit` without ever calling `assume_init()` on the full struct.
/// The result is wrapped in `ManuallyDrop` so the uninitialized
/// `platform_specific` field is never dropped.
fn make_key_event(
    logical_key: Key,
    physical_key: PhysicalKey,
    text: Option<winit::keyboard::SmolStr>,
    location: KeyLocation,
    state: ElementState,
    repeat: bool,
) -> std::mem::ManuallyDrop<winit::event::KeyEvent> {
    use std::mem::{ManuallyDrop, MaybeUninit};
    use std::ptr::addr_of_mut;
    unsafe {
        let mut uninit = MaybeUninit::<winit::event::KeyEvent>::zeroed();
        let ptr = uninit.as_mut_ptr();
        addr_of_mut!((*ptr).physical_key).write(physical_key);
        addr_of_mut!((*ptr).logical_key).write(logical_key);
        addr_of_mut!((*ptr).text).write(text);
        addr_of_mut!((*ptr).location).write(location);
        addr_of_mut!((*ptr).state).write(state);
        addr_of_mut!((*ptr).repeat).write(repeat);
        // platform_specific is left zeroed — never read by our encoding fns.
        // ManuallyDrop prevents drop of the partially-initialized struct.
        ManuallyDrop::new(uninit.assume_init())
    }
}

type TestEvent = std::mem::ManuallyDrop<winit::event::KeyEvent>;

fn named_key_event(key: NamedKey, state: ElementState, repeat: bool) -> TestEvent {
    named_key_event_loc(key, state, repeat, KeyLocation::Standard)
}

fn named_key_event_loc(
    key: NamedKey,
    state: ElementState,
    repeat: bool,
    location: KeyLocation,
) -> TestEvent {
    make_key_event(
        Key::Named(key),
        PhysicalKey::Unidentified(winit::keyboard::NativeKeyCode::Unidentified),
        None,
        location,
        state,
        repeat,
    )
}

fn char_key_event(ch: char, physical: KeyCode, state: ElementState) -> TestEvent {
    let s = winit::keyboard::SmolStr::new(ch.to_string());
    make_key_event(
        Key::Character(s.clone()),
        PhysicalKey::Code(physical),
        Some(s),
        KeyLocation::Standard,
        state,
        false,
    )
}

fn numpad_char_event(ch: &str, physical: KeyCode) -> TestEvent {
    let s = winit::keyboard::SmolStr::new(ch);
    make_key_event(
        Key::Character(s.clone()),
        PhysicalKey::Code(physical),
        Some(s),
        KeyLocation::Numpad,
        ElementState::Pressed,
        false,
    )
}

fn numpad_named_event(key: NamedKey, physical: KeyCode) -> TestEvent {
    make_key_event(
        Key::Named(key),
        PhysicalKey::Code(physical),
        None,
        KeyLocation::Numpad,
        ElementState::Pressed,
        false,
    )
}

// ── Shared utility tests ────────────────────────────────────────────

#[test]
fn physical_key_to_base_char_includes_symbol_keys() {
    assert_eq!(
        physical_key_to_base_char(PhysicalKey::Code(KeyCode::Minus)),
        Some('-')
    );
    assert_eq!(
        physical_key_to_base_char(PhysicalKey::Code(KeyCode::Equal)),
        Some('=')
    );
    assert_eq!(
        physical_key_to_base_char(PhysicalKey::Code(KeyCode::BracketLeft)),
        Some('[')
    );
    assert_eq!(
        physical_key_to_base_char(PhysicalKey::Code(KeyCode::BracketRight)),
        Some(']')
    );
    assert_eq!(
        physical_key_to_base_char(PhysicalKey::Code(KeyCode::Semicolon)),
        Some(';')
    );
    assert_eq!(
        physical_key_to_base_char(PhysicalKey::Code(KeyCode::Quote)),
        Some('\'')
    );
    assert_eq!(
        physical_key_to_base_char(PhysicalKey::Code(KeyCode::Backquote)),
        Some('`')
    );
    assert_eq!(
        physical_key_to_base_char(PhysicalKey::Code(KeyCode::Comma)),
        Some(',')
    );
    assert_eq!(
        physical_key_to_base_char(PhysicalKey::Code(KeyCode::Period)),
        Some('.')
    );
    assert_eq!(
        physical_key_to_base_char(PhysicalKey::Code(KeyCode::Slash)),
        Some('/')
    );
}

// ── Legacy encoding tests ───────────────────────────────────────────

#[test]
fn legacy_ascii_text_key_preserves_shifted_backslash_text() {
    assert_eq!(
        encode_legacy_ascii_text_key('\\', "|", false, true, false),
        Some(vec![b'|'])
    );
    assert_eq!(
        encode_legacy_ascii_text_key('\\', "|", false, true, true),
        Some(b"\x1b|".to_vec())
    );
}

#[test]
fn legacy_ctrl_mapping_matches_terminal_control_bytes() {
    assert_eq!(ctrl_mapping_for_legacy_key('a'), Some(0x01));
    assert_eq!(ctrl_mapping_for_legacy_key('3'), Some(0x1b));
    assert_eq!(ctrl_mapping_for_legacy_key('\\'), Some(0x1c));
    assert_eq!(ctrl_mapping_for_legacy_key('8'), Some(0x7f));
}

#[test]
fn legacy_c0_keys_follow_terminal_meta_rules() {
    assert_eq!(
        encode_legacy_c0_key(&NamedKey::Tab, false, true, true, false),
        Some(b"\x1b\x1b[Z".to_vec())
    );
    assert_eq!(
        encode_legacy_c0_key(&NamedKey::Backspace, true, false, true, false),
        Some(vec![0x1b, 0x08])
    );
    assert_eq!(
        encode_legacy_c0_key(&NamedKey::Escape, false, false, false, true),
        None
    );
    assert_eq!(
        encode_legacy_c0_key(&NamedKey::Space, true, false, false, true),
        None
    );
    assert_eq!(
        encode_legacy_c0_key(&NamedKey::Space, true, false, false, false),
        Some(vec![0x00])
    );
}

#[test]
fn kitty_compat_mode_keeps_shifted_text_as_text() {
    assert_eq!(
        encode_kitty_legacy_text_bytes(Some("|"), ElementState::Pressed, false, false, false),
        Some(vec![b'|'])
    );
    assert_eq!(
        encode_kitty_legacy_text_bytes(Some("|"), ElementState::Released, false, false, false),
        Some(vec![])
    );
    assert_eq!(
        encode_kitty_legacy_text_bytes(Some("|"), ElementState::Pressed, false, true, false),
        None
    );
}

#[test]
fn legacy_modifier_value_computation() {
    assert_eq!(legacy_modifier_value(false, false, false), 0);
    assert_eq!(legacy_modifier_value(false, true, false), 2);
    assert_eq!(legacy_modifier_value(false, false, true), 3);
    assert_eq!(legacy_modifier_value(true, false, false), 5);
    assert_eq!(legacy_modifier_value(true, true, false), 6);
    assert_eq!(legacy_modifier_value(true, true, true), 8);
}

#[test]
fn legacy_arrow_keys_with_modifiers() {
    assert_eq!(legacy_csi_special('A', 0, false), b"\x1b[A");
    assert_eq!(legacy_csi_special('A', 2, false), b"\x1b[1;2A");
    assert_eq!(legacy_csi_special('C', 5, false), b"\x1b[1;5C");
}

#[test]
fn legacy_arrow_keys_app_cursor_mode() {
    // DECCKM active, no modifiers → SS3 prefix
    assert_eq!(legacy_csi_special('A', 0, true), b"\x1bOA");
    assert_eq!(legacy_csi_special('B', 0, true), b"\x1bOB");
    assert_eq!(legacy_csi_special('C', 0, true), b"\x1bOC");
    assert_eq!(legacy_csi_special('D', 0, true), b"\x1bOD");
    assert_eq!(legacy_csi_special('H', 0, true), b"\x1bOH");
    assert_eq!(legacy_csi_special('F', 0, true), b"\x1bOF");
    // DECCKM active + modifiers → CSI form (modifiers override)
    assert_eq!(legacy_csi_special('A', 2, true), b"\x1b[1;2A");
}

#[test]
fn legacy_tilde_keys_with_modifiers() {
    assert_eq!(legacy_csi_tilde(3, 0), b"\x1b[3~");
    assert_eq!(legacy_csi_tilde(3, 2), b"\x1b[3;2~");
    assert_eq!(legacy_csi_tilde(5, 5), b"\x1b[5;5~");
}

#[test]
fn legacy_f1_f4_with_modifiers() {
    assert_eq!(legacy_f1_f4('P', 0), b"\x1bOP");
    assert_eq!(legacy_f1_f4('P', 2), b"\x1b[1;2P");
}

#[test]
fn legacy_shift_up_via_pty_bytes() {
    let event = named_key_event(NamedKey::ArrowUp, ElementState::Pressed, false);
    assert_eq!(
        key_event_to_pty_bytes(&event, false, true, false, false, false),
        b"\x1b[1;2A"
    );
}

#[test]
fn legacy_ctrl_delete_via_pty_bytes() {
    let event = named_key_event(NamedKey::Delete, ElementState::Pressed, false);
    assert_eq!(
        key_event_to_pty_bytes(&event, true, false, false, false, false),
        b"\x1b[3;5~"
    );
}

#[test]
fn legacy_arrow_up_app_cursor_via_pty_bytes() {
    let event = named_key_event(NamedKey::ArrowUp, ElementState::Pressed, false);
    // DECCKM on, no modifiers → SS3
    assert_eq!(
        key_event_to_pty_bytes(&event, false, false, false, true, false),
        b"\x1bOA"
    );
    // DECCKM on, with shift → CSI (modifiers override app cursor)
    assert_eq!(
        key_event_to_pty_bytes(&event, false, true, false, true, false),
        b"\x1b[1;2A"
    );
}

// ── Application keypad (DECKPAM) tests ─────────────────────────────

#[test]
fn legacy_numpad_enter_app_keypad() {
    let event = numpad_named_event(NamedKey::Enter, KeyCode::NumpadEnter);
    // DECKPAM active → SS3 M
    assert_eq!(
        key_event_to_pty_bytes(&event, false, false, false, false, true),
        b"\x1bOM"
    );
    // DECKPAM inactive → regular CR
    assert_eq!(
        key_event_to_pty_bytes(&event, false, false, false, false, false),
        b"\r"
    );
}

#[test]
fn legacy_numpad_digits_app_keypad() {
    let cases = [
        ("0", KeyCode::Numpad0, b'p'),
        ("1", KeyCode::Numpad1, b'q'),
        ("5", KeyCode::Numpad5, b'u'),
        ("9", KeyCode::Numpad9, b'y'),
    ];
    for (ch, code, ss3_byte) in cases {
        let event = numpad_char_event(ch, code);
        let expected = format!("\x1bO{}", ss3_byte as char).into_bytes();
        assert_eq!(
            key_event_to_pty_bytes(&event, false, false, false, false, true),
            expected,
            "numpad {ch} should produce SS3 {}",
            ss3_byte as char,
        );
    }
}

#[test]
fn legacy_numpad_operators_app_keypad() {
    let cases = [
        ("+", KeyCode::NumpadAdd, b'k'),
        ("-", KeyCode::NumpadSubtract, b'm'),
        ("*", KeyCode::NumpadMultiply, b'j'),
        ("/", KeyCode::NumpadDivide, b'o'),
        (".", KeyCode::NumpadDecimal, b'n'),
    ];
    for (ch, code, ss3_byte) in cases {
        let event = numpad_char_event(ch, code);
        let expected = format!("\x1bO{}", ss3_byte as char).into_bytes();
        assert_eq!(
            key_event_to_pty_bytes(&event, false, false, false, false, true),
            expected,
            "numpad {ch} should produce SS3 {}",
            ss3_byte as char,
        );
    }
}

#[test]
fn legacy_numpad_normal_mode_sends_text() {
    // Without DECKPAM, numpad sends normal characters
    let event = numpad_char_event("5", KeyCode::Numpad5);
    assert_eq!(
        key_event_to_pty_bytes(&event, false, false, false, false, false),
        b"5"
    );
}

#[test]
fn legacy_numpad_app_keypad_with_modifier_falls_through() {
    // Ctrl+Numpad5: modifier must not be silently dropped — fall through to normal path
    let event = numpad_char_event("5", KeyCode::Numpad5);
    // ctrl=true → should NOT produce SS3, should produce ctrl mapping for '5' (0x1d)
    let ctrl_result = key_event_to_pty_bytes(&event, true, false, false, false, true);
    assert_ne!(
        ctrl_result, b"\x1bOu",
        "Ctrl+Numpad5 must not produce bare SS3"
    );

    // Shift+Numpad5: should fall through, not produce SS3
    let shift_result = key_event_to_pty_bytes(&event, false, true, false, false, true);
    assert_ne!(
        shift_result, b"\x1bOu",
        "Shift+Numpad5 must not produce bare SS3"
    );

    // Alt+Numpad5: Alt is handled via meta prefix, SS3 with ESC prefix is acceptable
    let alt_result = key_event_to_pty_bytes(&event, false, false, true, false, true);
    assert_eq!(
        alt_result, b"\x1b\x1bOu",
        "Alt+Numpad5 should be ESC-prefixed SS3"
    );
}

// ── Kitty protocol level 1 (DISAMBIGUATE) ───────────────────────────

const LEVEL1: u16 = ciri_protocol::message::MODE_KITTY_KEYBOARD;

#[test]
fn kitty_level1_escape_uses_csi_u() {
    let e = named_key_event(NamedKey::Escape, ElementState::Pressed, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, false, false, LEVEL1),
        b"\x1b[27u"
    );
}

#[test]
fn kitty_level1_space_uses_csi_u() {
    let e = named_key_event(NamedKey::Space, ElementState::Pressed, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, false, false, LEVEL1),
        b"\x1b[32u"
    );
}

#[test]
fn kitty_level1_ctrl_space() {
    let e = named_key_event(NamedKey::Space, ElementState::Pressed, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, true, false, false, false, false, false, LEVEL1),
        b"\x1b[32;5u"
    );
}

#[test]
fn kitty_level1_enter_stays_legacy() {
    let e = named_key_event(NamedKey::Enter, ElementState::Pressed, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, false, false, LEVEL1),
        b"\r"
    );
}

#[test]
fn kitty_level1_ctrl_enter_stays_legacy() {
    let e = named_key_event(NamedKey::Enter, ElementState::Pressed, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, true, false, false, false, false, false, LEVEL1),
        b"\r"
    );
}

#[test]
fn kitty_level1_ctrl_tab_stays_legacy() {
    let e = named_key_event(NamedKey::Tab, ElementState::Pressed, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, true, false, false, false, false, false, LEVEL1),
        b"\t"
    );
}

#[test]
fn kitty_level1_ctrl_backspace_stays_legacy() {
    let e = named_key_event(NamedKey::Backspace, ElementState::Pressed, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, true, false, false, false, false, false, LEVEL1),
        b"\x08"
    );
}

#[test]
fn kitty_level1_arrow_up_with_shift() {
    let e = named_key_event(NamedKey::ArrowUp, ElementState::Pressed, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, true, false, false, false, false, LEVEL1),
        b"\x1b[1;2A"
    );
}

#[test]
fn kitty_level1_plain_char_stays_legacy() {
    let e = char_key_event('a', KeyCode::KeyA, ElementState::Pressed);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, false, false, LEVEL1),
        b"a"
    );
}

#[test]
fn kitty_level1_f1_no_mods() {
    let e = named_key_event(NamedKey::F1, ElementState::Pressed, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, false, false, LEVEL1),
        b"\x1b[P"
    );
}

#[test]
fn kitty_level1_f1_with_shift() {
    let e = named_key_event(NamedKey::F1, ElementState::Pressed, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, true, false, false, false, false, LEVEL1),
        b"\x1b[1;2P"
    );
}

#[test]
fn kitty_level1_f3_uses_tilde() {
    let e = named_key_event(NamedKey::F3, ElementState::Pressed, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, false, false, LEVEL1),
        b"\x1b[13~"
    );
}

#[test]
fn kitty_level1_f4_with_ctrl() {
    let e = named_key_event(NamedKey::F4, ElementState::Pressed, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, true, false, false, false, false, false, LEVEL1),
        b"\x1b[1;5S"
    );
}

#[test]
fn kitty_level1_f5_with_shift() {
    let e = named_key_event(NamedKey::F5, ElementState::Pressed, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, true, false, false, false, false, LEVEL1),
        b"\x1b[15;2~"
    );
}

// ── Kitty protocol level 2 (REPORT_EVENTS) ─────────────────────────

const LEVEL2: u16 = LEVEL1 | ciri_protocol::message::MODE_KITTY_REPORT_EVENTS;

#[test]
fn kitty_level2_plain_char_press_stays_legacy() {
    let e = char_key_event('a', KeyCode::KeyA, ElementState::Pressed);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, false, false, LEVEL2),
        b"a"
    );
}

#[test]
fn kitty_level2_escape_release() {
    let e = named_key_event(NamedKey::Escape, ElementState::Released, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, false, false, LEVEL2),
        b"\x1b[27;1:3u"
    );
}

#[test]
fn kitty_level2_escape_repeat() {
    let e = named_key_event(NamedKey::Escape, ElementState::Pressed, true);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, false, false, LEVEL2),
        b"\x1b[27;1:2u"
    );
}

#[test]
fn kitty_level2_enter_release_suppressed() {
    let e = named_key_event(NamedKey::Enter, ElementState::Released, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, false, false, LEVEL2),
        b""
    );
}

#[test]
fn kitty_level2_ctrl_enter_release_suppressed() {
    let e = named_key_event(NamedKey::Enter, ElementState::Released, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, true, false, false, false, false, false, LEVEL2),
        b""
    );
}

#[test]
fn kitty_level2_tab_release_suppressed() {
    let e = named_key_event(NamedKey::Tab, ElementState::Released, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, false, false, LEVEL2),
        b""
    );
}

#[test]
fn kitty_level2_backspace_release_suppressed() {
    let e = named_key_event(NamedKey::Backspace, ElementState::Released, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, true, false, false, false, false, false, LEVEL2),
        b""
    );
}

// ── Kitty protocol level 4 (REPORT_ALL) ─────────────────────────────

const LEVEL4: u16 = LEVEL1
    | ciri_protocol::message::MODE_KITTY_REPORT_EVENTS
    | ciri_protocol::message::MODE_KITTY_REPORT_ALTERNATES
    | ciri_protocol::message::MODE_KITTY_REPORT_ALL;

#[test]
fn kitty_level4_enter_csi_u() {
    let e = named_key_event(NamedKey::Enter, ElementState::Pressed, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, false, false, LEVEL4),
        b"\x1b[13u"
    );
}

#[test]
fn kitty_level4_enter_release_reported() {
    let e = named_key_event(NamedKey::Enter, ElementState::Released, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, false, false, LEVEL4),
        b"\x1b[13;1:3u"
    );
}

#[test]
fn kitty_level4_plain_char_csi_u() {
    let e = char_key_event('a', KeyCode::KeyA, ElementState::Pressed);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, false, false, LEVEL4),
        b"\x1b[97u"
    );
}

#[test]
fn kitty_level4_left_shift() {
    let e = named_key_event_loc(
        NamedKey::Shift,
        ElementState::Pressed,
        false,
        KeyLocation::Left,
    );
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, true, false, false, false, false, LEVEL4),
        b"\x1b[57441;2u"
    );
}

#[test]
fn kitty_level4_right_shift() {
    let e = named_key_event_loc(
        NamedKey::Shift,
        ElementState::Pressed,
        false,
        KeyLocation::Right,
    );
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, true, false, false, false, false, LEVEL4),
        b"\x1b[57447;2u"
    );
}

#[test]
fn kitty_level4_right_ctrl() {
    let e = named_key_event_loc(
        NamedKey::Control,
        ElementState::Pressed,
        false,
        KeyLocation::Right,
    );
    assert_eq!(
        key_event_to_kitty_bytes(&e, true, false, false, false, false, false, LEVEL4),
        b"\x1b[57448;5u"
    );
}

// ── Numpad ──────────────────────────────────────────────────────────

#[test]
fn kitty_numpad_digit() {
    let e = numpad_char_event("5", KeyCode::Numpad5);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, false, false, LEVEL1),
        b"\x1b[57404u"
    );
}

#[test]
fn kitty_numpad_enter() {
    let e = numpad_named_event(NamedKey::Enter, KeyCode::NumpadEnter);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, false, false, LEVEL1),
        b"\x1b[57414u"
    );
}

#[test]
fn kitty_numpad_with_modifier() {
    let e = numpad_char_event("5", KeyCode::Numpad5);
    assert_eq!(
        key_event_to_kitty_bytes(&e, true, false, false, false, false, false, LEVEL1),
        b"\x1b[57404;5u"
    );
}

// ── Lock key modifier bits ──────────────────────────────────────────

#[test]
fn kitty_caps_lock_modifier_bit() {
    // caps_lock = bit 6 = 64, modifier_val = 64 + 1 = 65
    let e = char_key_event('a', KeyCode::KeyA, ElementState::Pressed);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, true, false, LEVEL4),
        b"\x1b[97;65u"
    );
}

#[test]
fn kitty_num_lock_modifier_bit() {
    // num_lock = bit 7 = 128, modifier_val = 128 + 1 = 129
    let e = char_key_event('a', KeyCode::KeyA, ElementState::Pressed);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, false, true, LEVEL4),
        b"\x1b[97;129u"
    );
}

#[test]
fn kitty_both_locks_modifier_bits() {
    // caps_lock + num_lock = 64 + 128 = 192, modifier_val = 193
    let e = char_key_event('a', KeyCode::KeyA, ElementState::Pressed);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, true, true, LEVEL4),
        b"\x1b[97;193u"
    );
}

#[test]
fn kitty_caps_lock_with_shift() {
    // shift(1) + caps_lock(64) = 65, modifier_val = 66
    let e = char_key_event('a', KeyCode::KeyA, ElementState::Pressed);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, true, false, false, true, false, LEVEL4),
        b"\x1b[97;66u"
    );
}

#[test]
fn kitty_caps_lock_with_ctrl() {
    // ctrl(4) + caps_lock(64) = 68, modifier_val = 69
    let e = char_key_event('a', KeyCode::KeyA, ElementState::Pressed);
    assert_eq!(
        key_event_to_kitty_bytes(&e, true, false, false, false, true, false, LEVEL4),
        b"\x1b[97;69u"
    );
}

#[test]
fn kitty_level1_caps_lock_plain_char_stays_legacy() {
    // At level 1-3, plain character keys without ctrl/alt/super send as raw text
    // even with caps_lock active — consistent with kitty behavior.
    let e = char_key_event('a', KeyCode::KeyA, ElementState::Pressed);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, true, false, LEVEL1),
        b"a"
    );
}

#[test]
fn kitty_level1_caps_lock_with_ctrl_uses_csi_u() {
    // ctrl + caps_lock: ctrl(4) + caps_lock(64) = 68, modifier_val = 69
    let e = char_key_event('a', KeyCode::KeyA, ElementState::Pressed);
    assert_eq!(
        key_event_to_kitty_bytes(&e, true, false, false, false, true, false, LEVEL1),
        b"\x1b[97;69u"
    );
}

#[test]
fn kitty_level1_escape_with_caps_lock() {
    // Escape always uses CSI u in kitty mode. caps_lock(64), modifier_val = 65
    let e = named_key_event(NamedKey::Escape, ElementState::Pressed, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, true, false, LEVEL1),
        b"\x1b[27;65u"
    );
}

#[test]
fn kitty_level1_space_with_num_lock() {
    // Space always uses CSI u in kitty mode. num_lock(128), modifier_val = 129
    let e = named_key_event(NamedKey::Space, ElementState::Pressed, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, false, true, LEVEL1),
        b"\x1b[32;129u"
    );
}

// ── Extended keys ───────────────────────────────────────────────────

#[test]
fn kitty_f13() {
    let e = named_key_event(NamedKey::F13, ElementState::Pressed, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, false, false, LEVEL1),
        b"\x1b[57376u"
    );
}

#[test]
fn kitty_media_play_pause() {
    let e = named_key_event(NamedKey::MediaPlayPause, ElementState::Pressed, false);
    assert_eq!(
        key_event_to_kitty_bytes(&e, false, false, false, false, false, false, LEVEL1),
        b"\x1b[57430u"
    );
}
