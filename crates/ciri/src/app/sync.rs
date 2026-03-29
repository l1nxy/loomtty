use ciri_layout::column::ColumnWidth;
use ciri_layout::workspace::Workspace;
use ciri_protocol::message::*;

use super::App;
use crate::connection::ServerEvent;
use crate::grid::ClientPaneGrid;

use ciri_layout::column::Column;
use ciri_layout::tile::Tile;

impl App {
    fn finalize_authoritative_session_switch(&mut self, pane_ids: &[u64]) {
        let Some(session_name) = self.pending_session_name.take() else {
            return;
        };

        self.session_name = session_name;
        self.expected_pane_ids = pane_ids.iter().copied().collect();
        self.write_last_session();
        self.command_palette = None;
        if let Some(window) = &self.window {
            window.set_title(&format!(
                "{} [{}]",
                self.config.window.title, self.session_name
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
        let Some(rx) = self.server_rx.take() else {
            return false;
        };
        const BATCH: usize = 200;
        const MAX_DRAIN: std::time::Duration = std::time::Duration::from_millis(50);
        let drain_start = std::time::Instant::now();
        let mut needs_redraw = false;

        loop {
            let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
                .take(BATCH)
                .collect();
            let hit_budget = events.len() >= BATCH;
            if events.is_empty() {
                break;
            }

            for event in events {
                match event {
                    ServerEvent::Control(ServerMessage::StateSync { layout, pane_ids }) => {
                        self.finalize_authoritative_session_switch(&pane_ids);
                        self.expected_pane_ids = pane_ids.iter().copied().collect();
                        self.apply_layout(&layout);
                        for &id in &pane_ids {
                            self.pane_grids.entry(id).or_insert_with(|| {
                                ClientPaneGrid::new(80, 24, self.config.terminal.scrollback_lines)
                            });
                            self.anim_mgr.ensure_pane_registered(id);
                        }
                        self.connected = true;
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
                        // Cancel any active tile drag — layout indices may have changed
                        self.drag.tile_dragging = None;
                        needs_redraw = true;
                    }
                    ServerEvent::Control(ServerMessage::PaneCreated {
                        pane_id,
                        cols,
                        rows,
                        ..
                    }) => {
                        log::debug!("PaneCreated: pane_id={pane_id} {cols}x{rows}");
                        self.expected_pane_ids.insert(pane_id);
                        self.pane_grids.entry(pane_id).or_insert_with(|| {
                            ClientPaneGrid::new(cols, rows, self.config.terminal.scrollback_lines)
                        });
                        let params = self.anim_config();
                        self.anim_mgr.on_pane_created(pane_id, &params);
                        needs_redraw = true;
                    }
                    ServerEvent::Control(ServerMessage::PaneClosed { pane_id }) => {
                        self.expected_pane_ids.remove(&pane_id);
                        log::debug!("PaneClosed: pane_id={pane_id}");
                        // Capture pane rect for close animation before removing
                        let vox = self.anim_mgr.view_offset_x.value() as f32;
                        let voy = self.anim_mgr.view_offset_y.value() as f32;
                        let tiles = self.workspaces.visible_tiles_2d(vox, voy);
                        let params = self.anim_config();
                        if let Some((_, rect, _)) = tiles.iter().find(|(pid, _, _)| *pid == pane_id)
                        {
                            let geo = ciri_anim::manager::GeoRect {
                                x: rect.x, y: rect.y, w: rect.w, h: rect.h,
                            };
                            self.anim_mgr.on_pane_closed(pane_id, geo, &params);
                        } else {
                            // Off-screen pane: just remove state, no close animation
                            self.anim_mgr.on_pane_closed(
                                pane_id,
                                ciri_anim::manager::GeoRect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 },
                                &params,
                            );
                        }
                        self.pane_grids.remove(&pane_id);
                        self.invalidate_pane_cache(pane_id);
                        needs_redraw = true;
                    }
                    ServerEvent::Control(ServerMessage::ServerShutdown) => {
                        log::info!("server shut down, exiting");
                        self.should_exit = true;
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
                        self.anim_mgr.on_bell(pane_id, &params);

                        // Window urgency hint
                        if self.config.terminal.bell_urgency && !self.window_focused {
                            if let Some(ref window) = self.window {
                                window.request_user_attention(Some(
                                    winit::window::UserAttentionType::Informational,
                                ));
                            }
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
                        let threshold = self.config.terminal.notify_command_threshold_secs;
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
                    ServerEvent::Control(ServerMessage::ImagePlacement {
                        pane_id,
                        image_id,
                        col,
                        row,
                        width_cells,
                        height_cells,
                        pixel_width,
                        pixel_height,
                        ..
                    }) => {
                        log::debug!(
                            "image #{image_id} for pane {pane_id}: {width_cells}x{height_cells} cells"
                        );
                        let placements = self.image_placements.entry(pane_id).or_default();
                        placements.push(super::ClientImagePlacement {
                            image_id,
                            col,
                            row,
                            width_cells,
                            height_cells,
                            pixel_width,
                            pixel_height,
                        });
                        needs_redraw = true;
                    }
                    ServerEvent::Control(ServerMessage::SessionList { sessions }) => {
                        if let Some(palette) = &mut self.command_palette {
                            if palette.sessions_only {
                                palette.entries.clear();
                            } else {
                                // Remove old session entries, keep actions.
                                palette.entries.retain(|e| {
                                    matches!(e.kind, super::PaletteEntryKind::Action(_))
                                });
                            }
                            for s in &sessions {
                                palette.entries.push(super::PaletteEntry {
                                    label: format!("Switch to: {}", s.name),
                                    kind: super::PaletteEntryKind::SwitchSession(s.name.clone()),
                                });
                                if !palette.sessions_only && s.running {
                                    palette.entries.push(super::PaletteEntry {
                                        label: format!("Kill: {}", s.name),
                                        kind: super::PaletteEntryKind::KillSession(s.name.clone()),
                                    });
                                }
                            }
                            self.filter_palette();
                        }
                        needs_redraw = true;
                    }
                    ServerEvent::Control(ServerMessage::SessionSwitched { session_name }) => {
                        self.pending_session_name = Some(session_name);
                        needs_redraw = true;
                    }
                    ServerEvent::Control(ServerMessage::SessionKilled { .. })
                    | ServerEvent::Control(ServerMessage::TemplateApplied { .. })
                    | ServerEvent::Control(ServerMessage::TemplateList { .. })
                    | ServerEvent::Control(ServerMessage::TemplateSaved { .. })
                    | ServerEvent::Control(ServerMessage::Error { .. }) => {
                        // Session/template management and IPC responses — not yet handled by GUI client
                    }
                    ServerEvent::FullPaneSync(sync) => {
                        if !self.expected_pane_ids.contains(&sync.pane_id) {
                            continue;
                        }
                        let grid = self.pane_grids.entry(sync.pane_id).or_insert_with(|| {
                            ClientPaneGrid::new(
                                sync.cols,
                                sync.rows,
                                self.config.terminal.scrollback_lines,
                            )
                        });
                        grid.apply_full_sync(&sync);
                        self.send_lossy(ClientMessage::Ack {
                            generation: sync.generation,
                        });
                        // grid.dirty is set by apply_full_sync — no need to remove cached view
                        needs_redraw = true;
                    }
                    ServerEvent::CellDelta(delta) => {
                        if !self.expected_pane_ids.contains(&delta.pane_id) {
                            continue;
                        }
                        log::trace!(
                            "CellDelta: pane={} regions={} cursor=({},{})",
                            delta.pane_id,
                            delta.regions.len(),
                            delta.cursor_col,
                            delta.cursor_line
                        );
                        if let Some(grid) = self.pane_grids.get_mut(&delta.pane_id) {
                            grid.apply_delta_borrowed(&delta);
                            self.send_lossy(ClientMessage::Ack {
                                generation: delta.generation,
                            });
                            // grid.dirty is set by apply_delta_borrowed — no need to remove cached view
                            needs_redraw = true;
                        }
                    }
                    // IPC-only responses — not relevant for the GUI client
                    ServerEvent::Control(ServerMessage::SessionInfoReply { .. })
                    | ServerEvent::Control(ServerMessage::PaneListReply { .. })
                    | ServerEvent::Control(ServerMessage::CommandResult { .. })
                    | ServerEvent::Control(ServerMessage::LayoutReply { .. }) => {}
                    ServerEvent::Disconnected => {
                        // Preserve pane_grids for scrollback history — they'll be
                        // validated against server state on reconnect via FullPaneSync.
                        self.mark_disconnected_for_reconnect();
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

        self.server_rx = Some(rx);
        needs_redraw
    }

    /// Snapshot current pane screen positions for move animation.
    /// Uses animation target (not in-flight value) for stable snapshots.
    fn snapshot_pane_positions(&self) -> std::collections::HashMap<u64, (f32, f32)> {
        let vox = self.anim_mgr.view_offset_x.target() as f32;
        let voy = self.anim_mgr.view_offset_y.target() as f32;
        let tiles = self.workspaces.visible_tiles_2d(vox, voy);
        tiles
            .into_iter()
            .map(|(pid, rect, _)| (pid, (rect.x, rect.y)))
            .collect()
    }

    /// Rebuild the full 2D WorkspaceSet from the server's authoritative layout.
    pub fn apply_layout(&mut self, layout: &LayoutState) {
        log::debug!(
            "apply_layout: view_size={:?}, vox={:.1}, voy={:.1}",
            self.workspaces.view_size,
            self.anim_mgr.view_offset_x.value(),
            self.anim_mgr.view_offset_y.value()
        );

        // Snapshot old pane positions for move animation
        let old_positions = self.snapshot_pane_positions();

        let view_size = self.workspaces.view_size;
        let column_gap = self.workspaces.column_gap;

        let mut new_workspaces: Vec<Workspace> = layout
            .workspaces
            .iter()
            .map(|ws_state| {
                let mut ws = Workspace::new_with_gap(view_size, column_gap);
                for col_state in &ws_state.columns {
                    if let Some(first_tile) = col_state.tiles.first() {
                        let mut col = Column::new(first_tile.pane_id);
                        // Replace the default single tile with all tiles from state
                        col.tiles = col_state
                            .tiles
                            .iter()
                            .map(|t| {
                                let mut tile = Tile::new(t.pane_id);
                                tile.height = ciri_layout::tile::TileHeight::Auto {
                                    weight: t.weight as f64,
                                };
                                tile
                            })
                            .collect();
                        col.active_tile_idx = col_state
                            .active_tile_idx
                            .min(col.tiles.len().saturating_sub(1));
                        col.width = if let Some(px) = col_state.width_fixed_px {
                            ColumnWidth::Fixed(px)
                        } else {
                            ColumnWidth::Proportion(col_state.width_proportion)
                        };
                        ws.columns.push(col);
                    }
                }
                ws.active_column_idx = ws_state
                    .active_column_idx
                    .min(ws.columns.len().saturating_sub(1));
                ws
            })
            .collect();

        if new_workspaces.is_empty() {
            new_workspaces.push(Workspace::new_with_gap(view_size, column_gap));
        }

        self.workspaces.workspaces = new_workspaces;
        self.workspaces.active_workspace_idx = layout
            .active_workspace_idx
            .min(self.workspaces.workspaces.len().saturating_sub(1));
        self.sync_workspace_pane_memory();

        self.snap_all_col_widths();

        // Compute new positions and start move animations for shifted panes
        let new_positions = self.snapshot_pane_positions();
        if self.config.animation.enabled {
            let config = self.anim_config();
            for (pane_id, (old_x, old_y)) in &old_positions {
                if let Some(&(new_x, new_y)) = new_positions.get(pane_id) {
                    let dx = old_x - new_x;
                    let dy = old_y - new_y;
                    if dx.abs() > 1.0 || dy.abs() > 1.0 {
                        self.anim_mgr.ensure_pane_registered(*pane_id);
                        self.anim_mgr.start_move_animation(*pane_id, dx, dy, &config);
                    }
                }
            }
        }

        self.animate_to_active();
    }

    pub fn reload_config(&mut self) {
        match ciri_config::config::CiriConfig::load() {
            Ok(new_config) => {
                let font_changed = new_config.font.family != self.config.font.family
                    || (new_config.font.size - self.config.font.size).abs() > 0.01;
                self.config = new_config;
                self.cached_color_table = ciri_render::terminal::ColorTable::new(&self.config);
                self.input.reload_bindings(
                    &self.config.keys.leader,
                    match self.config.input.mode {
                        ciri_config::config::InputMode::Prefix => "prefix",
                        ciri_config::config::InputMode::Sticky => "sticky",
                    },
                    &self.config.keys.bindings,
                    &self.config.keys.modes,
                    &self.config.keys.direct_bindings,
                );
                Self::rebuild_binding_set(&mut self.input, &self.config);
                if font_changed {
                    self.destroy_gpu_resources();
                    if let Some(renderer) = &mut self.renderer {
                        let shaper = ciri_render::shaper::TextShaper::new(&self.config.font.family);
                        let (cache, atlas_gpu) = renderer.create_atlas(
                            self.config.font.size,
                            self.dpi_scale,
                            &self.config.font.family,
                            shaper.primary_font_path(),
                            shaper.emoji_font_path(),
                            shaper.emoji_font_id(),
                            shaper.cjk_font_path(),
                            shaper.cjk_font_id(),
                            &self.config.render,
                        );
                        self.glyph_cache = Some(cache);
                        self.glyph_atlas_gpu = Some(atlas_gpu);
                        self.text_shaper = Some(shaper);
                    }
                }
                self.cached_views.clear();
                self.cached_tile_glyphs.clear();
                for grid in self.pane_grids.values_mut() {
                    grid.dirty = true;
                }
                // Notify server of new cell dimensions after font change
                if font_changed {
                    let (cw, ch) = self.cell_dimensions();
                    let view = &self.workspaces.view_size;
                    self.send(ClientMessage::Resize {
                        cols: 0,
                        rows: 0,
                        width: view.width as u32,
                        height: view.height as u32,
                        cell_width: cw,
                        cell_height: ch,
                    });
                }
                log::info!("config reloaded");
            }
            Err(e) => log::warn!("config reload failed: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{CachedTileGlyphs, ClientImagePlacement};
    use crate::connection::ServerEvent;
    use ciri_config::config::CiriConfig;
    use ciri_protocol::message::{
        FullPaneSync, GraphemeExtras, HyperlinkExtras, LayoutState, PackedCell, ServerMessage,
        WorkspaceState,
    };

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

    fn blank_full_sync(pane_id: u64, generation: u64, title: &str) -> FullPaneSync {
        FullPaneSync {
            pane_id,
            generation,
            cols: 2,
            rows: 1,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: 0,
            mode_flags: 0,
            title: title.to_string(),
            scrollback: Vec::new(),
            scrollback_rows: 0,
                scrollback_replace: false,
            cells: vec![PackedCell::default(), PackedCell::default()],
            grapheme_extras: GraphemeExtras::new(),
            hyperlink_extras: HyperlinkExtras::new(),
            cwd: None,
        }
    }

    #[test]
    fn session_switch_state_sync_does_not_prune_retained_client_state() {
        let mut app = make_app();
        let (event_tx, rx) = crossbeam_channel::unbounded();
        app.server_rx = Some(rx);

        let stale = blank_full_sync(7, 1, "stale");
        app.pane_grids.insert(
            stale.pane_id,
            crate::grid::ClientPaneGrid::new(stale.cols, stale.rows, 100),
        );
        app.image_placements.insert(
            stale.pane_id,
            vec![ClientImagePlacement {
                image_id: 9,
                col: 0,
                row: 0,
                width_cells: 1,
                height_cells: 1,
                pixel_width: 8,
                pixel_height: 16,
            }],
        );
        app.write_last_session();
        app.cached_tile_glyphs.insert(
            stale.pane_id,
            CachedTileGlyphs {
                generation: 0,
                key: (0, 0, 0, 0),
                glyphs: Vec::new(),
                color_glyphs: Vec::new(),
            },
        );

        let new_sync = blank_full_sync(11, 2, "fresh");
        event_tx
            .send(ServerEvent::Control(ServerMessage::SessionSwitched {
                session_name: "other-session".to_string(),
            }))
            .unwrap();

        assert!(app.process_server_events());
        assert_eq!(app.session_name, "test-session");
        assert_eq!(app.pending_session_name.as_deref(), Some("other-session"));
        assert!(app.pane_grids.contains_key(&stale.pane_id));
        assert!(app.image_placements.contains_key(&stale.pane_id));
        assert!(app.cached_tile_glyphs.contains_key(&stale.pane_id));

        event_tx
            .send(ServerEvent::Control(ServerMessage::StateSync {
                layout: empty_layout(),
                pane_ids: vec![11],
            }))
            .unwrap();

        assert!(app.process_server_events());
        assert_eq!(app.session_name, "other-session");
        assert_eq!(app.pending_session_name, None);
        assert_eq!(
            app.expected_pane_ids.iter().copied().collect::<Vec<_>>(),
            vec![11]
        );
        assert!(app.pane_grids.contains_key(&stale.pane_id));
        assert!(app.image_placements.contains_key(&stale.pane_id));
        assert!(app.cached_tile_glyphs.contains_key(&stale.pane_id));

        event_tx.send(ServerEvent::FullPaneSync(new_sync)).unwrap();

        assert!(app.process_server_events());
        assert!(app.pane_grids.contains_key(&11));
        assert!(app.pane_grids.contains_key(&stale.pane_id));
        assert!(app.image_placements.contains_key(&stale.pane_id));
        assert!(app.cached_tile_glyphs.contains_key(&stale.pane_id));
    }

    #[test]
    fn session_switch_only_persists_last_session_after_authoritative_resync() {
        let mut app = make_app();
        let (event_tx, rx) = crossbeam_channel::unbounded();
        app.server_rx = Some(rx);
        let last_session_path = App::last_session_path();
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
        assert_eq!(app.pending_session_name.as_deref(), Some("other-session"));

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
        assert_eq!(app.pending_session_name, None);
        assert_eq!(
            std::fs::read_to_string(&last_session_path).unwrap(),
            "other-session"
        );
    }

    #[test]
    fn stale_frame_for_old_session_is_dropped_after_session_switch() {
        let mut app = make_app();
        let (event_tx, rx) = crossbeam_channel::unbounded();
        app.server_rx = Some(rx);

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
                                pane_id: fresh.pane_id,
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
                pane_ids: vec![fresh.pane_id],
            }))
            .unwrap();
        event_tx.send(ServerEvent::FullPaneSync(fresh)).unwrap();
        event_tx.send(ServerEvent::FullPaneSync(stale)).unwrap();

        assert!(app.process_server_events());
        assert_eq!(app.session_name, "other-session");
        assert!(app.pane_grids.contains_key(&11));
        assert!(!app.pane_grids.contains_key(&7));
    }
}
