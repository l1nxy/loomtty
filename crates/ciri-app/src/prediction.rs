use ciri_config::config::PredictionMode;
use ciri_protocol::message::*;
use std::collections::HashMap;
use std::time::Instant;

use crate::grid::ClientPaneGrid;

const PREDICTION_TIMEOUT_SECS: u64 = 5;
const PING_INTERVAL_SECS: u64 = 2;

pub struct PredictedCell {
    pub row: u16,
    pub col: u16,
    pub replacement: PackedCell,
    pub epoch: u64,
    pub created_at: Instant,
}

pub struct PredictedCursor {
    pub row: i16,
    pub col: u16,
    pub epoch: u64,
}

pub struct PaneOverlay {
    pub cells: Vec<PredictedCell>,
    pub cursor: Option<PredictedCursor>,
    pub prediction_epoch: u64,
    pub confirmed_epoch: u64,
}

impl PaneOverlay {
    fn new() -> Self {
        PaneOverlay {
            cells: Vec::new(),
            cursor: None,
            prediction_epoch: 0,
            confirmed_epoch: 0,
        }
    }

    fn increment_epoch(&mut self) {
        self.prediction_epoch += 1;
    }

    pub fn kill_epoch(&mut self, epoch: u64) {
        self.cells.retain(|c| c.epoch < epoch);
        if self.cursor.as_ref().is_some_and(|c| c.epoch >= epoch) {
            self.cursor = None;
        }
        if self.prediction_epoch >= epoch {
            self.prediction_epoch = epoch.saturating_sub(1);
        }
    }

    fn expire_old(&mut self, now: Instant) {
        self.cells
            .retain(|c| now.duration_since(c.created_at).as_secs() < PREDICTION_TIMEOUT_SECS);
        if self
            .cursor
            .as_ref()
            .is_some_and(|c| self.cells.iter().all(|cell| cell.epoch != c.epoch))
        {
            self.cursor = None;
        }
    }

    fn cursor_position(&self) -> Option<(i16, u16)> {
        self.cursor.as_ref().map(|c| (c.row, c.col))
    }

    fn is_empty(&self) -> bool {
        self.cells.is_empty() && self.cursor.is_none()
    }
}

pub struct PredictionEngine {
    overlays: HashMap<u64, PaneOverlay>,
    srtt_us: u64,
    mode: PredictionMode,
    threshold_ms: u64,
    show_underline: bool,
    ping_seq: u64,
    last_ping: Option<Instant>,
}

pub struct GridInfo {
    pub cursor_row: i16,
    pub cursor_col: u16,
    pub cols: u16,
    pub rows: u16,
    pub mode_flags: u16,
}

impl PredictionEngine {
    pub fn new(mode: PredictionMode, threshold_ms: u64, show_underline: bool) -> Self {
        PredictionEngine {
            overlays: HashMap::new(),
            srtt_us: 0,
            mode,
            threshold_ms,
            show_underline,
            ping_seq: 0,
            last_ping: None,
        }
    }

    pub fn update_config(&mut self, mode: PredictionMode, threshold_ms: u64, show_underline: bool) {
        self.mode = mode;
        self.threshold_ms = threshold_ms;
        self.show_underline = show_underline;
    }

    pub fn srtt_ms(&self) -> u64 {
        self.srtt_us / 1000
    }

    pub fn should_display(&self) -> bool {
        match self.mode {
            PredictionMode::Always => true,
            PredictionMode::Never => false,
            PredictionMode::Adaptive => self.srtt_us / 1000 > self.threshold_ms,
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

    pub fn new_user_input(&mut self, pane_id: u64, data: &[u8], info: &GridInfo) {
        if self.mode == PredictionMode::Never {
            return;
        }

        if info.mode_flags & MODE_ALT_SCREEN != 0
            || info.mode_flags & MODE_MOUSE_REPORT != 0
            || info.mode_flags & MODE_BRACKETED_PASTE != 0
        {
            return;
        }

        let overlay = self
            .overlays
            .entry(pane_id)
            .or_insert_with(PaneOverlay::new);
        let (crow, mut ccol) = overlay
            .cursor_position()
            .unwrap_or((info.cursor_row, info.cursor_col));

        // Handle arrow key escape sequences as whole-buffer matches
        if data == b"\x1B[C" {
            // Right arrow
            if ccol < info.cols.saturating_sub(1) {
                ccol += 1;
            }
            if ccol < info.cols {
                overlay.cursor = Some(PredictedCursor {
                    row: crow,
                    col: ccol,
                    epoch: overlay.prediction_epoch,
                });
            }
            return;
        }
        if data == b"\x1B[D" {
            // Left arrow
            if ccol > 0 {
                ccol -= 1;
            }
            overlay.cursor = Some(PredictedCursor {
                row: crow,
                col: ccol,
                epoch: overlay.prediction_epoch,
            });
            return;
        }

        for &byte in data {
            match byte {
                0x20..=0x7E => {
                    if ccol >= info.cols {
                        overlay.increment_epoch();
                        continue;
                    }
                    let mut cell = PackedCell::default();
                    cell.set_ch(byte as char);
                    if self.show_underline {
                        cell.flags = (cell.flags_u16() | FLAG_UNDERLINE).to_le_bytes();
                    }
                    overlay.cells.push(PredictedCell {
                        row: crow as u16,
                        col: ccol,
                        replacement: cell,
                        epoch: overlay.prediction_epoch,
                        created_at: Instant::now(),
                    });
                    ccol += 1;
                }
                0x08 | 0x7F => {
                    if ccol == 0 {
                        overlay.increment_epoch();
                        continue;
                    }
                    ccol -= 1;
                    let mut cell = PackedCell::default();
                    cell.set_ch(' ');
                    if self.show_underline {
                        cell.flags = (cell.flags_u16() | FLAG_UNDERLINE).to_le_bytes();
                    }
                    overlay.cells.push(PredictedCell {
                        row: crow as u16,
                        col: ccol,
                        replacement: cell,
                        epoch: overlay.prediction_epoch,
                        created_at: Instant::now(),
                    });
                }
                0x0D | 0x0A => {
                    overlay.increment_epoch();
                    return;
                }
                0x1B => {
                    overlay.increment_epoch();
                    return;
                }
                _ => {
                    overlay.increment_epoch();
                    return;
                }
            }
        }

        if ccol < info.cols {
            overlay.cursor = Some(PredictedCursor {
                row: crow,
                col: ccol,
                epoch: overlay.prediction_epoch,
            });
        }
    }

    pub fn on_server_sync(&mut self, pane_id: u64, grid: &ClientPaneGrid) {
        let Some(overlay) = self.overlays.get_mut(&pane_id) else {
            return;
        };

        if overlay.is_empty() {
            return;
        }

        overlay.expire_old(Instant::now());

        let cols = grid.cols as usize;
        let rows = grid.rows as usize;

        let mut epoch_ok: HashMap<u64, bool> = HashMap::new();

        overlay.cells.retain(|pred| {
            let r = pred.row as usize;
            let c = pred.col as usize;
            if r >= rows || c >= cols {
                return false;
            }

            let idx = r * cols + c;
            if idx >= grid.viewport.len() {
                return false;
            }

            let actual = &grid.viewport[idx];
            let pred_ch = pred.replacement.ch();
            let actual_ch = actual.ch();

            if pred_ch == actual_ch {
                epoch_ok.entry(pred.epoch).or_insert(true);
                false
            } else {
                true
            }
        });

        for (epoch, ok) in &epoch_ok {
            if *ok && *epoch >= overlay.confirmed_epoch {
                overlay.confirmed_epoch = *epoch + 1;
            }
        }

        if let Some(ref cur) = overlay.cursor {
            if cur.row == grid.cursor_line && cur.col == grid.cursor_col {
                overlay.cursor = None;
            }
        }

        if overlay.is_empty() {
            self.overlays.remove(&pane_id);
        }
    }

    pub fn get_overlay_cell(&self, pane_id: u64, row: u16, col: u16) -> Option<PackedCell> {
        if !self.should_display() {
            return None;
        }
        let overlay = self.overlays.get(&pane_id)?;
        overlay
            .cells
            .iter()
            .rev()
            .find(|c| {
                c.row == row
                    && c.col == col
                    && (self.mode == PredictionMode::Always || c.epoch <= overlay.confirmed_epoch)
            })
            .map(|c| c.replacement)
    }

    pub fn get_overlay_cursor(&self, pane_id: u64) -> Option<(i16, u16)> {
        if !self.should_display() {
            return None;
        }
        let overlay = self.overlays.get(&pane_id)?;
        let cur = overlay.cursor.as_ref()?;
        if self.mode == PredictionMode::Always || cur.epoch <= overlay.confirmed_epoch {
            Some((cur.row, cur.col))
        } else {
            None
        }
    }

    pub fn has_overlay(&self, pane_id: u64) -> bool {
        self.overlays.get(&pane_id).is_some_and(|o| !o.is_empty())
    }

    pub fn clear_pane(&mut self, pane_id: u64) {
        self.overlays.remove(&pane_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_info(cols: u16, rows: u16, cursor_col: u16, cursor_row: i16) -> GridInfo {
        GridInfo {
            cursor_row,
            cursor_col,
            cols,
            rows,
            mode_flags: 0,
        }
    }

    fn make_grid(cols: u16, rows: u16) -> ClientPaneGrid {
        ClientPaneGrid::new(cols, rows, 0)
    }

    #[test]
    fn printable_char_prediction() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, true);
        let info = make_info(80, 24, 0, 0);
        engine.new_user_input(1, b"A", &info);

        let cell = engine.get_overlay_cell(1, 0, 0);
        assert!(cell.is_some());
        assert_eq!(cell.unwrap().ch(), 'A');
        assert_ne!(cell.unwrap().flags_u16() & FLAG_UNDERLINE, 0);
    }

    #[test]
    fn cursor_advances() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let info = make_info(80, 24, 5, 0);
        engine.new_user_input(1, b"xyz", &info);

        let cursor = engine.get_overlay_cursor(1);
        assert_eq!(cursor, Some((0, 8)));
    }

    #[test]
    fn backspace_prediction() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let info = make_info(80, 24, 5, 0);
        engine.new_user_input(1, &[0x7F], &info);

        let cell = engine.get_overlay_cell(1, 0, 4);
        assert!(cell.is_some());
        assert_eq!(cell.unwrap().ch(), ' ');
        assert_eq!(engine.get_overlay_cursor(1), Some((0, 4)));
    }

    #[test]
    fn alt_screen_skips_prediction() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let mut info = make_info(80, 24, 0, 0);
        info.mode_flags = MODE_ALT_SCREEN;
        engine.new_user_input(1, b"A", &info);

        assert!(!engine.has_overlay(1));
    }

    #[test]
    fn never_mode_skips() {
        let mut engine = PredictionEngine::new(PredictionMode::Never, 0, false);
        let info = make_info(80, 24, 0, 0);
        engine.new_user_input(1, b"A", &info);

        assert!(!engine.has_overlay(1));
    }

    #[test]
    fn adaptive_mode_display_threshold() {
        let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
        assert!(!engine.should_display());

        engine.srtt_us = 50_000;
        assert!(engine.should_display());
    }

    #[test]
    fn server_sync_confirms_prediction() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let info = make_info(80, 24, 0, 0);
        engine.new_user_input(1, b"A", &info);
        assert!(engine.has_overlay(1));

        let mut grid = make_grid(80, 24);
        grid.viewport[0].set_ch('A');
        grid.cursor_col = 1;
        grid.cursor_line = 0;
        engine.on_server_sync(1, &grid);

        assert!(!engine.has_overlay(1));
    }

    #[test]
    fn srtt_computation() {
        let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);

        let base_us = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;

        engine.on_pong(1, base_us.saturating_sub(10_000));
        assert!(engine.srtt_us > 0);
    }

    #[test]
    fn ping_generation() {
        let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
        let ping = engine.maybe_send_ping();
        assert!(ping.is_some());
        let ping2 = engine.maybe_send_ping();
        assert!(ping2.is_none());
    }
}
