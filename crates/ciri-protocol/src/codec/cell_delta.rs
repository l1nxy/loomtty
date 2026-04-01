//! CellDelta encoding and decoding (state-machine binary format).

use crate::message::*;
use std::io;

use super::frame::{TAG_CELL_DELTA, TAG_CELL_DELTA_LZ4, maybe_compress_payload};
use super::state_machine::StateEncoder;
use super::util::*;

/// Encode a CellDelta frame by streaming cells through a StateEncoder.
pub fn encode_cell_delta_streaming_framed<F>(
    buf: &mut Vec<u8>,
    meta: &PaneFrameMeta,
    cols: u16,
    regions: &[(u16, u16, u16)],
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
    buf.reserve(5 + 27 + regions.len() * 32);
    buf.push(TAG_CELL_DELTA);
    buf.extend_from_slice(&[0u8; 4]);
    let payload_start = 5;
    buf.extend_from_slice(&meta.pane_id.to_le_bytes());
    buf.extend_from_slice(&meta.generation.to_le_bytes());
    buf.extend_from_slice(&meta.cursor_line.to_le_bytes());
    buf.extend_from_slice(&meta.cursor_col.to_le_bytes());
    buf.push(meta.cursor_shape);
    buf.extend_from_slice(&meta.mode_flags.to_le_bytes());
    buf.extend_from_slice(&cols.to_le_bytes());
    buf.extend_from_slice(&(regions.len() as u16).to_le_bytes());

    let mut encoder = StateEncoder::new();
    for &(line, left, right) in regions {
        buf.extend_from_slice(&line.to_le_bytes());
        buf.extend_from_slice(&left.to_le_bytes());
        buf.extend_from_slice(&right.to_le_bytes());
        encoder.reset();
        write_cells(line, left, right, &mut encoder);
        let sm_data = encoder.finish();
        buf.extend_from_slice(&(sm_data.len() as u32).to_le_bytes());
        buf.extend_from_slice(sm_data);
    }

    // Try LZ4 compression on the payload.
    let payload = &buf[payload_start..];
    let (tag, compressed) = maybe_compress_payload(TAG_CELL_DELTA, TAG_CELL_DELTA_LZ4, payload);
    buf.truncate(payload_start);
    buf[0] = tag;
    let payload_len = compressed.len() as u32;
    buf[1..5].copy_from_slice(&payload_len.to_le_bytes());
    buf.extend_from_slice(&compressed);
    Ok(())
}

/// Decode CellDelta: parse region metadata, store SM payload for on-demand decoding.
pub fn decode_cell_delta_borrowed(payload: Vec<u8>) -> io::Result<CellDeltaBorrowed> {
    if payload.len() < 27 {
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
    let mode_flags = read_u16_le(&payload, 21)?;
    let cols = read_u16_le(&payload, 23)?;
    let num_regions = read_u16_le(&payload, 25)? as usize;
    let mut offset = 27;
    let mut regions = Vec::with_capacity(num_regions);
    for _ in 0..num_regions {
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
    let meta = PaneFrameMeta {
        pane_id,
        generation,
        cursor_line,
        cursor_col,
        cursor_shape,
        mode_flags,
    };
    Ok(CellDeltaBorrowed::new(meta, cols, regions, payload))
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
