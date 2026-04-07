//! PTY key encoding: converts winit key events to terminal byte sequences.
//!
//! Two encoding paths:
//! - Legacy (`legacy`): traditional VT escape sequences (CSI ~, SS3, etc.)
//! - Kitty (`kitty`): progressive enhancement protocol (CSI u format)

mod kitty;
mod legacy;

#[cfg(test)]
mod tests;

use winit::keyboard::Key;
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

pub(crate) use kitty::key_event_to_kitty_bytes;
pub(crate) use legacy::key_event_to_pty_bytes;

// ── Shared utilities ─────────────────────────────────────────────────

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
    key: &winit::keyboard::NamedKey,
    ctrl: bool,
    shift: bool,
    alt: bool,
    kitty_mode: bool,
) -> Option<Vec<u8>> {
    use winit::keyboard::NamedKey;
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
