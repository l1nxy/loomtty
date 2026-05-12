use ciri_protocol::message::*;

use super::super::session::Session;
use super::{Server, ServerResponse};

impl Server {
    pub(super) fn handle_ipc(
        &mut self,
        msg: ClientMessage,
        client_id: u64,
        responses: &mut Vec<ServerResponse>,
    ) {
        match msg {
            ClientMessage::SendKeys {
                session_name: target,
                pane_id,
                keys,
            } => {
                let Some(session) = self.resolve_session_mut(&target, client_id, responses) else {
                    return;
                };
                if let Some(pane) = session.panes.get_mut(&pane_id) {
                    ciri_term::pane::Pane::write_to_pty(pane, &keys);
                    responses.push(ServerResponse::SendToClient(
                        client_id,
                        ServerMessage::CommandResult {
                            success: true,
                            message: "ok".to_string(),
                            pane_id: Some(pane_id),
                        },
                    ));
                } else {
                    Self::push_error(
                        client_id,
                        format!("pane {} not found in session '{}'", pane_id, target),
                        responses,
                    );
                }
            }
            ClientMessage::GetSessionInfo {
                session_name: target,
            } => {
                let Some(session) = self.resolve_session(&target, client_id, responses) else {
                    return;
                };
                let client_count = self
                    .clients
                    .values()
                    .filter(|c| c.session_name == target)
                    .count();
                let info = SessionDetailInfo {
                    name: target,
                    running: true,
                    pane_count: session.panes.len(),
                    client_count,
                    workspace_count: session.workspaces.workspaces.len(),
                    active_workspace: session.workspaces.active_workspace_idx,
                };
                responses.push(ServerResponse::SendToClient(
                    client_id,
                    ServerMessage::SessionInfoReply { info },
                ));
            }
            ClientMessage::ListPanes {
                session_name: target,
            } => {
                let Some(session) = self.resolve_session(&target, client_id, responses) else {
                    return;
                };
                let panes = Self::collect_pane_details(session);
                responses.push(ServerResponse::SendToClient(
                    client_id,
                    ServerMessage::PaneListReply { panes },
                ));
            }
            ClientMessage::FocusPaneById {
                session_name: target,
                pane_id,
            } => {
                let Some(session) = self.resolve_session_mut(&target, client_id, responses) else {
                    return;
                };
                if session.focus_pane(pane_id) {
                    Self::mark_layout_dirty(session, &target, responses);
                    responses.push(ServerResponse::SendToClient(
                        client_id,
                        ServerMessage::CommandResult {
                            success: true,
                            message: "ok".to_string(),
                            pane_id: Some(pane_id),
                        },
                    ));
                } else {
                    Self::push_error(
                        client_id,
                        format!("pane {} not found in session '{}'", pane_id, target),
                        responses,
                    );
                }
            }
            ClientMessage::ClosePaneById {
                session_name: target,
                pane_id,
            } => {
                if !Self::validate_session_name(&target, client_id, responses) {
                    return;
                }
                if let Some(mut session) = self.sessions.remove(&target) {
                    if session.panes.contains_key(&pane_id) {
                        Self::close_pane_and_sync_layout(
                            &mut session,
                            &mut self.clients,
                            &target,
                            pane_id,
                            responses,
                        );
                        responses.push(ServerResponse::SendToClient(
                            client_id,
                            ServerMessage::CommandResult {
                                success: true,
                                message: "ok".to_string(),
                                pane_id: Some(pane_id),
                            },
                        ));
                    } else {
                        Self::push_error(
                            client_id,
                            format!("pane {} not found in session '{}'", pane_id, target),
                            responses,
                        );
                    }
                    self.sessions.insert(target, session);
                } else {
                    Self::push_error(
                        client_id,
                        format!("session '{}' not found", target),
                        responses,
                    );
                }
            }
            ClientMessage::CreatePaneIn {
                session_name: target,
            } => {
                if !Self::validate_session_name(&target, client_id, responses) {
                    return;
                }
                if let Some(mut session) = self.sessions.remove(&target) {
                    match session.create_pane(&mut self.next_pane_id, &mut self.clients) {
                        Ok(id) => {
                            Self::create_pane_and_sync_layout(
                                &mut session,
                                &mut self.clients,
                                &target,
                                id,
                                responses,
                            );
                            responses.push(ServerResponse::SendToClient(
                                client_id,
                                ServerMessage::CommandResult {
                                    success: true,
                                    message: "ok".to_string(),
                                    pane_id: Some(id),
                                },
                            ));
                        }
                        Err(e) => {
                            Self::push_error(
                                client_id,
                                format!("failed to create pane: {e}"),
                                responses,
                            );
                        }
                    }
                    self.sessions.insert(target, session);
                } else {
                    Self::push_error(
                        client_id,
                        format!("session '{}' not found", target),
                        responses,
                    );
                }
            }
            ClientMessage::RunCommand {
                session_name: target,
                command,
                cwd,
            } => {
                if !Self::validate_session_name(&target, client_id, responses) {
                    return;
                }
                if let Some(mut session) = self.sessions.remove(&target) {
                    let cwd_path = cwd.as_deref().map(std::path::Path::new);
                    match session.create_pane_with_opts(
                        &mut self.next_pane_id,
                        &mut self.clients,
                        Some(&command),
                        cwd_path,
                    ) {
                        Ok(id) => {
                            Self::create_pane_and_sync_layout(
                                &mut session,
                                &mut self.clients,
                                &target,
                                id,
                                responses,
                            );
                            responses.push(ServerResponse::SendToClient(
                                client_id,
                                ServerMessage::CommandResult {
                                    success: true,
                                    message: "ok".to_string(),
                                    pane_id: Some(id),
                                },
                            ));
                        }
                        Err(e) => {
                            Self::push_error(
                                client_id,
                                format!("failed to create pane: {e}"),
                                responses,
                            );
                        }
                    }
                    self.sessions.insert(target, session);
                } else {
                    Self::push_error(
                        client_id,
                        format!("session '{}' not found", target),
                        responses,
                    );
                }
            }
            ClientMessage::GetLayout {
                session_name: target,
            } => {
                let Some(session) = self.resolve_session(&target, client_id, responses) else {
                    return;
                };
                let layout = session.layout_state();
                responses.push(ServerResponse::SendToClient(
                    client_id,
                    ServerMessage::LayoutReply {
                        layout,
                        session_name: target,
                    },
                ));
            }
            ClientMessage::CapturePane {
                session_name: target,
                pane_id,
                opts,
            } => {
                let Some(session) = self.resolve_session(&target, client_id, responses) else {
                    return;
                };
                if let Some(pane) = session.panes.get(&pane_id) {
                    let capture = pane.capture_text(&opts);
                    responses.push(ServerResponse::SendToClient(
                        client_id,
                        ServerMessage::PaneCapture {
                            session_name: target,
                            pane_id,
                            text: capture.text,
                            truncated: capture.truncated,
                        },
                    ));
                } else {
                    Self::push_error(
                        client_id,
                        format!("pane {} not found in session '{}'", pane_id, target),
                        responses,
                    );
                }
            }
            ClientMessage::ListPrompts {
                session_name: target,
                pane_id,
            } => {
                let Some(session) = self.resolve_session(&target, client_id, responses) else {
                    return;
                };
                if let Some(pane) = session.panes.get(&pane_id) {
                    let marks = pane.prompt_marks_for_ipc();
                    responses.push(ServerResponse::SendToClient(
                        client_id,
                        ServerMessage::PromptListReply {
                            session_name: target,
                            pane_id,
                            marks,
                        },
                    ));
                } else {
                    Self::push_error(
                        client_id,
                        format!("pane {} not found in session '{}'", pane_id, target),
                        responses,
                    );
                }
            }
            _ => {}
        }
    }

    // ── Helpers ──────────────────────────────────────────────────────

    fn validate_session_name(
        target: &str,
        client_id: u64,
        responses: &mut Vec<ServerResponse>,
    ) -> bool {
        if ciri_session::names::validate_name(target).is_err() {
            Self::push_error(
                client_id,
                format!("invalid session name: {target:?}"),
                responses,
            );
            false
        } else {
            true
        }
    }

    fn resolve_session(
        &self,
        target: &str,
        client_id: u64,
        responses: &mut Vec<ServerResponse>,
    ) -> Option<&Session> {
        if !Self::validate_session_name(target, client_id, responses) {
            return None;
        }
        match self.sessions.get(target) {
            Some(s) => Some(s),
            None => {
                Self::push_error(
                    client_id,
                    format!("session '{}' not found", target),
                    responses,
                );
                None
            }
        }
    }

    fn resolve_session_mut(
        &mut self,
        target: &str,
        client_id: u64,
        responses: &mut Vec<ServerResponse>,
    ) -> Option<&mut Session> {
        if ciri_session::names::validate_name(target).is_err() {
            Self::push_error(
                client_id,
                format!("invalid session name: {target:?}"),
                responses,
            );
            return None;
        }
        match self.sessions.get_mut(target) {
            Some(s) => Some(s),
            None => {
                Self::push_error(
                    client_id,
                    format!("session '{}' not found", target),
                    responses,
                );
                None
            }
        }
    }

    fn push_error(client_id: u64, message: String, responses: &mut Vec<ServerResponse>) {
        responses.push(ServerResponse::SendToClient(
            client_id,
            ServerMessage::Error { message },
        ));
    }

    fn collect_pane_details(session: &Session) -> Vec<PaneDetailInfo> {
        let mut panes = Vec::new();
        let active_ws = session.workspaces.active_workspace_idx;
        for (ws_idx, ws) in session.workspaces.workspaces.iter().enumerate() {
            let active_col = ws.active_column_idx;
            for (col_idx, col) in ws.columns.iter().enumerate() {
                let active_tile = col.active_tile_idx;
                for (tile_idx, tile) in col.tiles.iter().enumerate() {
                    let is_active =
                        ws_idx == active_ws && col_idx == active_col && tile_idx == active_tile;
                    let (cols, rows) = session
                        .panes
                        .get(&tile.pane_id)
                        .map(|p: &ciri_term::pane::Pane| (p.grid_cols(), p.grid_rows()))
                        .unwrap_or((0, 0));
                    let title = session
                        .panes
                        .get(&tile.pane_id)
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
        panes
    }
}
