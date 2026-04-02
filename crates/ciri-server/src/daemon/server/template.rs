use ciri_layout::column::ColumnWidth;
use ciri_protocol::message::*;
use ciri_term::pane::Pane;

use super::super::damage::DamageAccumulator;
use super::super::session::Session;
use super::{Server, ServerResponse};

impl Server {
    pub(super) fn handle_template(
        &mut self,
        msg: ClientMessage,
        client_id: u64,
        session_name: &str,
        responses: &mut Vec<ServerResponse>,
    ) {
        match msg {
            ClientMessage::ListTemplates => {
                self.handle_list_templates(client_id, responses);
            }
            ClientMessage::SaveTemplate {
                template_name,
                session_name: target_session,
            } => {
                let source = if target_session.is_empty() {
                    session_name
                } else {
                    &target_session
                };
                self.handle_save_template(client_id, &template_name, source, responses);
            }
            ClientMessage::ApplyTemplate {
                template_name,
                session_name: target_session,
            } => {
                let target = if target_session.is_empty() {
                    session_name.to_string()
                } else {
                    target_session
                };
                self.handle_apply_template(client_id, &template_name, &target, responses);
            }
            _ => {}
        }
    }

    fn handle_list_templates(&self, client_id: u64, responses: &mut Vec<ServerResponse>) {
        use ciri_session::template;
        let mut templates = Vec::new();
        match template::list_templates() {
            Ok(names) => {
                for name in names {
                    match template::load_template(&name) {
                        Ok(tpl) => {
                            let total_panes: usize = tpl
                                .workspaces
                                .iter()
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
        responses.push(ServerResponse::SendToClient(
            client_id,
            ServerMessage::TemplateList { templates },
        ));
    }

    fn handle_save_template(
        &self,
        client_id: u64,
        template_name: &str,
        source_session: &str,
        responses: &mut Vec<ServerResponse>,
    ) {
        use ciri_session::template::{
            self, LayoutTemplate, TemplateColumn, TemplateTile, TemplateWidth, TemplateWorkspace,
        };

        if let Some(session) = self.sessions.get(source_session) {
            let layout = session.layout_state();
            let tpl = LayoutTemplate {
                description: Some(format!("Saved from session '{}'", source_session)),
                workspaces: layout
                    .workspaces
                    .iter()
                    .map(|ws| TemplateWorkspace {
                        columns: ws
                            .columns
                            .iter()
                            .map(|col| TemplateColumn {
                                tiles: col
                                    .tiles
                                    .iter()
                                    .map(|tile| TemplateTile {
                                        command: String::new(),
                                        cwd: String::new(),
                                        weight: tile.weight as f64,
                                    })
                                    .collect(),
                                width: Some(TemplateWidth::Proportion {
                                    proportion: col.width_proportion,
                                }),
                            })
                            .collect(),
                        active_column: ws.active_column_idx,
                    })
                    .collect(),
            };
            match template::save_template(template_name, &tpl) {
                Ok(()) => {
                    log::info!(
                        "saved template '{}' from session '{}'",
                        template_name,
                        source_session
                    );
                    responses.push(ServerResponse::SendToClient(
                        client_id,
                        ServerMessage::TemplateSaved {
                            template_name: template_name.to_string(),
                        },
                    ));
                }
                Err(e) => {
                    responses.push(ServerResponse::SendToClient(
                        client_id,
                        ServerMessage::Error {
                            message: format!("failed to save template: {e}"),
                        },
                    ));
                }
            }
        } else {
            responses.push(ServerResponse::SendToClient(
                client_id,
                ServerMessage::Error {
                    message: format!("session '{}' not found", source_session),
                },
            ));
        }
    }

    fn handle_apply_template(
        &mut self,
        client_id: u64,
        template_name: &str,
        target: &str,
        responses: &mut Vec<ServerResponse>,
    ) {
        use ciri_session::template::{self, TemplateWidth};

        let tpl = match template::load_template(template_name) {
            Ok(tpl) => tpl,
            Err(e) => {
                responses.push(ServerResponse::SendToClient(
                    client_id,
                    ServerMessage::Error {
                        message: format!("failed to load template '{}': {}", template_name, e),
                    },
                ));
                return;
            }
        };

        // Remove existing session if present (kill all its panes)
        if let Some(old) = self.sessions.remove(target) {
            for client in self.clients.values() {
                if client.session_name == target && client.id != client_id {
                    self.send_to_client(client.id, &ServerMessage::ServerShutdown);
                }
            }
            drop(old);
        }

        // Create new session
        self.had_session = true;
        let mut session = Session::new(
            target,
            &self.default_shell,
            self.column_gap,
            self.terminal_colors.clone(),
        );
        session.default_column_width = self.default_column_width;
        session.pane_inset = self.pane_inset;

        let vw = session.workspaces.view_size.width;
        let vh = session.workspaces.view_size.height;
        let (_viewport_w, _viewport_h, cw, ch) =
            Session::effective_dims_from(&self.clients, target);

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

                    let cmd = if tpl_tile.command.is_empty() {
                        None
                    } else {
                        Some(tpl_tile.command.as_str())
                    };
                    let cwd = if tpl_tile.cwd.is_empty() {
                        None
                    } else {
                        Some(std::path::Path::new(&tpl_tile.cwd))
                    };

                    match Pane::new_with_opts(id, cols, rows, &session.default_shell, cmd, cwd) {
                        Ok(mut pane) => {
                            pane.set_cell_size(cw, ch);
                            pane.init_colors(&session.terminal_colors);
                            session.panes.insert(id, pane);
                            session.generation.insert(id, 0);
                            if let Some(col) = &mut col_opt {
                                let mut tile = ciri_layout::tile::Tile::new(id);
                                tile.height = ciri_layout::tile::TileHeight::Auto {
                                    weight: tpl_tile.weight,
                                };
                                col.tiles.push(tile);
                            } else {
                                let mut col = ciri_layout::column::Column::new(id);
                                col.width = ColumnWidth::Proportion(col_proportion);
                                if let Some(first_tile) = col.tiles.first_mut() {
                                    first_tile.height = ciri_layout::tile::TileHeight::Auto {
                                        weight: tpl_tile.weight,
                                    };
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
            session
                .workspaces
                .workspaces
                .push(ciri_layout::workspace::Workspace::new_with_gap(vs, cg));
            if let Err(e) = session.create_pane(&mut self.next_pane_id, &mut self.clients) {
                log::error!("failed to create fallback pane: {e}");
            }
        }

        session.workspaces.active_workspace_idx = 0;

        // Update client's session affinity
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.session_name = target.to_string();
            client.damage.clear();
            client.history_sent.clear();
        }

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

        let (sync_msg, pane_syncs, image_events) = session.build_state_sync();

        let pane_histories: Vec<(u64, usize)> = session
            .panes
            .iter()
            .map(|(&pid, pane): (&u64, &Pane)| (pid, pane.scrollback_total()))
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
        self.sessions.insert(target.to_string(), session);

        responses.push(ServerResponse::SendToClient(
            client_id,
            ServerMessage::TemplateApplied {
                session_name: target.to_string(),
            },
        ));
        responses.push(ServerResponse::SendToClient(client_id, sync_msg));
        for sync in pane_syncs {
            responses.push(ServerResponse::SendFullPaneSync(client_id, sync));
        }
        for msg in image_events {
            responses.push(ServerResponse::SendToClient(client_id, msg));
        }
    }
}
