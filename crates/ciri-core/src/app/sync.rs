use ciri_layout::column::{Column, ColumnWidth};
use ciri_layout::tile::Tile;
use ciri_layout::workspace::Workspace;
use ciri_protocol::message::LayoutState;

use super::CoreApp;

impl CoreApp {
    /// Rebuild the full 2D WorkspaceSet from the server's authoritative layout.
    pub fn apply_layout(&mut self, layout: &LayoutState) {
        log::debug!(
            "apply_layout: view_size={:?}, vox={:.1}, voy={:.1}",
            self.workspaces.view_size,
            self.anim_mgr.view_offset_x.value(),
            self.anim_mgr.view_offset_y.value()
        );

        // Snapshot old pane positions for move animation
        let old_positions = self.snapshot_pane_positions();

        let view_size = self.workspaces.view_size;
        let column_gap = self.workspaces.column_gap;

        let mut new_workspaces: Vec<Workspace> = layout
            .workspaces
            .iter()
            .map(|ws_state| {
                let mut ws = Workspace::new_with_gap(view_size, column_gap);
                for col_state in &ws_state.columns {
                    if let Some(first_tile) = col_state.tiles.first() {
                        let mut col = Column::new(first_tile.pane_id);
                        // Replace the default single tile with all tiles from state
                        col.tiles = col_state
                            .tiles
                            .iter()
                            .map(|t| {
                                let mut tile = Tile::new(t.pane_id);
                                tile.height = ciri_layout::tile::TileHeight::Auto {
                                    weight: t.weight as f64,
                                };
                                tile
                            })
                            .collect();
                        col.active_tile_idx = col_state
                            .active_tile_idx
                            .min(col.tiles.len().saturating_sub(1));
                        col.width = if let Some(px) = col_state.width_fixed_px {
                            ColumnWidth::Fixed(px)
                        } else {
                            ColumnWidth::Proportion(col_state.width_proportion)
                        };
                        ws.columns.push(col);
                    }
                }
                ws.active_column_idx = ws_state
                    .active_column_idx
                    .min(ws.columns.len().saturating_sub(1));
                ws
            })
            .collect();

        if new_workspaces.is_empty() {
            new_workspaces.push(Workspace::new_with_gap(view_size, column_gap));
        }

        self.workspaces.workspaces = new_workspaces;
        self.workspaces.active_workspace_idx = layout
            .active_workspace_idx
            .min(self.workspaces.workspaces.len().saturating_sub(1));
        self.sync_workspace_pane_memory();

        self.snap_all_col_widths();

        // Compute new positions and start move animations for shifted panes
        let new_positions = self.snapshot_pane_positions();
        if self.config.animation.enabled {
            let config = self.anim_config();
            for (pane_id, (old_x, old_y)) in &old_positions {
                if let Some(&(new_x, new_y)) = new_positions.get(pane_id) {
                    let dx = old_x - new_x;
                    let dy = old_y - new_y;
                    if dx.abs() > 1.0 || dy.abs() > 1.0 {
                        self.anim_mgr.ensure_pane_registered(*pane_id);
                        self.anim_mgr.start_move_animation(*pane_id, dx, dy, &config);
                    }
                }
            }
        }

        self.animate_to_active();
    }
}
