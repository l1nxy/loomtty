//! CellDelta encoding and decoding (state-machine binary format).

use crate::message::*;
use std::io;

use super::frame::{TAG_CELL_DELTA, TAG_CELL_DELTA_LZ4, finalize_frame_compression};
use super::state_machine::StateEncoder;
use super::util::SliceCursor;

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
    buf.extend_from_slice(&meta.received_ack.to_le_bytes());
    buf.extend_from_slice(&meta.echo_ack.to_le_bytes());
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

    finalize_frame_compression(buf, payload_start, TAG_CELL_DELTA, TAG_CELL_DELTA_LZ4);
    Ok(())
}

/// Decode CellDelta: parse region metadata, store SM payload for on-demand decoding.
pub fn decode_cell_delta_borrowed(payload: Vec<u8>) -> io::Result<CellDeltaBorrowed> {
    let mut cur = SliceCursor::new(&payload);

    let hdr: &CellDeltaHeader = cur.read_ref()?;
    let num_regions = hdr.num_regions.get() as usize;

    let meta = PaneFrameMeta {
        pane_id: hdr.pane_id.get(),
        generation: hdr.generation.get(),
        cursor_line: hdr.cursor_line.get(),
        cursor_col: hdr.cursor_col.get(),
        cursor_shape: hdr.cursor_shape,
        mode_flags: hdr.mode_flags.get(),
        received_ack: hdr.received_ack.get(),
        echo_ack: hdr.echo_ack.get(),
    };
    let cols = hdr.cols.get();

    let mut regions = Vec::with_capacity(num_regions);
    for _ in 0..num_regions {
        let rh: &CellDeltaRegionHeader = cur.read_ref()?;
        let left = rh.left.get();
        let right = rh.right.get();
        validate_damage_bounds(left, right)?;
        let sm_data_len = rh.sm_data_len.get() as usize;
        let sm_offset = cur.pos();
        let _ = cur.read_bytes(sm_data_len)?; // advance past SM data
        regions.push(BorrowedRegionMeta {
            line: rh.line.get(),
            left,
            right,
            sm_offset,
            sm_len: sm_data_len,
        });
    }

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
