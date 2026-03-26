// ─── Serialization format evaluation (ROADMAP item 5) ───────────────
//
// Control messages (ServerMessage/ClientMessage) use msgpack on the cold path —
// overhead is negligible since these are infrequent (resize, focus, layout).
//
// Hot-path messages (CellDelta/FullPaneSync) already use a custom binary format
// with bytemuck zero-copy for PackedCell data. CellDeltaBorrowed avoids even
// per-region Vec<PackedCell> allocation by casting directly from the payload.
//
// Flatbuffers was evaluated but is not worth the added complexity or dependency:
//   - Our hot-path encoding is already zero-copy where it matters (cell data).
//   - Flatbuffers would add a build-time codegen step and ~3k lines of generated code.
//   - The wire format savings would be minimal since cell data dominates frame size.
//
// Conclusion: keep the current approach (msgpack for control, custom binary + bytemuck
// for hot-path). Re-evaluate only if a new variable-length hot-path message is added.
// ─────────────────────────────────────────────────────────────────────

use crate::message::*;
use bytes::Buf;
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

// ─── Protocol handshake ─────────────────────────────────────────────

/// Pack semver "major.minor.patch" into u32: major(8).minor(8).patch(16).
const fn pack_version(major: u8, minor: u8, patch: u16) -> u32 {
    (major as u32) << 24 | (minor as u32) << 16 | patch as u32
}

fn parse_pkg_version() -> u32 {
    let v = env!("CARGO_PKG_VERSION");
    let parts: Vec<&str> = v.split('.').collect();
    assert!(
        parts.len() == 3,
        "CARGO_PKG_VERSION must be major.minor.patch"
    );
    let major: u8 = parts[0].parse().expect("bad major");
    let minor: u8 = parts[1].parse().expect("bad minor");
    let patch: u16 = parts[2].parse().expect("bad patch");
    pack_version(major, minor, patch)
}

fn unpack_version(v: u32) -> String {
    let major = (v >> 24) & 0xFF;
    let minor = (v >> 16) & 0xFF;
    let patch = v & 0xFFFF;
    format!("{major}.{minor}.{patch}")
}

const HANDSHAKE_MAGIC: [u8; 4] = *b"CIRI";
const CLIENT_HELLO_FIXED_FIELDS_LEN: usize = 16;
const CLIENT_HELLO_HEADER_LEN: usize = 11;
const SERVER_HELLO_LEN: usize = 8;
const MAX_SESSION_NAME_LEN: usize = 255;
const MAX_FRAME_LEN: u32 = 16 * 1024 * 1024;

/// Wire protocol version. Incremented whenever the handshake or frame format
/// changes in a backward-incompatible way. This is separate from CARGO_PKG_VERSION
/// so that rolling upgrades between patch/minor releases fail cleanly instead of
/// silently misparsing.
///
/// History:
///   1 = initial fixed 24-byte ClientHello
///   2 = variable-length ClientHello with session_name_len(u16) prefix
///   3 = state-machine cell encoding (replaces per-cell + RLE)
pub const WIRE_PROTOCOL_VERSION: u8 = 3;

/// Version compatibility result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionCompat {
    Exact(String),
    PatchMismatch { peer: String, local: String },
    MinorMismatch { peer: String, local: String },
}

fn check_version(peer: u32) -> io::Result<VersionCompat> {
    let local = parse_pkg_version();
    let peer_major = (peer >> 24) & 0xFF;
    let peer_minor = (peer >> 16) & 0xFF;
    let local_major = (local >> 24) & 0xFF;
    let local_minor = (local >> 16) & 0xFF;

    if peer_major != local_major {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "incompatible major version: peer={}, local={}",
                unpack_version(peer),
                unpack_version(local),
            ),
        ));
    }
    if peer == local {
        Ok(VersionCompat::Exact(unpack_version(peer)))
    } else if peer_minor != local_minor {
        Ok(VersionCompat::MinorMismatch {
            peer: unpack_version(peer),
            local: unpack_version(local),
        })
    } else {
        Ok(VersionCompat::PatchMismatch {
            peer: unpack_version(peer),
            local: unpack_version(local),
        })
    }
}

/// Client hello payload sent during handshake.
#[derive(Debug, Clone)]
pub struct ClientHello {
    pub session_name: String,
    pub width: u32,
    pub height: u32,
    pub cell_width: f32,
    pub cell_height: f32,
}

struct ClientHelloHeader {
    peer_version: u32,
    session_name_len: usize,
}

struct ServerHello {
    peer_version: u32,
}

/// ClientHello wire format:
/// [magic(4)][version(4)][wire_ver(1)][session_name_len(2)][session_name(N)][width(4)][height(4)][cell_w(4)][cell_h(4)]
///
/// `wire_ver` is checked independently of the cargo version. If it doesn't match,
/// the connection is rejected immediately, ensuring rolling upgrades fail cleanly.
pub async fn write_client_hello<W: AsyncWrite + Unpin>(
    writer: &mut W,
    hello: &ClientHello,
) -> io::Result<()> {
    let buf = build_client_hello(hello)?;
    writer.write_all(&buf).await?;
    writer.flush().await
}

pub fn build_client_hello(hello: &ClientHello) -> io::Result<Vec<u8>> {
    let name_bytes =
        validate_session_name_len(hello.session_name.as_bytes(), io::ErrorKind::InvalidInput)?;
    let mut buf = Vec::with_capacity(
        CLIENT_HELLO_HEADER_LEN + CLIENT_HELLO_FIXED_FIELDS_LEN + name_bytes.len(),
    );
    buf.extend_from_slice(&HANDSHAKE_MAGIC);
    buf.extend_from_slice(&parse_pkg_version().to_le_bytes());
    buf.push(WIRE_PROTOCOL_VERSION);
    buf.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
    buf.extend_from_slice(name_bytes);
    buf.extend_from_slice(&hello.width.to_le_bytes());
    buf.extend_from_slice(&hello.height.to_le_bytes());
    buf.extend_from_slice(&hello.cell_width.to_bits().to_le_bytes());
    buf.extend_from_slice(&hello.cell_height.to_bits().to_le_bytes());
    Ok(buf)
}

/// Server reads ClientHello. Returns version compat + hello payload.
pub async fn read_client_hello<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> io::Result<(VersionCompat, ClientHello)> {
    let header = read_client_hello_header(reader).await?;
    let compat = check_version(header.peer_version)?;

    let rest = read_client_hello_body(reader, header.session_name_len).await?;
    decode_client_hello_parts(header.session_name_len, &rest).map(|hello| (compat, hello))
}

async fn read_client_hello_header<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> io::Result<ClientHelloHeader> {
    let mut header = [0u8; CLIENT_HELLO_HEADER_LEN];
    reader.read_exact(&mut header).await?;
    parse_client_hello_header(&header)
}

async fn read_client_hello_body<R: AsyncRead + Unpin>(
    reader: &mut R,
    session_name_len: usize,
) -> io::Result<Vec<u8>> {
    let mut rest = vec![0u8; session_name_len + CLIENT_HELLO_FIXED_FIELDS_LEN];
    reader.read_exact(&mut rest).await?;
    Ok(rest)
}

fn parse_client_hello_header(
    header: &[u8; CLIENT_HELLO_HEADER_LEN],
) -> io::Result<ClientHelloHeader> {
    require_handshake_magic(&header[..4])?;
    let peer_version = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    let wire_version = header[8];
    if wire_version != WIRE_PROTOCOL_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "incompatible wire protocol: peer={wire_version}, local={WIRE_PROTOCOL_VERSION} \
                 (client and server binaries must be the same build)",
            ),
        ));
    }

    let session_name_len = validate_session_name_len_value(
        u16::from_le_bytes([header[9], header[10]]) as usize,
        io::ErrorKind::InvalidData,
    )?;

    Ok(ClientHelloHeader {
        peer_version,
        session_name_len,
    })
}

fn decode_client_hello_parts(session_name_len: usize, rest: &[u8]) -> io::Result<ClientHello> {
    let session_name = String::from_utf8(rest[..session_name_len].to_vec())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid session name utf8"))?;
    let mut cursor = &rest[session_name_len..] as &[u8];
    let hello = ClientHello {
        session_name,
        width: cursor.get_u32_le(),
        height: cursor.get_u32_le(),
        cell_width: cursor.get_f32_le(),
        cell_height: cursor.get_f32_le(),
    };
    validate_viewport_dims(&hello)?;
    Ok(hello)
}

/// ServerHello: [magic(4)][version(4)] = 8 bytes
pub async fn write_server_hello<W: AsyncWrite + Unpin>(writer: &mut W) -> io::Result<()> {
    let mut buf = [0u8; SERVER_HELLO_LEN];
    buf[0..4].copy_from_slice(&HANDSHAKE_MAGIC);
    buf[4..8].copy_from_slice(&parse_pkg_version().to_le_bytes());
    writer.write_all(&buf).await?;
    // Don't flush here — caller will send StateSync frames right after
    Ok(())
}

/// Client reads ServerHello.
pub async fn read_server_hello<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<VersionCompat> {
    let hello = read_server_hello_payload(reader).await?;
    check_version(hello.peer_version)
}

async fn read_server_hello_payload<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> io::Result<ServerHello> {
    let mut buf = [0u8; SERVER_HELLO_LEN];
    reader.read_exact(&mut buf).await?;
    parse_server_hello(&buf)
}

fn parse_server_hello(buf: &[u8; SERVER_HELLO_LEN]) -> io::Result<ServerHello> {
    require_handshake_magic(&buf[0..4])?;
    Ok(ServerHello {
        peer_version: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
    })
}

fn require_handshake_magic(magic: &[u8]) -> io::Result<()> {
    if magic == HANDSHAKE_MAGIC {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bad magic bytes",
        ))
    }
}

fn validate_session_name_len(name_bytes: &[u8], kind: io::ErrorKind) -> io::Result<&[u8]> {
    validate_session_name_len_value(name_bytes.len(), kind)?;
    Ok(name_bytes)
}

fn validate_session_name_len_value(len: usize, kind: io::ErrorKind) -> io::Result<usize> {
    if len > MAX_SESSION_NAME_LEN {
        Err(io::Error::new(kind, "session name too long"))
    } else {
        Ok(len)
    }
}

fn validate_viewport_dims(hello: &ClientHello) -> io::Result<()> {
    if !hello.cell_width.is_finite() || hello.cell_width <= 0.0 || hello.cell_width > 200.0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid cell_width: {}", hello.cell_width),
        ));
    }
    if !hello.cell_height.is_finite() || hello.cell_height <= 0.0 || hello.cell_height > 200.0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid cell_height: {}", hello.cell_height),
        ));
    }
    if hello.width == 0 || hello.width > 16384 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid viewport width: {}", hello.width),
        ));
    }
    if hello.height == 0 || hello.height > 16384 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid viewport height: {}", hello.height),
        ));
    }
    Ok(())
}

// ─── Safe integer readers ───────────────────────────────────────────

fn read_u16_le(buf: &[u8], off: usize) -> io::Result<u16> {
    buf.get(off..off + 2)
        .and_then(|s| s.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "truncated"))
}

fn read_u32_le(buf: &[u8], off: usize) -> io::Result<u32> {
    buf.get(off..off + 4)
        .and_then(|s| s.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "truncated"))
}

fn read_u64_le(buf: &[u8], off: usize) -> io::Result<u64> {
    buf.get(off..off + 8)
        .and_then(|s| s.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "truncated"))
}

fn read_i16_le(buf: &[u8], off: usize) -> io::Result<i16> {
    buf.get(off..off + 2)
        .and_then(|s| s.try_into().ok())
        .map(i16::from_le_bytes)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "truncated"))
}

// ─── Frame tags ─────────────────────────────────────────────────────

// Client → Server (msgpack)
const TAG_CLIENT_MSG: u8 = 0x01;

// Server → Client (msgpack)
const TAG_SERVER_MSG: u8 = 0x10;

// Server → Client (custom binary, hot path)
const TAG_CELL_DELTA: u8 = 0x20;
const TAG_FULL_PANE_SYNC: u8 = 0x21;

// ─── Frame format: [u8 tag][u32 LE payload_len][payload] ───────────

/// Write a framed message to an async writer.
async fn write_frame<W: AsyncWrite + Unpin>(
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

fn frame_len_u32(payload: &[u8]) -> io::Result<u32> {
    u32::try_from(payload.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame payload too large"))
}

fn build_frame(tag: u8, payload: &[u8]) -> io::Result<Vec<u8>> {
    let mut frame = Vec::with_capacity(5 + payload.len());
    frame.push(tag);
    frame.extend_from_slice(&frame_len_u32(payload)?.to_le_bytes());
    frame.extend_from_slice(payload);
    Ok(frame)
}

/// Read a frame header, returning (tag, payload_length).
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

/// Build a complete framed `ServerMessage` as `[tag][u32 LE len][msgpack payload]`.
/// Returns `None` if serialization fails.
pub fn frame_server_msg(msg: &ServerMessage) -> Option<Vec<u8>> {
    let payload = rmp_serde::to_vec(msg).ok()?;
    build_frame(TAG_SERVER_MSG, &payload).ok()
}

/// Build a complete framed `ServerMessage` into a caller-supplied buffer (for pooled use).
/// Returns `false` if serialization fails.
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

/// Build a complete framed `FullPaneSync` as `[tag][u32 LE len][payload]`.
/// Returns `None` if encoding fails.
pub fn frame_full_pane_sync(sync: &FullPaneSync) -> Option<Vec<u8>> {
    let payload = encode_full_pane_sync_payload(sync).ok()?;
    build_frame(TAG_FULL_PANE_SYNC, &payload).ok()
}

// ─── State-machine opcode constants ─────────────────────────────────

const OP_SET_FG: u8 = 0x01;
const OP_SET_BG: u8 = 0x02;
const OP_SET_FLAGS: u8 = 0x03;
const OP_SET_FG_BG: u8 = 0x04;
const OP_RESET: u8 = 0x05;

const OP_CHAR1: u8 = 0x10;
const OP_CHARS: u8 = 0x11;
const OP_REPEAT: u8 = 0x12;
const OP_CHARS_LONG: u8 = 0x13;

const OP_END: u8 = 0xFF;

// ─── State-machine encoder ──────────────────────────────────────────

/// Encodes a stream of `PackedCell`s into a compact opcode stream.
/// Emits attribute-change opcodes only when fg/bg/flags differ from the
/// current state, and coalesces consecutive characters into bulk opcodes.
///
/// Each damage region should use a fresh encoder (or call `reset()`).
pub struct StateEncoder {
    cur_fg: PackedColor,
    cur_bg: PackedColor,
    cur_flags: u16,
    run_ch: Option<[u8; 4]>,
    run_count: u16,
    char_buf: Vec<[u8; 4]>,
    out: Vec<u8>,
}

fn default_cell_state() -> (PackedColor, PackedColor, u16) {
    (DEFAULT_FOREGROUND, DEFAULT_BACKGROUND, DEFAULT_CELL_FLAGS)
}

impl StateEncoder {
    pub fn new() -> Self {
        let (fg, bg, flags) = default_cell_state();
        Self {
            cur_fg: fg,
            cur_bg: bg,
            cur_flags: flags,
            run_ch: None,
            run_count: 0,
            char_buf: Vec::new(),
            out: Vec::with_capacity(256),
        }
    }

    /// Feed a cell into the encoder.
    pub fn push_cell(&mut self, cell: &PackedCell) {
        let fg = cell.fg;
        let bg = cell.bg;
        let flags = cell.flags_u16();

        // 1. If attributes changed, flush pending chars then emit attribute opcodes
        if fg != self.cur_fg || bg != self.cur_bg || flags != self.cur_flags {
            self.flush_run();
            self.flush_char_buf();

            let target_is_default =
                fg == DEFAULT_FOREGROUND && bg == DEFAULT_BACKGROUND && flags == DEFAULT_CELL_FLAGS;

            if target_is_default {
                self.out.push(OP_RESET);
            } else {
                let fg_changed = fg != self.cur_fg;
                let bg_changed = bg != self.cur_bg;

                if fg_changed && bg_changed {
                    self.out.push(OP_SET_FG_BG);
                    self.out.extend_from_slice(bytemuck::bytes_of(&fg));
                    self.out.extend_from_slice(bytemuck::bytes_of(&bg));
                } else if fg_changed {
                    self.out.push(OP_SET_FG);
                    self.out.extend_from_slice(bytemuck::bytes_of(&fg));
                } else if bg_changed {
                    self.out.push(OP_SET_BG);
                    self.out.extend_from_slice(bytemuck::bytes_of(&bg));
                }

                if flags != self.cur_flags {
                    self.out.push(OP_SET_FLAGS);
                    self.out.extend_from_slice(&flags.to_le_bytes());
                }
            }

            self.cur_fg = fg;
            self.cur_bg = bg;
            self.cur_flags = flags;
        }

        // 2. Character accumulation with repeat detection
        let ch = cell.ch_bytes;
        if let Some(run_ch) = self.run_ch {
            if run_ch == ch {
                self.run_count += 1;
                // Flush before u16 overflow (max repeat count is 65535)
                if self.run_count == u16::MAX {
                    self.flush_char_buf();
                    self.emit_repeat(self.run_count, &run_ch);
                    self.run_ch = Some(ch);
                    self.run_count = 0;
                }
                return;
            }
            // Different char — flush current run
            if self.run_count >= 3 {
                self.flush_char_buf();
                self.emit_repeat(self.run_count, &run_ch);
            } else {
                for _ in 0..self.run_count {
                    self.char_buf.push(run_ch);
                }
            }
        }
        self.run_ch = Some(ch);
        self.run_count = 1;
    }

    fn emit_repeat(&mut self, count: u16, ch: &[u8; 4]) {
        self.out.push(OP_REPEAT);
        self.out.extend_from_slice(&count.to_le_bytes());
        self.out.extend_from_slice(ch);
    }

    fn flush_run(&mut self) {
        if let Some(run_ch) = self.run_ch.take() {
            if self.run_count >= 3 {
                self.flush_char_buf();
                self.emit_repeat(self.run_count, &run_ch);
            } else {
                for _ in 0..self.run_count {
                    self.char_buf.push(run_ch);
                }
            }
            self.run_count = 0;
        }
    }

    fn flush_char_buf(&mut self) {
        let count = self.char_buf.len();
        if count == 0 {
            return;
        }
        if count == 1 {
            self.out.push(OP_CHAR1);
            self.out.extend_from_slice(&self.char_buf[0]);
        } else if count <= 255 {
            self.out.push(OP_CHARS);
            self.out.push(count as u8);
            for ch in &self.char_buf {
                self.out.extend_from_slice(ch);
            }
        } else {
            self.out.push(OP_CHARS_LONG);
            self.out.extend_from_slice(&(count as u16).to_le_bytes());
            for ch in &self.char_buf {
                self.out.extend_from_slice(ch);
            }
        }
        self.char_buf.clear();
    }

    /// Flush remaining data and append the End opcode. Returns the encoded bytes.
    pub fn finish(&mut self) -> &[u8] {
        self.flush_run();
        self.flush_char_buf();
        self.out.push(OP_END);
        &self.out
    }

    /// Reset the encoder for reuse with a new region.
    pub fn reset(&mut self) {
        let (fg, bg, flags) = default_cell_state();
        self.cur_fg = fg;
        self.cur_bg = bg;
        self.cur_flags = flags;
        self.run_ch = None;
        self.run_count = 0;
        self.char_buf.clear();
        self.out.clear();
    }
}

impl Default for StateEncoder {
    fn default() -> Self {
        Self::new()
    }
}

// ─── State-machine decoder ──────────────────────────────────────────

/// Decode an SM opcode stream, writing cells into the provided slice.
/// Returns the number of cells written. Initial state: default PackedCell attrs.
pub fn decode_sm_cells(data: &[u8], cells: &mut [PackedCell]) -> io::Result<usize> {
    let (mut fg, mut bg, mut flags) = default_cell_state();
    let mut pos = 0;
    let mut ci = 0; // cell index

    while pos < data.len() {
        let op = data[pos];
        pos += 1;

        match op {
            OP_SET_FG => {
                if pos + 4 > data.len() {
                    return Err(truncated_err("SetFg"));
                }
                fg = *bytemuck::from_bytes::<PackedColor>(&data[pos..pos + 4]);
                pos += 4;
            }
            OP_SET_BG => {
                if pos + 4 > data.len() {
                    return Err(truncated_err("SetBg"));
                }
                bg = *bytemuck::from_bytes::<PackedColor>(&data[pos..pos + 4]);
                pos += 4;
            }
            OP_SET_FLAGS => {
                if pos + 2 > data.len() {
                    return Err(truncated_err("SetFlags"));
                }
                flags = u16::from_le_bytes([data[pos], data[pos + 1]]);
                pos += 2;
            }
            OP_SET_FG_BG => {
                if pos + 8 > data.len() {
                    return Err(truncated_err("SetFgBg"));
                }
                fg = *bytemuck::from_bytes::<PackedColor>(&data[pos..pos + 4]);
                bg = *bytemuck::from_bytes::<PackedColor>(&data[pos + 4..pos + 8]);
                pos += 8;
            }
            OP_RESET => {
                (fg, bg, flags) = default_cell_state();
            }
            OP_CHAR1 => {
                if pos + 4 > data.len() {
                    return Err(truncated_err("Char1"));
                }
                if ci >= cells.len() {
                    return Err(overflow_err());
                }
                cells[ci] = PackedCell {
                    ch_bytes: [data[pos], data[pos + 1], data[pos + 2], data[pos + 3]],
                    fg,
                    bg,
                    flags: flags.to_le_bytes(),
                };
                ci += 1;
                pos += 4;
            }
            OP_CHARS => {
                if pos + 1 > data.len() {
                    return Err(truncated_err("Chars count"));
                }
                let count = data[pos] as usize;
                pos += 1;
                if pos + count * 4 > data.len() {
                    return Err(truncated_err("Chars data"));
                }
                let flags_le = flags.to_le_bytes();
                for i in 0..count {
                    if ci >= cells.len() {
                        return Err(overflow_err());
                    }
                    let off = pos + i * 4;
                    cells[ci] = PackedCell {
                        ch_bytes: [data[off], data[off + 1], data[off + 2], data[off + 3]],
                        fg,
                        bg,
                        flags: flags_le,
                    };
                    ci += 1;
                }
                pos += count * 4;
            }
            OP_REPEAT => {
                if pos + 6 > data.len() {
                    return Err(truncated_err("Repeat"));
                }
                let count = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
                pos += 2;
                let ch_bytes = [data[pos], data[pos + 1], data[pos + 2], data[pos + 3]];
                pos += 4;
                let cell = PackedCell {
                    ch_bytes,
                    fg,
                    bg,
                    flags: flags.to_le_bytes(),
                };
                for _ in 0..count {
                    if ci >= cells.len() {
                        return Err(overflow_err());
                    }
                    cells[ci] = cell;
                    ci += 1;
                }
            }
            OP_CHARS_LONG => {
                if pos + 2 > data.len() {
                    return Err(truncated_err("CharsLong count"));
                }
                let count = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
                pos += 2;
                if pos + count * 4 > data.len() {
                    return Err(truncated_err("CharsLong data"));
                }
                let flags_le = flags.to_le_bytes();
                for i in 0..count {
                    if ci >= cells.len() {
                        return Err(overflow_err());
                    }
                    let off = pos + i * 4;
                    cells[ci] = PackedCell {
                        ch_bytes: [data[off], data[off + 1], data[off + 2], data[off + 3]],
                        fg,
                        bg,
                        flags: flags_le,
                    };
                    ci += 1;
                }
                pos += count * 4;
            }
            OP_END => break,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unknown SM opcode: 0x{op:02x}"),
                ));
            }
        }
    }
    Ok(ci)
}

/// Convenience: SM-encode a cell slice and return the opcode bytes.
fn sm_encode_cells(cells: &[PackedCell]) -> Vec<u8> {
    let mut enc = StateEncoder::new();
    for cell in cells {
        enc.push_cell(cell);
    }
    enc.finish().to_vec()
}

/// Convenience: SM-decode into a newly allocated Vec.
fn sm_decode_cells_vec(data: &[u8], expected: usize) -> io::Result<Vec<PackedCell>> {
    let mut cells = vec![PackedCell::default(); expected];
    let decoded = decode_sm_cells(data, &mut cells)?;
    if decoded != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected {expected} cells but SM decoded {decoded}"),
        ));
    }
    Ok(cells)
}

fn truncated_err(ctx: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("truncated SM opcode: {ctx}"),
    )
}

fn overflow_err() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "SM decoded more cells than output buffer",
    )
}

// ─── Encode CellDelta (state-machine) ───────────────────────────────
//
// Wire format:
//   [u64 pane_id][u64 generation][i16 cursor_line][u16 cursor_col]
//   [u8 cursor_shape][u8 mode_flags][u16 cols][u16 num_regions]
//   per region:
//     [u16 line][u16 left][u16 right][u32 sm_data_len][sm_data...]

/// Encode a CellDelta frame by streaming cells through a StateEncoder.
/// The `write_cells` callback pushes packed cells into the encoder for each region.
#[allow(clippy::too_many_arguments)]
pub fn encode_cell_delta_streaming_framed<F>(
    buf: &mut Vec<u8>,
    pane_id: u64,
    generation: u64,
    cursor_line: i16,
    cursor_col: u16,
    cursor_shape: u8,
    mode_flags: u8,
    cols: u16,
    regions: &[(u16, u16, u16)], // (line, left, right)
    mut write_cells: F,
) -> io::Result<()>
where
    F: FnMut(u16, u16, u16, &mut StateEncoder),
{
    if regions.len() > u16::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "too many regions for CellDelta (exceeds u16::MAX)",
        ));
    }
    buf.clear();
    // tag(1) + len(4) + header(26) + per-region overhead
    buf.reserve(5 + 26 + regions.len() * 32);
    buf.push(TAG_CELL_DELTA);
    buf.extend_from_slice(&[0u8; 4]); // placeholder for payload length
    let payload_start = 5;
    buf.extend_from_slice(&pane_id.to_le_bytes());
    buf.extend_from_slice(&generation.to_le_bytes());
    buf.extend_from_slice(&cursor_line.to_le_bytes());
    buf.extend_from_slice(&cursor_col.to_le_bytes());
    buf.push(cursor_shape);
    buf.push(mode_flags);
    buf.extend_from_slice(&cols.to_le_bytes());
    buf.extend_from_slice(&(regions.len() as u16).to_le_bytes());

    let mut encoder = StateEncoder::new();
    for &(line, left, right) in regions {
        buf.extend_from_slice(&line.to_le_bytes());
        buf.extend_from_slice(&left.to_le_bytes());
        buf.extend_from_slice(&right.to_le_bytes());
        // Encode cells using SM — each region gets fresh state
        encoder.reset();
        write_cells(line, left, right, &mut encoder);
        let sm_data = encoder.finish();
        buf.extend_from_slice(&(sm_data.len() as u32).to_le_bytes());
        buf.extend_from_slice(sm_data);
    }

    let payload_len = (buf.len() - payload_start) as u32;
    buf[1..5].copy_from_slice(&payload_len.to_le_bytes());
    Ok(())
}

/// Decode CellDelta: parse region metadata, store SM payload for on-demand decoding.
pub fn decode_cell_delta_borrowed(payload: Vec<u8>) -> io::Result<CellDeltaBorrowed> {
    // Minimum: pane_id(8) + gen(8) + cursor(6) + cols(2) + num_regions(2) = 26
    if payload.len() < 26 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "CellDelta too short",
        ));
    }
    let pane_id = read_u64_le(&payload, 0)?;
    let generation = read_u64_le(&payload, 8)?;
    let cursor_line = read_i16_le(&payload, 16)?;
    let cursor_col = read_u16_le(&payload, 18)?;
    let cursor_shape = payload[20];
    let mode_flags = payload[21];
    let cols = read_u16_le(&payload, 22)?;
    let num_regions = read_u16_le(&payload, 24)? as usize;
    let mut offset = 26;
    let mut regions = Vec::with_capacity(num_regions);
    for _ in 0..num_regions {
        // line(2) + left(2) + right(2) + sm_data_len(4) = 10
        if offset + 10 > payload.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "truncated region header",
            ));
        }
        let line = read_u16_le(&payload, offset)?;
        let left = read_u16_le(&payload, offset + 2)?;
        let right = read_u16_le(&payload, offset + 4)?;
        let sm_data_len = read_u32_le(&payload, offset + 6)? as usize;
        offset += 10;
        validate_damage_bounds(left, right)?;
        if offset + sm_data_len > payload.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "truncated SM data",
            ));
        }
        regions.push(BorrowedRegionMeta {
            line,
            left,
            right,
            sm_offset: offset,
            sm_len: sm_data_len,
        });
        offset += sm_data_len;
    }
    Ok(CellDeltaBorrowed::new(
        pane_id,
        generation,
        cursor_line,
        cursor_col,
        cursor_shape,
        mode_flags,
        cols,
        regions,
        payload,
    ))
}

fn validate_damage_bounds(left: u16, right: u16) -> io::Result<()> {
    if left > right {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid damage region: left > right",
        ))
    } else {
        Ok(())
    }
}

// ─── Encode FullPaneSync (state-machine) ────────────────────────────
//
// Wire format:
//   mandatory:
//     [pane_id(8), generation(8), cols(2), rows(2), cursor_line(2), cursor_col(2),
//      cursor_shape(1), mode_flags(1), title_len(2), title(N)]
//     [scrollback_rows(2), scrollback_sm_len(4), scrollback_sm_data...]
//     [viewport_sm_len(4), viewport_sm_data...]
//   optional extras (backward-compatible tail):
//     [grapheme_extras...]
//     [hyperlink_extras...]

const FULL_PANE_SYNC_MIN_HEADER_LEN: usize = 28;
const FULL_PANE_SYNC_SCROLLBACK_HEADER_LEN: usize = 6;
const FULL_PANE_SYNC_VIEWPORT_HEADER_LEN: usize = 4;

#[derive(Debug)]
struct FullPaneSyncMandatorySections<'a> {
    scrollback: &'a [u8],
    viewport: &'a [u8],
    extra_offset: usize,
}

fn validate_full_pane_sync_title(title: &[u8]) -> io::Result<()> {
    if title.len() > u16::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "title too long for FullPaneSync (exceeds u16::MAX)",
        ));
    }
    Ok(())
}

fn write_full_pane_sync_header(
    buf: &mut Vec<u8>,
    sync: &FullPaneSync,
    title_bytes: &[u8],
) -> io::Result<()> {
    validate_full_pane_sync_title(title_bytes)?;
    buf.extend_from_slice(&sync.pane_id.to_le_bytes());
    buf.extend_from_slice(&sync.generation.to_le_bytes());
    buf.extend_from_slice(&sync.cols.to_le_bytes());
    buf.extend_from_slice(&sync.rows.to_le_bytes());
    buf.extend_from_slice(&sync.cursor_line.to_le_bytes());
    buf.extend_from_slice(&sync.cursor_col.to_le_bytes());
    buf.push(sync.cursor_shape);
    buf.push(sync.mode_flags);
    buf.extend_from_slice(&(title_bytes.len() as u16).to_le_bytes());
    buf.extend_from_slice(title_bytes);
    Ok(())
}

fn write_full_pane_sync_grapheme_extras(buf: &mut Vec<u8>, sync: &FullPaneSync) {
    let extras = &sync.grapheme_extras.0;
    buf.extend_from_slice(&(extras.len() as u16).to_le_bytes());
    for (idx, extra) in extras {
        buf.extend_from_slice(&idx.to_le_bytes());
        let bytes = extra.as_bytes();
        buf.push(bytes.len().min(255) as u8);
        buf.extend_from_slice(&bytes[..bytes.len().min(255)]);
    }
}

fn write_full_pane_sync_hyperlink_extras(buf: &mut Vec<u8>, sync: &FullPaneSync) {
    let extras = &sync.hyperlink_extras;
    buf.extend_from_slice(&(extras.cell_links.len() as u16).to_le_bytes());
    for &(cell_idx, link_id) in &extras.cell_links {
        buf.extend_from_slice(&cell_idx.to_le_bytes());
        buf.extend_from_slice(&link_id.to_le_bytes());
    }
    buf.extend_from_slice(&(extras.link_map.len() as u16).to_le_bytes());
    for (link_id, uri) in &extras.link_map {
        buf.extend_from_slice(&link_id.to_le_bytes());
        let uri_bytes = uri.as_bytes();
        let len = uri_bytes.len().min(u16::MAX as usize);
        buf.extend_from_slice(&(len as u16).to_le_bytes());
        buf.extend_from_slice(&uri_bytes[..len]);
    }
}

fn write_full_pane_sync_cwd(buf: &mut Vec<u8>, sync: &FullPaneSync) {
    match &sync.cwd {
        Some(cwd) => {
            let bytes = cwd.as_bytes();
            let len = bytes.len().min(u16::MAX as usize);
            buf.extend_from_slice(&(len as u16).to_le_bytes());
            buf.extend_from_slice(&bytes[..len]);
        }
        None => {
            buf.extend_from_slice(&0u16.to_le_bytes());
        }
    }
}

fn decode_cwd(payload: &[u8], offset: &mut usize) -> Option<String> {
    if *offset + 2 > payload.len() {
        return None;
    }
    let len = match read_u16_le(payload, *offset) {
        Ok(len) => len as usize,
        Err(_) => return None,
    };
    *offset += 2;
    if len == 0 {
        return None;
    }
    if *offset + len > payload.len() {
        return None;
    }
    let s = std::str::from_utf8(&payload[*offset..*offset + len])
        .ok()?
        .to_string();
    *offset += len;
    Some(s)
}

fn read_full_pane_sync_mandatory_sections(
    payload: &[u8],
    mut offset: usize,
) -> io::Result<(u16, String, FullPaneSyncMandatorySections<'_>)> {
    let title_len = read_u16_le(payload, 26)? as usize;
    if offset + title_len > payload.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "truncated title",
        ));
    }
    let title = String::from_utf8_lossy(&payload[offset..offset + title_len]);
    offset += title_len;

    if offset + FULL_PANE_SYNC_SCROLLBACK_HEADER_LEN > payload.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "truncated scrollback header",
        ));
    }
    let scrollback_rows = read_u16_le(payload, offset)?;
    offset += 2;
    let scrollback_len = read_u32_le(payload, offset)? as usize;
    offset += 4;
    if offset + scrollback_len > payload.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "truncated scrollback data",
        ));
    }
    let scrollback = &payload[offset..offset + scrollback_len];
    offset += scrollback_len;

    if offset + FULL_PANE_SYNC_VIEWPORT_HEADER_LEN > payload.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "truncated cell header",
        ));
    }
    let viewport_len = read_u32_le(payload, offset)? as usize;
    offset += 4;
    if offset + viewport_len > payload.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "truncated cell data",
        ));
    }
    let viewport = &payload[offset..offset + viewport_len];
    offset += viewport_len;

    Ok((
        scrollback_rows,
        title.into_owned(),
        FullPaneSyncMandatorySections {
            scrollback,
            viewport,
            extra_offset: offset,
        },
    ))
}

fn decode_grapheme_extras(payload: &[u8], offset: &mut usize) -> GraphemeExtras {
    let mut grapheme_extras = GraphemeExtras::new();
    if *offset + 2 > payload.len() {
        return grapheme_extras;
    }

    let count = match read_u16_le(payload, *offset) {
        Ok(count) => count as usize,
        Err(_) => return grapheme_extras,
    };
    *offset += 2;

    for _ in 0..count {
        if *offset + 5 > payload.len() {
            break;
        }
        let idx = match read_u32_le(payload, *offset) {
            Ok(idx) => idx,
            Err(_) => break,
        };
        *offset += 4;
        let len = payload[*offset] as usize;
        *offset += 1;
        if *offset + len > payload.len() {
            break;
        }
        let extra = String::from_utf8_lossy(&payload[*offset..*offset + len]).to_string();
        *offset += len;
        grapheme_extras.push(idx, &extra);
    }

    grapheme_extras
}

fn decode_hyperlink_extras(payload: &[u8], offset: &mut usize) -> HyperlinkExtras {
    let mut hyperlink_extras = HyperlinkExtras::new();
    if *offset + 2 > payload.len() {
        return hyperlink_extras;
    }

    let cell_links_count = match read_u16_le(payload, *offset) {
        Ok(count) => count as usize,
        Err(_) => return hyperlink_extras,
    };
    *offset += 2;
    for _ in 0..cell_links_count {
        if *offset + 6 > payload.len() {
            return hyperlink_extras;
        }
        let cell_idx = match read_u32_le(payload, *offset) {
            Ok(cell_idx) => cell_idx,
            Err(_) => return hyperlink_extras,
        };
        *offset += 4;
        let link_id = match read_u16_le(payload, *offset) {
            Ok(link_id) => link_id,
            Err(_) => return hyperlink_extras,
        };
        *offset += 2;
        hyperlink_extras.cell_links.push((cell_idx, link_id));
    }

    if *offset + 2 > payload.len() {
        return hyperlink_extras;
    }
    let link_map_count = match read_u16_le(payload, *offset) {
        Ok(count) => count as usize,
        Err(_) => return hyperlink_extras,
    };
    *offset += 2;
    for _ in 0..link_map_count {
        if *offset + 4 > payload.len() {
            return hyperlink_extras;
        }
        let link_id = match read_u16_le(payload, *offset) {
            Ok(link_id) => link_id,
            Err(_) => return hyperlink_extras,
        };
        *offset += 2;
        let uri_len = match read_u16_le(payload, *offset) {
            Ok(uri_len) => uri_len as usize,
            Err(_) => return hyperlink_extras,
        };
        *offset += 2;
        if *offset + uri_len > payload.len() {
            return hyperlink_extras;
        }
        let uri = String::from_utf8_lossy(&payload[*offset..*offset + uri_len]).to_string();
        *offset += uri_len;
        hyperlink_extras.link_map.push((link_id, uri));
    }

    hyperlink_extras
}

pub fn encode_full_pane_sync_payload(sync: &FullPaneSync) -> io::Result<Vec<u8>> {
    let title_bytes = sync.title.as_bytes();
    validate_full_pane_sync_title(title_bytes)?;
    let sm_scrollback = sm_encode_cells(&sync.scrollback);
    let sm_viewport = sm_encode_cells(&sync.cells);
    let mut buf =
        Vec::with_capacity(38 + title_bytes.len() + sm_scrollback.len() + sm_viewport.len());

    write_full_pane_sync_header(&mut buf, sync, title_bytes)?;
    buf.extend_from_slice(&sync.scrollback_rows.to_le_bytes());
    buf.extend_from_slice(&(sm_scrollback.len() as u32).to_le_bytes());
    buf.extend_from_slice(&sm_scrollback);
    buf.extend_from_slice(&(sm_viewport.len() as u32).to_le_bytes());
    buf.extend_from_slice(&sm_viewport);
    write_full_pane_sync_grapheme_extras(&mut buf, sync);
    write_full_pane_sync_cwd(&mut buf, sync);
    Ok(buf)
}

/// Encode a FullPaneSync directly into `buf` as a complete frame.
pub fn encode_full_pane_sync_framed(buf: &mut Vec<u8>, sync: &FullPaneSync) -> io::Result<()> {
    buf.clear();
    let title_bytes = sync.title.as_bytes();
    validate_full_pane_sync_title(title_bytes)?;
    // Tag + length placeholder
    buf.push(TAG_FULL_PANE_SYNC);
    buf.extend_from_slice(&[0u8; 4]);
    let payload_start = 5;
    write_full_pane_sync_header(buf, sync, title_bytes)?;
    // Scrollback (SM-encoded)
    buf.extend_from_slice(&sync.scrollback_rows.to_le_bytes());
    let mut encoder = StateEncoder::new();
    for cell in &sync.scrollback {
        encoder.push_cell(cell);
    }
    let sb_data = encoder.finish();
    buf.extend_from_slice(&(sb_data.len() as u32).to_le_bytes());
    buf.extend_from_slice(sb_data);
    // Viewport (SM-encoded)
    encoder.reset();
    for cell in &sync.cells {
        encoder.push_cell(cell);
    }
    let vp_data = encoder.finish();
    buf.extend_from_slice(&(vp_data.len() as u32).to_le_bytes());
    buf.extend_from_slice(vp_data);
    write_full_pane_sync_grapheme_extras(buf, sync);
    write_full_pane_sync_hyperlink_extras(buf, sync);
    write_full_pane_sync_cwd(buf, sync);
    // Patch the length field
    let payload_len = (buf.len() - payload_start) as u32;
    buf[1..5].copy_from_slice(&payload_len.to_le_bytes());
    Ok(())
}

pub async fn encode_full_pane_sync<W: AsyncWrite + Unpin>(
    writer: &mut W,
    sync: &FullPaneSync,
) -> io::Result<()> {
    let payload = encode_full_pane_sync_payload(sync)?;
    write_frame(writer, TAG_FULL_PANE_SYNC, &payload).await
}

pub fn decode_full_pane_sync(payload: &[u8]) -> io::Result<FullPaneSync> {
    if payload.len() < FULL_PANE_SYNC_MIN_HEADER_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "FullPaneSync too short",
        ));
    }
    let pane_id = read_u64_le(payload, 0)?;
    let generation = read_u64_le(payload, 8)?;
    let cols = read_u16_le(payload, 16)?;
    let rows = read_u16_le(payload, 18)?;
    let total_cells = cols as usize * rows as usize;
    if total_cells > MAX_GRID_CELLS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("grid too large: {cols}x{rows} = {total_cells} cells (max {MAX_GRID_CELLS})"),
        ));
    }
    let cursor_line = read_i16_le(payload, 20)?;
    let cursor_col = read_u16_le(payload, 22)?;
    let cursor_shape = payload[24];
    let mode_flags = payload[25];
    let (scrollback_rows, title, sections) =
        read_full_pane_sync_mandatory_sections(payload, FULL_PANE_SYNC_MIN_HEADER_LEN)?;
    let sb_expected = scrollback_rows as usize * cols as usize;
    let scrollback = sm_decode_cells_vec(sections.scrollback, sb_expected)?;
    let cells = sm_decode_cells_vec(sections.viewport, total_cells)?;
    let mut extra_offset = sections.extra_offset;
    let grapheme_extras = decode_grapheme_extras(payload, &mut extra_offset);
    let hyperlink_extras = decode_hyperlink_extras(payload, &mut extra_offset);
    let cwd = decode_cwd(payload, &mut extra_offset);

    Ok(FullPaneSync {
        pane_id,
        generation,
        cols,
        rows,
        cursor_line,
        cursor_col,
        cursor_shape,
        mode_flags,
        title: title.to_string(),
        scrollback,
        scrollback_rows,
        cells,
        grapheme_extras,
        hyperlink_extras,
        cwd,
    })
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

/// Read one frame from an async reader.
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

#[cfg(test)]
mod tests {
    use super::*;

    // ─── SM encoder/decoder roundtrip tests ─────────────────────────

    #[test]
    fn sm_roundtrip_blank_line() {
        // 80 default cells → should use a single Repeat opcode
        let cells: Vec<PackedCell> = vec![PackedCell::default(); 80];
        let encoded = sm_encode_cells(&cells);
        let mut decoded = vec![PackedCell::default(); 80];
        let n = decode_sm_cells(&encoded, &mut decoded).unwrap();
        assert_eq!(n, 80);
        assert_eq!(decoded, cells);
        // Should be very compact: Reset(0 if already default) + Repeat + End
        assert!(
            encoded.len() < 20,
            "blank line encoded as {}B",
            encoded.len()
        );
    }

    #[test]
    fn sm_roundtrip_mixed_attributes() {
        let mut cells = Vec::new();
        // 3 cells with color A
        for _ in 0..3 {
            let mut c = PackedCell::with_ch('A');
            c.fg = PackedColor::rgb(255, 0, 0);
            c.bg = PackedColor::named(0);
            cells.push(c);
        }
        // 2 cells with color B + bold
        for _ in 0..2 {
            let mut c = PackedCell::with_ch('B');
            c.fg = PackedColor::named(7);
            c.bg = PackedColor::rgb(0, 0, 128);
            c.flags = FLAG_BOLD.to_le_bytes();
            cells.push(c);
        }
        // 1 cell back to default
        cells.push(PackedCell::with_ch('C'));

        let encoded = sm_encode_cells(&cells);
        let mut decoded = vec![PackedCell::default(); cells.len()];
        let n = decode_sm_cells(&encoded, &mut decoded).unwrap();
        assert_eq!(n, cells.len());
        assert_eq!(decoded, cells);
    }

    #[test]
    fn sm_roundtrip_repeat_run() {
        // 100 identical non-default cells → Repeat
        let mut cell = PackedCell::with_ch('X');
        cell.fg = PackedColor::indexed(196);
        let cells: Vec<PackedCell> = vec![cell; 100];
        let encoded = sm_encode_cells(&cells);
        let mut decoded = vec![PackedCell::default(); 100];
        let n = decode_sm_cells(&encoded, &mut decoded).unwrap();
        assert_eq!(n, 100);
        assert_eq!(decoded, cells);
    }

    #[test]
    fn sm_roundtrip_all_color_types() {
        let mut cells = Vec::new();
        // Named color
        let mut c = PackedCell::with_ch('N');
        c.fg = PackedColor::named(NAMED_RED);
        c.bg = PackedColor::named(NAMED_BACKGROUND);
        cells.push(c);
        // RGB color
        let mut c = PackedCell::with_ch('R');
        c.fg = PackedColor::rgb(128, 64, 32);
        c.bg = PackedColor::rgb(0, 0, 0);
        cells.push(c);
        // Indexed color
        let mut c = PackedCell::with_ch('I');
        c.fg = PackedColor::indexed(200);
        c.bg = PackedColor::indexed(50);
        cells.push(c);

        let encoded = sm_encode_cells(&cells);
        let mut decoded = vec![PackedCell::default(); 3];
        let n = decode_sm_cells(&encoded, &mut decoded).unwrap();
        assert_eq!(n, 3);
        assert_eq!(decoded, cells);
    }

    #[test]
    fn sm_roundtrip_all_flags() {
        let flags_to_test = [
            FLAG_WIDE_CHAR,
            FLAG_BOLD,
            FLAG_ITALIC,
            FLAG_UNDERLINE,
            FLAG_INVERSE,
            FLAG_DIM,
            FLAG_STRIKEOUT,
            FLAG_HIDDEN,
            FLAG_UNDERLINE | FLAG_UNDERLINE_DOUBLE,
            FLAG_UNDERLINE | FLAG_UNDERLINE_CURLY,
        ];
        let mut cells = Vec::new();
        for (i, &f) in flags_to_test.iter().enumerate() {
            let mut c = PackedCell::with_ch(char::from(b'a' + i as u8));
            c.flags = f.to_le_bytes();
            cells.push(c);
        }
        let encoded = sm_encode_cells(&cells);
        let mut decoded = vec![PackedCell::default(); cells.len()];
        let n = decode_sm_cells(&encoded, &mut decoded).unwrap();
        assert_eq!(n, cells.len());
        assert_eq!(decoded, cells);
    }

    #[test]
    fn sm_roundtrip_cjk_wide_char() {
        let mut cells = Vec::new();
        // Wide char cell
        let mut c = PackedCell::with_ch('中');
        c.fg = PackedColor::indexed(196);
        c.flags = FLAG_WIDE_CHAR.to_le_bytes();
        cells.push(c);
        // Spacer cell
        let s = PackedCell {
            ch_bytes: [0; 4],
            fg: PackedColor::indexed(196),
            flags: FLAG_WIDE_CHAR_SPACER.to_le_bytes(),
            ..PackedCell::default()
        };
        cells.push(s);

        let encoded = sm_encode_cells(&cells);
        let mut decoded = vec![PackedCell::default(); 2];
        let n = decode_sm_cells(&encoded, &mut decoded).unwrap();
        assert_eq!(n, 2);
        assert_eq!(decoded, cells);
    }

    #[test]
    fn sm_compression_ratio_blank() {
        // 200x50 terminal of blank cells — should compress >99%
        let cells: Vec<PackedCell> = vec![PackedCell::default(); 200 * 50];
        let raw_size = cells.len() * PACKED_CELL_SIZE;
        let compressed = sm_encode_cells(&cells);
        assert!(
            compressed.len() < raw_size / 100,
            "SM compressed {}B vs raw {}B — ratio {:.1}%",
            compressed.len(),
            raw_size,
            compressed.len() as f64 / raw_size as f64 * 100.0
        );
    }

    #[test]
    fn sm_compression_better_than_shell_line() {
        // Simulate 80-col shell blank line: all default cells
        let cells: Vec<PackedCell> = vec![PackedCell::default(); 80];
        let encoded = sm_encode_cells(&cells);
        // Per plan: 1120B → ~8B (99%)
        assert!(
            encoded.len() < 20,
            "shell blank line: {}B (raw 1120B)",
            encoded.len()
        );
    }

    #[test]
    fn sm_truncated_opcode() {
        // Just a SetFg opcode with missing payload
        let data = [OP_SET_FG, 0x01]; // needs 4 more bytes
        let mut cells = [PackedCell::default(); 1];
        assert!(decode_sm_cells(&data, &mut cells).is_err());
    }

    #[test]
    fn sm_unknown_opcode() {
        let data = [0xFE]; // unknown opcode (not 0xFF which is End)
        let mut cells = [PackedCell::default(); 1];
        assert!(decode_sm_cells(&data, &mut cells).is_err());
    }

    #[test]
    fn sm_short_run_not_repeated() {
        // 2 identical cells followed by a different one — should NOT use Repeat (threshold is 3)
        let mut cells = vec![PackedCell::with_ch('A'); 2];
        cells.push(PackedCell::with_ch('B'));
        let encoded = sm_encode_cells(&cells);
        let mut decoded = vec![PackedCell::default(); 3];
        let n = decode_sm_cells(&encoded, &mut decoded).unwrap();
        assert_eq!(n, 3);
        assert_eq!(decoded, cells);
        // Verify no Repeat opcode was used
        assert!(!encoded.contains(&OP_REPEAT));
    }

    #[test]
    fn sm_reset_opcode() {
        // Non-default cell followed by default cell — should use Reset opcode
        let mut cells = Vec::new();
        let mut c = PackedCell::with_ch('X');
        c.fg = PackedColor::rgb(255, 0, 0);
        c.bg = PackedColor::rgb(0, 255, 0);
        c.flags = FLAG_BOLD.to_le_bytes();
        cells.push(c);
        cells.push(PackedCell::with_ch('D')); // default attrs

        let encoded = sm_encode_cells(&cells);
        let mut decoded = vec![PackedCell::default(); 2];
        let n = decode_sm_cells(&encoded, &mut decoded).unwrap();
        assert_eq!(n, 2);
        assert_eq!(decoded, cells);
        // Verify Reset opcode was used (saves bytes vs SetFg+SetBg+SetFlags)
        assert!(encoded.contains(&OP_RESET));
    }

    // ─── CellDelta SM roundtrip ─────────────────────────────────────

    #[test]
    fn cell_delta_sm_roundtrip() {
        // Build cells for one region
        let cells = vec![
            {
                let mut c = PackedCell::with_ch('A');
                c.fg = PackedColor::named(7);
                c.bg = PackedColor::named(0);
                c
            },
            {
                let mut c = PackedCell::with_ch('B');
                c.fg = PackedColor::rgb(255, 0, 0);
                c.bg = PackedColor::named(0);
                c.flags = FLAG_BOLD.to_le_bytes();
                c
            },
            {
                let mut c = PackedCell::with_ch('C');
                c.fg = PackedColor::indexed(196);
                c.bg = PackedColor::named(0);
                c
            },
        ];

        let mut buf = Vec::new();
        encode_cell_delta_streaming_framed(
            &mut buf,
            42,
            100,
            5,
            10,
            0,
            0,
            80,
            &[(5, 10, 12)],
            |_line, _left, _right, enc| {
                for c in &cells {
                    enc.push_cell(c);
                }
            },
        )
        .unwrap();

        // Skip frame header (tag + len = 5 bytes) to get payload
        let payload = buf[5..].to_vec();
        let delta = decode_cell_delta_borrowed(payload).unwrap();
        assert_eq!(delta.pane_id, 42);
        assert_eq!(delta.generation, 100);
        assert_eq!(delta.cols, 80);
        assert_eq!(delta.regions.len(), 1);
        assert_eq!(delta.regions[0].line, 5);
        assert_eq!(delta.regions[0].left, 10);
        assert_eq!(delta.regions[0].right, 12);

        // Decode the SM data for the region
        let sm_data = delta.sm_data(0);
        let mut decoded = vec![PackedCell::default(); 3];
        let n = decode_sm_cells(sm_data, &mut decoded).unwrap();
        assert_eq!(n, 3);
        assert_eq!(decoded[0].ch(), 'A');
        assert_eq!(decoded[1].ch(), 'B');
        assert_eq!(decoded[1].flags_u16(), FLAG_BOLD);
        assert_eq!(decoded[2].ch(), 'C');
        assert_eq!(decoded[2].fg, PackedColor::indexed(196));
    }

    // ─── FullPaneSync SM roundtrip ──────────────────────────────────

    #[test]
    fn full_pane_sync_roundtrip() {
        let blank = PackedCell::default();
        let cells: Vec<PackedCell> = vec![blank; 80 * 24];
        let sync = FullPaneSync {
            pane_id: 1,
            generation: 50,
            cols: 80,
            rows: 24,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            title: "bash".to_string(),
            scrollback: Vec::new(),
            scrollback_rows: 0,
            cells,
            grapheme_extras: GraphemeExtras::new(),
            hyperlink_extras: HyperlinkExtras::new(),
            cwd: None,
        };
        let payload = encode_full_pane_sync_payload(&sync).unwrap();
        // SM should compress blank cells significantly
        assert!(payload.len() < 80 * 24 * PACKED_CELL_SIZE);
        let decoded = decode_full_pane_sync(&payload).unwrap();
        assert_eq!(decoded.pane_id, 1);
        assert_eq!(decoded.cols, 80);
        assert_eq!(decoded.rows, 24);
        assert_eq!(decoded.cells.len(), 80 * 24);
        assert_eq!(decoded.title, "bash");
    }

    #[test]
    fn full_pane_sync_with_scrollback() {
        let mut sb = Vec::new();
        for i in 0..3u8 {
            for _ in 0..10 {
                let mut c = PackedCell::with_ch(char::from(b'0' + i));
                c.fg = PackedColor::indexed(i);
                sb.push(c);
            }
        }
        let cells: Vec<PackedCell> = vec![PackedCell::default(); 10 * 5];
        let sync = FullPaneSync {
            pane_id: 2,
            generation: 10,
            cols: 10,
            rows: 5,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            title: "test".to_string(),
            scrollback: sb.clone(),
            scrollback_rows: 3,
            cells,
            grapheme_extras: GraphemeExtras::new(),
            hyperlink_extras: HyperlinkExtras::new(),
            cwd: None,
        };
        let payload = encode_full_pane_sync_payload(&sync).unwrap();
        let decoded = decode_full_pane_sync(&payload).unwrap();
        assert_eq!(decoded.scrollback_rows, 3);
        assert_eq!(decoded.scrollback.len(), 30);
        assert_eq!(decoded.scrollback, sb);
    }

    #[test]
    fn full_pane_sync_rejects_truncated_mandatory_sections() {
        let sync = FullPaneSync {
            pane_id: 7,
            generation: 12,
            cols: 4,
            rows: 2,
            cursor_line: 1,
            cursor_col: 2,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            title: "pane".to_string(),
            scrollback: vec![PackedCell::default(); 4],
            scrollback_rows: 1,
            cells: vec![PackedCell::default(); 8],
            grapheme_extras: GraphemeExtras::new(),
            hyperlink_extras: HyperlinkExtras::new(),
            cwd: None,
        };
        let payload = encode_full_pane_sync_payload(&sync).unwrap();

        let truncated_title = &payload[..27];
        assert_eq!(
            decode_full_pane_sync(truncated_title).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );

        let truncated_scrollback = &payload[..payload.len() - 5];
        assert_eq!(
            decode_full_pane_sync(truncated_scrollback)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn full_pane_sync_ignores_truncated_optional_extras() {
        let mut sync = FullPaneSync {
            pane_id: 9,
            generation: 99,
            cols: 2,
            rows: 1,
            cursor_line: 0,
            cursor_col: 1,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            title: "links".to_string(),
            scrollback: Vec::new(),
            scrollback_rows: 0,
            cells: vec![PackedCell::with_ch('A'), PackedCell::with_ch('B')],
            grapheme_extras: GraphemeExtras::new(),
            hyperlink_extras: HyperlinkExtras::new(),
            cwd: None,
        };
        sync.grapheme_extras.push(0, "é");
        sync.hyperlink_extras.cell_links.push((1, 3));
        sync.hyperlink_extras
            .link_map
            .push((3, "https://example.test".to_string()));

        let mut framed = Vec::new();
        encode_full_pane_sync_framed(&mut framed, &sync).unwrap();
        let payload = &framed[5..];

        let mut grapheme_only = sync.clone();
        grapheme_only.hyperlink_extras = HyperlinkExtras::new();
        let grapheme_only_payload = encode_full_pane_sync_payload(&grapheme_only).unwrap();
        // Cut 3 bytes to truncate into the grapheme extras section
        // (payload has 2-byte CWD trailer after grapheme extras)
        let grapheme_cut = &grapheme_only_payload[..grapheme_only_payload.len() - 3];
        let decoded = decode_full_pane_sync(grapheme_cut).unwrap();
        assert_eq!(decoded.pane_id, sync.pane_id);
        assert_eq!(decoded.cells, sync.cells);
        assert_eq!(decoded.scrollback, sync.scrollback);
        assert!(decoded.grapheme_extras.0.is_empty());
        assert!(decoded.hyperlink_extras.cell_links.is_empty());
        assert!(decoded.hyperlink_extras.link_map.is_empty());

        let hyperlink_cut = &payload[..payload.len() - 3];
        let decoded = decode_full_pane_sync(hyperlink_cut).unwrap();
        assert_eq!(decoded.pane_id, sync.pane_id);
        assert_eq!(decoded.cells, sync.cells);
        assert_eq!(decoded.grapheme_extras.0.len(), 1);
        assert_eq!(decoded.hyperlink_extras.cell_links, vec![(1, 3)]);
        assert!(decoded.hyperlink_extras.link_map.is_empty());
    }

    // ─── Frame-level roundtrip ──────────────────────────────────────

    #[tokio::test]
    async fn frame_roundtrip_client_msg() {
        let msg = ClientMessage::Input {
            pane_id: 1,
            data: b"hello".to_vec(),
        };
        let mut buf = Vec::new();
        encode_client_msg(&mut buf, &msg).await.unwrap();
        let frame = read_frame(&mut &buf[..]).await.unwrap();
        match frame {
            Frame::ClientMsg(ClientMessage::Input { pane_id, data }) => {
                assert_eq!(pane_id, 1);
                assert_eq!(data, b"hello");
            }
            _ => panic!("wrong frame type"),
        }
    }

    #[tokio::test]
    async fn frame_roundtrip_cell_delta_sm() {
        let cell = PackedCell::with_ch('X');
        let mut buf = Vec::new();
        encode_cell_delta_streaming_framed(
            &mut buf,
            1,
            1,
            0,
            0,
            0,
            0,
            80,
            &[(0, 0, 0)],
            |_line, _left, _right, enc| {
                enc.push_cell(&cell);
            },
        )
        .unwrap();

        let frame = read_frame(&mut &buf[..]).await.unwrap();
        match frame {
            Frame::CellDelta(d) => {
                let sm = d.sm_data(0);
                let mut decoded = [PackedCell::default(); 1];
                let n = decode_sm_cells(sm, &mut decoded).unwrap();
                assert_eq!(n, 1);
                assert_eq!(decoded[0].ch(), 'X');
            }
            _ => panic!("wrong frame type"),
        }
    }

    #[test]
    fn sm_repeat_exceeding_u16_max() {
        // 70000 identical cells — exceeds u16::MAX (65535), must not overflow
        let count = 70_000usize;
        let cell = PackedCell::with_ch(' ');
        let cells: Vec<PackedCell> = vec![cell; count];
        let encoded = sm_encode_cells(&cells);
        let mut decoded = vec![PackedCell::default(); count];
        let n = decode_sm_cells(&encoded, &mut decoded).unwrap();
        assert_eq!(n, count);
        assert_eq!(decoded, cells);
    }

    #[test]
    fn decode_cell_delta_rejects_inverted_region_bounds() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&1u64.to_le_bytes());
        payload.extend_from_slice(&2u64.to_le_bytes());
        payload.extend_from_slice(&0i16.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.push(0);
        payload.push(0);
        payload.extend_from_slice(&80u16.to_le_bytes());
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.extend_from_slice(&3u16.to_le_bytes());
        payload.extend_from_slice(&5u16.to_le_bytes());
        payload.extend_from_slice(&4u16.to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes());

        let err = decode_cell_delta_borrowed(payload).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("left > right"));
    }

    #[test]
    fn decode_cell_delta_rejects_truncated_region_data() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&1u64.to_le_bytes());
        payload.extend_from_slice(&2u64.to_le_bytes());
        payload.extend_from_slice(&0i16.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.push(0);
        payload.push(0);
        payload.extend_from_slice(&80u16.to_le_bytes());
        payload.extend_from_slice(&1u16.to_le_bytes());
        payload.extend_from_slice(&3u16.to_le_bytes());
        payload.extend_from_slice(&4u16.to_le_bytes());
        payload.extend_from_slice(&6u16.to_le_bytes());
        payload.extend_from_slice(&4u32.to_le_bytes());
        payload.extend_from_slice(&[OP_END]);

        let err = decode_cell_delta_borrowed(payload).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("truncated SM data"));
    }

    #[tokio::test]
    async fn read_client_hello_rejects_bad_magic() {
        let mut hello = build_client_hello(&ClientHello {
            session_name: "main".to_string(),
            width: 1024,
            height: 768,
            cell_width: 8.0,
            cell_height: 16.0,
        })
        .unwrap();
        hello[0..4].copy_from_slice(b"NOPE");
        let err = read_client_hello(&mut &hello[..]).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("bad magic"));
    }

    #[tokio::test]
    async fn read_client_hello_rejects_wrong_wire_version() {
        let mut hello = build_client_hello(&ClientHello {
            session_name: "main".to_string(),
            width: 1024,
            height: 768,
            cell_width: 8.0,
            cell_height: 16.0,
        })
        .unwrap();
        hello[8] = WIRE_PROTOCOL_VERSION + 1;
        let err = read_client_hello(&mut &hello[..]).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("incompatible wire protocol"));
    }

    #[tokio::test]
    async fn read_client_hello_rejects_invalid_utf8_session_name() {
        let mut hello = build_client_hello(&ClientHello {
            session_name: "main".to_string(),
            width: 1024,
            height: 768,
            cell_width: 8.0,
            cell_height: 16.0,
        })
        .unwrap();
        let name_start = CLIENT_HELLO_HEADER_LEN;
        hello[name_start..name_start + 4].copy_from_slice(&[0xff, 0xfe, 0xfd, 0xfc]);
        let err = read_client_hello(&mut &hello[..]).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("invalid session name utf8"));
    }

    #[tokio::test]
    async fn read_client_hello_rejects_invalid_cell_width() {
        let mut hello = build_client_hello(&ClientHello {
            session_name: "main".to_string(),
            width: 1024,
            height: 768,
            cell_width: 8.0,
            cell_height: 16.0,
        })
        .unwrap();
        let width_offset = CLIENT_HELLO_HEADER_LEN + 4 + 4 + 4;
        hello[width_offset..width_offset + 4].copy_from_slice(&f32::NAN.to_bits().to_le_bytes());

        let err = read_client_hello(&mut &hello[..]).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("invalid cell_width"));
    }

    #[tokio::test]
    async fn read_client_hello_rejects_zero_viewport_width() {
        let mut hello = build_client_hello(&ClientHello {
            session_name: "main".to_string(),
            width: 1024,
            height: 768,
            cell_width: 8.0,
            cell_height: 16.0,
        })
        .unwrap();
        let width_offset = CLIENT_HELLO_HEADER_LEN + 4;
        hello[width_offset..width_offset + 4].copy_from_slice(&0u32.to_le_bytes());

        let err = read_client_hello(&mut &hello[..]).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("invalid viewport width"));
    }

    #[tokio::test]
    async fn read_server_hello_rejects_bad_magic() {
        let mut hello = [0u8; SERVER_HELLO_LEN];
        hello[0..4].copy_from_slice(b"NOPE");
        hello[4..8].copy_from_slice(&parse_pkg_version().to_le_bytes());

        let err = read_server_hello(&mut &hello[..]).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("bad magic"));
    }

    #[tokio::test]
    async fn read_frame_rejects_unknown_tag() {
        let mut frame = vec![0x7f];
        frame.extend_from_slice(&1u32.to_le_bytes());
        frame.push(0);
        let err = read_frame(&mut &frame[..]).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("unknown frame tag"));
    }

    #[tokio::test]
    async fn read_frame_rejects_truncated_header() {
        let frame = [TAG_SERVER_MSG, 0, 0, 0];
        let err = read_frame(&mut &frame[..]).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn read_frame_rejects_truncated_payload() {
        let msg = ServerMessage::ServerShutdown;
        let payload = rmp_serde::to_vec(&msg).unwrap();
        let mut frame = build_frame(TAG_SERVER_MSG, &payload).unwrap();
        frame.pop();
        let err = read_frame(&mut &frame[..]).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn read_frame_rejects_oversized_payload() {
        let mut frame = vec![TAG_SERVER_MSG];
        frame.extend_from_slice(&(MAX_FRAME_LEN + 1).to_le_bytes());
        let err = read_frame(&mut &frame[..]).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("frame too large"));
    }

    fn sample_layout_state() -> LayoutState {
        LayoutState {
            workspaces: vec![WorkspaceState {
                columns: vec![ColumnState {
                    tiles: vec![
                        TileState {
                            pane_id: 10,
                            weight: 1.0,
                        },
                        TileState {
                            pane_id: 11,
                            weight: 2.0,
                        },
                    ],
                    active_tile_idx: 1,
                    width_proportion: 0.6,
                    width_fixed_px: Some(480.0),
                }],
                active_column_idx: 0,
            }],
            active_workspace_idx: 0,
        }
    }

    fn sample_session_info() -> SessionInfo {
        SessionInfo {
            name: "main".to_string(),
            running: true,
            pane_count: 3,
            client_count: 2,
        }
    }

    fn sample_session_detail_info() -> SessionDetailInfo {
        SessionDetailInfo {
            name: "main".to_string(),
            running: true,
            pane_count: 3,
            client_count: 2,
            workspace_count: 2,
            active_workspace: 1,
        }
    }

    fn sample_pane_detail_info() -> PaneDetailInfo {
        PaneDetailInfo {
            pane_id: 42,
            cols: 120,
            rows: 40,
            title: "shell".to_string(),
            cwd: Some("/tmp/project".to_string()),
            is_active: true,
            workspace_idx: 1,
            column_idx: 2,
            tile_idx: 0,
        }
    }

    fn sample_template_info() -> TemplateInfo {
        TemplateInfo {
            name: "dev".to_string(),
            description: Some("Dev workspace".to_string()),
            workspace_count: 2,
            total_panes: 5,
        }
    }

    #[tokio::test]
    async fn frame_roundtrip_server_message_variants() {
        let layout = sample_layout_state();
        let session_info = sample_session_info();
        let session_detail = sample_session_detail_info();
        let pane_detail = sample_pane_detail_info();
        let template_info = sample_template_info();

        let cases = vec![
            ServerMessage::StateSync {
                layout: layout.clone(),
                pane_ids: vec![10, 11, 12],
            },
            ServerMessage::LayoutUpdate {
                layout: layout.clone(),
            },
            ServerMessage::PaneCreated {
                pane_id: 42,
                column_idx: 2,
                cols: 80,
                rows: 24,
            },
            ServerMessage::PaneClosed { pane_id: 42 },
            ServerMessage::ServerShutdown,
            ServerMessage::ClipboardStore {
                data: "copied text".to_string(),
            },
            ServerMessage::SessionList {
                sessions: vec![session_info.clone()],
            },
            ServerMessage::SessionSwitched {
                session_name: "main".to_string(),
            },
            ServerMessage::SessionKilled {
                session_name: "old".to_string(),
            },
            ServerMessage::Error {
                message: "boom".to_string(),
            },
            ServerMessage::Bell { pane_id: 7 },
            ServerMessage::CommandCompleted {
                pane_id: 7,
                duration_secs: 3,
                exit_code: Some(1),
            },
            ServerMessage::ImagePlacement {
                pane_id: 9,
                image_id: 5,
                col: 3,
                row: 4,
                width_cells: 6,
                height_cells: 7,
                pixel_width: 240,
                pixel_height: 112,
                format: "png".to_string(),
                data: vec![1, 2, 3, 4],
            },
            ServerMessage::SessionInfoReply {
                info: session_detail.clone(),
            },
            ServerMessage::PaneListReply {
                panes: vec![pane_detail.clone()],
            },
            ServerMessage::CommandResult {
                success: true,
                message: "ok".to_string(),
                pane_id: Some(42),
            },
            ServerMessage::LayoutReply {
                layout: layout.clone(),
                session_name: "main".to_string(),
            },
            ServerMessage::TemplateApplied {
                session_name: "main".to_string(),
            },
            ServerMessage::TemplateList {
                templates: vec![template_info.clone()],
            },
            ServerMessage::TemplateSaved {
                template_name: "dev".to_string(),
            },
        ];

        for msg in cases {
            let expected = format!("{msg:?}");
            let frame = frame_server_msg(&msg).expect("server frame");
            match read_frame(&mut &frame[..]).await.unwrap() {
                Frame::ServerMsg(decoded) => assert_eq!(format!("{decoded:?}"), expected),
                other => panic!("expected server message frame, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn frame_roundtrip_client_message_variants() {
        let cases = vec![
            ClientMessage::Input {
                pane_id: 42,
                data: vec![0x1b, b'[', b'A'],
            },
            ClientMessage::CreatePane,
            ClientMessage::SplitDown,
            ClientMessage::ClosePane { pane_id: 42 },
            ClientMessage::FocusLeft,
            ClientMessage::FocusRight,
            ClientMessage::FocusUp,
            ClientMessage::FocusDown,
            ClientMessage::MovePaneLeft,
            ClientMessage::MovePaneRight,
            ClientMessage::Resize {
                cols: 80,
                rows: 24,
                width: 1280,
                height: 720,
                cell_width: 8.0,
                cell_height: 16.0,
            },
            ClientMessage::SetColumnWidth {
                proportion: 0.6,
                fixed_px: Some(320.0),
            },
            ClientMessage::AdjustColumnSplit { delta: -0.15 },
            ClientMessage::EqualizeColumnSplit,
            ClientMessage::Attach,
            ClientMessage::Detach,
            ClientMessage::Ack { generation: 7 },
            ClientMessage::MouseInput {
                pane_id: 42,
                button: 1,
                col: 12,
                row: 6,
                pressed: true,
                modifiers: 2,
            },
            ClientMessage::SwitchWorkspace { workspace_idx: 1 },
            ClientMessage::ConsumeIntoColumn,
            ClientMessage::ExpelFromColumn,
            ClientMessage::ListSessions { all: false },
            ClientMessage::KillSession {
                session_name: "old".to_string(),
            },
            ClientMessage::KillServer,
            ClientMessage::SwitchSession {
                session_name: "main".to_string(),
            },
            ClientMessage::SetTileWeights {
                column_idx: 1,
                top_tile_idx: 0,
                top_weight: 1.5,
                bottom_weight: 2.5,
            },
            ClientMessage::AdjustColumnSplitAt {
                column_idx: 2,
                delta: 0.25,
            },
            ClientMessage::FocusChange { focused: true },
            ClientMessage::FocusPane { pane_id: 99 },
            ClientMessage::SendKeys {
                session_name: "main".to_string(),
                pane_id: 42,
                keys: vec![b'l', b's', b'\n'],
            },
            ClientMessage::SaveTemplate {
                template_name: "dev".to_string(),
                session_name: "main".to_string(),
            },
            ClientMessage::RunCommand {
                session_name: "main".to_string(),
                command: "ls".to_string(),
                cwd: Some("/tmp".to_string()),
            },
            ClientMessage::GetSessionInfo {
                session_name: "main".to_string(),
            },
            ClientMessage::ListPanes {
                session_name: "main".to_string(),
            },
            ClientMessage::FocusPaneById {
                session_name: "main".to_string(),
                pane_id: 7,
            },
            ClientMessage::ClosePaneById {
                session_name: "main".to_string(),
                pane_id: 8,
            },
            ClientMessage::CreatePaneIn {
                session_name: "main".to_string(),
            },
            ClientMessage::GetLayout {
                session_name: "main".to_string(),
            },
            ClientMessage::ApplyTemplate {
                template_name: "dev".to_string(),
                session_name: "main".to_string(),
            },
            ClientMessage::ListTemplates,
            ClientMessage::ListSessions { all: true },
        ];

        for msg in cases {
            let expected = format!("{msg:?}");
            let mut frame = Vec::new();
            encode_client_msg(&mut frame, &msg).await.unwrap();
            match read_frame(&mut &frame[..]).await.unwrap() {
                Frame::ClientMsg(decoded) => assert_eq!(format!("{decoded:?}"), expected),
                other => panic!("expected client message frame, got {other:?}"),
            }
        }
    }
}
