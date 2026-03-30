use crate::agent::SavedAgent;
use serde::{Deserialize, Serialize};

/// Serializable session state for persistence (2D layout).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub name: String,
    /// Workspaces in this session. Previously called `rows`.
    #[serde(alias = "rows")]
    pub workspaces: Vec<SavedWorkspace>,
    /// Active workspace index. Previously called `active_row`.
    #[serde(alias = "active_row")]
    pub active_workspace_idx: usize,
}

/// One workspace in the saved layout. Previously called `SavedRow`.
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
    /// If set, column uses a fixed pixel width instead of proportion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_fixed_px: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedTile {
    pub pane_id: u64,
    pub weight: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Detected AI agent running in this pane (for auto-resume).
    #[serde(default, skip_serializing_if = "skip_unknown_agent")]
    pub agent: Option<SavedAgent>,
}

/// Skip serializing None agents and Unknown agents (forward compat:
/// re-saving would overwrite the original kind string with "unknown").
fn skip_unknown_agent(agent: &Option<SavedAgent>) -> bool {
    match agent {
        Some(a) => !a.should_serialize(),
        None => true,
    }
}
