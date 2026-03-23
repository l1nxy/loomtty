/// Parser for OSC 8 hyperlink sequences.
///
/// OSC 8 format:
///   `ESC ] 8 ; params ; URI ST`  — start hyperlink
///   `ESC ] 8 ; ; ST`             — end hyperlink
///
/// Where ST is either BEL (0x07) or `ESC \` (0x1b 0x5c).
///
/// Tracks current hyperlink state and assigns link IDs.
///
/// Uses `crate::esc_scanner::scan_osc` for sequence detection and winnow for
/// payload parsing.
use winnow::token::{rest, take_till};
use winnow::Parser;

use crate::esc_scanner;

pub(crate) struct Osc8Parser {
    /// Currently active hyperlink URI (None if not in a hyperlink).
    current_uri: Option<String>,
    /// Current link ID assigned to the active hyperlink.
    current_link_id: Option<u16>,
    /// Next link ID to assign.
    next_link_id: u16,
    /// Link ID → URI mapping for the current pane content.
    link_map: Vec<(u16, String)>,
    /// Partial OSC sequence from previous read.
    partial: Vec<u8>,
}

const MAX_OSC8_PARTIAL_SIZE: usize = 8192;

/// Parse an OSC 8 payload into (params, uri).
///
/// The payload (as returned by `scan_osc`) is everything between `8;` and ST.
/// For a start hyperlink `ESC ] 8 ; id=foo ; https://example.com ST`, the
/// payload is `id=foo;https://example.com`.
/// For an end hyperlink `ESC ] 8 ; ; ST`, the payload is `;`.
///
/// We split at the first `;` to get params (before) and URI (after).
fn parse_payload<'a>(input: &mut &'a [u8]) -> winnow::error::ModalResult<(&'a [u8], &'a [u8])> {
    let params = take_till(0.., |b: u8| b == b';').parse_next(input)?;
    let _ = winnow::token::literal(b";".as_slice()).parse_next(input)?;
    let uri = rest.parse_next(input)?;
    Ok((params, uri))
}

impl Osc8Parser {
    pub fn new() -> Self {
        Self {
            current_uri: None,
            current_link_id: None,
            next_link_id: 1,
            link_map: Vec::new(),
            partial: Vec::new(),
        }
    }

    /// Whether we are currently inside a hyperlink region.
    pub fn in_hyperlink(&self) -> bool {
        self.current_uri.is_some()
    }

    /// The link ID of the current hyperlink (if any).
    pub fn current_link_id(&self) -> Option<u16> {
        self.current_link_id
    }

    /// Get the current link ID → URI mapping.
    pub fn link_map(&self) -> &[(u16, String)] {
        &self.link_map
    }

    /// Scan PTY output for OSC 8 sequences and update hyperlink state.
    pub fn scan(&mut self, data: &[u8]) {
        let working_data;
        let data = if !self.partial.is_empty() {
            self.partial.extend_from_slice(data);
            working_data = std::mem::take(&mut self.partial);
            &working_data[..]
        } else {
            data
        };

        let result = esc_scanner::scan_osc(data, b"8");

        for (_offset, payload) in &result.sequences {
            let mut input = *payload;
            match parse_payload.parse_next(&mut input) {
                Ok((_params, uri_bytes)) => {
                    let uri = std::str::from_utf8(uri_bytes).unwrap_or("").to_string();
                    if uri.is_empty() {
                        // End hyperlink: ESC ] 8 ; ; ST
                        self.current_uri = None;
                        self.current_link_id = None;
                        log::debug!("OSC 8: hyperlink end");
                    } else {
                        // Start hyperlink: ESC ] 8 ; params ; URI ST
                        let link_id = self.next_link_id;
                        self.next_link_id = self.next_link_id.wrapping_add(1);
                        if self.next_link_id == 0 {
                            self.next_link_id = 1;
                        }
                        self.link_map.push((link_id, uri.clone()));
                        self.current_uri = Some(uri.clone());
                        self.current_link_id = Some(link_id);
                        log::debug!("OSC 8: hyperlink start id={link_id} uri={uri}");
                    }
                }
                Err(_) => {
                    // Malformed payload — treat as end hyperlink.
                    self.current_uri = None;
                    self.current_link_id = None;
                }
            }
        }

        // Buffer partial data for next read.
        if let Some(partial_start) = result.partial_start {
            self.buffer_partial(&data[partial_start..]);
        }
    }

    fn buffer_partial(&mut self, data: &[u8]) {
        if data.len() > MAX_OSC8_PARTIAL_SIZE {
            log::warn!("OSC 8 partial buffer exceeded limit, discarding");
            self.partial.clear();
        } else {
            self.partial = data.to_vec();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_and_end_hyperlink() {
        let mut parser = Osc8Parser::new();

        // Start hyperlink
        parser.scan(b"\x1b]8;;https://example.com\x1b\\");
        assert!(parser.in_hyperlink());
        assert!(parser.current_link_id().is_some());
        assert_eq!(parser.link_map().len(), 1);
        assert_eq!(parser.link_map()[0].1, "https://example.com");

        // End hyperlink
        parser.scan(b"\x1b]8;;\x1b\\");
        assert!(!parser.in_hyperlink());
        assert!(parser.current_link_id().is_none());
    }

    #[test]
    fn hyperlink_with_bel_terminator() {
        let mut parser = Osc8Parser::new();
        parser.scan(b"\x1b]8;;https://example.com\x07");
        assert!(parser.in_hyperlink());
        assert_eq!(parser.link_map()[0].1, "https://example.com");
    }

    #[test]
    fn hyperlink_with_params() {
        let mut parser = Osc8Parser::new();
        // OSC 8 with id parameter: ESC ] 8 ; id=foo ; URI ST
        parser.scan(b"\x1b]8;id=foo;https://example.com\x1b\\");
        assert!(parser.in_hyperlink());
        assert_eq!(parser.link_map()[0].1, "https://example.com");
    }

    #[test]
    fn multiple_hyperlinks() {
        let mut parser = Osc8Parser::new();
        parser.scan(b"\x1b]8;;https://a.com\x07text\x1b]8;;\x07\x1b]8;;https://b.com\x07text\x1b]8;;\x07");
        assert!(!parser.in_hyperlink());
        assert_eq!(parser.link_map().len(), 2);
        assert_eq!(parser.link_map()[0].1, "https://a.com");
        assert_eq!(parser.link_map()[1].1, "https://b.com");
    }

    #[test]
    fn partial_sequence() {
        let mut parser = Osc8Parser::new();

        // First read: incomplete
        parser.scan(b"\x1b]8;;https://exam");
        assert!(!parser.in_hyperlink());
        assert!(!parser.partial.is_empty());

        // Second read: completes
        parser.scan(b"ple.com\x1b\\");
        assert!(parser.in_hyperlink());
        assert_eq!(parser.link_map()[0].1, "https://example.com");
    }

    #[test]
    fn mixed_with_normal_text() {
        let mut parser = Osc8Parser::new();
        parser.scan(b"normal text\x1b]8;;https://example.com\x07link text\x1b]8;;\x07more text");
        assert!(!parser.in_hyperlink());
        assert_eq!(parser.link_map().len(), 1);
    }
}
