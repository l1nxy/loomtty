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
mod tests;
