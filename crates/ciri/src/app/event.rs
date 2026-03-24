use ciri_layout::geometry::ViewSize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Icon, WindowAttributes, WindowId};

use super::App;

fn load_window_icon() -> Option<Icon> {
    let png_bytes = include_bytes!("../../../../assets/icons/icon.png");
    let img = image::load_from_memory_with_format(png_bytes, image::ImageFormat::Png).ok()?;
    let rgba = img.into_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    Icon::from_rgba(rgba.into_raw(), w, h).ok()
}

impl ApplicationHandler for App {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if self.should_exit {
            self.destroy_gpu_resources();
            self.renderer = None;
            self.window = None;
            event_loop.exit();
            return;
        }

        // Resize strategy:
        // - preview locally on every resize event
        // - defer PTY/server resize AND swapchain reconfigure until settled
        // - during live resize, render at old swapchain size (compositor scales)
        const RESIZE_SETTLE: Duration = Duration::from_millis(20);
        if let Some((size, last_event)) = self.pending_resize {
            if last_event.elapsed() >= RESIZE_SETTLE {
                self.pending_resize = None;
                if let Some(renderer) = &mut self.renderer {
                    renderer.apply_surface();
                }
                self.apply_resize(size);
            }
        }

        // Idle-aware event loop: only poll at frame rate when animating or
        // expecting updates. Switch to Wait when idle to save power.
        let is_animating = self.view_offset_x.is_animating()
            || self.view_offset_y.is_animating()
            || self.overview.zoom.is_animating()
            || self.gestures.row_offset.is_animating()
            || self.col_widths.iter().any(|v| v.is_animating())
            || !self.pane_anims.open_opacity.is_empty()
            || !self.pane_anims.closing.is_empty();
        let has_server = self.server_rx.is_some();
        let is_reconnecting = self.reconnect_state.is_some();
        let wants_blink = self.config.terminal.cursor_blink;

        let has_pending = self.server_rx.as_ref().is_some_and(|rx| !rx.is_empty());

        let resize_deadline = self
            .pending_resize
            .map(|(_, last_event)| last_event + RESIZE_SETTLE);

        let is_resizing = resize_deadline.is_some();

        if let Some(resize_deadline) = resize_deadline {
            // During live resize: render at frame rate for a smooth preview.
            // Surface.configure() is deferred to render time so we only
            // rebuild the swapchain once per frame regardless of event count.
            let frame_wake = Instant::now() + self.frame_interval;
            event_loop
                .set_control_flow(ControlFlow::WaitUntil(frame_wake.min(resize_deadline)));
        } else if is_animating || has_pending || is_reconnecting {
            // Active rendering or pending data: poll at frame rate
            event_loop
                .set_control_flow(ControlFlow::WaitUntil(Instant::now() + self.frame_interval));
        } else if wants_blink || has_server {
            // Connected but idle: poll at reduced rate (50ms = 20fps idle)
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(50),
            ));
        } else {
            // Disconnected, no animations: fully idle
            event_loop.set_control_flow(ControlFlow::Wait);
        }

        if matches!(
            cause,
            StartCause::ResumeTimeReached { .. } | StartCause::Poll
        ) {
            let mut needs_redraw = is_animating || is_resizing;

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
                    match self.connect(viewport) {
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
                    self.cached_tile_glyphs.clear();
                    self.destroy_gpu_resources();
                    self.renderer = None;
                    self.window = None;
                    event_loop.exit();
                    return;
                }
            }

            // Exit if all panes gone
            if self.connected && self.pane_grids.is_empty() && self.workspaces.active().is_empty() {
                self.cached_views.clear();
                self.cached_tile_glyphs.clear();
                self.destroy_gpu_resources();
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
        let window_icon = load_window_icon();
        let attrs = WindowAttributes::default()
            .with_title(window_title)
            .with_window_icon(window_icon)
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
        let mut renderer = ciri_gpu::Renderer::new(
            window.clone(),
            &self.config.render,
        )
        .expect("renderer init failed");

        let shaper = ciri_render::shaper::TextShaper::new(&self.config.font.family);
        let (cache, atlas_gpu) = renderer.create_atlas(
            self.config.font.size,
            dpi_scale,
            &self.config.font.family,
            shaper.primary_font_path(),
            shaper.emoji_font_path(),
            shaper.emoji_font_id(),
            shaper.cjk_font_path(),
            shaper.cjk_font_id(),
            &self.config.render,
        );

        let (w, h) = renderer.surface_size();
        let bar_padding = self
            .config
            .statusbar
            .height_padding
            .unwrap_or(cache.cell_height * self.config.statusbar.padding_ratio);
        let bar_h = cache.cell_height + bar_padding;
        self.workspaces.resize_view(ViewSize {
            width: w as f32,
            height: h as f32 - bar_h,
        });

        log::info!(
            "cell: {:.1}x{:.1} ascent={:.1} (dpi_scale={:.2})",
            cache.cell_width,
            cache.cell_height,
            cache.ascent,
            dpi_scale
        );

        let view = &self.workspaces.view_size;
        let viewport = ciri_protocol::codec::ClientHello {
            session_name: self.session_name.clone(),
            width: view.width as u32,
            height: view.height as u32,
            cell_width: cache.cell_width,
            cell_height: cache.cell_height,
        };
        log::info!("connecting to session '{}'", self.session_name);
        match self.connect(viewport) {
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
        self.glyph_cache = Some(cache);
        self.glyph_atlas_gpu = Some(atlas_gpu);
        self.text_shaper = Some(shaper);
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
                self.cached_tile_glyphs.clear();
                self.destroy_gpu_resources();
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
                self.context_menu.visible = false;
                // Keep local viewport/layout state in sync immediately so the
                // user sees a smooth local preview while dragging.
                self.preview_resize(size);
                if let Some(renderer) = &mut self.renderer {
                    let (surface_w, surface_h) = renderer.surface_size();
                    if surface_w != size.width || surface_h != size.height {
                        renderer.resize(size.width, size.height);
                    }
                }
                // Only the expensive layout/PTY resize stays deferred.
                self.pending_resize = Some((size, Instant::now()));
                // Don't request_redraw() on every resize event — the event
                // loop timer will pick up the next frame at the right cadence.
                // This avoids queuing redundant redraws during fast drags.
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
                if let Some((mx, my)) = self.last_mouse_pos {
                    self.handle_mouse_pressed(winit::event::MouseButton::Right, mx, my);
                }
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
                    // Destroy old GPU atlas before creating new one
                    self.destroy_gpu_resources();

                    if let Some(renderer) = &mut self.renderer {
                        let shaper = ciri_render::shaper::TextShaper::new(&self.config.font.family);
                        let (cache, atlas_gpu) = renderer.create_atlas(
                            self.config.font.size,
                            scale_factor,
                            &self.config.font.family,
                            shaper.primary_font_path(),
                            shaper.emoji_font_path(),
                            shaper.emoji_font_id(),
                            shaper.cjk_font_path(),
                            shaper.cjk_font_id(),
                            &self.config.render,
                        );
                        log::info!(
                            "DPI changed: scale={:.2} cell={:.1}x{:.1}",
                            scale_factor,
                            cache.cell_width,
                            cache.cell_height
                        );
                        let bar_h =
                            cache.cell_height
                                + self.config.statusbar.height_padding.unwrap_or(
                                    cache.cell_height * self.config.statusbar.padding_ratio,
                                );
                        let (w, h) = renderer.surface_size();
                        self.workspaces.resize_view(ViewSize {
                            width: w as f32,
                            height: h as f32 - bar_h,
                        });
                        self.glyph_cache = Some(cache);
                        self.glyph_atlas_gpu = Some(atlas_gpu);
                        self.text_shaper = Some(shaper);
                        self.cached_views.clear();
                        self.cached_tile_glyphs.clear();
                        for grid in self.pane_grids.values_mut() {
                            grid.dirty = true;
                        }
                        // Notify server of new cell dimensions
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
                    }
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
            }

            WindowEvent::Focused(focused) => {
                self.window_focused = focused;
                self.send(ClientMessage::FocusChange { focused });
                if !focused {
                    self.context_menu.visible = false;
                }
            }

            WindowEvent::RedrawRequested => self.render(),

            _ => {}
        }
    }
}
