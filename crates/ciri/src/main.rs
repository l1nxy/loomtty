mod app;
mod cli;
mod connection;
mod grid;

use anyhow::Result;
use app::App;
use app::input_handler::key_event_to_pty_bytes;
use ciri_config::config::CiriConfig;
use ciri_input::action::Action;
use ciri_input::keybind::KeyCombo;
use ciri_layout::column::ColumnWidth;
use ciri_layout::geometry::ViewSize;
use ciri_protocol::message::*;
use ciri_render::glyph_cache::GlyphAtlas;
use cli::CliCommand;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, Ime, MouseScrollDelta, StartCause, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{WindowAttributes, WindowId};

// ─── ApplicationHandler (thin dispatcher) ──────────────────────────────

impl ApplicationHandler for App {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if self.should_exit {
            event_loop.exit();
            return;
        }
        // Idle-aware event loop: only poll at frame rate when animating or
        // expecting updates. Switch to Wait when idle to save power.
        let is_animating = self.view_offset_x.is_animating()
            || self.view_offset_y.is_animating()
            || self.overview_zoom.is_animating()
            || self.col_widths.iter().any(|v| v.is_animating());
        let has_server = self.server_rx.is_some();
        let is_reconnecting = self.reconnect_state.is_some();
        let wants_blink = self.config.terminal.cursor_blink;

        if is_animating || has_server || is_reconnecting || wants_blink {
            event_loop
                .set_control_flow(ControlFlow::WaitUntil(Instant::now() + self.frame_interval));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }

        if matches!(
            cause,
            StartCause::ResumeTimeReached { .. } | StartCause::Poll
        ) {
            let mut needs_redraw = is_animating;

            if self.process_server_events() {
                needs_redraw = true;
            }

            // Cursor blink
            if self.config.terminal.cursor_blink {
                let interval = Duration::from_millis(self.config.terminal.cursor_blink_interval_ms);
                if self.cursor_blink_timer.elapsed() >= interval {
                    self.cursor_blink_visible = !self.cursor_blink_visible;
                    self.cursor_blink_timer = Instant::now();
                    needs_redraw = true;
                }
            }

            // Config hot-reload
            if let Some(rx) = &self.config_change_rx {
                if rx.try_recv().is_ok() {
                    self.reload_config();
                    needs_redraw = true;
                }
            }

            // Auto-reconnect
            if !self.connected && self.server_rx.is_none() {
                let should_try = self
                    .reconnect_state
                    .as_ref()
                    .is_some_and(|s| Instant::now() >= s.next_retry);
                let gave_up = self
                    .reconnect_state
                    .as_ref()
                    .is_some_and(|s| s.attempt >= s.max_attempts);
                let has_reconnect = self.reconnect_state.is_some();

                if gave_up {
                    log::error!("max reconnect attempts reached, exiting");
                    event_loop.exit();
                    return;
                } else if should_try {
                    let (cw, ch) = self.cell_dimensions();
                    let view = &self.workspaces.view_size;
                    let viewport = ciri_protocol::codec::ClientViewport {
                        width: view.width as u32,
                        height: view.height as u32,
                        cell_width: cw,
                        cell_height: ch,
                    };
                    if let Some(state) = &mut self.reconnect_state {
                        state.attempt += 1;
                    }
                    match connection::connect_or_spawn(&self.session_name, viewport) {
                        Ok((tx, rx)) => {
                            log::info!("reconnected to session '{}'", self.session_name);
                            self.server_tx = Some(tx);
                            self.server_rx = Some(rx);
                            self.reconnect_state = None;
                        }
                        Err(e) => {
                            log::warn!("reconnect failed: {e}");
                            if let Some(state) = &mut self.reconnect_state {
                                state.backoff = (state.backoff * 2).min(Duration::from_secs(10));
                                state.next_retry = Instant::now() + state.backoff;
                            }
                        }
                    }
                    needs_redraw = true;
                } else if has_reconnect {
                    needs_redraw = true;
                } else {
                    log::info!("server connection lost, exiting");
                    self.cached_views.clear();
                    self.glyph_atlas = None;
                    self.renderer = None;
                    self.window = None;
                    event_loop.exit();
                    return;
                }
            }

            // Exit if all panes gone
            if self.connected && self.pane_grids.is_empty() && self.workspaces.active().is_empty() {
                self.cached_views.clear();
                self.glyph_atlas = None;
                self.renderer = None;
                self.window = None;
                event_loop.exit();
                return;
            }

            if needs_redraw && let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let window_title = format!("{} [{}]", self.config.window.title, self.session_name);
        let attrs = WindowAttributes::default()
            .with_title(window_title)
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.config.window.width,
                self.config.window.height,
            ));

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );
        window.set_ime_allowed(true);
        let dpi_scale = window.scale_factor();
        let mut renderer = pollster::block_on(ciri_render::renderer::Renderer::new(
            window.clone(),
            &self.config.render,
        ))
        .expect("renderer init failed");

        let fmt = renderer.surface_format();
        let atlas = GlyphAtlas::new(
            &renderer.device,
            fmt,
            &mut renderer.text.font_system,
            self.config.font.size,
            dpi_scale,
            &self.config.font.family,
            &self.config.render,
        );

        let (w, h) = renderer.surface_size();
        let bar_h = atlas.cell_height + self.config.statusbar.height_padding;
        self.workspaces.resize_view(ViewSize {
            width: w as f32,
            height: h as f32 - bar_h,
        });

        log::info!(
            "cell: {:.1}x{:.1} ascent={:.1} (dpi_scale={:.2})",
            atlas.cell_width,
            atlas.cell_height,
            atlas.ascent,
            dpi_scale
        );

        let (cw, ch) = self.cell_dimensions();
        let view = &self.workspaces.view_size;
        let viewport = ciri_protocol::codec::ClientViewport {
            width: view.width as u32,
            height: view.height as u32,
            cell_width: cw,
            cell_height: ch,
        };
        log::info!("connecting to session '{}'", self.session_name);
        match connection::connect_or_spawn(&self.session_name, viewport) {
            Ok((tx, rx)) => {
                self.server_tx = Some(tx);
                self.server_rx = Some(rx);
            }
            Err(e) => {
                log::error!("failed to connect to server: {e}");
            }
        }

        // Config hot-reload watcher
        {
            let (ctx, crx) = crossbeam_channel::bounded(1);
            let watcher = notify::recommended_watcher(move |res: Result<notify::Event, _>| {
                if let Ok(evt) = res {
                    if evt.kind.is_modify() {
                        let _ = ctx.try_send(());
                    }
                }
            })
            .ok();
            if let Some(mut w) = watcher {
                use notify::Watcher;
                let path = ciri_config::config::config_path();
                if let Err(e) = w.watch(&path, notify::RecursiveMode::NonRecursive) {
                    log::warn!("failed to watch config: {e}");
                } else {
                    self.config_watcher = Some(w);
                    self.config_change_rx = Some(crx);
                    log::info!("config watcher active: {}", path.display());
                }
            }
        }

        self.dpi_scale = dpi_scale;
        self.glyph_atlas = Some(atlas);
        self.snap_all_col_widths();
        self.animate_to_active();
        self.window = Some(window);
        self.renderer = Some(renderer);
        self.last_frame = Instant::now();

        self.window.as_ref().unwrap().request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.send(ClientMessage::Detach);
                self.pane_grids.clear();
                self.cached_views.clear();
                self.glyph_atlas = None;
                self.renderer = None;
                self.window = None;
                event_loop.exit();
            }

            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
            }

            WindowEvent::Resized(size) => {
                if size.width == 0 || size.height == 0 {
                    return;
                }
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
                let bar_h = self.status_bar_height();
                self.workspaces.resize_view(ViewSize {
                    width: size.width as f32,
                    height: size.height as f32 - bar_h,
                });
                self.snap_all_col_widths();
                for grid in self.pane_grids.values_mut() {
                    grid.dirty = true;
                }
                self.cached_views.clear();
                let t = self.workspaces.active_mut().target_offset_for_active();
                self.view_offset_x.jump_to(t as f64);
                for ws in &mut self.workspaces.rows {
                    ws.view_offset_x = t;
                }
                let (cols, rows) = self.compute_grid_size();
                let (cw, ch) = self.cell_dimensions();
                let view = &self.workspaces.view_size;
                self.send(ClientMessage::Resize {
                    cols,
                    rows,
                    width: view.width as u32,
                    height: view.height as u32,
                    cell_width: cw,
                    cell_height: ch,
                });
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                if self.ime_preedit_active {
                    return;
                }

                self.cursor_blink_visible = true;
                self.cursor_blink_timer = Instant::now();

                let ctrl = self.modifiers.control_key();
                let shift = self.modifiers.shift_key();
                let alt = self.modifiers.alt_key();
                let super_key = self.modifiers.super_key();

                log::debug!(
                    "key: ctrl={ctrl} shift={shift} alt={alt} super={super_key} logical={:?} physical={:?}",
                    event.logical_key,
                    event.physical_key
                );

                // Clipboard: Ctrl+Shift+V / Ctrl+Shift+C
                if ctrl && shift {
                    use winit::keyboard::{KeyCode, PhysicalKey};
                    log::info!("ctrl+shift detected, physical={:?}", event.physical_key);
                    match event.physical_key {
                        PhysicalKey::Code(KeyCode::KeyV) => {
                            log::info!("clipboard paste triggered");
                            match &mut self.clipboard {
                                None => log::warn!("clipboard not available"),
                                Some(cb) => match cb.get_text() {
                                    Err(e) => log::warn!("clipboard read failed: {e}"),
                                    Ok(text) => {
                                        log::info!("clipboard text: {} bytes", text.len());
                                        if let Some(pid) =
                                            self.workspaces.active_mut().active_pane_id()
                                        {
                                            self.send(ClientMessage::Input {
                                                pane_id: pid,
                                                data: text.into_bytes(),
                                            });
                                        }
                                    }
                                },
                            }
                            if let Some(w) = &self.window {
                                w.request_redraw();
                            }
                            return;
                        }
                        PhysicalKey::Code(KeyCode::KeyC) => {
                            log::info!(
                                "clipboard copy triggered, selection={}",
                                self.selection.is_some()
                            );
                            if let Some(text) = self.extract_selected_text() {
                                log::info!("copying {} bytes", text.len());
                                if let Some(cb) = &mut self.clipboard {
                                    let _ = cb.set_text(&text);
                                }
                            }
                            if let Some(w) = &self.window {
                                w.request_redraw();
                            }
                            return;
                        }
                        _ => {}
                    }
                }

                let is_modifier_only = matches!(
                    event.logical_key,
                    Key::Named(
                        NamedKey::Control | NamedKey::Shift | NamedKey::Alt | NamedKey::Super
                    )
                );
                if !is_modifier_only {
                    self.selection = None;
                }

                let key_name = match &event.logical_key {
                    Key::Named(n) => match n {
                        NamedKey::Space => "space",
                        NamedKey::Tab => "tab",
                        NamedKey::Escape => "escape",
                        NamedKey::Enter => "enter",
                        NamedKey::ArrowLeft => "Left",
                        NamedKey::ArrowRight => "Right",
                        NamedKey::ArrowUp => "Up",
                        NamedKey::ArrowDown => "Down",
                        _ => "",
                    },
                    Key::Character(c) => c.as_str(),
                    _ => "",
                };

                if self.overview_active {
                    let combo = KeyCombo::from_modifiers(
                        &key_name.to_lowercase(),
                        ctrl,
                        shift,
                        alt,
                        super_key,
                    );
                    if let Some(action) = self.overview_keybinds.lookup(&combo) {
                        match action {
                            Action::ExitOverview => {
                                self.overview_active = false;
                                self.overview_zoom
                                    .animate_to(1.0, self.config.animation.speed);
                                self.animate_to_active();
                            }
                            other => self.handle_action(other),
                        }
                    }
                } else {
                    use ciri_input::leader::InputResult;
                    let result = if !key_name.is_empty() {
                        self.input
                            .process_key(key_name, ctrl, shift, alt, super_key)
                    } else {
                        InputResult::PassThrough
                    };

                    match result {
                        InputResult::Action(action) => self.handle_action(action),
                        InputResult::Consumed => {}
                        InputResult::PassThrough => {
                            self.scroll_active_to_bottom();
                            let bytes = key_event_to_pty_bytes(&event, ctrl);
                            if !bytes.is_empty()
                                && let Some(pid) = self.workspaces.active_mut().active_pane_id()
                            {
                                self.send(ClientMessage::Input {
                                    pane_id: pid,
                                    data: bytes,
                                });
                            }
                        }
                    }
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                let mx = position.x as f32;
                let my = position.y as f32;
                self.last_mouse_pos = Some((mx, my));

                if self.overview_active {
                    if let Some((row_idx, pane_id)) = self.hit_test_overview(mx, my) {
                        if row_idx < self.workspaces.rows.len() {
                            self.workspaces.active_row = row_idx;
                            let ws = self.workspaces.active_mut();
                            for (col_idx, col) in ws.columns.iter().enumerate() {
                                if col.pane_id == pane_id {
                                    ws.active_column_idx = col_idx;
                                    break;
                                }
                            }
                        }
                        if let Some(w) = &self.window {
                            w.request_redraw();
                        }
                    }

                    if self.overview_dragging {
                        if let Some((lx, ly)) = self.drag_last_pos {
                            let zoom = self.overview_zoom.value() as f32;
                            let dx = (mx - lx) / zoom;
                            let dy = (my - ly) / zoom;
                            let cur_x = self.view_offset_x.value();
                            self.view_offset_x.jump_to(cur_x - dx as f64);
                            let vox = self.view_offset_x.value() as f32;
                            for ws in &mut self.workspaces.rows {
                                ws.view_offset_x = vox;
                            }
                            let cur_y = self.view_offset_y.value();
                            self.view_offset_y.jump_to(cur_y - dy as f64);
                            self.workspaces.view_offset_y = self.view_offset_y.value() as f32;
                            if let Some(w) = &self.window {
                                w.request_redraw();
                            }
                        }
                        self.drag_last_pos = Some((mx, my));
                    }
                } else {
                    if let Some(drag_col) = self.resize_dragging {
                        let delta = mx - self.resize_drag_start_x;
                        let new_width = (self.resize_drag_start_width + delta).max(50.0);
                        let vw = self.workspaces.active().view_size.width;
                        let proportion = (new_width as f64 / vw as f64).clamp(0.1, 0.9);
                        self.workspaces.active_mut().set_column_width_by_index(
                            drag_col,
                            ColumnWidth::Proportion(proportion),
                        );
                        self.snap_all_col_widths();
                        if let Some(w) = &self.window {
                            w.request_redraw();
                        }
                    } else {
                        let ws = self.workspaces.active();
                        let vox = ws.view_offset_x;
                        let mut near_border = false;
                        for i in 1..ws.columns.len() {
                            let col_x = ws.column_x(i) - vox;
                            if (mx - col_x).abs() < 4.0 {
                                near_border = true;
                                break;
                            }
                        }
                        if let Some(w) = &self.window {
                            if near_border {
                                w.set_cursor(winit::window::CursorIcon::ColResize);
                            } else {
                                w.set_cursor(winit::window::CursorIcon::Default);
                            }
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

            WindowEvent::MouseInput {
                state,
                button: winit::event::MouseButton::Left,
                ..
            } => {
                if let Some((mx, my)) = self.last_mouse_pos {
                    if state == ElementState::Pressed {
                        if self.overview_active {
                            if let Some((row_idx, pane_id)) = self.hit_test_overview(mx, my) {
                                if row_idx < self.workspaces.rows.len() {
                                    self.workspaces.active_row = row_idx;
                                    let ws = self.workspaces.active_mut();
                                    for (col_idx, col) in ws.columns.iter().enumerate() {
                                        if col.pane_id == pane_id {
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
                            let vox = ws.view_offset_x;
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

                            if !started_drag {
                                self.mouse_left_held = true;
                                let shift = self.modifiers.shift_key();
                                if let Some((pane_id, col, buf_row)) = self.pixel_to_cell(mx, my) {
                                    let click_now = Instant::now();
                                    let is_double_click = !shift
                                        && self.is_double_left_click(pane_id, col, buf_row, click_now);
                                    let ws = self.workspaces.active_mut();
                                    for col_idx in 0..ws.columns.len() {
                                        if ws.columns[col_idx].pane_id == pane_id {
                                            ws.active_column_idx = col_idx;
                                            break;
                                        }
                                    }
                                    self.animate_to_active();
                                    self.remember_left_click(pane_id, col, buf_row, click_now);

                                    if shift {
                                        self.selection = Some(app::Selection {
                                            pane_id,
                                            start: (col, buf_row),
                                            end: (col, buf_row),
                                            active: true,
                                        });
                                    } else if is_double_click {
                                        if !self.select_word_at(pane_id, col, buf_row) {
                                            self.selection = Some(app::Selection {
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
                                        self.selection = Some(app::Selection {
                                            pane_id,
                                            start: (col, buf_row),
                                            end: (col, buf_row),
                                            active: true,
                                        });
                                    }
                                }
                            }
                        }
                    } else {
                        self.mouse_left_held = false;
                        if self.resize_dragging.is_some() {
                            self.resize_dragging = None;
                            self.snap_all_col_widths();
                            if let Some(w) = &self.window {
                                w.set_cursor(winit::window::CursorIcon::Default);
                            }
                        }
                        self.overview_dragging = false;
                        self.drag_last_pos = None;

                        if let Some((pane_id, col, row)) = self.pixel_to_viewport_cell(mx, my) {
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
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
            }

            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: winit::event::MouseButton::Right,
                ..
            } => {
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

            WindowEvent::MouseWheel { delta, phase, .. } => {
                if self.overview_active {
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
                        let has_mouse = self
                            .workspaces
                            .active()
                            .active_pane_id()
                            .and_then(|pid| self.pane_grids.get(&pid))
                            .is_some_and(|g| {
                                g.mode_flags & ciri_protocol::message::MODE_MOUSE_REPORT != 0
                            });

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
                        } else {
                            if dy > 0 {
                                self.scroll_active_up(dy as usize);
                            } else {
                                self.scroll_active_down((-dy) as usize);
                            }
                        }
                    }

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
                            let val = self.view_offset_x.value() as f32;
                            for ws in &mut self.workspaces.rows {
                                ws.view_offset_x = val;
                            }
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

            WindowEvent::Ime(ime) => match ime {
                Ime::Commit(text) => {
                    self.ime_preedit_active = false;
                    if let Some(pid) = self.workspaces.active_mut().active_pane_id() {
                        self.send(ClientMessage::Input {
                            pane_id: pid,
                            data: text.into_bytes(),
                        });
                    }
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
                Ime::Preedit(text, _cursor) => {
                    self.ime_preedit_active = !text.is_empty();
                }
                Ime::Enabled | Ime::Disabled => {}
            },

            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if (scale_factor - self.dpi_scale).abs() > 0.01 {
                    self.dpi_scale = scale_factor;
                    if let Some(renderer) = &mut self.renderer {
                        let fmt = renderer.surface_format();
                        let atlas = GlyphAtlas::new(
                            &renderer.device,
                            fmt,
                            &mut renderer.text.font_system,
                            self.config.font.size,
                            scale_factor,
                            &self.config.font.family,
                            &self.config.render,
                        );
                        log::info!(
                            "DPI changed: scale={:.2} cell={:.1}x{:.1}",
                            scale_factor,
                            atlas.cell_width,
                            atlas.cell_height
                        );
                        let bar_h = atlas.cell_height + self.config.statusbar.height_padding;
                        let (w, h) = renderer.surface_size();
                        self.workspaces.resize_view(ViewSize {
                            width: w as f32,
                            height: h as f32 - bar_h,
                        });
                        self.glyph_atlas = Some(atlas);
                        self.cached_views.clear();
                    }
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
            }

            WindowEvent::RedrawRequested => self.render(),

            _ => {}
        }
    }
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,wgpu_hal=warn,wgpu_core=warn,naga=warn"),
    )
    .init();

    let cli = match cli::parse_args(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(usage) => {
            eprintln!("{usage}");
            std::process::exit(2);
        }
    };
    if matches!(cli, CliCommand::Help) {
        println!("{}", cli::usage());
        return Ok(());
    }

    let config = CiriConfig::load().unwrap_or_default();
    log::info!(
        "config: font={} size={}",
        config.font.family,
        config.font.size
    );

    let event_loop = EventLoop::new()?;
    let session_name = match cli {
        CliCommand::Run { session_name } => session_name,
        CliCommand::Help => unreachable!("help exits before app startup"),
    };
    let mut app = App::new(config, session_name);
    event_loop.run_app(&mut app)?;
    Ok(())
}
