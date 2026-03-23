use ciri_layout::column::ColumnWidth;
use ciri_protocol::message::*;
use ciri_protocol::transport;
use ciri_term::pane::Pane;
use std::collections::HashMap;

use super::client::ClientState;
use super::damage::DamageAccumulator;
use super::session::Session;

pub(crate) struct Server {
    pub(crate) sessions: HashMap<String, Session>,
    pub(crate) clients: HashMap<u64, ClientState>,
    pub(crate) next_client_id: u64,
    pub(crate) next_pane_id: u64,
    pub(crate) default_shell: String,
    pub(crate) default_column_width: ColumnWidth,
    pub(crate) column_gap: f32,
    pub(crate) pane_inset: f32,
    /// True once server has had at least one session. Prevents premature
    /// shutdown on startup before any client has connected.
    pub(crate) had_session: bool,
}

/// Internal response type for message handling.
pub(crate) enum ServerResponse {
    BroadcastToSession(String, ServerMessage),
    SendToClient(u64, ServerMessage),
    SendFullPaneSync(u64, FullPaneSync),
    RemoveClient(u64),
    ShutdownServer,
}

impl Server {
    pub(crate) fn new(shell: &str, column_gap: f32) -> Self {
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
    pub(crate) fn get_or_create_session(&mut self, session_name: &str) -> &mut Session {
        if !self.sessions.contains_key(session_name) {
            self.had_session = true;
            let mut session = Session::new(session_name, &self.default_shell, self.column_gap);
            session.default_column_width = self.default_column_width;
            session.pane_inset = self.pane_inset;

            // Try restoring saved session
            let restored =
                ciri_session::restore::restore_session(session_name, &transport::state_dir())
                    .ok()
                    .flatten();
            if let Some(saved) = restored {
                log::info!(
                    "restoring session '{}' ({} workspaces)",
                    session_name,
                    saved.workspaces.len()
                );
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
                        let restored_width = if let Some(px) = saved_col.width_fixed_px {
                            ColumnWidth::Fixed(px)
                        } else {
                            ColumnWidth::Proportion(saved_col.width_proportion)
                        };
                        let col_w = match restored_width {
                            ColumnWidth::Fixed(px) => px as f32,
                            ColumnWidth::Proportion(p) => (vw as f64 * p) as f32,
                        };

                        // Restore every tile in this column (not just the first)
                        let mut col_opt: Option<ciri_layout::column::Column> = None;
                        for saved_tile in &saved_col.tiles {
                            let id = self.next_pane_id;
                            self.next_pane_id += 1;
                            let tile_h = vh / saved_col.tiles.len() as f32;
                            let (cols, rows) =
                                session.pane_grid_size_with_cells(col_w, tile_h, 8.0, 16.0);
                            match Pane::new(id, cols, rows, &session.default_shell) {
                                Ok(pane) => {
                                    session.panes.insert(id, pane);
                                    session.generation.insert(id, 0);
                                    if let Some(col) = &mut col_opt {
                                        // Add as stacked tile with saved weight
                                        let mut tile = ciri_layout::tile::Tile::new(id);
                                        tile.height = ciri_layout::tile::TileHeight::Auto {
                                            weight: saved_tile.weight as f64,
                                        };
                                        col.tiles.push(tile);
                                    } else {
                                        // First tile: create the column
                                        let mut col = ciri_layout::column::Column::new(id);
                                        col.width = restored_width;
                                        // Set weight on the first tile too
                                        if let Some(first_tile) = col.tiles.first_mut() {
                                            first_tile.height =
                                                ciri_layout::tile::TileHeight::Auto {
                                                    weight: saved_tile.weight as f64,
                                                };
                                        }
                                        col_opt = Some(col);
                                    }
                                }
                                Err(e) => log::error!("failed to restore pane: {e}"),
                            }
                        }
                        if let Some(mut col) = col_opt {
                            col.active_tile_idx = saved_col
                                .active_tile_idx
                                .min(col.tiles.len().saturating_sub(1));
                            ws.columns.push(col);
                        }
                    }
                    ws.active_column_idx = saved_ws
                        .active_column_idx
                        .min(ws.columns.len().saturating_sub(1));
                    session.workspaces.workspaces.push(ws);
                }
                session.workspaces.active_workspace_idx = saved
                    .active_workspace_idx
                    .min(session.workspaces.workspaces.len().saturating_sub(1));
            }

            // If restore produced no panes (all failed or no saved session), create a default one
            if session.panes.is_empty() {
                let vs = session.workspaces.view_size;
                let cg = session.workspaces.column_gap;
                session.workspaces.workspaces.clear();
                session
                    .workspaces
                    .workspaces
                    .push(ciri_layout::workspace::Workspace::new_with_gap(vs, cg));
                if let Err(e) = session.create_pane(&mut self.next_pane_id, &mut self.clients) {
                    log::error!(
                        "failed to create initial pane for session '{}': {e}",
                        session_name
                    );
                }
            }

            self.sessions.insert(session_name.to_string(), session);
        }
        self.sessions.get_mut(session_name).unwrap()
    }

    /// Send a framed control message to a specific client.
    pub(crate) fn send_to_client(&self, client_id: u64, msg: &ServerMessage) {
        if let (Some(client), Some(frame)) = (
            self.clients.get(&client_id),
            ciri_protocol::codec::frame_server_msg(msg),
        ) {
            if let Err(e) = client.tx.try_send(bytes::Bytes::from(frame)) {
                log::warn!("failed to send to client {client_id}: {e}");
            }
        }
    }

    /// Broadcast a control message to all clients of a session.
    pub(crate) fn broadcast_to_session(&self, session_name: &str, msg: &ServerMessage) {
        if let Some(frame) = ciri_protocol::codec::frame_server_msg(msg) {
            let frame = bytes::Bytes::from(frame);
            for client in self.clients.values() {
                if client.session_name == session_name && client.tx.try_send(frame.clone()).is_err()
                {
                    log::warn!("failed to broadcast to client {}", client.id);
                }
            }
        }
    }

    /// Temporarily remove a session from the map, call `f` with it and `&mut self.clients`,
    /// then re-insert it. This works around the borrow checker while guaranteeing reinsertion.
    fn with_session<F, R>(&mut self, name: &str, f: F) -> Option<R>
    where
        F: FnOnce(&mut Session, &mut HashMap<u64, ClientState>) -> R,
    {
        let mut session = self.sessions.remove(name)?;
        let result = f(&mut session, &mut self.clients);
        self.sessions.insert(name.to_string(), session);
        Some(result)
    }

    /// Common tail for handlers that mutate session layout: mark dirty, optionally
    /// resize panes, and broadcast a `LayoutUpdate`.
    fn layout_changed(
        session: &mut Session,
        clients: &mut HashMap<u64, ClientState>,
        session_name: &str,
        resize: bool,
        responses: &mut Vec<ServerResponse>,
    ) {
        session.mark_session_dirty();
        if resize {
            let _ = session.resize_all_panes(clients);
        }
        responses.push(ServerResponse::BroadcastToSession(
            session_name.to_string(),
            ServerMessage::LayoutUpdate {
                layout: session.layout_state(),
            },
        ));
    }

    pub(crate) fn handle_message(
        &mut self,
        msg: ClientMessage,
        client_id: u64,
    ) -> Vec<ServerResponse> {
        let mut responses = Vec::new();

        // Get the client's session name
        let session_name = match self.clients.get(&client_id) {
            Some(client) => client.session_name.clone(),
            None => return responses,
        };

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
                            responses.push(ServerResponse::BroadcastToSession(
                                session_name.clone(),
                                ServerMessage::PaneCreated {
                                    pane_id: id,
                                    column_idx: session.workspaces.active().active_column_idx,
                                    cols,
                                    rows,
                                },
                            ));
                            Self::layout_changed(
                                &mut session,
                                &mut self.clients,
                                &session_name,
                                false,
                                &mut responses,
                            );
                        }
                        Err(e) => log::error!("failed to create pane: {e}"),
                    }
                    self.sessions.insert(session_name, session);
                }
            }
            ClientMessage::SplitDown => {
                if let Some(mut session) = self.sessions.remove(&session_name) {
                    match session
                        .create_pane_in_new_workspace(&mut self.next_pane_id, &mut self.clients)
                    {
                        Ok(id) => {
                            session.resize_all_panes(&mut self.clients);
                            session.mark_session_dirty();
                            let (cols, rows) = session.pane_grid_dims(id);
                            responses.push(ServerResponse::BroadcastToSession(
                                session_name.clone(),
                                ServerMessage::PaneCreated {
                                    pane_id: id,
                                    column_idx: session.workspaces.active().active_column_idx,
                                    cols,
                                    rows,
                                },
                            ));
                            Self::layout_changed(
                                &mut session,
                                &mut self.clients,
                                &session_name,
                                false,
                                &mut responses,
                            );
                        }
                        Err(e) => log::error!("failed to split: {e}"),
                    }
                    self.sessions.insert(session_name, session);
                }
            }
            ClientMessage::ClosePane { pane_id } => {
                self.with_session(&session_name, |session, clients| {
                    session.close_pane(pane_id, clients);
                    responses.push(ServerResponse::BroadcastToSession(
                        session_name.clone(),
                        ServerMessage::PaneClosed { pane_id },
                    ));
                    Self::layout_changed(session, clients, &session_name, true, &mut responses);
                });
            }
            ClientMessage::FocusLeft => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    session.workspaces.active_mut().focus_left();
                    Self::layout_changed(
                        session,
                        &mut self.clients,
                        &session_name,
                        false,
                        &mut responses,
                    );
                }
            }
            ClientMessage::FocusRight => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    session.workspaces.active_mut().focus_right();
                    Self::layout_changed(
                        session,
                        &mut self.clients,
                        &session_name,
                        false,
                        &mut responses,
                    );
                }
            }
            ClientMessage::FocusUp => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    if !session.workspaces.active_mut().focus_tile_up() {
                        session.workspaces.focus_up();
                    }
                    Self::layout_changed(
                        session,
                        &mut self.clients,
                        &session_name,
                        false,
                        &mut responses,
                    );
                }
            }
            ClientMessage::FocusDown => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    if !session.workspaces.active_mut().focus_tile_down() {
                        session.workspaces.focus_down();
                    }
                    Self::layout_changed(
                        session,
                        &mut self.clients,
                        &session_name,
                        false,
                        &mut responses,
                    );
                }
            }
            ClientMessage::MovePaneLeft => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    session.workspaces.active_mut().move_pane_left();
                    Self::layout_changed(
                        session,
                        &mut self.clients,
                        &session_name,
                        false,
                        &mut responses,
                    );
                }
            }
            ClientMessage::MovePaneRight => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    session.workspaces.active_mut().move_pane_right();
                    Self::layout_changed(
                        session,
                        &mut self.clients,
                        &session_name,
                        false,
                        &mut responses,
                    );
                }
            }
            ClientMessage::Resize {
                cols: _,
                rows: _,
                width,
                height,
                cell_width,
                cell_height,
            } => {
                log::debug!(
                    "client {client_id} Resize: {width}x{height}px, cell={cell_width:.1}x{cell_height:.1}"
                );
                if cell_width.is_finite()
                    && cell_width > 0.0
                    && cell_width <= 200.0
                    && cell_height.is_finite()
                    && cell_height > 0.0
                    && cell_height <= 200.0
                    && width > 0
                    && width <= 16384
                    && height > 0
                    && height <= 16384
                {
                    if let Some(client) = self.clients.get_mut(&client_id) {
                        client.cell_width = cell_width;
                        client.cell_height = cell_height;
                        client.viewport_width = width as f32;
                        client.viewport_height = height as f32;
                    }
                    self.with_session(&session_name, |session, clients| {
                        if session.resize_all_panes(clients) {
                            session.mark_session_dirty();
                            responses.push(ServerResponse::BroadcastToSession(
                                session_name.to_string(),
                                ServerMessage::LayoutUpdate {
                                    layout: session.layout_state(),
                                },
                            ));
                        }
                    });
                } else {
                    log::warn!(
                        "ignoring invalid resize from client {client_id}: {width}x{height} cell={cell_width}x{cell_height}"
                    );
                }
            }
            ClientMessage::SetColumnWidth {
                proportion,
                fixed_px,
            } => {
                self.with_session(&session_name, |session, clients| {
                    let width = match fixed_px {
                        Some(px) => ColumnWidth::Fixed(px),
                        None => ColumnWidth::Proportion(proportion),
                    };
                    session
                        .workspaces
                        .active_mut()
                        .set_active_column_width(width);
                    Self::layout_changed(session, clients, &session_name, true, &mut responses);
                });
            }
            ClientMessage::AdjustColumnSplit { delta } => {
                self.with_session(&session_name, |session, clients| {
                    session
                        .workspaces
                        .active_mut()
                        .resize_active_with_neighbor(delta);
                    Self::layout_changed(session, clients, &session_name, true, &mut responses);
                });
            }
            ClientMessage::EqualizeColumnSplit => {
                self.with_session(&session_name, |session, clients| {
                    session
                        .workspaces
                        .active_mut()
                        .equalize_active_with_neighbor();
                    Self::layout_changed(session, clients, &session_name, true, &mut responses);
                });
            }
            ClientMessage::SetTileWeights {
                column_idx,
                top_tile_idx,
                top_weight,
                bottom_weight,
            } => {
                self.with_session(&session_name, |session, clients| {
                    if let Some(col) = session.workspaces.active_mut().columns.get_mut(column_idx) {
                        col.set_tile_weights(top_tile_idx, top_weight, bottom_weight);
                    }
                    Self::layout_changed(session, clients, &session_name, true, &mut responses);
                });
            }
            ClientMessage::AdjustColumnSplitAt { column_idx, delta } => {
                self.with_session(&session_name, |session, clients| {
                    let ws = session.workspaces.active_mut();
                    let saved_idx = ws.active_column_idx;
                    ws.active_column_idx = column_idx;
                    ws.resize_active_with_neighbor(delta);
                    ws.active_column_idx = saved_idx;
                    Self::layout_changed(session, clients, &session_name, true, &mut responses);
                });
            }
            ClientMessage::ConsumeIntoColumn => {
                self.with_session(&session_name, |session, clients| {
                    session.workspaces.active_mut().consume_from_right();
                    Self::layout_changed(session, clients, &session_name, true, &mut responses);
                });
            }
            ClientMessage::ExpelFromColumn => {
                self.with_session(&session_name, |session, clients| {
                    session.workspaces.active_mut().expel_active_tile();
                    Self::layout_changed(session, clients, &session_name, true, &mut responses);
                });
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
                    Self::layout_changed(
                        session,
                        &mut self.clients,
                        &session_name,
                        false,
                        &mut responses,
                    );
                }
            }
            ClientMessage::MouseInput {
                pane_id,
                button,
                col,
                row,
                pressed,
                modifiers,
            } => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    if let Some(pane) = session.panes.get_mut(&pane_id) {
                        if pane.has_mouse_mode() {
                            pane.send_mouse_input(button, col, row, pressed, modifiers);
                        }
                    }
                }
            }
            ClientMessage::ListSessions { all } => {
                // Collect running sessions, sorted by last_attached (most recent first)
                let mut sessions: Vec<SessionInfo> = self
                    .sessions
                    .iter()
                    .filter(|(name, _)| !name.starts_with("__"))
                    .map(|(name, sess)| {
                        let client_count = self
                            .clients
                            .values()
                            .filter(|c| c.session_name == *name)
                            .count();
                        (sess.last_attached, SessionInfo {
                            name: name.clone(),
                            running: true,
                            pane_count: sess.panes.len(),
                            client_count,
                        })
                    })
                    .collect::<Vec<_>>()
                    .into_iter()
                    .map(|(_ts, info)| info)
                    .collect();
                // Sort running sessions by last_attached descending
                {
                    let mut with_ts: Vec<_> = self
                        .sessions
                        .iter()
                        .map(|(name, sess)| (name.clone(), sess.last_attached))
                        .collect();
                    with_ts.sort_by(|a, b| b.1.cmp(&a.1));
                    let order: HashMap<String, usize> = with_ts
                        .iter()
                        .enumerate()
                        .map(|(i, (n, _))| (n.clone(), i))
                        .collect();
                    sessions.sort_by_key(|s| order.get(&s.name).copied().unwrap_or(usize::MAX));
                }
                // Optionally merge saved sessions
                if all {
                    let running_names: std::collections::HashSet<String> =
                        sessions.iter().map(|s| s.name.clone()).collect();
                    if let Ok(saved_names) =
                        ciri_session::restore::list_sessions(&transport::state_dir())
                    {
                        for name in saved_names {
                            if !running_names.contains(&name) {
                                sessions.push(SessionInfo {
                                    name,
                                    running: false,
                                    pane_count: 0,
                                    client_count: 0,
                                });
                            }
                        }
                    }
                }
                responses.push(ServerResponse::SendToClient(
                    client_id,
                    ServerMessage::SessionList { sessions },
                ));
            }
            ClientMessage::KillSession {
                session_name: target,
            } => {
                if let Some(session) = self.sessions.remove(&target) {
                    // Kill all panes in the session
                    drop(session);
                    // Delete saved session file
                    let _ = ciri_session::restore::delete_session(&target, &transport::state_dir());
                    // Notify clients attached to the killed session
                    let affected_clients: Vec<u64> = self
                        .clients
                        .iter()
                        .filter(|(_, c)| c.session_name == target)
                        .map(|(id, _)| *id)
                        .collect();
                    for cid in affected_clients {
                        self.send_to_client(cid, &ServerMessage::ServerShutdown);
                        responses.push(ServerResponse::RemoveClient(cid));
                    }
                    // Notify the requesting client that the session was killed
                    responses.push(ServerResponse::SendToClient(
                        client_id,
                        ServerMessage::SessionKilled {
                            session_name: target,
                        },
                    ));
                } else {
                    // Try to delete saved session file even if not running
                    let _ = ciri_session::restore::delete_session(&target, &transport::state_dir());
                    responses.push(ServerResponse::SendToClient(
                        client_id,
                        ServerMessage::SessionKilled {
                            session_name: target,
                        },
                    ));
                }
            }
            ClientMessage::FocusChange { focused } => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    // Send focus event to the active pane (if it has DECSET 1004 enabled)
                    if let Some(pane_id) = session.workspaces.active().active_pane_id() {
                        if let Some(pane) = session.panes.get_mut(&pane_id) {
                            pane.write_focus_event(focused);
                        }
                    }
                }
            }
            ClientMessage::FocusPane { pane_id } => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    if session.focus_pane(pane_id) {
                        session.mark_session_dirty();
                        responses.push(ServerResponse::BroadcastToSession(
                            session_name.clone(),
                            ServerMessage::LayoutUpdate {
                                layout: session.layout_state(),
                            },
                        ));
                    } else {
                        log::debug!("focus_pane: pane {} not found in session '{}'", pane_id, session_name);
                    }
                }
            }
            ClientMessage::KillServer => {
                responses.push(ServerResponse::ShutdownServer);
            }
            // ─── IPC commands (from __control__ clients) ────────────────
            ClientMessage::SendKeys { session_name: target, pane_id, keys } => {
                if let Some(session) = self.sessions.get_mut(&target) {
                    if let Some(pane) = session.panes.get_mut(&pane_id) {
                        pane.write_to_pty(&keys);
                        responses.push(ServerResponse::SendToClient(client_id, ServerMessage::CommandResult {
                            success: true, message: "ok".to_string(), pane_id: Some(pane_id),
                        }));
                    } else {
                        responses.push(ServerResponse::SendToClient(client_id, ServerMessage::Error {
                            message: format!("pane {} not found in session '{}'", pane_id, target),
                        }));
                    }
                } else {
                    responses.push(ServerResponse::SendToClient(client_id, ServerMessage::Error {
                        message: format!("session '{}' not found", target),
                    }));
                }
            }
            ClientMessage::GetSessionInfo { session_name: target } => {
                if let Some(session) = self.sessions.get(&target) {
                    let client_count = self.clients.values()
                        .filter(|c| c.session_name == target)
                        .count();
                    let info = SessionDetailInfo {
                        name: target.clone(),
                        running: true,
                        pane_count: session.panes.len(),
                        client_count,
                        workspace_count: session.workspaces.workspaces.len(),
                        active_workspace: session.workspaces.active_workspace_idx,
                    };
                    responses.push(ServerResponse::SendToClient(client_id, ServerMessage::SessionInfoReply { info }));
                } else {
                    responses.push(ServerResponse::SendToClient(client_id, ServerMessage::Error {
                        message: format!("session '{}' not found", target),
                    }));
                }
            }
            ClientMessage::ListPanes { session_name: target } => {
                if let Some(session) = self.sessions.get(&target) {
                    let mut panes = Vec::new();
                    let active_ws = session.workspaces.active_workspace_idx;
                    for (ws_idx, ws) in session.workspaces.workspaces.iter().enumerate() {
                        let active_col = ws.active_column_idx;
                        for (col_idx, col) in ws.columns.iter().enumerate() {
                            let active_tile = col.active_tile_idx;
                            for (tile_idx, tile) in col.tiles.iter().enumerate() {
                                let is_active = ws_idx == active_ws && col_idx == active_col && tile_idx == active_tile;
                                let (cols, rows) = session.panes.get(&tile.pane_id)
                                    .map(|p| (p.grid_cols(), p.grid_rows()))
                                    .unwrap_or((0, 0));
                                let title = session.panes.get(&tile.pane_id)
                                    .map(|p| p.title.clone())
                                    .unwrap_or_default();
                                panes.push(PaneDetailInfo {
                                    pane_id: tile.pane_id,
                                    cols,
                                    rows,
                                    title,
                                    cwd: None,
                                    is_active,
                                    workspace_idx: ws_idx,
                                    column_idx: col_idx,
                                    tile_idx,
                                });
                            }
                        }
                    }
                    responses.push(ServerResponse::SendToClient(client_id, ServerMessage::PaneListReply { panes }));
                } else {
                    responses.push(ServerResponse::SendToClient(client_id, ServerMessage::Error {
                        message: format!("session '{}' not found", target),
                    }));
                }
            }
            ClientMessage::FocusPaneById { session_name: target, pane_id } => {
                if let Some(session) = self.sessions.get_mut(&target) {
                    if session.focus_pane(pane_id) {
                        session.mark_session_dirty();
                        let layout = session.layout_state();
                        responses.push(ServerResponse::BroadcastToSession(target.clone(), ServerMessage::LayoutUpdate { layout }));
                        responses.push(ServerResponse::SendToClient(client_id, ServerMessage::CommandResult {
                            success: true, message: "ok".to_string(), pane_id: Some(pane_id),
                        }));
                    } else {
                        responses.push(ServerResponse::SendToClient(client_id, ServerMessage::Error {
                            message: format!("pane {} not found in session '{}'", pane_id, target),
                        }));
                    }
                } else {
                    responses.push(ServerResponse::SendToClient(client_id, ServerMessage::Error {
                        message: format!("session '{}' not found", target),
                    }));
                }
            }
            ClientMessage::ClosePaneById { session_name: target, pane_id } => {
                if let Some(mut session) = self.sessions.remove(&target) {
                    if session.panes.contains_key(&pane_id) {
                        session.close_pane(pane_id, &mut self.clients);
                        session.resize_all_panes(&mut self.clients);
                        session.mark_session_dirty();
                        responses.push(ServerResponse::BroadcastToSession(target.clone(), ServerMessage::PaneClosed { pane_id }));
                        let layout = session.layout_state();
                        responses.push(ServerResponse::BroadcastToSession(target.clone(), ServerMessage::LayoutUpdate { layout }));
                        responses.push(ServerResponse::SendToClient(client_id, ServerMessage::CommandResult {
                            success: true, message: "ok".to_string(), pane_id: Some(pane_id),
                        }));
                    } else {
                        responses.push(ServerResponse::SendToClient(client_id, ServerMessage::Error {
                            message: format!("pane {} not found in session '{}'", pane_id, target),
                        }));
                    }
                    self.sessions.insert(target, session);
                } else {
                    responses.push(ServerResponse::SendToClient(client_id, ServerMessage::Error {
                        message: format!("session '{}' not found", target),
                    }));
                }
            }
            ClientMessage::CreatePaneIn { session_name: target } => {
                if let Some(mut session) = self.sessions.remove(&target) {
                    match session.create_pane(&mut self.next_pane_id, &mut self.clients) {
                        Ok(id) => {
                            session.resize_all_panes(&mut self.clients);
                            session.mark_session_dirty();
                            let (cols, rows) = session.pane_grid_dims(id);
                            responses.push(ServerResponse::BroadcastToSession(target.clone(), ServerMessage::PaneCreated {
                                pane_id: id, column_idx: session.workspaces.active().active_column_idx, cols, rows,
                            }));
                            let layout = session.layout_state();
                            responses.push(ServerResponse::BroadcastToSession(target.clone(), ServerMessage::LayoutUpdate { layout }));
                            responses.push(ServerResponse::SendToClient(client_id, ServerMessage::CommandResult {
                                success: true, message: "ok".to_string(), pane_id: Some(id),
                            }));
                        }
                        Err(e) => {
                            responses.push(ServerResponse::SendToClient(client_id, ServerMessage::Error {
                                message: format!("failed to create pane: {e}"),
                            }));
                        }
                    }
                    self.sessions.insert(target, session);
                } else {
                    responses.push(ServerResponse::SendToClient(client_id, ServerMessage::Error {
                        message: format!("session '{}' not found", target),
                    }));
                }
            }
            ClientMessage::RunCommand { session_name: target, command: _, cwd: _ } => {
                // For now, just create a regular pane (same as CreatePaneIn).
                if let Some(mut session) = self.sessions.remove(&target) {
                    match session.create_pane(&mut self.next_pane_id, &mut self.clients) {
                        Ok(id) => {
                            session.resize_all_panes(&mut self.clients);
                            session.mark_session_dirty();
                            let (cols, rows) = session.pane_grid_dims(id);
                            responses.push(ServerResponse::BroadcastToSession(target.clone(), ServerMessage::PaneCreated {
                                pane_id: id, column_idx: session.workspaces.active().active_column_idx, cols, rows,
                            }));
                            let layout = session.layout_state();
                            responses.push(ServerResponse::BroadcastToSession(target.clone(), ServerMessage::LayoutUpdate { layout }));
                            responses.push(ServerResponse::SendToClient(client_id, ServerMessage::CommandResult {
                                success: true, message: "ok".to_string(), pane_id: Some(id),
                            }));
                        }
                        Err(e) => {
                            responses.push(ServerResponse::SendToClient(client_id, ServerMessage::Error {
                                message: format!("failed to create pane: {e}"),
                            }));
                        }
                    }
                    self.sessions.insert(target, session);
                } else {
                    responses.push(ServerResponse::SendToClient(client_id, ServerMessage::Error {
                        message: format!("session '{}' not found", target),
                    }));
                }
            }
            ClientMessage::GetLayout { session_name: target } => {
                if let Some(session) = self.sessions.get(&target) {
                    responses.push(ServerResponse::SendToClient(client_id, ServerMessage::LayoutReply {
                        layout: session.layout_state(),
                        session_name: target,
                    }));
                } else {
                    responses.push(ServerResponse::SendToClient(client_id, ServerMessage::Error {
                        message: format!("session '{}' not found", target),
                    }));
                }
            }
            // ─── Template commands ────────────────────────────────────
            ClientMessage::ListTemplates => {
                use ciri_session::template;
                let mut templates = Vec::new();
                match template::list_templates() {
                    Ok(names) => {
                        for name in names {
                            match template::load_template(&name) {
                                Ok(tpl) => {
                                    let total_panes: usize = tpl.workspaces.iter()
                                        .map(|ws| ws.columns.iter().map(|c| c.tiles.len()).sum::<usize>())
                                        .sum();
                                    templates.push(TemplateInfo {
                                        name,
                                        description: tpl.description,
                                        workspace_count: tpl.workspaces.len(),
                                        total_panes,
                                    });
                                }
                                Err(e) => {
                                    log::warn!("failed to load template '{name}': {e}");
                                }
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("failed to list templates: {e}");
                    }
                }
                responses.push(ServerResponse::SendToClient(client_id, ServerMessage::TemplateList { templates }));
            }
            ClientMessage::SaveTemplate { template_name, session_name: target_session } => {
                use ciri_session::template::{self, LayoutTemplate, TemplateWorkspace, TemplateColumn, TemplateTile, TemplateWidth};
                let source_session = if target_session.is_empty() { &session_name } else { &target_session };
                if let Some(session) = self.sessions.get(source_session) {
                    let layout = session.layout_state();
                    let tpl = LayoutTemplate {
                        description: Some(format!("Saved from session '{}'", source_session)),
                        workspaces: layout.workspaces.iter().map(|ws| TemplateWorkspace {
                            columns: ws.columns.iter().map(|col| TemplateColumn {
                                tiles: col.tiles.iter().map(|tile| TemplateTile {
                                    command: String::new(),
                                    cwd: String::new(),
                                    weight: tile.weight as f64,
                                }).collect(),
                                width: Some(TemplateWidth::Proportion { proportion: col.width_proportion }),
                            }).collect(),
                            active_column: ws.active_column_idx,
                        }).collect(),
                    };
                    match template::save_template(&template_name, &tpl) {
                        Ok(()) => {
                            log::info!("saved template '{}' from session '{}'", template_name, source_session);
                            responses.push(ServerResponse::SendToClient(client_id, ServerMessage::TemplateSaved { template_name }));
                        }
                        Err(e) => {
                            responses.push(ServerResponse::SendToClient(client_id, ServerMessage::Error {
                                message: format!("failed to save template: {e}"),
                            }));
                        }
                    }
                } else {
                    responses.push(ServerResponse::SendToClient(client_id, ServerMessage::Error {
                        message: format!("session '{}' not found", source_session),
                    }));
                }
            }
            ClientMessage::ApplyTemplate { template_name, session_name: target_session } => {
                use ciri_session::template::{self, TemplateWidth};
                match template::load_template(&template_name) {
                    Ok(tpl) => {
                        let target = if target_session.is_empty() {
                            session_name.clone()
                        } else {
                            target_session
                        };

                        // Remove existing session if present (kill all its panes)
                        if let Some(old) = self.sessions.remove(&target) {
                            // Notify other clients on this session
                            for client in self.clients.values() {
                                if client.session_name == target && client.id != client_id {
                                    self.send_to_client(client.id, &ServerMessage::ServerShutdown);
                                }
                            }
                            drop(old);
                        }

                        // Create new session
                        self.had_session = true;
                        let mut session = Session::new(&target, &self.default_shell, self.column_gap);
                        session.default_column_width = self.default_column_width;
                        session.pane_inset = self.pane_inset;

                        let vw = session.workspaces.view_size.width;
                        let vh = session.workspaces.view_size.height;
                        let (_viewport_w, _viewport_h, cw, ch) = Session::effective_dims_from(&self.clients, &target);

                        // Clear the default workspace
                        session.workspaces.workspaces.clear();

                        for tpl_ws in &tpl.workspaces {
                            let mut ws = ciri_layout::workspace::Workspace::new_with_gap(
                                session.workspaces.view_size,
                                session.workspaces.column_gap,
                            );

                            for tpl_col in &tpl_ws.columns {
                                let col_proportion = match &tpl_col.width {
                                    Some(TemplateWidth::Proportion { proportion }) => *proportion,
                                    Some(TemplateWidth::Fixed { fixed }) => *fixed / vw as f64,
                                    None => 0.5,
                                };
                                let col_w = (vw as f64 * col_proportion) as f32;

                                let mut col_opt: Option<ciri_layout::column::Column> = None;
                                for tpl_tile in &tpl_col.tiles {
                                    let id = self.next_pane_id;
                                    self.next_pane_id += 1;
                                    let tile_h = vh / tpl_col.tiles.len() as f32;
                                    let (cols, rows) = session.pane_grid_size_with_cells(col_w, tile_h, cw, ch);

                                    let cmd = if tpl_tile.command.is_empty() { None } else { Some(tpl_tile.command.as_str()) };
                                    let cwd = if tpl_tile.cwd.is_empty() {
                                        None
                                    } else {
                                        Some(std::path::Path::new(&tpl_tile.cwd))
                                    };

                                    match Pane::new_with_opts(id, cols, rows, &session.default_shell, cmd, cwd) {
                                        Ok(pane) => {
                                            session.panes.insert(id, pane);
                                            session.generation.insert(id, 0);
                                            if let Some(col) = &mut col_opt {
                                                let mut tile = ciri_layout::tile::Tile::new(id);
                                                tile.height = ciri_layout::tile::TileHeight::Auto { weight: tpl_tile.weight };
                                                col.tiles.push(tile);
                                            } else {
                                                let mut col = ciri_layout::column::Column::new(id);
                                                col.width = ColumnWidth::Proportion(col_proportion);
                                                if let Some(first_tile) = col.tiles.first_mut() {
                                                    first_tile.height = ciri_layout::tile::TileHeight::Auto { weight: tpl_tile.weight };
                                                }
                                                col_opt = Some(col);
                                            }
                                        }
                                        Err(e) => {
                                            log::error!("failed to create pane from template: {e}");
                                        }
                                    }
                                }
                                if let Some(col) = col_opt {
                                    ws.columns.push(col);
                                }
                            }

                            ws.active_column_idx = tpl_ws.active_column.min(ws.columns.len().saturating_sub(1));
                            session.workspaces.workspaces.push(ws);
                        }

                        // Ensure at least one workspace with one pane
                        if session.panes.is_empty() {
                            let vs = session.workspaces.view_size;
                            let cg = session.workspaces.column_gap;
                            session.workspaces.workspaces.clear();
                            session.workspaces.workspaces.push(
                                ciri_layout::workspace::Workspace::new_with_gap(vs, cg),
                            );
                            if let Err(e) = session.create_pane(&mut self.next_pane_id, &mut self.clients) {
                                log::error!("failed to create fallback pane: {e}");
                            }
                        }

                        session.workspaces.active_workspace_idx = 0;

                        // Update client's session affinity
                        if let Some(client) = self.clients.get_mut(&client_id) {
                            client.session_name = target.clone();
                            client.damage.clear();
                            client.history_sent.clear();
                        }

                        // Resize all panes
                        session.resize_all_panes(&mut self.clients);

                        // Mark all panes for full sync
                        let pane_keys: Vec<u64> = session.panes.keys().copied().collect();
                        if let Some(client) = self.clients.get_mut(&client_id) {
                            for pane_id in &pane_keys {
                                let mut acc = DamageAccumulator::default();
                                acc.mark_full();
                                client.damage.insert(*pane_id, acc);
                            }
                        }

                        // Build state sync
                        let (sync_msg, pane_syncs) = session.build_state_sync();

                        // Record history
                        let pane_histories: Vec<(u64, usize)> = session.panes.iter()
                            .map(|(&pid, pane)| (pid, pane.history_size()))
                            .collect();
                        if let Some(client) = self.clients.get_mut(&client_id) {
                            for (pane_id, history) in pane_histories {
                                client.history_sent.insert(pane_id, history);
                            }
                            for acc in client.damage.values_mut() {
                                *acc = DamageAccumulator::default();
                            }
                        }

                        session.mark_session_dirty();
                        self.sessions.insert(target.clone(), session);

                        // Send responses
                        responses.push(ServerResponse::SendToClient(client_id, ServerMessage::TemplateApplied {
                            session_name: target.clone(),
                        }));
                        responses.push(ServerResponse::SendToClient(client_id, sync_msg));
                        for sync in pane_syncs {
                            responses.push(ServerResponse::SendFullPaneSync(client_id, sync));
                        }
                    }
                    Err(e) => {
                        responses.push(ServerResponse::SendToClient(client_id, ServerMessage::Error {
                            message: format!("failed to load template '{}': {}", template_name, e),
                        }));
                    }
                }
            }
            ClientMessage::SwitchSession {
                session_name: target,
            } => {
                if ciri_session::names::validate_name(&target).is_err() {
                    responses.push(ServerResponse::SendToClient(
                        client_id,
                        ServerMessage::Error {
                            message: format!("invalid session name: {target:?}"),
                        },
                    ));
                    return responses;
                }

                let old_session_name = session_name.clone();

                // Update client's session affinity
                if let Some(client) = self.clients.get_mut(&client_id) {
                    client.session_name = target.clone();
                    client.damage.clear();
                    client.history_sent.clear();
                }

                // Get or create target session
                self.get_or_create_session(&target);

                self.with_session(&target, |new_session, clients| {
                    new_session.resize_all_panes(clients);

                    // Mark all panes for full sync for this client
                    if let Some(client) = clients.get_mut(&client_id) {
                        for &pane_id in new_session.panes.keys() {
                            let mut acc = DamageAccumulator::default();
                            acc.mark_full();
                            client.damage.insert(pane_id, acc);
                        }
                    }

                    // Build state sync for the new session
                    let (sync_msg, pane_syncs) = new_session.build_state_sync();

                    responses.push(ServerResponse::SendToClient(
                        client_id,
                        ServerMessage::SessionSwitched {
                            session_name: target.clone(),
                        },
                    ));
                    responses.push(ServerResponse::SendToClient(client_id, sync_msg));
                    for sync in pane_syncs {
                        responses.push(ServerResponse::SendFullPaneSync(client_id, sync));
                    }

                    // Record history_sent for the new session's panes
                    if let Some(client) = clients.get_mut(&client_id) {
                        for (&pid, pane) in &new_session.panes {
                            client.history_sent.insert(pid, pane.history_size());
                        }
                        for acc in client.damage.values_mut() {
                            *acc = DamageAccumulator::default();
                        }
                    }
                });

                // Resize panes in old session (client left, viewport may change)
                if old_session_name != target {
                    self.with_session(&old_session_name, |old_session, clients| {
                        old_session.resize_all_panes(clients);
                    });
                }
            }
        }

        responses
    }
}
