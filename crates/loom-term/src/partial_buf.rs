//! Reusable partial-buffer for escape sequence parsers.
//!
//! Terminal escape sequences frequently span PTY read boundaries. Every parser
//! needs the same logic: store a trailing fragment, prepend it to the next read,
//! and discard if the buffer grows too large. `PartialBuf` centralises this.

/// A bounded byte buffer for accumulating partial escape sequences across reads.
pub(crate) struct PartialBuf {
    buf: Vec<u8>,
    max_size: usize,
    label: &'static str,
}

impl PartialBuf {
    /// Create a new buffer with the given size limit and log label.
    pub fn new(max_size: usize, label: &'static str) -> Self {
        Self {
            buf: Vec::new(),
            max_size,
            label,
        }
    }

    /// Store a partial fragment. If it exceeds the size limit, log a warning
    /// and discard both the new data and any previously buffered data.
    pub fn store(&mut self, data: &[u8]) {
        if data.len() > self.max_size {
            log::warn!(
                "{} partial buffer exceeded {}B limit, discarding",
                self.label,
                self.max_size,
            );
            self.buf.clear();
        } else {
            self.buf = data.to_vec();
        }
    }

    /// If a partial fragment is buffered, prepend it to `data` using `tmp` as
    /// scratch space and return the merged slice. Otherwise return `data` as-is.
    ///
    /// After this call the internal buffer is empty — the caller is expected to
    /// process the returned slice and then call [`store`] again if a new partial
    /// fragment remains at the end.
    pub fn prepend_to<'a>(&mut self, data: &'a [u8], tmp: &'a mut Vec<u8>) -> &'a [u8] {
        if self.buf.is_empty() {
            return data;
        }
        tmp.clear();
        tmp.reserve(self.buf.len() + data.len());
        tmp.extend_from_slice(&self.buf);
        tmp.extend_from_slice(data);
        self.buf.clear();
        tmp.as_slice()
    }

    // Used by osc8_parser, shell_integration, sixel parsers in this crate.
    // Clippy false-positive: methods on pub(crate) struct appear unused.
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_and_prepend() {
        let mut pb = PartialBuf::new(1024, "test");
        pb.store(b"hello ");
        assert!(!pb.is_empty());

        let mut tmp = Vec::new();
        let merged = pb.prepend_to(b"world", &mut tmp);
        assert_eq!(merged, b"hello world");
        assert!(pb.is_empty());
    }

    #[test]
    fn oversized_store_discards() {
        let mut pb = PartialBuf::new(4, "test");
        pb.store(b"12345");
        assert!(pb.is_empty());
    }

    #[test]
    fn prepend_without_stored_returns_input() {
        let mut pb = PartialBuf::new(1024, "test");
        let mut tmp = Vec::new();
        let result = pb.prepend_to(b"data", &mut tmp);
        assert_eq!(result, b"data");
    }
}
