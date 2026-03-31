use crate::agent::SavedAgent;
use anyhow::Result;
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

impl SessionState {
    pub fn validate_structure(&self) -> Result<()> {
        if self.workspaces.is_empty() {
            anyhow::bail!("session '{}' has no workspaces", self.name);
        }

        if self.active_workspace_idx >= self.workspaces.len() {
            anyhow::bail!(
                "session '{}' active workspace {} out of bounds for {} workspaces",
                self.name,
                self.active_workspace_idx,
                self.workspaces.len()
            );
        }

        for (workspace_idx, workspace) in self.workspaces.iter().enumerate() {
            workspace.validate_structure(&self.name, workspace_idx)?;
        }

        Ok(())
    }
}

/// One workspace in the saved layout. Previously called `SavedRow`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedWorkspace {
    pub columns: Vec<SavedColumn>,
    pub active_column_idx: usize,
}

impl SavedWorkspace {
    fn validate_structure(&self, session_name: &str, workspace_idx: usize) -> Result<()> {
        if self.columns.is_empty() {
            anyhow::bail!(
                "session '{}' workspace {} has no columns",
                session_name,
                workspace_idx
            );
        }

        if self.active_column_idx >= self.columns.len() {
            anyhow::bail!(
                "session '{}' workspace {} active column {} out of bounds for {} columns",
                session_name,
                workspace_idx,
                self.active_column_idx,
                self.columns.len()
            );
        }

        for (column_idx, column) in self.columns.iter().enumerate() {
            column.validate_structure(session_name, workspace_idx, column_idx)?;
        }

        Ok(())
    }
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

impl SavedColumn {
    fn validate_structure(
        &self,
        session_name: &str,
        workspace_idx: usize,
        column_idx: usize,
    ) -> Result<()> {
        if self.tiles.is_empty() {
            anyhow::bail!(
                "session '{}' workspace {} column {} has no tiles",
                session_name,
                workspace_idx,
                column_idx
            );
        }

        if self.active_tile_idx >= self.tiles.len() {
            anyhow::bail!(
                "session '{}' workspace {} column {} active tile {} out of bounds for {} tiles",
                session_name,
                workspace_idx,
                column_idx,
                self.active_tile_idx,
                self.tiles.len()
            );
        }

        Ok(())
    }
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
