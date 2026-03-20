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
            let restored = ciri_session::restore::restore_session(session_name, &transport::state_dir())
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
    pub(crate) fn broadcast_to_session(&self, session_name: &str, msg: &ServerMessage) {
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
                            responses.push(ServerResponse::BroadcastToSession(
                                session_name.clone(),
                                ServerMessage::LayoutUpdate {
                                    layout: session.layout_state(),
                                },
                            ));
                        }
                        Err(e) => log::error!("failed to create pane: {e}"),
                    }
                    self.sessions.insert(session_name, session);
                }
            }
            ClientMessage::SplitDown => {
                if let Some(mut session) = self.sessions.remove(&session_name) {
                    match session.create_pane_in_new_workspace(
                        &mut self.next_pane_id,
                        &mut self.clients,
                    ) {
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
                            responses.push(ServerResponse::BroadcastToSession(
                                session_name.clone(),
                                ServerMessage::LayoutUpdate {
                                    layout: session.layout_state(),
                                },
                            ));
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
                    responses.push(ServerResponse::BroadcastToSession(
                        session_name.clone(),
                        ServerMessage::PaneClosed { pane_id },
                    ));
                    responses.push(ServerResponse::BroadcastToSession(
                        session_name.clone(),
                        ServerMessage::LayoutUpdate {
                            layout: session.layout_state(),
                        },
                    ));
                    self.sessions.insert(session_name, session);
                }
            }
            ClientMessage::FocusLeft => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    session.workspaces.active_mut().focus_left();
                    session.mark_session_dirty();
                    responses.push(ServerResponse::BroadcastToSession(
                        session_name,
                        ServerMessage::LayoutUpdate {
                            layout: session.layout_state(),
                        },
                    ));
                }
            }
            ClientMessage::FocusRight => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    session.workspaces.active_mut().focus_right();
                    session.mark_session_dirty();
                    responses.push(ServerResponse::BroadcastToSession(
                        session_name,
                        ServerMessage::LayoutUpdate {
                            layout: session.layout_state(),
                        },
                    ));
                }
            }
            ClientMessage::FocusUp => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    if !session.workspaces.active_mut().focus_tile_up() {
                        session.workspaces.focus_up();
                    }
                    session.mark_session_dirty();
                    responses.push(ServerResponse::BroadcastToSession(
                        session_name,
                        ServerMessage::LayoutUpdate {
                            layout: session.layout_state(),
                        },
                    ));
                }
            }
            ClientMessage::FocusDown => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    if !session.workspaces.active_mut().focus_tile_down() {
                        session.workspaces.focus_down();
                    }
                    session.mark_session_dirty();
                    responses.push(ServerResponse::BroadcastToSession(
                        session_name,
                        ServerMessage::LayoutUpdate {
                            layout: session.layout_state(),
                        },
                    ));
                }
            }
            ClientMessage::MovePaneLeft => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    session.workspaces.active_mut().move_pane_left();
                    session.mark_session_dirty();
                    responses.push(ServerResponse::BroadcastToSession(
                        session_name,
                        ServerMessage::LayoutUpdate {
                            layout: session.layout_state(),
                        },
                    ));
                }
            }
            ClientMessage::MovePaneRight => {
                if let Some(session) = self.sessions.get_mut(&session_name) {
                    session.workspaces.active_mut().move_pane_right();
                    session.mark_session_dirty();
                    responses.push(ServerResponse::BroadcastToSession(
                        session_name,
                        ServerMessage::LayoutUpdate {
                            layout: session.layout_state(),
                        },
                    ));
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
                    if let Some(mut session) = self.sessions.remove(&session_name) {
                        session.resize_all_panes(&mut self.clients);
                        responses.push(ServerResponse::BroadcastToSession(
                            session_name.clone(),
                            ServerMessage::LayoutUpdate {
                                layout: session.layout_state(),
                            },
                        ));
                        self.sessions.insert(session_name, session);
                    }
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
                if let Some(mut session) = self.sessions.remove(&session_name) {
                    let width = match fixed_px {
                        Some(px) => ColumnWidth::Fixed(px),
                        None => ColumnWidth::Proportion(proportion),
                    };
                    session
                        .workspaces
                        .active_mut()
                        .set_active_column_width(width);
                    session.mark_session_dirty();
                    session.resize_all_panes(&mut self.clients);
                    responses.push(ServerResponse::BroadcastToSession(
                        session_name.clone(),
                        ServerMessage::LayoutUpdate {
                            layout: session.layout_state(),
                        },
                    ));
                    self.sessions.insert(session_name, session);
                }
            }
            ClientMessage::AdjustColumnSplit { delta } => {
                if let Some(mut session) = self.sessions.remove(&session_name) {
                    session
                        .workspaces
                        .active_mut()
                        .resize_active_with_neighbor(delta);
                    session.mark_session_dirty();
                    session.resize_all_panes(&mut self.clients);
                    responses.push(ServerResponse::BroadcastToSession(
                        session_name.clone(),
                        ServerMessage::LayoutUpdate {
                            layout: session.layout_state(),
                        },
                    ));
                    self.sessions.insert(session_name, session);
                }
            }
            ClientMessage::EqualizeColumnSplit => {
                if let Some(mut session) = self.sessions.remove(&session_name) {
                    session
                        .workspaces
                        .active_mut()
                        .equalize_active_with_neighbor();
                    session.mark_session_dirty();
                    session.resize_all_panes(&mut self.clients);
                    responses.push(ServerResponse::BroadcastToSession(
                        session_name.clone(),
                        ServerMessage::LayoutUpdate {
                            layout: session.layout_state(),
                        },
                    ));
                    self.sessions.insert(session_name, session);
                }
            }
            ClientMessage::SetTileWeights {
                column_idx,
                top_tile_idx,
                top_weight,
                bottom_weight,
            } => {
                if let Some(mut session) = self.sessions.remove(&session_name) {
                    let ws = session.workspaces.active_mut();
                    if let Some(col) = ws.columns.get_mut(column_idx) {
                        col.set_tile_weights(top_tile_idx, top_weight, bottom_weight);
                    }
                    session.mark_session_dirty();
                    session.resize_all_panes(&mut self.clients);
                    responses.push(ServerResponse::BroadcastToSession(
                        session_name.clone(),
                        ServerMessage::LayoutUpdate {
                            layout: session.layout_state(),
                        },
                    ));
                    self.sessions.insert(session_name, session);
                }
            }
            ClientMessage::AdjustColumnSplitAt {
                column_idx,
                delta,
            } => {
                if let Some(mut session) = self.sessions.remove(&session_name) {
                    let ws = session.workspaces.active_mut();
                    let saved_idx = ws.active_column_idx;
                    ws.active_column_idx = column_idx;
                    ws.resize_active_with_neighbor(delta);
                    ws.active_column_idx = saved_idx;
                    session.mark_session_dirty();
                    session.resize_all_panes(&mut self.clients);
                    responses.push(ServerResponse::BroadcastToSession(
                        session_name.clone(),
                        ServerMessage::LayoutUpdate {
                            layout: session.layout_state(),
                        },
                    ));
                    self.sessions.insert(session_name, session);
                }
            }
            ClientMessage::ConsumeIntoColumn => {
                if let Some(mut session) = self.sessions.remove(&session_name) {
                    session.workspaces.active_mut().consume_from_right();
                    session.resize_all_panes(&mut self.clients);
                    session.mark_session_dirty();
                    responses.push(ServerResponse::BroadcastToSession(
                        session_name.clone(),
                        ServerMessage::LayoutUpdate {
                            layout: session.layout_state(),
                        },
                    ));
                    self.sessions.insert(session_name, session);
                }
            }
            ClientMessage::ExpelFromColumn => {
                if let Some(mut session) = self.sessions.remove(&session_name) {
                    session.workspaces.active_mut().expel_active_tile();
                    session.resize_all_panes(&mut self.clients);
                    session.mark_session_dirty();
                    responses.push(ServerResponse::BroadcastToSession(
                        session_name.clone(),
                        ServerMessage::LayoutUpdate {
                            layout: session.layout_state(),
                        },
                    ));
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
                    responses.push(ServerResponse::BroadcastToSession(
                        session_name,
                        ServerMessage::LayoutUpdate {
                            layout: session.layout_state(),
                        },
                    ));
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
                    let client_count = self
                        .clients
                        .values()
                        .filter(|c| c.session_name == *name)
                        .count();
                    session_map.insert(
                        name.clone(),
                        SessionInfo {
                            name: name.clone(),
                            running: true,
                            pane_count: sess.panes.len(),
                            client_count,
                        },
                    );
                }
                // Merge with saved sessions
                if let Ok(saved_names) =
                    ciri_session::restore::list_sessions(&transport::state_dir())
                {
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
                    let _ =
                        ciri_session::restore::delete_session(&target, &transport::state_dir());
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
                    let _ =
                        ciri_session::restore::delete_session(&target, &transport::state_dir());
                    responses.push(ServerResponse::SendToClient(
                        client_id,
                        ServerMessage::SessionKilled {
                            session_name: target,
                        },
                    ));
                }
            }
            ClientMessage::KillServer => {
                responses.push(ServerResponse::ShutdownServer);
            }
            ClientMessage::SwitchSession {
                session_name: target,
            } => {
                if ciri_session::save::validate_session_name(&target).is_err() {
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
                    let pane_histories: Vec<(u64, usize)> = new_session
                        .panes
                        .iter()
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
