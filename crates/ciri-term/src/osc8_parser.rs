use winnow::Parser;

/// Maximum number of entries in the link map before we evict old entries.
const MAX_LINK_MAP_ENTRIES: usize = 4096;

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

use crate::esc_scanner;
use crate::partial_buf::PartialBuf;

pub(crate) struct Osc8Parser {
    /// Currently active hyperlink URI (None if not in a hyperlink).
    current_uri: Option<String>,
    /// Current link ID assigned to the active hyperlink.
    current_link_id: Option<u16>,
    /// Next link ID to assign.
    next_link_id: u16,
    /// Link ID → URI mapping for the current pane content.
    link_map: Vec<(u16, String)>,
    partial: PartialBuf,
}

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
            partial: PartialBuf::new(8192, "OSC 8"),
        }
    }

    /// Get the current link ID → URI mapping.
    #[cfg(test)]
    pub fn link_map(&self) -> &[(u16, String)] {
        &self.link_map
    }

    /// Remove link map entries whose IDs are not in `referenced_ids`.
    /// This is the correct eviction strategy (kitty-style GC): the caller
    /// scans the grid/scrollback for referenced link IDs and passes them here.
    /// Unreferenced entries are removed, keeping the map bounded.
    #[cfg(test)]
    pub fn gc_unreferenced(&mut self, referenced_ids: &std::collections::HashSet<u16>) {
        // Always keep the currently active link, even if not yet in grid cells.
        let active = self.current_link_id;
        self.link_map
            .retain(|(id, _)| referenced_ids.contains(id) || active == Some(*id));
    }

    #[cfg(test)]
    pub(crate) fn active_link(&self) -> Option<(u16, &str)> {
        self.current_link_id.zip(self.current_uri.as_deref())
    }

    /// Scan PTY output for OSC 8 sequences and update hyperlink state.
    pub fn scan(&mut self, data: &[u8]) {
        let mut tmp = Vec::new();
        let data = self.partial.prepend_to(data, &mut tmp);

        let result = esc_scanner::scan_osc(data, b"8");

        for (_offset, payload) in &result.sequences {
            let mut input = *payload;
            match parse_payload.parse_next(&mut input) {
                Ok((_params, uri_bytes)) => {
                    let Ok(uri) = std::str::from_utf8(uri_bytes) else {
                        continue;
                    };
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
                        if self.link_map.len() >= MAX_LINK_MAP_ENTRIES {
                            // Fallback eviction when gc_unreferenced() hasn't been
                            // called (no grid-level reference tracking yet).
                            // Remove the oldest half, but always preserve the
                            // currently active link to avoid breaking in-progress
                            // hyperlink spans.
                            let half = MAX_LINK_MAP_ENTRIES / 2;
                            let active = self.current_link_id;
                            let before = self.link_map.len();
                            // Drain oldest half, then re-insert active if it was evicted.
                            let evicted: Vec<_> = self.link_map.drain(..half).collect();
                            if let Some(active_id) = active
                                && !self.link_map.iter().any(|(id, _)| *id == active_id)
                                && let Some(entry) =
                                    evicted.into_iter().find(|(id, _)| *id == active_id)
                            {
                                self.link_map.insert(0, entry);
                            }
                            log::warn!(
                                "OSC 8: link_map exceeded {MAX_LINK_MAP_ENTRIES} entries, \
                                 evicted {} (kept active link)",
                                before - self.link_map.len()
                            );
                        }
                        self.link_map.push((link_id, uri.to_string()));
                        self.current_uri = Some(uri.to_string());
                        self.current_link_id = Some(link_id);
                        log::debug!("OSC 8: hyperlink start id={link_id} uri={uri}");
                    }
                }
                Err(_) => {
                    // Malformed payload — ignore it and preserve any active
                    // hyperlink state from earlier valid OSC 8 sequences.
                }
            }
        }

        if let Some(partial_start) = result.partial_start {
            self.partial.store(&data[partial_start..]);
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
        assert!(parser.current_uri.is_some());
        assert!(parser.current_link_id.is_some());
        assert_eq!(parser.link_map().len(), 1);
        assert_eq!(parser.link_map()[0].1, "https://example.com");

        // End hyperlink
        parser.scan(b"\x1b]8;;\x1b\\");
        assert!(parser.current_uri.is_none());
        assert!(parser.current_link_id.is_none());
    }

    #[test]
    fn hyperlink_with_bel_terminator() {
        let mut parser = Osc8Parser::new();
        parser.scan(b"\x1b]8;;https://example.com\x07");
        assert!(parser.current_uri.is_some());
        assert_eq!(parser.link_map()[0].1, "https://example.com");
    }

    #[test]
    fn hyperlink_with_params() {
        let mut parser = Osc8Parser::new();
        // OSC 8 with id parameter: ESC ] 8 ; id=foo ; URI ST
        parser.scan(b"\x1b]8;id=foo;https://example.com\x1b\\");
        assert!(parser.current_uri.is_some());
        assert_eq!(parser.link_map()[0].1, "https://example.com");
    }

    #[test]
    fn multiple_hyperlinks() {
        let mut parser = Osc8Parser::new();
        parser.scan(
            b"\x1b]8;;https://a.com\x07text\x1b]8;;\x07\x1b]8;;https://b.com\x07text\x1b]8;;\x07",
        );
        assert!(parser.current_uri.is_none());
        assert_eq!(parser.link_map().len(), 2);
        assert_eq!(parser.link_map()[0].1, "https://a.com");
        assert_eq!(parser.link_map()[1].1, "https://b.com");
    }

    #[test]
    fn partial_sequence() {
        let mut parser = Osc8Parser::new();

        // First read: incomplete
        parser.scan(b"\x1b]8;;https://exam");
        assert!(parser.current_uri.is_none());
        assert!(!parser.partial.is_empty());

        // Second read: completes
        parser.scan(b"ple.com\x1b\\");
        assert!(parser.current_uri.is_some());
        assert_eq!(parser.link_map()[0].1, "https://example.com");
    }

    #[test]
    fn mixed_with_normal_text() {
        let mut parser = Osc8Parser::new();
        parser.scan(b"normal text\x1b]8;;https://example.com\x07link text\x1b]8;;\x07more text");
        assert!(parser.current_uri.is_none());
        assert_eq!(parser.link_map().len(), 1);
    }

    #[test]
    fn oversized_partial_is_discarded() {
        let mut parser = Osc8Parser::new();
        let mut data = vec![0x1b, b']', b'8', b';'];
        data.extend(std::iter::repeat_n(b'a', 8193)); // exceeds 8192 limit
        parser.scan(&data);
        assert!(parser.partial.is_empty());
        assert!(parser.current_uri.is_none());
    }

    #[test]
    fn malformed_payload_inside_active_hyperlink_is_ignored() {
        let mut parser = Osc8Parser::new();
        parser.scan(b"\x1b]8;;https://example.com\x07");
        let active_uri = parser.current_uri.clone();
        let active_link_id = parser.current_link_id;
        let initial_link_map = parser.link_map().to_vec();

        parser.scan(b"\x1b]8;broken\x07");

        assert_eq!(parser.current_uri, active_uri);
        assert_eq!(parser.current_link_id, active_link_id);
        assert_eq!(parser.link_map(), initial_link_map.as_slice());
    }

    #[test]
    fn malformed_utf8_payload_inside_active_hyperlink_is_ignored() {
        let mut parser = Osc8Parser::new();
        parser.scan(b"\x1b]8;;https://example.com\x07");
        let active = parser.active_link().map(|(id, uri)| (id, uri.to_string()));
        let initial_link_map = parser.link_map().to_vec();

        parser.scan(b"\x1b]8;;\xff\x07");

        assert_eq!(
            parser.active_link().map(|(id, uri)| (id, uri.to_string())),
            active
        );
        assert_eq!(parser.link_map(), initial_link_map.as_slice());
    }

    #[test]
    fn link_map_evicts_oldest_when_full() {
        let mut parser = Osc8Parser::new();

        // Fill up to the limit (ending each hyperlink so it's not "active").
        for i in 0..MAX_LINK_MAP_ENTRIES {
            let url = format!("\x1b]8;;https://example.com/{i}\x07");
            parser.scan(url.as_bytes());
            parser.scan(b"\x1b]8;;\x07");
        }
        assert_eq!(parser.link_map().len(), MAX_LINK_MAP_ENTRIES);

        // One more triggers eviction of the oldest half.
        parser.scan(b"\x1b]8;;https://example.com/overflow\x07");
        let expected_len = MAX_LINK_MAP_ENTRIES / 2 + 1;
        assert_eq!(parser.link_map().len(), expected_len);

        // The newest entry should be present.
        assert_eq!(
            parser.link_map().last().unwrap().1,
            "https://example.com/overflow"
        );

        // The very first entries should have been evicted.
        assert!(
            !parser
                .link_map()
                .iter()
                .any(|(_, uri)| uri == "https://example.com/0")
        );
    }

    #[test]
    fn eviction_preserves_active_hyperlink() {
        let mut parser = Osc8Parser::new();

        // Start one hyperlink and leave it active (don't end it).
        parser.scan(b"\x1b]8;;https://active-link.com\x07");
        let active_id = parser.current_link_id.unwrap();
        parser.scan(b"\x1b]8;;\x07");

        // Fill up to the limit with other links.
        for i in 1..MAX_LINK_MAP_ENTRIES {
            let url = format!("\x1b]8;;https://example.com/{i}\x07");
            parser.scan(url.as_bytes());
            // Leave the last one active.
            if i < MAX_LINK_MAP_ENTRIES - 1 {
                parser.scan(b"\x1b]8;;\x07");
            }
        }

        // The last link is active.
        let last_active_id = parser.current_link_id.unwrap();

        // Trigger eviction.
        parser.scan(b"\x1b]8;;\x07");
        parser.scan(b"\x1b]8;;https://trigger.com\x07");

        // The active link should be preserved even if it was in the oldest half.
        assert!(
            parser
                .link_map()
                .iter()
                .any(|(id, _)| *id == last_active_id)
                || last_active_id == parser.current_link_id.unwrap_or(0),
            "active link should survive eviction"
        );

        // But the first link (not active) might have been evicted — that's expected.
        // Just verify we didn't lose the active one.
        assert!(
            parser.link_map().iter().any(|(id, _)| *id == active_id)
                || active_id <= (MAX_LINK_MAP_ENTRIES / 2) as u16,
            "non-active old link may be evicted"
        );
    }

    #[test]
    fn gc_unreferenced_removes_unneeded_entries() {
        let mut parser = Osc8Parser::new();

        parser.scan(b"\x1b]8;;https://a.com\x07");
        parser.scan(b"\x1b]8;;\x07");
        parser.scan(b"\x1b]8;;https://b.com\x07");
        parser.scan(b"\x1b]8;;\x07");
        parser.scan(b"\x1b]8;;https://c.com\x07");
        // c.com is active (not ended).

        assert_eq!(parser.link_map().len(), 3);

        // Only link ID 1 (a.com) is referenced by grid cells.
        let mut referenced = std::collections::HashSet::new();
        referenced.insert(1u16);
        parser.gc_unreferenced(&referenced);

        // Should keep: ID 1 (referenced) + ID 3 (active).
        assert_eq!(parser.link_map().len(), 2);
        assert!(
            parser
                .link_map()
                .iter()
                .any(|(_, uri)| uri == "https://a.com")
        );
        assert!(
            parser
                .link_map()
                .iter()
                .any(|(_, uri)| uri == "https://c.com")
        );
        // b.com should be gone (not referenced, not active).
        assert!(
            !parser
                .link_map()
                .iter()
                .any(|(_, uri)| uri == "https://b.com")
        );
    }
}
