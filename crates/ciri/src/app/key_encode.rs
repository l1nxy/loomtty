//! PTY key encoding: converts winit key events to terminal byte sequences.
//!
//! Two encoding paths:
//! - Legacy: traditional VT escape sequences (CSI ~, SS3, etc.)
//! - Kitty: progressive enhancement protocol (CSI u format)

use winit::keyboard::{Key, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

pub(crate) fn key_event_to_pty_bytes(
    event: &winit::event::KeyEvent,
    ctrl: bool,
    shift: bool,
    alt: bool,
) -> Vec<u8> {
    if let Key::Named(key) = &event.logical_key {
        if let Some(bytes) = encode_legacy_c0_key(key, ctrl, shift, alt, false) {
            return bytes;
        }
        match key {
            NamedKey::ArrowUp => return b"\x1b[A".to_vec(),
            NamedKey::ArrowDown => return b"\x1b[B".to_vec(),
            NamedKey::ArrowRight => return b"\x1b[C".to_vec(),
            NamedKey::ArrowLeft => return b"\x1b[D".to_vec(),
            NamedKey::Home => return b"\x1b[H".to_vec(),
            NamedKey::End => return b"\x1b[F".to_vec(),
            NamedKey::PageUp => return b"\x1b[5~".to_vec(),
            NamedKey::PageDown => return b"\x1b[6~".to_vec(),
            NamedKey::Delete => return b"\x1b[3~".to_vec(),
            NamedKey::Insert => return b"\x1b[2~".to_vec(),
            NamedKey::F1 => return b"\x1bOP".to_vec(),
            NamedKey::F2 => return b"\x1bOQ".to_vec(),
            NamedKey::F3 => return b"\x1bOR".to_vec(),
            NamedKey::F4 => return b"\x1bOS".to_vec(),
            NamedKey::F5 => return b"\x1b[15~".to_vec(),
            NamedKey::F6 => return b"\x1b[17~".to_vec(),
            NamedKey::F7 => return b"\x1b[18~".to_vec(),
            NamedKey::F8 => return b"\x1b[19~".to_vec(),
            NamedKey::F9 => return b"\x1b[20~".to_vec(),
            NamedKey::F10 => return b"\x1b[21~".to_vec(),
            NamedKey::F11 => return b"\x1b[23~".to_vec(),
            NamedKey::F12 => return b"\x1b[24~".to_vec(),
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
    // `event.text` is the normal text payload for printable keys.
    if let Some(text) = &event.text {
        let s: &str = text;
        if !s.is_empty() {
            return Some(s);
        }
    }

    // Some platforms are more reliable here for shifted punctuation and other
    // keys whose printed glyph depends on modifiers.
    if let Some(text) = event.text_with_all_modifiers()
        && !text.is_empty()
    {
        return Some(text);
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

fn with_meta_prefix(bytes: Vec<u8>, alt: bool) -> Vec<u8> {
    if !alt {
        return bytes;
    }
    let mut prefixed = Vec::with_capacity(bytes.len() + 1);
    prefixed.push(0x1b);
    prefixed.extend_from_slice(&bytes);
    prefixed
}

fn encode_legacy_c0_key(
    key: &NamedKey,
    ctrl: bool,
    shift: bool,
    alt: bool,
    disambiguate_escape: bool,
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
            if disambiguate_escape {
                None
            } else {
                Some(with_meta_prefix(vec![0x1b], alt))
            }
        }
        NamedKey::Space => Some(with_meta_prefix(vec![if ctrl { 0x00 } else { b' ' }], alt)),
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
    report_all: bool,
) -> Option<Vec<u8>> {
    if report_all || ctrl || alt || super_key {
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
/// Modifier bits: shift=1, alt=2, ctrl=4, super=8 (value = bits + 1)
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
    let mut modifier_bits: u8 = 0;
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

    if !report_all {
        if let Key::Named(key) = &event.logical_key
            && let Some(bytes) = encode_legacy_c0_key(key, ctrl, shift, alt, true)
        {
            if event.state == winit::event::ElementState::Released {
                return vec![];
            }
            return bytes;
        }

        if let Some(bytes) = encode_kitty_legacy_text_bytes(
            key_event_text_for_input(event),
            event.state,
            ctrl,
            alt,
            super_key,
            report_all,
        ) {
            return bytes;
        }
    }

    // Build the modifier+event_type parameter: "modifiers:event_type" or just "modifiers"
    let format_modifier_param = |mod_val: u8, evt_type: u8| -> String {
        if evt_type > 0 && (evt_type != 1 || mod_val > 1) {
            // Include event type: either non-press event, or press with modifiers
            format!("{mod_val}:{evt_type}")
        } else if mod_val > 1 {
            format!("{mod_val}")
        } else {
            String::new()
        }
    };

    // Get the text associated with the key for level 5
    let associated_text: Option<String> = if report_text {
        event.text_with_all_modifiers().and_then(|t| {
            let s: &str = t;
            if s.is_empty() || ctrl || alt || super_key {
                None
            } else {
                Some(
                    s.chars()
                        .map(|c| format!("{}", c as u32))
                        .collect::<Vec<_>>()
                        .join(":"),
                )
            }
        })
    } else {
        None
    };

    // Helper: get the shifted key codepoint for level 3 (from the text with shift).
    // Only meaningful when Shift is the sole modifier; with Ctrl/Alt/Super the
    // text_with_all_modifiers() value is not the "shifted" variant of the key.
    let shifted_key: u32 = if report_alternates && shift && !ctrl && !alt && !super_key {
        event
            .text_with_all_modifiers()
            .and_then(|t| t.chars().next())
            .map(|c| c as u32)
            .unwrap_or(0)
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
    let csi_special = |suffix: char| -> Vec<u8> {
        let mod_param = format_modifier_param(modifier_val, event_type);
        if !mod_param.is_empty() {
            format!("\x1b[1;{}{}", mod_param, suffix).into_bytes()
        } else {
            // Fall back to legacy encoding when no modifiers (unless report_all)
            if report_all {
                format!("\x1b[1;1{}", suffix).into_bytes()
            } else {
                format!("\x1b[{}", suffix).into_bytes()
            }
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

    // Named keys first
    if let Key::Named(key) = &event.logical_key {
        // Level 4 (report_all): use CSI u for keys that normally have legacy encoding
        let use_csi_u_for_special = report_all;

        return match key {
            NamedKey::Enter => {
                if use_csi_u_for_special || modifier_val > 1 || event_type > 0 {
                    csi_u_full(13)
                } else {
                    vec![b'\r']
                }
            }
            NamedKey::Tab => {
                if use_csi_u_for_special || modifier_val > 1 || event_type > 0 {
                    csi_u_full(9)
                } else {
                    vec![b'\t']
                }
            }
            NamedKey::Backspace => {
                if use_csi_u_for_special || modifier_val > 1 || event_type > 0 {
                    csi_u_full(127)
                } else {
                    vec![0x7f]
                }
            }
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
            NamedKey::F1 => csi_tilde(11),
            NamedKey::F2 => csi_tilde(12),
            NamedKey::F3 => csi_tilde(13),
            NamedKey::F4 => csi_tilde(14),
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
            NamedKey::Shift => {
                if report_all {
                    csi_u_full(57441)
                } else {
                    vec![]
                }
            }
            NamedKey::Control => {
                if report_all {
                    csi_u_full(57442)
                } else {
                    vec![]
                }
            }
            NamedKey::Alt => {
                if report_all {
                    csi_u_full(57443)
                } else {
                    vec![]
                }
            }
            NamedKey::Super => {
                if report_all {
                    csi_u_full(57444)
                } else {
                    vec![]
                }
            }
            _ => vec![],
        };
    }

    // Character keys: encode as CSI <unicode_codepoint> [; modifier] u
    if let Key::Character(c) = &event.logical_key {
        let text = c.as_str();
        if let Some(ch) = text.chars().next() {
            let codepoint = if ctrl && (ch as u32) < 0x20 {
                // Recover the original letter from physical key
                key_event_base_char(event)
                    .map(|c| c as u32)
                    .unwrap_or(ch as u32)
            } else if shift && !ctrl && !alt && !super_key {
                // Shift-only: use the base (unshifted) key
                key_event_base_char(event)
                    .map(|c| c as u32)
                    .unwrap_or(ch as u32)
            } else if alt && !ctrl && !super_key {
                // Alt-only or Alt+Shift: use the base key
                key_event_base_char(event)
                    .map(|c| c as u32)
                    .unwrap_or(ch as u32)
            } else {
                ch as u32
            };

            // Level 4: even plain printable chars use CSI u
            // Level 1-3: only use CSI u if there are modifiers or the key is ambiguous
            if report_all || modifier_val > 1 || event_type > 0 {
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
    use super::{
        ctrl_mapping_for_legacy_key, encode_kitty_legacy_text_bytes, encode_legacy_ascii_text_key,
        encode_legacy_c0_key, physical_key_to_base_char,
    };
    use winit::event::ElementState;
    use winit::keyboard::NamedKey;
    use winit::keyboard::{KeyCode, PhysicalKey};

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
            encode_legacy_c0_key(&NamedKey::Escape, false, false, false, true),
            None
        );
    }

    #[test]
    fn kitty_compat_mode_keeps_shifted_text_as_text() {
        assert_eq!(
            encode_kitty_legacy_text_bytes(
                Some("|"),
                ElementState::Pressed,
                false,
                false,
                false,
                false
            ),
            Some(vec![b'|'])
        );
        assert_eq!(
            encode_kitty_legacy_text_bytes(
                Some("|"),
                ElementState::Released,
                false,
                false,
                false,
                false
            ),
            Some(vec![])
        );
        assert_eq!(
            encode_kitty_legacy_text_bytes(
                Some("|"),
                ElementState::Pressed,
                false,
                true,
                false,
                false
            ),
            None
        );
    }
}
