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
    Frame, encode_client_msg, encode_server_msg, frame_full_pane_sync, frame_server_msg,
    frame_server_msg_into, read_frame,
};
pub use full_sync::{
    decode_full_pane_sync, encode_full_pane_sync, encode_full_pane_sync_framed,
    encode_full_pane_sync_payload,
};
pub use handshake::{
    ClientHello, SERVER_HELLO_LEN, VersionCompat, WIRE_PROTOCOL_VERSION, build_client_hello,
    read_client_hello, read_server_hello, write_client_hello, write_server_hello,
};
pub use state_machine::{StateEncoder, decode_sm_cells};

#[cfg(test)]
mod tests;
