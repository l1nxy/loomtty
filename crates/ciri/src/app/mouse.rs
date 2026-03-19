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

        if self.overview_active {
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

            if self.overview_dragging {
                if let Some((lx, ly)) = self.drag_last_pos {
                    let zoom = self.overview_zoom.value() as f32;
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
                self.drag_last_pos = Some((mx, my));
            }
        } else {
            if let Some((col_idx, top_tile_idx)) = self.tile_resize_dragging {
                let delta_y = my - self.tile_resize_drag_start_y;
                self.workspaces.active_mut().resize_tile_pair(col_idx, top_tile_idx, delta_y);
                self.tile_resize_drag_start_y = my;
                if let Some(w) = &self.window { w.request_redraw(); }
            } else if let Some(drag_col) = self.resize_dragging {
                let delta_px = mx - self.resize_drag_start_x;
                let vw = self.workspaces.active().view_size.width;
                if vw > 0.0 {
                    let delta_proportion = delta_px as f64 / vw as f64;
                    // Temporarily focus the left column to use resize_active_with_neighbor
                    let ws = self.workspaces.active_mut();
                    let saved_idx = ws.active_column_idx;
                    ws.active_column_idx = drag_col;
                    ws.resize_active_with_neighbor(delta_proportion);
                    ws.active_column_idx = saved_idx;
                    // Reset drag baseline so next move is incremental
                    self.resize_drag_start_x = mx;
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
        if self.overview_active {
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
                self.overview_active = false;
                self.overview_zoom
                    .animate_to(1.0, self.config.animation.speed);
                self.animate_to_active();
            } else {
                self.overview_dragging = true;
                self.drag_last_pos = Some((mx, my));
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
                    self.resize_dragging = Some(left_col_idx);
                    self.resize_drag_start_x = mx;
                    self.resize_drag_start_width = left_col_width;
                    started_drag = true;
                    break;
                }
            }

            // Check for tile border drag
            if !started_drag {
                if let Some((col_idx, top_tile_idx)) = self.workspaces.active().hit_test_tile_border(self.view_offset_x.value() as f32, mx, my, 4.0) {
                    self.tile_resize_dragging = Some((col_idx, top_tile_idx));
                    self.tile_resize_drag_start_y = my;
                    started_drag = true;
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
        if self.tile_resize_dragging.is_some() {
            self.tile_resize_dragging = None;
            if let Some(w) = &self.window {
                w.set_cursor(winit::window::CursorIcon::Default);
            }
        }
        if self.resize_dragging.is_some() {
            // Sync final width to server
            let ws = self.workspaces.active();
            if let Some(col) = ws.columns.get(ws.active_column_idx) {
                let p = col.proportion(ws.view_size.width);
                self.send(ClientMessage::SetColumnWidth { proportion: p });
            }
            self.resize_dragging = None;
            self.snap_all_col_widths();
            if let Some(w) = &self.window {
                w.set_cursor(winit::window::CursorIcon::Default);
            }
        }
        self.overview_dragging = false;
        self.drag_last_pos = None;

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
        if self.overview_active {
            // Overview mode: vertical scroll controls zoom level
            let dy = match delta {
                MouseScrollDelta::LineDelta(_, y) => y as f64 * 0.05,
                MouseScrollDelta::PixelDelta(pos) => pos.y * 0.001,
            };
            let cur_zoom = self.overview_zoom.value();
            let new_zoom = (cur_zoom + dy).clamp(0.05, 1.0);
            let omega = self.config.animation.speed;
            if new_zoom >= self.config.animation.zoom_threshold as f64 {
                self.overview_active = false;
                self.overview_zoom.animate_to(1.0, omega);
                self.animate_to_active();
            } else {
                self.overview_zoom.animate_to(new_zoom, omega);
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
                        self.gesture_row_active = true;
                        self.gesture_row_start = self.workspaces.active_workspace_idx;
                        self.gesture_row_offset.begin_gesture();
                    }
                    TouchPhase::Moved => {
                        if self.gesture_row_active {
                            self.gesture_row_offset.update_gesture_unclamped(py);
                            let accum = self.gesture_row_offset.value();
                            if accum > threshold {
                                self.workspaces.focus_down();
                                self.gesture_row_offset.jump_to(0.0);
                                self.gesture_row_offset.begin_gesture();
                                self.animate_to_active();
                            } else if accum < -threshold {
                                self.workspaces.focus_up();
                                self.gesture_row_offset.jump_to(0.0);
                                self.gesture_row_offset.begin_gesture();
                                self.animate_to_active();
                            }
                        }
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        self.gesture_row_active = false;
                        self.gesture_row_offset.end_gesture(0.0, omega);
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
                        self.gesture_scroll_accum = 0.0;
                    }
                    TouchPhase::Moved | TouchPhase::Ended | TouchPhase::Cancelled => {
                        self.gesture_scroll_accum += py;
                        let ppl = self.config.gesture.scroll_pixels_per_line;
                        let lines = (self.gesture_scroll_accum / ppl) as i64;
                        if lines != 0 {
                            self.gesture_scroll_accum -= lines as f64 * ppl;
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
                    let t = self.workspaces.active_mut().target_offset_for_active();
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
                if self.overview_active {
                    // In overview: pinch out (delta > 0) zooms in toward normal
                    let cur_zoom = self.overview_zoom.value();
                    let new_zoom = (cur_zoom + zoom_delta).clamp(0.05, 1.0);
                    if new_zoom >= self.config.animation.zoom_threshold as f64 {
                        self.overview_active = false;
                        self.overview_zoom.animate_to(1.0, omega);
                        self.animate_to_active();
                    } else {
                        self.overview_zoom.animate_to(new_zoom, omega);
                    }
                } else {
                    // In normal mode: pinch in (delta < 0) enters overview
                    if zoom_delta < -0.02 {
                        self.overview_active = true;
                        self.refresh_overview_zoom();
                        self.view_offset_x.animate_to(0.0, omega);
                        self.view_offset_y.animate_to(0.0, omega);
                    }
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                // Snap: if barely zoomed out, snap back to normal
                if self.overview_active
                    && self.overview_zoom.value() > self.config.animation.zoom_threshold as f64
                {
                    self.overview_active = false;
                    self.overview_zoom.animate_to(1.0, omega);
                    self.animate_to_active();
                }
            }
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}
