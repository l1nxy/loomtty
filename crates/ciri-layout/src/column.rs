use crate::tile::{PaneId, Tile, TileHeight};

/// Width specification for a column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColumnWidth {
    Proportion(f64),
    Fixed(f64),
}

impl Default for ColumnWidth {
    fn default() -> Self {
        ColumnWidth::Proportion(0.5)
    }
}

/// A column is a vertical stack of tiles in a horizontal row.
#[derive(Debug, Clone)]
pub struct Column {
    pub tiles: Vec<Tile>,
    pub active_tile_idx: usize,
    pub width: ColumnWidth,
    /// Currently selected preset index, or None if manually resized.
    pub preset_width_idx: Option<usize>,
    /// Current rendered width in pixels. Set once at creation/resize,
    /// then only changed by explicit animation or jump.
    rendered_width: Option<f32>,
}

impl Column {
    pub fn new(pane_id: PaneId) -> Self {
        Column {
            tiles: vec![Tile::new(pane_id)],
            active_tile_idx: 0,
            width: ColumnWidth::default(),
            preset_width_idx: None,
            rendered_width: None,
        }
    }

    pub fn new_with_tile(tile: Tile) -> Self {
        Column {
            tiles: vec![tile],
            active_tile_idx: 0,
            width: ColumnWidth::default(),
            preset_width_idx: None,
            rendered_width: None,
        }
    }

    /// Returns the pane_id of the active tile.
    pub fn active_pane_id(&self) -> PaneId {
        self.tiles[self.active_tile_idx].pane_id
    }

    /// Returns all pane_ids across all tiles.
    pub fn all_pane_ids(&self) -> Vec<PaneId> {
        self.tiles.iter().map(|t| t.pane_id).collect()
    }

    /// Returns true if any tile in this column contains the given pane_id.
    pub fn contains_pane(&self, pane_id: PaneId) -> bool {
        self.tiles.iter().any(|t| t.pane_id == pane_id)
    }

    /// Returns the number of tiles in this column.
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// Get the column's width as a proportion of viewport.
    pub fn proportion(&self, viewport_w: f32) -> f64 {
        match self.width {
            ColumnWidth::Proportion(p) => p,
            ColumnWidth::Fixed(px) => {
                if viewport_w > 0.0 {
                    px / viewport_w as f64
                } else {
                    0.5
                }
            }
        }
    }

    pub fn resolve_width(&self, viewport_w: f32) -> f32 {
        match self.width {
            ColumnWidth::Proportion(p) => (viewport_w as f64 * p) as f32,
            ColumnWidth::Fixed(px) => px as f32,
        }
    }

    pub fn effective_width(&self, viewport_w: f32) -> f32 {
        self.rendered_width
            .unwrap_or_else(|| self.resolve_width(viewport_w))
    }

    /// Set the rendered width immediately (no animation).
    pub fn snap_width(&mut self, viewport_w: f32) {
        self.rendered_width = Some(self.resolve_width(viewport_w));
    }

    /// Set the rendered width to an explicit pixel value.
    pub fn set_rendered_width(&mut self, w: f32) {
        self.rendered_width = Some(w);
    }

    /// Set absolute weights for two adjacent tiles. Used by the server when
    /// receiving the final drag result from the client.
    pub fn set_tile_weights(&mut self, top_idx: usize, top_weight: f64, bottom_weight: f64) {
        let bot_idx = top_idx + 1;
        if bot_idx >= self.tiles.len() {
            return;
        }
        let min_w = 0.05;
        self.tiles[top_idx].height = TileHeight::Auto {
            weight: top_weight.max(min_w),
        };
        self.tiles[bot_idx].height = TileHeight::Auto {
            weight: bottom_weight.max(min_w),
        };
    }

    /// Compute (pane_id, y_offset, height) for each tile based on weights.
    /// `tile_gap` is the vertical gap inserted between adjacent tiles.
    pub fn tile_rects(
        &self,
        _col_width: f32,
        col_height: f32,
        tile_gap: f32,
    ) -> Vec<(PaneId, f32, f32)> {
        if self.tiles.len() == 1 {
            return vec![(self.tiles[0].pane_id, 0.0, col_height)];
        }

        let total_weight: f64 = self
            .tiles
            .iter()
            .map(|t| match t.height {
                TileHeight::Auto { weight } => weight,
                TileHeight::Fixed(_) => 0.0,
            })
            .sum();

        let fixed_total: f32 = self
            .tiles
            .iter()
            .map(|t| match t.height {
                TileHeight::Fixed(px) => px as f32,
                _ => 0.0,
            })
            .sum();

        let gaps_total = (self.tiles.len() - 1) as f32 * tile_gap;
        let auto_height = (col_height - fixed_total - gaps_total).max(0.0);

        let mut result = Vec::with_capacity(self.tiles.len());
        let mut y = 0.0f32;
        let last = self.tiles.len() - 1;
        for (i, tile) in self.tiles.iter().enumerate() {
            let h = match tile.height {
                TileHeight::Auto { weight } => {
                    if total_weight > 0.0 {
                        (auto_height as f64 * weight / total_weight) as f32
                    } else {
                        auto_height / self.tiles.len() as f32
                    }
                }
                TileHeight::Fixed(px) => px as f32,
            };
            result.push((tile.pane_id, y, h));
            y += h;
            if i != last {
                y += tile_gap;
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_width() {
        let col = Column::new(1);
        assert_eq!(col.resolve_width(1000.0), 500.0);
    }

    #[test]
    fn effective_width_uses_rendered() {
        let mut col = Column::new(1);
        col.set_rendered_width(300.0);
        assert_eq!(col.effective_width(1000.0), 300.0);
    }

    #[test]
    fn snap_width_sets_resolve() {
        let mut col = Column::new(1);
        col.snap_width(1000.0);
        assert_eq!(col.effective_width(1000.0), 500.0);
    }

    #[test]
    fn fixed_width_independent_of_viewport() {
        let mut col = Column::new(1);
        col.width = ColumnWidth::Fixed(300.0);
        assert_eq!(col.resolve_width(1000.0), 300.0);
        assert_eq!(col.resolve_width(500.0), 300.0);
        assert_eq!(col.resolve_width(2000.0), 300.0);
    }

    #[test]
    fn proportion_from_fixed_width() {
        let mut col = Column::new(1);
        col.width = ColumnWidth::Fixed(250.0);
        assert!((col.proportion(1000.0) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn proportion_with_zero_viewport_returns_default() {
        let mut col = Column::new(1);
        col.width = ColumnWidth::Fixed(250.0);
        assert_eq!(col.proportion(0.0), 0.5); // fallback
    }

    #[test]
    fn tile_rects_single_tile_fills_full_height() {
        let col = Column::new(1);
        let rects = col.tile_rects(500.0, 600.0, 0.0);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0], (1, 0.0, 600.0));
    }

    #[test]
    fn tile_rects_equal_weight_splits_evenly() {
        let mut col = Column::new(1);
        col.tiles.push(Tile::new(2));
        let rects = col.tile_rects(500.0, 600.0, 0.0);
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0], (1, 0.0, 300.0));
        assert_eq!(rects[1], (2, 300.0, 300.0));
    }

    #[test]
    fn tile_rects_unequal_weights() {
        let mut col = Column::new(1);
        col.tiles[0].height = TileHeight::Auto { weight: 3.0 };
        col.tiles.push(Tile {
            pane_id: 2,
            height: TileHeight::Auto { weight: 1.0 },
        });
        let rects = col.tile_rects(500.0, 400.0, 0.0);
        assert_eq!(rects.len(), 2);
        // 3/4 of 400 = 300, 1/4 of 400 = 100
        assert!((rects[0].2 - 300.0).abs() < 1e-3);
        assert!((rects[1].2 - 100.0).abs() < 1e-3);
    }

    #[test]
    fn tile_rects_mixed_auto_and_fixed() {
        let mut col = Column::new(1);
        col.tiles[0].height = TileHeight::Auto { weight: 1.0 };
        col.tiles.push(Tile {
            pane_id: 2,
            height: TileHeight::Fixed(100.0),
        });
        let rects = col.tile_rects(500.0, 400.0, 0.0);
        // Fixed tile takes 100px, auto tile gets remaining 300px
        assert!((rects[0].2 - 300.0).abs() < 1e-3);
        assert!((rects[1].2 - 100.0).abs() < 1e-3);
    }

    #[test]
    fn tile_rects_all_fixed_exceeding_column_height() {
        let mut col = Column::new(1);
        col.tiles[0].height = TileHeight::Fixed(300.0);
        col.tiles.push(Tile {
            pane_id: 2,
            height: TileHeight::Fixed(400.0),
        });
        let rects = col.tile_rects(500.0, 500.0, 0.0);
        // Both fixed, no auto tiles. total fixed = 700 > col_height 500.
        // auto_height = max(0, 500-700) = 0. Fixed tiles keep their px.
        assert!((rects[0].2 - 300.0).abs() < 1e-3);
        assert!((rects[1].2 - 400.0).abs() < 1e-3);
    }

    #[test]
    fn set_tile_weights_clamps_minimum() {
        let mut col = Column::new(1);
        col.tiles.push(Tile::new(2));
        col.set_tile_weights(0, 0.01, 0.01);
        // Both should be clamped to min 0.05
        match col.tiles[0].height {
            TileHeight::Auto { weight } => assert!((weight - 0.05).abs() < 1e-6),
            _ => panic!("expected Auto"),
        }
        match col.tiles[1].height {
            TileHeight::Auto { weight } => assert!((weight - 0.05).abs() < 1e-6),
            _ => panic!("expected Auto"),
        }
    }

    #[test]
    fn set_tile_weights_out_of_range_noop() {
        let mut col = Column::new(1);
        let original = col.tiles[0].height;
        // Only one tile, so bot_idx = 1 >= tiles.len() → noop
        col.set_tile_weights(0, 0.5, 0.5);
        assert_eq!(col.tiles[0].height, original);
    }

    #[test]
    fn contains_pane_and_all_pane_ids() {
        let mut col = Column::new(1);
        col.tiles.push(Tile::new(2));
        col.tiles.push(Tile::new(3));
        assert!(col.contains_pane(1));
        assert!(col.contains_pane(2));
        assert!(col.contains_pane(3));
        assert!(!col.contains_pane(4));
        assert_eq!(col.all_pane_ids(), vec![1, 2, 3]);
    }
}
