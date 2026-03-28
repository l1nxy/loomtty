//! Safe integer readers for binary decoding.

use std::io;

pub(super) fn read_u16_le(buf: &[u8], off: usize) -> io::Result<u16> {
    buf.get(off..off + 2)
        .and_then(|s| s.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "truncated"))
}

pub(super) fn read_u32_le(buf: &[u8], off: usize) -> io::Result<u32> {
    buf.get(off..off + 4)
        .and_then(|s| s.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "truncated"))
}

pub(super) fn read_u64_le(buf: &[u8], off: usize) -> io::Result<u64> {
    buf.get(off..off + 8)
        .and_then(|s| s.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "truncated"))
}

pub(super) fn read_i16_le(buf: &[u8], off: usize) -> io::Result<i16> {
    buf.get(off..off + 2)
        .and_then(|s| s.try_into().ok())
        .map(i16::from_le_bytes)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "truncated"))
}
