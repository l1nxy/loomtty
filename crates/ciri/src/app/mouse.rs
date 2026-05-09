use ciri_anim::spring::SpringParams;
use ciri_protocol::message::ClientMessage;
use ciri_protocol::message::{MODE_ALT_SCREEN, MODE_ALTERNATE_SCROLL, MODE_MOUSE_REPORT};
use std::time::Instant;
use winit::dpi::PhysicalPosition;
use winit::event::{MouseButton, MouseScrollDelta, TouchPhase};

use super::App;

impl App {
    // ── Coordinate conversion ──

    /// Convert pixel coordinates to (pane_id, col, buffer_row) using absolute buffer indices.
    pub fn pixel_to_cell(&self, mx: f32, my: f32) -> Option<(u64, u16, usize)> {
        let (cw, ch) = self.cell_dimensions();
        if cw <= 0.0 || ch <= 0.0 {
            return None;
        }
        let my = self.content_y_from_screen(my)?;
        let (vw, _) = self.command_palette_viewport_size();
        let mx = self.content_x_from_screen(mx, vw)?;
        let border_w = self.core.config.appearance.border_width;
        let padding = self.core.config.appearance.padding;
        let vox = self.core.anim_mgr.view_offset_x.value() as f32;
        let tiles = self.core.workspaces.active().visible_tiles(vox);
        for (pane_id, rect, _) in &tiles {
            if rect.contains(mx, my) {
                let inner_x = rect.x + border_w + padding;
                let inner_y = rect.y + border_w + padding;
                let col = ((mx - inner_x) / cw).floor().max(0.0) as u16;
                let viewport_row = ((my - inner_y) / ch).floor().max(0.0) as u16;
                if let Some(grid) = self.core.pane_grids.get(pane_id) {
                    let col = col.min(grid.cols.saturating_sub(1));
                    let viewport_row = viewport_row.min(grid.rows.saturating_sub(1));
                    let buffer_row = grid.viewport_to_buffer_row(viewport_row);
                    return Some((*pane_id, col, buffer_row));
                }
            }
        }
        None
    }

    /// Convert pixel coordinates to (pane_id, col, viewport_row) for mouse forwarding.
    pub fn pixel_to_viewport_cell(&self, mx: f32, my: f32) -> Option<(u64, u16, u16)> {
        let (cw, ch) = self.cell_dimensions();
        if cw <= 0.0 || ch <= 0.0 {
            return None;
        }
        let my = self.content_y_from_screen(my)?;
        let (vw, _) = self.command_palette_viewport_size();
        let mx = self.content_x_from_screen(mx, vw)?;
        let border_w = self.core.config.appearance.border_width;
        let padding = self.core.config.appearance.padding;
        let vox = self.core.anim_mgr.view_offset_x.value() as f32;
        let tiles = self.core.workspaces.active().visible_tiles(vox);
        for (pane_id, rect, _) in &tiles {
            if rect.contains(mx, my) {
                let inner_x = rect.x + border_w + padding;
                let inner_y = rect.y + border_w + padding;
                let col = ((mx - inner_x) / cw).floor().max(0.0) as u16;
                let row = ((my - inner_y) / ch).floor().max(0.0) as u16;
                if let Some(grid) = self.core.pane_grids.get(pane_id) {
                    let col = col.min(grid.cols.saturating_sub(1));
                    let row = row.min(grid.rows.saturating_sub(1));
                    return Some((*pane_id, col, row));
                }
            }
        }
        None
    }

    // ── Selection delegates ──

    pub fn extract_selected_text(&self) -> Option<String> {
        self.core.extract_selected_text()
    }

    pub fn advance_click_count(
        &mut self,
        pane_id: u64,
        col: u16,
        buffer_row: usize,
        now: Instant,
    ) -> u8 {
        self.core.advance_click_count(pane_id, col, buffer_row, now)
    }

    pub fn select_word_at(&mut self, pane_id: u64, col: u16, buffer_row: usize) -> bool {
        self.core.select_word_at(pane_id, col, buffer_row)
    }

    pub fn select_line_at(&mut self, pane_id: u64, buffer_row: usize) {
        self.core.select_line_at(pane_id, buffer_row)
    }

    // ── Scroll helpers ──

    pub fn scroll_active_up(&mut self, lines: usize) {
        if let Some(pid) = self.core.workspaces.active().active_pane_id() {
            self.scroll_pane_up(pid, lines);
        }
    }

    pub fn scroll_active_down(&mut self, lines: usize) {
        if let Some(pid) = self.core.workspaces.active().active_pane_id() {
            self.scroll_pane_down(pid, lines);
        }
    }

    pub fn scroll_active_to_bottom(&mut self) {
        if let Some(pid) = self.core.workspaces.active().active_pane_id()
            && let Some(grid) = self.core.pane_grids.get_mut(&pid)
        {
            grid.scroll_to_bottom();
            self.invalidate_pane_cache(pid);
        }
    }

    pub(crate) fn pane_reports_mouse(&self, pane_id: u64) -> bool {
        let mode_flags = self
            .core
            .pane_grids
            .get(&pane_id)
            .map(|grid| grid.mode_flags)
            .unwrap_or(0);
        mode_reports_mouse(mode_flags)
    }

    pub(crate) fn pane_prefers_mouse_passthrough(&self, pane_id: u64) -> bool {
        self.pane_reports_mouse(pane_id) && !self.modifiers.shift_key()
    }

    pub(crate) fn handle_cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        let mx = position.x as f32;
        let my = position.y as f32;
        self.last_mouse_pos = Some((mx, my));

        if self.core.overview.active && self.core.overview.dragging {
            self.handle_overview_cursor_moved(mx, my);
        } else if self.handle_ui_cursor_hover(mx, my) {
        } else if self.core.overview.active {
            self.handle_overview_cursor_moved(mx, my);
        } else {
            self.handle_main_cursor_moved(mx, my);
        }
    }

    pub(crate) fn handle_mouse_pressed(&mut self, button: MouseButton, mx: f32, my: f32) {
        match button {
            MouseButton::Left => {
                // Capture the chrome `hit_id` under the cursor BEFORE
                // dispatching the click so the next paint can apply
                // any `.active()` refinement targeting that hit_id.
                // Only press-friendly elements (top-bar pane tabs)
                // currently return Some — others dismiss on click and
                // would never have a frame to render the active style.
                self.active_hit_id = self.capture_active_press_hit_id(mx, my);
                if self.dispatch_ui_click(mx, my) {
                    self.schedule_redraw();
                    return;
                }

                self.handle_left_mouse_pressed(mx, my);
            }
            MouseButton::Middle => {
                if self.dispatch_ui_middle_click(mx, my) {
                    self.schedule_redraw();
                }
                // Terminals don't have a meaningful middle-button action
                // outside a tab close, so swallow it entirely when the UI
                // doesn't claim it — avoids accidental paste on platforms
                // that map MMB to clipboard.
            }
            MouseButton::Right => self.handle_right_mouse_pressed(mx, my),
            _ => {}
        }
    }

    pub(crate) fn handle_mouse_released(&mut self, button: MouseButton) {
        if button == MouseButton::Left {
            // Drop the press-state hit_id so any `.active()` refinement
            // releases on the next paint. Done unconditionally — even
            // if the press was outside any active-friendly chrome,
            // `active_hit_id` would already be `None` and clearing is a
            // no-op.
            if self.active_hit_id.take().is_some() {
                self.schedule_redraw();
            }
            self.handle_left_mouse_released()
        }
    }

    fn handle_left_mouse_pressed(&mut self, mx: f32, my: f32) {
        self.mouse_left_passthrough = false;
        if self.core.overview.active {
            self.dispatch_ui_click(mx, my);
        } else {
            let mut started_drag = self.start_column_resize_drag(mx);
            if !started_drag {
                started_drag = self.start_tile_resize_drag(mx, my);
            }

            // Check for scrollbar click/drag
            if !started_drag && let Some(hit) = self.hit_test_scrollbar(mx, my) {
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
                self.schedule_redraw();
            }

            if !started_drag {
                let shift = self.modifiers.shift_key();
                if let Some((pane_id, col, buf_row)) = self.pixel_to_cell(mx, my) {
                    let passthrough = self.pane_prefers_mouse_passthrough(pane_id);
                    if !shift
                        && !passthrough
                        && self.link_activation_modifier_active()
                        && let Some(url) = self.hovered_link_url_at(pane_id, col, buf_row)
                    {
                        self.core.selection = None;
                        self.open_url(&url);
                        self.schedule_redraw();
                        return;
                    }

                    let was_already_focused =
                        self.core.workspaces.active().active_pane_id() == Some(pane_id);
                    self.mouse_left_held = true;
                    self.mouse_left_passthrough = passthrough;
                    let click_now = Instant::now();
                    let click_count = if shift {
                        1 // shift-click is always single-click (extend selection)
                    } else {
                        self.advance_click_count(pane_id, col, buf_row, click_now)
                    };
                    let ws = self.core.workspaces.active_mut();
                    for col_idx in 0..ws.columns.len() {
                        if ws.columns[col_idx].contains_pane(pane_id) {
                            ws.focus_column(col_idx);
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
                    self.remember_workspace_pane(
                        self.core.workspaces.active_workspace_idx,
                        pane_id,
                    );
                    self.send(ClientMessage::FocusPane { pane_id });
                    self.animate_to_active();

                    if passthrough {
                        if was_already_focused {
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
                        }
                        self.core.selection = None;
                        return;
                    }

                    if shift {
                        // Shift-click: extend existing selection or start new
                        self.core.selection = Some(super::Selection {
                            pane_id,
                            start: (col, buf_row),
                            end: (col, buf_row),
                            active: true,
                        });
                    } else if click_count == 3 {
                        // Triple click: select entire line
                        self.select_line_at(pane_id, buf_row);
                    } else if click_count == 2 {
                        // Double click: select word
                        if !self.select_word_at(pane_id, col, buf_row) {
                            self.core.selection = Some(super::Selection {
                                pane_id,
                                start: (col, buf_row),
                                end: (col, buf_row),
                                active: true,
                            });
                        }
                    } else {
                        // Single click: clear selection, set anchor for potential
                        // drag.  The selection is start==end (zero-width) and will
                        // not be rendered until the mouse drags to a different cell.
                        // This matches Alacritty/WezTerm/kitty behavior.
                        //
                        // Only forward the mouse event if the pane was already
                        // focused — clicking to switch focus should not inject a
                        // mouse press into the newly-focused application (which
                        // would cause e.g. neovim to enter visual mode).
                        if was_already_focused {
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
                        }
                        self.core.selection = Some(super::Selection {
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
        let passthrough = self.mouse_left_passthrough;
        self.mouse_left_held = false;
        self.mouse_left_passthrough = false;
        if self.finish_resize_drag() {
            return;
        }
        self.core.overview.dragging = false;
        self.core.overview.drag_last_pos = None;

        if had_left_hold
            && passthrough
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

        if let Some(sel) = &self.core.selection {
            log::debug!(
                "selection release: start={:?} end={:?} active={}",
                sel.start,
                sel.end,
                sel.active
            );
            if sel.active
                && sel.start != sel.end
                && self.core.config.terminal.copy_on_select
                && let Some(text) = self.extract_selected_text()
            {
                log::info!("copy-on-select: {} bytes", text.len());
                if let Some(cb) = &mut self.clipboard {
                    let _ = cb.set_text(&text);
                }
            }
        }
        if let Some(sel) = &mut self.core.selection {
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

        // Settings panel covers the viewport with a full backdrop;
        // forwarding wheel events to terminal / overview / topbar
        // scroll handlers lets the underlying content scroll while
        // the modal sits on top — visually disconcerting and
        // sometimes destructive (e.g. scrolling a pane's history
        // out of view while the user is reading settings). Same
        // gating shape as `handle_focus_follows_mouse` above.
        // Pending paste is also a Base-tier modal with a backdrop;
        // include it for the same reason.
        if self.core.settings_panel_visible || self.core.pending_paste.is_some() {
            return;
        }

        if self.core.overview.active {
            self.handle_overview_wheel(delta);
        } else {
            self.handle_main_wheel(delta, phase);
        }
        self.request_mouse_redraw();
    }

    /// Handle pinch-to-zoom gesture (macOS/trackpad).
    pub(crate) fn handle_pinch_gesture(&mut self, delta: f64, phase: TouchPhase) {
        if !self.core.config.gesture.enabled || !delta.is_finite() {
            return;
        }
        let sensitivity = self.core.config.gesture.pinch_sensitivity;

        match phase {
            TouchPhase::Started => {}
            TouchPhase::Moved => {
                let zoom_delta = delta * sensitivity;
                if self.core.overview.active {
                    // In overview: pinch out (delta > 0) zooms in toward normal
                    let cur_zoom = self.core.anim_mgr.overview_zoom.value();
                    let new_zoom = (cur_zoom + zoom_delta).clamp(0.05, 1.0);
                    let sp = SpringParams::default();
                    if new_zoom >= self.core.config.animation.zoom_threshold as f64 {
                        // Route through `App::exit_overview` (clears
                        // `overview_hovered_pane`, animates zoom +
                        // view offsets via `AppModel::exit_overview`)
                        // — bare field mutation skipped both. Same
                        // fix shape as the R12 `handle_overview_wheel`
                        // exit path.
                        self.exit_overview();
                        let _ = sp;
                    } else {
                        self.core.anim_mgr.overview_zoom.animate_to(new_zoom, sp);
                    }
                } else {
                    // In normal mode: pinch in (delta < 0) enters overview.
                    // Go through `AppModel::toggle_overview` (via the
                    // App-level `toggle_overview` wrapper that also
                    // resets `overview_hovered_pane`) so the close-
                    // others discipline runs — settings panel,
                    // command palette, paste dialog, search bar all
                    // get dismissed the same way the keyboard
                    // `Action::ToggleOverview` path enforces. Inlining
                    // `overview.active = true` here used to leak
                    // those modals into overview mode and lock the
                    // user out of overview keyboard navigation.
                    if zoom_delta < -0.02 && !self.core.overview.active {
                        self.toggle_overview();
                        self.refresh_overview_zoom();
                        let sp = SpringParams::default();
                        self.core.anim_mgr.view_offset_x.animate_to(0.0, sp);
                        self.core.anim_mgr.view_offset_y.animate_to(0.0, sp);
                    }
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                // Snap: if barely zoomed out, snap back to normal.
                // Use `exit_overview` so `overview_hovered_pane` is
                // cleared and the animations are driven by AppModel —
                // bare mutation here used to leak hover state into
                // normal mode after pinch end.
                if self.core.overview.active
                    && self.core.anim_mgr.overview_zoom.value()
                        > self.core.config.animation.zoom_threshold as f64
                {
                    self.exit_overview();
                }
            }
        }
        self.schedule_redraw();
    }

    /// Result of a scrollbar hit-test.
    fn hit_test_scrollbar(&self, mx: f32, my: f32) -> Option<super::resize::ScrollbarHit> {
        let border_w = self.core.config.appearance.border_width;
        let padding = self.core.config.appearance.padding;
        let vox = self.core.anim_mgr.view_offset_x.value() as f32;
        let tiles = self.core.workspaces.active().visible_tiles(vox);
        let my = self.content_y_from_screen(my)?;
        let (vw, _) = self.command_palette_viewport_size();
        let mx = self.content_x_from_screen(mx, vw)?;
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

            let grid = self.core.pane_grids.get(pane_id)?;
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
                scrollbar_rect: Some(*sb),
            });
        }
        None
    }

    /// Scroll a specific pane up (into history) by `lines`.
    fn scroll_pane_up(&mut self, pane_id: u64, lines: usize) {
        if let Some(grid) = self.core.pane_grids.get_mut(&pane_id) {
            grid.scroll_up(lines);
            self.invalidate_pane_cache(pane_id);
        }
    }

    /// Scroll a specific pane down (toward live) by `lines`.
    fn scroll_pane_down(&mut self, pane_id: u64, lines: usize) {
        if let Some(grid) = self.core.pane_grids.get_mut(&pane_id) {
            grid.scroll_down(lines);
            self.invalidate_pane_cache(pane_id);
        }
    }

    fn handle_ui_cursor_hover(&mut self, mx: f32, my: f32) -> bool {
        let ui_hover = self.dispatch_ui_hover(mx, my);
        if let Some(w) = &self.window {
            w.set_cursor(ui_hover.cursor);
        }
        if ui_hover.needs_redraw {
            self.schedule_redraw();
        }
        ui_hover.handled
    }

    fn handle_overview_cursor_moved(&mut self, mx: f32, my: f32) {
        let hover_changed = self.clear_hovered_link();
        if hover_changed {
            self.request_mouse_redraw();
        }

        if !self.core.overview.dragging {
            return;
        }

        if let Some((lx, ly)) = self.core.overview.drag_last_pos {
            let zoom = self.core.anim_mgr.overview_zoom.value() as f32;
            let dx = (mx - lx) / zoom;
            let dy = (my - ly) / zoom;
            self.core
                .anim_mgr
                .view_offset_x
                .jump_to(self.core.anim_mgr.view_offset_x.value() - dx as f64);
            self.core
                .anim_mgr
                .view_offset_y
                .jump_to(self.core.anim_mgr.view_offset_y.value() - dy as f64);
            self.request_mouse_redraw();
        }
        self.core.overview.drag_last_pos = Some((mx, my));
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
            } else if self.core.hovered_link.is_some() {
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
        if !self.core.config.input.focus_follows_mouse
            || self.mouse_left_held
            || self.core.search_state.is_some()
            || self.core.command_palette.is_some()
            || self.core.context_menu.visible
            // Settings panel covers the whole viewport with a backdrop;
            // moving the cursor over a "different pane" while the
            // panel is up shouldn't silently switch focus, otherwise
            // closing the panel lands the user on a pane they didn't
            // intend. Same shape as the other modal gates in this
            // chain.
            || self.core.settings_panel_visible
            || self.core.pending_paste.is_some()
        {
            return;
        }

        let hover_pane = self.hovered_pane_at(mx, my);
        let Some(pane_id) = hover_pane else {
            self.last_focus_follows_mouse = None;
            return;
        };

        if self.core.workspaces.active().active_pane_id() == Some(pane_id) {
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
        self.remember_workspace_pane(self.core.workspaces.active_workspace_idx, pane_id);
        self.send_lossy(ClientMessage::FocusPane { pane_id });
    }

    fn hovered_pane_at(&self, mx: f32, my: f32) -> Option<u64> {
        let my = self.content_y_from_screen(my)?;
        let (vw, _) = self.command_palette_viewport_size();
        let mx = self.content_x_from_screen(mx, vw)?;
        let vox = self.core.anim_mgr.view_offset_x.value() as f32;
        self.core
            .workspaces
            .active()
            .visible_tiles(vox)
            .into_iter()
            .find_map(|(pane_id, rect, _)| rect.contains(mx, my).then_some(pane_id))
    }

    fn handle_mouse_drag_selection(&mut self, mx: f32, my: f32) {
        if !self.mouse_left_held {
            return;
        }

        // Suppress selection extension while the viewport is animating (e.g. after
        // clicking a pane in a different column).  The sliding viewport changes the
        // pixel→cell mapping, which would otherwise look like a drag selection.
        if self.core.anim_mgr.view_offset_x.is_animating() {
            return;
        }

        if self.mouse_left_passthrough
            && let Some((pane_id, col, row)) = self.pixel_to_viewport_cell(mx, my)
        {
            if self.core.selection.take().is_some() {
                self.request_mouse_redraw();
            }
            self.send_lossy(ClientMessage::MouseInput {
                pane_id,
                button: 32,
                col,
                row,
                pressed: true,
                modifiers: 0,
            });
            return;
        }

        if self.core.selection.as_ref().is_some_and(|s| s.active)
            && let Some((_, col, buf_row)) = self.pixel_to_cell(mx, my)
        {
            if let Some(sel) = &mut self.core.selection {
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
        if !self.core.context_menu.visible {
            return false;
        }
        self.core.context_menu.visible = false;
        self.request_mouse_redraw();
        true
    }

    fn handle_palette_wheel(&mut self, delta: MouseScrollDelta) -> bool {
        let Some(palette) = &mut self.core.command_palette else {
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
            palette.move_selection(steps as i32, false);
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
            MouseScrollDelta::LineDelta(_, y) => -y * self.cell_dimensions().0 * 3.0,
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
        let cur_zoom = self.core.anim_mgr.overview_zoom.value();
        let new_zoom = (cur_zoom + dy).clamp(0.05, 1.0);
        let sp = SpringParams::default();
        if new_zoom >= self.core.config.animation.zoom_threshold as f64 {
            // Use the App-level wrapper that clears
            // `overview_hovered_pane` — bare `overview.active = false`
            // leaves a stale hover hit_id pointing at a pane the user
            // selected in overview mode, which renders a phantom hover
            // highlight on the wrong pane after exit. (Same fix shape
            // as the R11 pinch-gesture fix.) `AppModel::exit_overview`
            // animates `overview_zoom` and `view_offset_*` itself, so
            // we don't need the inline animations.
            self.exit_overview();
            let _ = sp;
        } else {
            self.core.anim_mgr.overview_zoom.animate_to(new_zoom, sp);
        }
    }

    fn handle_main_wheel(&mut self, delta: MouseScrollDelta, phase: TouchPhase) {
        let gestures_enabled = self.core.config.gesture.enabled;
        let smooth_scroll = gestures_enabled && self.core.config.gesture.smooth_scroll;
        // Route the wheel to the pane currently under the cursor, even if it's
        // not the active pane — feels natural and doesn't steal focus.
        let target_pid = self
            .last_mouse_pos
            .and_then(|(mx, my)| self.hovered_pane_at(mx, my))
            .or_else(|| self.core.workspaces.active().active_pane_id());
        let target_grid = target_pid.and_then(|pid| self.core.pane_grids.get(&pid));
        let has_mouse = target_grid.is_some_and(|g| g.mode_flags & MODE_MOUSE_REPORT != 0);
        let is_alt_screen = target_grid.is_some_and(|g| g.mode_flags & MODE_ALT_SCREEN != 0);
        let has_alternate_scroll =
            target_grid.is_some_and(|g| g.mode_flags & MODE_ALTERNATE_SCROLL != 0);

        if self.handle_workspace_row_swipe(delta, phase, gestures_enabled) {
            return;
        }
        if self.handle_smooth_scrollback(
            delta,
            phase,
            smooth_scroll,
            has_mouse,
            is_alt_screen,
            target_pid,
        ) {
            self.handle_horizontal_gesture(delta, phase);
            return;
        }

        self.handle_discrete_scroll(
            delta,
            has_mouse,
            is_alt_screen,
            has_alternate_scroll,
            target_pid,
        );
        self.handle_horizontal_gesture(delta, phase);
    }

    fn handle_workspace_row_swipe(
        &mut self,
        delta: MouseScrollDelta,
        phase: TouchPhase,
        gestures_enabled: bool,
    ) -> bool {
        let shift_held = self.modifiers.shift_key();
        let multi_row = self.core.workspaces.workspaces.len() > 1;
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
        let threshold = self.core.config.gesture.vertical_swipe_threshold;

        match phase {
            TouchPhase::Started => {
                self.gestures.row_active = true;
                self.gestures.row_start = self.core.workspaces.active_workspace_idx;
                self.core.anim_mgr.gesture_row_offset.begin_gesture();
            }
            TouchPhase::Moved => {
                if self.gestures.row_active {
                    self.core
                        .anim_mgr
                        .gesture_row_offset
                        .update_gesture_unclamped(py);
                    let accum = self.core.anim_mgr.gesture_row_offset.value();
                    if accum > threshold {
                        self.core.workspaces.focus_down();
                        self.core.anim_mgr.gesture_row_offset.jump_to(0.0);
                        self.core.anim_mgr.gesture_row_offset.begin_gesture();
                        self.animate_to_active();
                    } else if accum < -threshold {
                        self.core.workspaces.focus_up();
                        self.core.anim_mgr.gesture_row_offset.jump_to(0.0);
                        self.core.anim_mgr.gesture_row_offset.begin_gesture();
                        self.animate_to_active();
                    }
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                self.gestures.row_active = false;
                let sp = SpringParams::default();
                self.core.anim_mgr.gesture_row_offset.end_gesture(0.0, sp);
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
        target_pid: Option<u64>,
    ) -> bool {
        if !smooth_scroll
            || !matches!(delta, MouseScrollDelta::PixelDelta(_))
            || has_mouse
            || is_alt_screen
        {
            return false;
        }

        let MouseScrollDelta::PixelDelta(pos) = delta else {
            unreachable!();
        };
        let py = if self.core.config.gesture.natural_scroll {
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
                let ppl = self.core.config.gesture.scroll_pixels_per_line;
                let lines = (self.gestures.scroll_accum / ppl) as i64;
                if lines != 0
                    && let Some(pid) = target_pid
                {
                    self.gestures.scroll_accum -= lines as f64 * ppl;
                    if lines > 0 {
                        self.scroll_pane_up(pid, lines as usize);
                    } else {
                        self.scroll_pane_down(pid, (-lines) as usize);
                    }
                }
            }
        }
        true
    }

    fn handle_discrete_scroll(
        &mut self,
        delta: MouseScrollDelta,
        has_mouse: bool,
        is_alt_screen: bool,
        has_alternate_scroll: bool,
        target_pid: Option<u64>,
    ) {
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
        let Some(pid) = target_pid else {
            return;
        };

        if has_mouse {
            self.forward_scroll_to_mouse_mode(dy, pid);
        } else if is_alt_screen && has_alternate_scroll {
            self.forward_scroll_to_alt_screen(dy, pid);
        } else if dy > 0 {
            self.scroll_pane_up(pid, dy as usize);
        } else {
            self.scroll_pane_down(pid, (-dy) as usize);
        }
    }

    fn forward_scroll_to_mouse_mode(&mut self, dy: i32, pid: u64) {
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

    fn forward_scroll_to_alt_screen(&mut self, dy: i32, pid: u64) {
        let app_cursor = self
            .core
            .pane_grids
            .get(&pid)
            .is_some_and(|g| g.mode_flags & ciri_protocol::message::MODE_APP_CURSOR != 0);
        let key: &[u8] = match (dy > 0, app_cursor) {
            (true, false) => b"\x1b[A",
            (true, true) => b"\x1bOA",
            (false, false) => b"\x1b[B",
            (false, true) => b"\x1bOB",
        };
        let count = dy.unsigned_abs().min(10) as usize;
        for _ in 0..count {
            self.send(ClientMessage::Input {
                pane_id: pid,
                data: key.to_vec(),
                input_seq: 0,
            });
        }
    }

    fn handle_horizontal_gesture(&mut self, delta: MouseScrollDelta, phase: TouchPhase) {
        let scroll_mult = self.core.config.input.scroll_multiplier;
        let dx = match delta {
            MouseScrollDelta::LineDelta(x, _) => x as f64 * scroll_mult,
            MouseScrollDelta::PixelDelta(pos) => pos.x,
        };
        match phase {
            TouchPhase::Started => {
                self.core.anim_mgr.view_offset_x.begin_gesture();
            }
            TouchPhase::Moved => {
                self.core.anim_mgr.view_offset_x.update_gesture(dx);
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                let center_strategy = match self.core.config.layout.center_focused_column {
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
                let current_vox = self.core.anim_mgr.view_offset_x.value() as f32;
                let t = self
                    .core
                    .workspaces
                    .active_mut()
                    .target_offset_for_active_with_strategy(center_strategy, current_vox);
                let sp = SpringParams::default();
                self.core.anim_mgr.view_offset_x.end_gesture(t as f64, sp);
            }
        }
    }

    fn request_mouse_redraw(&mut self) {
        self.schedule_redraw();
    }
}

fn mode_reports_mouse(mode_flags: u16) -> bool {
    mode_flags & MODE_MOUSE_REPORT != 0
}

#[cfg(test)]
mod tests {
    use super::{App, mode_reports_mouse};
    use ciri_config::config::{CiriConfig, StatusBarPosition, TabBarPosition};
    use ciri_layout::column::ColumnWidth;
    use ciri_protocol::message::{MODE_ALT_SCREEN, MODE_MOUSE_REPORT};
    use winit::dpi::PhysicalSize;

    fn make_hover_app(tab_position: TabBarPosition) -> App {
        let mut config = CiriConfig::default();
        config.window.width = 900.0;
        config.window.height = 700.0;
        config.statusbar.position = StatusBarPosition::Top;
        config.tabbar.position = tab_position;
        config.tabbar.width = 96.0;

        let mut app = App::new(config, "test-session");
        app.preview_resize(PhysicalSize::new(900, 700));
        app.core
            .workspaces
            .active_mut()
            .add_column_right(1, ColumnWidth::Proportion(1.0));
        app
    }

    #[test]
    fn mouse_reporting_is_not_implied_by_alt_screen() {
        assert!(!mode_reports_mouse(0));
        assert!(mode_reports_mouse(MODE_MOUSE_REPORT));
        assert!(!mode_reports_mouse(MODE_ALT_SCREEN));
        assert!(mode_reports_mouse(MODE_MOUSE_REPORT | MODE_ALT_SCREEN));
    }

    #[test]
    fn hovered_pane_at_converts_left_tab_bar_and_top_bar_coords() {
        let app = make_hover_app(TabBarPosition::Left);

        assert_eq!(
            app.hovered_pane_at(app.content_origin_x() + 12.0, app.content_origin_y() + 12.0),
            Some(1)
        );
        assert_eq!(
            app.hovered_pane_at(12.0, app.content_origin_y() + 12.0),
            None
        );
        assert_eq!(
            app.hovered_pane_at(app.content_origin_x() + 12.0, 12.0),
            None
        );
    }

    #[test]
    fn hovered_pane_at_ignores_right_tab_bar_strip() {
        let app = make_hover_app(TabBarPosition::Right);
        let window_width = app.command_palette_viewport_size().0;

        assert_eq!(
            app.hovered_pane_at(12.0, app.content_origin_y() + 12.0),
            Some(1)
        );
        assert_eq!(
            app.hovered_pane_at(
                window_width - app.core.config.tabbar.width / 2.0,
                app.content_origin_y() + 12.0
            ),
            None
        );
    }
}
