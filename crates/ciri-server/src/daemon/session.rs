use anyhow::Result;
use ciri_layout::column::ColumnWidth;
use ciri_layout::geometry::ViewSize;
use ciri_layout::workspace_set::WorkspaceSet;
use ciri_protocol::message::*;
use ciri_protocol::transport;
use ciri_session::agent::SavedAgent;
use ciri_session::save::save_session;
use ciri_session::state::{SavedColumn, SavedTile, SavedWorkspace, SessionState};
use ciri_term::pane::{Pane, TerminalColors};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{Duration, Instant};

use super::client::ClientState;

pub(crate) const SESSION_AUTOSAVE_DEBOUNCE_MS: u64 = 250;

pub(crate) struct Session {
    pub(crate) workspaces: WorkspaceSet,
    pub(crate) panes: HashMap<u64, Pane>,
    pub(crate) session_name: String,
    pub(crate) generation: HashMap<u64, u64>, // per pane_id
    pub(crate) default_shell: String,
    pub(crate) default_column_width: ColumnWidth,
    /// Total inset per pane: (padding + border_width) * 2, subtracted from pane pixel size
    /// before computing grid cols/rows.
    pub(crate) pane_inset: f32,
    /// Theme colors for initializing terminal palettes.
    pub(crate) terminal_colors: TerminalColors,
    pub(crate) session_dirty: bool,
    pub(crate) last_session_change: Option<Instant>,
    /// Updated each time a client attaches to this session.
    pub(crate) last_attached: Instant,
    /// Cached agent detection results per pane (updated on slow timer).
    pub(crate) detected_agents: HashMap<u64, Option<SavedAgent>>,
    /// Last time agent detection ran.
    pub(crate) last_agent_save: Option<Instant>,
    /// Last-known cursor state per pane, for detecting cursor-only changes.
    pub(crate) last_cursor: HashMap<u64, (i16, u16, u8, u8)>,
}

impl Session {
    pub(crate) fn new(session_name: &str, shell: &str, column_gap: f32, terminal_colors: TerminalColors) -> Self {
        Session {
            workspaces: WorkspaceSet::new_with_gaps(
                ViewSize {
                    width: 1024.0,
                    height: 768.0,
                },
                column_gap,
                column_gap,
            ),
            panes: HashMap::new(),
            session_name: session_name.to_string(),
            generation: HashMap::new(),
            default_shell: shell.to_string(),
            default_column_width: ColumnWidth::Proportion(0.5),
            pane_inset: 12.0, // (4.0 padding + 2.0 border) * 2 = 12.0 default
            terminal_colors,
            session_dirty: false,
            last_session_change: None,
            last_attached: Instant::now(),
            detected_agents: HashMap::new(),
            last_agent_save: None,
            last_cursor: HashMap::new(),
        }
    }

    /// Mark a pane as fully damaged for all clients in this session.
    fn mark_full_damage(clients: &mut HashMap<u64, ClientState>, session_name: &str, pane_id: u64) {
        for client in clients.values_mut() {
            if client.session_name == session_name {
                client.damage.entry(pane_id).or_default().mark_full();
            }
        }
    }

    /// Get the CWD of the active pane (from OSC 7), if available.
    pub(crate) fn active_pane_cwd(&self) -> Option<String> {
        let pane_id = self.workspaces.active().active_pane_id()?;
        let pane = self.panes.get(&pane_id)?;
        pane.cwd().map(|s| s.to_string())
    }

    /// Create a new pane in the active workspace's active position (column right).
    pub(crate) fn create_pane(
        &mut self,
        next_pane_id: &mut u64,
        clients: &mut HashMap<u64, ClientState>,
    ) -> Result<u64> {
        self.create_pane_with_opts(next_pane_id, clients, None, None)
    }

    /// Create a new pane with an optional command and/or working directory.
    pub(crate) fn create_pane_with_opts(
        &mut self,
        next_pane_id: &mut u64,
        clients: &mut HashMap<u64, ClientState>,
        command: Option<&str>,
        cwd: Option<&std::path::Path>,
    ) -> Result<u64> {
        let id = *next_pane_id;
        *next_pane_id += 1;
        // Compute initial size from the column's actual width (not full viewport)
        let vw = self.workspaces.view_size.width;
        let vh = self.workspaces.view_size.height;
        let col_width = self.default_column_width;
        let col_px = match col_width {
            ColumnWidth::Proportion(p) => (vw as f64 * p) as f32,
            ColumnWidth::Fixed(px) => px as f32,
        };
        // First pane gets full viewport width
        let pane_w = if self.workspaces.active().columns.is_empty() {
            vw
        } else {
            col_px
        };
        let (_, _, cw, ch) = Self::effective_dims_from(clients, &self.session_name);
        let (cols, rows) = self.pane_grid_size_with_cells(pane_w, vh, cw, ch);
        log::info!(
            "create_pane {id}: viewport={vw}x{vh} col_px={pane_w:.1} cell={cw}x{ch} inset={} → {cols}x{rows}",
            self.pane_inset
        );
        // Use provided CWD, or fall back to inheriting from the active pane (OSC 7)
        let inherited_cwd = if cwd.is_none() {
            self.active_pane_cwd()
        } else {
            None
        };
        let effective_cwd = cwd.or_else(|| inherited_cwd.as_deref().map(std::path::Path::new));
        let mut pane =
            Pane::new_with_opts(id, cols, rows, &self.default_shell, command, effective_cwd)?;
        pane.init_colors(&self.terminal_colors);
        self.panes.insert(id, pane);
        self.generation.insert(id, 0);
        self.workspaces
            .active_mut()
            .add_column_right(id, self.default_column_width);
        Self::mark_full_damage(clients, &self.session_name, id);
        Ok(id)
    }

    /// Create a new pane in a new workspace below the active one (SplitDown).
    pub(crate) fn create_pane_in_new_workspace(
        &mut self,
        next_pane_id: &mut u64,
        clients: &mut HashMap<u64, ClientState>,
    ) -> Result<u64> {
        let id = *next_pane_id;
        *next_pane_id += 1;
        // Inherit CWD from the active pane (if available via OSC 7)
        let cwd = self.active_pane_cwd();
        let vw = self.workspaces.view_size.width;
        let vh = self.workspaces.view_size.height;
        let (_, _, cw, ch) = Self::effective_dims_from(clients, &self.session_name);
        let (cols, rows) = self.pane_grid_size_with_cells(vw, vh, cw, ch);
        let mut pane = Pane::new_with_cwd(
            id,
            cols,
            rows,
            &self.default_shell,
            cwd.as_deref().map(std::path::Path::new),
        )?;
        pane.init_colors(&self.terminal_colors);
        self.panes.insert(id, pane);
        self.generation.insert(id, 0);
        self.workspaces.add_workspace_below(id);
        Self::mark_full_damage(clients, &self.session_name, id);
        Ok(id)
    }

    /// Compute effective viewport and cell dimensions from session clients (smallest wins).
    /// Returns `(viewport_w, viewport_h, cell_w, cell_h)`.
    pub(crate) fn effective_dims_from(
        clients: &HashMap<u64, ClientState>,
        session_name: &str,
    ) -> (f32, f32, f32, f32) {
        let mut vw = f32::MAX;
        let mut vh = f32::MAX;
        let mut cw = f32::MAX;
        let mut ch = f32::MAX;
        for client in clients.values() {
            if client.session_name == session_name {
                vw = vw.min(client.viewport_width);
                vh = vh.min(client.viewport_height);
                cw = cw.min(client.cell_width);
                ch = ch.min(client.cell_height);
            }
        }
        (
            if vw == f32::MAX { 1024.0 } else { vw },
            if vh == f32::MAX { 768.0 } else { vh },
            if cw == f32::MAX { 8.0 } else { cw },
            if ch == f32::MAX { 16.0 } else { ch },
        )
    }

    /// Compute cols/rows for a pane given its pixel area and specific cell dimensions.
    pub(crate) fn pane_grid_size_with_cells(
        &self,
        pane_width: f32,
        pane_height: f32,
        cw: f32,
        ch: f32,
    ) -> (u16, u16) {
        let usable_w = (pane_width - self.pane_inset).max(cw);
        let usable_h = (pane_height - self.pane_inset).max(ch);
        let mut cols = (usable_w / cw).floor().max(1.0) as u16;
        let mut rows = (usable_h / ch).floor().max(1.0) as u16;
        // Cap grid dimensions to prevent OOM from extreme viewport sizes or tiny cell dims
        use ciri_protocol::message::MAX_GRID_CELLS;
        while cols as usize * rows as usize > MAX_GRID_CELLS {
            if cols > rows {
                cols /= 2;
            } else {
                rows /= 2;
            }
        }
        (cols, rows)
    }

    /// Get a pane's current grid dimensions.
    pub(crate) fn pane_grid_dims(&self, pane_id: u64) -> (u16, u16) {
        self.panes
            .get(&pane_id)
            .map(|p| (p.grid_cols(), p.grid_rows()))
            .unwrap_or((80, 24))
    }

    /// Resize ALL panes from the full layout tree.
    pub(crate) fn resize_all_panes(&mut self, clients: &mut HashMap<u64, ClientState>) -> bool {
        let (vp_w, vp_h, cw, ch) = Self::effective_dims_from(clients, &self.session_name);
        self.workspaces.resize_view(ViewSize {
            width: vp_w,
            height: vp_h,
        });
        let vw = self.workspaces.view_size.width;
        let vh = self.workspaces.view_size.height;
        let mut changed = false;
        log::debug!(
            "resize_all_panes: viewport={vw}x{vh} cell={cw}x{ch} inset={}",
            self.pane_inset
        );
        for ws in &self.workspaces.workspaces {
            for col in &ws.columns {
                let col_w = col.effective_width(vw);
                let tile_rects = col.tile_rects(col_w, vh);
                for (pane_id, _y, tile_h) in &tile_rects {
                    let (cols, rows) = self.pane_grid_size_with_cells(col_w, *tile_h, cw, ch);
                    if let Some(pane) = self.panes.get_mut(pane_id) {
                        let old_cols = pane.grid_cols();
                        let old_rows = pane.grid_rows();
                        log::debug!(
                            "  pane {}: col_w={col_w:.1}px tile_h={tile_h:.1}px → {cols}x{rows} (was {old_cols}x{old_rows})",
                            pane_id
                        );
                        if old_cols != cols || old_rows != rows {
                            pane.resize(cols, rows);
                            let g = self.generation.entry(*pane_id).or_insert(0);
                            *g += 1;
                            Self::mark_full_damage(clients, &self.session_name, *pane_id);
                            changed = true;
                        }
                    }
                }
            }
        }
        changed
    }

    pub(crate) fn close_pane(&mut self, pane_id: u64, clients: &mut HashMap<u64, ClientState>) {
        // Remove from whichever workspace contains it
        for ws in &mut self.workspaces.workspaces {
            ws.close_pane(pane_id);
        }
        self.workspaces.cleanup_empty();
        self.panes.remove(&pane_id);
        self.generation.remove(&pane_id);
        self.detected_agents.remove(&pane_id);
        for client in clients.values_mut() {
            if client.session_name == self.session_name {
                client.damage.remove(&pane_id);
                client.history_sent.remove(&pane_id);
            }
        }
    }

    /// Find the workspace/column/tile indices for a given pane ID.
    pub(crate) fn find_pane_location(&self, pane_id: u64) -> Option<(usize, usize, usize)> {
        for (ws_idx, ws) in self.workspaces.workspaces.iter().enumerate() {
            for (col_idx, col) in ws.columns.iter().enumerate() {
                for (tile_idx, tile) in col.tiles.iter().enumerate() {
                    if tile.pane_id == pane_id {
                        return Some((ws_idx, col_idx, tile_idx));
                    }
                }
            }
        }
        None
    }

    /// Focus a specific pane by ID, updating all active indices.
    pub(crate) fn focus_pane(&mut self, pane_id: u64) -> bool {
        if let Some((ws_idx, col_idx, tile_idx)) = self.find_pane_location(pane_id) {
            self.workspaces.active_workspace_idx = ws_idx;
            self.workspaces.workspaces[ws_idx].active_column_idx = col_idx;
            self.workspaces.workspaces[ws_idx].columns[col_idx].active_tile_idx = tile_idx;
            true
        } else {
            false
        }
    }

    pub(crate) fn layout_state(&self) -> LayoutState {
        let state = LayoutState {
            workspaces: self
                .workspaces
                .workspaces
                .iter()
                .map(|ws| WorkspaceState {
                    columns: ws
                        .columns
                        .iter()
                        .map(|c| ColumnState {
                            tiles: c
                                .tiles
                                .iter()
                                .map(|t| TileState {
                                    pane_id: t.pane_id,
                                    weight: t.height.weight(),
                                })
                                .collect(),
                            active_tile_idx: c.active_tile_idx,
                            width_proportion: c.proportion(self.workspaces.view_size.width),
                            width_fixed_px: match c.width {
                                ColumnWidth::Fixed(px) => Some(px),
                                _ => None,
                            },
                        })
                        .collect(),
                    active_column_idx: ws.active_column_idx,
                })
                .collect(),
            active_workspace_idx: self.workspaces.active_workspace_idx,
        };
        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "layout_state: {} ws, active={}",
                state.workspaces.len(),
                state.active_workspace_idx
            );
            for (i, ws) in state.workspaces.iter().enumerate() {
                for (j, col) in ws.columns.iter().enumerate() {
                    let panes: Vec<u64> = col.tiles.iter().map(|t| t.pane_id).collect();
                    log::debug!(
                        "  ws[{i}].col[{j}]: width={:.3}, panes={:?}",
                        col.width_proportion,
                        panes
                    );
                }
            }
        }
        state
    }

    pub(crate) fn save_session(&self) -> Result<()> {
        let layout = self.layout_state();
        let state = SessionState {
            name: self.session_name.clone(),
            workspaces: layout
                .workspaces
                .into_iter()
                .map(|ws| SavedWorkspace {
                    columns: ws
                        .columns
                        .into_iter()
                        .map(|c| SavedColumn {
                            tiles: c
                                .tiles
                                .into_iter()
                                .map(|t| {
                                    let pane = self.panes.get(&t.pane_id);
                                    let agent =
                                        self.detected_agents.get(&t.pane_id).cloned().flatten();
                                    SavedTile {
                                        pane_id: t.pane_id,
                                        weight: t.weight,
                                        cwd: pane.and_then(|p| p.cwd().map(|s| s.to_string())),
                                        title: None,
                                        agent,
                                    }
                                })
                                .collect(),
                            active_tile_idx: c.active_tile_idx,
                            width_proportion: c.width_proportion,
                            width_fixed_px: c.width_fixed_px,
                        })
                        .collect(),
                    active_column_idx: ws.active_column_idx,
                })
                .collect(),
            active_workspace_idx: layout.active_workspace_idx,
        };
        save_session(&state, &transport::state_dir())
    }

    pub(crate) fn mark_session_dirty(&mut self) {
        self.session_dirty = true;
        self.last_session_change = Some(Instant::now());
    }

    pub(crate) fn autosave_due_at(&self, now: Instant) -> bool {
        self.session_dirty
            && self.last_session_change.is_some_and(|changed_at| {
                now.duration_since(changed_at)
                    >= Duration::from_millis(SESSION_AUTOSAVE_DEBOUNCE_MS)
            })
    }

    pub(crate) fn autosave_if_due(&mut self, now: Instant) {
        if !self.autosave_due_at(now) || self.panes.is_empty() {
            return;
        }

        match self.save_session() {
            Ok(()) => {
                self.session_dirty = false;
                log::debug!("autosaved session '{}'", self.session_name);
            }
            Err(e) => {
                self.last_session_change = Some(now);
                log::warn!("failed to autosave session '{}': {e}", self.session_name);
            }
        }
    }

    /// Detect AI agents running in all panes.
    /// Called on a slow timer (~30s) and on graceful shutdown.
    /// Returns `true` if any detected agents changed since last call.
    pub(crate) fn detect_agents(&mut self) -> bool {
        let mut changed = false;
        for (&pane_id, pane) in &self.panes {
            let agent = Self::detect_agent_for_pane(pane);
            let prev = self.detected_agents.get(&pane_id);
            let new_kind = agent.as_ref().map(|a| a.kind);
            let old_kind = prev.and_then(|a| a.as_ref().map(|a| a.kind));
            if new_kind != old_kind {
                changed = true;
            }
            self.detected_agents.insert(pane_id, agent);
        }
        log::debug!(
            "agent detection: {} panes scanned, {} agents found, changed={}",
            self.panes.len(),
            self.detected_agents
                .values()
                .filter(|a| a.is_some())
                .count(),
            changed
        );
        changed
    }

    #[cfg(unix)]
    fn detect_agent_for_pane(pane: &Pane) -> Option<SavedAgent> {
        let shell_pid = pane.child_pid()?;
        let master_fd = pane.master_raw_fd()?;
        let info =
            ciri_procinfo::foreground_process(shell_pid, master_fd as ciri_procinfo::RawHandle)?;
        ciri_session::agent::detect_agent(&info.exe_name, &info.argv)
    }

    #[cfg(windows)]
    fn detect_agent_for_pane(pane: &Pane) -> Option<SavedAgent> {
        let shell_pid = pane.child_pid()?;
        let info = ciri_procinfo::foreground_process(shell_pid, 0)?;
        ciri_session::agent::detect_agent(&info.exe_name, &info.argv)
    }

    #[cfg(not(any(unix, windows)))]
    fn detect_agent_for_pane(_pane: &Pane) -> Option<SavedAgent> {
        None
    }

    /// Check if agent detection should run based on the configured interval.
    pub(crate) fn agent_detection_due(&self, now: Instant, interval_secs: u64) -> bool {
        let interval = Duration::from_secs(interval_secs.max(1));
        self.last_agent_save
            .is_none_or(|t| now.duration_since(t) >= interval)
    }

    /// Process PTY output for all panes, extract damage, and merge into per-client accumulators.
    /// Returns any clipboard store requests from OSC 52.
    pub(crate) fn process_pty_and_damage(
        &mut self,
        clients: &mut HashMap<u64, ClientState>,
    ) -> Vec<ServerMessage> {
        let mut clipboard_msgs = Vec::new();
        let pane_ids: Vec<u64> = self.panes.keys().copied().collect();
        for pane_id in pane_ids {
            let pane = self.panes.get_mut(&pane_id).unwrap();
            pane.process_pty_output();

            // Drain OSC 52 clipboard writes
            for data in pane.drain_clipboard() {
                clipboard_msgs.push(ServerMessage::ClipboardStore { data });
            }

            // Drain bell events
            if pane.drain_bell() {
                clipboard_msgs.push(ServerMessage::Bell { pane_id });
            }

            // Drain command completion events (OSC 133;D)
            if let Some(duration) = pane.drain_command_completion() {
                clipboard_msgs.push(ServerMessage::CommandCompleted {
                    pane_id,
                    duration_secs: duration.as_secs(),
                    exit_code: pane.last_exit_code(),
                });
            }

            // Drain image placements (Kitty/Sixel)
            for img in pane.drain_images() {
                clipboard_msgs.push(ServerMessage::ImagePlacement {
                    pane_id,
                    image_id: img.id,
                    col: img.col,
                    row: img.row,
                    width_cells: img.width_cells,
                    height_cells: img.height_cells,
                    pixel_width: img.pixel_width,
                    pixel_height: img.pixel_height,
                    format: img.format,
                    data: Arc::try_unwrap(img.data).unwrap_or_else(|arc| (*arc).clone()),
                });
            }
            if pane.drain_image_deletes() {
                clipboard_msgs.push(ServerMessage::ImageDeleted { pane_id });
            }
            // Detect cursor-only changes (no cell damage but cursor moved).
            let cur_cursor = pane.cursor_info();
            let prev_cursor = self.last_cursor.get(&pane_id).copied();
            let cursor_changed = prev_cursor.map_or(true, |prev| prev != cur_cursor);
            if cursor_changed {
                self.last_cursor.insert(pane_id, cur_cursor);
            }

            if let Some(ranges) = pane.extract_damage() {
                // Bump generation
                let g = self.generation.entry(pane_id).or_insert(0);
                *g += 1;

                for client in clients.values_mut() {
                    if client.session_name == self.session_name {
                        let acc = client.damage.entry(pane_id).or_default();
                        acc.merge_ranges(&ranges);
                        acc.cursor_dirty = true;
                    }
                }
            } else if cursor_changed {
                // No cell damage, but cursor position/shape/mode changed.
                for client in clients.values_mut() {
                    if client.session_name == self.session_name {
                        let acc = client.damage.entry(pane_id).or_default();
                        acc.cursor_dirty = true;
                    }
                }
            }
        }
        clipboard_msgs
    }

    /// Check for exited panes and clean them up. Returns list of closed pane IDs.
    pub(crate) fn cleanup_exited_panes(
        &mut self,
        clients: &mut HashMap<u64, ClientState>,
    ) -> Vec<u64> {
        let dead: Vec<u64> = self
            .panes
            .iter()
            .filter(|(_, p)| p.is_exited())
            .map(|(id, _)| *id)
            .collect();
        for &id in &dead {
            self.close_pane(id, clients);
        }
        dead
    }

    /// Build initial StateSync for a new client, including FullPaneSync for each pane.
    pub(crate) fn build_state_sync(
        &self,
    ) -> (ServerMessage, Vec<FullPaneSync>, Vec<ServerMessage>) {
        let pane_ids: Vec<u64> = self.workspaces.all_pane_ids();

        let syncs: Vec<FullPaneSync> = pane_ids
            .iter()
            .filter_map(|&id| {
                let pane = self.panes.get(&id)?;
                let pgen = self.generation.get(&id).copied().unwrap_or(0);
                Some(pane.snapshot(pgen))
            })
            .collect();

        let image_invalidations: Vec<ServerMessage> = pane_ids
            .iter()
            .filter_map(|&id| {
                let pane = self.panes.get(&id)?;
                pane.active_images()
                    .is_empty()
                    .then_some(ServerMessage::ImageDeleted { pane_id: id })
            })
            .collect();

        let msg = ServerMessage::StateSync {
            layout: self.layout_state(),
            pane_ids: pane_ids.clone(),
        };
        (msg, syncs, image_invalidations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::client::ClientState;
    use tokio::sync::mpsc;

    #[test]
    fn autosave_is_debounced() {
        let mut session = Session::new("default", "/bin/sh", 8.0, TerminalColors::default());
        session.session_dirty = true;
        let changed_at = Instant::now();
        session.last_session_change = Some(changed_at);

        assert!(!session.autosave_due_at(changed_at));
        assert!(!session.autosave_due_at(changed_at + Duration::from_millis(249)));
        assert!(session.autosave_due_at(changed_at + Duration::from_millis(250)));
    }

    #[test]
    fn mark_session_dirty_sets_dirty_and_timestamp() {
        let mut session = Session::new("default", "/bin/sh", 8.0, TerminalColors::default());
        assert!(!session.session_dirty);
        assert!(session.last_session_change.is_none());

        session.mark_session_dirty();

        assert!(session.session_dirty);
        assert!(session.last_session_change.is_some());
    }

    #[test]
    fn effective_dims_use_smallest_attached_client() {
        let (tx1, _rx1) = mpsc::channel(1);
        let (tx2, _rx2) = mpsc::channel(1);
        let (tx3, _rx3) = mpsc::channel(1);
        let mut clients = HashMap::new();
        clients.insert(
            1,
            ClientState {
                id: 1,
                tx: tx1,
                damage: HashMap::new(),
                last_acked_generation: 0,
                history_sent: HashMap::new(),
                send_failures: 0,
                cell_width: 9.0,
                cell_height: 18.0,
                viewport_width: 1200.0,
                viewport_height: 900.0,
                session_name: "alpha".to_string(),
            },
        );
        clients.insert(
            2,
            ClientState {
                id: 2,
                tx: tx2,
                damage: HashMap::new(),
                last_acked_generation: 0,
                history_sent: HashMap::new(),
                send_failures: 0,
                cell_width: 8.0,
                cell_height: 16.0,
                viewport_width: 900.0,
                viewport_height: 700.0,
                session_name: "alpha".to_string(),
            },
        );
        clients.insert(
            3,
            ClientState {
                id: 3,
                tx: tx3,
                damage: HashMap::new(),
                last_acked_generation: 0,
                history_sent: HashMap::new(),
                send_failures: 0,
                cell_width: 7.0,
                cell_height: 14.0,
                viewport_width: 640.0,
                viewport_height: 480.0,
                session_name: "beta".to_string(),
            },
        );

        let dims = Session::effective_dims_from(&clients, "alpha");

        assert_eq!(dims, (900.0, 700.0, 8.0, 16.0));
    }

    #[test]
    fn build_state_sync_emits_image_deleted_for_panes_without_active_images() {
        let mut session = Session::new("default", "/bin/sh", 8.0, TerminalColors::default());
        let mut next_pane_id = 1;
        let mut clients = HashMap::new();
        let pane_id = session
            .create_pane(&mut next_pane_id, &mut clients)
            .expect("pane");

        let image = ciri_term::pane::ImagePlacement {
            id: 42,
            row: 1,
            col: 2,
            width_cells: 3,
            height_cells: 4,
            pixel_width: 24,
            pixel_height: 64,
            format: "rgb".to_string(),
            data: std::sync::Arc::new(vec![1, 2, 3]),
        };
        session
            .panes
            .get_mut(&pane_id)
            .expect("pane exists")
            .test_add_active_image(image.clone());

        let (_sync, _pane_syncs, image_events) = session.build_state_sync();
        assert!(
            image_events.is_empty(),
            "active images should not emit deletes"
        );

        session
            .panes
            .get_mut(&pane_id)
            .expect("pane exists")
            .test_mark_image_deleted();
        let (_sync, _pane_syncs, image_events) = session.build_state_sync();
        assert!(matches!(
            image_events.as_slice(),
            [ServerMessage::ImageDeleted { pane_id: id }] if *id == pane_id
        ));
    }

    #[test]
    fn collect_runtime_messages_emits_image_deleted_after_clear() {
        let mut session = Session::new("default", "/bin/sh", 8.0, TerminalColors::default());
        let mut next_pane_id = 1;
        let mut clients = HashMap::new();
        let pane_id = session
            .create_pane(&mut next_pane_id, &mut clients)
            .expect("pane");
        let pane = session.panes.get_mut(&pane_id).expect("pane exists");
        pane.test_mark_image_deleted();

        let messages = session.process_pty_and_damage(&mut clients);

        assert!(messages.iter().any(
            |msg| matches!(msg, ServerMessage::ImageDeleted { pane_id: id } if id == &pane_id)
        ));
    }
}
