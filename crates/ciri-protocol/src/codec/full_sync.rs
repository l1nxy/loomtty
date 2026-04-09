//! FullPaneSync encoding and decoding.

use crate::message::*;
use std::io;
use tokio::io::AsyncWrite;

use super::frame::{
    TAG_FULL_PANE_SYNC, TAG_FULL_PANE_SYNC_LZ4, finalize_frame_compression, write_frame,
};
use super::state_machine::{StateEncoder, sm_decode_cells_vec, sm_encode_cells};
use super::util::SliceCursor;

// ─── Encoding helpers (unchanged) ───────────────────────────────────

fn validate_full_pane_sync_title(title: &[u8]) -> io::Result<()> {
    if title.len() > u16::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "title too long for FullPaneSync (exceeds u16::MAX)",
        ));
    }
    Ok(())
}

fn validate_grapheme_extra(extra: &str) -> io::Result<()> {
    if extra.len() > u8::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "grapheme extra too long for FullPaneSync (exceeds u8::MAX)",
        ));
    }
    Ok(())
}

fn validate_hyperlink_uri(uri: &str) -> io::Result<()> {
    if uri.len() > u16::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "hyperlink URI too long for FullPaneSync (exceeds u16::MAX)",
        ));
    }
    Ok(())
}

fn validate_cwd(cwd: &str) -> io::Result<()> {
    if cwd.len() > u16::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "cwd too long for FullPaneSync (exceeds u16::MAX)",
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
    buf.extend_from_slice(&sync.meta.pane_id.to_le_bytes());
    buf.extend_from_slice(&sync.meta.generation.to_le_bytes());
    buf.extend_from_slice(&sync.cols.to_le_bytes());
    buf.extend_from_slice(&sync.rows.to_le_bytes());
    buf.extend_from_slice(&sync.meta.cursor_line.to_le_bytes());
    buf.extend_from_slice(&sync.meta.cursor_col.to_le_bytes());
    buf.push(sync.meta.cursor_shape);
    buf.extend_from_slice(&sync.meta.mode_flags.to_le_bytes());
    buf.extend_from_slice(&sync.meta.echo_ack.to_le_bytes());
    buf.extend_from_slice(&(title_bytes.len() as u16).to_le_bytes());
    buf.extend_from_slice(title_bytes);
    Ok(())
}

fn write_full_pane_sync_grapheme_extras(buf: &mut Vec<u8>, sync: &FullPaneSync) -> io::Result<()> {
    let extras = &sync.grapheme_extras.0;
    buf.extend_from_slice(&(extras.len() as u16).to_le_bytes());
    for (idx, extra) in extras {
        validate_grapheme_extra(extra)?;
        buf.extend_from_slice(&idx.to_le_bytes());
        let bytes = extra.as_bytes();
        buf.push(bytes.len() as u8);
        buf.extend_from_slice(bytes);
    }
    Ok(())
}

fn write_full_pane_sync_hyperlink_extras(buf: &mut Vec<u8>, sync: &FullPaneSync) -> io::Result<()> {
    let extras = &sync.hyperlink_extras;
    buf.extend_from_slice(&(extras.cell_links.len() as u16).to_le_bytes());
    for &(cell_idx, link_id) in &extras.cell_links {
        buf.extend_from_slice(&cell_idx.to_le_bytes());
        buf.extend_from_slice(&link_id.to_le_bytes());
    }
    buf.extend_from_slice(&(extras.link_map.len() as u16).to_le_bytes());
    for (link_id, uri) in &extras.link_map {
        validate_hyperlink_uri(uri)?;
        buf.extend_from_slice(&link_id.to_le_bytes());
        let uri_bytes = uri.as_bytes();
        buf.extend_from_slice(&(uri_bytes.len() as u16).to_le_bytes());
        buf.extend_from_slice(uri_bytes);
    }
    Ok(())
}

fn write_full_pane_sync_cwd(buf: &mut Vec<u8>, sync: &FullPaneSync) -> io::Result<()> {
    match &sync.cwd {
        Some(cwd) => {
            validate_cwd(cwd)?;
            let bytes = cwd.as_bytes();
            buf.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
            buf.extend_from_slice(bytes);
        }
        None => {
            buf.extend_from_slice(&0u16.to_le_bytes());
        }
    }
    Ok(())
}

// ─── Decoding (rewritten with SliceCursor + zerocopy) ───────────────

fn decode_grapheme_extras(cur: &mut SliceCursor<'_>) -> GraphemeExtras {
    let mut extras = GraphemeExtras::new();
    let count = match cur.read_u16() {
        Ok(c) => c as usize,
        Err(_) => return extras,
    };
    for _ in 0..count {
        let idx = match cur.read_u32() {
            Ok(v) => v,
            Err(_) => break,
        };
        let len = match cur.read_u8() {
            Ok(v) => v as usize,
            Err(_) => break,
        };
        let bytes = match cur.read_bytes(len) {
            Ok(b) => b,
            Err(_) => break,
        };
        let extra = String::from_utf8_lossy(bytes).to_string();
        extras.push(idx, &extra);
    }
    extras
}

fn decode_hyperlink_extras(cur: &mut SliceCursor<'_>) -> HyperlinkExtras {
    let mut extras = HyperlinkExtras::new();
    let cell_links_count = match cur.read_u16() {
        Ok(c) => c as usize,
        Err(_) => return extras,
    };
    for _ in 0..cell_links_count {
        let cell_idx = match cur.read_u32() {
            Ok(v) => v,
            Err(_) => return extras,
        };
        let link_id = match cur.read_u16() {
            Ok(v) => v,
            Err(_) => return extras,
        };
        extras.cell_links.push((cell_idx, link_id));
    }

    let link_map_count = match cur.read_u16() {
        Ok(c) => c as usize,
        Err(_) => return extras,
    };
    for _ in 0..link_map_count {
        let link_id = match cur.read_u16() {
            Ok(v) => v,
            Err(_) => return extras,
        };
        let uri = match cur.read_lossy_string_u16() {
            Ok(s) => s,
            Err(_) => return extras,
        };
        extras.link_map.push((link_id, uri));
    }
    extras
}

fn decode_cwd(cur: &mut SliceCursor<'_>) -> Option<String> {
    let len = cur.read_u16().ok()? as usize;
    if len == 0 {
        return None;
    }
    let bytes = cur.read_bytes(len).ok()?;
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

// ─── Public encode functions ────────────────────────────────────────

pub fn encode_full_pane_sync_payload(sync: &FullPaneSync) -> io::Result<Vec<u8>> {
    let title_bytes = sync.title.as_bytes();
    validate_full_pane_sync_title(title_bytes)?;
    let sm_scrollback = sm_encode_cells(&sync.scrollback);
    let sm_viewport = sm_encode_cells(&sync.cells);
    let mut buf =
        Vec::with_capacity(38 + title_bytes.len() + sm_scrollback.len() + sm_viewport.len());

    write_full_pane_sync_header(&mut buf, sync, title_bytes)?;
    buf.extend_from_slice(&sync.scrollback_rows.to_le_bytes());
    buf.push(sync.scrollback_replace as u8);
    buf.extend_from_slice(&(sm_scrollback.len() as u32).to_le_bytes());
    buf.extend_from_slice(&sm_scrollback);
    buf.extend_from_slice(&(sm_viewport.len() as u32).to_le_bytes());
    buf.extend_from_slice(&sm_viewport);
    write_full_pane_sync_grapheme_extras(&mut buf, sync)?;
    write_full_pane_sync_hyperlink_extras(&mut buf, sync)?;
    write_full_pane_sync_cwd(&mut buf, sync)?;
    Ok(buf)
}

pub fn encode_full_pane_sync_framed(buf: &mut Vec<u8>, sync: &FullPaneSync) -> io::Result<()> {
    buf.clear();
    let title_bytes = sync.title.as_bytes();
    validate_full_pane_sync_title(title_bytes)?;
    buf.push(TAG_FULL_PANE_SYNC);
    buf.extend_from_slice(&[0u8; 4]);
    let payload_start = 5;
    write_full_pane_sync_header(buf, sync, title_bytes)?;
    buf.extend_from_slice(&sync.scrollback_rows.to_le_bytes());
    buf.push(sync.scrollback_replace as u8);
    let mut encoder = StateEncoder::new();
    for cell in &sync.scrollback {
        encoder.push_cell(cell);
    }
    let sb_data = encoder.finish();
    buf.extend_from_slice(&(sb_data.len() as u32).to_le_bytes());
    buf.extend_from_slice(sb_data);
    encoder.reset();
    for cell in &sync.cells {
        encoder.push_cell(cell);
    }
    let vp_data = encoder.finish();
    buf.extend_from_slice(&(vp_data.len() as u32).to_le_bytes());
    buf.extend_from_slice(vp_data);
    write_full_pane_sync_grapheme_extras(buf, sync)?;
    write_full_pane_sync_hyperlink_extras(buf, sync)?;
    write_full_pane_sync_cwd(buf, sync)?;
    finalize_frame_compression(
        buf,
        payload_start,
        TAG_FULL_PANE_SYNC,
        TAG_FULL_PANE_SYNC_LZ4,
    );
    Ok(())
}

pub async fn encode_full_pane_sync<W: AsyncWrite + Unpin>(
    writer: &mut W,
    sync: &FullPaneSync,
) -> io::Result<()> {
    let payload = encode_full_pane_sync_payload(sync)?;
    write_frame(writer, TAG_FULL_PANE_SYNC, &payload).await
}

// ─── Public decode function ─────────────────────────────────────────

pub fn decode_full_pane_sync(payload: &[u8]) -> io::Result<FullPaneSync> {
    let mut cur = SliceCursor::new(payload);

    // Read fixed header via zerocopy (37 bytes, one operation).
    let hdr: &FullPaneSyncHeader = cur.read_ref()?;
    let cols = hdr.cols.get();
    let rows = hdr.rows.get();
    let total_cells = cols as usize * rows as usize;
    if total_cells > MAX_GRID_CELLS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("grid too large: {cols}x{rows} = {total_cells} cells (max {MAX_GRID_CELLS})"),
        ));
    }

    let meta = PaneFrameMeta {
        pane_id: hdr.pane_id.get(),
        generation: hdr.generation.get(),
        cursor_line: hdr.cursor_line.get(),
        cursor_col: hdr.cursor_col.get(),
        cursor_shape: hdr.cursor_shape,
        mode_flags: hdr.mode_flags.get(),
        echo_ack: hdr.echo_ack.get(),
    };

    // Title (variable length).
    let title_len = hdr.title_len.get() as usize;
    let title_bytes = cur.read_bytes(title_len)?;
    let title = String::from_utf8_lossy(title_bytes).into_owned();

    // Scrollback section.
    let scrollback_rows = cur.read_u32()?;
    let scrollback_replace = cur.read_u8()? != 0;
    let scrollback_len = cur.read_u32()? as usize;
    let scrollback_data = cur.read_bytes(scrollback_len)?;

    let sb_expected = scrollback_rows as usize * cols as usize;
    if sb_expected > MAX_GRID_CELLS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "scrollback too large: {scrollback_rows} rows x {cols} cols = {sb_expected} cells (max {MAX_GRID_CELLS})"
            ),
        ));
    }

    // Viewport section.
    let viewport_len = cur.read_u32()? as usize;
    let viewport_data = cur.read_bytes(viewport_len)?;

    // Decode SM data into cells.
    let scrollback = sm_decode_cells_vec(scrollback_data, sb_expected)?;
    let cells = sm_decode_cells_vec(viewport_data, total_cells)?;

    // Optional extras.
    let grapheme_extras = decode_grapheme_extras(&mut cur);
    let hyperlink_extras = decode_hyperlink_extras(&mut cur);
    let cwd = decode_cwd(&mut cur);

    Ok(FullPaneSync {
        meta,
        cols,
        rows,
        title,
        scrollback,
        scrollback_rows,
        scrollback_replace,
        cells,
        grapheme_extras,
        hyperlink_extras,
        cwd,
    })
}

/// Borrowed decode: parse header + store SM data offsets, no cell allocation.
/// Cells are decoded on demand by the consumer (e.g. directly into client viewport).
/// Convert an owned FullPaneSync into a borrowed variant by encoding then decoding.
/// Useful for tests and backward-compat code paths.
pub fn full_pane_sync_to_borrowed(sync: &FullPaneSync) -> io::Result<FullPaneSyncBorrowed> {
    let payload = encode_full_pane_sync_payload(sync)?;
    decode_full_pane_sync_borrowed(payload)
}

/// Borrowed decode: parse header + store SM data offsets, no cell allocation.
/// Cells are decoded on demand by the consumer (e.g. directly into client viewport).
pub fn decode_full_pane_sync_borrowed(payload: Vec<u8>) -> io::Result<FullPaneSyncBorrowed> {
    let mut cur = SliceCursor::new(&payload);

    let hdr: &FullPaneSyncHeader = cur.read_ref()?;
    let cols = hdr.cols.get();
    let rows = hdr.rows.get();
    let total_cells = cols as usize * rows as usize;
    if total_cells > MAX_GRID_CELLS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("grid too large: {cols}x{rows} = {total_cells} cells (max {MAX_GRID_CELLS})"),
        ));
    }

    let meta = PaneFrameMeta {
        pane_id: hdr.pane_id.get(),
        generation: hdr.generation.get(),
        cursor_line: hdr.cursor_line.get(),
        cursor_col: hdr.cursor_col.get(),
        cursor_shape: hdr.cursor_shape,
        mode_flags: hdr.mode_flags.get(),
        echo_ack: hdr.echo_ack.get(),
    };

    let title_len = hdr.title_len.get() as usize;
    let title_bytes = cur.read_bytes(title_len)?;
    let title = String::from_utf8_lossy(title_bytes).into_owned();

    // Scrollback SM data — record offset, skip over bytes.
    let scrollback_rows = cur.read_u32()?;
    let scrollback_replace = cur.read_u8()? != 0;
    let scrollback_sm_len = cur.read_u32()? as usize;
    let scrollback_sm_offset = cur.pos();
    let _ = cur.read_bytes(scrollback_sm_len)?;

    let sb_expected = scrollback_rows as usize * cols as usize;
    if sb_expected > MAX_GRID_CELLS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "scrollback too large: {scrollback_rows} rows x {cols} cols = {sb_expected} cells (max {MAX_GRID_CELLS})"
            ),
        ));
    }

    // Viewport SM data — record offset, skip over bytes.
    let viewport_sm_len = cur.read_u32()? as usize;
    let viewport_sm_offset = cur.pos();
    let _ = cur.read_bytes(viewport_sm_len)?;

    // Optional extras (still eagerly parsed — they're tiny).
    let grapheme_extras = decode_grapheme_extras(&mut cur);
    let hyperlink_extras = decode_hyperlink_extras(&mut cur);
    let cwd = decode_cwd(&mut cur);

    Ok(FullPaneSyncBorrowed::new(
        meta,
        cols,
        rows,
        title,
        scrollback_rows,
        scrollback_replace,
        grapheme_extras,
        hyperlink_extras,
        cwd,
        scrollback_sm_offset,
        scrollback_sm_len,
        viewport_sm_offset,
        viewport_sm_len,
        payload,
    ))
}
