//! Frame format, tags, and msgpack encode/decode for control messages.

use crate::message::*;
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::cell_delta::decode_cell_delta_borrowed;
use super::full_sync::{decode_full_pane_sync, encode_full_pane_sync_payload};

// ─── Frame tags ─────────────────────────────────────────────────────

// Client → Server (msgpack)
pub(super) const TAG_CLIENT_MSG: u8 = 0x01;
pub(super) const TAG_SERVER_MSG: u8 = 0x10;

// Server → Client (custom binary, hot path)
pub(super) const TAG_CELL_DELTA: u8 = 0x20;
pub(super) const TAG_FULL_PANE_SYNC: u8 = 0x21;

pub(super) const MAX_FRAME_LEN: u32 = 16 * 1024 * 1024;

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
    if len > MAX_FRAME_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
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
    build_frame(TAG_FULL_PANE_SYNC, &payload).ok()
}

// ─── Unified frame reader ───────────────────────────────────────────

/// A decoded frame from the wire.
#[derive(Debug)]
pub enum Frame {
    ClientMsg(ClientMessage),
    ServerMsg(ServerMessage),
    CellDelta(CellDeltaBorrowed),
    FullPaneSync(FullPaneSync),
}

pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Frame> {
    let header = read_frame_header(reader).await?;
    let payload = read_frame_payload(reader, header.1).await?;
    let tag = header.0;
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
        TAG_FULL_PANE_SYNC => decode_full_pane_sync(&payload).map(Frame::FullPaneSync),
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
