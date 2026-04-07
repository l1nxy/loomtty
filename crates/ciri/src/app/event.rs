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
        if self.core.should_exit {
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
        if let Some((size, last_event)) = self.pending_resize
            && last_event.elapsed() >= RESIZE_SETTLE
        {
            self.pending_resize = None;

            // Apply deferred DPI change first (if any)
            if let Some(new_dpi) = self.pending_dpi.take() {
                self.dpi_scale = new_dpi;
                self.destroy_gpu_resources();
                if let Some(renderer) = &mut self.renderer {
                    let shaper =
                        ciri_render::shaper::TextShaper::new(&self.core.config.font.family);
                    let (cache, atlas_gpu) = match renderer
                        .create_atlas(&ciri_render::glyph_cache::FontInitParams {
                            font_size_pt: self.core.config.font.size,
                            dpi_scale: new_dpi,
                            family_name: &self.core.config.font.family,
                            primary_font_path: shaper.primary_font_path(),
                            emoji_font_path: shaper.emoji_font_path(),
                            emoji_font_id: shaper.emoji_font_id(),
                            cjk_font_path: shaper.cjk_font_path(),
                            cjk_font_id: shaper.cjk_font_id(),
                            render_config: &self.core.config.render,
                            font_resolver: shaper.font_resolver(),
                            #[cfg(windows)]
                            dwrite_resolver: shaper.dwrite_resolver(),
                        }) {
                        Ok(v) => v,
                        Err(e) => {
                            log::error!("failed to recreate glyph atlas on DPI change: {e}");
                            return;
                        }
                    };
                    log::info!(
                        "DPI changed: scale={:.2} cell={:.1}x{:.1}",
                        new_dpi,
                        cache.cell_width,
                        cache.cell_height
                    );
                    self.glyph_cache = Some(cache);
                    self.glyph_atlas_gpu = Some(atlas_gpu);
                    self.text_shaper = Some(shaper);
                    self.clear_render_caches();
                    for grid in self.core.pane_grids.values_mut() {
                        grid.dirty = true;
                    }
                }
            }

            if let Some(renderer) = &mut self.renderer {
                renderer.apply_surface();
            }
            self.apply_resize(size);
        }

        // Idle-aware event loop: only poll at frame rate when animating or
        // expecting updates. Switch to Wait when idle to save power.
        let is_animating = self.core.anim_mgr.is_animating();
        let is_reconnecting = self.core.reconnect_state.is_some();
        let wants_blink = self.core.config.terminal.cursor_blink;
        let has_remote_query =
            self.core.remote_query_rx.is_some() || !self.core.slot_session_pending.is_empty();

        let has_pending = self
            .core
            .server_rx
            .as_ref()
            .is_some_and(|rx| !rx.is_empty());

        let resize_deadline = self
            .pending_resize
            .map(|(_, last_event)| last_event + RESIZE_SETTLE);

        let is_resizing = resize_deadline.is_some();

        if matches!(
            cause,
            StartCause::ResumeTimeReached { .. }
                | StartCause::Poll
                | StartCause::WaitCancelled { .. }
        ) {
            // Proactive leader timeout (tmux-style): clear expired leader
            // state so the UI updates without waiting for the next key event.
            let mut leader_expired = false;
            if let Some(ld) = self.core.input.leader_deadline() {
                if ld <= Instant::now() {
                    self.core.input.poll_timeout();
                    leader_expired = !self.core.input.is_awaiting_action();
                }
            }

            let mut needs_redraw = is_animating || is_resizing || leader_expired;

            if self.process_server_events() {
                needs_redraw = true;
            }

            // Send prediction ping if due
            if let Some(ping) = self.core.prediction.maybe_send_ping() {
                self.send(ping);
            }

            // Poll async remote session query result
            if let Some(rx) = &self.core.remote_query_rx
                && let Ok(result) = rx.try_recv()
            {
                self.core.remote_query_rx = None;
                self.handle_remote_query_result(result);
                needs_redraw = true;
            }

            // Poll background slot session queries
            if !self.core.slot_session_pending.is_empty() {
                let timed_out = self
                    .core
                    .slot_session_query_start
                    .is_some_and(|t| t.elapsed() >= Duration::from_secs(5));
                if self.core.command_palette.is_none() || timed_out {
                    // Palette was closed or query timed out — stop polling
                    if timed_out {
                        log::warn!("slot session query timed out, giving up");
                    }
                    self.core.slot_session_pending.clear();
                    self.core.slot_session_query_start = None;
                } else {
                    self.poll_slot_sessions();
                    needs_redraw = true;
                }
            }

            // Cursor blink
            if self.core.config.terminal.cursor_blink {
                let interval =
                    Duration::from_millis(self.core.config.terminal.cursor_blink_interval_ms);
                if self.core.cursor_blink_timer.elapsed() >= interval {
                    self.core.cursor_blink_visible = !self.core.cursor_blink_visible;
                    self.core.cursor_blink_timer = Instant::now();
                    needs_redraw = true;
                }
            }

            // Config hot-reload
            if let Some(rx) = &self.config_change_rx
                && rx.try_recv().is_ok()
            {
                self.reload_config();
                needs_redraw = true;
            }

            // Auto-reconnect
            if !self.core.connected && self.core.server_rx.is_none() {
                let has_reconnect = self.core.reconnect_state.is_some();
                if let Some(plan) = self.prepare_reconnect() {
                    if plan.should_exit {
                        event_loop.exit();
                        return;
                    }
                    self.finish_reconnect_attempt(self.connect(plan.viewport));
                    needs_redraw = true;
                } else if has_reconnect {
                    needs_redraw = true;
                } else {
                    log::info!("server connection lost, exiting");
                    self.clear_render_caches();
                    self.destroy_gpu_resources();
                    self.renderer = None;
                    self.window = None;
                    event_loop.exit();
                    return;
                }
            }

            // Exit if all panes gone
            if self.core.connected
                && self.core.pane_grids.is_empty()
                && self.core.workspaces.active().is_empty()
            {
                self.clear_render_caches();
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

        // ── Schedule next wake ──
        // Computed *after* tick processing so deadlines reflect post-tick
        // state (e.g. cursor_blink_timer reset, leader state cleared).
        let leader_deadline = self.core.input.leader_deadline();
        let blink_deadline = if wants_blink {
            let interval = Duration::from_millis(self.core.config.terminal.cursor_blink_interval_ms);
            Some(self.core.cursor_blink_timer + interval)
        } else {
            None
        };
        let optional_wake = [leader_deadline, blink_deadline]
            .into_iter()
            .flatten()
            .min();

        if let Some(resize_deadline) = resize_deadline {
            let frame_wake = Instant::now() + self.core.frame_interval;
            let mut wake = frame_wake.min(resize_deadline);
            if let Some(ow) = optional_wake {
                wake = wake.min(ow);
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(wake));
        } else if is_animating || has_pending || is_reconnecting || has_remote_query {
            let mut wake = Instant::now() + self.core.frame_interval;
            if let Some(ow) = optional_wake {
                wake = wake.min(ow);
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(wake));
        } else if let Some(wake) = optional_wake {
            event_loop.set_control_flow(ControlFlow::WaitUntil(wake));
        } else {
            // Fully idle: wait for EventLoopProxy wake from server thread
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        // Woken by EventLoopProxy from the reader thread — process pending server events.
        if self.process_server_events() {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let window_title = format!(
            "{} [{}]",
            self.core.config.window.title, self.core.session_name
        );
        let window_icon = load_window_icon();
        let attrs = WindowAttributes::default()
            .with_title(window_title)
            .with_window_icon(window_icon)
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.core.config.window.width,
                self.core.config.window.height,
            ));

        // macOS: transparent titlebar for a cleaner look, but keep title visible
        // and do NOT use fullsize_content_view (no safe area handling for traffic lights)
        #[cfg(target_os = "macos")]
        {
            use winit::platform::macos::WindowAttributesExtMacOS;
            attrs = attrs.with_titlebar_transparent(true);
        }

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );
        window.set_ime_allowed(true);

        #[cfg(target_os = "macos")]
        {
            use winit::platform::macos::{OptionAsAlt, WindowExtMacOS};
            // OnlyLeft: left Option = Alt (for keybindings), right Option = special chars (é, ñ, #)
            window.set_option_as_alt(OptionAsAlt::OnlyLeft);
        }
        let dpi_scale = window.scale_factor();
        let mut renderer = ciri_gpu::Renderer::new(window.clone(), &self.core.config.render)
            .expect("renderer init failed");

        let shaper = ciri_render::shaper::TextShaper::new(&self.core.config.font.family);
        let (cache, atlas_gpu) = renderer
            .create_atlas(&ciri_render::glyph_cache::FontInitParams {
                font_size_pt: self.core.config.font.size,
                dpi_scale,
                family_name: &self.core.config.font.family,
                primary_font_path: shaper.primary_font_path(),
                emoji_font_path: shaper.emoji_font_path(),
                emoji_font_id: shaper.emoji_font_id(),
                cjk_font_path: shaper.cjk_font_path(),
                cjk_font_id: shaper.cjk_font_id(),
                render_config: &self.core.config.render,
                font_resolver: shaper.font_resolver(),
                #[cfg(windows)]
                dwrite_resolver: shaper.dwrite_resolver(),
            })
            .expect("initial glyph atlas creation failed");

        let (w, h) = renderer.surface_size();
        let bar_padding = self
            .core
            .config
            .statusbar
            .height_padding
            .unwrap_or(cache.cell_height * self.core.config.statusbar.padding_ratio);
        let bar_h = cache.cell_height + bar_padding;
        let chrome_h = bar_h * 2.0; // status bar + hints bar
        self.core.workspaces.resize_view(ViewSize {
            width: w as f32,
            height: h as f32 - chrome_h,
        });

        log::info!(
            "cell: {:.1}x{:.1} ascent={:.1} (dpi_scale={:.2})",
            cache.cell_width,
            cache.cell_height,
            cache.ascent,
            dpi_scale
        );

        let view = &self.core.workspaces.view_size;
        let viewport = ciri_protocol::codec::ClientHello {
            session_name: self.core.session_name.clone(),
            width: view.width as u32,
            height: view.height as u32,
            cell_width: cache.cell_width,
            cell_height: cache.cell_height,
        };
        log::info!("connecting to session '{}'", self.core.session_name);
        match self.connect(viewport) {
            Ok((tx, rx)) => {
                self.core.server_tx = Some(tx);
                self.core.server_rx = Some(rx);
            }
            Err(e) => {
                log::error!("failed to connect to server: {e}");
            }
        }

        // Config hot-reload watcher
        {
            let (ctx, crx) = crossbeam_channel::bounded(1);
            let watcher = notify::recommended_watcher(move |res: Result<notify::Event, _>| {
                if let Ok(evt) = res
                    && evt.kind.is_modify()
                {
                    let _ = ctx.try_send(());
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
        self.core.last_frame = Instant::now();

        self.window.as_ref().unwrap().request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        use ciri_protocol::message::ClientMessage;

        match event {
            WindowEvent::CloseRequested => {
                self.send(ClientMessage::Detach);
                self.core.pane_grids.clear();
                self.clear_render_caches();
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
                self.core.context_menu.visible = false;
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
                    // Defer DPI change — will be applied when resize settles.
                    // This avoids rebuilding the atlas repeatedly while the
                    // window is being dragged across monitors with different DPI.
                    self.pending_dpi = Some(scale_factor);
                }
            }

            WindowEvent::Focused(focused) => {
                self.window_focused = focused;
                self.send(ClientMessage::FocusChange { focused });
                if !focused {
                    self.core.context_menu.visible = false;
                }
            }

            WindowEvent::DroppedFile(path) => {
                let path_str = path.to_string_lossy();
                let quoted = if path_str
                    .contains(|c: char| c.is_whitespace() || "\"'\\$`!#&|;(){}[]<>?*~".contains(c))
                {
                    format!("'{}'", path_str.replace('\'', "'\\''"))
                } else {
                    path_str.into_owned()
                };
                let text = format!("{quoted} ");
                self.send_paste_to_active_pane(text.as_bytes());
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }

            WindowEvent::RedrawRequested => self.render(),

            _ => {}
        }
    }
}
