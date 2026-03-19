use anyhow::Result;
use ciri_layout::column::ColumnWidth;
use ciri_layout::geometry::ViewSize;
use ciri_layout::workspace_set::WorkspaceSet;
use ciri_protocol::codec;
use ciri_protocol::message::*;
use ciri_protocol::transport;
use ciri_session::save::save_session;
use ciri_session::state::{SavedColumn, SavedWorkspace, SavedTile, SessionState};
use ciri_term::pane::Pane;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncWriteExt, BufWriter};
#[cfg(unix)]
use tokio::net::UnixListener;
#[cfg(windows)]
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex, Notify};
use tokio::time::{interval, Duration, Instant};

const SESSION_AUTOSAVE_DEBOUNCE_MS: u64 = 250;

// ─── Per-client damage accumulator ──────────────────────────────────

struct DamageAccumulator {
    full: bool,
    /// Indexed by line. Some((left, right)) = dirty range for that line.
    line_damage: HashMap<u16, (u16, u16)>,
    /// Whether the cursor has moved since last send.
    cursor_dirty: bool,
}

impl DamageAccumulator {
    fn new() -> Self {
        DamageAccumulator {
            full: false,
            line_damage: HashMap::new(),
            cursor_dirty: false,
        }
    }

    fn mark_full(&mut self) {
        self.full = true;
        self.line_damage.clear();
    }

    fn merge_regions(&mut self, regions: &[DamageRegion]) {
        if self.full {
            return; // already marked for full sync
        }
        for region in regions {
            let entry = self.line_damage.entry(region.line).or_insert((u16::MAX, 0));
            entry.0 = entry.0.min(region.left);
            entry.1 = entry.1.max(region.right);
        }
    }

    fn is_empty(&self) -> bool {
        !self.full && self.line_damage.is_empty() && !self.cursor_dirty
    }

    fn take(&mut self) -> DamageAccumulator {
        std::mem::replace(self, DamageAccumulator::new())
    }
}

// ─── Client state ───────────────────────────────────────────────────

struct ClientState {
    id: u64,
    tx: mpsc::Sender<Vec<u8>>,
    damage: HashMap<u64, DamageAccumulator>, // per pane_id
    last_acked_generation: u64,
    /// Per-pane: how many history lines this client has received.
    history_sent: HashMap<u64, usize>,
    /// Consecutive try_send failures; used to detect slow clients.
    send_failures: u32,
    cell_width: f32,
    cell_height: f32,
    viewport_width: f32,
    viewport_height: f32,
    /// Which session this client is attached to.
    session_name: String,
}

// ─── Session (was ServerState) ──────────────────────────────────────

struct Session {
    workspaces: WorkspaceSet,
    panes: HashMap<u64, Pane>,
    session_name: String,
    generation: HashMap<u64, u64>, // per pane_id
    default_shell: String,
    default_column_width: ColumnWidth,
    /// Total inset per pane: (padding + border_width) * 2, subtracted from pane pixel size
    /// before computing grid cols/rows.
    pane_inset: f32,
    session_dirty: bool,
    last_session_change: Option<Instant>,
}

impl Session {
    fn new(session_name: &str, shell: &str, column_gap: f32) -> Self {
        Session {
            workspaces: WorkspaceSet::new_with_gaps(ViewSize {
                width: 1024.0,
                height: 768.0,
            }, column_gap, column_gap),
            panes: HashMap::new(),
            session_name: session_name.to_string(),
            generation: HashMap::new(),
            default_shell: shell.to_string(),
            default_column_width: ColumnWidth::Proportion(0.5),
            pane_inset: 12.0, // (4.0 padding + 2.0 border) * 2 = 12.0 default
            session_dirty: false,
            last_session_change: None,
        }
    }

    /// Create a new pane in the active workspace's active position (column right).
    fn create_pane(&mut self, next_pane_id: &mut u64, clients: &mut HashMap<u64, ClientState>) -> Result<u64> {
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
        let pane_w = if self.workspaces.active().columns.is_empty() { vw } else { col_px };
        let (cw, ch) = Self::effective_cell_dims_from(clients, &self.session_name);
        let (cols, rows) = self.pane_grid_size_with_cells(pane_w, vh, cw, ch);
        log::info!("create_pane {id}: viewport={vw}x{vh} col_px={pane_w:.1} cell={cw}x{ch} inset={} → {cols}x{rows} default_col_width={:?}", self.pane_inset, self.default_column_width);
        let pane = Pane::new(id, cols, rows, &self.default_shell)?;
        self.panes.insert(id, pane);
        self.generation.insert(id, 0);
        self.workspaces.active_mut().add_column_right(id, self.default_column_width);
        // Mark session clients for full sync of this new pane
        for client in clients.values_mut() {
            if client.session_name == self.session_name {
                client.damage.entry(id).or_insert_with(DamageAccumulator::new).mark_full();
            }
        }
        Ok(id)
    }

    /// Create a new pane in a new workspace below the active one (SplitDown).
    fn create_pane_in_new_workspace(&mut self, next_pane_id: &mut u64, clients: &mut HashMap<u64, ClientState>) -> Result<u64> {
        let id = *next_pane_id;
        *next_pane_id += 1;
        let vw = self.workspaces.view_size.width;
        let vh = self.workspaces.view_size.height;
        let (cw, ch) = Self::effective_cell_dims_from(clients, &self.session_name);
        let (cols, rows) = self.pane_grid_size_with_cells(vw, vh, cw, ch);
        let pane = Pane::new(id, cols, rows, &self.default_shell)?;
        self.panes.insert(id, pane);
        self.generation.insert(id, 0);
        self.workspaces.add_workspace_below(id);
        for client in clients.values_mut() {
            if client.session_name == self.session_name {
                client.damage.entry(id).or_insert_with(DamageAccumulator::new).mark_full();
            }
        }
        Ok(id)
    }

    /// Compute effective cell dimensions from a set of clients for this session (smallest wins).
    fn effective_cell_dims_from(clients: &HashMap<u64, ClientState>, session_name: &str) -> (f32, f32) {
        let mut cw = f32::MAX;
        let mut ch = f32::MAX;
        for client in clients.values() {
            if client.session_name == session_name {
                cw = cw.min(client.cell_width);
                ch = ch.min(client.cell_height);
            }
        }
        if cw == f32::MAX { cw = 8.0; }
        if ch == f32::MAX { ch = 16.0; }
        (cw, ch)
    }

    /// Compute effective viewport from clients for this session (smallest wins).
    fn effective_viewport_from(clients: &HashMap<u64, ClientState>, session_name: &str) -> (f32, f32) {
        let mut w = f32::MAX;
        let mut h = f32::MAX;
        for client in clients.values() {
            if client.session_name == session_name {
                w = w.min(client.viewport_width);
                h = h.min(client.viewport_height);
            }
        }
        if w == f32::MAX { w = 1024.0; }
        if h == f32::MAX { h = 768.0; }
        (w, h)
    }

    /// Compute cols/rows for a pane given its pixel area and specific cell dimensions.
    fn pane_grid_size_with_cells(&self, pane_width: f32, pane_height: f32, cw: f32, ch: f32) -> (u16, u16) {
        let usable_w = (pane_width - self.pane_inset).max(cw);
        let usable_h = (pane_height - self.pane_inset).max(ch);
        let mut cols = (usable_w / cw).floor().max(1.0) as u16;
        let mut rows = (usable_h / ch).floor().max(1.0) as u16;
        // Cap grid dimensions to prevent OOM from extreme viewport sizes
        const MAX_GRID_CELLS: usize = 10_000_000;
        while cols as usize * rows as usize > MAX_GRID_CELLS {
            if cols > rows { cols /= 2; } else { rows /= 2; }
        }
        (cols, rows)
    }

    /// Get a pane's current grid dimensions.
    fn pane_grid_dims(&self, pane_id: u64) -> (u16, u16) {
        self.panes.get(&pane_id)
            .map(|p| (p.grid_cols(), p.grid_rows()))
            .unwrap_or((80, 24))
    }

    /// Resize ALL panes from the full layout tree.
    fn resize_all_panes(&mut self, clients: &mut HashMap<u64, ClientState>) {
        let (vp_w, vp_h) = Self::effective_viewport_from(clients, &self.session_name);
        self.workspaces.resize_view(ViewSize { width: vp_w, height: vp_h });
        let vw = self.workspaces.view_size.width;
        let vh = self.workspaces.view_size.height;

        let (cw, ch) = Self::effective_cell_dims_from(clients, &self.session_name);
        log::info!("resize_all_panes: viewport={vw}x{vh} cell={cw}x{ch} inset={}", self.pane_inset);
        for (ws_idx, ws) in self.workspaces.workspaces.iter().enumerate() {
            for (col_idx, col) in ws.columns.iter().enumerate() {
                let col_w = col.effective_width(vw);
                log::info!("  ws[{ws_idx}] col[{col_idx}]: width={:?} effective={col_w:.1}px", col.width);
                let tile_rects = col.tile_rects(col_w, vh);
                for (pane_id, _y, tile_h) in &tile_rects {
                    let (cols, rows) = self.pane_grid_size_with_cells(col_w, *tile_h, cw, ch);
                    log::debug!("  pane {}: col_w={col_w:.1}px tile_h={tile_h:.1}px → {cols}x{rows} chars", pane_id);
                    if let Some(pane) = self.panes.get_mut(pane_id) {
                        pane.resize(cols, rows);
                        let g = self.generation.entry(*pane_id).or_insert(0);
                        *g += 1;
                        for client in clients.values_mut() {
                            if client.session_name == self.session_name {
                                client.damage.entry(*pane_id)
                                    .or_insert_with(DamageAccumulator::new).mark_full();
                            }
                        }
                    }
                }
            }
        }
    }

    fn close_pane(&mut self, pane_id: u64, clients: &mut HashMap<u64, ClientState>) {
        // Remove from whichever workspace contains it
        for ws in &mut self.workspaces.workspaces {
            ws.close_pane(pane_id);
        }
        self.workspaces.cleanup_empty();
        self.panes.remove(&pane_id);
        self.generation.remove(&pane_id);
        for client in clients.values_mut() {
            if client.session_name == self.session_name {
                client.damage.remove(&pane_id);
                client.history_sent.remove(&pane_id);
            }
        }
    }

    fn layout_state(&self) -> LayoutState {
        let state = LayoutState {
            workspaces: self.workspaces.workspaces.iter().map(|ws| WorkspaceState {
                columns: ws.columns.iter().map(|c| ColumnState {
                    tiles: c.tiles.iter().map(|t| TileState { pane_id: t.pane_id, weight: t.height.weight() }).collect(),
                    active_tile_idx: c.active_tile_idx,
                    width_proportion: c.proportion(self.workspaces.view_size.width),
                }).collect(),
                active_column_idx: ws.active_column_idx,
            }).collect(),
            active_workspace_idx: self.workspaces.active_workspace_idx,
        };
        log::debug!("layout_state: {} ws, active={}", state.workspaces.len(), state.active_workspace_idx);
        for (i, ws) in state.workspaces.iter().enumerate() {
            for (j, col) in ws.columns.iter().enumerate() {
                let panes: Vec<u64> = col.tiles.iter().map(|t| t.pane_id).collect();
                log::debug!("  ws[{i}].col[{j}]: width={:.3}, panes={:?}", col.width_proportion, panes);
            }
        }
        state
    }

    fn save_session(&self) -> Result<()> {
        let state = SessionState {
            name: self.session_name.clone(),
            workspaces: self.workspaces.workspaces.iter().map(|ws| SavedWorkspace {
                columns: ws.columns.iter().map(|c| SavedColumn {
                    tiles: c.tiles.iter().map(|t| SavedTile { pane_id: t.pane_id, weight: t.height.weight(), cwd: None, title: None }).collect(),
                    active_tile_idx: c.active_tile_idx,
                    width_proportion: c.proportion(self.workspaces.view_size.width),
                }).collect(),
                active_column_idx: ws.active_column_idx,
            }).collect(),
            active_workspace_idx: self.workspaces.active_workspace_idx,
        };
        save_session(&state, &transport::state_dir())
    }

    fn mark_session_dirty(&mut self) {
        self.session_dirty = true;
        self.last_session_change = Some(Instant::now());
    }

    fn autosave_due_at(&self, now: Instant) -> bool {
        self.session_dirty
            && self
                .last_session_change
                .is_some_and(|changed_at| now.duration_since(changed_at) >= Duration::from_millis(SESSION_AUTOSAVE_DEBOUNCE_MS))
    }

    fn autosave_if_due(&mut self, now: Instant) {
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

    /// Process PTY output for all panes, extract damage, and merge into per-client accumulators.
    /// Returns any clipboard store requests from OSC 52.
    fn process_pty_and_damage(&mut self, clients: &mut HashMap<u64, ClientState>) -> Vec<ServerMessage> {
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
                    data: img.data,
                });
            }

            if let Some(regions) = pane.extract_damage() {
                // Bump generation
                let g = self.generation.entry(pane_id).or_insert(0);
                *g += 1;

                // If all terminal rows were damaged, this is likely a scroll or full repaint.
                let is_full = regions.len() >= pane.grid_rows() as usize;

                for client in clients.values_mut() {
                    if client.session_name == self.session_name {
                        let acc = client.damage.entry(pane_id).or_insert_with(DamageAccumulator::new);
                        if is_full {
                            acc.mark_full();
                        } else {
                            acc.merge_regions(&regions);
                        }
                        acc.cursor_dirty = true;
                    }
                }
            }
        }
        clipboard_msgs
    }

    /// Check for exited panes and clean them up. Returns list of closed pane IDs.
    fn cleanup_exited_panes(&mut self, clients: &mut HashMap<u64, ClientState>) -> Vec<u64> {
        let dead: Vec<u64> = self.panes.iter()
            .filter(|(_, p)| p.is_exited())
            .map(|(id, _)| *id)
            .collect();
        for &id in &dead {
            self.close_pane(id, clients);
        }
        dead
    }

    /// Build initial StateSync for a new client, including FullPaneSync for each pane.
    fn build_state_sync(&self) -> (ServerMessage, Vec<FullPaneSync>) {
        let pane_ids: Vec<u64> = self.workspaces.all_pane_ids();

        let syncs: Vec<FullPaneSync> = pane_ids.iter().filter_map(|&id| {
            let pane = self.panes.get(&id)?;
            let pgen = self.generation.get(&id).copied().unwrap_or(0);
            Some(pane.snapshot(pgen))
        }).collect();

        let msg = ServerMessage::StateSync {
            layout: self.layout_state(),
            pane_ids: pane_ids.clone(),
        };
        (msg, syncs)
    }
}

// ─── Server (multi-session) ─────────────────────────────────────────

struct Server {
    sessions: HashMap<String, Session>,
    clients: HashMap<u64, ClientState>,
    next_client_id: u64,
    next_pane_id: u64,
    default_shell: String,
    default_column_width: ColumnWidth,
    column_gap: f32,
    pane_inset: f32,
    /// True once server has had at least one session. Prevents premature
    /// shutdown on startup before any client has connected.
    had_session: bool,
}

impl Server {
    fn new(shell: &str, column_gap: f32) -> Self {
        Server {
            sessions: HashMap::new(),
            clients: HashMap::new(),
            next_client_id: 1,
            next_pane_id: 1,
            default_shell: shell.to_string(),
            default_column_width: ColumnWidth::Proportion(0.5),
            column_gap,
            pane_inset: 12.0,
            had_session: false,
        }
    }

    /// Get or create a session by name, restoring from saved state if available.
    fn get_or_create_session(&mut self, session_name: &str) -> &mut Session {
        if !self.sessions.contains_key(session_name) {
            self.had_session = true;
            let mut session = Session::new(session_name, &self.default_shell, self.column_gap);
            session.default_column_width = self.default_column_width;
            session.pane_inset = self.pane_inset;

            // Try restoring saved session
            let restored = ciri_session::restore::restore_session(session_name, &transport::state_dir())
                .ok()
                .flatten();
            if let Some(saved) = restored {
                log::info!("restoring session '{}' ({} workspaces)", session_name, saved.workspaces.len());
                session.workspaces.workspaces.clear();
                for saved_ws in &saved.workspaces {
                    let mut ws = ciri_layout::workspace::Workspace::new_with_gap(
                        session.workspaces.view_size,
                        session.workspaces.column_gap,
                    );
                    for saved_col in &saved_ws.columns {
                        if saved_col.tiles.is_empty() {
                            continue;
                        }
                        let vw = session.workspaces.view_size.width;
                        let vh = session.workspaces.view_size.height;
                        let col_w = (vw as f64 * saved_col.width_proportion) as f32;

                        // Restore every tile in this column (not just the first)
                        let mut col_opt: Option<ciri_layout::column::Column> = None;
                        for saved_tile in &saved_col.tiles {
                            let id = self.next_pane_id;
                            self.next_pane_id += 1;
                            let tile_h = vh / saved_col.tiles.len() as f32;
                            let (cols, rows) = session.pane_grid_size_with_cells(col_w, tile_h, 8.0, 16.0);
                            match Pane::new(id, cols, rows, &session.default_shell) {
                                Ok(pane) => {
                                    session.panes.insert(id, pane);
                                    session.generation.insert(id, 0);
                                    if let Some(col) = &mut col_opt {
                                        // Add as stacked tile with saved weight
                                        let mut tile = ciri_layout::tile::Tile::new(id);
                                        tile.height = ciri_layout::tile::TileHeight::Auto { weight: saved_tile.weight as f64 };
                                        col.tiles.push(tile);
                                    } else {
                                        // First tile: create the column
                                        let mut col = ciri_layout::column::Column::new(id);
                                        col.width = ColumnWidth::Proportion(saved_col.width_proportion);
                                        // Set weight on the first tile too
                                        if let Some(first_tile) = col.tiles.first_mut() {
                                            first_tile.height = ciri_layout::tile::TileHeight::Auto { weight: saved_tile.weight as f64 };
                                        }
                                        col_opt = Some(col);
                                    }
                                }
                                Err(e) => log::error!("failed to restore pane: {e}"),
                            }
                        }
                        if let Some(mut col) = col_opt {
                            col.active_tile_idx = saved_col.active_tile_idx
                                .min(col.tiles.len().saturating_sub(1));
                            ws.columns.push(col);
                        }
                    }
                    ws.active_column_idx = saved_ws.active_column_idx
                        .min(ws.columns.len().saturating_sub(1));
                    session.workspaces.workspaces.push(ws);
                }
                session.workspaces.active_workspace_idx = saved.active_workspace_idx
                    .min(session.workspaces.workspaces.len().saturating_sub(1));
            }

            // If restore produced no panes (all failed or no saved session), create a default one
            if session.panes.is_empty() {
                let vs = session.workspaces.view_size;
                let cg = session.workspaces.column_gap;
                session.workspaces.workspaces.clear();
                session.workspaces.workspaces.push(ciri_layout::workspace::Workspace::new_with_gap(vs, cg));
                if let Err(e) = session.create_pane(&mut self.next_pane_id, &mut self.clients) {
                    log::error!("failed to create initial pane for session '{}': {e}", session_name);
                }
            }

            self.sessions.insert(session_name.to_string(), session);
        }
        self.sessions.get_mut(session_name).unwrap()
    }

    /// Send a framed control message to a specific client.
    fn send_to_client(&self, client_id: u64, msg: &ServerMessage) {
        if let Some(client) = self.clients.get(&client_id) {
            if let Ok(payload) = rmp_serde::to_vec(msg) {
                let mut frame = Vec::with_capacity(5 + payload.len());
                frame.push(0x10);
                frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                frame.extend_from_slice(&payload);
                if let Err(e) = client.tx.try_send(frame) {
                    log::warn!("failed to send to client {client_id}: {e}");
                }
            }
        }
    }

    /// Broadcast a control message to all clients of a session.
    fn broadcast_to_session(&self, session_name: &str, msg: &ServerMessage) {
        if let Ok(payload) = rmp_serde::to_vec(msg) {
            let mut frame = Vec::with_capacity(5 + payload.len());
            frame.push(0x10);
            frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            frame.extend_from_slice(&payload);
            for client in self.clients.values() {
                if client.session_name == session_name {
                    if let Err(e) = client.tx.try_send(frame.clone()) {
                        log::warn!("failed to broadcast to client {}: {e}", client.id);
                    }
                }
            }
        }
    }

    fn handle_message(&mut self, msg: ClientMessage, client_id: u64) -> Vec<ServerResponse> {
        let mut responses = Vec::new();

        // Get the client's session name
        let session_name = match self.clients.get(&client_id) {
            Some(client) => client.session_name.clone(),
            None => return responses,
        };

        // Helper macro: for commands that need mutable session + mutable clients,
        // we temporarily remove the session from the map, operate on it, then put it back.
        match msg {
            ClientMessage::Input { pane_id, data } => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    if let Some(pane) = session.panes.get_mut(&pane_id) {
                        pane.write_to_pty(&data);
                    }
                }
            }
            ClientMessage::CreatePane => {
                if let Some(mut session) = self.sessions.remove(&session_name) {
                    match session.create_pane(&mut self.next_pane_id, &mut self.clients) {
                        Ok(id) => {
                            session.resize_all_panes(&mut self.clients);
                            session.mark_session_dirty();
                            let (cols, rows) = session.pane_grid_dims(id);
                            responses.push(ServerResponse::BroadcastToSession(session_name.clone(), ServerMessage::PaneCreated {
                                pane_id: id,
                                column_idx: session.workspaces.active().active_column_idx,
                                cols, rows,
                            }));
                            responses.push(ServerResponse::BroadcastToSession(session_name.clone(), ServerMessage::LayoutUpdate {
                                layout: session.layout_state(),
                            }));
                        }
                        Err(e) => log::error!("failed to create pane: {e}"),
                    }
                    self.sessions.insert(session_name, session);
                }
            }
            ClientMessage::SplitDown => {
                if let Some(mut session) = self.sessions.remove(&session_name) {
                    match session.create_pane_in_new_workspace(&mut self.next_pane_id, &mut self.clients) {
                        Ok(id) => {
                            session.resize_all_panes(&mut self.clients);
                            session.mark_session_dirty();
                            let (cols, rows) = session.pane_grid_dims(id);
                            responses.push(ServerResponse::BroadcastToSession(session_name.clone(), ServerMessage::PaneCreated {
                                pane_id: id,
                                column_idx: session.workspaces.active().active_column_idx,
                                cols, rows,
                            }));
                            responses.push(ServerResponse::BroadcastToSession(session_name.clone(), ServerMessage::LayoutUpdate {
                                layout: session.layout_state(),
                            }));
                        }
                        Err(e) => log::error!("failed to split: {e}"),
                    }
                    self.sessions.insert(session_name, session);
                }
            }
            ClientMessage::ClosePane { pane_id } => {
                if let Some(mut session) = self.sessions.remove(&session_name) {
                    session.close_pane(pane_id, &mut self.clients);
                    session.resize_all_panes(&mut self.clients);
                    session.mark_session_dirty();
                    responses.push(ServerResponse::BroadcastToSession(session_name.clone(), ServerMessage::PaneClosed { pane_id }));
                    responses.push(ServerResponse::BroadcastToSession(session_name.clone(), ServerMessage::LayoutUpdate {
                        layout: session.layout_state(),
                    }));
                    self.sessions.insert(session_name, session);
                }
            }
            ClientMessage::FocusLeft => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    session.workspaces.active_mut().focus_left();
                    session.mark_session_dirty();
                    responses.push(ServerResponse::BroadcastToSession(session_name, ServerMessage::LayoutUpdate {
                        layout: session.layout_state(),
                    }));
                }
            }
            ClientMessage::FocusRight => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    session.workspaces.active_mut().focus_right();
                    session.mark_session_dirty();
                    responses.push(ServerResponse::BroadcastToSession(session_name, ServerMessage::LayoutUpdate {
                        layout: session.layout_state(),
                    }));
                }
            }
            ClientMessage::FocusUp => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    if !session.workspaces.active_mut().focus_tile_up() {
                        session.workspaces.focus_up();
                    }
                    session.mark_session_dirty();
                    responses.push(ServerResponse::BroadcastToSession(session_name, ServerMessage::LayoutUpdate {
                        layout: session.layout_state(),
                    }));
                }
            }
            ClientMessage::FocusDown => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    if !session.workspaces.active_mut().focus_tile_down() {
                        session.workspaces.focus_down();
                    }
                    session.mark_session_dirty();
                    responses.push(ServerResponse::BroadcastToSession(session_name, ServerMessage::LayoutUpdate {
                        layout: session.layout_state(),
                    }));
                }
            }
            ClientMessage::MovePaneLeft => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    session.workspaces.active_mut().move_pane_left();
                    session.mark_session_dirty();
                    responses.push(ServerResponse::BroadcastToSession(session_name, ServerMessage::LayoutUpdate {
                        layout: session.layout_state(),
                    }));
                }
            }
            ClientMessage::MovePaneRight => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    session.workspaces.active_mut().move_pane_right();
                    session.mark_session_dirty();
                    responses.push(ServerResponse::BroadcastToSession(session_name, ServerMessage::LayoutUpdate {
                        layout: session.layout_state(),
                    }));
                }
            }
            ClientMessage::Resize { cols: _, rows: _, width, height, cell_width, cell_height } => {
                log::debug!("client {client_id} Resize: {width}x{height}px, cell={cell_width:.1}x{cell_height:.1}");
                if cell_width.is_finite() && cell_width > 0.0 && cell_width <= 200.0
                    && cell_height.is_finite() && cell_height > 0.0 && cell_height <= 200.0
                    && width > 0 && width <= 16384 && height > 0 && height <= 16384
                {
                    if let Some(client) = self.clients.get_mut(&client_id) {
                        client.cell_width = cell_width;
                        client.cell_height = cell_height;
                        client.viewport_width = width as f32;
                        client.viewport_height = height as f32;
                    }
                    if let Some(mut session) = self.sessions.remove(&session_name) {
                        session.resize_all_panes(&mut self.clients);
                        responses.push(ServerResponse::BroadcastToSession(session_name.clone(), ServerMessage::LayoutUpdate {
                            layout: session.layout_state(),
                        }));
                        self.sessions.insert(session_name, session);
                    }
                } else {
                    log::warn!("ignoring invalid resize from client {client_id}: {width}x{height} cell={cell_width}x{cell_height}");
                }
            }
            ClientMessage::SetColumnWidth { proportion, fixed_px } => {
                if let Some(mut session) = self.sessions.remove(&session_name) {
                    let width = match fixed_px {
                        Some(px) => ColumnWidth::Fixed(px),
                        None => ColumnWidth::Proportion(proportion),
                    };
                    session.workspaces.active_mut().set_active_column_width(width);
                    session.mark_session_dirty();
                    session.resize_all_panes(&mut self.clients);
                    responses.push(ServerResponse::BroadcastToSession(session_name.clone(), ServerMessage::LayoutUpdate {
                        layout: session.layout_state(),
                    }));
                    self.sessions.insert(session_name, session);
                }
            }
            ClientMessage::AdjustColumnSplit { delta } => {
                if let Some(mut session) = self.sessions.remove(&session_name) {
                    session.workspaces.active_mut().resize_active_with_neighbor(delta);
                    session.mark_session_dirty();
                    session.resize_all_panes(&mut self.clients);
                    responses.push(ServerResponse::BroadcastToSession(session_name.clone(), ServerMessage::LayoutUpdate {
                        layout: session.layout_state(),
                    }));
                    self.sessions.insert(session_name, session);
                }
            }
            ClientMessage::EqualizeColumnSplit => {
                if let Some(mut session) = self.sessions.remove(&session_name) {
                    session.workspaces.active_mut().equalize_active_with_neighbor();
                    session.mark_session_dirty();
                    session.resize_all_panes(&mut self.clients);
                    responses.push(ServerResponse::BroadcastToSession(session_name.clone(), ServerMessage::LayoutUpdate {
                        layout: session.layout_state(),
                    }));
                    self.sessions.insert(session_name, session);
                }
            }
            ClientMessage::SetTileWeights { column_idx, top_tile_idx, top_weight, bottom_weight } => {
                if let Some(mut session) = self.sessions.remove(&session_name) {
                    let ws = session.workspaces.active_mut();
                    if let Some(col) = ws.columns.get_mut(column_idx) {
                        col.set_tile_weights(top_tile_idx, top_weight, bottom_weight);
                    }
                    session.mark_session_dirty();
                    session.resize_all_panes(&mut self.clients);
                    responses.push(ServerResponse::BroadcastToSession(session_name.clone(), ServerMessage::LayoutUpdate {
                        layout: session.layout_state(),
                    }));
                    self.sessions.insert(session_name, session);
                }
            }
            ClientMessage::AdjustColumnSplitAt { column_idx, delta } => {
                if let Some(mut session) = self.sessions.remove(&session_name) {
                    let ws = session.workspaces.active_mut();
                    let saved_idx = ws.active_column_idx;
                    ws.active_column_idx = column_idx;
                    ws.resize_active_with_neighbor(delta);
                    ws.active_column_idx = saved_idx;
                    session.mark_session_dirty();
                    session.resize_all_panes(&mut self.clients);
                    responses.push(ServerResponse::BroadcastToSession(session_name.clone(), ServerMessage::LayoutUpdate {
                        layout: session.layout_state(),
                    }));
                    self.sessions.insert(session_name, session);
                }
            }
            ClientMessage::ConsumeIntoColumn => {
                if let Some(mut session) = self.sessions.remove(&session_name) {
                    session.workspaces.active_mut().consume_from_right();
                    session.resize_all_panes(&mut self.clients);
                    session.mark_session_dirty();
                    responses.push(ServerResponse::BroadcastToSession(session_name.clone(), ServerMessage::LayoutUpdate {
                        layout: session.layout_state(),
                    }));
                    self.sessions.insert(session_name, session);
                }
            }
            ClientMessage::ExpelFromColumn => {
                if let Some(mut session) = self.sessions.remove(&session_name) {
                    session.workspaces.active_mut().expel_active_tile();
                    session.resize_all_panes(&mut self.clients);
                    session.mark_session_dirty();
                    responses.push(ServerResponse::BroadcastToSession(session_name.clone(), ServerMessage::LayoutUpdate {
                        layout: session.layout_state(),
                    }));
                    self.sessions.insert(session_name, session);
                }
            }
            ClientMessage::Attach => {
                // Viewport already applied during ClientHello handshake
            }
            ClientMessage::Detach => {
                responses.push(ServerResponse::RemoveClient(client_id));
            }
            ClientMessage::Ack { generation } => {
                if let Some(client) = self.clients.get_mut(&client_id) {
                    client.last_acked_generation = generation;
                }
            }
            ClientMessage::SwitchWorkspace { workspace_idx } => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    session.workspaces.switch_to(workspace_idx);
                    session.mark_session_dirty();
                    responses.push(ServerResponse::BroadcastToSession(session_name, ServerMessage::LayoutUpdate {
                        layout: session.layout_state(),
                    }));
                }
            }
            ClientMessage::MouseInput { pane_id, button, col, row, pressed, modifiers } => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    if let Some(pane) = session.panes.get(&pane_id) {
                        if pane.has_mouse_mode() {
                            pane.send_mouse_input(button, col, row, pressed, modifiers);
                        }
                    }
                }
            }
            ClientMessage::ListSessions => {
                // Collect running sessions
                let mut session_map: HashMap<String, SessionInfo> = HashMap::new();
                for (name, sess) in &self.sessions {
                    let client_count = self.clients.values()
                        .filter(|c| c.session_name == *name)
                        .count();
                    session_map.insert(name.clone(), SessionInfo {
                        name: name.clone(),
                        running: true,
                        pane_count: sess.panes.len(),
                        client_count,
                    });
                }
                // Merge with saved sessions
                if let Ok(saved_names) = ciri_session::restore::list_sessions(&transport::state_dir()) {
                    for name in saved_names {
                        session_map.entry(name.clone()).or_insert(SessionInfo {
                            name,
                            running: false,
                            pane_count: 0,
                            client_count: 0,
                        });
                    }
                }
                let mut sessions: Vec<SessionInfo> = session_map.into_values().collect();
                sessions.sort_by(|a, b| a.name.cmp(&b.name));
                responses.push(ServerResponse::SendToClient(client_id, ServerMessage::SessionList { sessions }));
            }
            ClientMessage::KillSession { session_name: target } => {
                if let Some(session) = self.sessions.remove(&target) {
                    // Kill all panes in the session
                    drop(session);
                    // Delete saved session file
                    let _ = ciri_session::restore::delete_session(&target, &transport::state_dir());
                    // Notify clients attached to the killed session
                    let affected_clients: Vec<u64> = self.clients.iter()
                        .filter(|(_, c)| c.session_name == target)
                        .map(|(id, _)| *id)
                        .collect();
                    for cid in affected_clients {
                        self.send_to_client(cid, &ServerMessage::ServerShutdown);
                        responses.push(ServerResponse::RemoveClient(cid));
                    }
                    // Notify the requesting client that the session was killed
                    responses.push(ServerResponse::SendToClient(client_id, ServerMessage::SessionKilled { session_name: target }));
                } else {
                    // Try to delete saved session file even if not running
                    let _ = ciri_session::restore::delete_session(&target, &transport::state_dir());
                    responses.push(ServerResponse::SendToClient(client_id, ServerMessage::SessionKilled { session_name: target }));
                }
            }
            ClientMessage::KillServer => {
                responses.push(ServerResponse::ShutdownServer);
            }
            ClientMessage::SwitchSession { session_name: target } => {
                if ciri_session::save::validate_session_name(&target).is_err() {
                    responses.push(ServerResponse::SendToClient(client_id, ServerMessage::Error {
                        message: format!("invalid session name: {target:?}"),
                    }));
                    return responses;
                }

                let old_session_name = session_name.clone();

                // Update client's session affinity
                if let Some(client) = self.clients.get_mut(&client_id) {
                    client.session_name = target.clone();
                    // Clear old damage and history
                    client.damage.clear();
                    client.history_sent.clear();
                }

                // Get or create target session
                self.get_or_create_session(&target);

                // Temporarily remove the new session to work with it + clients
                if let Some(mut new_session) = self.sessions.remove(&target) {
                    new_session.resize_all_panes(&mut self.clients);

                    // Mark all panes for full sync for this client
                    let pane_keys: Vec<u64> = new_session.panes.keys().copied().collect();
                    if let Some(client) = self.clients.get_mut(&client_id) {
                        for pane_id in &pane_keys {
                            let mut acc = DamageAccumulator::new();
                            acc.mark_full();
                            client.damage.insert(*pane_id, acc);
                        }
                    }

                    // Build state sync for the new session
                    let (sync_msg, pane_syncs) = new_session.build_state_sync();

                    // Send SessionSwitched, then StateSync, then FullPaneSyncs
                    responses.push(ServerResponse::SendToClient(client_id, ServerMessage::SessionSwitched {
                        session_name: target.clone(),
                    }));
                    responses.push(ServerResponse::SendToClient(client_id, sync_msg));
                    for sync in pane_syncs {
                        responses.push(ServerResponse::SendFullPaneSync(client_id, sync));
                    }

                    // Record history_sent for the new session's panes
                    let pane_histories: Vec<(u64, usize)> = new_session.panes.iter()
                        .map(|(&pid, pane)| (pid, pane.history_size()))
                        .collect();
                    if let Some(client) = self.clients.get_mut(&client_id) {
                        for (pane_id, history) in pane_histories {
                            client.history_sent.insert(pane_id, history);
                        }
                        // Clear damage markers (we just sent full sync)
                        for acc in client.damage.values_mut() {
                            *acc = DamageAccumulator::new();
                        }
                    }

                    // Put new session back
                    self.sessions.insert(target.clone(), new_session);
                }

                // Resize panes in old session (client left, viewport may change)
                if old_session_name != target {
                    if let Some(mut old_session) = self.sessions.remove(&old_session_name) {
                        old_session.resize_all_panes(&mut self.clients);
                        self.sessions.insert(old_session_name, old_session);
                    }
                }
            }
        }

        responses
    }
}

/// Internal response type for message handling.
enum ServerResponse {
    BroadcastToSession(String, ServerMessage),
    SendToClient(u64, ServerMessage),
    SendFullPaneSync(u64, FullPaneSync),
    RemoveClient(u64),
    ShutdownServer,
}

// ─── Graceful shutdown helper ───────────────────────────────────────

/// Perform graceful shutdown: save all sessions, notify all clients, remove socket.
async fn graceful_shutdown(state: &Arc<Mutex<Server>>) {
    let s = state.lock().await;
    // Save all sessions
    for session in s.sessions.values() {
        let _ = session.save_session();
    }
    log::info!("shutting down gracefully");

    // Send shutdown to all clients
    if let Ok(payload) = rmp_serde::to_vec(&ServerMessage::ServerShutdown) {
        for client in s.clients.values() {
            let mut frame = Vec::with_capacity(5 + payload.len());
            frame.push(0x10);
            frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            frame.extend_from_slice(&payload);
            if let Err(e) = client.tx.try_send(frame) {
                log::warn!("failed to send shutdown to client {}: {e}", client.id);
            }
        }
    }
    drop(s);
    let _ = std::fs::remove_file(&transport::server_socket_path());
}

// ─── Client cleanup helper ──────────────────────────────────────────

/// Ensures client is removed from server state. Safe to call multiple times.
async fn cleanup_client(state: &Arc<Mutex<Server>>, client_id: u64) {
    let mut s = state.lock().await;
    if s.clients.remove(&client_id).is_some() {
        log::info!("client {client_id} disconnected");
    }
}

// ─── Main daemon loop ───────────────────────────────────────────────

pub async fn run_daemon() -> Result<()> {
    let config = ciri_config::config::CiriConfig::load().unwrap_or_default();
    let shell = config.terminal.shell.clone();

    let sock_path = transport::server_socket_path();
    if let Some(parent) = sock_path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }

    #[cfg(unix)]
    {
        if sock_path.exists() {
            std::fs::remove_file(&sock_path)?;
        }
    }

    #[cfg(unix)]
    let listener = UnixListener::bind(&sock_path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o700);
        if let Err(e) = std::fs::set_permissions(&sock_path, perms) {
            log::warn!("failed to set socket permissions: {e}");
        }
    }
    #[cfg(windows)]
    let listener = TcpListener::bind(
        format!("127.0.0.1:{}", transport::server_port())
    ).await?;

    // On Windows, create a marker file so list_running_sessions can discover us
    #[cfg(windows)]
    {
        std::fs::write(&sock_path, transport::server_port().to_string())?;
    }

    log::info!("ciri-server listening on {}", sock_path.display());

    let mut server = Server::new(&shell, config.appearance.column_gap);
    // Apply default_column_width from config
    if let Some(ref pw) = config.layout.default_column_width {
        use ciri_config::config::PresetWidth;
        server.default_column_width = match pw {
            PresetWidth::Proportion { proportion } => ColumnWidth::Proportion(*proportion),
            PresetWidth::Fixed { fixed } => ColumnWidth::Fixed(*fixed),
        };
    }
    // If no explicit default_column_width, keep the default (0.5)
    server.pane_inset = (config.appearance.padding + config.appearance.border_width) * 2.0;
    let state = Arc::new(Mutex::new(server));

    // Shutdown signal shared between tick loop, signal handler, and accept loop
    let shutdown = Arc::new(Notify::new());

    // Spawn tick loop (16ms = ~60fps)
    let tick_state = state.clone();
    let tick_shutdown = shutdown.clone();
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_millis(16));

        // ── Frame buffer pool (optimization #2) ─────────────────────
        // Reusable Vec<u8> buffers to avoid per-frame allocation.
        // Capped at 64 to bound memory usage.
        const FRAME_POOL_CAP: usize = 64;
        let mut frame_pool: Vec<Vec<u8>> = Vec::with_capacity(FRAME_POOL_CAP);

        loop {
            ticker.tick().await;

            // ── Phase 1 (locked): process PTY, extract damage, collect snapshots ──
            //
            // Snapshot-then-release pattern (optimization #5): we read all pane
            // data while holding the lock, collect it into lightweight snapshot
            // structs, clone the tx handles we need, then drop the lock before
            // doing any encoding or sending.

            /// Raw snapshot data extracted under the lock for deferred encoding.
            enum Snapshot {
                FullSync {
                    sync: FullPaneSync,
                    current_history: usize,
                    pane_id: u64,
                },
                Delta(CellDelta),
            }

            struct PendingSend {
                client_id: u64,
                session_name: String,
                snapshot: Snapshot,
            }

            let mut pending_sends: Vec<PendingSend> = Vec::new();
            let mut should_shutdown = false;

            {
                let mut s = tick_state.lock().await;

                // Iterate all sessions
                let session_names: Vec<String> = s.sessions.keys().cloned().collect();
                let mut sessions_to_remove: Vec<String> = Vec::new();

                for session_name in &session_names {
                    // Temporarily remove session from the map to avoid borrow conflicts
                    // between session and s.clients.
                    let mut session = match s.sessions.remove(session_name) {
                        Some(sess) => sess,
                        None => continue,
                    };

                    // Process PTY output and extract damage
                    let clipboard_msgs = session.process_pty_and_damage(&mut s.clients);

                    // Send OSC 52 clipboard writes to clients of this session
                    for clip_msg in &clipboard_msgs {
                        if let Ok(payload) = rmp_serde::to_vec(clip_msg) {
                            let mut frame = frame_pool.pop().unwrap_or_default();
                            frame.clear();
                            frame.reserve(5 + payload.len());
                            frame.push(0x10); // TAG_SERVER_MSG
                            frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                            frame.extend_from_slice(&payload);
                            for client in s.clients.values() {
                                if client.session_name == *session_name {
                                    let _ = client.tx.try_send(frame.clone());
                                }
                            }
                            if frame_pool.len() < FRAME_POOL_CAP {
                                frame_pool.push(frame);
                            }
                        }
                    }

                    // Clean up exited panes
                    let dead = session.cleanup_exited_panes(&mut s.clients);
                    if !dead.is_empty() {
                        session.mark_session_dirty();
                        // Broadcast close messages to session clients
                        let mut broadcasts = Vec::new();
                        for &id in &dead {
                            let close_payload = rmp_serde::to_vec(&ServerMessage::PaneClosed { pane_id: id })
                                .unwrap_or_default();
                            broadcasts.push(close_payload);
                        }
                        let layout_payload = rmp_serde::to_vec(&ServerMessage::LayoutUpdate {
                            layout: session.layout_state(),
                        }).unwrap_or_default();
                        broadcasts.push(layout_payload);

                        for client in s.clients.values() {
                            if client.session_name == *session_name {
                                for payload in &broadcasts {
                                    let mut frame = frame_pool.pop().unwrap_or_default();
                                    frame.clear();
                                    frame.reserve(5 + payload.len());
                                    frame.push(0x10);
                                    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                                    frame.extend_from_slice(payload);
                                    if let Err(e) = client.tx.try_send(frame.clone()) {
                                        log::warn!("failed to send close frame to client {}: {e}", client.id);
                                    }
                                    if frame_pool.len() < FRAME_POOL_CAP {
                                        frame_pool.push(frame);
                                    }
                                }
                            }
                        }
                    }

                    session.autosave_if_due(Instant::now());

                    // If session has no panes left, mark for removal
                    if session.panes.is_empty() {
                        let _ = ciri_session::restore::delete_session(
                            session_name, &transport::state_dir()
                        );
                        log::info!("session '{}': all panes exited, removing session", session_name);

                        // Send shutdown to clients of this session
                        if let Ok(payload) = rmp_serde::to_vec(&ServerMessage::ServerShutdown) {
                            for client in s.clients.values() {
                                if client.session_name == *session_name {
                                    let mut frame = frame_pool.pop().unwrap_or_default();
                                    frame.clear();
                                    frame.reserve(5 + payload.len());
                                    frame.push(0x10);
                                    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                                    frame.extend_from_slice(&payload);
                                    if let Err(e) = client.tx.try_send(frame.clone()) {
                                        log::warn!("failed to send shutdown to client {}: {e}", client.id);
                                    }
                                    if frame_pool.len() < FRAME_POOL_CAP {
                                        frame_pool.push(frame);
                                    }
                                }
                            }
                        }

                        // Remove clients of this session
                        let to_remove: Vec<u64> = s.clients.iter()
                            .filter(|(_, c)| c.session_name == *session_name)
                            .map(|(id, _)| *id)
                            .collect();
                        for cid in to_remove {
                            s.clients.remove(&cid);
                        }

                        sessions_to_remove.push(session_name.clone());
                        // Don't re-insert this session
                    } else {
                        // Collect damage snapshots for clients of this session
                        let session_client_ids: Vec<u64> = s.clients.iter()
                            .filter(|(_, c)| c.session_name == *session_name)
                            .map(|(id, _)| *id)
                            .collect();

                        let mut pending: Vec<(u64, u64, DamageAccumulator)> = Vec::new();
                        for &cid in &session_client_ids {
                            if let Some(client) = s.clients.get_mut(&cid) {
                                let pane_ids: Vec<u64> = client.damage.keys().copied().collect();
                                for pane_id in pane_ids {
                                    let acc = client.damage.get_mut(&pane_id).unwrap();
                                    if !acc.is_empty() {
                                        pending.push((cid, pane_id, acc.take()));
                                    }
                                }
                            }
                        }

                        // Phase 1: read pane data under lock, build snapshot structs,
                        // clone tx handles for deferred sending.
                        for (cid, pane_id, damage) in pending {
                            let pgen = session.generation.get(&pane_id).copied().unwrap_or(0);
                            if !s.clients.contains_key(&cid) {
                                continue;
                            }

                            if damage.full {
                                if let Some(pane) = session.panes.get(&pane_id) {
                                    let last_sent = s.clients.get(&cid)
                                        .and_then(|c| c.history_sent.get(&pane_id).copied())
                                        .unwrap_or(0);
                                    let sync = pane.snapshot_incremental(pgen, last_sent);
                                    let current_history = pane.history_size();
                                    pending_sends.push(PendingSend {
                                        client_id: cid,
                                        session_name: session_name.clone(),
                                        snapshot: Snapshot::FullSync { sync, current_history, pane_id },
                                    });
                                }
                            } else if let Some(pane) = session.panes.get(&pane_id) {
                                let (cursor_line, cursor_col, cursor_shape, mode_flags) = pane.cursor_info();
                                let mut regions = Vec::new();

                                for (&line, &(left, right)) in &damage.line_damage {
                                    let cells = pane.read_cells(line, left, right);
                                    regions.push(DamageRegion { line, left, right, cells });
                                }

                                let delta = CellDelta {
                                    pane_id,
                                    generation: pgen,
                                    cursor_line,
                                    cursor_col,
                                    cursor_shape,
                                    mode_flags,
                                    regions,
                                };
                                pending_sends.push(PendingSend {
                                    client_id: cid,
                                    session_name: session_name.clone(),
                                    snapshot: Snapshot::Delta(delta),
                                });
                            }
                        }

                        // Put session back
                        s.sessions.insert(session_name.clone(), session);
                    }
                }

                // Remove dead sessions
                for name in sessions_to_remove {
                    s.sessions.remove(&name);
                }

                // Server shutdown: had sessions before but now all gone and no clients
                if s.had_session && s.sessions.is_empty() && s.clients.is_empty() {
                    log::info!("all sessions ended and no clients, shutting down server");
                    should_shutdown = true;
                }
            } // lock dropped here — Phase 1 complete

            // Signal shutdown
            if should_shutdown {
                let _ = std::fs::remove_file(&transport::server_socket_path());
                tick_shutdown.notify_one();
                return;
            }

            // ── Phase 2 (unlocked): encode snapshots, then re-lock to validate + send ──
            //
            // Encoding (RLE, bytemuck serialization) happens without holding
            // the server lock. We use the frame pool to avoid per-frame alloc.
            //
            // After encoding, we re-acquire the lock to validate client session
            // affinity before sending. A client may have switched sessions
            // (SwitchSession) between Phase 1 and Phase 2; sending old-session
            // data to such a client would corrupt its state.
            if !pending_sends.is_empty() {
                struct EncodedFrame {
                    client_id: u64,
                    session_name: String,
                    buf: Vec<u8>,
                    history_update: Option<(u64, usize)>,
                }
                let mut encoded: Vec<EncodedFrame> = Vec::new();

                for PendingSend { client_id, session_name, snapshot } in pending_sends.drain(..) {
                    let mut buf = frame_pool.pop().unwrap_or_default();
                    let (history_update, encode_ok) = match &snapshot {
                        Snapshot::FullSync { sync, current_history, pane_id } => {
                            let ok = codec::encode_full_pane_sync_framed(&mut buf, sync).is_ok();
                            (Some((*pane_id, *current_history)), ok)
                        }
                        Snapshot::Delta(delta) => {
                            let ok = codec::encode_cell_delta_framed(&mut buf, delta).is_ok();
                            (None, ok)
                        }
                    };

                    if encode_ok {
                        encoded.push(EncodedFrame { client_id, session_name, buf, history_update });
                    } else {
                        // Return buffer to pool on encode failure
                        if frame_pool.len() < FRAME_POOL_CAP {
                            frame_pool.push(buf);
                        }
                    }
                }

                // Re-lock to validate affinity and send
                let mut s = tick_state.lock().await;
                let mut to_disconnect = Vec::new();
                for EncodedFrame { client_id, session_name, buf, history_update } in encoded.drain(..) {
                    if let Some(client) = s.clients.get_mut(&client_id) {
                        // Revalidate client affinity: if the client switched sessions
                        // between Phase 1 and now, drop the stale frame.
                        if client.session_name != session_name {
                            log::debug!(
                                "client {client_id} switched session ({session_name} -> {}), dropping stale frame",
                                client.session_name
                            );
                            if frame_pool.len() < FRAME_POOL_CAP {
                                frame_pool.push(buf);
                            }
                            continue;
                        }
                        match client.tx.try_send(buf) {
                            Ok(()) => {
                                // buf consumed by channel — do not return to pool
                                client.send_failures = 0;
                                if let Some((pid, hist)) = history_update {
                                    client.history_sent.insert(pid, hist);
                                }
                            }
                            Err(e) => {
                                client.send_failures += 1;
                                if client.send_failures >= 100 {
                                    log::warn!("disconnecting slow client {client_id}: {} consecutive failures", client.send_failures);
                                    to_disconnect.push(client_id);
                                } else {
                                    log::debug!("send to client {client_id} failed (#{})", client.send_failures);
                                }
                                // On failure for FullPaneSync, re-mark full so it retries next tick
                                if let Some((pid, _)) = history_update {
                                    client.damage.entry(pid).or_insert_with(DamageAccumulator::new).mark_full();
                                }
                                // Recover the buffer and return it to the pool
                                let buf = e.into_inner();
                                if frame_pool.len() < FRAME_POOL_CAP {
                                    frame_pool.push(buf);
                                }
                            }
                        }
                    } else {
                        // Client not found — return buffer to pool
                        if frame_pool.len() < FRAME_POOL_CAP {
                            frame_pool.push(buf);
                        }
                    }
                }
                for cid in to_disconnect {
                    s.clients.remove(&cid);
                }
            }
        }
    });

    // Signal handling - SIGTERM and SIGINT
    let signal_state = state.clone();
    let signal_shutdown = shutdown.clone();
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm = signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");
            let mut sigint = signal(SignalKind::interrupt()).expect("failed to register SIGINT handler");
            tokio::select! {
                _ = sigterm.recv() => {
                    log::info!("received SIGTERM");
                }
                _ = sigint.recv() => {
                    log::info!("received SIGINT");
                }
            }
            graceful_shutdown(&signal_state).await;
            signal_shutdown.notify_one();
        }
        #[cfg(windows)]
        {
            let _ = tokio::signal::ctrl_c().await;
            log::info!("received Ctrl-C");
            graceful_shutdown(&signal_state).await;
            signal_shutdown.notify_one();
        }
    });

    // Accept connections, with graceful shutdown via select!
    loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, _) = result?;
                let state = state.clone();
                let client_shutdown = shutdown.clone();

                tokio::spawn(async move {
                    let (reader, writer) = stream.into_split();
                    let mut reader = tokio::io::BufReader::new(reader);
                    let mut writer = BufWriter::new(writer);

                    // Read ClientHello (version + viewport + session name)
                    let hello = match codec::read_client_hello(&mut reader).await {
                        Ok((codec::VersionCompat::Exact(v), h)) => {
                            log::info!("client handshake ok (v{v}), session={}", h.session_name);
                            h
                        }
                        Ok((codec::VersionCompat::PatchMismatch { peer, local }, h)) => {
                            log::warn!("client version {peer} differs from server {local} (patch mismatch)");
                            h
                        }
                        Ok((codec::VersionCompat::MinorMismatch { peer, local }, h)) => {
                            log::warn!("client version {peer} differs from server {local} (minor mismatch, may be unstable)");
                            h
                        }
                        Err(e) => {
                            log::debug!("client hello rejected: {e}");
                            return;
                        }
                    };

                    let requested_session = hello.session_name.clone();
                    log::info!("client requested session: {}", requested_session);

                    // Validate session name (allow __control__ for CLI commands)
                    let is_control = requested_session == "__control__";
                    if !is_control && ciri_session::save::validate_session_name(&requested_session).is_err() {
                        log::error!("invalid session name from client: {:?}", requested_session);
                        return;
                    }

                    let client_viewport_w = hello.width as f32;
                    let client_viewport_h = hello.height as f32;
                    let client_cell_w = hello.cell_width;
                    let client_cell_h = hello.cell_height;

                    // Send ServerHello
                    if let Err(e) = codec::write_server_hello(&mut writer).await {
                        log::error!("failed to send server hello: {e}");
                        return;
                    }

                    // Register client and get/create session
                    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(256);
                    let client_id;
                    let initial_frames: Vec<Vec<u8>>;
                    {
                        let mut s = state.lock().await;
                        client_id = s.next_client_id;
                        s.next_client_id += 1;

                        // Register client with session affinity
                        s.clients.insert(client_id, ClientState {
                            id: client_id,
                            tx: tx.clone(),
                            damage: HashMap::new(),
                            last_acked_generation: 0,
                            history_sent: HashMap::new(),
                            send_failures: 0,
                            cell_width: client_cell_w,
                            cell_height: client_cell_h,
                            viewport_width: client_viewport_w,
                            viewport_height: client_viewport_h,
                            session_name: requested_session.clone(),
                        });

                        // Skip session creation for control clients (CLI commands)
                        if !is_control {
                            s.get_or_create_session(&requested_session);
                        }

                        // Temporarily remove session to avoid borrow conflicts
                        let session_opt = s.sessions.remove(&requested_session);

                        if let Some(ref session) = session_opt {
                            // Mark all session panes for full sync for this client
                            let pane_keys: Vec<u64> = session.panes.keys().copied().collect();
                            if let Some(client) = s.clients.get_mut(&client_id) {
                                for pane_id in &pane_keys {
                                    let mut acc = DamageAccumulator::new();
                                    acc.mark_full();
                                    client.damage.insert(*pane_id, acc);
                                }
                            }
                        }

                        // Recompute effective viewport and build initial sync frames
                        let mut frames = Vec::new();
                        if let Some(mut session) = session_opt {
                            session.resize_all_panes(&mut s.clients);

                            log::info!("client {client_id} connected to session '{}'", requested_session);

                            let (sync_msg, pane_syncs) = session.build_state_sync();
                            if let Ok(payload) = rmp_serde::to_vec(&sync_msg) {
                                let mut frame = Vec::with_capacity(5 + payload.len());
                                frame.push(0x10);
                                frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                                frame.extend_from_slice(&payload);
                                frames.push(frame);
                            }
                            for sync in &pane_syncs {
                                if let Ok(payload) = codec::encode_full_pane_sync_payload(sync) {
                                    let mut frame = Vec::with_capacity(5 + payload.len());
                                    frame.push(0x21);
                                    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                                    frame.extend_from_slice(&payload);
                                    frames.push(frame);
                                }
                            }

                            let pane_histories: Vec<(u64, usize)> = session.panes.iter().map(|(&pid, pane)| {
                                (pid, pane.history_size())
                            }).collect();

                            if let Some(client) = s.clients.get_mut(&client_id) {
                                for acc in client.damage.values_mut() {
                                    *acc = DamageAccumulator::new();
                                }
                                for (pane_id, history) in pane_histories {
                                    client.history_sent.insert(pane_id, history);
                                }
                            }

                            s.sessions.insert(requested_session.clone(), session);
                        } else {
                            log::info!("control client {client_id} connected (no session)");
                        }

                        initial_frames = frames;
                    } // lock dropped here

                    // Send frames without holding the lock
                    for frame in initial_frames {
                        let _ = tx.send(frame).await;
                    }

                    // Spawn writer task
                    let write_handle = tokio::spawn(async move {
                        while let Some(frame) = rx.recv().await {
                            if writer.write_all(&frame).await.is_err() {
                                break;
                            }
                            if writer.flush().await.is_err() {
                                break;
                            }
                        }
                    });

                    // Reader loop with guaranteed cleanup on panic or error
                    let reader_result = std::panic::AssertUnwindSafe(async {
                        loop {
                            match codec::read_frame(&mut reader).await {
                                Ok(codec::Frame::ClientMsg(msg)) => {
                                    let mut s = state.lock().await;
                                    let responses = s.handle_message(msg, client_id);

                                    for resp in responses {
                                        match resp {
                                            ServerResponse::BroadcastToSession(session_name, server_msg) => {
                                                s.broadcast_to_session(&session_name, &server_msg);
                                            }
                                            ServerResponse::SendToClient(cid, server_msg) => {
                                                s.send_to_client(cid, &server_msg);
                                            }
                                            ServerResponse::SendFullPaneSync(cid, sync) => {
                                                if let Some(client) = s.clients.get(&cid) {
                                                    if let Ok(payload) = codec::encode_full_pane_sync_payload(&sync) {
                                                        let mut frame = Vec::with_capacity(5 + payload.len());
                                                        frame.push(0x21);
                                                        frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                                                        frame.extend_from_slice(&payload);
                                                        if let Err(e) = client.tx.try_send(frame) {
                                                            log::warn!("failed to send full pane sync to client {cid}: {e}");
                                                        }
                                                    }
                                                }
                                            }
                                            ServerResponse::RemoveClient(cid) => {
                                                s.clients.remove(&cid);
                                                log::info!("client {cid} detached");
                                                if cid == client_id {
                                                    return;
                                                }
                                            }
                                            ServerResponse::ShutdownServer => {
                                                // Save all sessions, notify all clients
                                                for session in s.sessions.values() {
                                                    let _ = session.save_session();
                                                }
                                                if let Ok(payload) = rmp_serde::to_vec(&ServerMessage::ServerShutdown) {
                                                    for client in s.clients.values() {
                                                        let mut frame = Vec::with_capacity(5 + payload.len());
                                                        frame.push(0x10);
                                                        frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                                                        frame.extend_from_slice(&payload);
                                                        let _ = client.tx.try_send(frame);
                                                    }
                                                }
                                                drop(s);
                                                let _ = std::fs::remove_file(&transport::server_socket_path());
                                                // Signal the accept loop and tick loop to shut down
                                                client_shutdown.notify_one();
                                                return;
                                            }
                                        }
                                    }
                                }
                                Ok(_) => {
                                    log::warn!("unexpected frame type from client {client_id}");
                                }
                                Err(e) => {
                                    if e.kind() != std::io::ErrorKind::UnexpectedEof {
                                        log::warn!("client {client_id} read error: {e}");
                                    }
                                    break;
                                }
                            }
                        }
                    }).await;

                    // Cleanup always runs
                    let _ = reader_result;
                    cleanup_client(&state, client_id).await;
                    write_handle.abort();
                });
            }
            _ = shutdown.notified() => {
                log::info!("accept loop shutting down");
                break;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autosave_is_debounced() {
        let mut session = Session::new("default", "/bin/sh", 8.0);
        session.session_dirty = true;
        let changed_at = Instant::now();
        session.last_session_change = Some(changed_at);

        assert!(!session.autosave_due_at(changed_at));
        assert!(!session.autosave_due_at(changed_at + Duration::from_millis(249)));
        assert!(session.autosave_due_at(changed_at + Duration::from_millis(250)));
    }

    #[test]
    fn mark_session_dirty_sets_dirty_and_timestamp() {
        let mut session = Session::new("default", "/bin/sh", 8.0);
        assert!(!session.session_dirty);
        assert!(session.last_session_change.is_none());

        session.mark_session_dirty();

        assert!(session.session_dirty);
        assert!(session.last_session_change.is_some());
    }
}
