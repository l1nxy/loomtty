//! PTY key encoding: converts winit key events to terminal byte sequences.
//!
//! Two encoding paths:
//! - Legacy: traditional VT escape sequences (CSI ~, SS3, etc.)
//! - Kitty: progressive enhancement protocol (CSI u format)

use winit::keyboard::{Key, KeyLocation, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

pub(crate) fn key_event_to_pty_bytes(
    event: &winit::event::KeyEvent,
    ctrl: bool,
    shift: bool,
    alt: bool,
) -> Vec<u8> {
    if let Key::Named(key) = &event.logical_key {
        if let Some(bytes) = encode_legacy_c0_key(key, ctrl, shift, alt, false /* not kitty */) {
            return bytes;
        }

        // Compute xterm-style modifier value: 1 + bits (shift=1, alt=2, ctrl=4)
        let mod_val = legacy_modifier_value(ctrl, shift, alt);

        match key {
            // CSI 1 ; modifier letter  (arrows, Home, End)
            NamedKey::ArrowUp => return legacy_csi_special('A', mod_val),
            NamedKey::ArrowDown => return legacy_csi_special('B', mod_val),
            NamedKey::ArrowRight => return legacy_csi_special('C', mod_val),
            NamedKey::ArrowLeft => return legacy_csi_special('D', mod_val),
            NamedKey::Home => return legacy_csi_special('H', mod_val),
            NamedKey::End => return legacy_csi_special('F', mod_val),
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

    if let Some(text) = key_event_text_for_input(event) {
        return text.as_bytes().to_vec();
    }

    vec![]
}

pub(crate) fn key_event_text_for_input(event: &winit::event::KeyEvent) -> Option<&str> {
    // `event.text` is the authoritative text payload for printable keys.
    // Do NOT fall back to `text_with_all_modifiers()` — it produces partial
    // dead-key composition text which is incorrect for input.
    if let Some(text) = &event.text {
        let s: &str = text;
        if !s.is_empty() {
            return Some(s);
        }
    }

    if let Key::Character(c) = &event.logical_key {
        let s = c.as_str();
        if !s.is_empty() {
            return Some(s);
        }
    }

    None
}

pub(crate) fn key_event_base_char(event: &winit::event::KeyEvent) -> Option<char> {
    event
        .key_without_modifiers()
        .to_text()
        .and_then(|text| text.chars().next())
        .or_else(|| physical_key_to_base_char(event.physical_key))
}

// ── Legacy encoding helpers ──────────────────────────────────────────

/// Compute xterm-style modifier value: 1 + (shift=1 | alt=2 | ctrl=4).
/// Returns 0 when no modifiers are active (meaning: omit the parameter).
fn legacy_modifier_value(ctrl: bool, shift: bool, alt: bool) -> u8 {
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
/// Without modifiers: CSI letter
fn legacy_csi_special(suffix: char, mod_val: u8) -> Vec<u8> {
    if mod_val > 0 {
        format!("\x1b[1;{mod_val}{suffix}").into_bytes()
    } else {
        format!("\x1b[{suffix}").into_bytes()
    }
}

/// CSI number ; modifier ~  (e.g. Insert, Delete, PageUp, F5-F12)
/// Without modifiers: CSI number ~
fn legacy_csi_tilde(number: u32, mod_val: u8) -> Vec<u8> {
    if mod_val > 0 {
        format!("\x1b[{number};{mod_val}~").into_bytes()
    } else {
        format!("\x1b[{number}~").into_bytes()
    }
}

/// F1-F4: SS3 letter (no mods) or CSI 1 ; modifier letter (with mods)
fn legacy_f1_f4(letter: char, mod_val: u8) -> Vec<u8> {
    if mod_val > 0 {
        format!("\x1b[1;{mod_val}{letter}").into_bytes()
    } else {
        format!("\x1bO{letter}").into_bytes()
    }
}

fn with_meta_prefix(bytes: Vec<u8>, alt: bool) -> Vec<u8> {
    if !alt {
        return bytes;
    }
    let mut prefixed = Vec::with_capacity(bytes.len() + 1);
    prefixed.push(0x1b);
    prefixed.extend_from_slice(&bytes);
    prefixed
}

/// Encode "crash-safe" C0 keys using legacy byte sequences.
///
/// In kitty mode (`kitty_mode = true`), only Enter/Tab/Backspace retain legacy
/// encoding (so the user can type `reset` after a crash).  Escape and Space are
/// NOT crash-safe and should fall through to CSI u encoding in kitty mode.
fn encode_legacy_c0_key(
    key: &NamedKey,
    ctrl: bool,
    shift: bool,
    alt: bool,
    kitty_mode: bool,
) -> Option<Vec<u8>> {
    match key {
        NamedKey::Enter => Some(with_meta_prefix(vec![b'\r'], alt)),
        NamedKey::Backspace => Some(with_meta_prefix(vec![if ctrl { 0x08 } else { 0x7f }], alt)),
        NamedKey::Tab => {
            let bytes = if shift {
                b"\x1b[Z".to_vec()
            } else {
                vec![b'\t']
            };
            Some(with_meta_prefix(bytes, alt))
        }
        NamedKey::Escape => {
            if kitty_mode {
                None // Always use CSI 27u in kitty mode
            } else {
                Some(with_meta_prefix(vec![0x1b], alt))
            }
        }
        NamedKey::Space => {
            if kitty_mode {
                None // Use CSI 32u in kitty mode (not crash-safe)
            } else {
                Some(with_meta_prefix(vec![if ctrl { 0x00 } else { b' ' }], alt))
            }
        }
        _ => None,
    }
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

fn ctrl_mapping_for_legacy_key(ch: char) -> Option<u8> {
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

fn encode_legacy_ascii_text_key(
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

    Some(with_meta_prefix(bytes, alt))
}

fn encode_legacy_text_key(
    event: &winit::event::KeyEvent,
    ctrl: bool,
    shift: bool,
    alt: bool,
) -> Option<Vec<u8>> {
    let base_char = key_event_base_char(event)?;
    let text = key_event_text_for_input(event).unwrap_or_default();
    encode_legacy_ascii_text_key(base_char, text, ctrl, shift, alt)
}

fn encode_kitty_legacy_text_bytes(
    text: Option<&str>,
    state: winit::event::ElementState,
    ctrl: bool,
    alt: bool,
    super_key: bool,
) -> Option<Vec<u8>> {
    if ctrl || alt || super_key {
        return None;
    }

    let text = text?;
    if state == winit::event::ElementState::Released {
        return Some(vec![]);
    }

    Some(text.as_bytes().to_vec())
}

/// Encode a key event using the Kitty keyboard protocol (CSI u format).
///
/// Full format: CSI unicode-key-code:shifted-key:base-layout-key ; modifiers:event-type ; text-as-codepoints u
/// Modifier bits: shift=1, alt=2, ctrl=4, super=8, hyper=16, meta=32, caps_lock=64, num_lock=128
/// Transmitted modifier value = bits + 1 (1 = no modifiers).
///
/// Levels:
///   1 (DISAMBIGUATE): CSI u encoding for ambiguous keys
///   2 (REPORT_EVENTS): Include event type (press=1, repeat=2, release=3)
///   3 (REPORT_ALTERNATES): Include shifted/base layout key codepoints
///   4 (REPORT_ALL): ALL keys use CSI u, no legacy sequences
///   5 (REPORT_TEXT): Include associated text as colon-separated codepoints
pub(crate) fn key_event_to_kitty_bytes(
    event: &winit::event::KeyEvent,
    ctrl: bool,
    shift: bool,
    alt: bool,
    super_key: bool,
    caps_lock: bool,
    num_lock: bool,
    kitty_flags: u16,
) -> Vec<u8> {
    use ciri_protocol::message::{
        MODE_KITTY_REPORT_ALL, MODE_KITTY_REPORT_ALTERNATES, MODE_KITTY_REPORT_EVENTS,
        MODE_KITTY_REPORT_TEXT,
    };

    let report_events = kitty_flags & MODE_KITTY_REPORT_EVENTS != 0;
    let report_alternates = kitty_flags & MODE_KITTY_REPORT_ALTERNATES != 0;
    let report_all = kitty_flags & MODE_KITTY_REPORT_ALL != 0;
    let report_text = kitty_flags & MODE_KITTY_REPORT_TEXT != 0;

    // Compute modifier value (kitty uses modifier_bits + 1)
    let mut modifier_bits: u16 = 0;
    if shift {
        modifier_bits |= 1;
    }
    if alt {
        modifier_bits |= 2;
    }
    if ctrl {
        modifier_bits |= 4;
    }
    if super_key {
        modifier_bits |= 8;
    }
    // hyper (16) and meta (32) are not available from winit
    if caps_lock {
        modifier_bits |= 64;
    }
    if num_lock {
        modifier_bits |= 128;
    }
    let modifier_val = modifier_bits + 1; // 1 = no modifiers

    // Determine event type for level 2+
    let event_type: u8 = if report_events {
        match event.state {
            winit::event::ElementState::Pressed => {
                if event.repeat {
                    2
                } else {
                    1
                }
            }
            winit::event::ElementState::Released => 3,
        }
    } else {
        0 // not reported
    };

    // For level 2+, skip release events if the app didn't request event reporting
    if !report_events && event.state == winit::event::ElementState::Released {
        return vec![];
    }

    // Build the modifier+event_type parameter: "modifiers:event_type" or just "modifiers"
    // Per spec: event_type=1 (press) is the default and should be omitted.
    let format_modifier_param = |mod_val: u16, evt_type: u8| -> String {
        if evt_type > 1 {
            // Non-press event (repeat=2, release=3): always include
            format!("{mod_val}:{evt_type}")
        } else if mod_val > 1 {
            format!("{mod_val}")
        } else {
            String::new()
        }
    };

    // Get the text associated with the key for level 5.
    // Per spec: C0 (< U+0020) and C1 (U+0080-U+009F) control codes are forbidden.
    let associated_text: Option<String> = if report_text {
        event.text_with_all_modifiers().and_then(|t| {
            let s: &str = t;
            if s.is_empty() || ctrl || alt || super_key {
                None
            } else {
                let codepoints: Vec<String> = s
                    .chars()
                    .filter(|&c| {
                        let cp = c as u32;
                        cp >= 0x20 && !(0x80..=0x9F).contains(&cp)
                    })
                    .map(|c| format!("{}", c as u32))
                    .collect();
                if codepoints.is_empty() {
                    None
                } else {
                    Some(codepoints.join(":"))
                }
            }
        })
    } else {
        None
    };

    // Helper: get the shifted key codepoint for level 3.
    // Per spec: "the shifted key must be present only if shift is also present
    // in the modifiers" and should be "the character that would be produced if
    // only the Shift key was held."
    let shifted_key: u32 = if report_alternates && shift {
        if !ctrl && !alt && !super_key {
            // Shift is the only modifier: text_with_all_modifiers is correct.
            event
                .text_with_all_modifiers()
                .and_then(|t| t.chars().next())
                .map(|c| c as u32)
                .unwrap_or(0)
        } else {
            // Other modifiers also active: text_with_all_modifiers includes
            // ctrl/alt effects (e.g. control byte for Ctrl+Shift+A). Derive
            // the shift-only text from the base character instead.
            key_event_base_char(event)
                .map(|c| {
                    if c.is_ascii_lowercase() {
                        c.to_ascii_uppercase() as u32
                    } else {
                        // Non-letter keys (e.g. '1' → '!'): we can't reliably
                        // determine shift-only text without platform help. Omit.
                        0
                    }
                })
                .unwrap_or(0)
        }
    } else {
        0
    };

    // Helper: get the base layout key codepoint for level 3
    let base_layout_key: u32 = if report_alternates {
        event
            .key_without_modifiers()
            .to_text()
            .and_then(|t| t.chars().next())
            .map(|c| c as u32)
            .unwrap_or(0)
    } else {
        0
    };

    // Helper: format CSI <keycode>[:shifted[:base]] [; modifier[:event_type] [; text]] u
    let csi_u_full = |keycode: u32| -> Vec<u8> {
        let mod_param = format_modifier_param(modifier_val, event_type);
        let text_param = associated_text.as_deref().unwrap_or("");

        // Build the key part: keycode[:shifted_key[:base_layout_key]]
        let key_part = if report_alternates && (shifted_key != 0 || base_layout_key != 0) {
            if base_layout_key != 0 && base_layout_key != keycode {
                format!(
                    "{}:{}:{}",
                    keycode,
                    if shifted_key != 0 && shifted_key != keycode {
                        shifted_key.to_string()
                    } else {
                        String::new()
                    },
                    base_layout_key
                )
            } else if shifted_key != 0 && shifted_key != keycode {
                format!("{}:{}", keycode, shifted_key)
            } else {
                format!("{}", keycode)
            }
        } else {
            format!("{}", keycode)
        };

        if !text_param.is_empty() {
            // Level 5: CSI key ; mod:event ; text u
            let mod_str = if mod_param.is_empty() {
                "1".to_string()
            } else {
                mod_param
            };
            format!("\x1b[{};{};{}u", key_part, mod_str, text_param).into_bytes()
        } else if !mod_param.is_empty() {
            format!("\x1b[{};{}u", key_part, mod_param).into_bytes()
        } else {
            format!("\x1b[{}u", key_part).into_bytes()
        }
    };

    // Helper: format CSI 1 ; modifier <suffix> for special keys (arrows, home, end)
    // Per spec: "if the encoded [modifier] value is 1, it should be omitted."
    let csi_special = |suffix: char| -> Vec<u8> {
        let mod_param = format_modifier_param(modifier_val, event_type);
        if !mod_param.is_empty() {
            format!("\x1b[1;{}{}", mod_param, suffix).into_bytes()
        } else {
            format!("\x1b[{}", suffix).into_bytes()
        }
    };

    // Helper: format CSI <keycode> ; modifier ~ for tilde keys
    let csi_tilde = |keycode: u32| -> Vec<u8> {
        let mod_param = format_modifier_param(modifier_val, event_type);
        if !mod_param.is_empty() {
            format!("\x1b[{};{}~", keycode, mod_param).into_bytes()
        } else {
            format!("\x1b[{}~", keycode).into_bytes()
        }
    };

    // Numpad keys: always use KP_* codes in kitty mode (before legacy fallback).
    if event.location == KeyLocation::Numpad {
        let kp_code: Option<u32> = match &event.logical_key {
            Key::Character(c) => match c.as_str() {
                "0" => Some(57399),
                "1" => Some(57400),
                "2" => Some(57401),
                "3" => Some(57402),
                "4" => Some(57403),
                "5" => Some(57404),
                "6" => Some(57405),
                "7" => Some(57406),
                "8" => Some(57407),
                "9" => Some(57408),
                "." => Some(57409),
                "/" => Some(57410),
                "*" => Some(57411),
                "-" => Some(57412),
                "+" => Some(57413),
                "=" => Some(57415),
                _ => None,
            },
            Key::Named(key) => match key {
                NamedKey::Enter => Some(57414),
                NamedKey::ArrowLeft => Some(57417),
                NamedKey::ArrowRight => Some(57418),
                NamedKey::ArrowUp => Some(57419),
                NamedKey::ArrowDown => Some(57420),
                NamedKey::PageUp => Some(57421),
                NamedKey::PageDown => Some(57422),
                NamedKey::Home => Some(57423),
                NamedKey::End => Some(57424),
                NamedKey::Insert => Some(57425),
                NamedKey::Delete => Some(57426),
                // KP_BEGIN: numpad 5 with numlock off
                NamedKey::Clear => Some(57427),
                _ => None,
            },
            _ => None,
        };
        if let Some(code) = kp_code {
            return csi_u_full(code);
        }
    }

    // Legacy fallback for levels 1-3: crash-safe keys + plain text keys.
    // Must be AFTER numpad (numpad Enter should not fall into legacy \r).
    if !report_all {
        // Crash-safe keys (Enter/Tab/Backspace) keep legacy encoding at levels 1-3.
        // Escape and Space fall through to CSI u encoding.
        if let Key::Named(key) = &event.logical_key
            && let Some(bytes) = encode_legacy_c0_key(key, ctrl, shift, alt, true /* kitty */)
        {
            if event.state == winit::event::ElementState::Released {
                return vec![];
            }
            return bytes;
        }

        // Plain text keys without special modifiers: send as raw text (legacy compat).
        if let Some(bytes) = encode_kitty_legacy_text_bytes(
            key_event_text_for_input(event),
            event.state,
            ctrl,
            alt,
            super_key,
        ) {
            return bytes;
        }
    }

    // Named keys
    if let Key::Named(key) = &event.logical_key {
        // Level 4 (report_all): use CSI u for keys that normally have legacy encoding
        let use_csi_u_for_special = report_all;

        return match key {
            // Crash-safe keys: levels 1-3 are handled by `encode_legacy_c0_key`
            // above (which returns before reaching this match). This arm is only
            // reached at level 4 (report_all), so always use CSI u.
            NamedKey::Enter => csi_u_full(13),
            NamedKey::Tab => csi_u_full(9),
            NamedKey::Backspace => csi_u_full(127),
            NamedKey::Escape => csi_u_full(27),
            NamedKey::Space => csi_u_full(32),
            NamedKey::ArrowUp => csi_special('A'),
            NamedKey::ArrowDown => csi_special('B'),
            NamedKey::ArrowRight => csi_special('C'),
            NamedKey::ArrowLeft => csi_special('D'),
            NamedKey::Home => csi_special('H'),
            NamedKey::End => csi_special('F'),
            NamedKey::PageUp => csi_tilde(5),
            NamedKey::PageDown => csi_tilde(6),
            NamedKey::Insert => csi_tilde(2),
            NamedKey::Delete => csi_tilde(3),
            // F1-F4: use letter suffix form (CSI 1;mod P/Q/R/S), same as legacy.
            // Note: F3 avoids CSI R which conflicts with Cursor Position Report.
            NamedKey::F1 => csi_special('P'),
            NamedKey::F2 => csi_special('Q'),
            NamedKey::F3 => csi_tilde(13),
            NamedKey::F4 => csi_special('S'),
            NamedKey::F5 => csi_tilde(15),
            NamedKey::F6 => csi_tilde(17),
            NamedKey::F7 => csi_tilde(18),
            NamedKey::F8 => csi_tilde(19),
            NamedKey::F9 => csi_tilde(20),
            NamedKey::F10 => csi_tilde(21),
            NamedKey::F11 => csi_tilde(23),
            NamedKey::F12 => csi_tilde(24),
            NamedKey::CapsLock => csi_u_full(57358),
            NamedKey::ScrollLock => csi_u_full(57359),
            NamedKey::NumLock => csi_u_full(57360),
            NamedKey::PrintScreen => csi_u_full(57361),
            NamedKey::Pause => csi_u_full(57362),
            NamedKey::ContextMenu => csi_u_full(57363),
            // Modifier-only keys: send in level 4+ (report_all)
            // Left/right distinguished by KeyLocation
            NamedKey::Shift => {
                if report_all {
                    let code = if event.location == KeyLocation::Right { 57447 } else { 57441 };
                    csi_u_full(code)
                } else {
                    vec![]
                }
            }
            NamedKey::Control => {
                if report_all {
                    let code = if event.location == KeyLocation::Right { 57448 } else { 57442 };
                    csi_u_full(code)
                } else {
                    vec![]
                }
            }
            NamedKey::Alt => {
                if report_all {
                    let code = if event.location == KeyLocation::Right { 57449 } else { 57443 };
                    csi_u_full(code)
                } else {
                    vec![]
                }
            }
            NamedKey::Super => {
                if report_all {
                    let code = if event.location == KeyLocation::Right { 57450 } else { 57444 };
                    csi_u_full(code)
                } else {
                    vec![]
                }
            }
            NamedKey::Hyper => {
                if report_all {
                    let code = if event.location == KeyLocation::Right { 57451 } else { 57445 };
                    csi_u_full(code)
                } else {
                    vec![]
                }
            }
            NamedKey::Meta => {
                if report_all {
                    let code = if event.location == KeyLocation::Right { 57452 } else { 57446 };
                    csi_u_full(code)
                } else {
                    vec![]
                }
            }
            // F13-F35
            NamedKey::F13 => csi_u_full(57376),
            NamedKey::F14 => csi_u_full(57377),
            NamedKey::F15 => csi_u_full(57378),
            NamedKey::F16 => csi_u_full(57379),
            NamedKey::F17 => csi_u_full(57380),
            NamedKey::F18 => csi_u_full(57381),
            NamedKey::F19 => csi_u_full(57382),
            NamedKey::F20 => csi_u_full(57383),
            NamedKey::F21 => csi_u_full(57384),
            NamedKey::F22 => csi_u_full(57385),
            NamedKey::F23 => csi_u_full(57386),
            NamedKey::F24 => csi_u_full(57387),
            NamedKey::F25 => csi_u_full(57388),
            NamedKey::F26 => csi_u_full(57389),
            NamedKey::F27 => csi_u_full(57390),
            NamedKey::F28 => csi_u_full(57391),
            NamedKey::F29 => csi_u_full(57392),
            NamedKey::F30 => csi_u_full(57393),
            NamedKey::F31 => csi_u_full(57394),
            NamedKey::F32 => csi_u_full(57395),
            NamedKey::F33 => csi_u_full(57396),
            NamedKey::F34 => csi_u_full(57397),
            NamedKey::F35 => csi_u_full(57398),
            // Media keys
            NamedKey::MediaPlay => csi_u_full(57428),
            NamedKey::MediaPause => csi_u_full(57429),
            NamedKey::MediaPlayPause => csi_u_full(57430),
            NamedKey::MediaStop => csi_u_full(57432),
            NamedKey::MediaFastForward => csi_u_full(57433),
            NamedKey::MediaRewind => csi_u_full(57434),
            NamedKey::MediaTrackNext => csi_u_full(57435),
            NamedKey::MediaTrackPrevious => csi_u_full(57436),
            NamedKey::MediaRecord => csi_u_full(57437),
            NamedKey::AudioVolumeDown => csi_u_full(57438),
            NamedKey::AudioVolumeUp => csi_u_full(57439),
            NamedKey::AudioVolumeMute => csi_u_full(57440),
            _ => vec![],
        };
    }

    // Character keys: encode as CSI <unicode_codepoint> [; modifier] u
    if let Key::Character(c) = &event.logical_key {
        let text = c.as_str();
        if let Some(ch) = text.chars().next() {
            // Kitty spec: key code is always the lowercase/unshifted codepoint.
            // When any modifier changes the character (ctrl → control byte,
            // shift → uppercase, alt → unchanged), recover the base key.
            let codepoint = if ctrl || shift || alt || super_key {
                key_event_base_char(event)
                    .map(|c| c as u32)
                    .unwrap_or(ch as u32)
            } else {
                ch as u32
            };

            // Level 4: even plain printable chars use CSI u
            // Level 1-3: only use CSI u if there are modifiers or the key is ambiguous
            // Level 4: all keys as CSI u. Levels 1-3: only when modifiers present
            // or non-press event (repeat=2, release=3). Plain press stays legacy text.
            if report_all || modifier_val > 1 || event_type > 1 {
                return csi_u_full(codepoint);
            } else {
                // Plain character, no modifiers, level 1-3: send as-is (legacy)
                return text.as_bytes().to_vec();
            }
        }
    }

    vec![]
}

/// Map a physical key code to its base (unshifted, unmodified) character.
fn physical_key_to_base_char(key: winit::keyboard::PhysicalKey) -> Option<char> {
    use winit::keyboard::{KeyCode, PhysicalKey};
    match key {
        PhysicalKey::Code(code) => match code {
            KeyCode::KeyA => Some('a'),
            KeyCode::KeyB => Some('b'),
            KeyCode::KeyC => Some('c'),
            KeyCode::KeyD => Some('d'),
            KeyCode::KeyE => Some('e'),
            KeyCode::KeyF => Some('f'),
            KeyCode::KeyG => Some('g'),
            KeyCode::KeyH => Some('h'),
            KeyCode::KeyI => Some('i'),
            KeyCode::KeyJ => Some('j'),
            KeyCode::KeyK => Some('k'),
            KeyCode::KeyL => Some('l'),
            KeyCode::KeyM => Some('m'),
            KeyCode::KeyN => Some('n'),
            KeyCode::KeyO => Some('o'),
            KeyCode::KeyP => Some('p'),
            KeyCode::KeyQ => Some('q'),
            KeyCode::KeyR => Some('r'),
            KeyCode::KeyS => Some('s'),
            KeyCode::KeyT => Some('t'),
            KeyCode::KeyU => Some('u'),
            KeyCode::KeyV => Some('v'),
            KeyCode::KeyW => Some('w'),
            KeyCode::KeyX => Some('x'),
            KeyCode::KeyY => Some('y'),
            KeyCode::KeyZ => Some('z'),
            KeyCode::Digit0 => Some('0'),
            KeyCode::Digit1 => Some('1'),
            KeyCode::Digit2 => Some('2'),
            KeyCode::Digit3 => Some('3'),
            KeyCode::Digit4 => Some('4'),
            KeyCode::Digit5 => Some('5'),
            KeyCode::Digit6 => Some('6'),
            KeyCode::Digit7 => Some('7'),
            KeyCode::Digit8 => Some('8'),
            KeyCode::Digit9 => Some('9'),
            KeyCode::Space => Some(' '),
            KeyCode::Minus => Some('-'),
            KeyCode::Equal => Some('='),
            KeyCode::BracketLeft => Some('['),
            KeyCode::BracketRight => Some(']'),
            KeyCode::Backslash => Some('\\'),
            KeyCode::IntlBackslash => Some('\\'),
            KeyCode::Semicolon => Some(';'),
            KeyCode::Quote => Some('\''),
            KeyCode::Backquote => Some('`'),
            KeyCode::Comma => Some(','),
            KeyCode::Period => Some('.'),
            KeyCode::Slash => Some('/'),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::event::ElementState;
    use winit::keyboard::{KeyCode, KeyLocation, NamedKey, PhysicalKey};

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
            encode_legacy_c0_key(&NamedKey::Escape, false, false, false, true /* kitty */),
            None
        );
        // Space in kitty mode → None (falls through to CSI u)
        assert_eq!(
            encode_legacy_c0_key(&NamedKey::Space, true, false, false, true),
            None
        );
        // Space in legacy mode with ctrl → 0x00
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

    // ── Legacy functional key modifier tests ─────────────────────────

    #[test]
    fn legacy_modifier_value_computation() {
        assert_eq!(legacy_modifier_value(false, false, false), 0);
        assert_eq!(legacy_modifier_value(false, true, false), 2); // shift=1, +1=2
        assert_eq!(legacy_modifier_value(false, false, true), 3); // alt=2, +1=3
        assert_eq!(legacy_modifier_value(true, false, false), 5); // ctrl=4, +1=5
        assert_eq!(legacy_modifier_value(true, true, false), 6); // ctrl+shift=5, +1=6
        assert_eq!(legacy_modifier_value(true, true, true), 8); // all=7, +1=8
    }

    #[test]
    fn legacy_arrow_keys_with_modifiers() {
        // No modifiers: plain CSI A
        assert_eq!(legacy_csi_special('A', 0), b"\x1b[A");
        // Shift+Up: CSI 1;2A
        assert_eq!(legacy_csi_special('A', 2), b"\x1b[1;2A");
        // Ctrl+Right: CSI 1;5C
        assert_eq!(legacy_csi_special('C', 5), b"\x1b[1;5C");
    }

    #[test]
    fn legacy_tilde_keys_with_modifiers() {
        // Delete no mods
        assert_eq!(legacy_csi_tilde(3, 0), b"\x1b[3~");
        // Shift+Delete: CSI 3;2~
        assert_eq!(legacy_csi_tilde(3, 2), b"\x1b[3;2~");
        // Ctrl+PageUp: CSI 5;5~
        assert_eq!(legacy_csi_tilde(5, 5), b"\x1b[5;5~");
    }

    #[test]
    fn legacy_f1_f4_with_modifiers() {
        // F1 no mods: SS3 P
        assert_eq!(legacy_f1_f4('P', 0), b"\x1bOP");
        // Shift+F1: CSI 1;2P
        assert_eq!(legacy_f1_f4('P', 2), b"\x1b[1;2P");
    }

    #[test]
    fn legacy_shift_up_via_pty_bytes() {
        let event = named_key_event(NamedKey::ArrowUp, ElementState::Pressed, false);
        assert_eq!(
            key_event_to_pty_bytes(&event, false, true, false),
            b"\x1b[1;2A"
        );
    }

    #[test]
    fn legacy_ctrl_delete_via_pty_bytes() {
        let event = named_key_event(NamedKey::Delete, ElementState::Pressed, false);
        assert_eq!(
            key_event_to_pty_bytes(&event, true, false, false),
            b"\x1b[3;5~"
        );
    }

    // ── Kitty protocol level 1 (DISAMBIGUATE) ───────────────────────

    const LEVEL1: u16 = ciri_protocol::message::MODE_KITTY_KEYBOARD;

    #[test]
    fn kitty_level1_escape_uses_csi_u() {
        let event = named_key_event(NamedKey::Escape, ElementState::Pressed, false);
        assert_eq!(key_event_to_kitty_bytes(&event, false, false, false, false, false, false, LEVEL1), b"\x1b[27u");
    }

    #[test]
    fn kitty_level1_space_uses_csi_u() {
        let event = named_key_event(NamedKey::Space, ElementState::Pressed, false);
        assert_eq!(key_event_to_kitty_bytes(&event, false, false, false, false, false, false, LEVEL1), b"\x1b[32u");
    }

    #[test]
    fn kitty_level1_ctrl_space_uses_csi_u() {
        let event = named_key_event(NamedKey::Space, ElementState::Pressed, false);
        assert_eq!(key_event_to_kitty_bytes(&event, true, false, false, false, false, false, LEVEL1), b"\x1b[32;5u");
    }

    #[test]
    fn kitty_level1_enter_stays_legacy() {
        let event = named_key_event(NamedKey::Enter, ElementState::Pressed, false);
        assert_eq!(key_event_to_kitty_bytes(&event, false, false, false, false, false, false, LEVEL1), b"\r");
    }

    // Fix #2/#3: Ctrl+Enter MUST stay legacy at levels 1-3 (crash-safe)
    #[test]
    fn kitty_level1_ctrl_enter_stays_legacy() {
        let event = named_key_event(NamedKey::Enter, ElementState::Pressed, false);
        assert_eq!(key_event_to_kitty_bytes(&event, true, false, false, false, false, false, LEVEL1), b"\r");
    }

    #[test]
    fn kitty_level1_ctrl_tab_stays_legacy() {
        let event = named_key_event(NamedKey::Tab, ElementState::Pressed, false);
        assert_eq!(key_event_to_kitty_bytes(&event, true, false, false, false, false, false, LEVEL1), b"\t");
    }

    #[test]
    fn kitty_level1_ctrl_backspace_stays_legacy() {
        let event = named_key_event(NamedKey::Backspace, ElementState::Pressed, false);
        // ctrl+backspace = 0x08
        assert_eq!(key_event_to_kitty_bytes(&event, true, false, false, false, false, false, LEVEL1), b"\x08");
    }

    #[test]
    fn kitty_level1_arrow_up_with_shift() {
        let event = named_key_event(NamedKey::ArrowUp, ElementState::Pressed, false);
        assert_eq!(key_event_to_kitty_bytes(&event, false, true, false, false, false, false, LEVEL1), b"\x1b[1;2A");
    }

    #[test]
    fn kitty_level1_plain_char_stays_legacy() {
        let event = char_key_event('a', KeyCode::KeyA, ElementState::Pressed);
        assert_eq!(key_event_to_kitty_bytes(&event, false, false, false, false, false, false, LEVEL1), b"a");
    }

    // Fix #1: F1 uses letter suffix (CSI 1;mod P), not tilde
    #[test]
    fn kitty_level1_f1_no_mods() {
        let event = named_key_event(NamedKey::F1, ElementState::Pressed, false);
        assert_eq!(key_event_to_kitty_bytes(&event, false, false, false, false, false, false, LEVEL1), b"\x1b[P");
    }

    #[test]
    fn kitty_level1_f1_with_shift() {
        let event = named_key_event(NamedKey::F1, ElementState::Pressed, false);
        assert_eq!(key_event_to_kitty_bytes(&event, false, true, false, false, false, false, LEVEL1), b"\x1b[1;2P");
    }

    #[test]
    fn kitty_level1_f3_uses_tilde() {
        // F3 uses tilde form (CSI 13~) to avoid conflict with Cursor Position Report
        let event = named_key_event(NamedKey::F3, ElementState::Pressed, false);
        assert_eq!(key_event_to_kitty_bytes(&event, false, false, false, false, false, false, LEVEL1), b"\x1b[13~");
    }

    #[test]
    fn kitty_level1_f4_with_ctrl() {
        let event = named_key_event(NamedKey::F4, ElementState::Pressed, false);
        assert_eq!(key_event_to_kitty_bytes(&event, true, false, false, false, false, false, LEVEL1), b"\x1b[1;5S");
    }

    #[test]
    fn kitty_level1_f5_uses_tilde() {
        let event = named_key_event(NamedKey::F5, ElementState::Pressed, false);
        assert_eq!(key_event_to_kitty_bytes(&event, false, true, false, false, false, false, LEVEL1), b"\x1b[15;2~");
    }

    // ── Kitty protocol level 2 (REPORT_EVENTS) ─────────────────────

    const LEVEL2: u16 = LEVEL1 | ciri_protocol::message::MODE_KITTY_REPORT_EVENTS;

    #[test]
    fn kitty_level2_plain_char_press_stays_legacy() {
        // Issue B: plain 'a' press at level 2 must still be raw text, not CSI 97u
        let event = char_key_event('a', KeyCode::KeyA, ElementState::Pressed);
        assert_eq!(key_event_to_kitty_bytes(&event, false, false, false, false, false, false, LEVEL2), b"a");
    }

    #[test]
    fn kitty_level2_escape_release() {
        let event = named_key_event(NamedKey::Escape, ElementState::Released, false);
        assert_eq!(key_event_to_kitty_bytes(&event, false, false, false, false, false, false, LEVEL2), b"\x1b[27;1:3u");
    }

    #[test]
    fn kitty_level2_escape_repeat() {
        let event = named_key_event(NamedKey::Escape, ElementState::Pressed, true);
        assert_eq!(key_event_to_kitty_bytes(&event, false, false, false, false, false, false, LEVEL2), b"\x1b[27;1:2u");
    }

    // Fix #8: Enter release suppressed at level 2 (even with modifiers)
    #[test]
    fn kitty_level2_enter_release_suppressed_no_mods() {
        let event = named_key_event(NamedKey::Enter, ElementState::Released, false);
        assert_eq!(key_event_to_kitty_bytes(&event, false, false, false, false, false, false, LEVEL2), b"");
    }

    #[test]
    fn kitty_level2_ctrl_enter_release_suppressed() {
        let event = named_key_event(NamedKey::Enter, ElementState::Released, false);
        assert_eq!(key_event_to_kitty_bytes(&event, true, false, false, false, false, false, LEVEL2), b"");
    }

    #[test]
    fn kitty_level2_tab_release_suppressed() {
        let event = named_key_event(NamedKey::Tab, ElementState::Released, false);
        assert_eq!(key_event_to_kitty_bytes(&event, false, false, false, false, false, false, LEVEL2), b"");
    }

    #[test]
    fn kitty_level2_backspace_release_suppressed() {
        let event = named_key_event(NamedKey::Backspace, ElementState::Released, false);
        assert_eq!(key_event_to_kitty_bytes(&event, true, false, false, false, false, false, LEVEL2), b"");
    }

    // ── Kitty protocol level 4 (REPORT_ALL) ─────────────────────────

    const LEVEL4: u16 = LEVEL1
        | ciri_protocol::message::MODE_KITTY_REPORT_EVENTS
        | ciri_protocol::message::MODE_KITTY_REPORT_ALTERNATES
        | ciri_protocol::message::MODE_KITTY_REPORT_ALL;

    #[test]
    fn kitty_level4_enter_uses_csi_u() {
        let event = named_key_event(NamedKey::Enter, ElementState::Pressed, false);
        assert_eq!(key_event_to_kitty_bytes(&event, false, false, false, false, false, false, LEVEL4), b"\x1b[13u");
    }

    #[test]
    fn kitty_level4_enter_release_reported() {
        // At level 4, Enter release IS reported (unlike levels 1-3)
        let event = named_key_event(NamedKey::Enter, ElementState::Released, false);
        assert_eq!(key_event_to_kitty_bytes(&event, false, false, false, false, false, false, LEVEL4), b"\x1b[13;1:3u");
    }

    #[test]
    fn kitty_level4_plain_char_uses_csi_u() {
        let event = char_key_event('a', KeyCode::KeyA, ElementState::Pressed);
        assert_eq!(key_event_to_kitty_bytes(&event, false, false, false, false, false, false, LEVEL4), b"\x1b[97u");
    }

    #[test]
    fn kitty_level4_modifier_keys_reported() {
        let event = named_key_event_loc(NamedKey::Shift, ElementState::Pressed, false, KeyLocation::Left);
        assert_eq!(key_event_to_kitty_bytes(&event, false, true, false, false, false, false, LEVEL4), b"\x1b[57441;2u");
    }

    #[test]
    fn kitty_level4_right_shift_uses_right_code() {
        let event = named_key_event_loc(NamedKey::Shift, ElementState::Pressed, false, KeyLocation::Right);
        assert_eq!(key_event_to_kitty_bytes(&event, false, true, false, false, false, false, LEVEL4), b"\x1b[57447;2u");
    }

    #[test]
    fn kitty_level4_right_ctrl_uses_right_code() {
        let event = named_key_event_loc(NamedKey::Control, ElementState::Pressed, false, KeyLocation::Right);
        assert_eq!(key_event_to_kitty_bytes(&event, true, false, false, false, false, false, LEVEL4), b"\x1b[57448;5u");
    }

    // ── Numpad keys ─────────────────────────────────────────────────

    #[test]
    fn kitty_numpad_digit() {
        let event = numpad_char_event("5", KeyCode::Numpad5);
        assert_eq!(key_event_to_kitty_bytes(&event, false, false, false, false, false, false, LEVEL1), b"\x1b[57404u");
    }

    #[test]
    fn kitty_numpad_enter() {
        let event = numpad_named_event(NamedKey::Enter, KeyCode::NumpadEnter);
        assert_eq!(key_event_to_kitty_bytes(&event, false, false, false, false, false, false, LEVEL1), b"\x1b[57414u");
    }

    #[test]
    fn kitty_numpad_with_modifier() {
        let event = numpad_char_event("5", KeyCode::Numpad5);
        assert_eq!(key_event_to_kitty_bytes(&event, true, false, false, false, false, false, LEVEL1), b"\x1b[57404;5u");
    }

    // ── Caps Lock modifier bit ──────────────────────────────────────

    #[test]
    fn kitty_caps_lock_modifier_bit() {
        let event = char_key_event('a', KeyCode::KeyA, ElementState::Pressed);
        assert_eq!(key_event_to_kitty_bytes(&event, false, false, false, false, true, false, LEVEL4), b"\x1b[97;65u");
    }

    #[test]
    fn kitty_num_lock_modifier_bit() {
        let event = char_key_event('a', KeyCode::KeyA, ElementState::Pressed);
        // num_lock=128, +1 = 129
        assert_eq!(key_event_to_kitty_bytes(&event, false, false, false, false, false, true, LEVEL4), b"\x1b[97;129u");
    }

    // ── F13+ and media keys ─────────────────────────────────────────

    #[test]
    fn kitty_f13() {
        let event = named_key_event(NamedKey::F13, ElementState::Pressed, false);
        assert_eq!(key_event_to_kitty_bytes(&event, false, false, false, false, false, false, LEVEL1), b"\x1b[57376u");
    }

    #[test]
    fn kitty_media_play_pause() {
        let event = named_key_event(NamedKey::MediaPlayPause, ElementState::Pressed, false);
        assert_eq!(key_event_to_kitty_bytes(&event, false, false, false, false, false, false, LEVEL1), b"\x1b[57430u");
    }
}
