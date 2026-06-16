//! Binary decoding utilities: cursor-based parser for protocol wire formats.

use std::io;

fn truncated() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "truncated")
}

// ─── SliceCursor: auto-advancing reader over a byte slice ───────────

/// A cursor over a byte slice that tracks position and provides
/// bounds-checked reads with automatic advancement.
pub(super) struct SliceCursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> SliceCursor<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    #[inline]
    pub fn pos(&self) -> usize {
        self.pos
    }

    #[inline]
    fn require(&self, n: usize) -> io::Result<()> {
        // checked_add guards against pos + n overflowing usize on 32-bit hosts
        // when `n` is an attacker-controlled length read from the wire.
        match self.pos.checked_add(n) {
            Some(end) if end <= self.data.len() => Ok(()),
            _ => Err(truncated()),
        }
    }

    #[inline]
    pub fn read_u8(&mut self) -> io::Result<u8> {
        self.require(1)?;
        let v = self.data[self.pos];
        self.pos += 1;
        Ok(v)
    }

    #[inline]
    pub fn read_u16(&mut self) -> io::Result<u16> {
        self.require(2)?;
        let v = u16::from_le_bytes([self.data[self.pos], self.data[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }

    #[inline]
    pub fn read_u32(&mut self) -> io::Result<u32> {
        self.require(4)?;
        let v = u32::from_le_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(v)
    }

    /// Read `n` bytes and return a sub-slice (no copy).
    #[inline]
    pub fn read_bytes(&mut self, n: usize) -> io::Result<&'a [u8]> {
        self.require(n)?;
        let slice = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    /// Read a u16-length-prefixed UTF-8 string (lossy).
    pub fn read_lossy_string_u16(&mut self) -> io::Result<String> {
        let len = self.read_u16()? as usize;
        if len == 0 {
            return Ok(String::new());
        }
        let bytes = self.read_bytes(len)?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    /// Read a zerocopy struct from the current position.
    #[inline]
    pub fn read_ref<T: zerocopy::FromBytes + zerocopy::KnownLayout + zerocopy::Immutable>(
        &mut self,
    ) -> io::Result<&'a T> {
        let size = size_of::<T>();
        self.require(size)?;
        let bytes = &self.data[self.pos..self.pos + size];
        self.pos += size;
        zerocopy::Ref::<&[u8], T>::from_bytes(bytes)
            .map(zerocopy::Ref::into_ref)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "zerocopy alignment"))
    }
}
