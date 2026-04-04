use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use ciri_protocol::message::*;

use super::cell::{collect_scrollback_cells, collect_viewport_cells, full_damage_rows, pack_cell, partial_damage_rows};
use super::{Pane, SnapshotScrollback, cursor_shape_to_u8};

impl Pane {
    pub fn snapshot_incremental(&self, generation: u64, history_sent: usize) -> FullPaneSync {
        let scrollback =
            SnapshotScrollback::incremental(self.term.grid().history_size(), history_sent);
        self.build_snapshot(generation, scrollback)
    }

    pub fn snapshot(&self, generation: u64) -> FullPaneSync {
        let scrollback = SnapshotScrollback::full(self.term.grid().history_size());
        self.build_snapshot(generation, scrollback)
    }

    fn build_snapshot(&self, generation: u64, scrollback: SnapshotScrollback) -> FullPaneSync {
        let term = &self.term;
        let grid = term.grid();
        let cols = grid.columns();
        let rows = grid.screen_lines();
        let content = term.renderable_content();

        let (cells, grapheme_extras) = collect_viewport_cells(grid, rows, cols);
        let sb_cells = collect_scrollback_cells(grid, cols, scrollback.rows);

        let hyperlink_extras = {
            let link_map = self.parsers.osc8.link_map();
            if link_map.is_empty() {
                HyperlinkExtras::new()
            } else {
                HyperlinkExtras {
                    cell_links: Vec::new(),
                    link_map: link_map.to_vec(),
                }
            }
        };

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
            scrollback_replace: false,
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
