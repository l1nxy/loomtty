use ciri_app::app::ModalKind;
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
                    let shaper = ciri_render::shaper::TextShaper::with_options(
                        &self.core.config.font.family,
                        &ciri_render::shaper::ShapingOptions {
                            preferred_weight: self.core.config.font.weight,
                            features: self.core.config.font.parsed_features(),
                        },
                    );
                    let ui_init = App::resolve_ui_font_init(&self.core.config, new_dpi);
                    let (cache, atlas_gpu) =
                        match renderer.create_atlas(&ciri_render::glyph_cache::FontInitParams {
                            font_size_pt: self.core.config.font.size,
                            dpi_scale: new_dpi,
                            family_name: &self.core.config.font.family,
                            ui_family_name: self.core.config.font.ui.as_ref().and_then(|ui| {
                                (!ui.family.is_empty()).then_some(ui.family.as_str())
                            }),
                            primary_font_path: shaper.primary_font_path(),
                            emoji_font_path: shaper.emoji_font_path(),
                            emoji_font_id: shaper.emoji_font_id(),
                            cjk_font_path: shaper.cjk_font_path(),
                            cjk_font_id: shaper.cjk_font_id(),
                            ui_font_path: ui_init.path.clone(),
                            ui_font_id: ui_init.id,
                            ui_pixel_size: ui_init.pixel_size,
                            render_config: &self.core.config.render,
                            font_resolver: shaper.font_resolver(),
                            #[cfg(windows)]
                            dwrite_resolver: shaper.dwrite_resolver(),
                            cell_width_scale: Some(self.core.config.font.adjust_cell_width),
                            cell_height_scale: Some(self.core.config.font.adjust_cell_height),
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
                    let ui_shaper = App::build_ui_shaper(
                        &ui_init,
                        &shaper,
                        self.core.config.font.size,
                        new_dpi,
                        cache.cell_width,
                        cache.cell_height,
                    );
                    self.glyph_cache = Some(cache);
                    self.glyph_atlas_gpu = Some(atlas_gpu);
                    self.text_shaper = Some(shaper);
                    self.ui_shaper = Some(std::cell::RefCell::new(ui_shaper));
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
        // The motion_ticker tracks ciri-ui chrome animations (banner dots,
        // palette fades, hover transitions); without it in this OR the
        // event loop goes to Wait as soon as anim_mgr settles and chrome
        // animations freeze mid-flight.
        let is_animating = self.core.anim_mgr.is_animating() || self.motion_ticker.is_animating();
        // The connection banner needs the event loop to tick so the dot
        // spinner can animate. Covers three states: backing off between
        // retries, initial pre-handshake "Connecting…", and the halted
        // banner (stays alive so the user can press Esc to dismiss).
        let is_reconnecting = self.core.reconnect_state.is_some()
            || self.core.is_halted()
            || (!self.core.connected && self.core.server_rx.is_some());
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

            // Poll background slot session queries.
            //
            // We poll regardless of palette state because the cycle_session
            // path also issues `refresh_all_slot_session_caches()` to keep the
            // cross-slot cycle complete — those responses must land in
            // `cached_slot_sessions` even when no palette is open.
            if !self.core.slot_session_pending.is_empty() {
                let timed_out = self
                    .core
                    .slot_session_query_start
                    .is_some_and(|t| t.elapsed() >= Duration::from_secs(5));
                if timed_out {
                    log::warn!("slot session query timed out, giving up");
                    self.core.slot_session_pending.clear();
                    self.core.slot_session_query_start = None;
                } else if self.poll_slot_sessions() {
                    needs_redraw = true;
                }
            }

            // Cursor blink
            if self.core.config.terminal.cursor_blink {
                let interval =
                    Duration::from_millis(self.core.config.terminal.cursor_blink_interval_ms);
                if self.cursor_blink_timer.elapsed() >= interval {
                    self.cursor_blink_visible = !self.cursor_blink_visible;
                    self.cursor_blink_timer = Instant::now();
                    needs_redraw = true;
                }
            }

            // Config hot-reload
            if let Some(rx) = &self.config_change_rx
                && rx.try_recv().is_ok()
            {
                let now = Instant::now();
                let echoed_self_write = self
                    .pending_self_config_write_deadline
                    .is_some_and(|deadline| now <= deadline);
                // Always clear the deadline once we've made a decision:
                // either we just consumed the echo (skip), or it
                // expired without one (and the next reload should not
                // be silently skipped if it arrives shortly after).
                self.pending_self_config_write_deadline = None;
                if echoed_self_write {
                    // We just wrote this file ourselves; the in-memory
                    // config already holds the new value. Skip the
                    // reload to avoid clobbering an in-flight stepper
                    // press / dropdown selection that landed between
                    // write and watcher fire.
                } else {
                    self.reload_config();
                    needs_redraw = true;
                }
            }

            // Auto-reconnect
            if !self.core.connected && self.core.server_rx.is_none() {
                let has_reconnect = self.core.reconnect_state.is_some();
                let halted = self.core.is_halted();
                if let Some(plan) = self.prepare_reconnect() {
                    if plan.should_exit {
                        event_loop.exit();
                        return;
                    }
                    self.finish_reconnect_attempt(self.connect(plan.viewport));
                    needs_redraw = true;
                } else if has_reconnect || halted {
                    // Either backing off between retries or parked on a
                    // permanent failure — render the banner and wait.
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

            if needs_redraw {
                self.schedule_redraw();
            }
        }

        // ── Schedule next wake ──
        // Computed *after* tick processing so deadlines reflect post-tick
        // state (e.g. cursor_blink_timer reset, leader state cleared).
        let leader_deadline = self.core.input.leader_deadline();
        let blink_deadline = if wants_blink {
            let interval =
                Duration::from_millis(self.core.config.terminal.cursor_blink_interval_ms);
            Some(self.cursor_blink_timer + interval)
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
        // Woken by EventLoopProxy from a background thread. Could be the
        // reader thread (server events) or the wallpaper-decode worker;
        // both share the `()` event type, so just check both. Either
        // path's `did anything change` signal must trigger a redraw —
        // an idle session would otherwise hold the stale frame until
        // the next user input.
        let bg_applied = self.apply_pending_background_image();
        let server_changed = self.process_server_events();
        if bg_applied || server_changed {
            self.schedule_redraw();
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
        #[allow(unused_mut)]
        let mut attrs = WindowAttributes::default()
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

        let shaper = ciri_render::shaper::TextShaper::with_options(
            &self.core.config.font.family,
            &ciri_render::shaper::ShapingOptions {
                preferred_weight: self.core.config.font.weight,
                features: self.core.config.font.parsed_features(),
            },
        );
        let ui_init = App::resolve_ui_font_init(&self.core.config, dpi_scale);
        let (cache, atlas_gpu) = renderer
            .create_atlas(&ciri_render::glyph_cache::FontInitParams {
                font_size_pt: self.core.config.font.size,
                dpi_scale,
                family_name: &self.core.config.font.family,
                ui_family_name: self
                    .core
                    .config
                    .font
                    .ui
                    .as_ref()
                    .and_then(|ui| (!ui.family.is_empty()).then_some(ui.family.as_str())),
                primary_font_path: shaper.primary_font_path(),
                emoji_font_path: shaper.emoji_font_path(),
                emoji_font_id: shaper.emoji_font_id(),
                cjk_font_path: shaper.cjk_font_path(),
                cjk_font_id: shaper.cjk_font_id(),
                ui_font_path: ui_init.path.clone(),
                ui_font_id: ui_init.id,
                ui_pixel_size: ui_init.pixel_size,
                render_config: &self.core.config.render,
                font_resolver: shaper.font_resolver(),
                #[cfg(windows)]
                dwrite_resolver: shaper.dwrite_resolver(),
                cell_width_scale: Some(self.core.config.font.adjust_cell_width),
                cell_height_scale: Some(self.core.config.font.adjust_cell_height),
            })
            .expect("initial glyph atlas creation failed");
        let ui_shaper = App::build_ui_shaper(
            &ui_init,
            &shaper,
            self.core.config.font.size,
            dpi_scale,
            cache.cell_width,
            cache.cell_height,
        );

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
            Ok((tx, rx, cancel)) => {
                self.core.server_tx = Some(tx);
                self.core.server_rx = Some(rx);
                self.connection_cancel = Some(cancel);
            }
            Err(e) => {
                log::error!("failed to connect to server: {e}");
            }
        }

        // Config hot-reload watcher. Watches the config file's
        // PARENT directory rather than the file itself: our writer
        // saves atomically (write to `*.tmp`, then `rename` over the
        // target), which replaces the file's inode. On Linux inotify
        // the watch follows the inode, so a watch installed on the
        // file directly dies the first time the settings panel
        // persists a change — every later external edit then goes
        // unobserved until restart. Watching the directory keeps the
        // observation live across rename-style saves; we filter the
        // events by file name so unrelated dir activity doesn't
        // trigger reloads.
        {
            let (ctx, crx) = crossbeam_channel::bounded(1);
            let path = ciri_config::config::config_path();
            let target_name = path.file_name().map(|n| n.to_os_string());
            let watcher = notify::recommended_watcher(move |res: Result<notify::Event, _>| {
                let Ok(evt) = res else { return };
                // Accept Modify (in-place editor save), Create (the
                // new file landing after our atomic rename, or an
                // external rename-based editor), and Remove (the old
                // inode being unlinked during a replace). Any one
                // of them indicates the file we care about changed.
                if !(evt.kind.is_modify() || evt.kind.is_create() || evt.kind.is_remove()) {
                    return;
                }
                let touches_config = match &target_name {
                    Some(name) => evt.paths.iter().any(|p| p.file_name() == Some(name)),
                    None => true,
                };
                if touches_config {
                    let _ = ctx.try_send(());
                }
            })
            .ok();
            if let Some(mut w) = watcher {
                use notify::Watcher;
                let watch_target = path.parent().unwrap_or(&path).to_path_buf();
                // Ensure the parent dir exists — first run before
                // any save would otherwise fail the watch attempt.
                let _ = std::fs::create_dir_all(&watch_target);
                if let Err(e) = w.watch(&watch_target, notify::RecursiveMode::NonRecursive) {
                    log::warn!(
                        "failed to watch config dir {}: {e}",
                        watch_target.display()
                    );
                } else {
                    self.config_watcher = Some(w);
                    self.config_change_rx = Some(crx);
                    log::info!(
                        "config watcher active: {} (watching parent {})",
                        path.display(),
                        watch_target.display(),
                    );
                }
            }
        }

        self.dpi_scale = dpi_scale;
        self.glyph_cache = Some(cache);
        self.glyph_atlas_gpu = Some(atlas_gpu);
        self.text_shaper = Some(shaper);
        self.ui_shaper = Some(std::cell::RefCell::new(ui_shaper));
        self.snap_all_col_widths();
        self.animate_to_active();
        self.window = Some(window);
        self.renderer = Some(renderer);
        self.last_frame = Instant::now();

        // Push the configured background image to the renderer, if any.
        // No-op when the path is empty or the active backend doesn't yet
        // implement the textured-quad pipeline (everything except DX in
        // this commit).
        self.reload_background_image();

        self.schedule_redraw();
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
                    self.schedule_redraw();
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

            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: winit::event::MouseButton::Middle,
                ..
            } => {
                if let Some((mx, my)) = self.last_mouse_pos {
                    self.handle_mouse_pressed(winit::event::MouseButton::Middle, mx, my);
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
                    // Dismiss every modal-ish overlay on focus loss
                    // — alt-tabbing back into a stale palette /
                    // search / paste dialog and discovering the
                    // first keystrokes went there is bad UX. Drag
                    // teardown handles the case where the
                    // finalising mouse-up lands in another window.
                    self.enter_modal_close_peers(ModalKind::None);
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
                self.schedule_redraw();
            }

            WindowEvent::RedrawRequested => self.render(),

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        self.flush_pending_redraw();
    }
}
