/// OSC 7 working directory parser.
/// Parses: ESC ] 7 ; file://hostname/path ST
use percent_encoding::percent_decode_str;
use winnow::prelude::*;
use winnow::token::{literal, take_until};

use crate::esc_scanner;
use crate::partial_buf::PartialBuf;

pub struct Osc7Parser {
    current_cwd: Option<String>,
    partial: PartialBuf,
}

impl Default for Osc7Parser {
    fn default() -> Self {
        Self::new()
    }
}

impl Osc7Parser {
    pub fn new() -> Self {
        Self {
            current_cwd: None,
            partial: PartialBuf::new(4096, "OSC 7"),
        }
    }

    /// Scan raw PTY output for OSC 7 sequences.
    pub fn scan(&mut self, data: &[u8]) {
        let mut tmp = Vec::new();
        let data = self.partial.prepend_to(data, &mut tmp);

        let result = esc_scanner::scan_osc(data, b"7");
        for (_offset, payload) in &result.sequences {
            self.parse_uri(payload);
        }

        if let Some(partial_start) = result.partial_start {
            self.partial.store(&data[partial_start..]);
        }
    }

    fn parse_uri(&mut self, uri_bytes: &[u8]) {
        if let Ok(uri) = std::str::from_utf8(uri_bytes)
            && let Ok((hostname, path)) = parse_file_uri.parse(uri)
        {
            // Security: only accept local hostnames to prevent a remote SSH
            // session from setting the CWD to an attacker-controlled path.
            if !is_local_hostname(hostname) {
                log::warn!("OSC 7: rejecting non-local hostname: {hostname}");
                return;
            }
            #[allow(unused_mut)]
            let mut decoded = percent_decode_str(path)
                .decode_utf8()
                .map(|c| c.into_owned())
                .unwrap_or_else(|_| path.to_string());
            // A Windows drive file URI is `file:///C:/path`, whose path
            // component is `/C:/path`. Strip the leading slash so the stored
            // cwd is a usable drive-qualified path (`C:/path`) — `Path::new`
            // treats `/C:/path` as drive-relative and fails to resolve it, so
            // pane cwd inheritance and session restore would silently fall
            // back. Gated to Windows so Unix `/home/...` paths are untouched.
            #[cfg(windows)]
            if is_drive_uri_path(&decoded) {
                decoded.remove(0);
            }
            self.current_cwd = Some(decoded);
        }
    }

    /// Return the most recently reported working directory.
    pub fn cwd(&self) -> Option<&str> {
        self.current_cwd.as_deref()
    }
}

/// Parse `file://hostname/path` and return `(hostname, path)`.
fn parse_file_uri<'a>(input: &mut &'a str) -> ModalResult<(&'a str, &'a str)> {
    literal("file://").parse_next(input)?;
    // hostname: everything up to the first '/'
    let hostname = take_until(0.., "/").parse_next(input)?;
    // The rest is the path (including leading '/')
    let path = *input;
    *input = "";
    Ok((hostname, path))
}

/// True for an OSC 7 path of the form `/C:/…` (leading slash + drive letter +
/// colon), i.e. a Windows drive path that still carries the URI's leading slash.
#[cfg(windows)]
fn is_drive_uri_path(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':'
}

/// Check if a hostname refers to the local machine.
/// Empty hostname and "localhost" are always local (per RFC 8089).
/// Otherwise, compare against the system's DNS hostname via `gethostname`.
fn is_local_hostname(hostname: &str) -> bool {
    if hostname.is_empty() || hostname.eq_ignore_ascii_case("localhost") {
        return true;
    }
    let local = gethostname::gethostname();
    local
        .to_str()
        .is_some_and(|l| l.eq_ignore_ascii_case(hostname))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_osc7_esc_backslash() {
        let mut parser = Osc7Parser::new();
        let data = b"\x1b]7;file://localhost/home/user/projects\x1b\\";
        parser.scan(data);
        assert_eq!(parser.cwd(), Some("/home/user/projects"));
    }

    #[test]
    fn parse_osc7_empty_hostname() {
        let mut parser = Osc7Parser::new();
        let data = b"\x1b]7;file:///home/user/projects\x1b\\";
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
        let data = b"\x1b]7;file://localhost/home/user/my%20dir\x1b\\";
        parser.scan(data);
        assert_eq!(parser.cwd(), Some("/home/user/my dir"));
    }

    #[test]
    fn parse_osc7_updates_cwd() {
        let mut parser = Osc7Parser::new();
        parser.scan(b"\x1b]7;file:///first\x1b\\");
        assert_eq!(parser.cwd(), Some("/first"));
        parser.scan(b"\x1b]7;file:///second\x1b\\");
        assert_eq!(parser.cwd(), Some("/second"));
    }

    #[test]
    fn parse_osc7_rejects_non_local_hostname() {
        let mut parser = Osc7Parser::new();
        let data = b"\x1b]7;file://attacker.com/malicious/path\x1b\\";
        parser.scan(data);
        assert_eq!(parser.cwd(), None);
    }

    #[cfg(windows)]
    #[test]
    fn parse_osc7_windows_drive_path_strips_leading_slash() {
        // What loom.ps1 emits for cwd C:\Users\me: file://HOST/C%3A/Users/me.
        // The stored cwd must be the drive-qualified `C:/Users/me`, not
        // `/C:/Users/me` (which Path::new can't resolve on Windows).
        let mut parser = Osc7Parser::new();
        let data = b"\x1b]7;file://localhost/C%3A/Users/me\x1b\\";
        parser.scan(data);
        assert_eq!(parser.cwd(), Some("C:/Users/me"));
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
        parser.scan(b"prefix\x1b]7;file://localhost/home/use");
        assert_eq!(parser.cwd(), None);
        parser.scan(b"r/project\x07suffix");
        assert_eq!(parser.cwd(), Some("/home/user/project"));
    }

    #[test]
    fn oversized_partial_is_discarded() {
        let mut parser = Osc7Parser::new();
        let mut data = vec![0x1b, b']', b'7', b';'];
        data.extend(std::iter::repeat_n(b'a', 4097)); // exceeds 4096 limit
        parser.scan(&data);
        assert!(parser.partial.is_empty());
        assert_eq!(parser.cwd(), None);
    }
}
