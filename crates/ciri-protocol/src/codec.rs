use crate::message::*;
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
    assert!(parts.len() == 3, "CARGO_PKG_VERSION must be major.minor.patch");
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
                unpack_version(peer), unpack_version(local),
            ),
        ));
    }
    if peer == local {
        Ok(VersionCompat::Exact(unpack_version(peer)))
    } else if peer_minor != local_minor {
        Ok(VersionCompat::MinorMismatch {
            peer: unpack_version(peer), local: unpack_version(local),
        })
    } else {
        Ok(VersionCompat::PatchMismatch {
            peer: unpack_version(peer), local: unpack_version(local),
        })
    }
}

/// Client viewport info sent in the hello message.
#[derive(Debug, Clone)]
pub struct ClientViewport {
    pub width: u32,
    pub height: u32,
    pub cell_width: f32,
    pub cell_height: f32,
}

/// ClientHello: [magic(4)][version(4)][width(4)][height(4)][cell_w(4)][cell_h(4)] = 24 bytes
pub async fn write_client_hello<W: AsyncWrite + Unpin>(
    writer: &mut W,
    viewport: &ClientViewport,
) -> io::Result<()> {
    let mut buf = [0u8; 24];
    buf[0..4].copy_from_slice(&HANDSHAKE_MAGIC);
    buf[4..8].copy_from_slice(&parse_pkg_version().to_le_bytes());
    buf[8..12].copy_from_slice(&viewport.width.to_le_bytes());
    buf[12..16].copy_from_slice(&viewport.height.to_le_bytes());
    buf[16..20].copy_from_slice(&viewport.cell_width.to_bits().to_le_bytes());
    buf[20..24].copy_from_slice(&viewport.cell_height.to_bits().to_le_bytes());
    writer.write_all(&buf).await?;
    writer.flush().await
}

/// Server reads ClientHello. Returns version compat + viewport.
pub async fn read_client_hello<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> io::Result<(VersionCompat, ClientViewport)> {
    let mut buf = [0u8; 24];
    reader.read_exact(&mut buf).await?;
    if buf[0..4] != HANDSHAKE_MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad magic bytes"));
    }
    let peer_ver = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    let compat = check_version(peer_ver)?;
    let viewport = ClientViewport {
        width: u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]),
        height: u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]),
        cell_width: f32::from_bits(u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]])),
        cell_height: f32::from_bits(u32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]])),
    };
    // Validate viewport values
    if !viewport.cell_width.is_finite() || viewport.cell_width <= 0.0 || viewport.cell_width > 200.0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData,
            format!("invalid cell_width: {}", viewport.cell_width)));
    }
    if !viewport.cell_height.is_finite() || viewport.cell_height <= 0.0 || viewport.cell_height > 200.0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData,
            format!("invalid cell_height: {}", viewport.cell_height)));
    }
    if viewport.width == 0 || viewport.width > 16384 {
        return Err(io::Error::new(io::ErrorKind::InvalidData,
            format!("invalid viewport width: {}", viewport.width)));
    }
    if viewport.height == 0 || viewport.height > 16384 {
        return Err(io::Error::new(io::ErrorKind::InvalidData,
            format!("invalid viewport height: {}", viewport.height)));
    }
    Ok((compat, viewport))
}

/// ServerHello: [magic(4)][version(4)] = 8 bytes
pub async fn write_server_hello<W: AsyncWrite + Unpin>(writer: &mut W) -> io::Result<()> {
    let mut buf = [0u8; 8];
    buf[0..4].copy_from_slice(&HANDSHAKE_MAGIC);
    buf[4..8].copy_from_slice(&parse_pkg_version().to_le_bytes());
    writer.write_all(&buf).await?;
    // Don't flush here — caller will send StateSync frames right after
    Ok(())
}

/// Client reads ServerHello.
pub async fn read_server_hello<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<VersionCompat> {
    let mut buf = [0u8; 8];
    reader.read_exact(&mut buf).await?;
    if buf[0..4] != HANDSHAKE_MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad magic bytes"));
    }
    let peer_ver = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    check_version(peer_ver)
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

fn read_packed_cell(buf: &[u8], off: usize) -> io::Result<PackedCell> {
    let end = off + PACKED_CELL_SIZE;
    let slice = buf.get(off..end)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "truncated cell"))?;
    Ok(*bytemuck::from_bytes::<PackedCell>(slice))
}

/// Zero-copy: interpret a byte slice as a slice of PackedCells.
pub fn cells_from_bytes(buf: &[u8]) -> &[PackedCell] {
    bytemuck::cast_slice(buf)
}

/// Zero-copy: view a slice of PackedCells as bytes.
pub fn cells_to_bytes(cells: &[PackedCell]) -> &[u8] {
    bytemuck::cast_slice(cells)
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
    let len = payload.len() as u32;
    let mut header = [0u8; 5];
    header[0] = tag;
    header[1..5].copy_from_slice(&len.to_le_bytes());
    writer.write_all(&header).await?;
    writer.write_all(payload).await?;
    Ok(())
}

/// Read a frame header, returning (tag, payload_length).
async fn read_frame_header<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> io::Result<(u8, u32)> {
    let mut header = [0u8; 5];
    reader.read_exact(&mut header).await?;
    let tag = header[0];
    let len = u32::from_le_bytes([header[1], header[2], header[3], header[4]]);
    if len > 16 * 1024 * 1024 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    Ok((tag, len))
}

// ─── Encode / decode ClientMessage (msgpack) ────────────────────────

pub async fn encode_client_msg<W: AsyncWrite + Unpin>(
    writer: &mut W,
    msg: &ClientMessage,
) -> io::Result<()> {
    let payload = rmp_serde::to_vec(msg)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    write_frame(writer, TAG_CLIENT_MSG, &payload).await
}

// ─── Encode / decode ServerMessage (msgpack) ────────────────────────

pub async fn encode_server_msg<W: AsyncWrite + Unpin>(
    writer: &mut W,
    msg: &ServerMessage,
) -> io::Result<()> {
    let payload = rmp_serde::to_vec(msg)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    write_frame(writer, TAG_SERVER_MSG, &payload).await
}

// ─── Encode CellDelta (custom binary) ───────────────────────────────

pub fn encode_cell_delta_payload(delta: &CellDelta) -> io::Result<Vec<u8>> {
    if delta.regions.len() > u16::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "too many regions for CellDelta (exceeds u16::MAX)",
        ));
    }
    // [u64 pane_id][u64 generation][i16 cursor_line][u16 cursor_col][u8 cursor_shape][u8 mode_flags][u16 num_regions]
    // per region: [u16 line][u16 left][u16 right][PackedCell × (right-left+1)]
    let mut buf = Vec::with_capacity(256);
    buf.extend_from_slice(&delta.pane_id.to_le_bytes());
    buf.extend_from_slice(&delta.generation.to_le_bytes());
    buf.extend_from_slice(&delta.cursor_line.to_le_bytes());
    buf.extend_from_slice(&delta.cursor_col.to_le_bytes());
    buf.push(delta.cursor_shape);
    buf.push(delta.mode_flags);
    buf.extend_from_slice(&(delta.regions.len() as u16).to_le_bytes());
    for region in &delta.regions {
        buf.extend_from_slice(&region.line.to_le_bytes());
        buf.extend_from_slice(&region.left.to_le_bytes());
        buf.extend_from_slice(&region.right.to_le_bytes());
        buf.extend_from_slice(cells_to_bytes(&region.cells));
    }
    Ok(buf)
}

pub async fn encode_cell_delta<W: AsyncWrite + Unpin>(
    writer: &mut W,
    delta: &CellDelta,
) -> io::Result<()> {
    let payload = encode_cell_delta_payload(delta)?;
    write_frame(writer, TAG_CELL_DELTA, &payload).await
}

pub fn decode_cell_delta(payload: &[u8]) -> io::Result<CellDelta> {
    // 8 + 8 + 2 + 2 + 1 + 1 + 2 = 24 bytes minimum
    if payload.len() < 24 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "CellDelta too short"));
    }
    let pane_id = read_u64_le(payload, 0)?;
    let generation = read_u64_le(payload, 8)?;
    let cursor_line = read_i16_le(payload, 16)?;
    let cursor_col = read_u16_le(payload, 18)?;
    let cursor_shape = payload[20];
    let mode_flags = payload[21];
    let num_regions = read_u16_le(payload, 22)? as usize;
    let mut offset = 24;
    let mut regions = Vec::with_capacity(num_regions);
    for _ in 0..num_regions {
        if offset + 6 > payload.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "truncated region header"));
        }
        let line = read_u16_le(payload, offset)?;
        let left = read_u16_le(payload, offset + 2)?;
        let right = read_u16_le(payload, offset + 4)?;
        offset += 6;
        if left > right {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid damage region: left > right",
            ));
        }
        let count = (right - left + 1) as usize;
        let cell_bytes = count * PACKED_CELL_SIZE;
        if offset + cell_bytes > payload.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "truncated cell data"));
        }
        let mut cells = Vec::with_capacity(count);
        for i in 0..count {
            let cell = read_packed_cell(payload, offset + i * PACKED_CELL_SIZE)?;
            cells.push(cell);
        }
        offset += cell_bytes;
        regions.push(DamageRegion { line, left, right, cells });
    }
    Ok(CellDelta { pane_id, generation, cursor_line, cursor_col, cursor_shape, mode_flags, regions })
}

// ─── Encode FullPaneSync (custom binary with RLE) ───────────────────

const RLE_MARKER: u8 = 0xFF;

fn rle_encode_cells(cells: &[PackedCell]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(cells.len() * PACKED_CELL_SIZE / 2);
    let mut i = 0;
    while i < cells.len() {
        let cell = cells[i];
        let mut run = 1u16;
        while (i + run as usize) < cells.len()
            && cells[i + run as usize] == cell
            && run < u16::MAX
        {
            run += 1;
        }
        if run >= 3 {
            // RLE: [0xFF][u16 count][14B cell]
            buf.push(RLE_MARKER);
            buf.extend_from_slice(&run.to_le_bytes());
            buf.extend_from_slice(bytemuck::bytes_of(&cell));
            i += run as usize;
        } else {
            // Raw cell(s)
            for _ in 0..run {
                let bytes = bytemuck::bytes_of(&cells[i]);
                // If first byte happens to be 0xFF, escape it with run=1
                if bytes[0] == RLE_MARKER {
                    buf.push(RLE_MARKER);
                    buf.extend_from_slice(&1u16.to_le_bytes());
                    buf.extend_from_slice(bytes);
                } else {
                    buf.extend_from_slice(bytes);
                }
                i += 1;
            }
        }
    }
    buf
}

fn rle_decode_cells(data: &[u8], expected_count: usize) -> io::Result<Vec<PackedCell>> {
    let mut cells = Vec::with_capacity(expected_count);
    let mut offset = 0;
    while offset < data.len() && cells.len() < expected_count {
        if data[offset] == RLE_MARKER {
            if offset + 3 + PACKED_CELL_SIZE > data.len() {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "truncated RLE"));
            }
            let count = read_u16_le(data, offset + 1)? as usize;
            let cell = read_packed_cell(data, offset + 3)?;
            let to_add = count.min(expected_count.saturating_sub(cells.len()));
            cells.extend(std::iter::repeat(cell).take(to_add));
            offset += 3 + PACKED_CELL_SIZE;
        } else {
            if offset + PACKED_CELL_SIZE > data.len() {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "truncated cell"));
            }
            let cell = read_packed_cell(data, offset)?;
            cells.push(cell);
            offset += PACKED_CELL_SIZE;
        }
    }
    Ok(cells)
}

pub fn encode_full_pane_sync_payload(sync: &FullPaneSync) -> io::Result<Vec<u8>> {
    let title_bytes = sync.title.as_bytes();
    if title_bytes.len() > u16::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "title too long for FullPaneSync (exceeds u16::MAX)",
        ));
    }
    let rle_scrollback = rle_encode_cells(&sync.scrollback);
    let rle_data = rle_encode_cells(&sync.cells);
    // Header: pane_id(8) + gen(8) + cols(2) + rows(2) + cursor(5) + mode_flags(1) + title_len(2) + sb_rows(2) + sb_data_len(4) + cell_data_len(4)
    let mut buf = Vec::with_capacity(38 + title_bytes.len() + rle_scrollback.len() + rle_data.len());

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
    buf.extend_from_slice(&sync.scrollback_rows.to_le_bytes());
    buf.extend_from_slice(&(rle_scrollback.len() as u32).to_le_bytes());
    buf.extend_from_slice(&rle_scrollback);
    buf.extend_from_slice(&(rle_data.len() as u32).to_le_bytes());
    buf.extend_from_slice(&rle_data);
    Ok(buf)
}

pub async fn encode_full_pane_sync<W: AsyncWrite + Unpin>(
    writer: &mut W,
    sync: &FullPaneSync,
) -> io::Result<()> {
    let payload = encode_full_pane_sync_payload(sync)?;
    write_frame(writer, TAG_FULL_PANE_SYNC, &payload).await
}

pub fn decode_full_pane_sync(payload: &[u8]) -> io::Result<FullPaneSync> {
    if payload.len() < 32 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "FullPaneSync too short"));
    }
    let pane_id = read_u64_le(payload, 0)?;
    let generation = read_u64_le(payload, 8)?;
    let cols = read_u16_le(payload, 16)?;
    let rows = read_u16_le(payload, 18)?;
    let total_cells = cols as usize * rows as usize;
    if total_cells > 10_000_000 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("grid too large: {cols}x{rows} = {total_cells} cells (max 10M)"),
        ));
    }
    let cursor_line = read_i16_le(payload, 20)?;
    let cursor_col = read_u16_le(payload, 22)?;
    let cursor_shape = payload[24];
    let mode_flags = payload[25];
    let title_len = read_u16_le(payload, 26)? as usize;
    let mut offset = 28;
    if offset + title_len > payload.len() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "truncated title"));
    }
    let title = String::from_utf8_lossy(&payload[offset..offset + title_len]).to_string();
    offset += title_len;

    // Scrollback lines
    if offset + 6 > payload.len() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "truncated scrollback header"));
    }
    let scrollback_rows = read_u16_le(payload, offset)?;
    offset += 2;
    let sb_data_len = read_u32_le(payload, offset)? as usize;
    offset += 4;
    if offset + sb_data_len > payload.len() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "truncated scrollback data"));
    }
    let sb_expected = scrollback_rows as usize * cols as usize;
    let scrollback = rle_decode_cells(&payload[offset..offset + sb_data_len], sb_expected)?;
    offset += sb_data_len;

    // Viewport cells
    if offset + 4 > payload.len() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "truncated cell header"));
    }
    let cell_data_len = read_u32_le(payload, offset)? as usize;
    offset += 4;
    if offset + cell_data_len > payload.len() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "truncated cell data"));
    }
    let expected = cols as usize * rows as usize;
    let cells = rle_decode_cells(&payload[offset..offset + cell_data_len], expected)?;

    Ok(FullPaneSync {
        pane_id, generation, cols, rows,
        cursor_line, cursor_col, cursor_shape, mode_flags,
        title, scrollback, scrollback_rows, cells,
    })
}

// ─── Unified frame reader ───────────────────────────────────────────

/// A decoded frame from the wire.
#[derive(Debug)]
pub enum Frame {
    ClientMsg(ClientMessage),
    ServerMsg(ServerMessage),
    CellDelta(CellDelta),
    FullPaneSync(FullPaneSync),
}

/// Read one frame from an async reader.
pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Frame> {
    let (tag, len) = read_frame_header(reader).await?;
    let mut payload = vec![0u8; len as usize];
    reader.read_exact(&mut payload).await?;

    match tag {
        TAG_CLIENT_MSG => {
            let msg: ClientMessage = rmp_serde::from_slice(&payload)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            Ok(Frame::ClientMsg(msg))
        }
        TAG_SERVER_MSG => {
            let msg: ServerMessage = rmp_serde::from_slice(&payload)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            Ok(Frame::ServerMsg(msg))
        }
        TAG_CELL_DELTA => {
            let delta = decode_cell_delta(&payload)?;
            Ok(Frame::CellDelta(delta))
        }
        TAG_FULL_PANE_SYNC => {
            let sync = decode_full_pane_sync(&payload)?;
            Ok(Frame::FullPaneSync(sync))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unknown frame tag: 0x{tag:02x}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_delta_roundtrip() {
        let delta = CellDelta {
            pane_id: 42,
            generation: 100,
            cursor_line: 5,
            cursor_col: 10,
            cursor_shape: 0,
            mode_flags: 0,
            regions: vec![
                DamageRegion {
                    line: 5,
                    left: 10,
                    right: 12,
                    cells: vec![
                        { let mut c = PackedCell::with_ch('A'); c.fg = PackedColor::named(7); c.bg = PackedColor::named(0); c },
                        { let mut c = PackedCell::with_ch('B'); c.fg = PackedColor::rgb(255, 0, 0); c.bg = PackedColor::named(0); c.flags = FLAG_BOLD.to_le_bytes(); c },
                        { let mut c = PackedCell::with_ch('C'); c.fg = PackedColor::indexed(196); c.bg = PackedColor::named(0); c },
                    ],
                },
            ],
        };
        let payload = encode_cell_delta_payload(&delta).unwrap();
        let decoded = decode_cell_delta(&payload).unwrap();
        assert_eq!(decoded.pane_id, 42);
        assert_eq!(decoded.generation, 100);
        assert_eq!(decoded.regions.len(), 1);
        assert_eq!(decoded.regions[0].line, 5);
        assert_eq!(decoded.regions[0].cells.len(), 3);
        assert_eq!(decoded.regions[0].cells[0].ch(), 'A');
        assert_eq!(decoded.regions[0].cells[1].flags_u16(), FLAG_BOLD);
    }

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
        };
        let payload = encode_full_pane_sync_payload(&sync).unwrap();
        // RLE should compress blank cells significantly
        assert!(payload.len() < 80 * 24 * PACKED_CELL_SIZE);
        let decoded = decode_full_pane_sync(&payload).unwrap();
        assert_eq!(decoded.pane_id, 1);
        assert_eq!(decoded.cols, 80);
        assert_eq!(decoded.rows, 24);
        assert_eq!(decoded.cells.len(), 80 * 24);
        assert_eq!(decoded.title, "bash");
    }

    #[test]
    fn rle_compression_ratio() {
        // 200x50 terminal of blank cells should compress well
        let blank = PackedCell::default();
        let cells: Vec<PackedCell> = vec![blank; 200 * 50];
        let raw_size = cells.len() * PACKED_CELL_SIZE;
        let compressed = rle_encode_cells(&cells);
        // Should be much smaller than raw
        assert!(compressed.len() < raw_size / 100, "RLE compressed {}B vs raw {}B", compressed.len(), raw_size);
    }

    #[tokio::test]
    async fn frame_roundtrip_client_msg() {
        let msg = ClientMessage::Input { pane_id: 1, data: b"hello".to_vec() };
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
    async fn frame_roundtrip_cell_delta() {
        let delta = CellDelta {
            pane_id: 1,
            generation: 1,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: 0,
            mode_flags: 0,
            regions: vec![DamageRegion {
                line: 0,
                left: 0,
                right: 0,
                cells: vec![PackedCell::with_ch('X')],
            }],
        };
        let mut buf = Vec::new();
        encode_cell_delta(&mut buf, &delta).await.unwrap();
        let frame = read_frame(&mut &buf[..]).await.unwrap();
        match frame {
            Frame::CellDelta(d) => {
                assert_eq!(d.regions[0].cells[0].ch(), 'X');
            }
            _ => panic!("wrong frame type"),
        }
    }
}
