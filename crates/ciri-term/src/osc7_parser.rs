/// OSC 7 working directory parser.
/// Parses: ESC ] 7 ; file://hostname/path ST
use percent_encoding::percent_decode_str;
use winnow::prelude::*;
use winnow::token::{literal, take_until};

use crate::esc_scanner;

#[derive(Debug, Default)]
pub struct Osc7Parser {
    current_cwd: Option<String>,
}

impl Osc7Parser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Scan raw PTY output for OSC 7 sequences.
    pub fn scan(&mut self, data: &[u8]) {
        let result = esc_scanner::scan_osc(data, b"7");
        for (_offset, payload) in &result.sequences {
            self.parse_uri(payload);
        }
    }

    fn parse_uri(&mut self, uri_bytes: &[u8]) {
        if let Ok(uri) = std::str::from_utf8(uri_bytes) {
            if let Some(path) = parse_file_uri.parse(uri).ok() {
                let decoded = percent_decode_str(path)
                    .decode_utf8()
                    .map(|c| c.into_owned())
                    .unwrap_or_else(|_| path.to_string());
                self.current_cwd = Some(decoded);
            }
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
    let _hostname = take_until(1.., "/").parse_next(input)?;
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
}
