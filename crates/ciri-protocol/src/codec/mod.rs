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

mod cell_delta;
mod frame;
mod full_sync;
mod handshake;
mod state_machine;
mod util;

pub use cell_delta::{decode_cell_delta_borrowed, encode_cell_delta_streaming_framed};
pub use frame::{
    Frame, MAX_CONTROL_FRAME_LEN, MAX_DATA_FRAME_LEN, TAG_CELL_DELTA, TAG_CELL_DELTA_LZ4,
    TAG_CLIENT_MSG, TAG_FULL_PANE_SYNC, TAG_FULL_PANE_SYNC_LZ4, TAG_SERVER_MSG, encode_client_msg,
    encode_server_msg, frame_full_pane_sync, frame_server_msg, frame_server_msg_into, read_frame,
    read_frame_reuse,
};
pub use full_sync::{
    decode_full_pane_sync, decode_full_pane_sync_borrowed, encode_full_pane_sync,
    encode_full_pane_sync_framed, encode_full_pane_sync_payload, full_pane_sync_to_borrowed,
};
pub use handshake::{
    ClientHello, SERVER_HELLO_LEN, VersionCompat, WIRE_PROTOCOL_VERSION, build_client_hello,
    read_client_hello, read_server_hello, write_client_hello, write_server_hello,
};
pub use state_machine::{
    OP_ASCII, OP_ASCII_REPEAT, OP_CHAR1, OP_CHARS, OP_CHARS_LONG, OP_END, OP_REPEAT, OP_RESET,
    OP_SET_BG, OP_SET_BG_INDEXED, OP_SET_BG_NAMED, OP_SET_FG, OP_SET_FG_BG, OP_SET_FG_INDEXED,
    OP_SET_FG_NAMED, OP_SET_FLAGS, StateEncoder, decode_sm_cells,
};

#[cfg(test)]
mod tests;
