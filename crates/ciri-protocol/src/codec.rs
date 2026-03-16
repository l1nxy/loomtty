use serde::{Deserialize, Serialize};
use std::io::{self, Read};

/// Length-prefixed JSON codec for control messages.
/// Format: [4 bytes big-endian length][JSON payload]

pub fn encode<T: Serialize>(msg: &T) -> io::Result<Vec<u8>> {
    let json = serde_json::to_vec(msg).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let len = json.len() as u32;
    let mut buf = Vec::with_capacity(4 + json.len());
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&json);
    Ok(buf)
}

pub fn decode_from<T: for<'de> Deserialize<'de>>(reader: &mut impl Read) -> io::Result<T> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > 16 * 1024 * 1024 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
    }

    let mut json_buf = vec![0u8; len];
    reader.read_exact(&mut json_buf)?;

    serde_json::from_slice(&json_buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
