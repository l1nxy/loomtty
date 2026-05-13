use ciri_app::app::ModalKind;
use ciri_protocol::message::*;
use std::sync::Arc;

use super::App;
use crate::connection::ServerEvent;
use crate::grid::ClientPaneGrid;

impl App {
    pub(crate) fn reload_input_config(&mut self) {
        self.core.input.reload_bindings(
            &self.core.config.keys.leader,
            match self.core.config.input.mode {
                ciri_config::config::InputMode::Prefix => "prefix",
                ciri_config::config::InputMode::Sticky => "sticky",
            },
            &self.core.config.keys.bindings,
            &self.core.config.keys.modes,
            &self.core.config.keys.direct_bindings,
        );
        Self::rebuild_binding_set(&mut self.core.input, &self.core.config);
    }

    pub(crate) fn update_prediction_config(&mut self) {
        self.core.prediction.update_config(
            self.core.config.prediction.mode,
            self.core.config.prediction.threshold_ms,
            self.core.config.prediction.show_underline,
        );
    }

    pub(crate) fn apply_font_config_change(&mut self) -> bool {
        self.destroy_gpu_resources();
        if let Some(renderer) = &mut self.renderer {
            let shaper = ciri_render::shaper::TextShaper::with_options(
                &self.core.config.font.family,
                &ciri_render::shaper::ShapingOptions {
                    preferred_weight: self.core.config.font.weight,
                    features: self.core.config.font.parsed_features(),
                },
            );
            let ui_init = App::resolve_ui_font_init(&self.core.config, self.dpi_scale);
            let (cache, atlas_gpu) =
                match renderer.create_atlas(&ciri_render::glyph_cache::FontInitParams {
                    font_size_pt: self.core.config.font.size,
                    dpi_scale: self.dpi_scale,
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
                }) {
                    Ok(v) => v,
                    Err(e) => {
                        log::error!("failed to recreate glyph atlas on font change: {e}");
                        return false;
                    }
                };
            let ui_shaper = App::build_ui_shaper(
                &ui_init,
                &shaper,
                self.core.config.font.size,
                self.dpi_scale,
                cache.cell_width,
                cache.cell_height,
            );
            self.glyph_cache = Some(cache);
            self.glyph_atlas_gpu = Some(atlas_gpu);
            self.text_shaper = Some(shaper);
            self.ui_shaper = Some(std::cell::RefCell::new(ui_shaper));
        }
        self.clear_render_caches();
        for grid in self.core.pane_grids.values_mut() {
            grid.dirty = true;
        }
        let (cw, ch) = self.cell_dimensions();
        let view = &self.core.workspaces.view_size;
        self.send(ClientMessage::Resize {
            cols: 0,
            rows: 0,
            width: view.width as u32,
            height: view.height as u32,
            cell_width: cw,
            cell_height: ch,
        });
        true
    }

    fn finalize_authoritative_session_switch(&mut self, pane_ids: &[u64]) {
        let Some(session_name) = self.core.pending_session_name.take() else {
            return;
        };

        // Close every modal BEFORE the pane_grids map is replaced by
        // the incoming layout — once the old pane is gone,
        // `close_search_restore_scroll` (run inside the helper) would
        // find no grid to update and silently drop the offset.
        // Without this, a session switch initiated while a search
        // was active leaves an orphaned `search_state.pane_id`
        // pointing into the new session's pane set.
        self.enter_modal_close_peers(ModalKind::None);
        self.core.session_name = session_name;
        self.core.expected_pane_ids = pane_ids.iter().copied().collect();
        self.write_last_session();
        self.core.slot_session_pending.clear();
        self.core.slot_session_query_start = None;
        // The incoming layout is unrelated to the previous session's columns.
        // Instead of letting stale col_widths spring into the new targets,
        // request an "equalize then settle" pass: all columns start at the
        // viewport's average width and spring to their real targets. Subtle
        // when the new layout is near-uniform, gives a clear "re-layout"
        // beat when it isn't.
        self.core.anim_mgr.col_widths_equalize_pending = true;
        if let Some(window) = &self.window {
            window.set_title(&format!(
                "{} [{}]",
                self.core.config.window.title, self.core.session_name
            ));
        }
    }

    /// Process all pending server events. Returns true if a redraw is needed.
    ///
    /// Drains up to `BATCH` events at a time, repeating until the channel is
    /// empty or `MAX_DRAIN` time has elapsed.  This avoids rendering
    /// intermediate states when a large backlog has accumulated (e.g. after
    /// the window was in the background), while still bounding the time spent
    /// here so the UI thread stays responsive.
    pub fn process_server_events(&mut self) -> bool {
        let Some(rx) = self.core.server_rx.take() else {
            return false;
        };
        const BATCH: usize = 256;
        const MAX_DRAIN: std::time::Duration = std::time::Duration::from_millis(12);
        let drain_start = std::time::Instant::now();
        let mut needs_redraw = false;

        // Drain buffered events from restored slots first (they arrived
        // while the slot was backgrounded and must be replayed in order).
        let mut buffered: Vec<_> = self.core.buffered_events.drain(..).collect();

        loop {
            let mut events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
                .take(BATCH)
                .collect();
            let hit_budget = events.len() >= BATCH;
            // Prepend buffered events (only on the first iteration)
            if !buffered.is_empty() {
                buffered.append(&mut events);
                events = std::mem::take(&mut buffered);
            }
            if events.is_empty() {
                break;
            }

            for event in events {
                match event {
                    ServerEvent::Control(ServerMessage::StateSync { layout, pane_ids }) => {
                        self.finalize_authoritative_session_switch(&pane_ids);
                        self.core.expected_pane_ids = pane_ids.iter().copied().collect();
                        self.apply_layout(&layout);
                        for &id in &pane_ids {
                            self.core.pane_grids.entry(id).or_insert_with(|| {
                                ClientPaneGrid::new(
                                    80,
                                    24,
                                    self.core.config.terminal.scrollback_lines,
                                )
                            });
                            self.core.anim_mgr.ensure_pane_registered(id);
                        }
                        self.core.connected = true;
                        // Pre-fetch session list so the palette / cycle has
                        // data immediately. Also refresh background slot caches
                        // so cycling across slots can see the full session list
                        // on the other side.
                        self.send(ClientMessage::ListSessions { all: false });
                        self.core.refresh_all_slot_session_caches();
                        needs_redraw = true;
                    }
                    ServerEvent::Control(ServerMessage::LayoutUpdate { layout }) => {
                        log::trace!(
                            "LayoutUpdate: {} workspaces, active={}",
                            layout.workspaces.len(),
                            layout.active_workspace_idx
                        );
                        for (i, ws) in layout.workspaces.iter().enumerate() {
                            log::trace!(
                                "  ws[{}]: {} columns, active_col={}",
                                i,
                                ws.columns.len(),
                                ws.active_column_idx
                            );
                            for (j, col) in ws.columns.iter().enumerate() {
                                log::trace!(
                                    "    col[{}]: width={:.3}, {} tiles",
                                    j,
                                    col.width_proportion,
                                    col.tiles.len()
                                );
                            }
                        }
                        self.apply_layout(&layout);
                        // Only cancel an in-flight tile drag if indices became
                        // invalid; focus-only syncs must not abort it.
                        if let Some((col_idx, top_tile_idx)) = self.drag.tile_dragging {
                            let still_valid = self
                                .core
                                .workspaces
                                .active()
                                .columns
                                .get(col_idx)
                                .is_some_and(|col| top_tile_idx + 1 < col.tiles.len());
                            if !still_valid {
                                self.drag.tile_dragging = None;
                            }
                        }
                        needs_redraw = true;
                    }
                    ServerEvent::Control(ServerMessage::PaneCreated {
                        pane_id,
                        cols,
                        rows,
                        ..
                    }) => {
                        log::debug!("PaneCreated: pane_id={pane_id} {cols}x{rows}");
                        self.core.expected_pane_ids.insert(pane_id);
                        self.core.pane_grids.entry(pane_id).or_insert_with(|| {
                            ClientPaneGrid::new(
                                cols,
                                rows,
                                self.core.config.terminal.scrollback_lines,
                            )
                        });
                        let params = self.anim_config();
                        self.core.anim_mgr.on_pane_created(pane_id, &params);
                        needs_redraw = true;
                    }
                    ServerEvent::Control(ServerMessage::PaneClosed { pane_id }) => {
                        self.core.expected_pane_ids.remove(&pane_id);
                        log::debug!("PaneClosed: pane_id={pane_id}");
                        // Capture pane rect for close animation before removing
                        let vox = self.core.anim_mgr.view_offset_x.value() as f32;
                        let voy = self.core.anim_mgr.view_offset_y.value() as f32;
                        let tiles = self.core.workspaces.visible_tiles_2d(vox, voy);
                        let params = self.anim_config();
                        if let Some((_, rect, _)) = tiles.iter().find(|(pid, _, _)| *pid == pane_id)
                        {
                            let geo = ciri_anim::manager::GeoRect {
                                x: rect.x,
                                y: rect.y,
                                w: rect.w,
                                h: rect.h,
                            };
                            self.core.anim_mgr.on_pane_closed(pane_id, geo, &params);
                        } else {
                            // Off-screen pane: just remove state, no close animation
                            self.core.anim_mgr.on_pane_closed(
                                pane_id,
                                ciri_anim::manager::GeoRect {
                                    x: 0.0,
                                    y: 0.0,
                                    w: 0.0,
                                    h: 0.0,
                                },
                                &params,
                            );
                        }
                        self.core.pane_grids.remove(&pane_id);
                        self.core.prediction.clear_pane(pane_id);
                        self.core.image_placements.remove(&pane_id);
                        self.invalidate_pane_images(pane_id);
                        self.invalidate_pane_cache(pane_id);
                        needs_redraw = true;
                    }
                    ServerEvent::Control(ServerMessage::ServerShutdown) => {
                        log::info!("server shut down, exiting");
                        self.core.should_exit = true;
                        break;
                    }
                    ServerEvent::Control(ServerMessage::ClipboardStore { data }) => {
                        // OSC 52: TUI app wrote to clipboard via server
                        if let Some(cb) = &mut self.clipboard {
                            if let Err(e) = cb.set_text(&data) {
                                log::warn!("OSC 52 clipboard write failed: {e}");
                            } else {
                                log::debug!("OSC 52: clipboard set ({} bytes)", data.len());
                            }
                        }
                    }
                    ServerEvent::Control(ServerMessage::Bell { pane_id }) => {
                        log::debug!("bell from pane {pane_id}");
                        let params = self.anim_config();
                        self.core.anim_mgr.on_bell(pane_id, &params);

                        // Window urgency hint
                        if self.core.config.terminal.bell_urgency
                            && !self.window_focused
                            && let Some(ref window) = self.window
                        {
                            window.request_user_attention(Some(
                                winit::window::UserAttentionType::Informational,
                            ));
                        }

                        // Bell audio
                        self.play_bell_audio();

                        needs_redraw = true;
                    }
                    ServerEvent::Control(ServerMessage::CommandCompleted {
                        pane_id,
                        duration_secs,
                        exit_code,
                    }) => {
                        let threshold = self.core.config.terminal.notify_command_threshold_secs;
                        if threshold > 0 && duration_secs >= threshold && !self.window_focused {
                            self.send_desktop_notification(
                                "Command completed",
                                &format!(
                                    "Pane {} finished after {}s (exit: {})",
                                    pane_id,
                                    duration_secs,
                                    exit_code.unwrap_or(0)
                                ),
                            );
                        }
                    }
                    ServerEvent::Control(ServerMessage::Notification {
                        pane_id,
                        title,
                        body,
                    }) => {
                        log::debug!("notification from pane {pane_id}: {title}: {body}");
                        let title = if title.is_empty() {
                            "ciritty".to_string()
                        } else {
                            title
                        };
                        self.send_desktop_notification(&title, &body);
                    }
                    ServerEvent::Control(ServerMessage::ImagePlacement {
                        pane_id,
                        image_id,
                        col,
                        row,
                        width_cells,
                        height_cells,
                        pixel_width,
                        pixel_height,
                        display_mode,
                        format,
                        data,
                    }) => {
                        log::debug!(
                            "image #{image_id} for pane {pane_id}: {width_cells}x{height_cells} cells"
                        );
                        let placements = self.core.image_placements.entry(pane_id).or_default();
                        placements.push(super::ClientImagePlacement {
                            image_id,
                            col,
                            row,
                            width_cells,
                            height_cells,
                            pixel_width,
                            pixel_height,
                            display_mode,
                            format,
                            data: Arc::new(data),
                        });
                        needs_redraw = true;
                    }
                    ServerEvent::Control(ServerMessage::ImageDeleted { pane_id }) => {
                        self.core.image_placements.remove(&pane_id);
                        self.invalidate_pane_images(pane_id);
                        self.invalidate_pane_cache(pane_id);
                        needs_redraw = true;
                    }
                    ServerEvent::Control(ServerMessage::TitleChanged { pane_id, title }) => {
                        if let Some(grid) = self.core.pane_grids.get_mut(&pane_id) {
                            grid.title = title;
                        }
                        needs_redraw = true;
                    }
                    ServerEvent::Control(ServerMessage::SessionList { sessions }) => {
                        log::debug!(
                            "SessionList received: {} entries: {:?}",
                            sessions.len(),
                            sessions
                                .iter()
                                .map(|s| (&s.name, s.running))
                                .collect::<Vec<_>>(),
                        );
                        // Update cache with only running sessions
                        self.core.cached_local_sessions =
                            sessions.into_iter().filter(|s| s.running).collect();
                        log::debug!(
                            "cached_local_sessions after filter: {:?}",
                            self.core
                                .cached_local_sessions
                                .iter()
                                .map(|s| &s.name)
                                .collect::<Vec<_>>(),
                        );
                        // Rebuild palette entries from cache if palette is open
                        if self.core.command_palette.is_some() {
                            self.core.rebuild_palette_entries();
                        }
                        needs_redraw = true;
                    }
                    ServerEvent::Control(ServerMessage::SessionSwitched { session_name }) => {
                        self.core.pending_session_name = Some(session_name);
                        needs_redraw = true;
                    }
                    ServerEvent::Control(ServerMessage::SessionKilled { session_name }) => {
                        // Remove killed session from cache and rebuild palette.
                        self.core
                            .cached_local_sessions
                            .retain(|s| s.name != session_name);
                        if self.core.command_palette.is_some() {
                            self.core.rebuild_palette_entries();
                        }

                        // If we were on that session, the server has already
                        // auto-switched us when possible (a SessionSwitched
                        // arrives in this batch). If it didn't (no other
                        // sessions on this server), gracefully fall back to
                        // another connection slot, otherwise detach.
                        let killed_was_ours = self.core.session_name == session_name;
                        let server_switched_us = self.core.pending_session_name.is_some();
                        if killed_was_ours && !server_switched_us {
                            if !self.core.background_slots.is_empty() {
                                let mut ids: Vec<String> =
                                    self.core.background_slots.keys().cloned().collect();
                                ids.sort();
                                let target = ids[0].clone();
                                self.switch_to_slot(&target);
                            } else {
                                log::info!(
                                    "killed session '{session_name}' was active and no fallback available — detaching"
                                );
                                self.core.should_exit = true;
                            }
                        }

                        needs_redraw = true;
                    }
                    ServerEvent::Control(ServerMessage::TemplateApplied { .. })
                    | ServerEvent::Control(ServerMessage::TemplateList { .. })
                    | ServerEvent::Control(ServerMessage::TemplateSaved { .. })
                    | ServerEvent::Control(ServerMessage::Error { .. }) => {
                        // Session/template management and IPC responses — not yet handled by GUI client
                    }
                    ServerEvent::FullPaneSync(sync) => {
                        if !self.core.expected_pane_ids.contains(&sync.meta.pane_id) {
                            continue;
                        }
                        let grid = self
                            .core
                            .pane_grids
                            .entry(sync.meta.pane_id)
                            .or_insert_with(|| {
                                ClientPaneGrid::new(
                                    sync.cols,
                                    sync.rows,
                                    self.core.config.terminal.scrollback_lines,
                                )
                            });
                        grid.apply_full_sync(&sync);
                        self.core.prediction.on_server_sync(
                            sync.meta.pane_id,
                            grid,
                            sync.meta.echo_ack,
                        );
                        self.send_lossy(ClientMessage::Ack {
                            generation: sync.meta.generation,
                        });
                        // grid.dirty is set by apply_full_sync — no need to remove cached view
                        needs_redraw = true;
                    }
                    ServerEvent::CellDelta(delta) => {
                        if !self.core.expected_pane_ids.contains(&delta.meta.pane_id) {
                            continue;
                        }
                        log::trace!(
                            "CellDelta: pane={} regions={} cursor=({},{})",
                            delta.meta.pane_id,
                            delta.regions.len(),
                            delta.meta.cursor_col,
                            delta.meta.cursor_line
                        );
                        if let Some(grid) = self.core.pane_grids.get_mut(&delta.meta.pane_id) {
                            grid.apply_delta_borrowed(&delta);
                            self.core.prediction.on_server_sync(
                                delta.meta.pane_id,
                                grid,
                                delta.meta.echo_ack,
                            );
                            self.send_lossy(ClientMessage::Ack {
                                generation: delta.meta.generation,
                            });
                            // grid.dirty is set by apply_delta_borrowed — no need to remove cached view
                            needs_redraw = true;
                        }
                    }
                    ServerEvent::Control(ServerMessage::BounceEdge { direction }) => {
                        self.core.bounce_edge(direction);
                        needs_redraw = true;
                    }
                    ServerEvent::Control(ServerMessage::Pong {
                        seq,
                        client_time_us,
                    }) => {
                        self.core.prediction.on_pong(seq, client_time_us);
                    }
                    // IPC-only responses — not relevant for the GUI client
                    ServerEvent::Control(ServerMessage::SessionInfoReply { .. })
                    | ServerEvent::Control(ServerMessage::PaneListReply { .. })
                    | ServerEvent::Control(ServerMessage::CommandResult { .. })
                    | ServerEvent::Control(ServerMessage::LayoutReply { .. })
                    | ServerEvent::Control(ServerMessage::PaneCapture { .. }) => {
                        // PaneCapture is a control-channel-only reply (sent
                        // to `__control__` clients in response to `ciritty
                        // msg capture-pane`). The GUI client should never
                        // receive one; if it does (routing bug), drop it
                        // silently — the alternative would be to surface a
                        // toast, but the control channel is the
                        // authoritative path.
                    }
                    ServerEvent::Disconnected(reason) => {
                        // If the text-input palette is still open (async failure
                        // arrived before the user closed it), mirror the reason
                        // into the footer so they see DNS/auth errors instead of
                        // the palette just vanishing.
                        if let Some(palette) = &mut self.core.command_palette
                            && palette.remote_input_mode
                        {
                            palette.remote_error =
                                Some(("Connection".to_string(), reason.to_string()));
                        }
                        // Preserve pane_grids for scrollback history — they'll be
                        // validated against server state on reconnect via FullPaneSync.
                        self.mark_disconnected_for_reconnect(reason);
                        return true;
                    }
                }
            }
            // If we consumed a full batch and still have time, loop to drain more
            // before rendering — this avoids showing intermediate states after a
            // backlog (e.g. returning from background).
            if !hit_budget || drain_start.elapsed() >= MAX_DRAIN {
                break;
            }
        } // end loop

        self.core.server_rx = Some(rx);
        needs_redraw
    }

    /// Delegate: rebuild the full 2D WorkspaceSet from the server's authoritative layout.
    pub fn apply_layout(&mut self, layout: &LayoutState) {
        self.core.apply_layout(layout);
    }

    /// (Re)load the background image into the renderer's texture slot.
    ///
    /// Spawns a worker thread to do the (potentially expensive) image
    /// decode off the main thread — a 4K JPEG is ~200ms of CPU on the
    /// decode path alone, which would visibly stall the event loop if
    /// done inline. The worker sends its result back through
    /// `pending_background_image_decode` and wakes the event loop via the
    /// proxy; `apply_pending_background_image` (called from `user_event` and
    /// at the top of each frame) drains it and pushes the RGBA bytes
    /// to the renderer.
    ///
    /// Empty path → drops any uploaded wallpaper synchronously (no
    /// decode needed) and cancels any in-flight decode.
    pub fn reload_background_image(&mut self) {
        let path_value = self.core.config.appearance.background_image.clone();
        if path_value.is_empty() {
            // Cancel any in-flight decode (its eventual result will be
            // discarded by `apply_pending_background_image`'s path check) and
            // drop any uploaded image right now.
            self.pending_background_image_decode = None;
            if let Some(renderer) = self.renderer.as_mut() {
                renderer.clear_background_image();
            }
            return;
        }
        let proxy = self.event_loop_proxy.clone();
        let (tx, rx) = crossbeam_channel::bounded(1);
        // Replacing any prior in-flight decode just drops the old
        // receiver — the worker will still finish its work and try
        // to send, but the send fails silently when nobody's listening.
        self.pending_background_image_decode = Some((path_value.clone(), rx));
        let worker_path = path_value.clone();
        std::thread::spawn(move || {
            let result = crate::app::background_image::load_background_image(&worker_path);
            let _ = tx.send(result);
            if let Some(p) = proxy {
                let _ = p.send_event(());
            }
        });
    }

    /// Drain the pending wallpaper decode (if the worker has finished)
    /// and push its result to the renderer. Returns `true` iff the
    /// renderer state changed, so the caller can schedule a redraw —
    /// without that, an idle session in overview mode would silently
    /// hold the old (or empty) wallpaper until the next user input.
    ///
    /// Called from `user_event` (when the proxy wake fires) and at the
    /// start of each `render` so a missed wake doesn't strand the
    /// upload until the next user action.
    pub fn apply_pending_background_image(&mut self) -> bool {
        let Some((path, rx)) = self.pending_background_image_decode.as_ref() else {
            return false;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(crossbeam_channel::TryRecvError::Empty) => return false,
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                // Worker dropped its sender without sending — typically
                // means the decode thread panicked (e.g. malformed image
                // tickled an `image` crate edge case). Clear the slot
                // so we don't keep polling the dead receiver forever
                // and surface a warning so the user knows their wallpaper
                // didn't load silently.
                log::warn!(
                    "background image decode worker disconnected without sending result for '{path}'"
                );
                self.pending_background_image_decode = None;
                return false;
            }
        };
        // Stale-decode guard: the user may have changed `appearance
        // .background_image` between the spawn and the
        // result arriving. Compare the path the worker decoded
        // against the current config; drop on mismatch.
        let current = &self.core.config.appearance.background_image;
        let path_owned = path.clone();
        let stale = path_owned != *current;
        self.pending_background_image_decode = None;
        if stale {
            log::debug!(
                "discarding background image decode for stale path '{path_owned}' (current '{current}')"
            );
            return false;
        }
        let Some(renderer) = self.renderer.as_mut() else {
            return false;
        };
        match result {
            Ok(Some(img)) => {
                match renderer.set_background_image(&img.rgba, img.width, img.height) {
                    Ok(()) => log::info!(
                        "background image loaded: {}x{} ({})",
                        img.width,
                        img.height,
                        path_owned
                    ),
                    Err(e) => {
                        log::warn!("background image upload failed: {e}");
                        renderer.clear_background_image();
                    }
                }
            }
            Ok(None) => renderer.clear_background_image(),
            Err(e) => {
                log::warn!("background image load failed: {e:#}");
                renderer.clear_background_image();
            }
        }
        // Drop ALL render caches. Forces a clean rebuild of the
        // scene including per-tile glyph instances and pane bg
        // entries, both of which can carry stale color-channel
        // values keyed against the previous wallpaper / opacity
        // combination. `last_render_snapshot = None` alone only
        // bypasses the frame-hash early-return; the underlying
        // tile caches stay populated and serve stale data on the
        // first post-wallpaper frame for panes that weren't
        // otherwise dirtied. The interactive opacity stepper at
        // `interaction.rs::NudgePaneOpacity` already does the same
        // full clear for the analogous reason — keep them aligned.
        self.clear_render_caches();
        true
    }

    pub fn reload_config(&mut self) {
        match ciri_config::config::CiriConfig::load() {
            Ok(new_config) => {
                let font_changed = {
                    let old = &self.core.config.font;
                    let new = &new_config.font;
                    new.family != old.family
                        || (new.size - old.size).abs() > 0.01
                        || new.features != old.features
                        || new.disable_ligatures != old.disable_ligatures
                        || new.weight != old.weight
                        || (new.adjust_cell_width - old.adjust_cell_width).abs() > f32::EPSILON
                        || (new.adjust_cell_height - old.adjust_cell_height).abs() > f32::EPSILON
                        || (new.adjust_underline_position - old.adjust_underline_position).abs()
                            > f32::EPSILON
                        || (new.adjust_underline_thickness - old.adjust_underline_thickness).abs()
                            > f32::EPSILON
                        || (new.adjust_strikethrough_position - old.adjust_strikethrough_position)
                            .abs()
                            > f32::EPSILON
                        || (new.adjust_strikethrough_thickness - old.adjust_strikethrough_thickness)
                            .abs()
                            > f32::EPSILON
                };
                let background_image_changed = self.core.config.appearance.background_image
                    != new_config.appearance.background_image;
                self.core.config = new_config;
                self.cached_color_table = ciri_render::terminal::ColorTable::new(&self.core.config);
                self.cached_resolved_theme.reload(&self.core.config.theme);
                self.reload_input_config();
                if font_changed {
                    if !self.apply_font_config_change() {
                        return;
                    }
                }
                self.clear_render_caches();
                for grid in self.core.pane_grids.values_mut() {
                    grid.dirty = true;
                }
                log::info!("config reloaded");
                self.update_prediction_config();
                if background_image_changed {
                    self.reload_background_image();
                }
            }
            Err(e) => log::warn!("config reload failed: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{AppModel, CachedTileGlyphs, ClientImagePlacement, PaletteEntryKind};
    use crate::connection::ServerEvent;
    use ciri_config::config::CiriConfig;
    use ciri_protocol::message::{
        FullPaneSync, GraphemeExtras, HyperlinkExtras, LayoutState, PackedCell, ServerMessage,
        SessionInfo, WorkspaceState,
    };

    use ciri_anim::manager::AnimationManager;
    use ciri_layout::workspace_set::WorkspaceSet;

    /// Tests that touch the global `last-session` file must hold this lock
    /// to prevent flaky parallel failures in CI.
    static LAST_SESSION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct ScopedStateHome {
        path: std::path::PathBuf,
        previous: Option<std::ffi::OsString>,
    }

    impl ScopedStateHome {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "ciri-sync-{label}-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let previous = std::env::var_os("XDG_STATE_HOME");
            unsafe {
                std::env::set_var("XDG_STATE_HOME", &path);
            }
            Self { path, previous }
        }
    }

    impl Drop for ScopedStateHome {
        fn drop(&mut self) {
            unsafe {
                match &self.previous {
                    Some(value) => std::env::set_var("XDG_STATE_HOME", value),
                    None => std::env::remove_var("XDG_STATE_HOME"),
                }
            }
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn make_app() -> App {
        App::new(CiriConfig::default(), "test-session")
    }

    fn empty_layout() -> LayoutState {
        LayoutState {
            workspaces: vec![WorkspaceState {
                columns: Vec::new(),
                active_column_idx: 0,
            }],
            active_workspace_idx: 0,
        }
    }

    fn blank_full_sync(pane_id: u64, generation: u64, title: &str) -> FullPaneSyncBorrowed {
        let sync = FullPaneSync {
            meta: PaneFrameMeta {
                pane_id,
                generation,
                cursor_line: 0,
                cursor_col: 0,
                cursor_shape: 0,
                mode_flags: 0,
                echo_ack: 0,
            },
            cols: 2,
            rows: 1,
            title: title.to_string(),
            scrollback: Vec::new(),
            scrollback_rows: 0,
            scrollback_replace: false,
            cells: vec![PackedCell::default(), PackedCell::default()],
            grapheme_extras: GraphemeExtras::new(),
            hyperlink_extras: HyperlinkExtras::new(),
            cwd: None,
        };
        ciri_protocol::codec::full_pane_sync_to_borrowed(&sync).unwrap()
    }

    #[test]
    fn session_switch_state_sync_does_not_prune_retained_client_state() {
        let _lock = LAST_SESSION_LOCK.lock().unwrap();
        let _state_home = ScopedStateHome::new("state-sync-retains-client-state");
        let mut app = make_app();
        let (event_tx, rx) = crossbeam_channel::unbounded();
        app.core.server_rx = Some(rx);

        let stale = blank_full_sync(7, 1, "stale");
        app.core.pane_grids.insert(
            stale.meta.pane_id,
            crate::grid::ClientPaneGrid::new(stale.cols, stale.rows, 100),
        );
        app.core.image_placements.insert(
            stale.meta.pane_id,
            vec![ClientImagePlacement {
                image_id: 9,
                col: 0,
                row: 0,
                width_cells: 1,
                height_cells: 1,
                pixel_width: 8,
                pixel_height: 16,
                display_mode: ImageDisplayMode::Cells,
                format: "rgba".to_string(),
                data: Arc::new(vec![1, 2, 3, 4]),
            }],
        );
        app.write_last_session();
        app.cached_tile_glyphs.insert(
            stale.meta.pane_id,
            CachedTileGlyphs {
                key: (0, 0, 0, 0),
                rows: Vec::new(),
            },
        );

        let new_sync = blank_full_sync(11, 2, "fresh");
        event_tx
            .send(ServerEvent::Control(ServerMessage::SessionSwitched {
                session_name: "other-session".to_string(),
            }))
            .unwrap();

        assert!(app.process_server_events());
        assert_eq!(app.core.session_name, "test-session");
        assert_eq!(
            app.core.pending_session_name.as_deref(),
            Some("other-session")
        );
        assert!(app.core.pane_grids.contains_key(&stale.meta.pane_id));
        assert!(app.core.image_placements.contains_key(&stale.meta.pane_id));
        assert!(app.cached_tile_glyphs.contains_key(&stale.meta.pane_id));

        event_tx
            .send(ServerEvent::Control(ServerMessage::StateSync {
                layout: empty_layout(),
                pane_ids: vec![11],
            }))
            .unwrap();

        assert!(app.process_server_events());
        assert_eq!(app.core.session_name, "other-session");
        assert_eq!(app.core.pending_session_name, None);
        assert_eq!(
            app.core
                .expected_pane_ids
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            vec![11]
        );
        assert!(app.core.pane_grids.contains_key(&stale.meta.pane_id));
        assert!(app.core.image_placements.contains_key(&stale.meta.pane_id));
        assert!(app.cached_tile_glyphs.contains_key(&stale.meta.pane_id));

        event_tx.send(ServerEvent::FullPaneSync(new_sync)).unwrap();

        assert!(app.process_server_events());
        assert!(app.core.pane_grids.contains_key(&11));
        assert!(app.core.pane_grids.contains_key(&stale.meta.pane_id));
        assert!(app.core.image_placements.contains_key(&stale.meta.pane_id));
        assert!(app.cached_tile_glyphs.contains_key(&stale.meta.pane_id));
    }

    #[test]
    fn session_switch_only_persists_last_session_after_authoritative_resync() {
        let _lock = LAST_SESSION_LOCK.lock().unwrap();
        let _state_home = ScopedStateHome::new("last-session-authoritative-resync");
        let mut app = make_app();
        let (event_tx, rx) = crossbeam_channel::unbounded();
        app.core.server_rx = Some(rx);
        let last_session_path = AppModel::last_session_path();
        if let Some(parent) = last_session_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&last_session_path, "test-session").unwrap();

        event_tx
            .send(ServerEvent::Control(ServerMessage::SessionSwitched {
                session_name: "other-session".to_string(),
            }))
            .unwrap();

        assert!(app.process_server_events());
        assert_eq!(
            app.core.pending_session_name.as_deref(),
            Some("other-session")
        );
        event_tx
            .send(ServerEvent::Control(ServerMessage::StateSync {
                layout: empty_layout(),
                pane_ids: vec![11],
            }))
            .unwrap();
        event_tx
            .send(ServerEvent::FullPaneSync(blank_full_sync(11, 1, "fresh")))
            .unwrap();

        assert!(app.process_server_events());
        assert_eq!(app.core.pending_session_name, None);
        assert_eq!(
            std::fs::read_to_string(&last_session_path).unwrap(),
            "other-session"
        );
    }

    #[test]
    fn image_deleted_event_clears_client_placements() {
        let mut app = make_app();
        let (event_tx, rx) = crossbeam_channel::unbounded();
        app.core.server_rx = Some(rx);
        app.core.image_placements.insert(
            7,
            vec![ClientImagePlacement {
                image_id: 9,
                col: 0,
                row: 0,
                width_cells: 1,
                height_cells: 1,
                pixel_width: 8,
                pixel_height: 16,
                display_mode: ImageDisplayMode::Cells,
                format: "rgba".to_string(),
                data: Arc::new(vec![1, 2, 3, 4]),
            }],
        );

        event_tx
            .send(ServerEvent::Control(ServerMessage::ImageDeleted {
                pane_id: 7,
            }))
            .unwrap();

        assert!(app.process_server_events());
        assert!(!app.core.image_placements.contains_key(&7));
    }

    /// 创建一个 mock ConnectionSlot 用于后台 slot 测试
    pub(crate) fn make_bg_slot(
        id: &str,
        kind: super::super::ConnectionKind,
        session_name: &str,
        connected: bool,
    ) -> (
        super::super::ConnectionSlot,
        crossbeam_channel::Sender<ServerEvent>,
        crossbeam_channel::Receiver<ClientMessage>,
    ) {
        let (event_tx, event_rx) = crossbeam_channel::unbounded();
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
        let view = ciri_layout::geometry::ViewSize {
            width: 800.0,
            height: 600.0,
        };
        let slot = super::super::ConnectionSlot {
            id: id.to_string(),
            kind,
            session_name: session_name.to_string(),
            server_tx: cmd_tx,
            server_rx: event_rx,
            pane_grids: std::collections::HashMap::new(),
            workspaces: WorkspaceSet::new(view),
            expected_pane_ids: std::collections::HashSet::new(),
            connected,
            reconnect_state: None,
            pending_session_name: None,
            anim_mgr: AnimationManager::new(),
            workspace_last_pane_ids: std::collections::HashMap::new(),
            selection: None,
            broadcast_mode: false,
            image_placements: std::collections::HashMap::new(),
            pending_events: std::collections::VecDeque::new(),
        };
        (slot, event_tx, cmd_rx)
    }

    // -----------------------------------------------------------------------
    // cycle_session 测试
    // -----------------------------------------------------------------------

    /// Regression: server's SessionList comes sorted by last_attached, so the
    /// session you just switched to bubbles to the front. With order-sensitive
    /// cycling, pressing `i` after each switch would oscillate between the two
    /// most-recent sessions instead of advancing to the next slot. Cycle must
    /// use a stable order (sorted by name) regardless of cache order.
    #[test]
    fn cycle_session_advances_even_when_cache_reorders_after_switch() {
        let mut app = make_app();
        app.core.active_slot_id = "remote".to_string();
        app.core.session_name = "alpha".to_string();
        // Initial server response: alpha was attached most recently.
        app.core.cached_local_sessions = vec![
            SessionInfo {
                name: "alpha".into(),
                running: true,
                pane_count: 1,
                client_count: 1,
            },
            SessionInfo {
                name: "beta".into(),
                running: true,
                pane_count: 1,
                client_count: 1,
            },
        ];
        let (local_slot, _ev, _local_cmd_rx) =
            make_bg_slot("local", super::super::ConnectionKind::Local, "main", true);
        app.core.background_slots.insert("local".into(), local_slot);
        let (cmd_tx, _remote_cmd_rx) = crossbeam_channel::unbounded();
        app.core.server_tx = Some(cmd_tx);
        let (_ev_tx, server_rx) = crossbeam_channel::unbounded();
        app.core.server_rx = Some(server_rx);

        // Press 1: alpha → beta
        app.cycle_session(1);
        // Simulate server confirming the switch + sending a fresh SessionList
        // ordered by last_attached: beta is now first.
        app.core.session_name = "beta".to_string();
        app.core.pending_session_name = None;
        app.core.cached_local_sessions = vec![
            SessionInfo {
                name: "beta".into(),
                running: true,
                pane_count: 1,
                client_count: 1,
            },
            SessionInfo {
                name: "alpha".into(),
                running: true,
                pane_count: 1,
                client_count: 1,
            },
        ];

        // Press 2: must advance to the local slot — NOT loop back to alpha.
        app.cycle_session(1);
        assert_eq!(
            app.core.active_slot_id, "local",
            "press 2 must reach local; instead stayed on {}",
            app.core.active_slot_id
        );
    }

    #[test]
    fn cycle_session_reaches_bg_slot_after_traversing_remote_sessions() {
        // Setup: on remote with multi-session cache, local in bg.
        let mut app = make_app();
        app.core.active_slot_id = "remote:host:7890".to_string();
        app.core.session_name = "alpha".to_string();
        app.core.cached_local_sessions = vec![
            SessionInfo {
                name: "alpha".into(),
                running: true,
                pane_count: 1,
                client_count: 1,
            },
            SessionInfo {
                name: "beta".into(),
                running: true,
                pane_count: 1,
                client_count: 1,
            },
            SessionInfo {
                name: "gamma".into(),
                running: true,
                pane_count: 1,
                client_count: 1,
            },
        ];
        let (local_slot, _ev, local_cmd_rx) =
            make_bg_slot("default", super::super::ConnectionKind::Local, "main", true);
        app.core
            .background_slots
            .insert("default".into(), local_slot);
        // Hook current "remote" connection so cycle_session's SwitchSession
        // sends land somewhere (we drain to verify).
        let (cmd_tx, remote_cmd_rx) = crossbeam_channel::unbounded();
        app.core.server_tx = Some(cmd_tx);
        let (_ev_tx, server_rx) = crossbeam_channel::unbounded();
        app.core.server_rx = Some(server_rx);

        // Press 1: alpha → beta (within remote)
        app.cycle_session(1);
        let m1 = remote_cmd_rx.try_recv().unwrap();
        assert!(
            matches!(m1, ClientMessage::SwitchSession { ref session_name } if session_name == "beta"),
            "press 1 should send SwitchSession(beta), got {:?}",
            m1
        );

        // Press 2: beta → gamma (within remote, using `pending` to advance)
        app.cycle_session(1);
        let m2 = remote_cmd_rx.try_recv().unwrap();
        assert!(
            matches!(m2, ClientMessage::SwitchSession { ref session_name } if session_name == "gamma"),
            "press 2 should send SwitchSession(gamma), got {:?}",
            m2
        );

        // Press 3: gamma → (default, main) — CROSS-SLOT SWITCH expected.
        app.cycle_session(1);
        assert_eq!(
            app.core.active_slot_id, "default",
            "press 3 should switch to local slot, but active_slot_id={}",
            app.core.active_slot_id
        );
        assert_eq!(
            app.core.session_name, "main",
            "after switching to local slot, session_name should be the slot's saved name",
        );
        // No further SwitchSession should have been sent on press 3 since
        // the local slot's session already matches the cycle target.
        let _ = local_cmd_rx; // keep alive so channels don't drop
    }

    // -----------------------------------------------------------------------
    // buffered_events 机制测试
    // -----------------------------------------------------------------------

    #[test]
    fn process_server_events_drains_buffered_events_first() {
        // buffered_events 中的事件应在 server_rx 事件之前被处理
        let mut app = make_app();
        let (event_tx, rx) = crossbeam_channel::unbounded();
        app.core.server_rx = Some(rx);

        // 在 server_rx 中放入 PaneCreated(42)
        event_tx
            .send(ServerEvent::Control(ServerMessage::PaneCreated {
                pane_id: 42,
                column_idx: 0,
                cols: 80,
                rows: 24,
            }))
            .unwrap();

        // 在 buffered_events 中放入 PaneCreated(99)（应先被处理）
        app.core
            .buffered_events
            .push_back(ServerEvent::Control(ServerMessage::PaneCreated {
                pane_id: 99,
                column_idx: 0,
                cols: 80,
                rows: 24,
            }));

        app.process_server_events();

        // 两个事件都应被处理（buffered 的先，但两者最终都到位）
        assert!(
            app.core.pane_grids.contains_key(&99),
            "buffered_events 中的 PaneCreated 应被处理"
        );
        assert!(
            app.core.pane_grids.contains_key(&42),
            "server_rx 中的 PaneCreated 也应被处理"
        );
        assert!(
            app.core.buffered_events.is_empty(),
            "处理后 buffered_events 应为空"
        );
    }

    #[test]
    fn process_server_events_empty_buffered_events_unchanged() {
        // buffered_events 为空时行为不变
        let mut app = make_app();
        let (event_tx, rx) = crossbeam_channel::unbounded();
        app.core.server_rx = Some(rx);

        assert!(app.core.buffered_events.is_empty());

        event_tx
            .send(ServerEvent::Control(ServerMessage::PaneCreated {
                pane_id: 42,
                column_idx: 0,
                cols: 80,
                rows: 24,
            }))
            .unwrap();

        let needs_redraw = app.process_server_events();
        assert!(needs_redraw);
        assert!(app.core.pane_grids.contains_key(&42));
    }

    #[test]
    fn buffered_events_order_preserved_before_server_rx() {
        // buffered_events 中的事件顺序在 server_rx 事件之前
        let mut app = make_app();
        let (event_tx, rx) = crossbeam_channel::unbounded();
        app.core.server_rx = Some(rx);

        // buffered: 创建 pane 50
        app.core
            .buffered_events
            .push_back(ServerEvent::Control(ServerMessage::PaneCreated {
                pane_id: 50,
                column_idx: 0,
                cols: 80,
                rows: 24,
            }));
        // buffered: 创建 pane 51
        app.core
            .buffered_events
            .push_back(ServerEvent::Control(ServerMessage::PaneCreated {
                pane_id: 51,
                column_idx: 0,
                cols: 80,
                rows: 24,
            }));

        // server_rx: 创建 pane 60
        event_tx
            .send(ServerEvent::Control(ServerMessage::PaneCreated {
                pane_id: 60,
                column_idx: 0,
                cols: 80,
                rows: 24,
            }))
            .unwrap();

        app.process_server_events();

        // 所有三个 pane 都应该存在
        assert!(app.core.pane_grids.contains_key(&50));
        assert!(app.core.pane_grids.contains_key(&51));
        assert!(app.core.pane_grids.contains_key(&60));
    }

    // -----------------------------------------------------------------------
    // save/restore slot 测试
    // -----------------------------------------------------------------------

    #[test]
    fn save_current_to_slot_preserves_buffered_events_as_pending() {
        // save_current_to_slot 将 buffered_events 保存到 pending_events
        let mut app = make_app();
        let (tx, _rx_cmd) = crossbeam_channel::unbounded();
        let (_ev_tx, rx) = crossbeam_channel::unbounded();
        app.core.server_tx = Some(tx);
        app.core.server_rx = Some(rx);
        app.core.session_name = "slot-session".to_string();

        // 放入一些 buffered events
        app.core
            .buffered_events
            .push_back(ServerEvent::Control(ServerMessage::PaneCreated {
                pane_id: 100,
                column_idx: 0,
                cols: 80,
                rows: 24,
            }));
        app.core
            .buffered_events
            .push_back(ServerEvent::Control(ServerMessage::Bell { pane_id: 100 }));

        let slot = app.save_current_to_slot().expect("应成功创建 slot");

        assert_eq!(
            slot.pending_events.len(),
            2,
            "buffered_events 应转移到 pending_events"
        );
        assert!(
            app.core.buffered_events.is_empty(),
            "save 后 buffered_events 应为空"
        );
    }

    #[test]
    fn restore_from_slot_puts_pending_events_into_buffered() {
        // restore_from_slot 将 pending_events 放入 buffered_events
        let mut app = make_app();

        let (slot_tx, _slot_cmd_rx) = crossbeam_channel::unbounded();
        let (_slot_ev_tx, slot_rx) = crossbeam_channel::unbounded();
        let view = ciri_layout::geometry::ViewSize {
            width: 800.0,
            height: 600.0,
        };

        let mut pending = std::collections::VecDeque::new();
        pending.push_back(ServerEvent::Control(ServerMessage::PaneCreated {
            pane_id: 200,
            column_idx: 0,
            cols: 80,
            rows: 24,
        }));

        let slot = super::super::ConnectionSlot {
            id: "restored-slot".to_string(),
            kind: super::super::ConnectionKind::Local,
            session_name: "restored".to_string(),
            server_tx: slot_tx,
            server_rx: slot_rx,
            pane_grids: std::collections::HashMap::new(),
            workspaces: WorkspaceSet::new(view),
            expected_pane_ids: std::collections::HashSet::new(),
            connected: true,
            reconnect_state: None,
            pending_session_name: None,
            anim_mgr: ciri_anim::manager::AnimationManager::new(),
            workspace_last_pane_ids: std::collections::HashMap::new(),
            selection: None,
            broadcast_mode: false,
            image_placements: std::collections::HashMap::new(),
            pending_events: pending,
        };

        assert!(app.core.buffered_events.is_empty());
        app.restore_from_slot(slot);

        assert_eq!(
            app.core.buffered_events.len(),
            1,
            "pending_events 应放入 buffered_events"
        );
    }

    #[test]
    fn finalize_authoritative_session_switch_clears_slot_query_state() {
        // finalize_authoritative_session_switch 清理 slot_session_pending 和 query_start
        let _lock = LAST_SESSION_LOCK.lock().unwrap();
        let _state_home = ScopedStateHome::new("finalize-authoritative-session-switch");
        let mut app = make_app();
        let (event_tx, rx) = crossbeam_channel::unbounded();
        app.core.server_rx = Some(rx);

        // 设置 slot 查询状态
        app.core.slot_session_pending.insert("slot-a".into());
        app.core.slot_session_pending.insert("slot-b".into());
        app.core.slot_session_query_start = Some(std::time::Instant::now());

        // 触发 session switch 完成流程
        event_tx
            .send(ServerEvent::Control(ServerMessage::SessionSwitched {
                session_name: "new-session".to_string(),
            }))
            .unwrap();
        event_tx
            .send(ServerEvent::Control(ServerMessage::StateSync {
                layout: empty_layout(),
                pane_ids: vec![30],
            }))
            .unwrap();

        app.process_server_events();

        assert!(
            app.core.slot_session_pending.is_empty(),
            "finalize 后 slot_session_pending 应为空"
        );
        assert!(
            app.core.slot_session_query_start.is_none(),
            "finalize 后 slot_session_query_start 应为 None"
        );
    }

    // -----------------------------------------------------------------------
    // poll_slot_sessions 测试
    // -----------------------------------------------------------------------

    #[test]
    fn poll_slot_sessions_collects_session_list_response() {
        // 成功收到 SessionList 响应时添加 SlotSession 条目
        let mut app = make_app();
        let (tx, _cmd_rx) = crossbeam_channel::unbounded();
        app.core.server_tx = Some(tx);

        // 创建后台 slot 并设为 pending
        let (slot, ev_tx, _cmd_rx) =
            make_bg_slot("bg-slot", super::super::ConnectionKind::Local, "s", true);
        app.core.background_slots.insert("bg-slot".into(), slot);
        app.core.slot_session_pending.insert("bg-slot".into());

        // 打开 session palette
        app.core.open_session_palette();

        // 模拟 slot 返回 SessionList
        ev_tx
            .send(ServerEvent::Control(ServerMessage::SessionList {
                sessions: vec![SessionInfo {
                    name: "alpha".into(),
                    running: true,
                    pane_count: 1,
                    client_count: 1,
                }],
            }))
            .unwrap();

        app.poll_slot_sessions();

        // 应已移出 pending
        assert!(!app.core.slot_session_pending.contains("bg-slot"));

        // 应有 background slot 的 GoToSession 条目
        let palette = app.core.command_palette.as_ref().unwrap();
        let slot_sessions: Vec<_> = palette
            .entries
            .iter()
            .filter(|e| matches!(&e.kind, PaletteEntryKind::GoToSession { slot_id, .. } if slot_id == "bg-slot"))
            .collect();
        assert!(
            !slot_sessions.is_empty(),
            "应有 bg-slot 的 GoToSession 条目"
        );
    }

    #[test]
    fn poll_slot_sessions_buffers_non_session_list_events() {
        // 非 SessionList 事件应被缓冲到 slot.pending_events
        let mut app = make_app();
        let (tx, _cmd_rx) = crossbeam_channel::unbounded();
        app.core.server_tx = Some(tx);

        let (slot, ev_tx, _cmd_rx) =
            make_bg_slot("bg-slot2", super::super::ConnectionKind::Local, "s", true);
        app.core.background_slots.insert("bg-slot2".into(), slot);
        app.core.slot_session_pending.insert("bg-slot2".into());

        app.core.open_session_palette();

        // 发送一个非 SessionList 事件，然后是 SessionList
        ev_tx
            .send(ServerEvent::Control(ServerMessage::PaneCreated {
                pane_id: 777,
                column_idx: 0,
                cols: 80,
                rows: 24,
            }))
            .unwrap();
        ev_tx
            .send(ServerEvent::Control(ServerMessage::SessionList {
                sessions: vec![],
            }))
            .unwrap();

        app.poll_slot_sessions();

        // PaneCreated 应被缓冲
        let slot = app.core.background_slots.get("bg-slot2").unwrap();
        assert_eq!(
            slot.pending_events.len(),
            1,
            "非 SessionList 事件应被缓冲到 pending_events"
        );
    }

    #[test]
    fn poll_slot_sessions_handles_disconnected_channel() {
        // channel 断连时应从 pending 移除
        let mut app = make_app();
        let (tx, _cmd_rx) = crossbeam_channel::unbounded();
        app.core.server_tx = Some(tx);

        let (slot, ev_tx, _cmd_rx) =
            make_bg_slot("dead-slot", super::super::ConnectionKind::Local, "s", true);
        app.core.background_slots.insert("dead-slot".into(), slot);
        app.core.slot_session_pending.insert("dead-slot".into());

        app.core.open_session_palette();

        // 关闭发送端模拟断连
        drop(ev_tx);

        app.poll_slot_sessions();

        assert!(
            !app.core.slot_session_pending.contains("dead-slot"),
            "断连的 slot 应从 pending 移除"
        );
    }

    #[test]
    fn poll_slot_sessions_removed_slot_cleaned_up() {
        // slot 被移除后 pending 中的条目应被清理
        let mut app = make_app();
        let (tx, _cmd_rx) = crossbeam_channel::unbounded();
        app.core.server_tx = Some(tx);

        // 只添加到 pending，不添加到 background_slots
        app.core.slot_session_pending.insert("ghost-slot".into());

        // 需要 palette 存在
        app.core.open_session_palette();

        app.poll_slot_sessions();

        assert!(
            !app.core.slot_session_pending.contains("ghost-slot"),
            "不存在的 slot 应从 pending 移除"
        );
    }

    // -----------------------------------------------------------------------
    // SessionList 响应处理（完整 App 层面）
    // -----------------------------------------------------------------------

    #[test]
    fn session_list_in_sessions_only_keeps_slot_session_entries() {
        // sessions_only 模式下 SessionList 响应保留 SlotSession 条目
        let mut app = make_app();
        let (event_tx, rx) = crossbeam_channel::unbounded();
        let (cmd_tx, _cmd_rx) = crossbeam_channel::unbounded();
        app.core.server_rx = Some(rx);
        app.core.server_tx = Some(cmd_tx);

        // 创建后台 slot
        let (slot, _ev, _cmd) =
            make_bg_slot("bg-s", super::super::ConnectionKind::Local, "sess", true);
        app.core.background_slots.insert("bg-s".into(), slot);

        // 打开 session palette 并添加 SlotSession
        app.core.open_session_palette();
        app.core.apply_slot_session_result(
            "bg-s",
            vec![SessionInfo {
                name: "bg-alpha".into(),
                running: true,
                pane_count: 1,
                client_count: 1,
            }],
        );

        // 通过 server_rx 发送 SessionList
        event_tx
            .send(ServerEvent::Control(ServerMessage::SessionList {
                sessions: vec![SessionInfo {
                    name: "local-main".into(),
                    running: true,
                    pane_count: 2,
                    client_count: 1,
                }],
            }))
            .unwrap();

        app.process_server_events();

        let palette = app.core.command_palette.as_ref().unwrap();
        let active = app.core.active_slot_id.clone();
        // bg-s slot 的 GoToSession 应保留
        let bg_count = palette
            .entries
            .iter()
            .filter(|e| matches!(&e.kind, PaletteEntryKind::GoToSession { slot_id, .. } if slot_id == "bg-s"))
            .count();
        assert!(bg_count >= 1, "SessionList 后 bg-s 的 GoToSession 应保留");

        // 当前 slot 的 GoToSession 也应存在
        let current_count = palette
            .entries
            .iter()
            .filter(|e| matches!(&e.kind, PaletteEntryKind::GoToSession { slot_id, .. } if slot_id == &active))
            .count();
        assert!(
            current_count >= 1,
            "SessionList 后当前 slot 的 GoToSession 应存在"
        );
    }

    #[test]
    fn stale_frame_for_old_session_is_dropped_after_session_switch() {
        let _lock = LAST_SESSION_LOCK.lock().unwrap();
        let _state_home = ScopedStateHome::new("stale-frame-session-switch");
        let mut app = make_app();
        let (event_tx, rx) = crossbeam_channel::unbounded();
        app.core.server_rx = Some(rx);

        let stale = blank_full_sync(7, 1, "stale");
        let fresh = blank_full_sync(11, 2, "fresh");

        event_tx
            .send(ServerEvent::Control(ServerMessage::SessionSwitched {
                session_name: "other-session".to_string(),
            }))
            .unwrap();
        event_tx
            .send(ServerEvent::Control(ServerMessage::StateSync {
                layout: LayoutState {
                    workspaces: vec![WorkspaceState {
                        columns: vec![ColumnState {
                            tiles: vec![TileState {
                                pane_id: fresh.meta.pane_id,
                                weight: 1.0,
                            }],
                            active_tile_idx: 0,
                            width_proportion: 1.0,
                            width_fixed_px: None,
                        }],
                        active_column_idx: 0,
                    }],
                    active_workspace_idx: 0,
                },
                pane_ids: vec![fresh.meta.pane_id],
            }))
            .unwrap();
        event_tx.send(ServerEvent::FullPaneSync(fresh)).unwrap();
        event_tx.send(ServerEvent::FullPaneSync(stale)).unwrap();

        assert!(app.process_server_events());
        assert_eq!(app.core.session_name, "other-session");
        assert!(app.core.pane_grids.contains_key(&11));
        assert!(!app.core.pane_grids.contains_key(&7));
    }
}
