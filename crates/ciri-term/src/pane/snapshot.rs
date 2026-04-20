use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use ciri_protocol::message::*;
use std::hash::{Hash, Hasher};

use super::cell::{
    collect_scrollback_cells, collect_viewport_cells, full_damage_rows, pack_cell,
    partial_damage_rows,
};
use super::{Pane, SnapshotScrollback, cursor_shape_to_u8};

impl Pane {
    pub fn snapshot_incremental(&self, generation: u64, history_sent: usize) -> FullPaneSync {
        let scrollback =
            SnapshotScrollback::incremental(self.term.grid().history_size(), history_sent);
        // Tick layer owns the incremental `scrollback_replace` decision (it
        // knows the watermark delta vs history_size) and overwrites this.
        self.build_snapshot(generation, scrollback, false)
    }

    pub fn snapshot(&self, generation: u64) -> FullPaneSync {
        let scrollback = SnapshotScrollback::full(self.term.grid().history_size());
        // Full snapshots happen on attach / session switch; clients must
        // discard stale scrollback before appending to avoid duplication.
        //
        // Exception: in alt-screen `history_size()` reflects only the alt
        // buffer (always 0) and does not describe the primary-screen history
        // the client may already hold. Telling the client to replace would
        // permanently drop that history, because `history_sent` is then
        // advanced to `scrollback_total` and the rows are never resent.
        let replace = !self.is_alt_screen();
        self.build_snapshot(generation, scrollback, replace)
    }

    fn build_snapshot(
        &self,
        generation: u64,
        scrollback: SnapshotScrollback,
        scrollback_replace: bool,
    ) -> FullPaneSync {
        let term = &self.term;
        let grid = term.grid();
        let cols = grid.columns();
        let rows = grid.screen_lines();
        let content = term.renderable_content();

        let (cells, mut grapheme_extras, hyperlink_extras) = collect_viewport_cells(grid, rows, cols);
        let (sb_cells, mut sb_grapheme_extras) = collect_scrollback_cells(grid, cols, scrollback.rows);
        let scrollback_cell_count = sb_cells.len() as u32;
        for (idx, extra) in grapheme_extras.0.drain(..) {
            sb_grapheme_extras.0.push((scrollback_cell_count + idx, extra));
        }
        let grapheme_extras = sb_grapheme_extras;

        FullPaneSync {
            meta: PaneFrameMeta {
                pane_id: self.id,
                generation,
                cursor_line: content.cursor.point.line.0 as i16,
                cursor_col: content.cursor.point.column.0 as u16,
                cursor_shape: cursor_shape_to_u8(content.cursor.shape),
                mode_flags: self.mode_flags_from_term(term),
                echo_ack: 0,
            },
            cols: cols as u16,
            rows: rows as u16,
            title: self.title.clone(),
            scrollback: sb_cells,
            scrollback_rows: scrollback.rows as u32,
            scrollback_replace,
            cells,
            grapheme_extras,
            hyperlink_extras,
            cwd: self.cwd().map(|s| s.to_string()),
        }
    }

    pub(super) fn damage_bounds(&self) -> Option<(usize, u16)> {
        let cols = self.term.grid().columns();
        let total_rows = self.term.grid().screen_lines();
        if cols == 0 || total_rows == 0 {
            None
        } else {
            Some((total_rows, cols.saturating_sub(1) as u16))
        }
    }

    pub fn extract_damage(&mut self) -> Option<Vec<(u16, u16, u16)>> {
        let Some((total_rows, right)) = self.damage_bounds() else {
            self.term.reset_damage();
            return None;
        };

        use alacritty_terminal::term::TermDamage;
        let ranges = match self.term.damage() {
            TermDamage::Full => full_damage_rows(total_rows, right),
            TermDamage::Partial(iter) => partial_damage_rows(iter, right),
        };

        self.term.reset_damage();
        if ranges.is_empty() {
            None
        } else {
            Some(ranges)
        }
    }

    pub fn viewport_row_fingerprint(&self, line: u16) -> Option<u64> {
        let grid = self.term.grid();
        let rows = grid.screen_lines();
        let cols = grid.columns();
        let line = line as usize;
        if cols == 0 || line >= rows {
            return None;
        }

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for col in 0..cols {
            let point = Point::new(Line(line as i32), Column(col));
            let cell = pack_cell(&grid[point]);
            cell.ch_bytes.hash(&mut hasher);
            [cell.fg.tag, cell.fg.b1, cell.fg.b2, cell.fg.b3].hash(&mut hasher);
            [cell.bg.tag, cell.bg.b1, cell.bg.b2, cell.bg.b3].hash(&mut hasher);
            cell.flags.hash(&mut hasher);
        }
        Some(hasher.finish())
    }

    pub fn write_cells_into_sm(
        &self,
        line: u16,
        left: u16,
        right: u16,
        encoder: &mut ciri_protocol::codec::StateEncoder,
    ) {
        let grid = self.term.grid();
        for col in left..=right {
            let point = Point::new(Line(line as i32), Column(col as usize));
            encoder.push_cell(&pack_cell(&grid[point]));
        }
    }
}
