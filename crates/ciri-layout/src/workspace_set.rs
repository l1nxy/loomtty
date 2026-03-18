use crate::column::ColumnWidth;
use crate::geometry::{Rect, ViewSize};
use crate::tile::PaneId;
use crate::workspace::Workspace;

/// A 2D grid of panes: rows × columns.
/// Each row is a horizontal strip of columns (a Workspace).
/// Vertical axis: rows stack downward, each row = viewport height.
/// Both axes scroll infinitely.
#[derive(Debug)]
pub struct WorkspaceSet {
    pub rows: Vec<Workspace>,
    pub active_row: usize,
    pub view_size: ViewSize,
    /// Vertical scroll offset (pixels). Animated by App.
    pub view_offset_y: f32,
    pub row_gap: f32,
    pub column_gap: f32,
}

impl WorkspaceSet {
    pub fn new_with_gaps(view_size: ViewSize, row_gap: f32, column_gap: f32) -> Self {
        WorkspaceSet {
            rows: vec![Workspace::new_with_gap(view_size, column_gap)],
            active_row: 0,
            view_size,
            view_offset_y: 0.0,
            row_gap,
            column_gap,
        }
    }

    pub fn new(view_size: ViewSize) -> Self {
        Self::new_with_gaps(view_size, 8.0, 8.0)
    }

    pub fn active(&self) -> &Workspace {
        &self.rows[self.active_row]
    }

    pub fn active_mut(&mut self) -> &mut Workspace {
        &mut self.rows[self.active_row]
    }

    /// Y position of a row in world coordinates.
    pub fn row_y(&self, idx: usize) -> f32 {
        idx as f32 * (self.view_size.height + self.row_gap)
    }

    /// Target view_offset_y to center the active row.
    pub fn target_offset_y(&self) -> f32 {
        let row_center = self.row_y(self.active_row) + self.view_size.height / 2.0;
        (row_center - self.view_size.height / 2.0).max(0.0)
    }

    /// Move focus up one row. Tries to keep the same column index.
    pub fn focus_up(&mut self) {
        if self.active_row > 0 {
            let col_idx = self.active().active_column_idx;
            self.active_row -= 1;
            // Clamp column index to new row's range
            let max = self.rows[self.active_row].columns.len().saturating_sub(1);
            self.rows[self.active_row].active_column_idx = col_idx.min(max);
        }
    }

    /// Move focus down one row. Tries to keep the same column index.
    pub fn focus_down(&mut self) {
        if self.active_row + 1 < self.rows.len() {
            let col_idx = self.active().active_column_idx;
            self.active_row += 1;
            let max = self.rows[self.active_row].columns.len().saturating_sub(1);
            self.rows[self.active_row].active_column_idx = col_idx.min(max);
        }
    }

    /// Add a new row below the active row with one pane, switch to it.
    pub fn add_row_below(&mut self, pane_id: PaneId) {
        let insert_at = self.active_row + 1;
        let mut ws = Workspace::new_with_gap(self.view_size, self.column_gap);
        ws.add_column_right(pane_id);
        ws.set_active_column_width(ColumnWidth::Proportion(1.0));
        self.rows.insert(insert_at, ws);
        self.active_row = insert_at;
    }

    /// Switch to row by index, creating rows if needed.
    pub fn switch_to(&mut self, idx: usize) {
        while self.rows.len() <= idx {
            self.rows.push(Workspace::new_with_gap(self.view_size, self.column_gap));
        }
        self.active_row = idx;
    }

    /// Get ALL visible tiles across all visible rows with screen coordinates.
    /// Returns (pane_id, screen_rect, is_active).
    pub fn visible_tiles_2d(&self) -> Vec<(PaneId, Rect, bool)> {
        let mut result = Vec::new();
        let vp_top = self.view_offset_y;
        let vp_bottom = vp_top + self.view_size.height;

        let active_pane = self.active().active_pane_id();

        for (row_idx, row) in self.rows.iter().enumerate() {
            let ry = self.row_y(row_idx);
            let row_h = self.view_size.height;

            // Skip rows fully off-screen
            if ry + row_h < vp_top || ry > vp_bottom {
                continue;
            }

            let screen_y = ry - self.view_offset_y;

            // Get visible columns in this row
            let vp_left = row.view_offset_x;
            let vp_right = vp_left + self.view_size.width;

            for (col_idx, col) in row.columns.iter().enumerate() {
                let col_x = row.column_x(col_idx);
                let col_w = col.effective_width(self.view_size.width);

                if col_x + col_w < vp_left || col_x > vp_right {
                    continue;
                }

                let is_active = row_idx == self.active_row && Some(col.pane_id) == active_pane;
                result.push((
                    col.pane_id,
                    Rect::new(col_x - row.view_offset_x, screen_y, col_w, row_h),
                    is_active,
                ));
            }
        }

        result
    }

    /// Get ALL tiles without culling (for overview).
    pub fn all_tiles_2d(&self) -> Vec<(PaneId, Rect, bool)> {
        let mut result = Vec::new();
        let active_pane = self.active().active_pane_id();

        for (row_idx, row) in self.rows.iter().enumerate() {
            let ry = self.row_y(row_idx) - self.view_offset_y;

            for (col_idx, col) in row.columns.iter().enumerate() {
                let col_x = row.column_x(col_idx) - row.view_offset_x;
                let col_w = col.effective_width(self.view_size.width);
                let is_active = row_idx == self.active_row && Some(col.pane_id) == active_pane;
                result.push((
                    col.pane_id,
                    Rect::new(col_x, ry, col_w, self.view_size.height),
                    is_active,
                ));
            }
        }
        result
    }

    pub fn all_pane_ids(&self) -> Vec<PaneId> {
        self.rows.iter().flat_map(|r| r.all_pane_ids()).collect()
    }

    pub fn resize_view(&mut self, size: ViewSize) {
        self.view_size = size;
        for row in &mut self.rows {
            row.resize_view(size);
        }
    }

    pub fn active_workspace_idx(&self) -> usize {
        self.active_row
    }

    /// Remove empty rows (keep at least one).
    /// If the active row is removed, focus moves to the row above (or below if at top).
    pub fn cleanup_empty(&mut self) {
        // First: if the active row itself is empty, move focus before cleanup
        if self.rows.len() > 1 && self.rows[self.active_row].is_empty() {
            if self.active_row > 0 {
                self.active_row -= 1;
            } else {
                // active_row is 0 and empty — find the first non-empty row
                for i in 1..self.rows.len() {
                    if !self.rows[i].is_empty() {
                        self.active_row = i;
                        break;
                    }
                }
            }
        }

        // Now remove all empty rows (except keep at least one)
        let mut i = 0;
        while i < self.rows.len() && self.rows.len() > 1 {
            if self.rows[i].is_empty() {
                self.rows.remove(i);
                if self.active_row > i {
                    self.active_row -= 1;
                } else if self.active_row == i {
                    // Shouldn't happen after the fix above, but clamp defensively
                    self.active_row = self.active_row.min(self.rows.len().saturating_sub(1));
                }
            } else {
                i += 1;
            }
        }
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wss() -> WorkspaceSet {
        let mut ws = WorkspaceSet::new(ViewSize { width: 1000.0, height: 600.0 });
        ws.active_mut().add_column_right(1);
        ws
    }

    #[test]
    fn focus_up_down_navigates_rows() {
        let mut ws = wss();
        ws.add_row_below(2);
        ws.add_row_below(3);
        assert_eq!(ws.active_row, 2);
        ws.focus_up();
        assert_eq!(ws.active_row, 1);
        ws.focus_down();
        assert_eq!(ws.active_row, 2);
    }

    #[test]
    fn focus_preserves_column_index() {
        let mut ws = WorkspaceSet::new(ViewSize { width: 1000.0, height: 600.0 });
        // Row 0: 3 columns
        ws.active_mut().add_column_right(1);
        ws.active_mut().add_column_right(2);
        ws.active_mut().add_column_right(3);
        // active_column_idx = 2 (rightmost)

        // Row 1: 2 columns
        ws.add_row_below(4);
        ws.active_mut().add_column_right(5);
        // active_column_idx = 1

        // Go back to row 0 — should restore column 1 (clamped from 1)
        ws.focus_up();
        assert_eq!(ws.active_row, 0);
        assert_eq!(ws.active().active_column_idx, 1); // clamped from row 1's index

        // Go to row 1
        ws.focus_down();
        assert_eq!(ws.active_row, 1);
    }

    #[test]
    fn visible_tiles_2d_basic() {
        let mut ws = wss();
        ws.add_row_below(2);
        // Both rows visible when view_offset_y = 0 (rows are at y=0 and y=608)
        // But row 1 at y=608 > viewport height 600, so it's off-screen
        let tiles = ws.visible_tiles_2d();
        // Only row 0 is visible (its pane 1)... wait, row 0 has pane 1 but view_offset_y=0
        // and row 1 is at y=608 which is > 600, so not visible
        assert_eq!(tiles.len(), 1);
    }

    #[test]
    fn target_offset_y() {
        let mut ws = wss();
        ws.add_row_below(2);
        // active_row = 1, row_y(1) = 608
        let target = ws.target_offset_y();
        assert_eq!(target, 608.0); // center of row 1 = 608+300=908, - 300 = 608
    }

    #[test]
    fn all_tiles_2d() {
        let mut ws = wss();
        ws.add_row_below(2);
        ws.active_mut().add_column_right(3);
        let all = ws.all_tiles_2d();
        assert_eq!(all.len(), 3); // row 0: pane 1, row 1: pane 2 + pane 3
    }

    #[test]
    fn cleanup_empty_focuses_previous_row() {
        let mut ws = wss(); // row 0 has pane 1
        ws.add_row_below(2); // row 1 has pane 2, active_row = 1
        ws.add_row_below(3); // row 2 has pane 3, active_row = 2
        assert_eq!(ws.active_row, 2);

        // Close the pane in active row (row 2) — making it empty
        ws.active_mut().close_pane(3);
        assert!(ws.rows[2].is_empty());

        // cleanup_empty should remove row 2 and focus row 1
        ws.cleanup_empty();
        assert_eq!(ws.rows.len(), 2);
        assert_eq!(ws.active_row, 1);
        assert_eq!(ws.active().active_pane_id(), Some(2));
    }

    #[test]
    fn cleanup_empty_active_row_0() {
        let mut ws = WorkspaceSet::new(ViewSize { width: 1000.0, height: 600.0 });
        ws.active_mut().add_column_right(1); // row 0
        ws.add_row_below(2); // row 1, active
        ws.active_row = 0; // switch back to row 0
        ws.active_mut().close_pane(1); // row 0 is now empty

        ws.cleanup_empty();
        assert_eq!(ws.rows.len(), 1);
        assert_eq!(ws.active_row, 0);
        assert_eq!(ws.active().active_pane_id(), Some(2));
    }
}
