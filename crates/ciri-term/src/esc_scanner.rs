use winnow::Parser;
/// Shared escape sequence scanning utilities for terminal parsers.
///
/// All scanners operate on `&[u8]` data that may be split across PTY read
/// boundaries.  They report where incomplete sequences start so callers can
/// buffer the remainder for the next read.
use winnow::combinator::alt;
use winnow::error::ModalResult;
use winnow::token::{literal, take_while};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Result from scanning: found sequences and the byte offset where an
/// incomplete sequence starts (if any).
#[derive(Debug)]
pub(crate) struct ScanResult<'a> {
    /// Complete sequences found: (byte offset in original data, payload bytes).
    pub sequences: Vec<(usize, &'a [u8])>,
    /// If the data ends with an incomplete sequence, this is the byte offset
    /// where it starts.  The caller should buffer `data[partial_start..]` for
    /// the next read.
    pub partial_start: Option<usize>,
}

// ---------------------------------------------------------------------------
// String Terminator helper
// ---------------------------------------------------------------------------

/// Find a String Terminator (ST) starting from `start` in `data`.
///
/// ST is either BEL (0x07) or ESC \\ (0x1b 0x5c).
/// Returns `(payload_end, consumed_end)` where `payload_end` is the index of
/// the first ST byte (i.e. one past the last payload byte) and `consumed_end`
/// is the index one past the last ST byte.
fn find_st(data: &[u8], start: usize) -> Option<(usize, usize)> {
    let mut i = start;
    while i < data.len() {
        // C1 ST (0x9C) — single-byte string terminator.
        if data[i] == 0x9C {
            return Some((i, i + 1));
        }
        if data[i] == 0x07 {
            return Some((i, i + 1));
        }
        if data[i] == 0x1b {
            if i + 1 < data.len() {
                if data[i + 1] == b'\\' {
                    return Some((i, i + 2));
                }
                // ESC followed by something else — not ST, keep scanning.
            } else {
                // ESC at very end — could be start of ESC \, can't tell yet.
                return None;
            }
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// winnow parsers (used after we locate ESC and the introducer)
// ---------------------------------------------------------------------------

/// Match the OSC introducer `] <number> ;` at position `i+1` in `data`
/// (i.e. right after ESC). Returns the index right after `;` on success.
fn match_osc_intro(data: &[u8], esc_pos: usize, osc_number_bytes: &[u8]) -> Option<usize> {
    let mut pos = esc_pos + 1; // skip ESC
    if pos >= data.len() || data[pos] != b']' {
        return None;
    }
    pos += 1;
    // Match the number bytes.
    for &expected in osc_number_bytes {
        if pos >= data.len() || data[pos] != expected {
            return None;
        }
        pos += 1;
    }
    // Match `;`.
    if pos >= data.len() || data[pos] != b';' {
        return None;
    }
    pos += 1;
    Some(pos)
}

/// Parse DEC private mode params: digits and semicolons followed by `h` or `l`.
fn dec_params_and_terminator<'a>(input: &mut &'a [u8]) -> ModalResult<&'a [u8]> {
    let before = *input;
    let _params = take_while(1.., |b: u8| b.is_ascii_digit() || b == b';').parse_next(input)?;
    let _term = alt((literal(b"h".as_slice()), literal(b"l".as_slice()))).parse_next(input)?;
    let consumed_len = before.len() - input.len();
    Ok(&before[..consumed_len])
}

/// Parse DCS params: digits/semicolons then `q`.
fn dcs_introducer(input: &mut &[u8]) -> ModalResult<()> {
    take_while(0.., |b: u8| b.is_ascii_digit() || b == b';').parse_next(input)?;
    literal(b"q".as_slice()).parse_next(input)?;
    Ok(())
}

/// Parse Kitty graphics introducer: just `G`.
fn kitty_introducer(input: &mut &[u8]) -> ModalResult<()> {
    literal(b"G".as_slice()).parse_next(input)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Public scan functions
// ---------------------------------------------------------------------------

/// Scan for OSC sequences: `ESC ] <number> ; <payload> ST`
///
/// ST is either BEL (0x07) or ESC \\ (0x1b 0x5c).
/// Returns payloads (everything between the semicolon after the number and ST).
pub(crate) fn scan_osc<'a>(data: &'a [u8], osc_number_bytes: &[u8]) -> ScanResult<'a> {
    let mut result = ScanResult {
        sequences: Vec::new(),
        partial_start: None,
    };

    // intro_len: ] + number + ; (all after ESC)
    let intro_len = 1 + osc_number_bytes.len() + 1;

    let mut i = 0;
    while i < data.len() {
        if data[i] != 0x1b {
            i += 1;
            continue;
        }

        // Found ESC at position i.
        let remaining = data.len() - i;

        // Need at least ESC + intro_len bytes to even have a valid start.
        if remaining < 1 + intro_len {
            // Partial — not enough data to know if this is our sequence.
            result.partial_start = Some(i);
            break;
        }

        if let Some(payload_start) = match_osc_intro(data, i, osc_number_bytes) {
            match find_st(data, payload_start) {
                Some((payload_end, consumed_end)) => {
                    result
                        .sequences
                        .push((i, &data[payload_start..payload_end]));
                    i = consumed_end;
                }
                None => {
                    // Incomplete — ST not found.
                    result.partial_start = Some(i);
                    break;
                }
            }
        } else {
            i += 1;
        }
    }

    result
}

/// Scan for CSI DEC private mode sequences: `ESC [ ? <params> h/l`
///
/// Returns the full params+terminator as payload (e.g. `b"1004;2026h"`).
pub(crate) fn scan_csi_dec<'a>(data: &'a [u8]) -> ScanResult<'a> {
    let mut result = ScanResult {
        sequences: Vec::new(),
        partial_start: None,
    };

    let mut i = 0;
    while i < data.len() {
        if data[i] != 0x1b {
            i += 1;
            continue;
        }

        let remaining = data.len() - i;

        // Need ESC [ ? <at-least-one-digit> <h|l> = minimum 5 bytes
        if remaining < 5 {
            // Could be a partial CSI DEC sequence.
            // Check if what we have looks like the start of one.
            if remaining >= 2 && data[i + 1] == b'[' && remaining >= 3 && data[i + 2] == b'?' {
                result.partial_start = Some(i);
                break;
            }
            // Just a lone ESC at the end — might be partial for some sequence.
            if remaining == 1 {
                result.partial_start = Some(i);
                break;
            }
            i += 1;
            continue;
        }

        // Check for ESC [ ?
        if data[i + 1] == b'[' && data[i + 2] == b'?' {
            let mut slice = &data[i + 3..];
            match dec_params_and_terminator.parse_next(&mut slice) {
                Ok(payload) => {
                    let consumed_end = data.len() - slice.len();
                    result.sequences.push((i, payload));
                    i = consumed_end;
                }
                Err(_) => {
                    // Could be partial: digits but no h/l yet.
                    let rest = &data[i + 3..];
                    let all_params = rest.iter().all(|&b| b.is_ascii_digit() || b == b';');
                    if all_params && !rest.is_empty() {
                        result.partial_start = Some(i);
                        break;
                    }
                    i += 1;
                }
            }
        } else {
            i += 1;
        }
    }

    result
}

/// Scan for DCS sequences: `ESC P <params> q <payload> ST`
///
/// Returns the sixel data (everything between `q` and ST).
pub(crate) fn scan_dcs<'a>(data: &'a [u8]) -> ScanResult<'a> {
    let mut result = ScanResult {
        sequences: Vec::new(),
        partial_start: None,
    };

    let mut i = 0;
    while i < data.len() {
        if data[i] != 0x1b {
            i += 1;
            continue;
        }

        let remaining = data.len() - i;

        // Need ESC P ... q ... ST — minimum ESC P q ST(BEL) = 4 bytes
        if remaining < 4 {
            if remaining >= 2 && data[i + 1] == b'P' {
                result.partial_start = Some(i);
                break;
            }
            if remaining == 1 {
                result.partial_start = Some(i);
                break;
            }
            i += 1;
            continue;
        }

        if data[i + 1] == b'P' {
            let mut slice = &data[i + 2..];
            if dcs_introducer.parse_next(&mut slice).is_ok() {
                // slice now points right after 'q'
                let payload_start = data.len() - slice.len();
                match find_st(data, payload_start) {
                    Some((payload_end, consumed_end)) => {
                        result
                            .sequences
                            .push((i, &data[payload_start..payload_end]));
                        i = consumed_end;
                    }
                    None => {
                        result.partial_start = Some(i);
                        break;
                    }
                }
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    result
}

/// Scan for Kitty graphics APC sequences: `ESC _ G <payload> ESC \\`
///
/// Returns payload (everything between `G` and `ESC \\`).
///
/// Note: APC is terminated only by ST = ESC \\ (not BEL).  However, for
/// robustness we also accept BEL as a terminator.
pub(crate) fn scan_apc_kitty<'a>(data: &'a [u8]) -> ScanResult<'a> {
    let mut result = ScanResult {
        sequences: Vec::new(),
        partial_start: None,
    };

    let mut i = 0;
    while i < data.len() {
        if data[i] != 0x1b {
            i += 1;
            continue;
        }

        let remaining = data.len() - i;

        // Need ESC _ G ... ESC \ — minimum 6 bytes (ESC _ G x ESC \)
        if remaining < 5 {
            if remaining >= 2 && data[i + 1] == b'_' {
                result.partial_start = Some(i);
                break;
            }
            if remaining == 1 {
                result.partial_start = Some(i);
                break;
            }
            i += 1;
            continue;
        }

        if data[i + 1] == b'_' {
            let mut slice = &data[i + 2..];
            if kitty_introducer.parse_next(&mut slice).is_ok() {
                let payload_start = data.len() - slice.len();
                match find_st(data, payload_start) {
                    Some((payload_end, consumed_end)) => {
                        result
                            .sequences
                            .push((i, &data[payload_start..payload_end]));
                        i = consumed_end;
                    }
                    None => {
                        result.partial_start = Some(i);
                        break;
                    }
                }
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    result
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // scan_osc
    // -----------------------------------------------------------------------

    #[test]
    fn osc_complete_bel() {
        let data = b"\x1b]52;SGVsbG8=\x07";
        let r = scan_osc(data, b"52");
        assert_eq!(r.sequences.len(), 1);
        assert_eq!(r.sequences[0].1, b"SGVsbG8=");
        assert!(r.partial_start.is_none());
    }

    #[test]
    fn osc_complete_esc_backslash() {
        let data = b"\x1b]52;SGVsbG8=\x1b\\";
        let r = scan_osc(data, b"52");
        assert_eq!(r.sequences.len(), 1);
        assert_eq!(r.sequences[0].1, b"SGVsbG8=");
        assert!(r.partial_start.is_none());
    }

    #[test]
    fn osc_embedded_in_other_data() {
        let data = b"hello\x1b]7;file://host/tmp\x07world";
        let r = scan_osc(data, b"7");
        assert_eq!(r.sequences.len(), 1);
        assert_eq!(r.sequences[0].0, 5); // offset of ESC
        assert_eq!(r.sequences[0].1, b"file://host/tmp");
        assert!(r.partial_start.is_none());
    }

    #[test]
    fn osc_partial_no_st() {
        let data = b"text\x1b]52;partial-payload";
        let r = scan_osc(data, b"52");
        assert!(r.sequences.is_empty());
        assert_eq!(r.partial_start, Some(4));
    }

    #[test]
    fn osc_partial_just_esc() {
        let data = b"text\x1b";
        let r = scan_osc(data, b"52");
        assert!(r.sequences.is_empty());
        assert_eq!(r.partial_start, Some(4));
    }

    #[test]
    fn osc_multiple_sequences() {
        let data = b"\x1b]8;id=1;https://a.com\x07click\x1b]8;;\x07";
        let r = scan_osc(data, b"8");
        assert_eq!(r.sequences.len(), 2);
        assert_eq!(r.sequences[0].1, b"id=1;https://a.com");
        assert_eq!(r.sequences[1].1, b";");
        assert!(r.partial_start.is_none());
    }

    #[test]
    fn osc_wrong_number_ignored() {
        let data = b"\x1b]99;payload\x07";
        let r = scan_osc(data, b"52");
        assert!(r.sequences.is_empty());
        assert!(r.partial_start.is_none());
    }

    #[test]
    fn osc_partial_introducer() {
        // ESC ] 5 — only part of "52;" so we can't tell if it matches.
        let data = b"x\x1b]5";
        let r = scan_osc(data, b"52");
        assert!(r.sequences.is_empty());
        assert_eq!(r.partial_start, Some(1));
    }

    // -----------------------------------------------------------------------
    // scan_csi_dec
    // -----------------------------------------------------------------------

    #[test]
    fn csi_dec_set_single() {
        let data = b"\x1b[?1004h";
        let r = scan_csi_dec(data);
        assert_eq!(r.sequences.len(), 1);
        assert_eq!(r.sequences[0].1, b"1004h");
        assert!(r.partial_start.is_none());
    }

    #[test]
    fn csi_dec_reset_single() {
        let data = b"\x1b[?2026l";
        let r = scan_csi_dec(data);
        assert_eq!(r.sequences.len(), 1);
        assert_eq!(r.sequences[0].1, b"2026l");
    }

    #[test]
    fn csi_dec_combined_modes() {
        let data = b"\x1b[?1004;2026h";
        let r = scan_csi_dec(data);
        assert_eq!(r.sequences.len(), 1);
        assert_eq!(r.sequences[0].1, b"1004;2026h");
    }

    #[test]
    fn csi_dec_embedded() {
        let data = b"text\x1b[?1004hmore\x1b[?2026l";
        let r = scan_csi_dec(data);
        assert_eq!(r.sequences.len(), 2);
        assert_eq!(r.sequences[0].0, 4);
        assert_eq!(r.sequences[0].1, b"1004h");
        assert_eq!(r.sequences[1].1, b"2026l");
    }

    #[test]
    fn csi_dec_partial_no_terminator() {
        let data = b"\x1b[?1004";
        let r = scan_csi_dec(data);
        assert!(r.sequences.is_empty());
        assert_eq!(r.partial_start, Some(0));
    }

    #[test]
    fn csi_dec_partial_just_esc_bracket_question() {
        let data = b"x\x1b[?";
        let r = scan_csi_dec(data);
        assert!(r.sequences.is_empty());
        assert_eq!(r.partial_start, Some(1));
    }

    #[test]
    fn csi_dec_not_private_mode_ignored() {
        // Regular CSI (no ?) — should not match.
        let data = b"\x1b[1004h";
        let r = scan_csi_dec(data);
        assert!(r.sequences.is_empty());
    }

    // scan_dcs
    // -----------------------------------------------------------------------

    #[test]
    fn dcs_complete_bel() {
        let data = b"\x1bPq#0;2;0;0;0#1;2;100;100;0~--\x07";
        let r = scan_dcs(data);
        assert_eq!(r.sequences.len(), 1);
        assert_eq!(r.sequences[0].1, b"#0;2;0;0;0#1;2;100;100;0~--");
    }

    #[test]
    fn dcs_complete_esc_backslash() {
        let data = b"\x1bPq~data\x1b\\";
        let r = scan_dcs(data);
        assert_eq!(r.sequences.len(), 1);
        assert_eq!(r.sequences[0].1, b"~data");
    }

    #[test]
    fn dcs_with_params() {
        // DCS with numeric params before q: ESC P 0;1;0 q payload ST
        let data = b"\x1bP0;1;0q~row\x07";
        let r = scan_dcs(data);
        assert_eq!(r.sequences.len(), 1);
        assert_eq!(r.sequences[0].1, b"~row");
    }

    #[test]
    fn dcs_partial_no_st() {
        let data = b"\x1bPq~incomplete";
        let r = scan_dcs(data);
        assert!(r.sequences.is_empty());
        assert_eq!(r.partial_start, Some(0));
    }

    #[test]
    fn dcs_multiple() {
        let data = b"\x1bPq~a\x07\x1bPq~b\x1b\\";
        let r = scan_dcs(data);
        assert_eq!(r.sequences.len(), 2);
        assert_eq!(r.sequences[0].1, b"~a");
        assert_eq!(r.sequences[1].1, b"~b");
    }

    #[test]
    fn dcs_embedded() {
        let data = b"junk\x1bPq~pixels\x07more";
        let r = scan_dcs(data);
        assert_eq!(r.sequences.len(), 1);
        assert_eq!(r.sequences[0].0, 4);
        assert_eq!(r.sequences[0].1, b"~pixels");
    }

    // -----------------------------------------------------------------------
    // scan_apc_kitty
    // -----------------------------------------------------------------------

    #[test]
    fn apc_kitty_complete() {
        let data = b"\x1b_Ga=t,f=100;base64data\x1b\\";
        let r = scan_apc_kitty(data);
        assert_eq!(r.sequences.len(), 1);
        assert_eq!(r.sequences[0].1, b"a=t,f=100;base64data");
    }

    #[test]
    fn apc_kitty_bel_terminator() {
        let data = b"\x1b_Gpayload\x07";
        let r = scan_apc_kitty(data);
        assert_eq!(r.sequences.len(), 1);
        assert_eq!(r.sequences[0].1, b"payload");
    }

    #[test]
    fn apc_kitty_embedded() {
        let data = b"pre\x1b_Gstuff\x1b\\post";
        let r = scan_apc_kitty(data);
        assert_eq!(r.sequences.len(), 1);
        assert_eq!(r.sequences[0].0, 3);
        assert_eq!(r.sequences[0].1, b"stuff");
    }

    #[test]
    fn apc_kitty_partial() {
        let data = b"\x1b_Gincomplete";
        let r = scan_apc_kitty(data);
        assert!(r.sequences.is_empty());
        assert_eq!(r.partial_start, Some(0));
    }

    #[test]
    fn apc_kitty_multiple() {
        let data = b"\x1b_Ga=t;AAA\x1b\\\x1b_Ga=t;BBB\x1b\\";
        let r = scan_apc_kitty(data);
        assert_eq!(r.sequences.len(), 2);
        assert_eq!(r.sequences[0].1, b"a=t;AAA");
        assert_eq!(r.sequences[1].1, b"a=t;BBB");
    }

    #[test]
    fn apc_kitty_partial_esc_underscore() {
        let data = b"x\x1b_";
        let r = scan_apc_kitty(data);
        assert!(r.sequences.is_empty());
        assert_eq!(r.partial_start, Some(1));
    }

    // -----------------------------------------------------------------------
    // find_st helper
    // -----------------------------------------------------------------------

    #[test]
    fn find_st_bel() {
        let data = b"payload\x07rest";
        let (pe, ce) = find_st(data, 0).unwrap();
        assert_eq!(pe, 7);
        assert_eq!(ce, 8);
    }

    #[test]
    fn find_st_esc_backslash() {
        let data = b"payload\x1b\\rest";
        let (pe, ce) = find_st(data, 0).unwrap();
        assert_eq!(pe, 7);
        assert_eq!(ce, 9);
    }

    #[test]
    fn find_st_none() {
        let data = b"no terminator here";
        assert!(find_st(data, 0).is_none());
    }

    #[test]
    fn find_st_esc_at_end() {
        // ESC at very end — ambiguous, could be start of ESC \.
        let data = b"payload\x1b";
        assert!(find_st(data, 0).is_none());
    }

    // -----------------------------------------------------------------------
    // Edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn empty_input() {
        let data = b"";
        assert!(scan_osc(data, b"52").sequences.is_empty());
        assert!(scan_csi_dec(data).sequences.is_empty());
        assert!(scan_dcs(data).sequences.is_empty());
        assert!(scan_apc_kitty(data).sequences.is_empty());
    }

    #[test]
    fn no_sequences_in_plain_text() {
        let data = b"Hello, this is plain terminal output with no escapes.";
        let r = scan_osc(data, b"52");
        assert!(r.sequences.is_empty());
        assert!(r.partial_start.is_none());
    }

    #[test]
    fn lone_esc_at_end() {
        let data = b"text\x1b";
        // All scanners should treat a trailing ESC as potentially partial.
        assert_eq!(scan_osc(data, b"7").partial_start, Some(4));
        assert_eq!(scan_csi_dec(data).partial_start, Some(4));
        assert_eq!(scan_dcs(data).partial_start, Some(4));
        assert_eq!(scan_apc_kitty(data).partial_start, Some(4));
    }
}
