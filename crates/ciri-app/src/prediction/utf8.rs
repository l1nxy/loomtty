/// Incremental UTF-8 byte accumulator for decoding multi-byte characters
/// arriving split across input chunks.
pub(super) struct Utf8Accum {
    buf: [u8; 4],
    len: u8,
    expected: u8,
}

impl Utf8Accum {
    pub fn new() -> Self {
        Self {
            buf: [0; 4],
            len: 0,
            expected: 0,
        }
    }

    pub fn reset(&mut self) {
        self.len = 0;
        self.expected = 0;
    }

    /// Push a leading byte. Returns expected total byte count (2-4), or 0 if invalid.
    pub fn start(&mut self, byte: u8) -> u8 {
        self.len = 1;
        self.buf[0] = byte;
        self.expected = match byte {
            0xC2..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4,
            _ => {
                self.reset();
                return 0;
            }
        };
        // Reject overlong sequences: E0 must be followed by A0..BF,
        // F0 must be followed by 90..BF.
        self.expected
    }

    /// Push a continuation byte. Returns `Some(char)` when the codepoint is complete.
    pub fn push_cont(&mut self, byte: u8) -> Option<char> {
        if self.expected == 0 || (byte & 0xC0) != 0x80 {
            self.reset();
            return None;
        }
        // Overlong / surrogate checks on second byte.
        if self.len == 1 {
            match self.buf[0] {
                0xE0 if byte < 0xA0 => {
                    self.reset();
                    return None;
                }
                0xED if byte > 0x9F => {
                    self.reset();
                    return None;
                }
                0xF0 if byte < 0x90 => {
                    self.reset();
                    return None;
                }
                0xF4 if byte > 0x8F => {
                    self.reset();
                    return None;
                }
                _ => {}
            }
        }
        self.buf[self.len as usize] = byte;
        self.len += 1;
        if self.len == self.expected {
            let s = std::str::from_utf8(&self.buf[..self.len as usize]).ok()?;
            let ch = s.chars().next()?;
            self.reset();
            Some(ch)
        } else {
            None
        }
    }
}
