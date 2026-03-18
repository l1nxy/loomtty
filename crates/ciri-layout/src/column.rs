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
                if viewport_w > 0.0 { px / viewport_w as f64 } else { 0.5 }
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
        self.rendered_width.unwrap_or_else(|| self.resolve_width(viewport_w))
    }

    /// Set the rendered width immediately (no animation).
    pub fn snap_width(&mut self, viewport_w: f32) {
        self.rendered_width = Some(self.resolve_width(viewport_w));
    }

    /// Set the rendered width to an explicit pixel value.
    pub fn set_rendered_width(&mut self, w: f32) {
        self.rendered_width = Some(w);
    }

    /// Compute (pane_id, y_offset, height) for each tile based on weights.
    pub fn tile_rects(&self, _col_width: f32, col_height: f32) -> Vec<(PaneId, f32, f32)> {
        if self.tiles.len() == 1 {
            return vec![(self.tiles[0].pane_id, 0.0, col_height)];
        }

        let total_weight: f64 = self.tiles.iter().map(|t| match t.height {
            TileHeight::Auto { weight } => weight,
            TileHeight::Fixed(_) => 0.0,
        }).sum();

        let fixed_total: f32 = self.tiles.iter().map(|t| match t.height {
            TileHeight::Fixed(px) => px as f32,
            _ => 0.0,
        }).sum();

        let auto_height = (col_height - fixed_total).max(0.0);

        let mut result = Vec::with_capacity(self.tiles.len());
        let mut y = 0.0f32;
        for tile in &self.tiles {
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
}
