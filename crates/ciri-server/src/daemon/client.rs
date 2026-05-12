use bytes::Bytes;
use std::collections::HashMap;
use tokio::sync::mpsc;

use super::damage::DamageAccumulator;

pub(crate) struct ClientState {
    pub(crate) id: u64,
    pub(crate) tx: mpsc::Sender<Bytes>,
    pub(crate) damage: HashMap<u64, DamageAccumulator>, // per pane_id
    pub(crate) last_acked_generation: u64,
    /// Per-pane: highest input_seq for which PTY output has already flowed
    /// back. This is what gets advertised as `echo_ack` on data frames — it
    /// guarantees that the framebuffer state in the frame reflects the
    /// effects of the acknowledged input. Equivalent to mosh's
    /// `local_frame_late_acked`.
    pub(crate) max_input_seq: HashMap<u64, u64>,
    /// Per-pane: highest input_seq received but not yet promoted to
    /// `max_input_seq`. Promotion happens after the pane's PTY produces
    /// any output, which proves the application has had a chance to react
    /// to the input. Without this two-stage ack, predictions can be judged
    /// against framebuffer state that hasn't yet caught up to the input.
    pub(crate) received_input_seq: HashMap<u64, u64>,
    /// Per-pane: primary-screen scrollback watermark mirrored to this client.
    ///
    /// This only advances when the client actually receives primary-screen
    /// scrollback. While a pane is in alt-screen, the watermark is preserved
    /// so the server can resend the hidden primary history after alt-screen
    /// exits.
    pub(crate) history_sent: HashMap<u64, usize>,
    /// Consecutive try_send failures; used to detect slow clients.
    pub(crate) send_failures: u32,
    pub(crate) cell_width: f32,
    pub(crate) cell_height: f32,
    pub(crate) viewport_width: f32,
    pub(crate) viewport_height: f32,
    /// Which session this client is attached to.
    pub(crate) session_name: String,
}
