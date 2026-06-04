pub type PaneId = u64;

/// Height specification for a tile within a column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TileHeight {
    /// Proportional height based on weight relative to siblings.
    Auto { weight: f64 },
    /// Fixed pixel height.
    Fixed(f64),
}

impl TileHeight {
    /// Get the weight value (for serialization).
    /// Auto tiles return their weight; Fixed tiles return pixel value.
    pub fn weight(&self) -> f32 {
        match self {
            TileHeight::Auto { weight } => *weight as f32,
            TileHeight::Fixed(px) => *px as f32,
        }
    }
}

impl Default for TileHeight {
    fn default() -> Self {
        TileHeight::Auto { weight: 1.0 }
    }
}

/// A tile is one pane within a column, stacked vertically.
#[derive(Debug, Clone)]
pub struct Tile {
    pub pane_id: PaneId,
    pub height: TileHeight,
}

impl Tile {
    pub fn new(pane_id: PaneId) -> Self {
        Tile {
            pane_id,
            height: TileHeight::default(),
        }
    }
}
