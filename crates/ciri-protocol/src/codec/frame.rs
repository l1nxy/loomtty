//! Frame format, tags, and msgpack encode/decode for control messages.

use crate::message::*;
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::cell_delta::decode_cell_delta_borrowed;
use super::full_sync::{decode_full_pane_sync_borrowed, encode_full_pane_sync_payload};

// ─── Frame tags ─────────────────────────────────────────────────────

// Client → Server (msgpack)
pub(super) const TAG_CLIENT_MSG: u8 = 0x01;
pub(super) const TAG_SERVER_MSG: u8 = 0x10;

// Server → Client (custom binary, hot path)
pub(super) const TAG_CELL_DELTA: u8 = 0x20;
pub(super) const TAG_FULL_PANE_SYNC: u8 = 0x21;
// LZ4-compressed variants (payload is [u32 LE uncompressed_len][lz4 data])
pub(super) const TAG_CELL_DELTA_LZ4: u8 = 0x22;
pub(super) const TAG_FULL_PANE_SYNC_LZ4: u8 = 0x23;

/// Minimum payload size before LZ4 compression kicks in (bytes).
/// Below this threshold, compression overhead exceeds savings.
const LZ4_COMPRESS_THRESHOLD: usize = 128;

/// Maximum allowed ratio of uncompressed-to-compressed size for LZ4 frames.
/// Legitimate terminal data rarely exceeds 20:1 (repeated whitespace/colors).
/// A ratio above this strongly suggests a decompression bomb.
const MAX_LZ4_RATIO: usize = 64;

/// Maximum frame size for control messages (msgpack).  Control messages are
/// small — 1 MiB is generous.
pub(super) const MAX_CONTROL_FRAME_LEN: u32 = 1024 * 1024;

/// Maximum frame size for data frames (CellDelta, FullPaneSync).
/// Large grids with scrollback can legitimately reach several MiB.
pub const MAX_DATA_FRAME_LEN: u32 = 16 * 1024 * 1024;

// ─── Frame format: [u8 tag][u32 LE payload_len][payload] ───────────

pub(super) async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    tag: u8,
    payload: &[u8],
) -> io::Result<()> {
    let len = frame_len_u32(payload)?;
    let mut header = [0u8; 5];
    header[0] = tag;
    header[1..5].copy_from_slice(&len.to_le_bytes());
    writer.write_all(&header).await?;
    writer.write_all(payload).await?;
    Ok(())
}

pub(super) fn frame_len_u32(payload: &[u8]) -> io::Result<u32> {
    u32::try_from(payload.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame payload too large"))
}

pub(crate) fn build_frame(tag: u8, payload: &[u8]) -> io::Result<Vec<u8>> {
    let mut frame = Vec::with_capacity(5 + payload.len());
    frame.push(tag);
    frame.extend_from_slice(&frame_len_u32(payload)?.to_le_bytes());
    frame.extend_from_slice(payload);
    Ok(frame)
}

async fn read_frame_header<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<(u8, u32)> {
    let mut header = [0u8; 5];
    reader.read_exact(&mut header).await?;
    let tag = header[0];
    let len = u32::from_le_bytes([header[1], header[2], header[3], header[4]]);
    // Apply per-tag size limits: control messages are small, data frames can be larger.
    let limit = match tag {
        TAG_CLIENT_MSG | TAG_SERVER_MSG => MAX_CONTROL_FRAME_LEN,
        TAG_CELL_DELTA | TAG_FULL_PANE_SYNC | TAG_CELL_DELTA_LZ4 | TAG_FULL_PANE_SYNC_LZ4 => {
            MAX_DATA_FRAME_LEN
        }
        // Unknown tag: reject the frame at the header rather than letting
        // a malicious peer dictate up to 16 MiB of read buffer before the
        // payload is even examined.
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown frame tag: 0x{tag:02x}"),
            ));
        }
    };
    if len > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame too large: tag=0x{tag:02x}, len={len}, limit={limit}"),
        ));
    }
    Ok((tag, len))
}

// ─── Encode / decode ClientMessage (msgpack) ────────────────────────

pub async fn encode_client_msg<W: AsyncWrite + Unpin>(
    writer: &mut W,
    msg: &ClientMessage,
) -> io::Result<()> {
    let payload =
        rmp_serde::to_vec(msg).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    write_frame(writer, TAG_CLIENT_MSG, &payload).await
}

// ─── Encode / decode ServerMessage (msgpack) ────────────────────────

pub async fn encode_server_msg<W: AsyncWrite + Unpin>(
    writer: &mut W,
    msg: &ServerMessage,
) -> io::Result<()> {
    let payload =
        rmp_serde::to_vec(msg).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    write_frame(writer, TAG_SERVER_MSG, &payload).await
}

pub fn frame_server_msg(msg: &ServerMessage) -> Option<Vec<u8>> {
    let payload = rmp_serde::to_vec(msg).ok()?;
    build_frame(TAG_SERVER_MSG, &payload).ok()
}

pub fn frame_server_msg_into(buf: &mut Vec<u8>, msg: &ServerMessage) -> bool {
    let payload = match rmp_serde::to_vec(msg) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let payload_len = match frame_len_u32(&payload) {
        Ok(len) => len,
        Err(_) => return false,
    };
    buf.clear();
    buf.reserve(5 + payload.len());
    buf.push(TAG_SERVER_MSG);
    buf.extend_from_slice(&payload_len.to_le_bytes());
    buf.extend_from_slice(&payload);
    true
}

pub fn frame_full_pane_sync(sync: &FullPaneSync) -> Option<Vec<u8>> {
    let payload = encode_full_pane_sync_payload(sync).ok()?;
    let (tag, compressed) =
        maybe_compress_payload(TAG_FULL_PANE_SYNC, TAG_FULL_PANE_SYNC_LZ4, &payload);
    build_frame(tag, &compressed).ok()
}

// ─── Unified frame reader ───────────────────────────────────────────

/// A decoded frame from the wire.
#[derive(Debug)]
pub enum Frame {
    ClientMsg(ClientMessage),
    ServerMsg(ServerMessage),
    CellDelta(CellDeltaBorrowed),
    FullPaneSync(FullPaneSyncBorrowed),
}

pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Frame> {
    let header = read_frame_header(reader).await?;
    let payload = read_frame_payload(reader, header.1).await?;
    let tag = header.0;
    decode_frame(tag, payload)
}

/// Read a frame, reusing an existing buffer to avoid per-frame allocation.
///
/// The buffer is resized (not reallocated if capacity suffices) and filled
/// with the payload. After decoding, borrowed frame types (CellDelta,
/// FullPaneSync) take ownership of the buffer; the caller should reclaim it
/// via `into_payload()` and pass it back on the next call.
pub async fn read_frame_reuse<R: AsyncRead + Unpin>(
    reader: &mut R,
    buf: &mut Vec<u8>,
) -> io::Result<Frame> {
    let (tag, len) = read_frame_header(reader).await?;
    buf.clear();
    buf.resize(len as usize, 0);
    reader.read_exact(buf).await?;
    // Take the buffer, replacing with an empty Vec (no alloc).
    let payload = std::mem::take(buf);
    decode_frame(tag, payload)
}

async fn read_frame_payload<R: AsyncRead + Unpin>(reader: &mut R, len: u32) -> io::Result<Vec<u8>> {
    let mut payload = vec![0u8; len as usize];
    reader.read_exact(&mut payload).await?;
    Ok(payload)
}

fn decode_frame(tag: u8, payload: Vec<u8>) -> io::Result<Frame> {
    match tag {
        TAG_CLIENT_MSG => decode_msgpack_frame(&payload).map(Frame::ClientMsg),
        TAG_SERVER_MSG => decode_msgpack_frame(&payload).map(Frame::ServerMsg),
        TAG_CELL_DELTA => decode_cell_delta_borrowed(payload).map(Frame::CellDelta),
        TAG_FULL_PANE_SYNC => decode_full_pane_sync_borrowed(payload).map(Frame::FullPaneSync),
        TAG_CELL_DELTA_LZ4 => {
            let decompressed = decompress_lz4_payload(&payload)?;
            decode_cell_delta_borrowed(decompressed).map(Frame::CellDelta)
        }
        TAG_FULL_PANE_SYNC_LZ4 => {
            let decompressed = decompress_lz4_payload(&payload)?;
            decode_full_pane_sync_borrowed(decompressed).map(Frame::FullPaneSync)
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unknown frame tag: 0x{tag:02x}"),
        )),
    }
}

fn decode_msgpack_frame<T>(payload: &[u8]) -> io::Result<T>
where
    T: serde::de::DeserializeOwned,
{
    rmp_serde::from_slice(payload).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Decompress an LZ4 frame payload: [u32 LE uncompressed_len][lz4 data] → raw bytes.
pub(crate) fn decompress_lz4_payload(payload: &[u8]) -> io::Result<Vec<u8>> {
    if payload.len() < 4 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "LZ4 payload too short",
        ));
    }
    let uncompressed_len =
        u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
    if uncompressed_len > MAX_DATA_FRAME_LEN as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "LZ4 uncompressed length exceeds limit",
        ));
    }
    // Reject decompression bombs: if the claimed uncompressed size is vastly
    // larger than the compressed data, this is likely an attack.
    let compressed_len = payload.len() - 4;
    // Reject zero-length compressed payloads that claim non-zero
    // uncompressed output. Otherwise the ratio guard below skips
    // (compressed_len > 0 is false), and lz4_flex::decompress would
    // attempt to allocate `uncompressed_len` bytes from an empty input
    // — a free-form OOM amplifier for malicious peers.
    if compressed_len == 0 && uncompressed_len > 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "LZ4 payload claims non-zero uncompressed length with zero compressed bytes",
        ));
    }
    // Multiplication form: `uncompressed > MAX_LZ4_RATIO * compressed`
    // avoids the integer-truncation gap of the division form (e.g.,
    // 193 / 3 == 64 would pass the `> 64` check even though the real
    // ratio is 64.33:1).
    if compressed_len > 0 && uncompressed_len > MAX_LZ4_RATIO.saturating_mul(compressed_len) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "LZ4 compression ratio {:.0}:1 exceeds limit {MAX_LZ4_RATIO}:1",
                uncompressed_len as f64 / compressed_len as f64,
            ),
        ));
    }
    lz4_flex::decompress(&payload[4..], uncompressed_len)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("LZ4 decompress: {e}")))
}

/// Try to LZ4-compress a payload, returning (compressed_tag, data).
/// Falls back to uncompressed if the payload is too small or compression doesn't help.
pub(crate) fn maybe_compress_payload(
    uncompressed_tag: u8,
    compressed_tag: u8,
    payload: &[u8],
) -> (u8, Vec<u8>) {
    if payload.len() < LZ4_COMPRESS_THRESHOLD {
        return (uncompressed_tag, payload.to_vec());
    }
    let compressed = lz4_flex::compress(payload);
    // Only use compressed if it's actually smaller (+ 4 bytes for uncompressed_len header).
    let lz4_payload_len = 4 + compressed.len();
    if lz4_payload_len < payload.len() {
        let mut out = Vec::with_capacity(lz4_payload_len);
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&compressed);
        (compressed_tag, out)
    } else {
        (uncompressed_tag, payload.to_vec())
    }
}

/// Try LZ4 compression in-place on a frame buffer.
///
/// `buf` layout: `[tag (1B)][len placeholder (4B)][payload...]`
/// On return, tag/len are updated and payload may be replaced with LZ4 data.
/// Avoids the intermediate `Vec` allocation that `maybe_compress_payload` requires.
pub(crate) fn finalize_frame_compression(
    buf: &mut Vec<u8>,
    payload_start: usize,
    uncompressed_tag: u8,
    compressed_tag: u8,
) {
    let payload = &buf[payload_start..];
    let payload_len = payload.len();

    if payload_len >= LZ4_COMPRESS_THRESHOLD {
        let compressed = lz4_flex::compress(payload);
        let lz4_total = 4 + compressed.len();
        if lz4_total < payload_len {
            // Compressed is smaller — replace payload with [u32 uncompressed_len][lz4 data].
            buf.truncate(payload_start);
            buf[0] = compressed_tag;
            let len = lz4_total as u32;
            buf[1..5].copy_from_slice(&len.to_le_bytes());
            buf.extend_from_slice(&(payload_len as u32).to_le_bytes());
            buf.extend_from_slice(&compressed);
            return;
        }
    }

    // No compression — just finalize tag + length in-place (zero copies).
    buf[0] = uncompressed_tag;
    let len = payload_len as u32;
    buf[1..5].copy_from_slice(&len.to_le_bytes());
}
