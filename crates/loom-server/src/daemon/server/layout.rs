use loom_layout::column::ColumnWidth;
use loom_protocol::message::*;

use super::{Server, ServerResponse};

impl Server {
    pub(super) fn handle_layout(
        &mut self,
        msg: ClientMessage,
        client_id: u64,
        session_name: &str,
        responses: &mut Vec<ServerResponse>,
    ) {
        match msg {
            ClientMessage::CreatePane => {
                if let Some(mut session) = self.sessions.remove(session_name) {
                    match session.create_pane(&mut self.next_pane_id, &mut self.clients) {
                        Ok(id) => Self::create_pane_and_sync_layout(
                            &mut session,
                            &mut self.clients,
                            session_name,
                            id,
                            responses,
                        ),
                        Err(e) => log::error!("failed to create pane: {e}"),
                    }
                    self.sessions.insert(session_name.to_string(), session);
                }
            }
            ClientMessage::SplitDown => {
                if let Some(mut session) = self.sessions.remove(session_name) {
                    match session
                        .create_pane_in_new_workspace(&mut self.next_pane_id, &mut self.clients)
                    {
                        Ok(id) => Self::create_pane_and_sync_layout(
                            &mut session,
                            &mut self.clients,
                            session_name,
                            id,
                            responses,
                        ),
                        Err(e) => log::error!("failed to split: {e}"),
                    }
                    self.sessions.insert(session_name.to_string(), session);
                }
            }
            ClientMessage::NewTileBelow => {
                if let Some(mut session) = self.sessions.remove(session_name) {
                    match session
                        .create_tile_in_active_column(&mut self.next_pane_id, &mut self.clients)
                    {
                        Ok(id) => Self::create_pane_and_sync_layout(
                            &mut session,
                            &mut self.clients,
                            session_name,
                            id,
                            responses,
                        ),
                        Err(e) => log::error!("failed to create stacked tile: {e}"),
                    }
                    self.sessions.insert(session_name.to_string(), session);
                }
            }
            ClientMessage::ClosePane { pane_id } => {
                self.with_session(session_name, |session, clients| {
                    Self::close_pane_and_sync_layout(
                        session,
                        clients,
                        session_name,
                        pane_id,
                        responses,
                    );
                });
            }
            ClientMessage::FocusLeft => {
                self.handle_focus_direction(
                    session_name,
                    client_id,
                    responses,
                    |ws| ws.focus_left(),
                    BounceDirection::Left,
                );
            }
            ClientMessage::FocusRight => {
                self.handle_focus_direction(
                    session_name,
                    client_id,
                    responses,
                    |ws| ws.focus_right(),
                    BounceDirection::Right,
                );
            }
            ClientMessage::FocusUp => {
                if let Some(session) = self.sessions.get_mut(session_name) {
                    let changed = session.workspaces.active_mut().focus_tile_up()
                        || session.workspaces.focus_up();
                    if changed {
                        Self::layout_changed(
                            session,
                            &mut self.clients,
                            session_name,
                            false,
                            responses,
                        );
                    } else {
                        responses.push(ServerResponse::SendToClient(
                            client_id,
                            ServerMessage::BounceEdge {
                                direction: BounceDirection::Up,
                            },
                        ));
                    }
                }
            }
            ClientMessage::FocusDown => {
                if let Some(session) = self.sessions.get_mut(session_name) {
                    let changed = session.workspaces.active_mut().focus_tile_down()
                        || session.workspaces.focus_down();
                    if changed {
                        Self::layout_changed(
                            session,
                            &mut self.clients,
                            session_name,
                            false,
                            responses,
                        );
                    } else {
                        responses.push(ServerResponse::SendToClient(
                            client_id,
                            ServerMessage::BounceEdge {
                                direction: BounceDirection::Down,
                            },
                        ));
                    }
                }
            }
            ClientMessage::MovePaneLeft => {
                if let Some(session) = self.sessions.get_mut(session_name) {
                    session.workspaces.active_mut().move_pane_left();
                    Self::layout_changed(
                        session,
                        &mut self.clients,
                        session_name,
                        false,
                        responses,
                    );
                }
            }
            ClientMessage::MovePaneRight => {
                if let Some(session) = self.sessions.get_mut(session_name) {
                    session.workspaces.active_mut().move_pane_right();
                    Self::layout_changed(
                        session,
                        &mut self.clients,
                        session_name,
                        false,
                        responses,
                    );
                }
            }
            ClientMessage::MovePaneUp => {
                if let Some(session) = self.sessions.get_mut(session_name) {
                    if session.workspaces.move_pane_up() {
                        Self::layout_changed(
                            session,
                            &mut self.clients,
                            session_name,
                            true,
                            responses,
                        );
                    } else {
                        responses.push(ServerResponse::SendToClient(
                            client_id,
                            ServerMessage::BounceEdge {
                                direction: BounceDirection::Up,
                            },
                        ));
                    }
                }
            }
            ClientMessage::MovePaneDown => {
                if let Some(session) = self.sessions.get_mut(session_name) {
                    if session.workspaces.move_pane_down() {
                        Self::layout_changed(
                            session,
                            &mut self.clients,
                            session_name,
                            true,
                            responses,
                        );
                    } else {
                        responses.push(ServerResponse::SendToClient(
                            client_id,
                            ServerMessage::BounceEdge {
                                direction: BounceDirection::Down,
                            },
                        ));
                    }
                }
            }
            ClientMessage::FocusPane { pane_id } => {
                if let Some(session) = self.sessions.get_mut(session_name) {
                    if session.focus_pane(pane_id) {
                        Self::mark_layout_dirty(session, session_name, responses);
                    } else {
                        log::debug!(
                            "focus_pane: pane {} not found in session '{}'",
                            pane_id,
                            session_name
                        );
                    }
                }
            }
            ClientMessage::SetColumnWidth {
                proportion,
                fixed_px,
            } => {
                self.with_session(session_name, |session, clients| {
                    let width = match fixed_px {
                        Some(px) => ColumnWidth::Fixed(px),
                        None => ColumnWidth::Proportion(proportion),
                    };
                    session
                        .workspaces
                        .active_mut()
                        .set_active_column_width(width);
                    Self::layout_changed(session, clients, session_name, true, responses);
                });
            }
            ClientMessage::AdjustColumnSplit { delta } => {
                self.with_session(session_name, |session, clients| {
                    session
                        .workspaces
                        .active_mut()
                        .resize_active_with_neighbor(delta);
                    Self::layout_changed(session, clients, session_name, true, responses);
                });
            }
            ClientMessage::EqualizeColumnSplit => {
                self.with_session(session_name, |session, clients| {
                    session
                        .workspaces
                        .active_mut()
                        .equalize_active_with_neighbor();
                    Self::layout_changed(session, clients, session_name, true, responses);
                });
            }
            ClientMessage::SetTileWeights {
                column_idx,
                top_tile_idx,
                top_weight,
                bottom_weight,
            } => {
                self.with_session(session_name, |session, clients| {
                    if let Some(col) = session.workspaces.active_mut().columns.get_mut(column_idx) {
                        col.set_tile_weights(top_tile_idx, top_weight, bottom_weight);
                    }
                    Self::layout_changed(session, clients, session_name, true, responses);
                });
            }
            ClientMessage::AdjustColumnSplitAt { column_idx, delta } => {
                self.with_session(session_name, |session, clients| {
                    let ws = session.workspaces.active_mut();
                    let right_idx = column_idx + 1;
                    if right_idx < ws.columns.len() {
                        ws.resize_column_pair(column_idx, right_idx, delta);
                    }
                    Self::layout_changed(session, clients, session_name, true, responses);
                });
            }
            ClientMessage::ConsumeIntoColumn => {
                self.with_session(session_name, |session, clients| {
                    session.workspaces.active_mut().consume_from_right();
                    Self::layout_changed(session, clients, session_name, true, responses);
                });
            }
            ClientMessage::ExpelFromColumn => {
                self.with_session(session_name, |session, clients| {
                    session.workspaces.active_mut().expel_active_tile();
                    Self::layout_changed(session, clients, session_name, true, responses);
                });
            }
            _ => {}
        }
    }

    fn handle_focus_direction(
        &mut self,
        session_name: &str,
        client_id: u64,
        responses: &mut Vec<ServerResponse>,
        focus_fn: impl FnOnce(&mut loom_layout::workspace::Workspace) -> bool,
        bounce: BounceDirection,
    ) {
        if let Some(session) = self.sessions.get_mut(session_name) {
            if focus_fn(session.workspaces.active_mut()) {
                Self::layout_changed(session, &mut self.clients, session_name, false, responses);
            } else {
                responses.push(ServerResponse::SendToClient(
                    client_id,
                    ServerMessage::BounceEdge { direction: bounce },
                ));
            }
        }
    }
}
