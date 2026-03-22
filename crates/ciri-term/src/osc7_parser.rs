/// OSC 7 working directory parser.
/// Parses: ESC ] 7 ; file://hostname/path ST
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
        let mut i = 0;
        while i < data.len() {
            // Look for ESC ] 7 ;
            if i + 3 < data.len()
                && data[i] == 0x1b
                && data[i + 1] == b']'
                && data[i + 2] == b'7'
                && data[i + 3] == b';'
            {
                i += 4;
                let start = i;
                // Find ST (BEL or ESC \)
                while i < data.len() {
                    if data[i] == 0x07 {
                        // BEL terminator
                        self.parse_uri(&data[start..i]);
                        i += 1;
                        break;
                    }
                    if i + 1 < data.len() && data[i] == 0x1b && data[i + 1] == b'\\' {
                        // ESC \ terminator
                        self.parse_uri(&data[start..i]);
                        i += 2;
                        break;
                    }
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
    }

    fn parse_uri(&mut self, uri_bytes: &[u8]) {
        if let Ok(uri) = std::str::from_utf8(uri_bytes) {
            // file://hostname/path
            if let Some(path) = uri.strip_prefix("file://") {
                // Skip hostname — find the first '/' after the hostname
                if let Some(slash_pos) = path.find('/') {
                    let path = &path[slash_pos..];
                    let decoded = percent_decode(path);
                    self.current_cwd = Some(decoded);
                }
            }
        }
    }

    /// Return the most recently reported working directory.
    pub fn cwd(&self) -> Option<&str> {
        self.current_cwd.as_deref()
    }
}

/// Decode percent-encoded (%XX) sequences in a URI path.
fn percent_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""),
                16,
            ) {
                result.push(byte as char);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
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
