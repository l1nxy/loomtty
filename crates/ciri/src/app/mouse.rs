use ciri_protocol::message::ClientMessage;
use std::time::Instant;
use winit::dpi::PhysicalPosition;
use winit::event::{MouseButton, MouseScrollDelta, TouchPhase};

use super::App;

impl App {
    pub(crate) fn handle_cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        let mx = position.x as f32;
        let my = position.y as f32;
        self.last_mouse_pos = Some((mx, my));

        if self.overview.active {
            let hover_changed = self.clear_hovered_link();
            if let Some((ws_idx, pane_id)) = self.hit_test_overview(mx, my) {
                if ws_idx < self.workspaces.workspaces.len() {
                    self.workspaces.active_workspace_idx = ws_idx;
                    let ws = self.workspaces.active_mut();
                    for (col_idx, col) in ws.columns.iter().enumerate() {
                        if col.contains_pane(pane_id) {
                            ws.active_column_idx = col_idx;
                            break;
                        }
                    }
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            if hover_changed && let Some(w) = &self.window {
                w.request_redraw();
            }

            if self.overview.dragging {
                if let Some((lx, ly)) = self.overview.drag_last_pos {
                    let zoom = self.overview.zoom.value() as f32;
                    let dx = (mx - lx) / zoom;
                    let dy = (my - ly) / zoom;
                    let cur_x = self.view_offset_x.value();
                    self.view_offset_x.jump_to(cur_x - dx as f64);
                    let cur_y = self.view_offset_y.value();
                    self.view_offset_y.jump_to(cur_y - dy as f64);
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
                self.overview.drag_last_pos = Some((mx, my));
            }
        } else {
            // Scrollbar dragging takes priority over all other mouse interactions
            if let Some(ref info) = self.drag.scrollbar_dragging {
                let pane_id = info.pane_id;
                let inner_y = info.pane_inner_y;
                let inner_h = info.pane_inner_h;
                let total_lines = info.total_lines;
                let visible_rows = info.visible_rows as usize;
                if total_lines > visible_rows {
                    let max_offset = total_lines - visible_rows;
                    let ratio = ((my - inner_y) / inner_h).clamp(0.0, 1.0);
                    // ratio 0.0 = top of pane = max scroll (furthest into history)
                    // ratio 1.0 = bottom of pane = offset 0 (live viewport)
                    let new_offset = ((1.0 - ratio) * max_offset as f32).round() as usize;
                    let new_offset = new_offset.min(max_offset);
                    if let Some(grid) = self.pane_grids.get_mut(&pane_id) {
                        if grid.scroll_offset != new_offset {
                            grid.scroll_offset = new_offset;
                            grid.dirty = true;
                            self.invalidate_pane_cache(pane_id);
                            if let Some(w) = &self.window {
                                w.request_redraw();
                            }
                        }
                    }
                }
                return;
            }

            if let Some((col_idx, top_tile_idx)) = self.drag.tile_dragging {
                let delta_y = my - self.drag.tile_start_y;
                self.workspaces.active_mut().resize_tile_pair(col_idx, top_tile_idx, delta_y);
                self.drag.tile_start_y = my;
                if let Some(w) = &self.window { w.request_redraw(); }
            } else if let Some(drag_col) = self.drag.col_dragging {
                let delta_px = mx - self.drag.col_start_x;
                let vw = self.workspaces.active().view_size.width;
                if vw > 0.0 {
                    let delta_proportion = delta_px as f64 / vw as f64;
                    // Temporarily focus the left column to use resize_active_with_neighbor
                    let ws = self.workspaces.active_mut();
                    let saved_idx = ws.active_column_idx;
                    ws.active_column_idx = drag_col;
                    ws.resize_active_with_neighbor(delta_proportion);
                    ws.active_column_idx = saved_idx;
                    self.drag.col_delta += delta_proportion;
                    // Reset drag baseline so next move is incremental
                    self.drag.col_start_x = mx;
                }
                self.snap_all_col_widths();
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            } else {
                let ws = self.workspaces.active();
                let vox = self.view_offset_x.value() as f32;
                let mut near_col_border = false;
                for i in 1..ws.columns.len() {
                    let col_x = ws.column_x(i) - vox;
                    if (mx - col_x).abs() < 4.0 {
                        near_col_border = true;
                        break;
                    }
                }
                let near_tile_border = ws.hit_test_tile_border(vox, mx, my, 4.0).is_some();
                let near_border = near_col_border || near_tile_border;
                let hover_changed = if near_border || self.mouse_left_held {
                    self.clear_hovered_link()
                } else {
                    self.update_hovered_link(mx, my)
                };
                if let Some(w) = &self.window {
                    if near_col_border {
                        w.set_cursor(winit::window::CursorIcon::ColResize);
                    } else if near_tile_border {
                        w.set_cursor(winit::window::CursorIcon::RowResize);
                    } else if self.hovered_link.is_some() {
                        w.set_cursor(winit::window::CursorIcon::Pointer);
                    } else {
                        w.set_cursor(winit::window::CursorIcon::Default);
                    }
                }
                if hover_changed && let Some(w) = &self.window {
                    w.request_redraw();
                }

                if self.mouse_left_held {
                    let sel_active = self.selection.as_ref().is_some_and(|s| s.active);
                    if sel_active {
                        if let Some((_, col, buf_row)) = self.pixel_to_cell(mx, my) {
                            if let Some(sel) = &mut self.selection {
                                sel.end = (col, buf_row);
                            }
                            if let Some(w) = &self.window {
                                w.request_redraw();
                            }
                        }
                    }
                    if let Some((pane_id, col, row)) = self.pixel_to_viewport_cell(mx, my) {
                        self.send_lossy(ClientMessage::MouseInput {
                            pane_id,
                            button: 32,
                            col,
                            row,
                            pressed: true,
                            modifiers: 0,
                        });
                    }
                }
            }
        }
    }

    pub(crate) fn handle_mouse_pressed(&mut self, button: MouseButton, mx: f32, my: f32) {
        match button {
            MouseButton::Left => self.handle_left_mouse_pressed(mx, my),
            MouseButton::Right => self.handle_right_mouse_pressed(),
            _ => {}
        }
    }

    pub(crate) fn handle_mouse_released(&mut self, button: MouseButton) {
        match button {
            MouseButton::Left => self.handle_left_mouse_released(),
            _ => {}
        }
    }

    fn handle_left_mouse_pressed(&mut self, mx: f32, my: f32) {
        if self.overview.active {
            if let Some((ws_idx, pane_id)) = self.hit_test_overview(mx, my) {
                if ws_idx < self.workspaces.workspaces.len() {
                    self.workspaces.active_workspace_idx = ws_idx;
                    let ws = self.workspaces.active_mut();
                    for (col_idx, col) in ws.columns.iter().enumerate() {
                        if col.contains_pane(pane_id) {
                            ws.active_column_idx = col_idx;
                            break;
                        }
                    }
                }
                self.overview.active = false;
                self.overview.zoom
                    .animate_to(1.0, self.config.animation.speed);
                self.animate_to_active();
            } else {
                self.overview.dragging = true;
                self.overview.drag_last_pos = Some((mx, my));
            }
        } else {
            let ws = self.workspaces.active();
            let vox = self.view_offset_x.value() as f32;
            let vw = ws.view_size.width;
            let mut started_drag = false;
            for i in 1..ws.columns.len() {
                let col_x = ws.column_x(i) - vox;
                if (mx - col_x).abs() < 4.0 {
                    let left_col_idx = i - 1;
                    let left_col_width =
                        ws.columns[left_col_idx].effective_width(vw);
                    self.drag.col_dragging = Some(left_col_idx);
                    self.drag.col_start_x = mx;
                    self.drag.col_start_width = left_col_width;
                    self.drag.col_delta = 0.0;
                    started_drag = true;
                    break;
                }
            }

            // Check for tile border drag
            if !started_drag {
                if let Some((col_idx, top_tile_idx)) = self.workspaces.active().hit_test_tile_border(self.view_offset_x.value() as f32, mx, my, 4.0) {
                    self.drag.tile_dragging = Some((col_idx, top_tile_idx));
                    self.drag.tile_start_y = my;
                    started_drag = true;
                }
            }

            // Check for scrollbar click/drag
            if !started_drag {
                if let Some(hit) = self.hit_test_scrollbar(mx, my) {
                    if hit.on_thumb {
                        // Start dragging the scrollbar thumb
                        self.drag.scrollbar_dragging = Some(super::ScrollbarDragInfo {
                            pane_id: hit.pane_id,
                            pane_inner_y: hit.inner_y,
                            pane_inner_h: hit.inner_h,
                            total_lines: hit.total_lines,
                            visible_rows: hit.visible_rows,
                        });
                    } else {
                        // Clicked on the track (not thumb) — page up or page down
                        let page = hit.visible_rows as usize;
                        if let Some(sb) = &hit.scrollbar_rect {
                            let thumb_screen_y = hit.inner_y + sb.y;
                            if my < thumb_screen_y {
                                // Clicked above thumb → page up (into history)
                                self.scroll_pane_up(hit.pane_id, page);
                            } else {
                                // Clicked below thumb → page down (toward live)
                                self.scroll_pane_down(hit.pane_id, page);
                            }
                        }
                    }
                    started_drag = true;
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
            }

            if !started_drag {
                let shift = self.modifiers.shift_key();
                if let Some((pane_id, col, buf_row)) = self.pixel_to_cell(mx, my) {
                    if !shift
                        && self.link_activation_modifier_active()
                        && let Some(url) =
                            self.hovered_link_url_at(pane_id, col, buf_row)
                    {
                        self.selection = None;
                        self.open_url(&url);
                        if let Some(w) = &self.window {
                            w.request_redraw();
                        }
                        return;
                    }

                    self.mouse_left_held = true;
                    let click_now = Instant::now();
                    let is_double_click = !shift
                        && self.is_double_left_click(pane_id, col, buf_row, click_now);
                    let ws = self.workspaces.active_mut();
                    for col_idx in 0..ws.columns.len() {
                        if ws.columns[col_idx].contains_pane(pane_id) {
                            ws.active_column_idx = col_idx;
                            // Also focus the specific tile within the column
                            if let Some(tile_idx) = ws.columns[col_idx].tiles.iter()
                                .position(|t| t.pane_id == pane_id)
                            {
                                ws.columns[col_idx].active_tile_idx = tile_idx;
                            }
                            break;
                        }
                    }
                    self.animate_to_active();
                    self.remember_left_click(pane_id, col, buf_row, click_now);

                    if shift {
                        self.selection = Some(super::Selection {
                            pane_id,
                            start: (col, buf_row),
                            end: (col, buf_row),
                            active: true,
                        });
                    } else if is_double_click {
                        if !self.select_word_at(pane_id, col, buf_row) {
                            self.selection = Some(super::Selection {
                                pane_id, start: (col, buf_row), end: (col, buf_row), active: true,
                            });
                        }
                    } else {
                        if let Some((_, vcol, vrow)) =
                            self.pixel_to_viewport_cell(mx, my)
                        {
                            self.send_lossy(ClientMessage::MouseInput {
                                pane_id,
                                button: 0,
                                col: vcol,
                                row: vrow,
                                pressed: true,
                                modifiers: 0,
                            });
                        }
                        self.selection = Some(super::Selection {
                            pane_id,
                            start: (col, buf_row),
                            end: (col, buf_row),
                            active: true,
                        });
                    }
                }
            }
        }
    }

    fn handle_left_mouse_released(&mut self) {
        let (mx, my) = match self.last_mouse_pos {
            Some(pos) => pos,
            None => return,
        };

        let had_left_hold = self.mouse_left_held;
        self.mouse_left_held = false;
        self.drag.scrollbar_dragging = None;
        if let Some((col_idx, top_tile_idx)) = self.drag.tile_dragging {
            // Send the final absolute tile weights to the server so PTYs are
            // resized and the layout is persisted. Read from local preview state
            // which already reflects the drag.
            let ws = self.workspaces.active();
            if let Some(col) = ws.columns.get(col_idx) {
                let bot_idx = top_tile_idx + 1;
                if bot_idx < col.tiles.len() {
                    let top_w = col.tiles[top_tile_idx].height.weight() as f64;
                    let bot_w = col.tiles[bot_idx].height.weight() as f64;
                    self.send(ClientMessage::SetTileWeights {
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
            // Send the accumulated delta to the specific column pair being dragged
            // (not the active column) using AdjustColumnSplitAt.
            self.send(ClientMessage::AdjustColumnSplitAt {
                column_idx: drag_col,
                delta: self.drag.col_delta,
            });
            self.drag.col_dragging = None;
            self.snap_all_col_widths();
            if let Some(w) = &self.window {
                w.set_cursor(winit::window::CursorIcon::Default);
            }
        }
        self.overview.dragging = false;
        self.overview.drag_last_pos = None;

        if had_left_hold
            && let Some((pane_id, col, row)) = self.pixel_to_viewport_cell(mx, my)
        {
            self.send_lossy(ClientMessage::MouseInput {
                pane_id,
                button: 3,
                col,
                row,
                pressed: false,
                modifiers: 0,
            });
        }

        if let Some(sel) = &self.selection {
            log::debug!(
                "selection release: start={:?} end={:?} active={}",
                sel.start,
                sel.end,
                sel.active
            );
            if sel.active && sel.start != sel.end {
                if let Some(text) = self.extract_selected_text() {
                    log::info!("auto-copy selection: {} bytes", text.len());
                    if let Some(cb) = &mut self.clipboard {
                        let _ = cb.set_text(&text);
                    }
                }
            }
        }
        if let Some(sel) = &mut self.selection {
            sel.active = false;
        }
    }

    fn handle_right_mouse_pressed(&mut self) {
        // Right-click = copy selection to clipboard (Ghostty-style)
        if let Some(text) = self.extract_selected_text() {
            if !text.is_empty() {
                if let Some(cb) = &mut self.clipboard {
                    let _ = cb.set_text(&text);
                    log::debug!("right-click copy: {} bytes", text.len());
                }
            }
        }
        self.selection = None;
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    pub(crate) fn handle_mouse_wheel(&mut self, delta: MouseScrollDelta, phase: TouchPhase) {
        if self.overview.active {
            // Overview mode: vertical scroll controls zoom level
            let dy = match delta {
                MouseScrollDelta::LineDelta(_, y) => y as f64 * 0.05,
                MouseScrollDelta::PixelDelta(pos) => pos.y * 0.001,
            };
            let cur_zoom = self.overview.zoom.value();
            let new_zoom = (cur_zoom + dy).clamp(0.05, 1.0);
            let omega = self.config.animation.speed;
            if new_zoom >= self.config.animation.zoom_threshold as f64 {
                self.overview.active = false;
                self.overview.zoom.animate_to(1.0, omega);
                self.animate_to_active();
            } else {
                self.overview.zoom.animate_to(new_zoom, omega);
            }
        } else {
            let gestures_enabled = self.config.gesture.enabled;
            let smooth_scroll = gestures_enabled && self.config.gesture.smooth_scroll;
            let has_mouse = self
                .workspaces
                .active()
                .active_pane_id()
                .and_then(|pid| self.pane_grids.get(&pid))
                .is_some_and(|g| {
                    g.mode_flags & ciri_protocol::message::MODE_MOUSE_REPORT != 0
                });
            let is_alt_screen = self
                .workspaces
                .active()
                .active_pane_id()
                .and_then(|pid| self.pane_grids.get(&pid))
                .is_some_and(|g| {
                    g.mode_flags & ciri_protocol::message::MODE_ALT_SCREEN != 0
                });
            let shift_held = self.modifiers.shift_key();
            let multi_row = self.workspaces.workspaces.len() > 1;

            // ── Shift + vertical scroll: workspace row switching gesture ──
            if gestures_enabled
                && shift_held
                && multi_row
                && matches!(delta, MouseScrollDelta::PixelDelta(_))
            {
                let py = match delta {
                    MouseScrollDelta::PixelDelta(pos) => pos.y,
                    _ => unreachable!(),
                };
                let threshold = self.config.gesture.vertical_swipe_threshold;
                let omega = self.config.animation.speed;

                match phase {
                    TouchPhase::Started => {
                        self.gestures.row_active = true;
                        self.gestures.row_start = self.workspaces.active_workspace_idx;
                        self.gestures.row_offset.begin_gesture();
                    }
                    TouchPhase::Moved => {
                        if self.gestures.row_active {
                            self.gestures.row_offset.update_gesture_unclamped(py);
                            let accum = self.gestures.row_offset.value();
                            if accum > threshold {
                                self.workspaces.focus_down();
                                self.gestures.row_offset.jump_to(0.0);
                                self.gestures.row_offset.begin_gesture();
                                self.animate_to_active();
                            } else if accum < -threshold {
                                self.workspaces.focus_up();
                                self.gestures.row_offset.jump_to(0.0);
                                self.gestures.row_offset.begin_gesture();
                                self.animate_to_active();
                            }
                        }
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        self.gestures.row_active = false;
                        self.gestures.row_offset.end_gesture(0.0, omega);
                        self.animate_to_active();
                    }
                }
            }
            // ── Smooth pixel-level scrollback (trackpad gestures) ──
            else if smooth_scroll
                && matches!(delta, MouseScrollDelta::PixelDelta(_))
                && !has_mouse
                && !is_alt_screen
            {
                let py = match delta {
                    MouseScrollDelta::PixelDelta(pos) => {
                        if self.config.gesture.natural_scroll {
                            pos.y
                        } else {
                            -pos.y
                        }
                    }
                    _ => unreachable!(),
                };

                match phase {
                    TouchPhase::Started => {
                        self.gestures.scroll_accum = 0.0;
                    }
                    TouchPhase::Moved | TouchPhase::Ended | TouchPhase::Cancelled => {
                        self.gestures.scroll_accum += py;
                        let ppl = self.config.gesture.scroll_pixels_per_line;
                        let lines = (self.gestures.scroll_accum / ppl) as i64;
                        if lines != 0 {
                            self.gestures.scroll_accum -= lines as f64 * ppl;
                            if lines > 0 {
                                self.scroll_active_up(lines as usize);
                            } else {
                                self.scroll_active_down((-lines) as usize);
                            }
                        }
                    }
                }
            } else {
                // Discrete line-based scrollback (mouse wheel or non-smooth mode)
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as i32 * 3,
                    MouseScrollDelta::PixelDelta(pos) => {
                        let (_, ch) = self.cell_dimensions();
                        if ch > 0.0 {
                            (pos.y as f32 / ch).round() as i32
                        } else {
                            0
                        }
                    }
                };
                if dy != 0 {
                    if has_mouse {
                        if let Some(pid) = self.workspaces.active().active_pane_id() {
                            if let Some((_, col, row)) = self
                                .last_mouse_pos
                                .and_then(|(mx, my)| self.pixel_to_viewport_cell(mx, my))
                            {
                                let button = if dy > 0 { 64u8 } else { 65u8 };
                                let count = dy.unsigned_abs().min(10);
                                for _ in 0..count {
                                    self.send_lossy(ClientMessage::MouseInput {
                                        pane_id: pid,
                                        button,
                                        col,
                                        row,
                                        pressed: true,
                                        modifiers: 0,
                                    });
                                }
                            }
                        }
                    } else if is_alt_screen {
                        // Alt screen but no mouse mode: send arrow keys for scrolling
                        if let Some(pid) = self.workspaces.active().active_pane_id() {
                            let key = if dy > 0 { b"\x1b[A" } else { b"\x1b[B" };
                            let count = dy.unsigned_abs().min(10) as usize;
                            for _ in 0..count {
                                self.send(ClientMessage::Input {
                                    pane_id: pid,
                                    data: key.to_vec(),
                                });
                            }
                        }
                    } else {
                        if dy > 0 {
                            self.scroll_active_up(dy as usize);
                        } else {
                            self.scroll_active_down((-dy) as usize);
                        }
                    }
                }
            }

            // ── Horizontal gesture: column switching ──
            let scroll_mult = self.config.input.scroll_multiplier;
            let dx = match delta {
                MouseScrollDelta::LineDelta(x, _) => x as f64 * scroll_mult,
                MouseScrollDelta::PixelDelta(pos) => pos.x,
            };
            match phase {
                TouchPhase::Started => {
                    self.view_offset_x.begin_gesture();
                }
                TouchPhase::Moved => {
                    self.view_offset_x.update_gesture(dx);
                }
                TouchPhase::Ended | TouchPhase::Cancelled => {
                    let center_strategy = match self.config.layout.center_focused_column {
                        ciri_config::config::CenterStrategy::Always => ciri_layout::workspace::CenterStrategy::Always,
                        ciri_config::config::CenterStrategy::OnOverflow => ciri_layout::workspace::CenterStrategy::OnOverflow,
                        ciri_config::config::CenterStrategy::Never => ciri_layout::workspace::CenterStrategy::Never,
                    };
                    let current_vox = self.view_offset_x.value() as f32;
                    let t = self.workspaces.active_mut()
                        .target_offset_for_active_with_strategy(center_strategy, current_vox);
                    let speed = self.config.animation.speed;
                    self.view_offset_x.end_gesture(t as f64, speed);
                }
            }
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// Handle pinch-to-zoom gesture (macOS/trackpad).
    pub(crate) fn handle_pinch_gesture(&mut self, delta: f64, phase: TouchPhase) {
        if !self.config.gesture.enabled || !delta.is_finite() {
            return;
        }
        let sensitivity = self.config.gesture.pinch_sensitivity;
        let omega = self.config.animation.speed;

        match phase {
            TouchPhase::Started => {}
            TouchPhase::Moved => {
                let zoom_delta = delta * sensitivity;
                if self.overview.active {
                    // In overview: pinch out (delta > 0) zooms in toward normal
                    let cur_zoom = self.overview.zoom.value();
                    let new_zoom = (cur_zoom + zoom_delta).clamp(0.05, 1.0);
                    if new_zoom >= self.config.animation.zoom_threshold as f64 {
                        self.overview.active = false;
                        self.overview.zoom.animate_to(1.0, omega);
                        self.animate_to_active();
                    } else {
                        self.overview.zoom.animate_to(new_zoom, omega);
                    }
                } else {
                    // In normal mode: pinch in (delta < 0) enters overview
                    if zoom_delta < -0.02 {
                        self.overview.active = true;
                        self.refresh_overview_zoom();
                        self.view_offset_x.animate_to(0.0, omega);
                        self.view_offset_y.animate_to(0.0, omega);
                    }
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                // Snap: if barely zoomed out, snap back to normal
                if self.overview.active
                    && self.overview.zoom.value() > self.config.animation.zoom_threshold as f64
                {
                    self.overview.active = false;
                    self.overview.zoom.animate_to(1.0, omega);
                    self.animate_to_active();
                }
            }
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// Result of a scrollbar hit-test.
    fn hit_test_scrollbar(&self, mx: f32, my: f32) -> Option<ScrollbarHit> {
        let border_w = self.config.appearance.border_width;
        let padding = self.config.appearance.padding;
        let vox = self.view_offset_x.value() as f32;
        let tiles = self.workspaces.active().visible_tiles(vox);
        // Wider hit area (8px from right edge) for comfortable clicking
        let hit_zone_width = 8.0f32;

        for (pane_id, rect, _) in &tiles {
            if !rect.contains(mx, my) {
                continue;
            }
            let inner_x = rect.x + border_w + padding;
            let inner_y = rect.y + border_w + padding;
            let inset = (border_w + padding) * 2.0;
            let inner_w = rect.w - inset;
            let inner_h = rect.h - inset;

            // Check if click is in the scrollbar hit zone (right edge of pane)
            let scrollbar_hit_left = inner_x + inner_w - hit_zone_width;
            if mx < scrollbar_hit_left {
                continue;
            }

            let grid = self.pane_grids.get(pane_id)?;
            let total_lines = grid.total_lines();
            let visible_rows = grid.rows;
            if total_lines <= visible_rows as usize {
                // No scrollbar when content fits
                return None;
            }

            // Get the cached scrollbar rect (relative to pane inner origin)
            let view = self.cached_views.get(pane_id)?;
            let sb = view.scrollbar_rect.as_ref()?;

            // The scrollbar rect is relative to pane inner origin.
            // Convert thumb Y to screen coords for comparison.
            let thumb_screen_y = inner_y + sb.y;
            let thumb_screen_bottom = thumb_screen_y + sb.h;

            let on_thumb = my >= thumb_screen_y && my <= thumb_screen_bottom;

            return Some(ScrollbarHit {
                pane_id: *pane_id,
                inner_y,
                inner_h,
                total_lines,
                visible_rows,
                on_thumb,
                scrollbar_rect: Some(sb.clone()),
            });
        }
        None
    }

    /// Scroll a specific pane up (into history) by `lines`.
    fn scroll_pane_up(&mut self, pane_id: u64, lines: usize) {
        if let Some(grid) = self.pane_grids.get_mut(&pane_id) {
            grid.scroll_up(lines);
            self.invalidate_pane_cache(pane_id);
        }
    }

    /// Scroll a specific pane down (toward live) by `lines`.
    fn scroll_pane_down(&mut self, pane_id: u64, lines: usize) {
        if let Some(grid) = self.pane_grids.get_mut(&pane_id) {
            grid.scroll_down(lines);
            self.invalidate_pane_cache(pane_id);
        }
    }
}

/// Scrollbar hit-test result.
struct ScrollbarHit {
    pane_id: u64,
    inner_y: f32,
    inner_h: f32,
    total_lines: usize,
    visible_rows: u16,
    on_thumb: bool,
    scrollbar_rect: Option<ciri_render::rect::Rect>,
}
