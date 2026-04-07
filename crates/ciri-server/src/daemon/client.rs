use bytes::Bytes;
use std::collections::HashMap;
use tokio::sync::mpsc;

use super::damage::DamageAccumulator;

pub(crate) struct ClientState {
    pub(crate) id: u64,
    pub(crate) tx: mpsc::Sender<Bytes>,
    pub(crate) damage: HashMap<u64, DamageAccumulator>, // per pane_id
    pub(crate) last_acked_generation: u64,
    /// Per-pane: highest input_seq received from this client (for echo-ack).
    pub(crate) max_input_seq: HashMap<u64, u64>,
    /// Per-pane: how many history lines this client has received.
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
