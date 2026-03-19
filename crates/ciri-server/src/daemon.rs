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
}

// ─── Server state ───────────────────────────────────────────────────

struct ServerState {
    workspaces: WorkspaceSet,
    panes: HashMap<u64, Pane>,
    next_pane_id: u64,
    session_name: String,
    generation: HashMap<u64, u64>, // per pane_id
    clients: HashMap<u64, ClientState>,
    next_client_id: u64,
    default_shell: String,
    default_column_width: ColumnWidth,
    /// Total inset per pane: (padding + border_width) * 2, subtracted from pane pixel size
    /// before computing grid cols/rows.
    pane_inset: f32,
    session_dirty: bool,
    last_session_change: Option<Instant>,
}

impl ServerState {
    fn new(session_name: &str, shell: &str, column_gap: f32) -> Self {
        ServerState {
            workspaces: WorkspaceSet::new_with_gaps(ViewSize {
                width: 1024.0,
                height: 768.0,
            }, column_gap, column_gap),
            panes: HashMap::new(),
            next_pane_id: 1,
            session_name: session_name.to_string(),
            generation: HashMap::new(),
            clients: HashMap::new(),
            next_client_id: 1,
            default_shell: shell.to_string(),
            default_column_width: ColumnWidth::Proportion(0.5),
            pane_inset: 12.0, // (4.0 padding + 2.0 border) * 2 = 12.0 default
            session_dirty: false,
            last_session_change: None,
        }
    }

    /// Create a new pane in the active workspace's active position (column right).
    fn create_pane(&mut self) -> Result<u64> {
        let id = self.next_pane_id;
        self.next_pane_id += 1;
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
        let (cw, ch) = self.effective_cell_dims();
        let (cols, rows) = self.pane_grid_size(pane_w, vh);
        log::info!("create_pane {id}: viewport={vw}x{vh} col_px={pane_w:.1} cell={cw}x{ch} inset={} → {cols}x{rows}", self.pane_inset);
        let pane = Pane::new(id, cols, rows, &self.default_shell)?;
        self.panes.insert(id, pane);
        self.generation.insert(id, 0);
        self.workspaces.active_mut().add_column_right(id, self.default_column_width);
        // Mark all clients for full sync of this new pane
        for client in self.clients.values_mut() {
            client.damage.entry(id).or_insert_with(DamageAccumulator::new).mark_full();
        }
        Ok(id)
    }

    /// Create a new pane in a new workspace below the active one (SplitDown).
    fn create_pane_in_new_workspace(&mut self) -> Result<u64> {
        let id = self.next_pane_id;
        self.next_pane_id += 1;
        let vw = self.workspaces.view_size.width;
        let vh = self.workspaces.view_size.height;
        let (cols, rows) = self.pane_grid_size(vw, vh);
        let pane = Pane::new(id, cols, rows, &self.default_shell)?;
        self.panes.insert(id, pane);
        self.generation.insert(id, 0);
        self.workspaces.add_workspace_below(id);
        for client in self.clients.values_mut() {
            client.damage.entry(id).or_insert_with(DamageAccumulator::new).mark_full();
        }
        Ok(id)
    }

    /// Compute effective cell dimensions (smallest client wins, like tmux).
    fn effective_cell_dims(&self) -> (f32, f32) {
        let mut cw = f32::MAX;
        let mut ch = f32::MAX;
        for client in self.clients.values() {
            cw = cw.min(client.cell_width);
            ch = ch.min(client.cell_height);
        }
        if cw == f32::MAX { cw = 8.0; }
        if ch == f32::MAX { ch = 16.0; }
        (cw, ch)
    }

    /// Compute effective viewport (smallest client wins).
    fn effective_viewport(&self) -> (f32, f32) {
        let mut w = f32::MAX;
        let mut h = f32::MAX;
        for client in self.clients.values() {
            w = w.min(client.viewport_width);
            h = h.min(client.viewport_height);
        }
        if w == f32::MAX { w = 1024.0; }
        if h == f32::MAX { h = 768.0; }
        (w, h)
    }

    /// Compute cols/rows for a pane given its pixel area and current cell dimensions.
    /// Subtracts padding + border inset from both dimensions.
    fn pane_grid_size(&self, pane_width: f32, pane_height: f32) -> (u16, u16) {
        let (cw, ch) = self.effective_cell_dims();
        let usable_w = (pane_width - self.pane_inset).max(cw);
        let usable_h = (pane_height - self.pane_inset).max(ch);
        let cols = (usable_w / cw).floor().max(1.0) as u16;
        let rows = (usable_h / ch).floor().max(1.0) as u16;
        (cols, rows)
    }

    /// Resize ALL panes from the full layout tree (not just visible ones).
    /// Off-screen panes get their size from their column width and workspace height,
    /// so PTY dimensions stay correct even when scrolled out of view.
    fn resize_all_panes(&mut self) {
        let (vp_w, vp_h) = self.effective_viewport();
        self.workspaces.resize_view(ViewSize { width: vp_w, height: vp_h });
        let vw = self.workspaces.view_size.width;
        let vh = self.workspaces.view_size.height;

        let (cw, ch) = self.effective_cell_dims();
        log::debug!("resize_all_panes: viewport={vw}x{vh} cell={cw}x{ch} inset={}", self.pane_inset);
        for ws in &self.workspaces.workspaces {
            for col in &ws.columns {
                let col_w = col.effective_width(vw);
                let tile_rects = col.tile_rects(col_w, vh);
                for (pane_id, _y, tile_h) in &tile_rects {
                    let (cols, rows) = self.pane_grid_size(col_w, *tile_h);
                    log::debug!("  pane {}: col_w={col_w:.1}px tile_h={tile_h:.1}px → {cols}x{rows} chars", pane_id);
                    if let Some(pane) = self.panes.get_mut(pane_id) {
                        pane.resize(cols, rows);
                        let g = self.generation.entry(*pane_id).or_insert(0);
                        *g += 1;
                        for client in self.clients.values_mut() {
                            client.damage.entry(*pane_id)
                                .or_insert_with(DamageAccumulator::new).mark_full();
                        }
                    }
                }
            }
        }
    }

    fn close_pane(&mut self, pane_id: u64) {
        // Remove from whichever workspace contains it
        for ws in &mut self.workspaces.workspaces {
            ws.close_pane(pane_id);
        }
        self.workspaces.cleanup_empty();
        self.panes.remove(&pane_id);
        self.generation.remove(&pane_id);
        for client in self.clients.values_mut() {
            client.damage.remove(&pane_id);
            client.history_sent.remove(&pane_id);
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
    fn process_pty_and_damage(&mut self) -> Vec<ServerMessage> {
        let mut clipboard_msgs = Vec::new();
        let pane_ids: Vec<u64> = self.panes.keys().copied().collect();
        for pane_id in pane_ids {
            let pane = self.panes.get_mut(&pane_id).unwrap();
            pane.process_pty_output();

            // Drain OSC 52 clipboard writes
            for data in pane.drain_clipboard() {
                clipboard_msgs.push(ServerMessage::ClipboardStore { data });
            }

            if let Some(regions) = pane.extract_damage() {
                // Bump generation
                let g = self.generation.entry(pane_id).or_insert(0);
                *g += 1;

                // If all terminal rows were damaged, this is likely a scroll or full repaint.
                // Mark as full so a FullPaneSync is sent (client needs it for scrollback).
                let is_full = regions.len() >= pane.grid_rows() as usize;

                for client in self.clients.values_mut() {
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
        clipboard_msgs
    }

    /// Check for exited panes and clean them up. Returns list of closed pane IDs.
    fn cleanup_exited_panes(&mut self) -> Vec<u64> {
        let dead: Vec<u64> = self.panes.iter()
            .filter(|(_, p)| p.is_exited())
            .map(|(id, _)| *id)
            .collect();
        for &id in &dead {
            self.close_pane(id);
        }
        dead
    }

    fn handle_message(&mut self, msg: ClientMessage, client_id: u64) -> Vec<ServerResponse> {
        let mut responses = Vec::new();

        match msg {
            ClientMessage::Input { pane_id, data } => {
                if let Some(pane) = self.panes.get_mut(&pane_id) {
                    pane.write_to_pty(&data);
                }
            }
            ClientMessage::CreatePane => {
                match self.create_pane() {
                    Ok(id) => {
                        self.resize_all_panes();
                        self.mark_session_dirty();
                        responses.push(ServerResponse::BroadcastControl(ServerMessage::PaneCreated {
                            pane_id: id,
                            column_idx: self.workspaces.active().active_column_idx,
                        }));
                        responses.push(ServerResponse::BroadcastControl(ServerMessage::LayoutUpdate {
                            layout: self.layout_state(),
                        }));
                    }
                    Err(e) => log::error!("failed to create pane: {e}"),
                }
            }
            ClientMessage::SplitDown => {
                match self.create_pane_in_new_workspace() {
                    Ok(id) => {
                        self.resize_all_panes();
                        self.mark_session_dirty();
                        responses.push(ServerResponse::BroadcastControl(ServerMessage::PaneCreated {
                            pane_id: id,
                            column_idx: self.workspaces.active().active_column_idx,
                        }));
                        responses.push(ServerResponse::BroadcastControl(ServerMessage::LayoutUpdate {
                            layout: self.layout_state(),
                        }));
                    }
                    Err(e) => log::error!("failed to split: {e}"),
                }
            }
            ClientMessage::ClosePane { pane_id } => {
                self.close_pane(pane_id);
                self.resize_all_panes();
                self.mark_session_dirty();
                responses.push(ServerResponse::BroadcastControl(ServerMessage::PaneClosed { pane_id }));
                responses.push(ServerResponse::BroadcastControl(ServerMessage::LayoutUpdate {
                    layout: self.layout_state(),
                }));
            }
            ClientMessage::FocusLeft => {
                self.workspaces.active_mut().focus_left();
                self.mark_session_dirty();
                responses.push(ServerResponse::BroadcastControl(ServerMessage::LayoutUpdate {
                    layout: self.layout_state(),
                }));
            }
            ClientMessage::FocusRight => {
                self.workspaces.active_mut().focus_right();
                self.mark_session_dirty();
                responses.push(ServerResponse::BroadcastControl(ServerMessage::LayoutUpdate {
                    layout: self.layout_state(),
                }));
            }
            ClientMessage::FocusUp => {
                // First try to move up within the column's tiles
                if !self.workspaces.active_mut().focus_tile_up() {
                    // If at top tile, switch to previous workspace
                    self.workspaces.focus_up();
                }
                self.mark_session_dirty();
                responses.push(ServerResponse::BroadcastControl(ServerMessage::LayoutUpdate {
                    layout: self.layout_state(),
                }));
            }
            ClientMessage::FocusDown => {
                // First try to move down within the column's tiles
                if !self.workspaces.active_mut().focus_tile_down() {
                    // If at bottom tile, switch to next workspace
                    self.workspaces.focus_down();
                }
                self.mark_session_dirty();
                responses.push(ServerResponse::BroadcastControl(ServerMessage::LayoutUpdate {
                    layout: self.layout_state(),
                }));
            }
            ClientMessage::MovePaneLeft => {
                self.workspaces.active_mut().move_pane_left();
                self.mark_session_dirty();
                responses.push(ServerResponse::BroadcastControl(ServerMessage::LayoutUpdate {
                    layout: self.layout_state(),
                }));
            }
            ClientMessage::MovePaneRight => {
                self.workspaces.active_mut().move_pane_right();
                self.mark_session_dirty();
                responses.push(ServerResponse::BroadcastControl(ServerMessage::LayoutUpdate {
                    layout: self.layout_state(),
                }));
            }
            ClientMessage::Resize { cols: _, rows: _, width, height, cell_width, cell_height } => {
                log::debug!("client {client_id} Resize: {width}x{height}px, cell={cell_width:.1}x{cell_height:.1}");
                // Validate
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
                    self.resize_all_panes();
                    responses.push(ServerResponse::BroadcastControl(ServerMessage::LayoutUpdate {
                        layout: self.layout_state(),
                    }));
                } else {
                    log::warn!("ignoring invalid resize from client {client_id}: {width}x{height} cell={cell_width}x{cell_height}");
                }
            }
            ClientMessage::SetColumnWidth { proportion } => {
                self.workspaces.active_mut().set_active_column_width(ColumnWidth::Proportion(proportion));
                self.mark_session_dirty();
                self.resize_all_panes();
                responses.push(ServerResponse::BroadcastControl(ServerMessage::LayoutUpdate {
                    layout: self.layout_state(),
                }));
            }
            ClientMessage::AdjustColumnSplit { delta } => {
                self.workspaces.active_mut().resize_active_with_neighbor(delta);
                self.mark_session_dirty();
                self.resize_all_panes();
                responses.push(ServerResponse::BroadcastControl(ServerMessage::LayoutUpdate {
                    layout: self.layout_state(),
                }));
            }
            ClientMessage::EqualizeColumnSplit => {
                self.workspaces.active_mut().equalize_active_with_neighbor();
                self.mark_session_dirty();
                self.resize_all_panes();
                responses.push(ServerResponse::BroadcastControl(ServerMessage::LayoutUpdate {
                    layout: self.layout_state(),
                }));
            }
            ClientMessage::ConsumeIntoColumn => {
                self.workspaces.active_mut().consume_from_right();
                self.resize_all_panes();
                self.mark_session_dirty();
                responses.push(ServerResponse::BroadcastControl(ServerMessage::LayoutUpdate {
                    layout: self.layout_state(),
                }));
            }
            ClientMessage::ExpelFromColumn => {
                self.workspaces.active_mut().expel_active_tile();
                self.resize_all_panes();
                self.mark_session_dirty();
                responses.push(ServerResponse::BroadcastControl(ServerMessage::LayoutUpdate {
                    layout: self.layout_state(),
                }));
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
                self.workspaces.switch_to(workspace_idx);
                self.mark_session_dirty();
                responses.push(ServerResponse::BroadcastControl(ServerMessage::LayoutUpdate {
                    layout: self.layout_state(),
                }));
            }
            ClientMessage::MouseInput { pane_id, button, col, row, pressed, modifiers } => {
                if let Some(pane) = self.panes.get(&pane_id) {
                    if pane.has_mouse_mode() {
                        pane.send_mouse_input(button, col, row, pressed, modifiers);
                    }
                }
            }
            ClientMessage::ListSessions
            | ClientMessage::KillSession { .. }
            | ClientMessage::KillServer
            | ClientMessage::SwitchSession { .. } => {
                log::warn!("session management not yet implemented");
            }
        }

        responses
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

/// Internal response type for message handling.
enum ServerResponse {
    BroadcastControl(ServerMessage),
    RemoveClient(u64),
}

// ─── Graceful shutdown helper ───────────────────────────────────────

/// Perform graceful shutdown: save session, notify clients, remove socket.
async fn graceful_shutdown(state: &Arc<Mutex<ServerState>>, _session_name: &str) {
    let s = state.lock().await;
    let _ = s.save_session();
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
async fn cleanup_client(state: &Arc<Mutex<ServerState>>, client_id: u64) {
    let mut s = state.lock().await;
    if s.clients.remove(&client_id).is_some() {
        log::info!("client {client_id} disconnected");
    }
}

// ─── Main daemon loop ───────────────────────────────────────────────

pub async fn run_daemon(session_name: &str) -> Result<()> {
    // Validate session name early to prevent path traversal in socket/state paths
    ciri_session::save::validate_session_name(session_name)?;

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

    let state = Arc::new(Mutex::new(ServerState::new(session_name, &shell, config.appearance.column_gap)));

    // Try restoring saved session, otherwise create initial pane
    {
        let mut s = state.lock().await;
        let restored = ciri_session::restore::restore_session(session_name, &transport::state_dir())
            .ok()
            .flatten();
        if let Some(saved) = restored {
            log::info!("restoring session '{}' ({} workspaces)", session_name, saved.workspaces.len());
            // Rebuild workspace layout and create panes
            s.workspaces.workspaces.clear();
            for saved_ws in &saved.workspaces {
                let mut ws = ciri_layout::workspace::Workspace::new_with_gap(
                    s.workspaces.view_size,
                    s.workspaces.column_gap,
                );
                for saved_col in &saved_ws.columns {
                    if let Some(_tile) = saved_col.tiles.first() {
                        let id = s.next_pane_id;
                        s.next_pane_id += 1;
                        let vw = s.workspaces.view_size.width;
                        let vh = s.workspaces.view_size.height;
                        let col_w = (vw as f64 * saved_col.width_proportion) as f32;
                        let (cols, rows) = s.pane_grid_size(col_w, vh);
                        match Pane::new(id, cols, rows, &s.default_shell) {
                            Ok(pane) => {
                                s.panes.insert(id, pane);
                                s.generation.insert(id, 0);
                                let mut col = ciri_layout::column::Column::new(id);
                                col.width = ColumnWidth::Proportion(saved_col.width_proportion);
                                ws.columns.push(col);
                            }
                            Err(e) => log::error!("failed to restore pane: {e}"),
                        }
                    }
                }
                ws.active_column_idx = saved_ws.active_column_idx
                    .min(ws.columns.len().saturating_sub(1));
                s.workspaces.workspaces.push(ws);
            }
            s.workspaces.active_workspace_idx = saved.active_workspace_idx
                .min(s.workspaces.workspaces.len().saturating_sub(1));
            // If restore produced no panes (all failed), create a default one
            if s.panes.is_empty() {
                let vs = s.workspaces.view_size;
                let cg = s.workspaces.column_gap;
                s.workspaces.workspaces.clear();
                s.workspaces.workspaces.push(ciri_layout::workspace::Workspace::new_with_gap(vs, cg));
                s.create_pane()?;
            }
        } else {
            s.create_pane()?;
        }
    }

    // Shutdown signal shared between tick loop, signal handler, and accept loop
    let shutdown = Arc::new(Notify::new());

    // Spawn tick loop (16ms = ~60fps)
    let tick_state = state.clone();
    let tick_shutdown = shutdown.clone();
    let session_name_owned = session_name.to_string();
    tokio::spawn(async move {
        let session_name = &session_name_owned;
        let mut ticker = interval(Duration::from_millis(16));
        loop {
            ticker.tick().await;

            // M6: Acquire lock, process state, collect outgoing data, then drop lock before sending
            let mut outgoing: Vec<(u64, Vec<u8>)> = Vec::new();
            let mut should_shutdown = false;

            {
                let mut s = tick_state.lock().await;

                // Process PTY output and extract damage
                let clipboard_msgs = s.process_pty_and_damage();

                // Broadcast OSC 52 clipboard writes to all clients
                for clip_msg in &clipboard_msgs {
                    if let Ok(payload) = rmp_serde::to_vec(clip_msg) {
                        let mut frame = Vec::with_capacity(5 + payload.len());
                        frame.push(0x10); // TAG_SERVER_MSG
                        frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                        frame.extend_from_slice(&payload);
                        for client in s.clients.values() {
                            let _ = client.tx.try_send(frame.clone());
                        }
                    }
                }

                // Clean up exited panes
                let dead = s.cleanup_exited_panes();
                if !dead.is_empty() {
                    s.mark_session_dirty();
                    // Broadcast close messages
                    let mut broadcasts = Vec::new();
                    for &id in &dead {
                        let close_payload = rmp_serde::to_vec(&ServerMessage::PaneClosed { pane_id: id })
                            .unwrap_or_default();
                        broadcasts.push(close_payload);
                    }
                    let layout_payload = rmp_serde::to_vec(&ServerMessage::LayoutUpdate {
                        layout: s.layout_state(),
                    }).unwrap_or_default();
                    broadcasts.push(layout_payload);

                    for client in s.clients.values() {
                        for payload in &broadcasts {
                            let mut frame = Vec::with_capacity(5 + payload.len());
                            frame.push(0x10); // TAG_SERVER_MSG
                            frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                            frame.extend_from_slice(payload);
                            if let Err(e) = client.tx.try_send(frame) {
                                log::warn!("failed to send close frame to client {}: {e}", client.id);
                            }
                        }
                    }
                }

                s.autosave_if_due(Instant::now());

                // Exit if no panes left
                if s.panes.is_empty() {
                    // Layout is empty — delete stale session file instead of saving.
                    let _ = ciri_session::restore::delete_session(
                        session_name, &transport::state_dir()
                    );
                    log::info!("all panes exited, shutting down");
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
                    should_shutdown = true;
                    // Drop lock before removing socket
                } else {
                    // Collect damage frames to send outside the lock
                    let mut pending: Vec<(u64, u64, DamageAccumulator)> = Vec::new();
                    for (&client_id, client) in s.clients.iter_mut() {
                        let pane_ids: Vec<u64> = client.damage.keys().copied().collect();
                        for pane_id in pane_ids {
                            let acc = client.damage.get_mut(&pane_id).unwrap();
                            if !acc.is_empty() {
                                pending.push((client_id, pane_id, acc.take()));
                            }
                        }
                    }

                    // Build frames from pane data
                    for (client_id, pane_id, damage) in pending {
                        let pgen = s.generation.get(&pane_id).copied().unwrap_or(0);

                        if damage.full {
                            let sync_result = if let Some(pane) = s.panes.get(&pane_id) {
                                let last_sent = s.clients.get(&client_id)
                                    .and_then(|c| c.history_sent.get(&pane_id).copied())
                                    .unwrap_or(0);
                                let sync = pane.snapshot_incremental(pgen, last_sent);
                                let current_history = pane.history_size();
                                Some((sync, current_history))
                            } else {
                                None
                            };
                            if let Some((sync, current_history)) = sync_result {
                                if let Some(client) = s.clients.get_mut(&client_id) {
                                    client.history_sent.insert(pane_id, current_history);
                                }

                                if let Ok(payload) = codec::encode_full_pane_sync_payload(&sync) {
                                    let mut frame = Vec::with_capacity(5 + payload.len());
                                    frame.push(0x21);
                                    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                                    frame.extend_from_slice(&payload);
                                    outgoing.push((client_id, frame));
                                }
                            }
                        } else if let Some(pane) = s.panes.get(&pane_id) {
                            let (cursor_line, cursor_col, cursor_shape, mode_flags) = pane.cursor_info();
                            let mut regions = Vec::new();

                            for (&line, &(left, right)) in &damage.line_damage {
                                let cells = pane.read_cells(line, left, right);
                                regions.push(DamageRegion { line, left, right, cells });
                            }

                            // Always send delta (even with empty regions) so cursor updates reach client
                            let delta = CellDelta {
                                pane_id,
                                generation: pgen,
                                cursor_line,
                                cursor_col,
                                cursor_shape,
                                mode_flags,
                                regions,
                            };
                            if let Ok(payload) = codec::encode_cell_delta_payload(&delta) {
                                let mut frame = Vec::with_capacity(5 + payload.len());
                                frame.push(0x20); // TAG_CELL_DELTA
                                frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                                frame.extend_from_slice(&payload);
                                outgoing.push((client_id, frame));
                            }
                        }
                    }
                }
            } // lock dropped here

            // C2: Signal shutdown instead of process::exit
            if should_shutdown {
                let _ = std::fs::remove_file(&transport::server_socket_path());
                tick_shutdown.notify_one();
                return;
            }

            // M6: Send frames outside the lock
            if !outgoing.is_empty() {
                let mut s = tick_state.lock().await;
                let mut to_disconnect = Vec::new();
                for (client_id, frame) in outgoing {
                    if let Some(client) = s.clients.get_mut(&client_id) {
                        if let Err(e) = client.tx.try_send(frame) {
                            client.send_failures += 1;
                            if client.send_failures >= 100 {
                                log::warn!("disconnecting slow client {client_id}: {} consecutive failures", client.send_failures);
                                to_disconnect.push(client_id);
                            } else {
                                log::debug!("send to client {client_id} failed (#{}) : {e}", client.send_failures);
                            }
                        } else {
                            client.send_failures = 0;
                        }
                    }
                }
                for cid in to_disconnect {
                    s.clients.remove(&cid);
                }
            }
        }
    });

    // H4: Signal handling - SIGTERM and SIGINT
    let signal_state = state.clone();
    let signal_shutdown = shutdown.clone();
    let signal_session = session_name.to_string();
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
            graceful_shutdown(&signal_state, &signal_session).await;
            signal_shutdown.notify_one();
        }
        #[cfg(windows)]
        {
            let _ = tokio::signal::ctrl_c().await;
            log::info!("received Ctrl-C");
            graceful_shutdown(&signal_state, &signal_session).await;
            signal_shutdown.notify_one();
        }
    });

    // Accept connections, with graceful shutdown via select!
    loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, _) = result?;
                let state = state.clone();

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
                            log::error!("client hello rejected: {e}");
                            return;
                        }
                    };

                    log::info!("client requested session: {}", hello.session_name);

                    // Store viewport locally; will be applied per-client after registration
                    let client_viewport_w = hello.width as f32;
                    let client_viewport_h = hello.height as f32;
                    let client_cell_w = hello.cell_width;
                    let client_cell_h = hello.cell_height;

                    // Send ServerHello (no flush — frames follow immediately)
                    if let Err(e) = codec::write_server_hello(&mut writer).await {
                        log::error!("failed to send server hello: {e}");
                        return;
                    }

                    // Register client
                    // C3: Collect frames while holding lock, then send after dropping lock
                    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(256);
                    let client_id;
                    let initial_frames: Vec<Vec<u8>>;
                    {
                        let mut s = state.lock().await;
                        client_id = s.next_client_id;
                        s.next_client_id += 1;

                        // Mark all panes for full sync
                        let mut damage_map = HashMap::new();
                        for &pane_id in s.panes.keys() {
                            let mut acc = DamageAccumulator::new();
                            acc.mark_full();
                            damage_map.insert(pane_id, acc);
                        }

                        s.clients.insert(client_id, ClientState {
                            id: client_id,
                            tx: tx.clone(),
                            damage: damage_map,
                            last_acked_generation: 0,
                            history_sent: HashMap::new(),
                            send_failures: 0,
                            cell_width: client_cell_w,
                            cell_height: client_cell_h,
                            viewport_width: client_viewport_w,
                            viewport_height: client_viewport_h,
                        });

                        // Recompute effective viewport now that this client is registered
                        s.resize_all_panes();

                        log::info!("client {client_id} connected");

                        // Build frames to send
                        let mut frames = Vec::new();
                        let (sync_msg, pane_syncs) = s.build_state_sync();
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

                        // Collect current history sizes before mutably borrowing clients
                        let pane_histories: Vec<(u64, usize)> = s.panes.iter().map(|(&pid, pane)| {
                            (pid, pane.history_size())
                        }).collect();

                        // Clear the initial full-sync markers and record history_sent
                        // so subsequent full syncs don't re-send the same scrollback.
                        if let Some(client) = s.clients.get_mut(&client_id) {
                            for acc in client.damage.values_mut() {
                                *acc = DamageAccumulator::new();
                            }
                            for (pane_id, history) in pane_histories {
                                client.history_sent.insert(pane_id, history);
                            }
                        }

                        initial_frames = frames;
                    } // lock dropped here

                    // C3: Now send frames without holding the lock
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

                    // M11: Reader loop with guaranteed cleanup on panic or error
                    let reader_result = std::panic::AssertUnwindSafe(async {
                        loop {
                            match codec::read_frame(&mut reader).await {
                                Ok(codec::Frame::ClientMsg(msg)) => {
                                    let mut s = state.lock().await;
                                    let responses = s.handle_message(msg, client_id);

                                    for resp in responses {
                                        match resp {
                                            ServerResponse::BroadcastControl(server_msg) => {
                                                if let Ok(payload) = rmp_serde::to_vec(&server_msg) {
                                                    let mut frame = Vec::with_capacity(5 + payload.len());
                                                    frame.push(0x10);
                                                    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                                                    frame.extend_from_slice(&payload);
                                                    // Broadcast to all clients
                                                    for client in s.clients.values() {
                                                        // M4: Log try_send failures
                                                        if let Err(e) = client.tx.try_send(frame.clone()) {
                                                            log::warn!(
                                                                "failed to send broadcast to client {}: {e}",
                                                                client.id
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                            ServerResponse::RemoveClient(cid) => {
                                                s.clients.remove(&cid);
                                                log::info!("client {cid} detached");
                                                return;
                                            }
                                        }
                                    }
                                }
                                Ok(_) => {
                                    // Unexpected frame type from client
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

                    // M11: Cleanup always runs, whether reader exited normally, on error, or detach
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
        let mut state = ServerState::new("default", "/bin/sh", 8.0);
        state.session_dirty = true;
        let changed_at = Instant::now();
        state.last_session_change = Some(changed_at);

        assert!(!state.autosave_due_at(changed_at));
        assert!(!state.autosave_due_at(changed_at + Duration::from_millis(249)));
        assert!(state.autosave_due_at(changed_at + Duration::from_millis(250)));
    }

    #[test]
    fn mark_session_dirty_sets_dirty_and_timestamp() {
        let mut state = ServerState::new("default", "/bin/sh", 8.0);
        assert!(!state.session_dirty);
        assert!(state.last_session_change.is_none());

        state.mark_session_dirty();

        assert!(state.session_dirty);
        assert!(state.last_session_change.is_some());
    }
}
