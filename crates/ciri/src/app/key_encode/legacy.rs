//! Legacy VT escape sequence encoding (CSI ~, SS3, etc.)

use winit::keyboard::{Key, KeyLocation, NamedKey};

use super::{encode_legacy_c0_key, key_event_base_char, key_event_text_for_input};

pub(crate) fn key_event_to_pty_bytes(
    event: &winit::event::KeyEvent,
    ctrl: bool,
    shift: bool,
    alt: bool,
    app_cursor: bool,
    app_keypad: bool,
) -> Vec<u8> {
    // Numpad keys in application keypad mode (DECKPAM): send SS3 sequences.
    // Must be checked before C0 / named-key handling so numpad Enter doesn't
    // fall through to the regular Enter → CR path.
    // Only bare keys use SS3; Ctrl/Shift modifiers fall through to the normal
    // encoding path so the modifier semantics are preserved.
    if app_keypad && !ctrl && !shift && event.location == KeyLocation::Numpad {
        if let Some(ss3) = numpad_app_keypad_byte(event) {
            return super::with_meta_prefix(format!("\x1bO{}", ss3 as char).into_bytes(), alt);
        }
    }

    if let Key::Named(key) = &event.logical_key {
        if let Some(bytes) = encode_legacy_c0_key(key, ctrl, shift, alt, false /* not kitty */) {
            return bytes;
        }

        // Compute xterm-style modifier value: 1 + bits (shift=1, alt=2, ctrl=4)
        let mod_val = legacy_modifier_value(ctrl, shift, alt);

        match key {
            // CSI 1 ; modifier letter  (arrows, Home, End)
            // When DECCKM (app_cursor) is active and no modifiers, use SS3 (ESC O).
            NamedKey::ArrowUp => return legacy_csi_special('A', mod_val, app_cursor),
            NamedKey::ArrowDown => return legacy_csi_special('B', mod_val, app_cursor),
            NamedKey::ArrowRight => return legacy_csi_special('C', mod_val, app_cursor),
            NamedKey::ArrowLeft => return legacy_csi_special('D', mod_val, app_cursor),
            NamedKey::Home => return legacy_csi_special('H', mod_val, app_cursor),
            NamedKey::End => return legacy_csi_special('F', mod_val, app_cursor),
            // CSI number ; modifier ~  (tilde keys)
            NamedKey::PageUp => return legacy_csi_tilde(5, mod_val),
            NamedKey::PageDown => return legacy_csi_tilde(6, mod_val),
            NamedKey::Delete => return legacy_csi_tilde(3, mod_val),
            NamedKey::Insert => return legacy_csi_tilde(2, mod_val),
            // F1-F4: SS3 letter (no mods) or CSI 1 ; modifier letter (with mods)
            NamedKey::F1 => return legacy_f1_f4('P', mod_val),
            NamedKey::F2 => return legacy_f1_f4('Q', mod_val),
            NamedKey::F3 => return legacy_f1_f4('R', mod_val),
            NamedKey::F4 => return legacy_f1_f4('S', mod_val),
            // F5-F12: CSI number ; modifier ~
            NamedKey::F5 => return legacy_csi_tilde(15, mod_val),
            NamedKey::F6 => return legacy_csi_tilde(17, mod_val),
            NamedKey::F7 => return legacy_csi_tilde(18, mod_val),
            NamedKey::F8 => return legacy_csi_tilde(19, mod_val),
            NamedKey::F9 => return legacy_csi_tilde(20, mod_val),
            NamedKey::F10 => return legacy_csi_tilde(21, mod_val),
            NamedKey::F11 => return legacy_csi_tilde(23, mod_val),
            NamedKey::F12 => return legacy_csi_tilde(24, mod_val),
            _ => {}
        }
    }

    if (ctrl || shift || alt)
        && let Some(bytes) = encode_legacy_text_key(event, ctrl, shift, alt)
    {
        return bytes;
    }

    if let Some(text) = key_event_text_for_input(event, shift) {
        return text.as_bytes().to_vec();
    }

    vec![]
}

/// Map a numpad key event to its DECKPAM SS3 suffix byte (VT220 standard).
/// Returns None if the key isn't a recognized numpad key.
fn numpad_app_keypad_byte(event: &winit::event::KeyEvent) -> Option<u8> {
    match &event.logical_key {
        Key::Character(c) => match c.as_str() {
            "0" => Some(b'p'),
            "1" => Some(b'q'),
            "2" => Some(b'r'),
            "3" => Some(b's'),
            "4" => Some(b't'),
            "5" => Some(b'u'),
            "6" => Some(b'v'),
            "7" => Some(b'w'),
            "8" => Some(b'x'),
            "9" => Some(b'y'),
            "." => Some(b'n'),
            "/" => Some(b'o'),
            "*" => Some(b'j'),
            "-" => Some(b'm'),
            "+" => Some(b'k'),
            "," => Some(b'l'),
            "=" => Some(b'X'),
            _ => None,
        },
        Key::Named(NamedKey::Enter) => Some(b'M'),
        _ => None,
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Compute xterm-style modifier value: 1 + (shift=1 | alt=2 | ctrl=4).
/// Returns 0 when no modifiers are active (meaning: omit the parameter).
pub(super) fn legacy_modifier_value(ctrl: bool, shift: bool, alt: bool) -> u8 {
    let mut bits: u8 = 0;
    if shift {
        bits |= 1;
    }
    if alt {
        bits |= 2;
    }
    if ctrl {
        bits |= 4;
    }
    if bits == 0 { 0 } else { bits + 1 }
}

/// CSI 1 ; modifier letter  (e.g. arrows, Home, End)
/// Without modifiers: CSI letter (normal) or SS3 letter (application cursor keys).
pub(super) fn legacy_csi_special(suffix: char, mod_val: u8, app_cursor: bool) -> Vec<u8> {
    if mod_val > 0 {
        // Modifiers always use CSI form, even in application mode.
        format!("\x1b[1;{mod_val}{suffix}").into_bytes()
    } else if app_cursor {
        // DECCKM active: use SS3 (ESC O) prefix.
        format!("\x1bO{suffix}").into_bytes()
    } else {
        format!("\x1b[{suffix}").into_bytes()
    }
}

/// CSI number ; modifier ~  (e.g. Insert, Delete, PageUp, F5-F12)
/// Without modifiers: CSI number ~
pub(super) fn legacy_csi_tilde(number: u32, mod_val: u8) -> Vec<u8> {
    if mod_val > 0 {
        format!("\x1b[{number};{mod_val}~").into_bytes()
    } else {
        format!("\x1b[{number}~").into_bytes()
    }
}

/// F1-F4: SS3 letter (no mods) or CSI 1 ; modifier letter (with mods)
pub(super) fn legacy_f1_f4(letter: char, mod_val: u8) -> Vec<u8> {
    if mod_val > 0 {
        format!("\x1b[1;{mod_val}{letter}").into_bytes()
    } else {
        format!("\x1bO{letter}").into_bytes()
    }
}

fn encode_legacy_text_key(
    event: &winit::event::KeyEvent,
    ctrl: bool,
    shift: bool,
    alt: bool,
) -> Option<Vec<u8>> {
    let base_char = key_event_base_char(event)?;
    let text = key_event_text_for_input(event, shift);
    encode_legacy_ascii_text_key(
        base_char,
        text.as_deref().unwrap_or_default(),
        ctrl,
        shift,
        alt,
    )
}

pub(super) fn encode_legacy_ascii_text_key(
    base_char: char,
    text: &str,
    ctrl: bool,
    shift: bool,
    alt: bool,
) -> Option<Vec<u8>> {
    if !is_legacy_ascii_text_key(base_char) {
        return None;
    }

    let mut bytes = Vec::new();
    if ctrl {
        bytes.push(ctrl_mapping_for_legacy_key(base_char)?);
    } else if shift {
        if text.is_empty() {
            return None;
        }
        bytes.extend_from_slice(text.as_bytes());
    } else {
        bytes.push(base_char as u8);
    }

    Some(super::with_meta_prefix(bytes, alt))
}

fn is_legacy_ascii_text_key(ch: char) -> bool {
    matches!(
        ch,
        'a'..='z'
            | '0'..='9'
            | '`'
            | '-'
            | '='
            | '['
            | ']'
            | '\\'
            | ';'
            | '\''
            | ','
            | '.'
            | '/'
    )
}

pub(super) fn ctrl_mapping_for_legacy_key(ch: char) -> Option<u8> {
    let ch = ch.to_ascii_lowercase();
    Some(match ch {
        'a'..='z' => ch as u8 - b'a' + 1,
        '2' => 0x00,
        '3' => 0x1b,
        '4' => 0x1c,
        '5' => 0x1d,
        '6' => 0x1e,
        '7' => 0x1f,
        '8' => 0x7f,
        '0' | '1' | '9' => ch as u8,
        '[' => 0x1b,
        '\\' => 0x1c,
        ']' => 0x1d,
        '/' => 0x1f,
        _ => return None,
    })
}
