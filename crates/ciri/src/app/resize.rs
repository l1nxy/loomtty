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
        let Some(info) = self.drag.scrollbar_dragging.as_ref() else {
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
        if let Some(grid) = self.pane_grids.get_mut(&pane_id)
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
        let Some((col_idx, top_tile_idx)) = self.drag.tile_dragging else {
            return false;
        };
        let delta_y = my - self.drag.tile_start_y;
        self.workspaces
            .active_mut()
            .resize_tile_pair(col_idx, top_tile_idx, delta_y);
        self.drag.tile_start_y = my;
        if let Some(w) = &self.window {
            w.request_redraw();
        }
        true
    }

    pub(crate) fn apply_column_resize_drag(&mut self, mx: f32) -> bool {
        let Some(left_idx) = self.drag.col_dragging else {
            return false;
        };
        let Some(right_idx) = self.drag.col_right_idx else {
            return false;
        };
        let delta_px = mx - self.drag.col_start_x;
        let vw = self.workspaces.active().view_size.width;
        if vw > 0.0 {
            let delta_proportion = delta_px as f64 / vw as f64;
            let ws = self.workspaces.active_mut();
            let before = ws.columns[left_idx].proportion(vw);
            ws.resize_column_pair(left_idx, right_idx, delta_proportion);
            let after = ws.columns[left_idx].proportion(vw);
            self.drag.col_delta += after - before;
            self.drag.col_start_x = mx;
        }
        self.snap_all_col_widths();
        if let Some(w) = &self.window {
            w.request_redraw();
        }
        true
    }

    pub(crate) fn content_resize_hit_test(
        &self,
        mx: f32,
        my: f32,
    ) -> (bool, bool) {
        let ws = self.workspaces.active();
        let vox = self.anim_mgr.view_offset_x.value() as f32;
        let mut near_col_border = false;
        for i in 1..ws.columns.len() {
            let col_x = ws.column_x(i) - vox;
            if (mx - col_x).abs() < 4.0 {
                near_col_border = true;
                break;
            }
        }
        let near_tile_border = ws.hit_test_tile_border(vox, mx, my, 4.0).is_some();
        (near_col_border, near_tile_border)
    }

    pub(crate) fn start_column_resize_drag(&mut self, mx: f32) -> bool {
        let ws = self.workspaces.active();
        let vox = self.anim_mgr.view_offset_x.value() as f32;
        let vw = ws.view_size.width;
        for i in 1..ws.columns.len() {
            let col_x = ws.column_x(i) - vox;
            if (mx - col_x).abs() < 4.0 {
                let left_col_idx = i - 1;
                let right_col_idx = i;
                let left_col_width = ws.columns[left_col_idx].effective_width(vw);
                let pane_id = ws.columns[left_col_idx].active_pane_id();
                self.remember_workspace_pane(self.workspaces.active_workspace_idx, pane_id);
                self.send(ciri_protocol::message::ClientMessage::FocusPane { pane_id });
                self.drag.col_dragging = Some(left_col_idx);
                self.drag.col_right_idx = Some(right_col_idx);
                self.drag.col_start_x = mx;
                self.drag.col_start_width = left_col_width;
                self.drag.col_delta = 0.0;
                return true;
            }
        }
        false
    }

    pub(crate) fn start_tile_resize_drag(&mut self, mx: f32, my: f32) -> bool {
        if let Some((col_idx, top_tile_idx)) = self
            .workspaces
            .active()
            .hit_test_tile_border(self.anim_mgr.view_offset_x.value() as f32, mx, my, 4.0)
        {
            if let Some(col) = self.workspaces.active().columns.get(col_idx)
                && let Some(tile) = col.tiles.get(top_tile_idx)
            {
                let pane_id = tile.pane_id;
                self.remember_workspace_pane(self.workspaces.active_workspace_idx, pane_id);
                self.send(ciri_protocol::message::ClientMessage::FocusPane { pane_id });
            }
            self.drag.tile_dragging = Some((col_idx, top_tile_idx));
            self.drag.tile_start_y = my;
            return true;
        }
        false
    }

    pub(crate) fn finish_resize_drag(&mut self) -> bool {
        if self.drag.scrollbar_dragging.is_some() {
            self.drag.scrollbar_dragging = None;
            return true;
        }
        if let Some((col_idx, top_tile_idx)) = self.drag.tile_dragging {
            let ws = self.workspaces.active();
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
            self.drag.tile_dragging = None;
            if let Some(w) = &self.window {
                w.set_cursor(winit::window::CursorIcon::Default);
            }
        }
        if let Some(drag_col) = self.drag.col_dragging {
            self.send(ciri_protocol::message::ClientMessage::AdjustColumnSplitAt {
                column_idx: drag_col,
                delta: self.drag.col_delta,
            });
            self.drag.col_dragging = None;
            self.snap_all_col_widths();
            if let Some(w) = &self.window {
                w.set_cursor(winit::window::CursorIcon::Default);
            }
        }
        false
    }
}
