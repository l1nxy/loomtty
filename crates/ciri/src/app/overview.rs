use ciri_layout::geometry::Rect as GeoRect;

use super::App;

impl App {
    pub fn hit_test_overview(&self, mx: f32, my: f32) -> Option<(usize, u64)> {
        // Gate on content area so clicks inside the status bar / side tab
        // bar don't fall through to overview.
        let (vw_for_x, _) = self.command_palette_viewport_size();
        self.content_y_from_screen(my)?;
        self.content_x_from_screen(mx, vw_for_x)?;
        let zoom = self.core.anim_mgr.overview_zoom.value() as f32;
        let vox = self.core.anim_mgr.view_offset_x.value() as f32;
        let voy = self.core.anim_mgr.view_offset_y.value() as f32;
        let tiles = if self.core.overview.active || zoom < self.core.config.animation.zoom_threshold
        {
            self.overview_visible_tiles(zoom, vox, voy)
        } else {
            self.core.workspaces.visible_tiles_2d(vox, voy)
        };
        let (vw, vh) = self.command_palette_viewport_size();
        let content_x = self.content_origin_x();
        let content_y = self.content_origin_y();

        for (pane_id, tile_rect, _) in &tiles {
            // Mirror the painter: offset by content origin, then transform
            // around the window center. Hit-testing in content space
            // against a content-space transform drifts from the painted
            // tile by `content_origin * (1 - zoom)`.
            let offset_rect = GeoRect::new(
                tile_rect.x + content_x,
                tile_rect.y + content_y,
                tile_rect.w,
                tile_rect.h,
            );
            let tr = self.transformed_tile_rect(offset_rect, zoom, vw, vh);
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

    /// Delegate: exit overview mode. Also clears the App-owned UI hover
    /// fields that used to live on `AppModel.overview` — the model-level
    /// reset went away when those fields moved to `App`, so the wrapper
    /// has to do it now or stale hover state would survive an Escape.
    pub(crate) fn exit_overview(&mut self) {
        self.core.exit_overview();
        self.overview_hovered_pane = None;
        self.overview_action_hover = None;
    }

    /// Delegate: toggle overview mode. Mirrors `exit_overview` for the
    /// hover-field reset on the off-and-on paths.
    pub(crate) fn toggle_overview(&mut self) {
        self.core.toggle_overview();
        self.overview_hovered_pane = None;
        self.overview_action_hover = None;
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
        self.overview_hovered_pane = None;
        self.exit_overview();
    }
}
