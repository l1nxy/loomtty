/// Parser for DEC private mode set/reset sequences (CSI ? N h / CSI ? N l).
///
/// Detects modes that alacritty_terminal doesn't expose (e.g. 1004, 2026).
/// Designed for use in the PTY output scanning pipeline alongside Osc133Parser.
pub(crate) struct DecModeParser {
    /// Tracking state for focus event reporting (DECSET 1004).
    pub focus_event_mode: bool,
    /// Tracking state for synchronized output (DEC 2026).
    pub sync_output_mode: bool,
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
        // Look for ESC [ ? ... h/l sequences
        // Format: 0x1b 0x5b 0x3f <digits> 0x68(h) or 0x6c(l)
        let len = data.len();
        let mut i = 0;
        while i + 3 < len {
            // Match ESC [
            if data[i] == 0x1b && data[i + 1] == b'[' && data[i + 2] == b'?' {
                i += 3;
                // Parse mode numbers (may be semicolon-separated, e.g. CSI ? 1004;2026 h)
                let start = i;
                while i < len && (data[i].is_ascii_digit() || data[i] == b';') {
                    i += 1;
                }
                if i < len && (data[i] == b'h' || data[i] == b'l') {
                    let set = data[i] == b'h';
                    // Parse each mode number from the parameter string
                    let params = &data[start..i];
                    for param in params.split(|&b| b == b';') {
                        if let Some(mode) = parse_decimal(param) {
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
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
    }
}

/// Parse an ASCII decimal number from a byte slice.
fn parse_decimal(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() {
        return None;
    }
    let mut n: u32 = 0;
    for &b in bytes {
        if !b.is_ascii_digit() {
            return None;
        }
        n = n.checked_mul(10)?.checked_add((b - b'0') as u32)?;
    }
    Some(n)
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
