/// OSC 7 working directory parser.
/// Parses: ESC ] 7 ; file://hostname/path ST
use percent_encoding::percent_decode_str;
use winnow::prelude::*;
use winnow::token::{literal, take_until};

use crate::esc_scanner;

const MAX_OSC7_PARTIAL_SIZE: usize = 4096;

#[derive(Debug, Default)]
pub struct Osc7Parser {
    current_cwd: Option<String>,
    /// Partial OSC sequence from a previous read.
    partial: Vec<u8>,
}

impl Osc7Parser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Scan raw PTY output for OSC 7 sequences.
    pub fn scan(&mut self, data: &[u8]) {
        // If we have a partial OSC from a previous read, prepend it
        let working_data;
        let data = if !self.partial.is_empty() {
            self.partial.extend_from_slice(data);
            working_data = std::mem::take(&mut self.partial);
            &working_data[..]
        } else {
            data
        };

        let result = esc_scanner::scan_osc(data, b"7");
        for (_offset, payload) in &result.sequences {
            self.parse_uri(payload);
        }

        // Buffer partial data for next read.
        if let Some(partial_start) = result.partial_start {
            let partial = &data[partial_start..];
            if partial.len() > MAX_OSC7_PARTIAL_SIZE {
                log::warn!(
                    "OSC 7 partial buffer exceeded {}B limit, discarding",
                    MAX_OSC7_PARTIAL_SIZE
                );
                self.partial.clear();
            } else {
                self.partial = partial.to_vec();
            }
        }
    }

    fn parse_uri(&mut self, uri_bytes: &[u8]) {
        if let Ok(uri) = std::str::from_utf8(uri_bytes)
            && let Ok(path) = parse_file_uri.parse(uri)
        {
            let decoded = percent_decode_str(path)
                .decode_utf8()
                .map(|c| c.into_owned())
                .unwrap_or_else(|_| path.to_string());
            self.current_cwd = Some(decoded);
        }
    }

    /// Return the most recently reported working directory.
    pub fn cwd(&self) -> Option<&str> {
        self.current_cwd.as_deref()
    }
}

/// Parse `file://hostname/path` and return the `/path` portion (including leading slash).
fn parse_file_uri<'a>(input: &mut &'a str) -> ModalResult<&'a str> {
    literal("file://").parse_next(input)?;
    // hostname: everything up to the first '/'
    let _hostname = take_until(0.., "/").parse_next(input)?;
    // The rest is the path (including leading '/')
    let path = *input;
    *input = "";
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_osc7_esc_backslash() {
        let mut parser = Osc7Parser::new();
        let data = b"\x1b]7;file://myhost/home/user/projects\x1b\\";
        parser.scan(data);
        assert_eq!(parser.cwd(), Some("/home/user/projects"));
    }

    #[test]
    fn parse_osc7_bel() {
        let mut parser = Osc7Parser::new();
        let data = b"\x1b]7;file://localhost/tmp/test\x07";
        parser.scan(data);
        assert_eq!(parser.cwd(), Some("/tmp/test"));
    }

    #[test]
    fn parse_osc7_percent_encoded() {
        let mut parser = Osc7Parser::new();
        let data = b"\x1b]7;file://host/home/user/my%20dir\x1b\\";
        parser.scan(data);
        assert_eq!(parser.cwd(), Some("/home/user/my dir"));
    }

    #[test]
    fn parse_osc7_updates_cwd() {
        let mut parser = Osc7Parser::new();
        parser.scan(b"\x1b]7;file://h/first\x1b\\");
        assert_eq!(parser.cwd(), Some("/first"));
        parser.scan(b"\x1b]7;file://h/second\x1b\\");
        assert_eq!(parser.cwd(), Some("/second"));
    }

    #[test]
    fn no_osc7_returns_none() {
        let mut parser = Osc7Parser::new();
        parser.scan(b"hello world");
        assert_eq!(parser.cwd(), None);
    }

    #[test]
    fn fragmented_sequence_keeps_plain_text_and_updates_cwd() {
        let mut parser = Osc7Parser::new();
        parser.scan(b"prefix\x1b]7;file://host/home/use");
        assert_eq!(parser.cwd(), None);
        parser.scan(b"r/project\x07suffix");
        assert_eq!(parser.cwd(), Some("/home/user/project"));
    }

    #[test]
    fn oversized_partial_is_discarded() {
        let mut parser = Osc7Parser::new();
        let mut data = vec![0x1b, b']', b'7', b';'];
        data.extend(std::iter::repeat_n(b'a', MAX_OSC7_PARTIAL_SIZE + 1));
        parser.scan(&data);
        assert!(parser.partial.is_empty());
        assert_eq!(parser.cwd(), None);
    }
}
