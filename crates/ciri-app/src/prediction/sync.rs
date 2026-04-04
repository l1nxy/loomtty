use ciri_protocol::message::PackedColor;
use std::time::Instant;

use super::{
    GLITCH_FLAG_THRESHOLD_MS, GLITCH_REPAIR_COUNT, GLITCH_REPAIR_MIN_INTERVAL_MS,
    GLITCH_THRESHOLD_MS, FLAG_TRIGGER_HIGH_MS, FLAG_TRIGGER_LOW_MS, PredictionEngine,
};
use crate::grid::ClientPaneGrid;

impl PredictionEngine {
    /// Validate predictions against authoritative server state.
    ///
    /// Two-pass algorithm:
    /// 1. Confirm correct predictions, update glitch trigger, propagate renditions
    /// 2. Detect wrong predictions — kill epoch or reset entirely
    pub fn on_server_sync(&mut self, pane_id: u64, grid: &ClientPaneGrid, echo_ack: u64) {
        // Reset on terminal resize.
        let dims = (grid.cols, grid.rows);
        let prev = self.last_dims.insert(pane_id, dims);
        if prev.is_some_and(|d| d != dims) {
            self.overlays.remove(&pane_id);
            return;
        }

        let Some(overlay) = self.overlays.get_mut(&pane_id) else {
            return;
        };
        if overlay.is_empty() {
            return;
        }

        let now = Instant::now();
        overlay.expire_old(now);

        let cols = grid.cols as usize;
        let rows = grid.rows as usize;

        let row_nums: Vec<u16> = overlay.rows.keys().copied().collect();

        // --- Pass 1: confirm correct predictions ---
        let mut max_confirmed = overlay.confirmed_epoch;

        for &row_num in &row_nums {
            let Some(row) = overlay.rows.get_mut(&row_num) else {
                continue;
            };
            let mut propagate_fg_bg: Option<(PackedColor, PackedColor)> = None;

            for col in 0..row.cells.len() {
                if !row.cells[col].active {
                    continue;
                }
                if let Some((fg, bg)) = propagate_fg_bg {
                    row.cells[col].replacement.fg = fg;
                    row.cells[col].replacement.bg = bg;
                }
                if row_num as usize >= rows || col >= cols {
                    row.cells[col].reset();
                    continue;
                }

                // Pending — server hasn't processed input yet.
                if echo_ack < row.cells[col].min_echo_ack {
                    let pending_ms =
                        now.duration_since(row.cells[col].created_at).as_millis() as u64;
                    if pending_ms >= GLITCH_FLAG_THRESHOLD_MS {
                        self.glitch_trigger = GLITCH_REPAIR_COUNT * 2;
                    } else if pending_ms >= GLITCH_THRESHOLD_MS
                        && self.glitch_trigger < GLITCH_REPAIR_COUNT
                    {
                        self.glitch_trigger = GLITCH_REPAIR_COUNT;
                    }
                    continue;
                }

                // Unknown cells: clear once mature.
                if row.cells[col].unknown {
                    row.cells[col].reset();
                    continue;
                }

                let idx = (row_num as usize) * cols + col;
                if idx >= grid.viewport.len() {
                    row.cells[col].reset();
                    continue;
                }

                let actual = &grid.viewport[idx];
                let pred_ch = row.cells[col].replacement.ch();
                let actual_ch = actual.ch();

                if pred_ch == actual_ch {
                    if pred_ch != row.cells[col].original_ch
                        && row.cells[col].epoch > max_confirmed
                    {
                        max_confirmed = row.cells[col].epoch;
                    }
                    // Glitch repair: quick confirmation reduces trigger.
                    let pred_ms =
                        now.duration_since(row.cells[col].created_at).as_millis() as u64;
                    if pred_ms < GLITCH_THRESHOLD_MS && self.glitch_trigger > 0 {
                        let can_repair = self.last_quick_confirm.map_or(true, |t| {
                            now.duration_since(t).as_millis() as u64
                                >= GLITCH_REPAIR_MIN_INTERVAL_MS
                        });
                        if can_repair {
                            self.glitch_trigger -= 1;
                            self.last_quick_confirm = Some(now);
                        }
                    }
                    propagate_fg_bg = Some((actual.fg, actual.bg));
                    row.cells[col].reset();
                }
            }
        }

        overlay.confirmed_epoch = max_confirmed;

        // Update flagging (underline) with hysteresis.
        let srtt_ms = self.srtt_us / 1000;
        if srtt_ms > FLAG_TRIGGER_HIGH_MS {
            self.flagging = true;
        } else if srtt_ms <= FLAG_TRIGGER_LOW_MS {
            self.flagging = false;
        }
        if self.glitch_trigger > GLITCH_REPAIR_COUNT {
            self.flagging = true;
        }

        // --- Pass 2: detect wrong predictions ---
        let confirmed = overlay.confirmed_epoch;
        let mut kill_at: Option<u64> = None;
        let mut need_reset = false;

        for &row_num in &row_nums {
            let Some(row) = overlay.rows.get_mut(&row_num) else {
                continue;
            };
            for col in 0..row.cells.len() {
                let cell = &row.cells[col];
                if !cell.active || cell.unknown {
                    continue;
                }
                if echo_ack < cell.min_echo_ack {
                    continue;
                }
                if row_num as usize >= rows || col >= cols {
                    continue;
                }
                let idx = (row_num as usize) * cols + col;
                if idx >= grid.viewport.len() {
                    continue;
                }

                let actual_ch = grid.viewport[idx].ch();
                let pred_ch = cell.replacement.ch();

                if pred_ch != actual_ch {
                    if cell.epoch <= confirmed {
                        need_reset = true;
                    } else {
                        kill_at = Some(kill_at.map_or(cell.epoch, |e| e.min(cell.epoch)));
                    }
                }
            }
        }

        if need_reset {
            self.overlays.remove(&pane_id);
            return;
        }

        let overlay = self.overlays.get_mut(&pane_id).unwrap();
        if let Some(epoch) = kill_at {
            overlay.kill_epoch(epoch);
        }

        // Validate cursor prediction.
        if let Some(ref cur) = overlay.cursor {
            if echo_ack >= cur.min_echo_ack {
                if cur.row == grid.cursor_line && cur.col == grid.cursor_col {
                    overlay.cursor = None;
                } else {
                    self.overlays.remove(&pane_id);
                    return;
                }
            }
        }

        if let Some(overlay) = self.overlays.get_mut(&pane_id) {
            overlay.gc_rows();
            if overlay.is_empty() {
                self.overlays.remove(&pane_id);
            }
        }
    }
}
