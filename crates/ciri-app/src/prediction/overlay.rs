use ciri_protocol::message::PackedCell;
use std::collections::HashMap;
use std::time::Instant;

use super::PREDICTION_TIMEOUT_SECS;
use super::utf8::Utf8Accum;
use crate::grid::ClientPaneGrid;

#[derive(Clone)]
pub(super) struct OverlayCell {
    pub active: bool,
    pub replacement: PackedCell,
    pub epoch: u64,
    pub created_at: Instant,
    pub min_echo_ack: u64,
    pub original_ch: char,
    pub unknown: bool,
}

impl OverlayCell {
    pub fn inactive(now: Instant) -> Self {
        Self {
            active: false,
            replacement: PackedCell::default(),
            epoch: 0,
            created_at: now,
            min_echo_ack: 0,
            original_ch: '\0',
            unknown: false,
        }
    }

    pub fn reset(&mut self) {
        self.active = false;
        self.unknown = false;
    }
}

pub(super) struct OverlayRow {
    pub cells: Vec<OverlayCell>,
}

impl OverlayRow {
    pub fn new(cols: u16) -> Self {
        let now = Instant::now();
        Self {
            cells: (0..cols).map(|_| OverlayCell::inactive(now)).collect(),
        }
    }

    pub fn has_active(&self) -> bool {
        self.cells.iter().any(|c| c.active)
    }
}

pub struct PredictedCursor {
    pub row: i16,
    pub col: u16,
    pub epoch: u64,
    pub min_echo_ack: u64,
    pub created_at: Instant,
}

pub struct PaneOverlay {
    pub(super) rows: HashMap<u16, OverlayRow>,
    /// Predicted cursor history, sorted by epoch ascending. The back is the
    /// "current" predicted position. Mirrors mosh's `std::list<ConditionalCursorMove> cursors`
    /// (terminaloverlay.cc) — keeping a per-epoch trail lets validation drop
    /// older mispredictions without resetting the whole overlay, and only
    /// reset when the newest cursor itself is wrong.
    pub cursors: Vec<PredictedCursor>,
    pub(super) local_edit_start: Option<(i16, u16)>,
    pub prediction_epoch: u64,
    pub confirmed_epoch: u64,
    pub(super) cols: u16,
    pub(super) utf8: Utf8Accum,
    /// Last server-cursor position that capped a misprediction. Set inside
    /// `on_server_sync` when a "predicted past server cursor" cap fires
    /// (e.g. Backspace at the prompt boundary). The input path consults this
    /// before further Backspaces — if the new predicted cursor would land
    /// at-or-before the floor, no overlay change is made at all, just an
    /// epoch bump. This kills the rubber-banding cycle where each Backspace
    /// would predict-then-cap, predict-then-cap, predict-then-cap, etc.
    ///
    /// Cleared automatically on the next sync where the server cursor has
    /// actually moved (signalling that the shell has now honoured some
    /// input and the floor is no longer authoritative).
    pub(super) cap_floor: Option<(i16, u16)>,
}

impl PaneOverlay {
    pub fn new(cols: u16) -> Self {
        Self {
            rows: HashMap::new(),
            cursors: Vec::new(),
            local_edit_start: None,
            prediction_epoch: 1,
            confirmed_epoch: 0,
            cols,
            utf8: Utf8Accum::new(),
            cap_floor: None,
        }
    }

    pub fn increment_epoch(&mut self) {
        self.prediction_epoch += 1;
    }

    pub(super) fn get_or_make_row(&mut self, row: u16) -> &mut OverlayRow {
        self.rows
            .entry(row)
            .or_insert_with(|| OverlayRow::new(self.cols))
    }

    pub(super) fn get_cell(&self, row: u16, col: u16) -> Option<&OverlayCell> {
        self.rows
            .get(&row)
            .and_then(|r| r.cells.get(col as usize))
            .filter(|c| c.active)
    }

    pub fn effective_cell(&self, grid: &ClientPaneGrid, row: u16, col: u16) -> PackedCell {
        if let Some(c) = self.get_cell(row, col) {
            return c.replacement;
        }
        let idx = (row as usize) * (grid.cols as usize) + (col as usize);
        grid.viewport.get(idx).copied().unwrap_or_default()
    }

    /// Last (most recent) predicted cursor, if any.
    pub(super) fn latest_cursor(&self) -> Option<&PredictedCursor> {
        self.cursors.last()
    }

    /// Update (or push, on epoch change) the working cursor used by input
    /// handlers. If the back already lives in `prediction_epoch`, mutate it
    /// in place. Otherwise push a fresh entry at the new epoch, matching
    /// mosh's `init_cursor` semantics.
    pub(super) fn set_or_push_cursor(
        &mut self,
        row: i16,
        col: u16,
        min_echo_ack: u64,
        now: Instant,
    ) {
        let epoch = self.prediction_epoch;
        if let Some(back) = self.cursors.last_mut()
            && back.epoch == epoch
        {
            back.row = row;
            back.col = col;
            back.min_echo_ack = min_echo_ack;
            back.created_at = now;
            return;
        }
        self.cursors.push(PredictedCursor {
            row,
            col,
            epoch,
            min_echo_ack,
            created_at: now,
        });
    }

    pub fn kill_epoch(&mut self, epoch: u64) {
        for row in self.rows.values_mut() {
            for cell in &mut row.cells {
                if cell.active && cell.epoch >= epoch {
                    cell.reset();
                }
            }
        }
        self.cursors.retain(|c| c.epoch < epoch);
        if self.prediction_epoch < epoch + 1 {
            self.prediction_epoch = epoch + 1;
        }
    }

    pub fn expire_old(&mut self, now: Instant) {
        for row in self.rows.values_mut() {
            for cell in &mut row.cells {
                if cell.active
                    && now.duration_since(cell.created_at).as_secs() >= PREDICTION_TIMEOUT_SECS
                {
                    cell.reset();
                }
            }
        }
        // Cursor reaping rules:
        //   1. Hard age cutoff still applies to every cursor.
        //   2. The **back** cursor (the one we render) is always preserved
        //      even when it has no peer cells. Cursor-only epochs like a CR
        //      after a printable land here — dropping them would let
        //      validation treat an older cursor as the back and trigger a
        //      spurious reset when the server cursor matches the latest
        //      prediction.
        //   3. Older cursors with no peer at their epoch are stale history
        //      and get pruned.
        //   4. With a hidden-edit overlay (`local_edit_start` set), keep
        //      every cursor — these overlays may legitimately have no
        //      cells in `rows` because Never mode suppresses them.
        let has_edit_start = self.local_edit_start.is_some();
        let rows = &self.rows;
        let last_idx = self.cursors.len().saturating_sub(1);
        let mut idx = 0;
        self.cursors.retain(|cur| {
            let i = idx;
            idx += 1;
            if now.duration_since(cur.created_at).as_secs() >= PREDICTION_TIMEOUT_SECS {
                return false;
            }
            if i == last_idx || has_edit_start {
                return true;
            }
            rows.values()
                .any(|r| r.cells.iter().any(|c| c.active && c.epoch == cur.epoch))
        });
    }

    pub fn cursor_position(&self) -> Option<(i16, u16)> {
        self.cursors.last().map(|c| (c.row, c.col))
    }

    pub fn is_empty(&self) -> bool {
        self.cursors.is_empty() && !self.rows.values().any(|r| r.has_active())
    }

    pub fn gc_rows(&mut self) {
        self.rows.retain(|_, r| r.has_active());
    }
}
