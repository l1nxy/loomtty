use ciri_input::keybind::KeybindMap;
use ciri_layout::column::ColumnWidth;
use ciri_layout::workspace::Workspace;
use ciri_protocol::message::*;
use ciri_render::glyph_cache::GlyphAtlas;

use super::App;
use crate::connection::ServerEvent;
use crate::grid::ClientPaneGrid;

use ciri_layout::column::Column;
use ciri_layout::tile::Tile;

impl App {
    /// Process all pending server events. Returns true if a redraw is needed.
    pub fn process_server_events(&mut self) -> bool {
        let Some(rx) = self.server_rx.as_ref() else {
            return false;
        };
        let budget = 200;
        let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
            .take(budget)
            .collect();
        let hit_budget = events.len() >= budget;
        if events.is_empty() {
            return false;
        }
        let mut needs_redraw = false;

        for event in events {
            match event {
                ServerEvent::Control(ServerMessage::StateSync { layout, pane_ids }) => {
                    self.apply_layout(&layout);
                    for &id in &pane_ids {
                        self.pane_grids.entry(id).or_insert_with(|| {
                            ClientPaneGrid::new(80, 24, self.config.terminal.scrollback_lines)
                        });
                    }
                    self.connected = true;
                    needs_redraw = true;
                }
                ServerEvent::Control(ServerMessage::LayoutUpdate { layout }) => {
                    log::trace!("LayoutUpdate: {} workspaces, active={}",
                        layout.workspaces.len(), layout.active_workspace_idx);
                    for (i, ws) in layout.workspaces.iter().enumerate() {
                        log::trace!("  ws[{}]: {} columns, active_col={}",
                            i, ws.columns.len(), ws.active_column_idx);
                        for (j, col) in ws.columns.iter().enumerate() {
                            log::trace!("    col[{}]: width={:.3}, {} tiles",
                                j, col.width_proportion, col.tiles.len());
                        }
                    }
                    self.apply_layout(&layout);
                    // Cancel any active tile drag — layout indices may have changed
                    self.tile_resize_dragging = None;
                    needs_redraw = true;
                }
                ServerEvent::Control(ServerMessage::PaneCreated { pane_id, cols, rows, .. }) => {
                    log::debug!("PaneCreated: pane_id={pane_id} {cols}x{rows}");
                    self.pane_grids.entry(pane_id).or_insert_with(|| {
                        ClientPaneGrid::new(cols, rows, self.config.terminal.scrollback_lines)
                    });
                    self.pane_open_opacity.insert(pane_id, 0.0); // start fade-in
                    needs_redraw = true;
                }
                ServerEvent::Control(ServerMessage::PaneClosed { pane_id }) => {
                    log::debug!("PaneClosed: pane_id={pane_id}");
                    // Capture pane rect for close animation before removing
                    let vox = self.view_offset_x.value() as f32;
                    let voy = self.view_offset_y.value() as f32;
                    let tiles = self.workspaces.visible_tiles_2d(vox, voy);
                    if let Some((_, rect, _)) = tiles.iter().find(|(pid, _, _)| *pid == pane_id) {
                        self.closing_panes.push(super::ClosingPaneState {
                            rect: *rect,
                            opacity: 1.0,
                            started: std::time::Instant::now(),
                            duration_ms: 200,
                        });
                    }
                    self.pane_grids.remove(&pane_id);
                    self.cached_views.remove(&pane_id);
                    self.pane_open_opacity.remove(&pane_id);
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
                    self.bell_flash = Some((pane_id, std::time::Instant::now()));

                    // Window urgency hint
                    if self.config.terminal.bell_urgency && !self.window_focused {
                        if let Some(ref window) = self.window {
                            window.request_user_attention(Some(winit::window::UserAttentionType::Informational));
                        }
                    }

                    // Bell audio
                    self.play_bell_audio();

                    needs_redraw = true;
                }
                ServerEvent::Control(ServerMessage::CommandCompleted { pane_id, duration_secs, exit_code }) => {
                    let threshold = self.config.terminal.notify_command_threshold_secs;
                    if threshold > 0 && duration_secs >= threshold && !self.window_focused {
                        self.send_desktop_notification(
                            "Command completed",
                            &format!("Pane {} finished after {}s (exit: {})",
                                pane_id, duration_secs, exit_code.unwrap_or(0)),
                        );
                    }
                }
                ServerEvent::Control(ServerMessage::ImagePlacement {
                    pane_id, image_id, col, row,
                    width_cells, height_cells,
                    pixel_width, pixel_height,
                    ..
                }) => {
                    log::debug!("image #{image_id} for pane {pane_id}: {width_cells}x{height_cells} cells");
                    let placements = self.image_placements.entry(pane_id).or_default();
                    placements.push(super::ClientImagePlacement {
                        image_id, col, row,
                        width_cells, height_cells,
                        pixel_width, pixel_height,
                    });
                    needs_redraw = true;
                }
                ServerEvent::Control(ServerMessage::SessionList { .. })
                | ServerEvent::Control(ServerMessage::SessionSwitched { .. })
                | ServerEvent::Control(ServerMessage::SessionKilled { .. })
                | ServerEvent::Control(ServerMessage::Error { .. }) => {
                    // Session management responses — not yet handled by GUI client
                }
                ServerEvent::FullPaneSync(sync) => {
                    let cols = sync.cols as usize;
                    for line in 0..3.min(sync.rows as usize) {
                        let start = line * cols;
                        let end = (start + 30).min(sync.cells.len());
                        let chars: String = sync.cells[start..end]
                            .iter()
                            .map(|c| {
                                let ch = c.ch();
                                if ch == '\0' || ch == ' ' { '.' } else { ch }
                            })
                            .collect();
                        log::info!("FullPaneSync pane={} line {line}: [{chars}]", sync.pane_id);
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
                    self.cached_views.remove(&sync.pane_id);
                    needs_redraw = true;
                }
                ServerEvent::CellDelta(delta) => {
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
                        self.cached_views.remove(&delta.pane_id);
                        needs_redraw = true;
                    }
                }
                ServerEvent::Disconnected => {
                    log::warn!("disconnected from server");
                    self.connected = false;
                    // Preserve pane_grids for scrollback history — they'll be
                    // validated against server state on reconnect via FullPaneSync.
                    for grid in self.pane_grids.values_mut() {
                        grid.dirty = true;
                    }
                    self.cached_views.clear();
                    self.server_tx = None;
                    self.server_rx = None;
                    self.reconnect_state = Some(super::ReconnectState {
                        attempt: 0,
                        max_attempts: 10,
                        next_retry: std::time::Instant::now()
                            + std::time::Duration::from_millis(500),
                        backoff: std::time::Duration::from_millis(500),
                    });
                    return true;
                }
            }
        }
        // If we hit the budget, there may be more events — ensure we get another tick
        if hit_budget {
            needs_redraw = true;
        }
        needs_redraw
    }

    /// Rebuild the full 2D WorkspaceSet from the server's authoritative layout.
    pub fn apply_layout(&mut self, layout: &LayoutState) {
        log::debug!("apply_layout: view_size={:?}, vox={:.1}, voy={:.1}",
            self.workspaces.view_size,
            self.view_offset_x.value(),
            self.view_offset_y.value());
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
                        col.tiles = col_state.tiles.iter().map(|t| {
                            let mut tile = Tile::new(t.pane_id);
                            tile.height = ciri_layout::tile::TileHeight::Auto { weight: t.weight as f64 };
                            tile
                        }).collect();
                        col.active_tile_idx = col_state.active_tile_idx.min(col.tiles.len().saturating_sub(1));
                        col.width = ColumnWidth::Proportion(col_state.width_proportion);
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

        self.snap_all_col_widths();
        self.animate_to_active();
    }

    pub fn reload_config(&mut self) {
        match ciri_config::config::CiriConfig::load() {
            Ok(new_config) => {
                let font_changed = new_config.font.family != self.config.font.family
                    || (new_config.font.size - self.config.font.size).abs() > 0.01;
                self.config = new_config;
                self.input.keybinds = KeybindMap::from_config(&self.config.keys.bindings);
                self.overview_keybinds =
                    KeybindMap::from_overview_config(&self.config.keys.overview_bindings);
                if font_changed {
                    if let Some(renderer) = &mut self.renderer {
                        let fmt = renderer.surface_format();
                        let atlas = GlyphAtlas::new(
                            &renderer.device,
                            fmt,
                            &mut renderer.text.font_system,
                            self.config.font.size,
                            self.dpi_scale,
                            &self.config.font.family,
                            &self.config.render,
                        );
                        self.glyph_atlas = Some(atlas);
                    }
                }
                self.cached_views.clear();
                for grid in self.pane_grids.values_mut() {
                    grid.dirty = true;
                }
                // Update leader key and input mode from config
                self.input.leader_key = ciri_input::leader::LeaderKey::parse(&self.config.keys.leader);
                self.input.input_mode = match self.config.input.mode.as_str() {
                    "sticky" => ciri_input::leader::InputMode::Sticky,
                    _ => ciri_input::leader::InputMode::Prefix,
                };
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
