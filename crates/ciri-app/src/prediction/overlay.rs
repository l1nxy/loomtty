use ciri_protocol::message::PackedCell;
use std::collections::HashMap;
use std::time::Instant;

use super::utf8::Utf8Accum;
use super::PREDICTION_TIMEOUT_SECS;
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
}

pub struct PaneOverlay {
    pub(super) rows: HashMap<u16, OverlayRow>,
    pub cursor: Option<PredictedCursor>,
    pub prediction_epoch: u64,
    pub confirmed_epoch: u64,
    pub(super) cols: u16,
    pub(super) utf8: Utf8Accum,
}

impl PaneOverlay {
    pub fn new(cols: u16) -> Self {
        Self {
            rows: HashMap::new(),
            cursor: None,
            prediction_epoch: 1,
            confirmed_epoch: 0,
            cols,
            utf8: Utf8Accum::new(),
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

    pub fn kill_epoch(&mut self, epoch: u64) {
        for row in self.rows.values_mut() {
            for cell in &mut row.cells {
                if cell.active && cell.epoch >= epoch {
                    cell.reset();
                }
            }
        }
        if self.cursor.as_ref().is_some_and(|c| c.epoch >= epoch) {
            self.cursor = None;
        }
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
        if let Some(ref cur) = self.cursor {
            let epoch = cur.epoch;
            let has_peer = self
                .rows
                .values()
                .any(|r| r.cells.iter().any(|c| c.active && c.epoch == epoch));
            if !has_peer {
                self.cursor = None;
            }
        }
    }

    pub fn cursor_position(&self) -> Option<(i16, u16)> {
        self.cursor.as_ref().map(|c| (c.row, c.col))
    }

    pub fn is_empty(&self) -> bool {
        self.cursor.is_none() && !self.rows.values().any(|r| r.has_active())
    }

    pub fn gc_rows(&mut self) {
        self.rows.retain(|_, r| r.has_active());
    }
}
