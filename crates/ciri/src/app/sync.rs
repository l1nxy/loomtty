use ciri_input::keybind::KeybindMap;
use ciri_layout::column::ColumnWidth;
use ciri_layout::workspace::Workspace;
use ciri_protocol::message::*;
use ciri_render::glyph_cache::GlyphAtlas;

use super::App;
use crate::connection::ServerEvent;
use crate::grid::ClientPaneGrid;

use ciri_layout::column::Column;

impl App {
    /// Process all pending server events. Returns true if a redraw is needed.
    pub fn process_server_events(&mut self) -> bool {
        let Some(rx) = self.server_rx.as_ref() else { return false };
        let budget = 200;
        let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
            .take(budget)
            .collect();
        let hit_budget = events.len() >= budget;
        if events.is_empty() { return false; }
        let mut needs_redraw = false;

        for event in events {
            match event {
                ServerEvent::Control(ServerMessage::StateSync { layout, pane_ids }) => {
                    self.apply_layout(&layout);
                    for &id in &pane_ids {
                        self.pane_grids.entry(id).or_insert_with(|| ClientPaneGrid::new(80, 24, self.config.terminal.scrollback_lines));
                    }
                    self.connected = true;
                    needs_redraw = true;
                }
                ServerEvent::Control(ServerMessage::LayoutUpdate { layout }) => {
                    self.apply_layout(&layout);
                    needs_redraw = true;
                }
                ServerEvent::Control(ServerMessage::PaneCreated { pane_id, .. }) => {
                    self.pane_grids.entry(pane_id).or_insert_with(|| ClientPaneGrid::new(80, 24, self.config.terminal.scrollback_lines));
                    needs_redraw = true;
                }
                ServerEvent::Control(ServerMessage::PaneClosed { pane_id }) => {
                    self.pane_grids.remove(&pane_id);
                    self.cached_views.remove(&pane_id);
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
                ServerEvent::FullPaneSync(sync) => {
                    let cols = sync.cols as usize;
                    for line in 0..3.min(sync.rows as usize) {
                        let start = line * cols;
                        let end = (start + 30).min(sync.cells.len());
                        let chars: String = sync.cells[start..end].iter().map(|c| {
                            let ch = c.ch();
                            if ch == '\0' || ch == ' ' { '.' } else { ch }
                        }).collect();
                        log::info!("FullPaneSync pane={} line {line}: [{chars}]", sync.pane_id);
                    }
                    let grid = self.pane_grids.entry(sync.pane_id)
                        .or_insert_with(|| ClientPaneGrid::new(sync.cols, sync.rows, self.config.terminal.scrollback_lines));
                    grid.apply_full_sync(&sync);
                    self.send_lossy(ClientMessage::Ack { generation: sync.generation });
                    self.cached_views.remove(&sync.pane_id);
                    needs_redraw = true;
                }
                ServerEvent::CellDelta(delta) => {
                    log::debug!("CellDelta: pane={} regions={} cursor=({},{})",
                        delta.pane_id, delta.regions.len(), delta.cursor_col, delta.cursor_line);
                    if let Some(grid) = self.pane_grids.get_mut(&delta.pane_id) {
                        grid.apply_delta(&delta);
                        self.send_lossy(ClientMessage::Ack { generation: delta.generation });
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
                        next_retry: std::time::Instant::now() + std::time::Duration::from_millis(500),
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
        let view_size = self.workspaces.view_size;
        let column_gap = self.workspaces.column_gap;

        let mut new_rows: Vec<Workspace> = layout.rows.iter().map(|row_state| {
            let mut ws = Workspace::new_with_gap(view_size, column_gap);
            for col_state in &row_state.columns {
                if let Some(tile) = col_state.tiles.first() {
                    let mut col = Column::new(tile.pane_id);
                    col.width = ColumnWidth::Proportion(col_state.width_proportion);
                    ws.columns.push(col);
                }
            }
            ws.active_column_idx = row_state.active_column_idx
                .min(ws.columns.len().saturating_sub(1));
            ws
        }).collect();

        if new_rows.is_empty() {
            new_rows.push(Workspace::new_with_gap(view_size, column_gap));
        }

        self.workspaces.rows = new_rows;
        self.workspaces.active_row = layout.active_row
            .min(self.workspaces.rows.len().saturating_sub(1));

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
                self.overview_keybinds = KeybindMap::from_overview_config(&self.config.keys.overview_bindings);
                if font_changed {
                    if let Some(renderer) = &mut self.renderer {
                        let fmt = renderer.surface_format();
                        let atlas = GlyphAtlas::new(
                            &renderer.device, fmt,
                            &mut renderer.text.font_system, self.config.font.size,
                            self.dpi_scale, &self.config.font.family,
                            &self.config.render,
                        );
                        self.glyph_atlas = Some(atlas);
                    }
                }
                self.cached_views.clear();
                for grid in self.pane_grids.values_mut() { grid.dirty = true; }
                // Update leader key from config
                let leader_str = &self.config.keys.leader;
                if let Some(rest) = leader_str.strip_prefix("ctrl+") {
                    self.input.leader_ctrl_key = rest.to_string();
                }
                // Notify server of new cell dimensions after font change
                if font_changed {
                    let (cw, ch) = self.cell_dimensions();
                    let view = &self.workspaces.view_size;
                    self.send(ClientMessage::Resize {
                        cols: 0, rows: 0,
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
