//! Kitty keyboard protocol encoding (CSI u format).
//!
//! Progressive enhancement levels:
//!   1 (DISAMBIGUATE): CSI u encoding for ambiguous keys
//!   2 (REPORT_EVENTS): Include event type (press=1, repeat=2, release=3)
//!   3 (REPORT_ALTERNATES): Include shifted/base layout key codepoints
//!   4 (REPORT_ALL): ALL keys use CSI u, no legacy sequences
//!   5 (REPORT_TEXT): Include associated text as colon-separated codepoints

use winit::keyboard::{Key, KeyLocation, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

use super::{
    encode_kitty_legacy_text_bytes, encode_legacy_c0_key, key_event_base_char,
    key_event_text_for_input,
};

/// Encode a key event using the Kitty keyboard protocol.
///
/// Full format: `CSI unicode-key-code:shifted-key:base-layout-key ; modifiers:event-type ; text-as-codepoints u`
/// Modifier bits: shift=1, alt=2, ctrl=4, super=8, hyper=16, meta=32, caps_lock=64, num_lock=128
/// Transmitted modifier value = bits + 1 (1 = no modifiers).
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
    use loom_protocol::message::{
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
            key_event_text_for_input(event, shift).as_deref(),
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
                    let code = if event.location == KeyLocation::Right {
                        57447
                    } else {
                        57441
                    };
                    csi_u_full(code)
                } else {
                    vec![]
                }
            }
            NamedKey::Control => {
                if report_all {
                    let code = if event.location == KeyLocation::Right {
                        57448
                    } else {
                        57442
                    };
                    csi_u_full(code)
                } else {
                    vec![]
                }
            }
            NamedKey::Alt => {
                if report_all {
                    let code = if event.location == KeyLocation::Right {
                        57449
                    } else {
                        57443
                    };
                    csi_u_full(code)
                } else {
                    vec![]
                }
            }
            NamedKey::Super => {
                if report_all {
                    let code = if event.location == KeyLocation::Right {
                        57450
                    } else {
                        57444
                    };
                    csi_u_full(code)
                } else {
                    vec![]
                }
            }
            NamedKey::Hyper => {
                if report_all {
                    let code = if event.location == KeyLocation::Right {
                        57451
                    } else {
                        57445
                    };
                    csi_u_full(code)
                } else {
                    vec![]
                }
            }
            NamedKey::Meta => {
                if report_all {
                    let code = if event.location == KeyLocation::Right {
                        57452
                    } else {
                        57446
                    };
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
