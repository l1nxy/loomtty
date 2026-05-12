use ciri_protocol::message::*;
use ciri_protocol::transport;

use super::{ResizeMessage, Server, ServerResponse};

impl Server {
    pub(super) fn handle_session(
        &mut self,
        msg: ClientMessage,
        client_id: u64,
        session_name: &str,
        responses: &mut Vec<ServerResponse>,
    ) {
        match msg {
            ClientMessage::Resize {
                cols: _,
                rows: _,
                width,
                height,
                cell_width,
                cell_height,
            } => {
                let resize = ResizeMessage {
                    width,
                    height,
                    cell_width,
                    cell_height,
                };
                log::debug!(
                    "client {client_id} Resize: {width}x{height}px, cell={cell_width:.1}x{cell_height:.1}"
                );
                if resize.cell_width.is_finite()
                    && resize.cell_width > 0.0
                    && resize.cell_width <= 200.0
                    && resize.cell_height.is_finite()
                    && resize.cell_height > 0.0
                    && resize.cell_height <= 200.0
                    && resize.width > 0
                    && resize.width <= 16384
                    && resize.height > 0
                    && resize.height <= 16384
                {
                    self.apply_resize_for_client(session_name, client_id, &resize, responses);
                } else {
                    log::warn!(
                        "ignoring invalid resize from client {client_id}: {width}x{height} cell={cell_width}x{cell_height}"
                    );
                }
            }
            ClientMessage::SwitchWorkspace { workspace_idx } => {
                if let Some(session) = self.sessions.get_mut(session_name) {
                    if session.workspaces.switch_to(workspace_idx) {
                        Self::layout_changed(
                            session,
                            &mut self.clients,
                            session_name,
                            false,
                            responses,
                        );
                    } else {
                        log::debug!(
                            "ignoring out-of-range workspace switch: client {client_id}, session {session_name}, idx {workspace_idx}, count {}",
                            session.workspaces.workspace_count()
                        );
                    }
                }
            }
            ClientMessage::ListSessions { all } => {
                self.handle_list_sessions(client_id, all, responses);
            }
            ClientMessage::KillSession {
                session_name: target,
            } => {
                self.handle_kill_session(client_id, &target, responses);
            }
            ClientMessage::SwitchSession {
                session_name: target,
            } => {
                self.handle_switch_session(client_id, session_name, &target, responses);
            }
            _ => {}
        }
    }

    fn handle_list_sessions(&self, client_id: u64, all: bool, responses: &mut Vec<ServerResponse>) {
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
                SessionInfo {
                    name: name.clone(),
                    running: true,
                    pane_count: sess.panes.len(),
                    client_count,
                }
            })
            .collect();

        // Sort: requester's current session first, then by last_attached descending.
        let requester_session = self
            .clients
            .get(&client_id)
            .map(|c| c.session_name.clone())
            .unwrap_or_default();
        {
            let mut with_ts: Vec<_> = self
                .sessions
                .iter()
                .map(|(name, sess)| (name.clone(), sess.last_attached))
                .collect();
            with_ts.sort_by(|a, b| b.1.cmp(&a.1));
            let order: std::collections::HashMap<String, usize> = with_ts
                .iter()
                .enumerate()
                .map(|(i, (n, _))| (n.clone(), i))
                .collect();
            sessions.sort_by_key(|s| {
                if s.name == requester_session {
                    (0, 0)
                } else {
                    (1, order.get(&s.name).copied().unwrap_or(usize::MAX))
                }
            });
        }

        if all {
            let running_names: std::collections::HashSet<String> =
                sessions.iter().map(|s| s.name.clone()).collect();
            if let Ok(saved_names) = ciri_session::restore::list_sessions(&transport::state_dir()) {
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

    /// Find the most-recently-attached session other than `exclude`, if any.
    /// Used to pick a fallback when killing a session that has attached clients.
    fn most_recently_attached_session(&self, exclude: &str) -> Option<String> {
        self.sessions
            .iter()
            .filter(|(name, _)| name.as_str() != exclude && !name.starts_with("__"))
            .max_by_key(|(_, sess)| sess.last_attached)
            .map(|(name, _)| name.clone())
    }

    fn handle_kill_session(
        &mut self,
        client_id: u64,
        target: &str,
        responses: &mut Vec<ServerResponse>,
    ) {
        let existed = self.sessions.remove(target).is_some();
        let _ = ciri_session::restore::delete_session(target, &transport::state_dir());
        if existed {
            self.finalize_session_removal(target, responses);
        }
        // Notify the requester (idempotent if they were also attached).
        responses.push(ServerResponse::SendToClient(
            client_id,
            ServerMessage::SessionKilled {
                session_name: target.to_string(),
            },
        ));
    }

    /// Post-removal fan-out: auto-switch clients attached to the just-removed
    /// session to a fallback, or notify them it's gone. Caller must have
    /// already removed `target` from `self.sessions` and deleted its on-disk
    /// state. Used by explicit `handle_kill_session` and by the tick loop
    /// when a session dies naturally (last pane exited).
    pub(crate) fn finalize_session_removal(
        &mut self,
        target: &str,
        responses: &mut Vec<ServerResponse>,
    ) {
        // Clients attached to the killed session need to go somewhere.
        // Pick the most-recently-attached other session (matches tmux/zellij:
        // detach but don't kill the client). If none exists, send
        // SessionKilled and let the client decide (it may switch to a
        // different connection slot or detach gracefully).
        let fallback_session = self.most_recently_attached_session(target);
        let affected_clients: Vec<u64> = self
            .clients
            .iter()
            .filter(|(_, c)| c.session_name == target)
            .map(|(id, _)| *id)
            .collect();
        for cid in affected_clients {
            if let Some(ref fb) = fallback_session {
                // Auto-switch this client to the fallback session.
                self.switch_client_session_affinity(cid, fb);
                self.refresh_session_attach_time(fb);
                responses.push(ServerResponse::SendToClient(
                    cid,
                    ServerMessage::SessionSwitched {
                        session_name: fb.clone(),
                    },
                ));
                self.prepare_full_sync_for_client(cid, fb, responses);
            } else {
                // No other sessions on this server — notify client; it can
                // try another connection slot or just detach.
                responses.push(ServerResponse::SendToClient(
                    cid,
                    ServerMessage::SessionKilled {
                        session_name: target.to_string(),
                    },
                ));
            }
        }
    }

    fn handle_switch_session(
        &mut self,
        client_id: u64,
        old_session_name: &str,
        target: &str,
        responses: &mut Vec<ServerResponse>,
    ) {
        if ciri_session::names::validate_name(target).is_err() {
            responses.push(ServerResponse::SendToClient(
                client_id,
                ServerMessage::Error {
                    message: format!("invalid session name: {target:?}"),
                },
            ));
            return;
        }

        self.switch_client_session_affinity(client_id, target);
        self.get_or_create_session(target);
        self.refresh_session_attach_time(target);

        responses.push(ServerResponse::SendToClient(
            client_id,
            ServerMessage::SessionSwitched {
                session_name: target.to_string(),
            },
        ));
        self.prepare_full_sync_for_client(client_id, target, responses);

        if old_session_name != target {
            self.with_session(old_session_name, |old_session, clients| {
                old_session.resize_all_panes(clients);
            });
        }
    }
}
