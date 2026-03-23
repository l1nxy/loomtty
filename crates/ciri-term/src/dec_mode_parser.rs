/// Parser for DEC private mode set/reset sequences (CSI ? N h / CSI ? N l).
///
/// Detects modes that alacritty_terminal doesn't expose (e.g. 1004, 2026).
/// Designed for use in the PTY output scanning pipeline alongside Osc133Parser.
use winnow::combinator::separated;
use winnow::prelude::*;
use winnow::token::take_while;

use crate::esc_scanner::scan_csi_dec;

pub(crate) struct DecModeParser {
    /// Tracking state for focus event reporting (DECSET 1004).
    pub focus_event_mode: bool,
    /// Tracking state for synchronized output (DEC 2026).
    pub sync_output_mode: bool,
}

/// Parse a single decimal number from ASCII digits.
fn parse_mode_number(input: &mut &[u8]) -> ModalResult<u32> {
    let digits = take_while(1.., |b: u8| b.is_ascii_digit()).parse_next(input)?;
    let mut n: u32 = 0;
    for &b in digits {
        n = n
            .checked_mul(10)
            .and_then(|n| n.checked_add((b - b'0') as u32))
            .ok_or_else(|| {
                winnow::error::ErrMode::Backtrack(winnow::error::ContextError::new())
            })?;
    }
    Ok(n)
}

/// Parse a semicolon-separated list of mode numbers from a payload slice.
fn parse_mode_numbers(input: &mut &[u8]) -> ModalResult<Vec<u32>> {
    separated(1.., parse_mode_number, b";").parse_next(input)
}

impl DecModeParser {
    pub fn new() -> Self {
        Self {
            focus_event_mode: false,
            sync_output_mode: false,
        }
    }

    /// Scan PTY output data for DEC private mode set/reset sequences.
    ///
    /// Detects `CSI ? <mode> h` (set) and `CSI ? <mode> l` (reset) for:
    /// - Mode 1004: Focus event reporting
    /// - Mode 2026: Synchronized output
    pub fn scan(&mut self, data: &[u8]) {
        let result = scan_csi_dec(data);

        for (_offset, payload) in &result.sequences {
            if payload.is_empty() {
                continue;
            }

            // Last byte is 'h' or 'l', everything before is the params.
            let (&terminator, params) = payload.split_last().unwrap();
            let set = terminator == b'h';

            // Parse mode numbers from the params using winnow.
            let mut input = params;
            if let Ok(modes) = parse_mode_numbers.parse_next(&mut input) {
                for mode in modes {
                    match mode {
                        1004 => {
                            self.focus_event_mode = set;
                            log::debug!("DECSET 1004 focus events: {}", set);
                        }
                        2026 => {
                            self.sync_output_mode = set;
                            log::debug!("DEC 2026 sync output: {}", set);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_focus_event_set() {
        let mut parser = DecModeParser::new();
        parser.scan(b"\x1b[?1004h");
        assert!(parser.focus_event_mode);
    }

    #[test]
    fn detect_focus_event_reset() {
        let mut parser = DecModeParser::new();
        parser.focus_event_mode = true;
        parser.scan(b"\x1b[?1004l");
        assert!(!parser.focus_event_mode);
    }

    #[test]
    fn detect_sync_output_set() {
        let mut parser = DecModeParser::new();
        parser.scan(b"\x1b[?2026h");
        assert!(parser.sync_output_mode);
    }

    #[test]
    fn detect_sync_output_reset() {
        let mut parser = DecModeParser::new();
        parser.sync_output_mode = true;
        parser.scan(b"\x1b[?2026l");
        assert!(!parser.sync_output_mode);
    }

    #[test]
    fn detect_combined_modes() {
        let mut parser = DecModeParser::new();
        // Some applications set multiple modes in one sequence
        parser.scan(b"\x1b[?1004;2026h");
        assert!(parser.focus_event_mode);
        assert!(parser.sync_output_mode);
    }

    #[test]
    fn mixed_data_detection() {
        let mut parser = DecModeParser::new();
        // Mode set embedded in other output
        let data = b"some text\x1b[?1004hmore text\x1b[?2026h";
        parser.scan(data);
        assert!(parser.focus_event_mode);
        assert!(parser.sync_output_mode);
    }

    #[test]
    fn ignore_non_dec_csi() {
        let mut parser = DecModeParser::new();
        // Regular CSI (no ?) should not trigger
        parser.scan(b"\x1b[1004h");
        assert!(!parser.focus_event_mode);
    }

    #[test]
    fn no_false_positives_on_partial() {
        let mut parser = DecModeParser::new();
        // Truncated sequence — just ignore, don't crash
        parser.scan(b"\x1b[?1004");
        assert!(!parser.focus_event_mode);
    }
}
