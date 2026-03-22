use ciri_layout::geometry::ViewSize;
use ciri_render::glyph_cache::GlyphAtlas;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{WindowAttributes, WindowId};

use super::App;
use crate::connection;

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
            || self.gesture_row_offset.is_animating()
            || self.col_widths.iter().any(|v| v.is_animating())
            || !self.pane_open_opacity.is_empty()
            || !self.closing_panes.is_empty();
        let has_server = self.server_rx.is_some();
        let is_reconnecting = self.reconnect_state.is_some();
        let wants_blink = self.config.terminal.cursor_blink;

        let has_pending = self.server_rx.as_ref().is_some_and(|rx| !rx.is_empty());

        if is_animating || has_pending || is_reconnecting {
            // Active rendering or pending data: poll at frame rate
            event_loop
                .set_control_flow(ControlFlow::WaitUntil(Instant::now() + self.frame_interval));
        } else if wants_blink || has_server {
            // Connected but idle: poll at reduced rate (50ms = 20fps idle)
            event_loop
                .set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(50)));
        } else {
            // Disconnected, no animations: fully idle
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
                    let viewport = ciri_protocol::codec::ClientHello {
                        session_name: self.session_name.clone(),
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
        let viewport = ciri_protocol::codec::ClientHello {
            session_name: self.session_name.clone(),
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
        use ciri_protocol::message::ClientMessage;

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
                log::debug!("window resized: {}x{}", size.width, size.height);
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
                let bar_h = self.status_bar_height();
                self.workspaces.resize_view(ViewSize {
                    width: size.width as f32,
                    height: size.height as f32 - bar_h,
                });
                log::debug!("  view_size: {}x{}", size.width as f32, size.height as f32 - bar_h);
                self.snap_all_col_widths();
                for grid in self.pane_grids.values_mut() {
                    grid.dirty = true;
                }
                self.cached_views.clear();
                let center_strategy = match self.config.layout.center_focused_column {
                    ciri_config::config::CenterStrategy::Always => ciri_layout::workspace::CenterStrategy::Always,
                    ciri_config::config::CenterStrategy::OnOverflow => ciri_layout::workspace::CenterStrategy::OnOverflow,
                    ciri_config::config::CenterStrategy::Never => ciri_layout::workspace::CenterStrategy::Never,
                };
                let current_vox = self.view_offset_x.value() as f32;
                let t = self.workspaces.active_mut().target_offset_for_active_with_strategy(center_strategy, current_vox);
                self.view_offset_x.jump_to(t as f64);
                let (cols, rows) = self.compute_grid_size();
                let (cw, ch) = self.cell_dimensions();
                let view = &self.workspaces.view_size;
                log::debug!("  sending Resize: {cols}x{rows} cells, {cw:.1}x{ch:.1} cell_px");
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
                self.handle_keyboard_input(&event, event_loop);
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.handle_cursor_moved(position);
            }

            WindowEvent::MouseInput {
                state,
                button: winit::event::MouseButton::Left,
                ..
            } => {
                if let Some((mx, my)) = self.last_mouse_pos {
                    if state == ElementState::Pressed {
                        self.handle_mouse_pressed(winit::event::MouseButton::Left, mx, my);
                    } else {
                        self.handle_mouse_released(winit::event::MouseButton::Left);
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
                self.handle_mouse_pressed(winit::event::MouseButton::Right, 0.0, 0.0);
            }

            WindowEvent::MouseWheel { delta, phase, .. } => {
                self.handle_mouse_wheel(delta, phase);
            }

            WindowEvent::PinchGesture { delta, phase, .. } => {
                self.handle_pinch_gesture(delta, phase);
            }

            WindowEvent::Ime(ime) => {
                self.handle_ime(ime);
            }

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

            WindowEvent::Focused(focused) => {
                self.window_focused = focused;
            }

            _ => {}
        }
    }
}
