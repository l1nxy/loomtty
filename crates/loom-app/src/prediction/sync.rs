use loom_protocol::message::PackedColor;
use std::time::Instant;

use super::{
    FLAG_TRIGGER_HIGH_MS, FLAG_TRIGGER_LOW_MS, GLITCH_FLAG_THRESHOLD_MS, GLITCH_REPAIR_COUNT,
    GLITCH_REPAIR_MIN_INTERVAL_MS, GLITCH_THRESHOLD_MS, PredictionEngine,
};
use crate::grid::ClientPaneGrid;

impl PredictionEngine {
    /// Reconcile against PTY output. Receipt alone does not prove that the
    /// shell has processed an input, even when its cursor has not moved.
    /// Confirm cells first, then reject mature mismatches and cursor history.
    pub fn on_server_sync(
        &mut self,
        pane_id: u64,
        grid: &ClientPaneGrid,
        received_ack: u64,
        echo_ack: u64,
    ) {
        if self.overlays.contains_key(&pane_id) {
            self.bump_visual_serial_for_pane(pane_id);
        }

        // Reset on terminal resize.
        let dims = (grid.cols, grid.rows);
        let prev = self.last_dims.insert(pane_id, dims);
        if prev.is_some_and(|d| d != dims) {
            self.overlays.remove(&pane_id);
            self.force_visible_panes.remove(&pane_id);
            return;
        }

        let Some(overlay) = self.overlays.get_mut(&pane_id) else {
            return;
        };
        let cursor = (grid.cursor_line, grid.cursor_col);
        if overlay.context_flags != Self::context_flags(grid)
            || grid.password_input
            || (overlay.needs_echo_evidence
                && (overlay
                    .local_edit_start
                    .is_some_and(|start| start.0 != cursor.0)
                    || (overlay.is_empty()
                        && overlay.last_server_cursor.is_some_and(|old| old != cursor))))
        {
            self.overlays.remove(&pane_id);
            self.force_visible_panes.remove(&pane_id);
            return;
        }
        overlay.last_server_cursor = Some(cursor);
        // Clear the cap floor as soon as the server cursor moves off it.
        // The floor is only authoritative while the shell stays put; once
        // it moves, subsequent Backspaces become legitimately predictable
        // again.
        if let Some(floor) = overlay.cap_floor
            && (grid.cursor_line, grid.cursor_col) != floor
        {
            overlay.cap_floor = None;
        }
        if overlay.is_empty() && overlay.cap_floor.is_none() {
            overlay.expires_at = None;
            return;
        }

        let now = Instant::now();
        overlay.expire_old(now);

        let cols = grid.cols as usize;
        let rows = grid.rows as usize;

        // --- Pass 1: confirm correct predictions ---
        let mut max_confirmed = overlay.confirmed_epoch;
        let mut text_echo = false;

        for (&row_num, row) in &mut overlay.rows {
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
                    text_echo |= row.cells[col].typed
                        && !pred_ch.is_whitespace()
                        && pred_ch != row.cells[col].original_ch;
                    if pred_ch != row.cells[col].original_ch && row.cells[col].epoch > max_confirmed
                    {
                        max_confirmed = row.cells[col].epoch;
                    }
                    // Glitch repair: quick confirmation reduces trigger.
                    let pred_ms = now.duration_since(row.cells[col].created_at).as_millis() as u64;
                    if pred_ms < GLITCH_THRESHOLD_MS && self.glitch_trigger > 0 {
                        let can_repair = self.last_quick_confirm.is_none_or(|t| {
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

        if text_echo && overlay.local_edit_start.is_some() {
            overlay.local_edit_confirmed = true;
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

        for (&row_num, row) in &mut overlay.rows {
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
            self.force_visible_panes.remove(&pane_id);
            return;
        }

        let Some(overlay) = self.overlays.get_mut(&pane_id) else {
            // Pane was removed concurrently (e.g. closed between event dispatch).
            return;
        };
        if let Some(epoch) = kill_at {
            overlay.kill_epoch(epoch);
        }

        // Only echoed cursors can be reconciled. A received-only frame can
        // precede a legitimate Backspace/Left echo by an arbitrary duration.
        // Old cursor positions are history, not bounds: a coalesced frame can
        // have advanced past them while later inputs are still in flight.
        let last_idx = overlay.cursors.len().saturating_sub(1);
        let mut catastrophic = false;
        let mut cap_epoch = None;
        let mut idx = 0;
        overlay.cursors.retain(|cur| {
            let is_back = idx == last_idx;
            idx += 1;
            if received_ack < cur.min_echo_ack || echo_ack < cur.min_echo_ack {
                return true;
            }
            let matched = (cur.row, cur.col) == (grid.cursor_line, grid.cursor_col);
            if !matched && is_back {
                if (cur.row, cur.col) < (grid.cursor_line, grid.cursor_col) {
                    cap_epoch = Some(cur.epoch);
                } else {
                    catastrophic = true;
                }
            }
            false
        });
        if let Some(epoch) = cap_epoch {
            overlay.cap_floor = Some((grid.cursor_line, grid.cursor_col));
            overlay.kill_epoch(epoch);
        }
        if catastrophic {
            self.overlays.remove(&pane_id);
            self.force_visible_panes.remove(&pane_id);
            return;
        }

        if let Some(overlay) = self.overlays.get_mut(&pane_id) {
            overlay.gc_rows();
            overlay.refresh_expiry();
            if overlay.is_empty() {
                self.force_visible_panes.remove(&pane_id);
                if overlay.local_edit_start.is_none() && overlay.cap_floor.is_none() {
                    self.overlays.remove(&pane_id);
                } else {
                    // The overlay is being kept around purely as the
                    // anchor for a follow-up force_visible Backspace.
                    // Advance `prediction_epoch` past `confirmed_epoch`
                    // so the *next* input lands in a fresh, tentative
                    // epoch — without this, loom's Adaptive/Always
                    // contract that "a freshly typed character starts
                    // tentative after a sync" would silently break
                    // whenever local_edit_start was set.
                    if overlay.prediction_epoch <= overlay.confirmed_epoch {
                        overlay.prediction_epoch = overlay.confirmed_epoch + 1;
                    }
                }
            }
        }
    }
}
