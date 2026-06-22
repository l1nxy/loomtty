use loom_protocol::message::PackedColor;
use std::time::Instant;

use super::{
    FLAG_TRIGGER_HIGH_MS, FLAG_TRIGGER_LOW_MS, GLITCH_FLAG_THRESHOLD_MS, GLITCH_REPAIR_COUNT,
    GLITCH_REPAIR_MIN_INTERVAL_MS, GLITCH_THRESHOLD_MS, PredictionEngine,
};
use crate::grid::ClientPaneGrid;

impl PredictionEngine {
    /// Validate predictions against authoritative server state using **dual ack**:
    ///   - `received_ack`: highest seq the server has *received* (Overwatch-style
    ///     packet-level ack, bumped synchronously in `handle_input` without
    ///     waiting for PTY output). Used for cursor validation, because the
    ///     server's reported cursor is authoritative the moment the input was
    ///     received — even when the shell silently drops the byte (Backspace at
    ///     prompt boundary, where late_ack would never advance).
    ///   - `echo_ack`: highest seq with PTY output drained. Used for cell
    ///     validation, since only echoed output reveals what the shell actually
    ///     wrote to its grid.
    ///
    /// Two-pass algorithm:
    /// 1. Confirm correct predictions, update glitch trigger, propagate renditions
    /// 2. Detect wrong predictions — kill epoch or reset entirely
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

        // Validate cursor predictions in epoch order. Use `received_ack`
        // (server-acked-on-receipt) instead of `echo_ack` for cursor decisions:
        // the server's reported cursor reflects PTY truth the moment the
        // input was received, so we don't need to wait for PTY output to
        // catch a misprediction. This is the fix for "hold Backspace at the
        // prompt boundary makes the predicted cursor walk past column 0
        // forever" — `echo_ack` would never advance there because the shell
        // emits nothing, but `received_ack` advances on every keystroke.
        //
        // Mismatch policy:
        //   - **Predicted past server cursor** (Backspace-past-prompt case):
        //     prediction was too aggressive; the shell silently rejected it.
        //     Kill the cursor's epoch (drops the bad cells + cursor without
        //     blowing up the whole overlay). Other epochs survive.
        //   - **Otherwise** (e.g. shell jumped cursor unexpectedly):
        //     keep the old behaviour — drop older mismatched cursors silently;
        //     catastrophic-reset on a back-cursor mismatch.
        let mut catastrophic = false;
        let mut idx = 0;
        while idx < overlay.cursors.len() {
            let cur = &overlay.cursors[idx];
            let matched = cur.row == grid.cursor_line && cur.col == grid.cursor_col;
            let predicted_past_server = (cur.row, cur.col) < (grid.cursor_line, grid.cursor_col);
            let received = received_ack >= cur.min_echo_ack;
            let echoed = echo_ack >= cur.min_echo_ack;
            let is_back = idx + 1 == overlay.cursors.len();

            if !received {
                idx += 1;
                continue;
            }

            if matched {
                // Server confirms the position — drop and move on.
                overlay.cursors.remove(idx);
                continue;
            }

            if predicted_past_server {
                // Soft cap: server received the input but its cursor is
                // *forward* of our prediction. The only way this happens is
                // when the shell silently rejected what we predicted to
                // delete (Backspace at the prompt boundary). Kill the bad
                // epoch's cells and cursor; preserve hidden-edit anchors and
                // other epochs.
                //
                // Record `cap_floor` so the input path can refuse follow-up
                // Backspaces before they cause another predict-then-cap
                // round trip (the rubber-banding artefact).
                let bad_epoch = cur.epoch;
                overlay.cap_floor = Some((grid.cursor_line, grid.cursor_col));
                overlay.kill_epoch(bad_epoch);
                continue;
            }

            // Predicted cursor is *ahead* of the server cursor. This is the
            // normal forward-speculation case — server may not have echoed
            // PTY yet. Wait for `echo_ack` before declaring this wrong.
            if !echoed {
                idx += 1;
                continue;
            }

            // PTY drained past this seq AND the cursor still doesn't match
            // — the prediction was genuinely wrong. Old behaviour: drop
            // history cursors silently, catastrophic-reset on the back.
            if is_back {
                catastrophic = true;
                break;
            }
            overlay.cursors.remove(idx);
        }
        if catastrophic {
            self.overlays.remove(&pane_id);
            self.force_visible_panes.remove(&pane_id);
            return;
        }

        if let Some(overlay) = self.overlays.get_mut(&pane_id) {
            overlay.gc_rows();
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
