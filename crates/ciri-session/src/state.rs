use serde::{Deserialize, Serialize};

/// Serializable session state for persistence (2D layout).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub name: String,
    pub workspaces: Vec<SavedWorkspace>,
    pub active_workspace_idx: usize,
}

/// One workspace in the saved layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedWorkspace {
    pub columns: Vec<SavedColumn>,
    pub active_column_idx: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedColumn {
    pub tiles: Vec<SavedTile>,
    pub active_tile_idx: usize,
    pub width_proportion: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedTile {
    pub pane_id: u64,
    pub weight: f32,
    pub cwd: Option<String>,
    pub title: Option<String>,
}
