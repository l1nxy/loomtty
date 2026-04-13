mod input;
mod ipc;
mod layout;
mod restore;
mod session_mgmt;
mod template;

use ciri_layout::column::ColumnWidth;
use ciri_protocol::message::*;
use ciri_term::pane::TerminalColors;
use std::collections::HashMap;
use std::time::{Duration, Instant};

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
    /// When set, the server will shut down after this instant if still idle.
    pub(crate) idle_deadline: Option<Instant>,
    /// How long to wait before shutting down after all sessions/clients gone.
    pub(crate) idle_timeout: Duration,
    /// Session restore configuration.
    pub(crate) session_config: ciri_config::schema::SessionConfig,
    /// Theme colors for initializing terminal palettes.
    pub(crate) terminal_colors: TerminalColors,
    /// Set to true after `graceful_shutdown` runs. Prevents double-shutdown.
    pub(crate) shut_down: bool,
    /// Callback to wake the tick loop when PTY output is available.
    pub(crate) pty_notify: Option<ciri_term::pty::PtyOutputNotify>,
}

const CONTROL_SESSION: &str = "__control__";

/// Internal response type for message handling.
pub(crate) enum ServerResponse {
    BroadcastToSession(String, ServerMessage),
    SendToClient(u64, ServerMessage),
    SendFullPaneSync(u64, FullPaneSync),
    RemoveClient(u64),
    ShutdownServer,
}

#[derive(Clone, Copy)]
pub(super) struct ResizeMessage {
    pub width: u32,
    pub height: u32,
    pub cell_width: f32,
    pub cell_height: f32,
}

impl Server {
    pub(crate) fn new(shell: &str, column_gap: f32, terminal_colors: TerminalColors) -> Self {
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
            idle_deadline: None,
            idle_timeout: Duration::from_secs(300),
            session_config: ciri_config::schema::SessionConfig::default(),
            terminal_colors,
            shut_down: false,
            pty_notify: None,
        }
    }

    // ── Communication helpers ────────────────────────────────────────

    /// Send a framed control message to a specific client.
    pub(crate) fn send_to_client(&self, client_id: u64, msg: &ServerMessage) {
        if let (Some(client), Some(frame)) = (
            self.clients.get(&client_id),
            ciri_protocol::codec::frame_server_msg(msg),
        ) && let Err(e) = client.tx.try_send(bytes::Bytes::from(frame))
        {
            log::warn!("failed to send to client {client_id}: {e}");
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

    // ── Borrow-checker workaround ────────────────────────────────────

    /// Temporarily remove a session from the map, call `f` with it and `&mut self.clients`,
    /// then re-insert it. Uses a drop guard to ensure re-insertion even if `f` panics.
    pub(super) fn with_session<F, R>(&mut self, name: &str, f: F) -> Option<R>
    where
        F: FnOnce(&mut Session, &mut HashMap<u64, ClientState>) -> R,
    {
        let session = self.sessions.remove(name)?;

        struct ReinsertGuard<'a> {
            sessions: &'a mut HashMap<String, Session>,
            name: String,
            session: Option<Session>,
        }
        impl<'a> Drop for ReinsertGuard<'a> {
            fn drop(&mut self) {
                if let Some(session) = self.session.take() {
                    self.sessions.insert(self.name.clone(), session);
                }
            }
        }

        let mut guard = ReinsertGuard {
            sessions: &mut self.sessions,
            name: name.to_string(),
            session: Some(session),
        };
        let result = f(guard.session.as_mut().unwrap(), &mut self.clients);
        let session = guard.session.take().unwrap();
        guard.sessions.insert(guard.name.clone(), session);
        std::mem::forget(guard);
        Some(result)
    }

    // ── Layout helpers (used by layout, ipc, template) ──────────────

    /// Common tail for handlers that mutate session layout.
    pub(super) fn layout_changed(
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

    pub(super) fn broadcast_layout_update(
        session: &Session,
        session_name: &str,
        responses: &mut Vec<ServerResponse>,
    ) {
        responses.push(ServerResponse::BroadcastToSession(
            session_name.to_string(),
            ServerMessage::LayoutUpdate {
                layout: session.layout_state(),
            },
        ));
    }

    pub(super) fn mark_layout_dirty(
        session: &mut Session,
        session_name: &str,
        responses: &mut Vec<ServerResponse>,
    ) {
        session.mark_session_dirty();
        Self::broadcast_layout_update(session, session_name, responses);
    }

    pub(super) fn close_pane_and_sync_layout(
        session: &mut Session,
        clients: &mut HashMap<u64, ClientState>,
        session_name: &str,
        pane_id: u64,
        responses: &mut Vec<ServerResponse>,
    ) {
        session.close_pane(pane_id, clients);
        responses.push(ServerResponse::BroadcastToSession(
            session_name.to_string(),
            ServerMessage::PaneClosed { pane_id },
        ));
        Self::layout_changed(session, clients, session_name, true, responses);
    }

    pub(super) fn create_pane_and_sync_layout(
        session: &mut Session,
        clients: &mut HashMap<u64, ClientState>,
        session_name: &str,
        pane_id: u64,
        responses: &mut Vec<ServerResponse>,
    ) {
        session.resize_all_panes(clients);
        session.mark_session_dirty();
        let (cols, rows) = session.pane_grid_dims(pane_id);
        responses.push(ServerResponse::BroadcastToSession(
            session_name.to_string(),
            ServerMessage::PaneCreated {
                pane_id,
                column_idx: session.workspaces.active().active_column_idx,
                cols,
                rows,
            },
        ));
        Self::layout_changed(session, clients, session_name, false, responses);
    }

    // ── Session helpers (used by session_mgmt) ──────────────────────

    fn session_is_control(session_name: &str) -> bool {
        session_name == CONTROL_SESSION
    }

    pub(super) fn refresh_session_attach_time(&mut self, session_name: &str) {
        if Self::session_is_control(session_name) {
            return;
        }
        if let Some(session) = self.sessions.get_mut(session_name) {
            session.last_attached = tokio::time::Instant::now();
        }
    }

    pub(super) fn switch_client_session_affinity(&mut self, client_id: u64, session_name: &str) {
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.session_name = session_name.to_string();
            client.damage.clear();
            client.history_sent.clear();
            client.last_acked_generation = 0;
            client.send_failures = 0;
        }
    }

    pub(super) fn prepare_full_sync_for_client(
        &mut self,
        client_id: u64,
        session_name: &str,
        responses: &mut Vec<ServerResponse>,
    ) {
        self.with_session(session_name, |session, clients| {
            session.resize_all_panes(clients);

            if let Some(client) = clients.get_mut(&client_id) {
                for &pane_id in session.panes.keys() {
                    let mut acc = DamageAccumulator::default();
                    acc.mark_full();
                    client.damage.insert(pane_id, acc);
                }
            }

            let (sync_msg, pane_syncs, image_events) = session.build_state_sync();

            if let Some(client) = clients.get_mut(&client_id) {
                for (&pid, pane) in &session.panes {
                    client.history_sent.insert(pid, pane.scrollback_total());
                }
                for acc in client.damage.values_mut() {
                    *acc = DamageAccumulator::default();
                }
            }

            responses.push(ServerResponse::SendToClient(client_id, sync_msg));
            for sync in pane_syncs {
                responses.push(ServerResponse::SendFullPaneSync(client_id, sync));
            }
            for msg in image_events {
                responses.push(ServerResponse::SendToClient(client_id, msg));
            }
        });
    }

    fn apply_resize_for_client(
        &mut self,
        session_name: &str,
        client_id: u64,
        resize: &ResizeMessage,
        responses: &mut Vec<ServerResponse>,
    ) {
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.cell_width = resize.cell_width;
            client.cell_height = resize.cell_height;
            client.viewport_width = resize.width as f32;
            client.viewport_height = resize.height as f32;
        }
        self.with_session(session_name, |session, clients| {
            if session.resize_all_panes(clients) {
                session.mark_session_dirty();
                Self::broadcast_layout_update(session, session_name, responses);
            }
        });
    }

    // ── Message dispatcher ──────────────────────────────────────────

    pub(crate) fn handle_message(
        &mut self,
        msg: ClientMessage,
        client_id: u64,
    ) -> Vec<ServerResponse> {
        let mut responses = Vec::new();

        let session_name = match self.clients.get(&client_id) {
            Some(client) => client.session_name.clone(),
            None => return responses,
        };

        match msg {
            // Input & terminal I/O
            ClientMessage::Input { .. }
            | ClientMessage::MouseInput { .. }
            | ClientMessage::FocusChange { .. } => {
                self.handle_input(msg, client_id, &session_name, &mut responses);
            }

            // Pane/layout management
            ClientMessage::CreatePane
            | ClientMessage::SplitDown
            | ClientMessage::ClosePane { .. }
            | ClientMessage::FocusLeft
            | ClientMessage::FocusRight
            | ClientMessage::FocusUp
            | ClientMessage::FocusDown
            | ClientMessage::MovePaneLeft
            | ClientMessage::MovePaneRight
            | ClientMessage::FocusPane { .. }
            | ClientMessage::SetColumnWidth { .. }
            | ClientMessage::AdjustColumnSplit { .. }
            | ClientMessage::EqualizeColumnSplit
            | ClientMessage::SetTileWeights { .. }
            | ClientMessage::AdjustColumnSplitAt { .. }
            | ClientMessage::ConsumeIntoColumn
            | ClientMessage::ExpelFromColumn => {
                self.handle_layout(msg, client_id, &session_name, &mut responses);
            }

            // Session management
            ClientMessage::Resize { .. }
            | ClientMessage::SwitchWorkspace { .. }
            | ClientMessage::ListSessions { .. }
            | ClientMessage::KillSession { .. }
            | ClientMessage::SwitchSession { .. } => {
                self.handle_session(msg, client_id, &session_name, &mut responses);
            }

            // IPC commands (from __control__ clients)
            ClientMessage::SendKeys { .. }
            | ClientMessage::GetSessionInfo { .. }
            | ClientMessage::ListPanes { .. }
            | ClientMessage::FocusPaneById { .. }
            | ClientMessage::ClosePaneById { .. }
            | ClientMessage::CreatePaneIn { .. }
            | ClientMessage::RunCommand { .. }
            | ClientMessage::GetLayout { .. } => {
                self.handle_ipc(msg, client_id, &mut responses);
            }

            // Templates
            ClientMessage::ListTemplates
            | ClientMessage::SaveTemplate { .. }
            | ClientMessage::ApplyTemplate { .. } => {
                self.handle_template(msg, client_id, &session_name, &mut responses);
            }

            // Lifecycle (trivial, inline)
            ClientMessage::Attach => {}
            ClientMessage::Detach => {
                responses.push(ServerResponse::RemoveClient(client_id));
            }
            ClientMessage::Ack { generation } => {
                if let Some(client) = self.clients.get_mut(&client_id) {
                    client.last_acked_generation = generation;
                }
            }
            ClientMessage::KillServer => {
                responses.push(ServerResponse::ShutdownServer);
            }
            ClientMessage::Ping {
                seq,
                client_time_us,
            } => {
                responses.push(ServerResponse::SendToClient(
                    client_id,
                    ServerMessage::Pong {
                        seq,
                        client_time_us,
                    },
                ));
            }
        }

        responses
    }

    // ── Tray integration ────────────────────────────────────────────

    /// Find the most recently active non-control client.
    pub(crate) fn most_recent_client(&self) -> Option<u64> {
        self.clients
            .values()
            .filter(|c| c.session_name != CONTROL_SESSION)
            .max_by_key(|c| c.id) // Higher IDs are more recently connected
            .map(|c| c.id)
    }

    /// Switch a client to a different session and dispatch all responses
    /// (SessionSwitched, StateSync, FullPaneSync) immediately.
    /// Used by the tray to switch sessions without going through IPC.
    pub(crate) fn tray_switch_client_to_session(&mut self, client_id: u64, target_session: &str) {
        if ciri_session::names::validate_name(target_session).is_err() {
            log::warn!("tray: invalid session name: {target_session:?}");
            return;
        }

        let old_session = match self.clients.get(&client_id) {
            Some(c) => c.session_name.clone(),
            None => return,
        };

        // Already on the target session — no-op.
        if old_session == target_session {
            return;
        }

        self.switch_client_session_affinity(client_id, target_session);
        self.get_or_create_session(target_session);
        self.refresh_session_attach_time(target_session);

        // Build responses and apply them inline.
        let mut responses = Vec::new();
        responses.push(ServerResponse::SendToClient(
            client_id,
            ServerMessage::SessionSwitched {
                session_name: target_session.to_string(),
            },
        ));
        self.prepare_full_sync_for_client(client_id, target_session, &mut responses);

        self.apply_responses(responses);

        // Resize the old session's panes now that one client moved away.
        self.with_session(&old_session, |old_session, clients| {
            old_session.resize_all_panes(clients);
        });
    }

    /// Generate a unique new session name that doesn't collide with existing ones.
    pub(crate) fn generate_session_name(&self) -> String {
        let existing: Vec<String> = self.sessions.keys().cloned().collect();
        ciri_session::names::unique_name(&existing)
    }

    /// Apply a list of ServerResponses by dispatching messages to clients.
    fn apply_responses(&mut self, responses: Vec<ServerResponse>) {
        for resp in responses {
            match resp {
                ServerResponse::BroadcastToSession(session_name, msg) => {
                    self.broadcast_to_session(&session_name, &msg);
                }
                ServerResponse::SendToClient(cid, msg) => {
                    self.send_to_client(cid, &msg);
                }
                ServerResponse::SendFullPaneSync(cid, sync) => {
                    if let (Some(client), Some(frame)) = (
                        self.clients.get(&cid),
                        ciri_protocol::codec::frame_full_pane_sync(&sync),
                    ) {
                        let _ = client.tx.try_send(bytes::Bytes::from(frame));
                    }
                }
                ServerResponse::RemoveClient(cid) => {
                    self.clients.remove(&cid);
                }
                ServerResponse::ShutdownServer => {}
            }
        }
    }
}

#[cfg(test)]
mod tests;
