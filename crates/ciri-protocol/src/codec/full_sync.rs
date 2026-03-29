//! FullPaneSync encoding and decoding.

use crate::message::*;
use std::io;
use tokio::io::AsyncWrite;

use super::frame::{write_frame, TAG_FULL_PANE_SYNC};
use super::state_machine::{sm_decode_cells_vec, sm_encode_cells, StateEncoder};
use super::util::*;

const FULL_PANE_SYNC_MIN_HEADER_LEN: usize = 28;
const FULL_PANE_SYNC_SCROLLBACK_HEADER_LEN: usize = 9; // u32 rows + u8 replace + u32 data_len
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
) -> io::Result<(u32, bool, String, FullPaneSyncMandatorySections<'_>)> {
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
    let scrollback_rows = read_u32_le(payload, offset)?;
    offset += 4;
    let scrollback_replace = payload[offset] != 0;
    offset += 1;
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
        scrollback_replace,
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
    buf.push(sync.scrollback_replace as u8);
    buf.extend_from_slice(&(sm_scrollback.len() as u32).to_le_bytes());
    buf.extend_from_slice(&sm_scrollback);
    buf.extend_from_slice(&(sm_viewport.len() as u32).to_le_bytes());
    buf.extend_from_slice(&sm_viewport);
    write_full_pane_sync_grapheme_extras(&mut buf, sync);
    write_full_pane_sync_hyperlink_extras(&mut buf, sync);
    write_full_pane_sync_cwd(&mut buf, sync);
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
    write_full_pane_sync_grapheme_extras(buf, sync);
    write_full_pane_sync_hyperlink_extras(buf, sync);
    write_full_pane_sync_cwd(buf, sync);
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
    let (scrollback_rows, scrollback_replace, title, sections) =
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
        scrollback_replace,
        cells,
        grapheme_extras,
        hyperlink_extras,
        cwd,
    })
}
