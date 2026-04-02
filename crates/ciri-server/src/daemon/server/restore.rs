use ciri_layout::column::ColumnWidth;
use ciri_protocol::transport;
use ciri_term::pane::Pane;
use std::collections::HashMap;

use super::super::client::ClientState;
use super::super::session::Session;
use super::Server;

impl Server {
    /// Get or create a session by name, restoring from saved state if available.
    pub(crate) fn get_or_create_session(&mut self, session_name: &str) -> &mut Session {
        if !self.sessions.contains_key(session_name) {
            self.had_session = true;
            let mut session = Session::new(
                session_name,
                &self.default_shell,
                self.column_gap,
                self.terminal_colors.clone(),
            );
            session.default_column_width = self.default_column_width;
            session.pane_inset = self.pane_inset;

            if let Some(saved) = Self::load_saved_session(session_name) {
                self.apply_restored_session(&mut session, &saved);
            }

            // If restore produced no panes, create a default one
            if session.panes.is_empty() {
                Self::init_default_workspace(
                    &mut session,
                    &mut self.next_pane_id,
                    &mut self.clients,
                );
            }

            self.sessions.insert(session_name.to_string(), session);
        }
        self.sessions.get_mut(session_name).unwrap()
    }

    fn load_saved_session(session_name: &str) -> Option<ciri_session::state::SessionState> {
        ciri_session::restore::restore_session(session_name, &transport::state_dir())
            .ok()
            .flatten()
    }

    fn apply_restored_session(
        &mut self,
        session: &mut Session,
        saved: &ciri_session::state::SessionState,
    ) {
        log::info!("restoring session ({} workspaces)", saved.workspaces.len());
        session.workspaces.workspaces.clear();
        for saved_ws in &saved.workspaces {
            let ws = self.restore_workspace(session, saved_ws);
            session.workspaces.workspaces.push(ws);
        }
        session.workspaces.active_workspace_idx = saved
            .active_workspace_idx
            .min(session.workspaces.workspaces.len().saturating_sub(1));
    }

    fn restore_workspace(
        &mut self,
        session: &mut Session,
        saved_ws: &ciri_session::state::SavedWorkspace,
    ) -> ciri_layout::workspace::Workspace {
        let mut ws = ciri_layout::workspace::Workspace::new_with_gap(
            session.workspaces.view_size,
            session.workspaces.column_gap,
        );
        for saved_col in &saved_ws.columns {
            if saved_col.tiles.is_empty() {
                continue;
            }
            if let Some(col) = self.restore_column(session, saved_col) {
                ws.columns.push(col);
            }
        }
        ws.active_column_idx = saved_ws
            .active_column_idx
            .min(ws.columns.len().saturating_sub(1));
        ws
    }

    fn restore_column(
        &mut self,
        session: &mut Session,
        saved_col: &ciri_session::state::SavedColumn,
    ) -> Option<ciri_layout::column::Column> {
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

        let mut col_opt: Option<ciri_layout::column::Column> = None;
        for saved_tile in &saved_col.tiles {
            let id = self.next_pane_id;
            self.next_pane_id += 1;
            let tile_h = vh / saved_col.tiles.len() as f32;
            let (cols, rows) = session.pane_grid_size_with_cells(col_w, tile_h, 8.0, 16.0);

            let resume_cmd = saved_tile
                .agent
                .as_ref()
                .filter(|_| self.session_config.restore_agents)
                .and_then(|a| {
                    let cmd = ciri_session::agent::resume_command(a.kind)?;
                    if cfg!(windows) {
                        Some(format!("{cmd} || cmd.exe"))
                    } else {
                        Some(format!("{cmd} || exec $SHELL"))
                    }
                });
            let cwd = saved_tile.cwd.as_deref().map(std::path::Path::new);
            match Pane::new_with_opts(
                id,
                cols,
                rows,
                &session.default_shell,
                resume_cmd.as_deref(),
                cwd,
            ) {
                Ok(mut pane) => {
                    pane.set_cell_size(8.0, 16.0);
                    pane.init_colors(&session.terminal_colors);
                    session.panes.insert(id, pane);
                    session.generation.insert(id, 0);
                    Self::push_tile_to_column(
                        &mut col_opt,
                        id,
                        restored_width,
                        saved_tile.weight as f64,
                    );
                }
                Err(e) => log::error!("failed to restore pane: {e}"),
            }
        }
        if let Some(mut col) = col_opt {
            col.active_tile_idx = saved_col
                .active_tile_idx
                .min(col.tiles.len().saturating_sub(1));
            Some(col)
        } else {
            None
        }
    }

    pub(super) fn push_tile_to_column(
        col_opt: &mut Option<ciri_layout::column::Column>,
        pane_id: u64,
        width: ColumnWidth,
        weight: f64,
    ) {
        if let Some(col) = col_opt {
            let mut tile = ciri_layout::tile::Tile::new(pane_id);
            tile.height = ciri_layout::tile::TileHeight::Auto { weight };
            col.tiles.push(tile);
        } else {
            let mut col = ciri_layout::column::Column::new(pane_id);
            col.width = width;
            if let Some(first_tile) = col.tiles.first_mut() {
                first_tile.height = ciri_layout::tile::TileHeight::Auto { weight };
            }
            *col_opt = Some(col);
        }
    }

    fn init_default_workspace(
        session: &mut Session,
        next_pane_id: &mut u64,
        clients: &mut HashMap<u64, ClientState>,
    ) {
        let vs = session.workspaces.view_size;
        let cg = session.workspaces.column_gap;
        session.workspaces.workspaces.clear();
        session
            .workspaces
            .workspaces
            .push(ciri_layout::workspace::Workspace::new_with_gap(vs, cg));
        if let Err(e) = session.create_pane(next_pane_id, clients) {
            log::error!("failed to create initial pane: {e}");
        }
    }
}
