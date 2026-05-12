mod input;
mod overlay;
mod sync;
pub(crate) mod utf8;

pub use overlay::{PaneOverlay, PredictedCursor};

use ciri_config::config::PredictionMode;
use ciri_protocol::message::*;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

const PREDICTION_TIMEOUT_SECS: u64 = 8;
const PING_INTERVAL_SECS: u64 = 2;

const GLITCH_THRESHOLD_MS: u64 = 250;
const GLITCH_FLAG_THRESHOLD_MS: u64 = 5000;
const GLITCH_REPAIR_COUNT: u32 = 10;
const GLITCH_REPAIR_MIN_INTERVAL_MS: u64 = 150;

const FLAG_TRIGGER_HIGH_MS: u64 = 80;
const FLAG_TRIGGER_LOW_MS: u64 = 50;

pub struct PredictionEngine {
    pub(super) overlays: HashMap<u64, overlay::PaneOverlay>,
    pub(super) pane_visual_serials: HashMap<u64, u64>,
    pub(super) srtt_us: u64,
    pub(super) mode: PredictionMode,
    pub(super) threshold_ms: u64,
    pub(super) show_underline: bool,
    pub(super) visual_serial: u64,
    ping_seq: u64,
    last_ping: Option<Instant>,
    pub(super) next_input_seq: u64,
    pub(super) last_dims: HashMap<u64, (u16, u16)>,
    pub(super) force_visible_panes: HashSet<u64>,
    pub(super) glitch_trigger: u32,
    pub(super) last_quick_confirm: Option<Instant>,
    pub(super) flagging: bool,
}

impl PredictionEngine {
    pub fn new(mode: PredictionMode, threshold_ms: u64, show_underline: bool) -> Self {
        Self {
            overlays: HashMap::new(),
            pane_visual_serials: HashMap::new(),
            srtt_us: 0,
            mode,
            threshold_ms,
            show_underline,
            visual_serial: 1,
            ping_seq: 0,
            last_ping: None,
            next_input_seq: 1,
            last_dims: HashMap::new(),
            force_visible_panes: HashSet::new(),
            glitch_trigger: 0,
            last_quick_confirm: None,
            flagging: false,
        }
    }

    /// Clear all per-connection state while keeping config (mode, threshold, etc.).
    pub fn reset(&mut self) {
        self.overlays.clear();
        self.pane_visual_serials.clear();
        self.srtt_us = 0;
        self.visual_serial = 1;
        self.ping_seq = 0;
        self.last_ping = None;
        self.next_input_seq = 1;
        self.last_dims.clear();
        self.force_visible_panes.clear();
        self.glitch_trigger = 0;
        self.last_quick_confirm = None;
        self.flagging = false;
    }

    pub fn next_input_seq(&mut self) -> u64 {
        let seq = self.next_input_seq;
        self.next_input_seq += 1;
        seq
    }

    pub fn update_config(&mut self, mode: PredictionMode, threshold_ms: u64, show_underline: bool) {
        self.mode = mode;
        self.threshold_ms = threshold_ms;
        self.show_underline = show_underline;
        self.bump_visual_serial();
        for serial in self.pane_visual_serials.values_mut() {
            *serial = serial.wrapping_add(1);
        }
    }

    pub fn srtt_ms(&self) -> u64 {
        self.srtt_us / 1000
    }

    pub fn should_display(&self) -> bool {
        match self.mode {
            PredictionMode::Always => true,
            PredictionMode::Never => false,
            PredictionMode::Adaptive => {
                self.srtt_us / 1000 > self.threshold_ms || self.glitch_trigger > 0
            }
        }
    }

    pub fn maybe_send_ping(&mut self) -> Option<ClientMessage> {
        let now = Instant::now();
        let should_ping = match self.last_ping {
            None => true,
            Some(t) => now.duration_since(t).as_secs() >= PING_INTERVAL_SECS,
        };
        if !should_ping {
            return None;
        }
        self.ping_seq += 1;
        self.last_ping = Some(now);
        let client_time_us = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;
        Some(ClientMessage::Ping {
            seq: self.ping_seq,
            client_time_us,
        })
    }

    pub fn on_pong(&mut self, _seq: u64, client_time_us: u64) {
        let now_us = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;
        let sample = now_us.saturating_sub(client_time_us);
        if self.srtt_us == 0 {
            self.srtt_us = sample;
        } else {
            self.srtt_us = (self.srtt_us * 7 + sample) / 8;
        }
    }

    // ---- Public overlay access ----

    pub fn get_overlay_cell(&self, pane_id: u64, row: u16, col: u16) -> Option<PackedCell> {
        let force_visible = self.force_visible_panes.contains(&pane_id);
        if !force_visible && !self.should_display() {
            return None;
        }
        let overlay = self.overlays.get(&pane_id)?;
        let cell = overlay.get_cell(row, col)?;
        if cell.unknown {
            return None;
        }
        if force_visible
            || self.mode == PredictionMode::Always
            || cell.epoch <= overlay.confirmed_epoch
        {
            Some(cell.replacement)
        } else {
            None
        }
    }

    pub fn get_overlay_cursor(&self, pane_id: u64) -> Option<(i16, u16)> {
        let force_visible = self.force_visible_panes.contains(&pane_id);
        if !force_visible && !self.should_display() {
            return None;
        }
        let overlay = self.overlays.get(&pane_id)?;
        let cur = overlay.cursor.as_ref()?;
        if force_visible
            || self.mode == PredictionMode::Always
            || cur.epoch <= overlay.confirmed_epoch
        {
            Some((cur.row, cur.col))
        } else {
            None
        }
    }

    pub fn has_overlay(&self, pane_id: u64) -> bool {
        self.overlays.get(&pane_id).is_some_and(|o| !o.is_empty())
    }

    pub fn dirty_rows(&self, pane_id: u64) -> Vec<u16> {
        self.overlays
            .get(&pane_id)
            .map(|overlay| overlay.rows.keys().copied().collect())
            .unwrap_or_default()
    }

    pub fn visual_serial(&self) -> u64 {
        self.visual_serial
    }

    pub fn pane_visual_serial(&self, pane_id: u64) -> u64 {
        self.pane_visual_serials.get(&pane_id).copied().unwrap_or(0)
    }

    pub fn clear_pane(&mut self, pane_id: u64) {
        if self.overlays.remove(&pane_id).is_some() {
            self.bump_visual_serial();
        }
        self.force_visible_panes.remove(&pane_id);
        self.pane_visual_serials.remove(&pane_id);
    }

    fn bump_visual_serial(&mut self) {
        self.visual_serial = self.visual_serial.wrapping_add(1);
    }

    pub(super) fn bump_visual_serial_for_pane(&mut self, pane_id: u64) {
        self.bump_visual_serial();
        let entry = self.pane_visual_serials.entry(pane_id).or_insert(1);
        *entry = entry.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests;
