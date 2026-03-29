use ciri_layout::geometry::Rect as GeoRect;

use super::App;

impl App {
    pub fn hit_test_overview(&self, mx: f32, my: f32) -> Option<(usize, u64)> {
        let my = self.content_y_from_screen(my)?;
        let zoom = self.core.anim_mgr.overview_zoom.value() as f32;
        let zoom_threshold = self.core.config.animation.zoom_threshold;
        let vox = self.core.anim_mgr.view_offset_x.value() as f32;
        let voy = self.core.anim_mgr.view_offset_y.value() as f32;
        let tiles = if self.core.overview.active || zoom < zoom_threshold {
            self.core.workspaces.all_tiles_2d(vox, voy)
        } else {
            self.core.workspaces.visible_tiles_2d(vox, voy)
        };
        let (vw, vh) = self.command_palette_viewport_size();
        let cx = vw / 2.0;
        let cy = vh / 2.0;

        for (pane_id, tile_rect, _) in &tiles {
            let tr = if zoom < zoom_threshold {
                GeoRect::new(
                    cx + (tile_rect.x - cx) * zoom,
                    cy + (tile_rect.y - cy) * zoom,
                    tile_rect.w * zoom,
                    tile_rect.h * zoom,
                )
            } else {
                *tile_rect
            };
            if tr.contains(mx, my) {
                for (ws_idx, ws) in self.core.workspaces.workspaces.iter().enumerate() {
                    if ws.columns.iter().any(|c| c.contains_pane(*pane_id)) {
                        return Some((ws_idx, *pane_id));
                    }
                }
            }
        }
        None
    }

    /// Delegate: exit overview mode.
    pub(crate) fn exit_overview(&mut self) {
        self.core.exit_overview();
    }

    /// Delegate: toggle overview mode.
    pub(crate) fn toggle_overview(&mut self) {
        self.core.toggle_overview();
    }

    pub(crate) fn focus_overview_target(&mut self, ws_idx: usize, pane_id: u64) {
        if ws_idx < self.core.workspaces.workspaces.len() {
            self.core.workspaces.active_workspace_idx = ws_idx;
            let ws = self.core.workspaces.active_mut();
            for (col_idx, col) in ws.columns.iter().enumerate() {
                if col.contains_pane(pane_id) {
                    ws.active_column_idx = col_idx;
                    if let Some(tile_idx) = col.tiles.iter().position(|t| t.pane_id == pane_id) {
                        ws.columns[col_idx].active_tile_idx = tile_idx;
                    }
                    break;
                }
            }
        }
        self.remember_workspace_pane(ws_idx, pane_id);
        self.send(ciri_protocol::message::ClientMessage::FocusPane { pane_id });
        self.core.overview.hovered_pane = None;
        self.exit_overview();
    }
}
