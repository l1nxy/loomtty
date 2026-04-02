//! Protocol handshake: ClientHello / ServerHello wire format.

use bytes::Buf;
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

// ─── Version helpers ─────────────────────────────────────────────────

/// Pack semver "major.minor.patch" into u32: major(8).minor(8).patch(16).
const fn pack_version(major: u8, minor: u8, patch: u16) -> u32 {
    (major as u32) << 24 | (minor as u32) << 16 | patch as u32
}

pub(super) fn parse_pkg_version() -> u32 {
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
pub(super) const CLIENT_HELLO_HEADER_LEN: usize = 11;
pub const SERVER_HELLO_LEN: usize = 8;
const MAX_SESSION_NAME_LEN: usize = 255;

/// Wire protocol version. Incremented whenever the handshake or frame format
/// changes in a backward-incompatible way.
///
/// History:
///   1 = initial codec layout
///   2 = mode_flags expanded from u8 to u16 (kitty keyboard levels 1-5)
///   3 = echo_ack (u64) added to PaneFrameMeta for input prediction timing
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

pub async fn write_server_hello<W: AsyncWrite + Unpin>(writer: &mut W) -> io::Result<()> {
    let mut buf = [0u8; SERVER_HELLO_LEN];
    buf[0..4].copy_from_slice(&HANDSHAKE_MAGIC);
    buf[4..8].copy_from_slice(&parse_pkg_version().to_le_bytes());
    writer.write_all(&buf).await?;
    Ok(())
}

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
