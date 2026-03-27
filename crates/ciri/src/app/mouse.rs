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

        if self.overview.active && self.overview.dragging {
            self.handle_overview_cursor_moved(mx, my);
        } else if self.handle_ui_cursor_hover(mx, my) {
            return;
        } else if self.overview.active {
            self.handle_overview_cursor_moved(mx, my);
        } else {
            self.handle_main_cursor_moved(mx, my);
        }
    }

    pub(crate) fn handle_mouse_pressed(&mut self, button: MouseButton, mx: f32, my: f32) {
        match button {
            MouseButton::Left => {
                if self.dispatch_ui_click(mx, my) {
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                    return;
                }

                self.handle_left_mouse_pressed(mx, my);
            }
            MouseButton::Right => self.handle_right_mouse_pressed(mx, my),
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
            self.dispatch_ui_click(mx, my);
        } else {
            let mut started_drag = self.start_column_resize_drag(mx);
            if !started_drag {
                started_drag = self.start_tile_resize_drag(mx, my);
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
                        && let Some(url) = self.hovered_link_url_at(pane_id, col, buf_row)
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
                    let is_double_click =
                        !shift && self.is_double_left_click(pane_id, col, buf_row, click_now);
                    let ws = self.workspaces.active_mut();
                    for col_idx in 0..ws.columns.len() {
                        if ws.columns[col_idx].contains_pane(pane_id) {
                            ws.active_column_idx = col_idx;
                            // Also focus the specific tile within the column
                            if let Some(tile_idx) = ws.columns[col_idx]
                                .tiles
                                .iter()
                                .position(|t| t.pane_id == pane_id)
                            {
                                ws.columns[col_idx].active_tile_idx = tile_idx;
                            }
                            break;
                        }
                    }
                    self.remember_workspace_pane(self.workspaces.active_workspace_idx, pane_id);
                    self.send(ClientMessage::FocusPane { pane_id });
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
                                pane_id,
                                start: (col, buf_row),
                                end: (col, buf_row),
                                active: true,
                            });
                        }
                    } else {
                        if let Some((_, vcol, vrow)) = self.pixel_to_viewport_cell(mx, my) {
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
        if self.finish_resize_drag() {
            return;
        }
        self.overview.dragging = false;
        self.overview.drag_last_pos = None;

        if had_left_hold && let Some((pane_id, col, row)) = self.pixel_to_viewport_cell(mx, my) {
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
            if sel.active && sel.start != sel.end && self.config.terminal.copy_on_select {
                if let Some(text) = self.extract_selected_text() {
                    log::info!("copy-on-select: {} bytes", text.len());
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

    fn handle_right_mouse_pressed(&mut self, mx: f32, my: f32) {
        self.open_context_menu(mx, my);
    }

    pub(crate) fn handle_mouse_wheel(&mut self, delta: MouseScrollDelta, phase: TouchPhase) {
        if self.dismiss_context_menu_on_scroll() {
            return;
        }

        if self.handle_palette_wheel(delta) {
            return;
        }

        if self.handle_top_bar_wheel(delta) {
            return;
        }

        if self.overview.active {
            self.handle_overview_wheel(delta);
        } else {
            self.handle_main_wheel(delta, phase);
        }
        self.request_mouse_redraw();
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
                    let cur_zoom = self.anim_mgr.overview_zoom.value();
                    let new_zoom = (cur_zoom + zoom_delta).clamp(0.05, 1.0);
                    let epsilon = self.config.animation.epsilon;
                    if new_zoom >= self.config.animation.zoom_threshold as f64 {
                        self.overview.hovered_pane = None;
                        self.overview.active = false;
                        self.anim_mgr.overview_zoom.animate_to(1.0, omega, epsilon);
                        self.animate_to_active();
                    } else {
                        self.anim_mgr.overview_zoom.animate_to(new_zoom, omega, epsilon);
                    }
                } else {
                    // In normal mode: pinch in (delta < 0) enters overview
                    if zoom_delta < -0.02 {
                        self.overview.active = true;
                        self.overview.hovered_pane = None;
                        self.context_menu.visible = false;
                        self.refresh_overview_zoom();
                        let epsilon = self.config.animation.epsilon;
                        self.anim_mgr.view_offset_x.animate_to(0.0, omega, epsilon);
                        self.anim_mgr.view_offset_y.animate_to(0.0, omega, epsilon);
                    }
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                // Snap: if barely zoomed out, snap back to normal
                if self.overview.active
                    && self.anim_mgr.overview_zoom.value() > self.config.animation.zoom_threshold as f64
                {
                    self.overview.hovered_pane = None;
                    self.overview.active = false;
                    self.anim_mgr.overview_zoom.animate_to(1.0, omega, self.config.animation.epsilon);
                    self.animate_to_active();
                }
            }
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// Result of a scrollbar hit-test.
    fn hit_test_scrollbar(&self, mx: f32, my: f32) -> Option<super::resize::ScrollbarHit> {
        let border_w = self.config.appearance.border_width;
        let padding = self.config.appearance.padding;
        let vox = self.anim_mgr.view_offset_x.value() as f32;
        let tiles = self.workspaces.active().visible_tiles(vox);
        let my = self.content_y_from_screen(my)?;
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

            return Some(super::resize::ScrollbarHit {
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

    fn handle_ui_cursor_hover(&mut self, mx: f32, my: f32) -> bool {
        let ui_hover = self.dispatch_ui_hover(mx, my);
        if let Some(w) = &self.window {
            w.set_cursor(ui_hover.cursor);
            if ui_hover.needs_redraw {
                w.request_redraw();
            }
        }
        ui_hover.handled
    }

    fn handle_overview_cursor_moved(&mut self, mx: f32, my: f32) {
        let hover_changed = self.clear_hovered_link();
        if hover_changed {
            self.request_mouse_redraw();
        }

        if !self.overview.dragging {
            return;
        }

        if let Some((lx, ly)) = self.overview.drag_last_pos {
            let zoom = self.anim_mgr.overview_zoom.value() as f32;
            let dx = (mx - lx) / zoom;
            let dy = (my - ly) / zoom;
            self.anim_mgr.view_offset_x
                .jump_to(self.anim_mgr.view_offset_x.value() - dx as f64);
            self.anim_mgr.view_offset_y
                .jump_to(self.anim_mgr.view_offset_y.value() - dy as f64);
            self.request_mouse_redraw();
        }
        self.overview.drag_last_pos = Some((mx, my));
    }

    fn handle_main_cursor_moved(&mut self, mx: f32, my: f32) {
        if self.apply_scrollbar_drag(my) {
            return;
        }
        if self.apply_tile_resize_drag(my) || self.apply_column_resize_drag(mx) {
            return;
        }

        self.update_main_hover_state(mx, my);
        self.handle_focus_follows_mouse(mx, my);
        self.handle_mouse_drag_selection(mx, my);
    }

    fn update_main_hover_state(&mut self, mx: f32, my: f32) {
        let (near_col_border, near_tile_border) = self.content_resize_hit_test(mx, my);
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

        if hover_changed {
            self.request_mouse_redraw();
        }
    }

    fn handle_focus_follows_mouse(&mut self, mx: f32, my: f32) {
        if !self.config.input.focus_follows_mouse
            || self.mouse_left_held
            || self.search_state.is_some()
            || self.command_palette.is_some()
            || self.context_menu.visible
        {
            return;
        }

        let hover_pane = self.hovered_pane_at(mx, my);
        let Some(pane_id) = hover_pane else {
            self.last_focus_follows_mouse = None;
            return;
        };

        if self.workspaces.active().active_pane_id() == Some(pane_id) {
            return;
        }

        let now = Instant::now();
        let should_switch = match self.last_focus_follows_mouse {
            Some((last_id, last_time)) => {
                pane_id != last_id || now.duration_since(last_time).as_millis() > 50
            }
            None => true,
        };
        if !should_switch {
            return;
        }

        self.last_focus_follows_mouse = Some((pane_id, now));
        self.remember_workspace_pane(self.workspaces.active_workspace_idx, pane_id);
        self.send_lossy(ClientMessage::FocusPane { pane_id });
    }

    fn hovered_pane_at(&self, mx: f32, my: f32) -> Option<u64> {
        let vox = self.anim_mgr.view_offset_x.value() as f32;
        self.workspaces
            .active()
            .visible_tiles(vox)
            .into_iter()
            .find_map(|(pane_id, rect, _)| rect.contains(mx, my).then_some(pane_id))
    }

    fn handle_mouse_drag_selection(&mut self, mx: f32, my: f32) {
        if !self.mouse_left_held {
            return;
        }

        if self.selection.as_ref().is_some_and(|s| s.active)
            && let Some((_, col, buf_row)) = self.pixel_to_cell(mx, my)
        {
            if let Some(sel) = &mut self.selection {
                sel.end = (col, buf_row);
            }
            self.request_mouse_redraw();
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

    fn dismiss_context_menu_on_scroll(&mut self) -> bool {
        if !self.context_menu.visible {
            return false;
        }
        self.context_menu.visible = false;
        self.request_mouse_redraw();
        true
    }

    fn handle_palette_wheel(&mut self, delta: MouseScrollDelta) -> bool {
        let Some(palette) = &mut self.command_palette else {
            return false;
        };
        if palette.filtered.is_empty() {
            return true;
        }

        let steps = match delta {
            MouseScrollDelta::LineDelta(_, y) => {
                if y > 0.0 {
                    -1
                } else if y < 0.0 {
                    1
                } else {
                    0
                }
            }
            MouseScrollDelta::PixelDelta(pos) => {
                if pos.y > 0.0 {
                    -1
                } else if pos.y < 0.0 {
                    1
                } else {
                    0
                }
            }
        };
        if steps != 0 {
            let len = palette.filtered.len() as isize;
            let current = palette.selected_idx as isize;
            palette.selected_idx = (current + steps).clamp(0, len - 1) as usize;
            palette.hovered_idx = None;
            self.request_mouse_redraw();
        }
        true
    }

    fn handle_top_bar_wheel(&mut self, delta: MouseScrollDelta) -> bool {
        let Some((mx, my)) = self.last_mouse_pos else {
            return false;
        };
        if !self.hit_test_top_bar(mx, my) {
            return false;
        }

        let delta_px = match delta {
            MouseScrollDelta::LineDelta(_, y) => -(y as f32) * self.cell_dimensions().0 * 3.0,
            MouseScrollDelta::PixelDelta(pos) => -(pos.y as f32),
        };
        let max_scroll = self.pane_tab_scroll_max();
        self.pane_tab_scroll = (self.pane_tab_scroll + delta_px).clamp(0.0, max_scroll);
        self.request_mouse_redraw();
        true
    }

    fn handle_overview_wheel(&mut self, delta: MouseScrollDelta) {
        let dy = match delta {
            MouseScrollDelta::LineDelta(_, y) => y as f64 * 0.05,
            MouseScrollDelta::PixelDelta(pos) => pos.y * 0.001,
        };
        let cur_zoom = self.anim_mgr.overview_zoom.value();
        let new_zoom = (cur_zoom + dy).clamp(0.05, 1.0);
        let omega = self.config.animation.speed;
        let epsilon = self.config.animation.epsilon;
        if new_zoom >= self.config.animation.zoom_threshold as f64 {
            self.overview.active = false;
            self.anim_mgr.overview_zoom.animate_to(1.0, omega, epsilon);
            self.animate_to_active();
        } else {
            self.anim_mgr.overview_zoom.animate_to(new_zoom, omega, epsilon);
        }
    }

    fn handle_main_wheel(&mut self, delta: MouseScrollDelta, phase: TouchPhase) {
        let gestures_enabled = self.config.gesture.enabled;
        let smooth_scroll = gestures_enabled && self.config.gesture.smooth_scroll;
        let has_mouse = self
            .workspaces
            .active()
            .active_pane_id()
            .and_then(|pid| self.pane_grids.get(&pid))
            .is_some_and(|g| g.mode_flags & ciri_protocol::message::MODE_MOUSE_REPORT != 0);
        let is_alt_screen = self
            .workspaces
            .active()
            .active_pane_id()
            .and_then(|pid| self.pane_grids.get(&pid))
            .is_some_and(|g| g.mode_flags & ciri_protocol::message::MODE_ALT_SCREEN != 0);

        if self.handle_workspace_row_swipe(delta, phase, gestures_enabled) {
            return;
        }
        if self.handle_smooth_scrollback(delta, phase, smooth_scroll, has_mouse, is_alt_screen) {
            self.handle_horizontal_gesture(delta, phase);
            return;
        }

        self.handle_discrete_scroll(delta, has_mouse, is_alt_screen);
        self.handle_horizontal_gesture(delta, phase);
    }

    fn handle_workspace_row_swipe(
        &mut self,
        delta: MouseScrollDelta,
        phase: TouchPhase,
        gestures_enabled: bool,
    ) -> bool {
        let shift_held = self.modifiers.shift_key();
        let multi_row = self.workspaces.workspaces.len() > 1;
        if !(gestures_enabled
            && shift_held
            && multi_row
            && matches!(delta, MouseScrollDelta::PixelDelta(_)))
        {
            return false;
        }

        let MouseScrollDelta::PixelDelta(pos) = delta else {
            unreachable!();
        };
        let py = pos.y;
        let threshold = self.config.gesture.vertical_swipe_threshold;
        let omega = self.config.animation.speed;

        match phase {
            TouchPhase::Started => {
                self.gestures.row_active = true;
                self.gestures.row_start = self.workspaces.active_workspace_idx;
                self.anim_mgr.gesture_row_offset.begin_gesture();
            }
            TouchPhase::Moved => {
                if self.gestures.row_active {
                    self.anim_mgr.gesture_row_offset.update_gesture_unclamped(py);
                    let accum = self.anim_mgr.gesture_row_offset.value();
                    if accum > threshold {
                        self.workspaces.focus_down();
                        self.anim_mgr.gesture_row_offset.jump_to(0.0);
                        self.anim_mgr.gesture_row_offset.begin_gesture();
                        self.animate_to_active();
                    } else if accum < -threshold {
                        self.workspaces.focus_up();
                        self.anim_mgr.gesture_row_offset.jump_to(0.0);
                        self.anim_mgr.gesture_row_offset.begin_gesture();
                        self.animate_to_active();
                    }
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                self.gestures.row_active = false;
                self.anim_mgr.gesture_row_offset.end_gesture(0.0, omega, self.config.animation.epsilon);
                self.animate_to_active();
            }
        }
        true
    }

    fn handle_smooth_scrollback(
        &mut self,
        delta: MouseScrollDelta,
        phase: TouchPhase,
        smooth_scroll: bool,
        has_mouse: bool,
        is_alt_screen: bool,
    ) -> bool {
        if !(smooth_scroll && matches!(delta, MouseScrollDelta::PixelDelta(_)) && !has_mouse && !is_alt_screen) {
            return false;
        }

        let MouseScrollDelta::PixelDelta(pos) = delta else {
            unreachable!();
        };
        let py = if self.config.gesture.natural_scroll {
            pos.y
        } else {
            -pos.y
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
        true
    }

    fn handle_discrete_scroll(&mut self, delta: MouseScrollDelta, has_mouse: bool, is_alt_screen: bool) {
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
        if dy == 0 {
            return;
        }

        if has_mouse {
            self.forward_scroll_to_mouse_mode(dy);
        } else if is_alt_screen {
            self.forward_scroll_to_alt_screen(dy);
        } else if dy > 0 {
            self.scroll_active_up(dy as usize);
        } else {
            self.scroll_active_down((-dy) as usize);
        }
    }

    fn forward_scroll_to_mouse_mode(&mut self, dy: i32) {
        let Some(pid) = self.workspaces.active().active_pane_id() else {
            return;
        };
        let Some((_, col, row)) = self
            .last_mouse_pos
            .and_then(|(mx, my)| self.pixel_to_viewport_cell(mx, my))
        else {
            return;
        };

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

    fn forward_scroll_to_alt_screen(&mut self, dy: i32) {
        let Some(pid) = self.workspaces.active().active_pane_id() else {
            return;
        };
        let key = if dy > 0 { b"\x1b[A" } else { b"\x1b[B" };
        let count = dy.unsigned_abs().min(10) as usize;
        for _ in 0..count {
            self.send(ClientMessage::Input {
                pane_id: pid,
                data: key.to_vec(),
            });
        }
    }

    fn handle_horizontal_gesture(&mut self, delta: MouseScrollDelta, phase: TouchPhase) {
        let scroll_mult = self.config.input.scroll_multiplier;
        let dx = match delta {
            MouseScrollDelta::LineDelta(x, _) => x as f64 * scroll_mult,
            MouseScrollDelta::PixelDelta(pos) => pos.x,
        };
        match phase {
            TouchPhase::Started => {
                self.anim_mgr.view_offset_x.begin_gesture();
            }
            TouchPhase::Moved => {
                self.anim_mgr.view_offset_x.update_gesture(dx);
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                let center_strategy = match self.config.layout.center_focused_column {
                    ciri_config::config::CenterStrategy::Always => {
                        ciri_layout::workspace::CenterStrategy::Always
                    }
                    ciri_config::config::CenterStrategy::OnOverflow => {
                        ciri_layout::workspace::CenterStrategy::OnOverflow
                    }
                    ciri_config::config::CenterStrategy::Never => {
                        ciri_layout::workspace::CenterStrategy::Never
                    }
                };
                let current_vox = self.anim_mgr.view_offset_x.value() as f32;
                let t = self
                    .workspaces
                    .active_mut()
                    .target_offset_for_active_with_strategy(center_strategy, current_vox);
                self.anim_mgr.view_offset_x.end_gesture(t as f64, self.config.animation.speed, self.config.animation.epsilon);
            }
        }
    }

    fn request_mouse_redraw(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}
