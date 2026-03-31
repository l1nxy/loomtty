use crate::column::{Column, ColumnWidth};
use crate::geometry::{Rect, ViewSize};
use crate::tile::{PaneId, TileHeight};

const MIN_COLUMN_PROPORTION: f64 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CenterStrategy {
    Always,
    OnOverflow,
    Never,
}

/// A workspace is one horizontal row of columns.
/// Each column = one pane. No vertical stacking within a row.
#[derive(Debug)]
pub struct Workspace {
    pub columns: Vec<Column>,
    pub active_column_idx: usize,
    pub view_size: ViewSize,
    pub column_gap: f32,
}

impl Workspace {
    fn active_column_idx_checked(&self) -> Option<usize> {
        (self.active_column_idx < self.columns.len()).then_some(self.active_column_idx)
    }

    fn clamp_active_column_idx(&mut self) {
        self.active_column_idx = self
            .active_column_idx
            .min(self.columns.len().saturating_sub(1));
    }

    pub fn new_with_gap(view_size: ViewSize, column_gap: f32) -> Self {
        Workspace {
            columns: Vec::new(),
            active_column_idx: 0,
            view_size,
            column_gap,
        }
    }

    pub fn new(view_size: ViewSize) -> Self {
        Self::new_with_gap(view_size, 8.0)
    }

    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    pub fn active_pane_id(&self) -> Option<PaneId> {
        self.columns
            .get(self.active_column_idx)
            .map(|c| c.active_pane_id())
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

    /// Compute target viewport offset for the active column.
    /// `current_offset` is the current (or animated) view offset X, needed for `Never` strategy.
    pub fn target_offset_for_active_with_strategy(
        &self,
        center: CenterStrategy,
        current_offset: f32,
    ) -> f32 {
        let Some(active_column_idx) = self.active_column_idx_checked() else {
            return 0.0;
        };
        let col = &self.columns[active_column_idx];
        let vw = self.view_size.width;
        let col_w = col.effective_width(vw);
        let col_x = self.column_x(active_column_idx);
        let max_offset = (self.total_width() - vw).max(0.0);

        let should_center = match center {
            CenterStrategy::Always => true,
            CenterStrategy::OnOverflow => col_w > vw,
            CenterStrategy::Never => false,
        };

        if should_center {
            let col_center = col_x + col_w / 2.0;
            let centered = col_center - vw / 2.0;
            centered.clamp(0.0, max_offset)
        } else {
            // Ensure the active column is fully visible, minimal scrolling.
            let left = col_x;
            let right = col_x + col_w;
            let mut offset = current_offset;
            if right > offset + vw {
                offset = right - vw;
            }
            if left < offset {
                offset = left;
            }
            offset.clamp(0.0, max_offset)
        }
    }

    /// Backward-compatible: always center.
    pub fn target_offset_for_active(&self) -> f32 {
        self.target_offset_for_active_with_strategy(CenterStrategy::Always, 0.0)
    }

    /// Get visible tiles as (pane_id, screen_rect, is_active).
    /// Multi-tile columns return one entry per tile, splitting column height by weight.
    pub fn visible_tiles(&self, view_offset_x: f32) -> Vec<(PaneId, Rect, bool)> {
        self.collect_tiles(true, view_offset_x)
    }

    /// Get ALL tiles without viewport culling (for overview).
    /// Multi-tile columns return one entry per tile.
    pub fn all_tiles_unculled(&self) -> Vec<(PaneId, Rect, bool)> {
        self.collect_tiles(false, 0.0)
    }

    fn collect_tiles(&self, cull: bool, view_offset_x: f32) -> Vec<(PaneId, Rect, bool)> {
        let mut result = Vec::new();
        let vp_left = view_offset_x;
        let vp_right = vp_left + self.view_size.width;
        let active_pane = self.active_pane_id();

        for (col_idx, col) in self.columns.iter().enumerate() {
            let col_x = self.column_x(col_idx);
            let col_w = col.effective_width(self.view_size.width);

            if cull && (col_x + col_w < vp_left || col_x > vp_right) {
                continue;
            }

            let screen_x = col_x - view_offset_x;
            let tile_rects = col.tile_rects(col_w, self.view_size.height);
            for (pane_id, y, h) in &tile_rects {
                let is_active = Some(*pane_id) == active_pane;
                result.push((*pane_id, Rect::new(screen_x, *y, col_w, *h), is_active));
            }
        }
        result
    }

    /// Add a new column to the right of the active column.
    /// When adding the second column, the first column is resized from full-width
    /// to the default width so both columns share the viewport equally.
    pub fn add_column_right(&mut self, pane_id: PaneId, default_width: ColumnWidth) {
        let vw = self.view_size.width;
        let vh = self.view_size.height;
        log::info!(
            "add_column_right: pane={pane_id} viewport={vw}x{vh} existing_cols={} default_width={default_width:?}",
            self.columns.len()
        );
        for (i, col) in self.columns.iter().enumerate() {
            log::info!(
                "  before: col[{i}] width={:?} effective={:.1}px",
                col.width,
                col.effective_width(vw)
            );
        }

        let insert_at = if self.columns.is_empty() {
            0
        } else {
            self.active_column_idx + 1
        };
        let width = if self.columns.is_empty() {
            // First column always gets full width
            ColumnWidth::Proportion(1.0)
        } else {
            // When adding the second column, shrink the first from full-width to default
            if self.columns.len() == 1
                && let ColumnWidth::Proportion(p) = self.columns[0].width
                && (p - 1.0).abs() < 1e-6
            {
                log::info!("  shrinking col[0] from 1.0 to {default_width:?}");
                self.columns[0].width = default_width;
                self.columns[0].preset_width_idx = None;
            }
            log::info!("  new col gets {default_width:?}");
            default_width
        };
        let mut col = Column::new(pane_id);
        col.width = width;
        self.columns.insert(insert_at, col);
        self.active_column_idx = insert_at;

        for (i, col) in self.columns.iter().enumerate() {
            log::info!(
                "  after: col[{i}] width={:?} effective={:.1}px",
                col.width,
                col.effective_width(vw)
            );
        }
    }

    /// Close a pane. If the pane is in a multi-tile column, only that tile is
    /// removed. If it's the last tile, the entire column is removed.
    pub fn close_pane(&mut self, pane_id: PaneId) -> Option<PaneId> {
        let idx = self.columns.iter().position(|c| c.contains_pane(pane_id))?;

        let col = &mut self.columns[idx];
        if col.tile_count() > 1 {
            // Multi-tile column: remove just this tile
            if let Some(tile_idx) = col.tiles.iter().position(|t| t.pane_id == pane_id) {
                col.tiles.remove(tile_idx);
                if tile_idx < col.active_tile_idx {
                    // Removed tile before active: shift index down
                    col.active_tile_idx -= 1;
                } else if col.active_tile_idx >= col.tiles.len() {
                    // Active was the last tile and it was removed
                    col.active_tile_idx = col.tiles.len() - 1;
                }
            }
        } else {
            // Single-tile column: remove entire column, viewport shrinks naturally
            self.columns.remove(idx);
            if self.columns.is_empty() {
                self.active_column_idx = 0;
            } else {
                self.clamp_active_column_idx();
                // When only one column remains, expand to full width
                if self.columns.len() == 1 {
                    self.columns[0].width = ColumnWidth::Proportion(1.0);
                    self.columns[0].preset_width_idx = None;
                }
            }
        }
        Some(pane_id)
    }

    pub fn close_active_pane(&mut self) -> Option<PaneId> {
        let pane_id = self.active_pane_id()?;
        self.close_pane(pane_id)
    }

    /// Consume: take the active pane from the column to the right and add it as a tile
    /// in the current column. If the right column becomes empty, remove it.
    /// Returns the consumed pane_id, or None if nothing to consume.
    pub fn consume_from_right(&mut self) -> Option<PaneId> {
        let right_idx = self.active_column_idx + 1;
        if right_idx >= self.columns.len() {
            return None;
        }

        let right_col = &mut self.columns[right_idx];
        let tile = right_col.tiles.remove(right_col.active_tile_idx);
        let pane_id = tile.pane_id;

        // Fix active_tile_idx of right column
        if right_col.tiles.is_empty() {
            // Remove the now-empty column
            self.columns.remove(right_idx);
            // When only one column remains, expand to full width
            if self.columns.len() == 1 {
                self.columns[0].width = ColumnWidth::Proportion(1.0);
                self.columns[0].preset_width_idx = None;
            }
        } else {
            right_col.active_tile_idx = right_col.active_tile_idx.min(right_col.tiles.len() - 1);
        }

        // Add tile to current column
        let col = &mut self.columns[self.active_column_idx];
        col.tiles.push(tile);
        col.active_tile_idx = col.tiles.len() - 1;

        Some(pane_id)
    }

    /// Expel: remove the active tile from the current column and create a new column
    /// to the right with it. Only works if the column has more than 1 tile.
    /// Returns the expelled pane_id, or None if column has only 1 tile.
    pub fn expel_active_tile(&mut self) -> Option<PaneId> {
        let col = &mut self.columns[self.active_column_idx];
        if col.tiles.len() <= 1 {
            return None;
        }

        let tile = col.tiles.remove(col.active_tile_idx);
        let pane_id = tile.pane_id;
        col.active_tile_idx = col.active_tile_idx.min(col.tiles.len() - 1);

        // Create new column to the right with the same width
        let width = col.width;
        let insert_at = self.active_column_idx + 1;
        let mut new_col = Column::new_with_tile(tile);
        new_col.width = width;
        self.columns.insert(insert_at, new_col);
        self.active_column_idx = insert_at;

        Some(pane_id)
    }

    /// Focus the next tile down within the current column.
    /// Returns true if focus moved within the column, false if at bottom.
    pub fn focus_tile_down(&mut self) -> bool {
        if let Some(col) = self.columns.get_mut(self.active_column_idx)
            && col.active_tile_idx + 1 < col.tiles.len()
        {
            col.active_tile_idx += 1;
            return true;
        }
        false
    }

    /// Focus the previous tile up within the current column.
    /// Returns true if focus moved within the column, false if at top.
    pub fn focus_tile_up(&mut self) -> bool {
        if let Some(col) = self.columns.get_mut(self.active_column_idx)
            && col.active_tile_idx > 0
        {
            col.active_tile_idx -= 1;
            return true;
        }
        false
    }

    pub fn focus_left(&mut self) -> bool {
        if self.active_column_idx > 0 {
            self.active_column_idx -= 1;
            return true;
        }
        false
    }

    pub fn focus_right(&mut self) -> bool {
        if self.active_column_idx + 1 < self.columns.len() {
            self.active_column_idx += 1;
            return true;
        }
        false
    }

    pub fn move_pane_left(&mut self) {
        if self.active_column_idx > 0 {
            self.columns
                .swap(self.active_column_idx, self.active_column_idx - 1);
            self.active_column_idx -= 1;
        }
    }

    pub fn move_pane_right(&mut self) {
        if self.active_column_idx + 1 < self.columns.len() {
            self.columns
                .swap(self.active_column_idx, self.active_column_idx + 1);
            self.active_column_idx += 1;
        }
    }

    /// Set the active column's width. Other columns are NOT affected (niri model:
    /// each column is independently sized, overflow handled by horizontal scrolling).
    pub fn set_active_column_width(&mut self, width: ColumnWidth) {
        if self.columns.is_empty() {
            return;
        }
        self.columns[self.active_column_idx].width =
            ColumnWidth::Proportion(self.clamp_column_width(width));
    }

    /// Resize the active column against its nearest neighbor.
    /// Only the pair is affected; other columns keep their widths.
    pub fn resize_active_with_neighbor(&mut self, delta_proportion: f64) {
        let Some(neighbor_idx) = self.active_neighbor_idx() else {
            return;
        };
        let idx = self.active_column_idx;
        let (new_active, new_neighbor) =
            self.resized_pair_proportions(idx, neighbor_idx, delta_proportion);

        self.columns[idx].width = ColumnWidth::Proportion(new_active);
        self.columns[idx].preset_width_idx = None; // manual resize clears preset
        self.columns[neighbor_idx].width = ColumnWidth::Proportion(new_neighbor);
        self.columns[neighbor_idx].preset_width_idx = None;
    }

    /// Resize a specific pair of adjacent columns by index.
    pub fn resize_column_pair(&mut self, left_idx: usize, right_idx: usize, delta: f64) {
        if left_idx >= self.columns.len() || right_idx >= self.columns.len() {
            return;
        }
        let (new_left, new_right) = self.resized_pair_proportions(left_idx, right_idx, delta);
        self.columns[left_idx].width = ColumnWidth::Proportion(new_left);
        self.columns[left_idx].preset_width_idx = None;
        self.columns[right_idx].width = ColumnWidth::Proportion(new_right);
        self.columns[right_idx].preset_width_idx = None;
    }

    /// Set a specific column's width by index (not just the active one).
    pub fn set_column_width_by_index(&mut self, idx: usize, width: ColumnWidth) {
        if let Some(col) = self.columns.get_mut(idx) {
            col.width = width;
            col.preset_width_idx = None;
        }
    }

    /// Equalize the active column and its right neighbor (or left if no right).
    /// Both columns get the average of their current proportions.
    pub fn equalize_active_with_neighbor(&mut self) {
        let Some(neighbor_idx) = self.active_neighbor_idx() else {
            return;
        };
        let idx = self.active_column_idx;
        let vw = self.view_size.width;
        let p1 = self.columns[idx].proportion(vw);
        let p2 = self.columns[neighbor_idx].proportion(vw);
        let avg = (p1 + p2) / 2.0;
        self.columns[idx].width = ColumnWidth::Proportion(avg);
        self.columns[neighbor_idx].width = ColumnWidth::Proportion(avg);
    }

    /// Cycle the active column's width through the given presets.
    /// Returns the new ColumnWidth (for syncing to server), or None if no columns.
    pub fn cycle_preset_width(
        &mut self,
        presets: &[ColumnWidth],
        reverse: bool,
    ) -> Option<ColumnWidth> {
        if presets.is_empty() || self.columns.is_empty() {
            return None;
        }
        let active_idx = self.active_column_idx;
        let current_width = self.columns[active_idx].width;
        let current_effective_width =
            self.columns[active_idx].effective_width(self.view_size.width);
        let current_idx = self.columns[active_idx].preset_width_idx;
        let vw_for_log = self.view_size.width;
        log::info!(
            "cycle_preset_width: active_col={} current_width={:?} effective={:.1}px preset_idx={:?} reverse={reverse} viewport={vw_for_log}x{}",
            active_idx,
            current_width,
            current_effective_width,
            current_idx,
            self.view_size.height
        );
        let new_idx = match current_idx {
            Some(idx) => {
                if reverse {
                    if idx == 0 { presets.len() - 1 } else { idx - 1 }
                } else {
                    (idx + 1) % presets.len()
                }
            }
            None => {
                // No preset selected: find the closest preset to current width, then advance
                let current_p = self.columns[active_idx].proportion(self.view_size.width);
                let closest = self.closest_preset_width_index(presets, current_p);
                if reverse {
                    if closest == 0 {
                        presets.len() - 1
                    } else {
                        closest - 1
                    }
                } else {
                    (closest + 1) % presets.len()
                }
            }
        };
        log::info!(
            "  cycle: current_idx={current_idx:?} → new_idx={new_idx} new_width={:?}",
            presets[new_idx]
        );
        self.columns[active_idx].preset_width_idx = Some(new_idx);
        let new_width = presets[new_idx];
        self.set_active_column_width(new_width);
        self.columns[active_idx].preset_width_idx = Some(new_idx);
        log::info!(
            "  after cycle: col width={:?} effective={:.1}px",
            self.columns[active_idx].width,
            self.columns[active_idx].effective_width(self.view_size.width)
        );
        Some(new_width)
    }

    /// Hit-test tile borders: returns (col_idx, top_tile_idx) if mouse is near
    /// a horizontal border between tiles within a visible column.
    pub fn hit_test_tile_border(
        &self,
        view_offset_x: f32,
        mx: f32,
        my: f32,
        threshold: f32,
    ) -> Option<(usize, usize)> {
        for (col_idx, col) in self.columns.iter().enumerate() {
            if col.tile_count() < 2 {
                continue;
            }
            let col_x = self.column_x(col_idx) - view_offset_x;
            let col_w = col.effective_width(self.view_size.width);
            if mx < col_x || mx > col_x + col_w {
                continue;
            }

            let border_ys = tile_border_positions(col, col_w, self.view_size.height);
            for (tile_idx, border_y) in border_ys.into_iter().enumerate() {
                if (my - border_y).abs() < threshold {
                    return Some((col_idx, tile_idx));
                }
            }
        }
        None
    }

    /// Resize two adjacent tiles within a column by pixel delta.
    /// Preserves the pair's combined weight so other tiles in the column are unaffected.
    pub fn resize_tile_pair(&mut self, col_idx: usize, top_tile_idx: usize, delta_y: f32) {
        let Some(col) = self.columns.get_mut(col_idx) else {
            return;
        };
        let bot_tile_idx = top_tile_idx + 1;
        if bot_tile_idx >= col.tiles.len() {
            return;
        }

        let col_w = col.effective_width(self.view_size.width);
        let rects = col.tile_rects(col_w, self.view_size.height);
        let top_h = rects[top_tile_idx].2;
        let bot_h = rects[bot_tile_idx].2;
        let total_h = top_h + bot_h;
        let new_top_h = clamped_tile_pair_height(top_h, total_h, delta_y);

        // Redistribute the pair's original combined weight proportionally,
        // so other tiles in the column keep their share unchanged.
        let original_top_w = col.tiles[top_tile_idx].height.weight() as f64;
        let original_bot_w = col.tiles[bot_tile_idx].height.weight() as f64;
        let total_w = original_top_w + original_bot_w;

        if total_h > 0.0 {
            let ratio = new_top_h as f64 / total_h as f64;
            col.tiles[top_tile_idx].height = TileHeight::Auto {
                weight: total_w * ratio,
            };
            col.tiles[bot_tile_idx].height = TileHeight::Auto {
                weight: total_w * (1.0 - ratio),
            };
        }
    }

    pub fn resize_view(&mut self, size: ViewSize) {
        self.view_size = size;
    }

    pub fn all_pane_ids(&self) -> Vec<PaneId> {
        self.columns.iter().flat_map(|c| c.all_pane_ids()).collect()
    }

    fn active_neighbor_idx(&self) -> Option<usize> {
        if self.columns.len() < 2 {
            None
        } else if self.active_column_idx + 1 < self.columns.len() {
            Some(self.active_column_idx + 1)
        } else if self.active_column_idx > 0 {
            Some(self.active_column_idx - 1)
        } else {
            None
        }
    }

    fn clamp_column_width(&self, width: ColumnWidth) -> f64 {
        match width {
            ColumnWidth::Proportion(p) => p.max(MIN_COLUMN_PROPORTION),
            ColumnWidth::Fixed(px) => {
                let vw = self.view_size.width;
                if vw > 0.0 {
                    (px / vw as f64).max(MIN_COLUMN_PROPORTION)
                } else {
                    0.5
                }
            }
        }
    }

    fn resized_pair_proportions(
        &self,
        first_idx: usize,
        second_idx: usize,
        delta: f64,
    ) -> (f64, f64) {
        let vw = self.view_size.width;
        let first_width = self.columns[first_idx].proportion(vw);
        let second_width = self.columns[second_idx].proportion(vw);
        let total = first_width + second_width;
        let min_width = MIN_COLUMN_PROPORTION.min(total / 2.0);
        let new_first = (first_width + delta).clamp(min_width, total - min_width);
        (new_first, total - new_first)
    }

    fn closest_preset_width_index(
        &self,
        presets: &[ColumnWidth],
        current_proportion: f64,
    ) -> usize {
        let viewport_width = self.view_size.width;
        presets
            .iter()
            .enumerate()
            .min_by(|(_, left), (_, right)| {
                let left_distance =
                    (column_width_to_proportion(**left, viewport_width) - current_proportion).abs();
                let right_distance = (column_width_to_proportion(**right, viewport_width)
                    - current_proportion)
                    .abs();
                left_distance.partial_cmp(&right_distance).unwrap()
            })
            .map(|(idx, _)| idx)
            .unwrap_or(0)
    }
}

fn column_width_to_proportion(width: ColumnWidth, viewport_width: f32) -> f64 {
    match width {
        ColumnWidth::Proportion(p) => p,
        ColumnWidth::Fixed(px) => px / viewport_width as f64,
    }
}

fn tile_border_positions(col: &Column, col_width: f32, column_height: f32) -> Vec<f32> {
    let rects = col.tile_rects(col_width, column_height);
    rects.windows(2).map(|pair| pair[0].1 + pair[0].2).collect()
}

fn clamped_tile_pair_height(top_height: f32, total_height: f32, delta_y: f32) -> f32 {
    let min_height = 30.0_f32;
    (top_height + delta_y).clamp(min_height, total_height - min_height)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DW: ColumnWidth = ColumnWidth::Proportion(0.5);

    fn ws() -> Workspace {
        Workspace::new(ViewSize {
            width: 1000.0,
            height: 600.0,
        })
    }

    trait TestHelper {
        fn add_column_right_default(&mut self, pane_id: PaneId);
    }
    impl TestHelper for Workspace {
        fn add_column_right_default(&mut self, pane_id: PaneId) {
            self.add_column_right(pane_id, DW);
        }
    }

    #[test]
    fn add_columns() {
        let mut w = ws();
        w.add_column_right_default(1);
        w.add_column_right_default(2);
        w.add_column_right_default(3);
        assert_eq!(w.columns.len(), 3);
        assert_eq!(w.active_column_idx, 2);
        assert_eq!(w.active_pane_id(), Some(3));
        // col[0] shrinks from 1.0 to DW(0.5), all subsequent columns get DW(0.5)
        assert!((w.columns[0].proportion(w.view_size.width) - 0.5).abs() < 1e-6);
        assert!((w.columns[1].proportion(w.view_size.width) - 0.5).abs() < 1e-6);
        assert!((w.columns[2].proportion(w.view_size.width) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn focus_navigation() {
        let mut w = ws();
        w.add_column_right_default(1);
        w.add_column_right_default(2);
        w.add_column_right_default(3);
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
        w.add_column_right_default(1);
        w.add_column_right_default(2);
        w.add_column_right_default(3);
        w.close_pane(1);
        assert_eq!(w.active_column_idx, 1);
        assert_eq!(w.active_pane_id(), Some(3));
    }

    #[test]
    fn close_last_pane() {
        let mut w = ws();
        w.add_column_right_default(1);
        w.close_pane(1);
        assert!(w.is_empty());
    }

    #[test]
    fn visible_tiles() {
        let mut w = ws();
        w.add_column_right_default(1);
        w.add_column_right_default(2);
        // Both columns are half-width (500), fitting within viewport
        let tiles = w.all_tiles_unculled();
        assert_eq!(tiles.len(), 2);
    }

    #[test]
    fn move_pane() {
        let mut w = ws();
        w.add_column_right_default(1);
        w.add_column_right_default(2);
        w.add_column_right_default(3);
        w.move_pane_left();
        assert_eq!(w.active_column_idx, 1);
        assert_eq!(w.all_pane_ids(), vec![1, 3, 2]);
    }

    #[test]
    fn column_x_calculation() {
        let mut w = ws();
        w.add_column_right_default(1);
        w.add_column_right_default(2);
        // First column shrinks to 0.5 (500px), second at 500+8=508
        assert_eq!(w.column_x(0), 0.0);
        assert_eq!(w.column_x(1), 508.0);
    }

    #[test]
    fn set_active_column_width_only_changes_active() {
        let mut w = ws();
        w.add_column_right_default(1);
        w.add_column_right_default(2);
        // Both columns start at 0.5 (first shrinks from 1.0 when second is added)
        let original_width_1 = w.columns[1].proportion(w.view_size.width);
        assert!((original_width_1 - 0.5).abs() < 1e-6);
        w.active_column_idx = 0;
        w.set_active_column_width(ColumnWidth::Proportion(0.7));

        let widths: Vec<f64> = w
            .columns
            .iter()
            .map(|c| c.proportion(w.view_size.width))
            .collect();
        assert!((widths[0] - 0.7).abs() < 1e-6);
        // Other column unchanged
        assert!((widths[1] - original_width_1).abs() < 1e-6);
    }

    #[test]
    fn resize_active_with_neighbor_preserves_pair_width() {
        let mut w = ws();
        w.add_column_right_default(1);
        w.add_column_right_default(2);
        w.add_column_right_default(3);
        w.active_column_idx = 1;

        let before: Vec<f64> = w
            .columns
            .iter()
            .map(|c| c.proportion(w.view_size.width))
            .collect();
        let pair_before = before[1] + before[2];
        w.resize_active_with_neighbor(0.1);
        let after: Vec<f64> = w
            .columns
            .iter()
            .map(|c| c.proportion(w.view_size.width))
            .collect();
        let pair_after = after[1] + after[2];

        // Column 0 unchanged
        assert!((after[0] - before[0]).abs() < 1e-6);
        // Active column grew, neighbor shrank
        assert!(after[1] > before[1]);
        assert!(after[2] < before[2]);
        // Pair total preserved
        assert!((pair_after - pair_before).abs() < 1e-6);
    }

    #[test]
    fn equalize_active_with_neighbor_averages_pair() {
        let mut w = ws();
        w.add_column_right_default(1);
        w.add_column_right_default(2);
        // Both columns start at 0.5; manually set col[0] to 0.8
        w.active_column_idx = 0;
        w.set_active_column_width(ColumnWidth::Proportion(0.8));
        // col[0]=0.8, col[1]=0.5
        w.equalize_active_with_neighbor();

        let widths: Vec<f64> = w
            .columns
            .iter()
            .map(|c| c.proportion(w.view_size.width))
            .collect();
        let expected = (0.8 + 0.5) / 2.0;
        assert!((widths[0] - expected).abs() < 1e-6);
        assert!((widths[1] - expected).abs() < 1e-6);
    }

    #[test]
    fn cycle_preset_width_advances_from_closest_preset() {
        let mut w = ws();
        w.add_column_right_default(1);
        w.add_column_right_default(2);
        w.active_column_idx = 0;
        w.set_active_column_width(ColumnWidth::Proportion(0.68));

        let presets = [
            ColumnWidth::Proportion(1.0 / 3.0),
            ColumnWidth::Proportion(0.5),
            ColumnWidth::Proportion(2.0 / 3.0),
            ColumnWidth::Proportion(1.0),
        ];

        let new_width = w.cycle_preset_width(&presets, false);

        assert_eq!(new_width, Some(ColumnWidth::Proportion(1.0)));
        assert_eq!(w.columns[0].preset_width_idx, Some(3));
        assert!((w.columns[0].proportion(w.view_size.width) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn set_column_width_by_index_clears_stale_preset_tracking() {
        let mut w = ws();
        w.add_column_right_default(1);
        w.add_column_right_default(2);

        w.columns[0].preset_width_idx = Some(1);
        w.set_column_width_by_index(0, ColumnWidth::Proportion(0.68));

        assert_eq!(w.columns[0].preset_width_idx, None);
        assert!((w.columns[0].proportion(w.view_size.width) - 0.68).abs() < 1e-6);
    }

    #[test]
    fn hit_test_tile_border_uses_screen_coordinates() {
        let mut w = ws();
        w.add_column_right_default(1);
        w.columns[0].tiles.push(crate::tile::Tile::new(2));

        let border_y = w.columns[0].tile_rects(500.0, w.view_size.height)[0].2;

        assert_eq!(
            w.hit_test_tile_border(0.0, 250.0, border_y + 1.0, 4.0),
            Some((0, 0))
        );
        assert_eq!(
            w.hit_test_tile_border(0.0, 250.0, border_y + 8.0, 4.0),
            None
        );
    }

    #[test]
    fn resize_tile_pair_preserves_pair_weight_total() {
        let mut w = ws();
        w.add_column_right_default(1);
        w.columns[0].tiles.push(crate::tile::Tile::new(2));
        w.columns[0].tiles.push(crate::tile::Tile::new(3));

        let before_total = w.columns[0].tiles[0].height.weight() as f64
            + w.columns[0].tiles[1].height.weight() as f64;

        w.resize_tile_pair(0, 0, 60.0);

        let top_weight = w.columns[0].tiles[0].height.weight() as f64;
        let bottom_weight = w.columns[0].tiles[1].height.weight() as f64;

        assert!(top_weight > bottom_weight);
        assert!(((top_weight + bottom_weight) - before_total).abs() < 1e-6);
        assert!((w.columns[0].tiles[2].height.weight() as f64 - 1.0).abs() < 1e-6);
    }
}
