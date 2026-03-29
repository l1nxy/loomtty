use super::App;

pub(crate) struct ScrollbarHit {
    pub pane_id: u64,
    pub on_thumb: bool,
    pub inner_y: f32,
    pub inner_h: f32,
    pub total_lines: usize,
    pub visible_rows: u16,
    pub scrollbar_rect: Option<ciri_render::rect::Rect>,
}

impl App {
    pub(crate) fn apply_scrollbar_drag(&mut self, my: f32) -> bool {
        let Some(info) = self.core.drag.scrollbar_dragging.as_ref() else {
            return false;
        };
        let pane_id = info.pane_id;
        let inner_y = info.pane_inner_y;
        let inner_h = info.pane_inner_h;
        let total_lines = info.total_lines;
        let visible_rows = info.visible_rows as usize;
        if total_lines <= visible_rows {
            return true;
        }

        let max_offset = total_lines - visible_rows;
        let ratio = ((my - inner_y) / inner_h).clamp(0.0, 1.0);
        let new_offset = ((1.0 - ratio) * max_offset as f32).round() as usize;
        let new_offset = new_offset.min(max_offset);
        if let Some(grid) = self.core.pane_grids.get_mut(&pane_id)
            && grid.scroll_offset != new_offset
        {
            grid.scroll_offset = new_offset;
            grid.dirty = true;
            self.invalidate_pane_cache(pane_id);
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
        true
    }

    pub(crate) fn apply_tile_resize_drag(&mut self, my: f32) -> bool {
        let Some((col_idx, top_tile_idx)) = self.core.drag.tile_dragging else {
            return false;
        };
        let delta_y = my - self.core.drag.tile_start_y;
        self.core.workspaces
            .active_mut()
            .resize_tile_pair(col_idx, top_tile_idx, delta_y);
        self.core.drag.tile_start_y = my;
        if let Some(w) = &self.window {
            w.request_redraw();
        }
        true
    }

    pub(crate) fn apply_column_resize_drag(&mut self, mx: f32) -> bool {
        let Some(left_idx) = self.core.drag.col_dragging else {
            return false;
        };
        let Some(right_idx) = self.core.drag.col_right_idx else {
            return false;
        };
        let delta_px = mx - self.core.drag.col_start_x;
        let vw = self.core.workspaces.active().view_size.width;
        if vw > 0.0 {
            let delta_proportion = delta_px as f64 / vw as f64;
            let ws = self.core.workspaces.active_mut();
            let before = ws.columns[left_idx].proportion(vw);
            ws.resize_column_pair(left_idx, right_idx, delta_proportion);
            let after = ws.columns[left_idx].proportion(vw);
            self.core.drag.col_delta += after - before;
            self.core.drag.col_start_x = mx;
        }
        self.snap_all_col_widths();
        if let Some(w) = &self.window {
            w.request_redraw();
        }
        true
    }

    /// Delegate: content-area resize hit test.
    pub(crate) fn content_resize_hit_test(&self, mx: f32, my: f32) -> (bool, bool) {
        self.core.content_resize_hit_test(mx, my)
    }

    pub(crate) fn start_column_resize_drag(&mut self, mx: f32) -> bool {
        let vox = self.core.anim_mgr.view_offset_x.value() as f32;
        let ws = self.core.workspaces.active();
        let vw = ws.view_size.width;

        // Find matching column border and collect info before mutating
        let mut found = None;
        for i in 1..ws.columns.len() {
            let col_x = ws.column_x(i) - vox;
            if (mx - col_x).abs() < 4.0 {
                let left_col_idx = i - 1;
                let right_col_idx = i;
                let left_col_width = ws.columns[left_col_idx].effective_width(vw);
                let pane_id = ws.columns[left_col_idx].active_pane_id();
                let dim_panes: Vec<_> = ws.columns[left_col_idx]
                    .tiles
                    .iter()
                    .chain(ws.columns[right_col_idx].tiles.iter())
                    .map(|t| t.pane_id)
                    .collect();
                found = Some((left_col_idx, right_col_idx, left_col_width, pane_id, dim_panes));
                break;
            }
        }

        if let Some((left_col_idx, right_col_idx, left_col_width, pane_id, dim_panes)) = found {
            self.remember_workspace_pane(self.core.workspaces.active_workspace_idx, pane_id);
            self.send(ciri_protocol::message::ClientMessage::FocusPane { pane_id });
            self.core.drag.col_dragging = Some(left_col_idx);
            self.core.drag.col_right_idx = Some(right_col_idx);
            self.core.drag.col_start_x = mx;
            self.core.drag.col_start_width = left_col_width;
            self.core.drag.col_delta = 0.0;

            let config = self.anim_config();
            for pid in dim_panes {
                self.core.anim_mgr.ensure_pane_registered(pid);
                self.core.anim_mgr.start_drag_dim(pid, &config);
            }

            return true;
        }
        false
    }

    pub(crate) fn start_tile_resize_drag(&mut self, mx: f32, my: f32) -> bool {
        if let Some((col_idx, top_tile_idx)) = self
            .core.workspaces
            .active()
            .hit_test_tile_border(self.core.anim_mgr.view_offset_x.value() as f32, mx, my, 4.0)
        {
            if let Some(col) = self.core.workspaces.active().columns.get(col_idx)
                && let Some(tile) = col.tiles.get(top_tile_idx)
            {
                let pane_id = tile.pane_id;
                self.remember_workspace_pane(self.core.workspaces.active_workspace_idx, pane_id);
                self.send(ciri_protocol::message::ClientMessage::FocusPane { pane_id });
            }
            self.core.drag.tile_dragging = Some((col_idx, top_tile_idx));
            self.core.drag.tile_start_y = my;
            return true;
        }
        false
    }

    pub(crate) fn finish_resize_drag(&mut self) -> bool {
        if self.core.drag.scrollbar_dragging.is_some() {
            self.core.drag.scrollbar_dragging = None;
            return true;
        }
        if let Some((col_idx, top_tile_idx)) = self.core.drag.tile_dragging {
            let ws = self.core.workspaces.active();
            if let Some(col) = ws.columns.get(col_idx) {
                let bot_idx = top_tile_idx + 1;
                if bot_idx < col.tiles.len() {
                    let top_w = col.tiles[top_tile_idx].height.weight() as f64;
                    let bot_w = col.tiles[bot_idx].height.weight() as f64;
                    self.send(ciri_protocol::message::ClientMessage::SetTileWeights {
                        column_idx: col_idx,
                        top_tile_idx,
                        top_weight: top_w,
                        bottom_weight: bot_w,
                    });
                }
            }
            self.core.drag.tile_dragging = None;
            if let Some(w) = &self.window {
                w.set_cursor(winit::window::CursorIcon::Default);
            }
        }
        if let Some(drag_col) = self.core.drag.col_dragging {
            self.send(ciri_protocol::message::ClientMessage::AdjustColumnSplitAt {
                column_idx: drag_col,
                delta: self.core.drag.col_delta,
            });

            // Restore opacity for dimmed panes
            let config = self.anim_config();
            let ws = self.core.workspaces.active();
            let right_idx = self.core.drag.col_right_idx.unwrap_or(drag_col + 1);
            let pane_ids: Vec<_> = [drag_col, right_idx]
                .iter()
                .filter_map(|&idx| ws.columns.get(idx))
                .flat_map(|col| col.tiles.iter().map(|t| t.pane_id))
                .collect();
            for pid in pane_ids {
                self.core.anim_mgr.end_drag_dim(pid, &config);
            }

            self.core.drag.col_dragging = None;
            self.snap_all_col_widths();
            if let Some(w) = &self.window {
                w.set_cursor(winit::window::CursorIcon::Default);
            }
        }
        false
    }
}
