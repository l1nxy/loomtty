use crate::column::{Column, ColumnWidth};
use crate::geometry::{Rect, ViewSize};
use crate::tile::PaneId;

/// A workspace is one horizontal row of columns.
/// Each column = one pane. No vertical stacking within a row.
#[derive(Debug)]
pub struct Workspace {
    pub columns: Vec<Column>,
    pub active_column_idx: usize,
    pub view_size: ViewSize,
    pub view_offset_x: f32,
    pub column_gap: f32,
}

impl Workspace {
    pub fn new(view_size: ViewSize) -> Self {
        Workspace {
            columns: Vec::new(),
            active_column_idx: 0,
            view_size,
            view_offset_x: 0.0,
            column_gap: 8.0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    pub fn active_pane_id(&self) -> Option<PaneId> {
        self.columns.get(self.active_column_idx).map(|c| c.pane_id)
    }

    pub fn column_x(&self, idx: usize) -> f32 {
        let mut x = 0.0;
        for i in 0..idx.min(self.columns.len()) {
            x += self.columns[i].effective_width(self.view_size.width) + self.column_gap;
        }
        x
    }

    pub fn total_width(&self) -> f32 {
        if self.columns.is_empty() {
            return 0.0;
        }
        let mut w = 0.0;
        for col in &self.columns {
            w += col.effective_width(self.view_size.width);
        }
        w += (self.columns.len() - 1) as f32 * self.column_gap;
        w
    }

    pub fn target_offset_for_active(&self) -> f32 {
        let Some(col) = self.columns.get(self.active_column_idx) else {
            return 0.0;
        };
        let vw = self.view_size.width;
        let col_w = col.effective_width(vw);
        let col_center = self.column_x(self.active_column_idx) + col_w / 2.0;
        let centered = col_center - vw / 2.0;
        // Clamp: don't scroll past the end of content (no blank space on right)
        let max_offset = (self.total_width() - vw).max(0.0);
        centered.clamp(0.0, max_offset)
    }

    /// Get visible columns as (pane_id, screen_rect, is_active).
    pub fn visible_tiles(&self) -> Vec<(PaneId, Rect, bool)> {
        let mut result = Vec::new();
        let vp_left = self.view_offset_x;
        let vp_right = vp_left + self.view_size.width;
        let active_pane = self.active_pane_id();

        for (col_idx, col) in self.columns.iter().enumerate() {
            let col_x = self.column_x(col_idx);
            let col_w = col.effective_width(self.view_size.width);

            if col_x + col_w < vp_left || col_x > vp_right {
                continue;
            }

            let rect = Rect::new(
                col_x - self.view_offset_x,
                0.0,
                col_w,
                self.view_size.height,
            );
            let is_active = Some(col.pane_id) == active_pane;
            result.push((col.pane_id, rect, is_active));
        }
        result
    }

    /// Get ALL columns without viewport culling (for overview).
    pub fn all_tiles_unculled(&self) -> Vec<(PaneId, Rect, bool)> {
        let active_pane = self.active_pane_id();
        self.columns.iter().enumerate().map(|(i, col)| {
            let x = self.column_x(i) - self.view_offset_x;
            let w = col.effective_width(self.view_size.width);
            let is_active = Some(col.pane_id) == active_pane;
            (col.pane_id, Rect::new(x, 0.0, w, self.view_size.height), is_active)
        }).collect()
    }

    pub fn add_column_right(&mut self, pane_id: PaneId) {
        let insert_at = if self.columns.is_empty() {
            0
        } else {
            self.active_column_idx + 1
        };
        self.columns.insert(insert_at, Column::new(pane_id));
        self.active_column_idx = insert_at;
        self.auto_size_new_column();
    }

    /// Set the width of the newly inserted column.
    /// Existing columns keep their widths; the camera scrolls to reveal the new one.
    /// Camera clamping ensures no blank space on the right.
    fn auto_size_new_column(&mut self) {
        let n = self.columns.len();
        if n == 0 { return; }
        if n == 1 {
            if let Some(col) = self.columns.first_mut() {
                col.width = ColumnWidth::Proportion(1.0);
            }
        } else if let Some(col) = self.columns.get_mut(self.active_column_idx) {
            col.width = ColumnWidth::Proportion(2.0 / 3.0);
        }
    }

    pub fn close_pane(&mut self, pane_id: PaneId) -> Option<PaneId> {
        if let Some(idx) = self.columns.iter().position(|c| c.pane_id == pane_id) {
            self.columns.remove(idx);
            if self.columns.is_empty() {
                self.active_column_idx = 0;
            } else if idx < self.active_column_idx {
                self.active_column_idx -= 1;
            } else if self.active_column_idx >= self.columns.len() {
                self.active_column_idx = self.columns.len() - 1;
            }
            return Some(pane_id);
        }
        None
    }

    pub fn close_active_pane(&mut self) -> Option<PaneId> {
        let pane_id = self.active_pane_id()?;
        self.close_pane(pane_id)
    }

    pub fn focus_left(&mut self) {
        if self.active_column_idx > 0 {
            self.active_column_idx -= 1;
        }
    }

    pub fn focus_right(&mut self) {
        if self.active_column_idx + 1 < self.columns.len() {
            self.active_column_idx += 1;
        }
    }

    pub fn move_pane_left(&mut self) {
        if self.active_column_idx > 0 {
            self.columns.swap(self.active_column_idx, self.active_column_idx - 1);
            self.active_column_idx -= 1;
        }
    }

    pub fn move_pane_right(&mut self) {
        if self.active_column_idx + 1 < self.columns.len() {
            self.columns.swap(self.active_column_idx, self.active_column_idx + 1);
            self.active_column_idx += 1;
        }
    }

    pub fn set_active_column_width(&mut self, width: ColumnWidth) {
        if let Some(col) = self.columns.get_mut(self.active_column_idx) {
            col.width = width;
        }
    }

    /// Resize the active column by a proportion delta.
    /// Clamped to 10%-100% of viewport.
    pub fn resize_active_column(&mut self, delta_proportion: f64) {
        if let Some(col) = self.columns.get_mut(self.active_column_idx) {
            let current_proportion = match col.width {
                ColumnWidth::Proportion(p) => p,
                ColumnWidth::Fixed(px) => {
                    if self.view_size.width > 0.0 {
                        px / self.view_size.width as f64
                    } else {
                        0.5
                    }
                }
            };
            let new_proportion = (current_proportion + delta_proportion).clamp(0.1, 1.0);
            col.width = ColumnWidth::Proportion(new_proportion);
        }
    }

    /// Set a specific column's width by index (not just the active one).
    pub fn set_column_width_by_index(&mut self, idx: usize, width: ColumnWidth) {
        if let Some(col) = self.columns.get_mut(idx) {
            col.width = width;
        }
    }

    pub fn resize_view(&mut self, size: ViewSize) {
        self.view_size = size;
    }

    pub fn all_pane_ids(&self) -> Vec<PaneId> {
        self.columns.iter().map(|c| c.pane_id).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws() -> Workspace {
        Workspace::new(ViewSize { width: 1000.0, height: 600.0 })
    }

    #[test]
    fn add_columns() {
        let mut w = ws();
        w.add_column_right(1);
        w.add_column_right(2);
        w.add_column_right(3);
        assert_eq!(w.columns.len(), 3);
        assert_eq!(w.active_column_idx, 2);
        assert_eq!(w.active_pane_id(), Some(3));
    }

    #[test]
    fn focus_navigation() {
        let mut w = ws();
        w.add_column_right(1);
        w.add_column_right(2);
        w.add_column_right(3);
        w.focus_left();
        assert_eq!(w.active_column_idx, 1);
        w.focus_left();
        assert_eq!(w.active_column_idx, 0);
        w.focus_left();
        assert_eq!(w.active_column_idx, 0);
        w.focus_right();
        assert_eq!(w.active_column_idx, 1);
    }

    #[test]
    fn close_pane_adjusts_index() {
        let mut w = ws();
        w.add_column_right(1);
        w.add_column_right(2);
        w.add_column_right(3);
        w.close_pane(1);
        assert_eq!(w.active_column_idx, 1);
        assert_eq!(w.active_pane_id(), Some(3));
    }

    #[test]
    fn close_last_pane() {
        let mut w = ws();
        w.add_column_right(1);
        w.close_pane(1);
        assert!(w.is_empty());
    }

    #[test]
    fn visible_tiles() {
        let mut w = ws();
        w.add_column_right(1);
        w.add_column_right(2);
        // First column is full-width (1000), second is half (500) and off-screen
        // so only 1 tile is visible without scrolling; 2 are visible if scrolled
        let tiles = w.all_tiles_unculled();
        assert_eq!(tiles.len(), 2);
    }

    #[test]
    fn move_pane() {
        let mut w = ws();
        w.add_column_right(1);
        w.add_column_right(2);
        w.add_column_right(3);
        w.move_pane_left();
        assert_eq!(w.active_column_idx, 1);
        assert_eq!(w.all_pane_ids(), vec![1, 3, 2]);
    }

    #[test]
    fn column_x_calculation() {
        let mut w = ws();
        w.add_column_right(1);
        w.add_column_right(2);
        // Col 0 = Proportion(1.0) = 1000px, col 1 at 1000+8=1008
        assert_eq!(w.column_x(0), 0.0);
        assert_eq!(w.column_x(1), 1008.0);
    }
}
