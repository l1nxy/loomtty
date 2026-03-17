use anyhow::Result;
use ciri_layout::column::ColumnWidth;
use ciri_layout::geometry::ViewSize;
use ciri_layout::workspace::Workspace;
use ciri_protocol::codec;
use ciri_protocol::message::{ClientMessage, ServerMessage, LayoutState, ColumnState, TileState};
use ciri_protocol::transport;
use ciri_session::save::save_session;
use ciri_session::state::{SavedColumn, SavedTile, SessionState};
use ciri_term::pane::Pane;
use std::collections::HashMap;
use std::io::Write;
use std::os::unix::net::UnixListener;
use std::sync::{Arc, Mutex};

struct ServerState {
    workspace: Workspace,
    panes: HashMap<u64, Pane>,
    next_pane_id: u64,
    session_name: String,
    default_cols: u16,
    default_rows: u16,
}

impl ServerState {
    fn new(session_name: &str) -> Self {
        ServerState {
            workspace: Workspace::new(ViewSize {
                width: 1024.0,
                height: 768.0,
            }),
            panes: HashMap::new(),
            next_pane_id: 1,
            session_name: session_name.to_string(),
            default_cols: 80,
            default_rows: 24,
        }
    }

    fn create_pane(&mut self) -> Result<u64, anyhow::Error> {
        let id = self.next_pane_id;
        self.next_pane_id += 1;
        let pane = Pane::new(id, self.default_cols, self.default_rows, "")?;
        self.panes.insert(id, pane);
        self.workspace.add_column_right(id);
        Ok(id)
    }

    fn create_pane_new_column(&mut self) -> Result<u64, anyhow::Error> {
        let id = self.next_pane_id;
        self.next_pane_id += 1;
        let pane = Pane::new(id, self.default_cols, self.default_rows, "")?;
        self.panes.insert(id, pane);
        self.workspace.add_column_right(id);
        Ok(id)
    }

    fn close_pane(&mut self, pane_id: u64) {
        self.workspace.close_pane(pane_id);
        self.panes.remove(&pane_id);
    }

    fn process_pty_all(&mut self) {
        for pane in self.panes.values_mut() {
            pane.process_pty_output();
        }
    }

    fn layout_state(&self) -> LayoutState {
        LayoutState {
            columns: self.workspace.columns.iter().map(|c| ColumnState {
                tiles: vec![TileState { pane_id: c.pane_id, weight: 1.0 }],
                active_tile_idx: 0,
                width_proportion: match c.width {
                    ColumnWidth::Proportion(p) => p,
                    ColumnWidth::Fixed(px) => px / self.workspace.view_size.width as f64,
                },
            }).collect(),
            active_column_idx: self.workspace.active_column_idx,
            view_offset_x: self.workspace.view_offset_x,
        }
    }

    fn save_session(&self) -> Result<(), anyhow::Error> {
        let state = SessionState {
            name: self.session_name.clone(),
            columns: self.workspace.columns.iter().map(|c| SavedColumn {
                tiles: vec![SavedTile { pane_id: c.pane_id, weight: 1.0, cwd: None, title: None }],
                active_tile_idx: 0,
                width_proportion: match c.width {
                    ColumnWidth::Proportion(p) => p,
                    ColumnWidth::Fixed(px) => px / self.workspace.view_size.width as f64,
                },
            })
                .collect(),
            active_column_idx: self.workspace.active_column_idx,
        };
        save_session(&state, &transport::state_dir())
    }

    fn handle_message(&mut self, msg: ClientMessage) -> Option<ServerMessage> {
        match msg {
            ClientMessage::Input { pane_id, data } => {
                if let Some(pane) = self.panes.get(&pane_id) {
                    pane.write_to_pty(&data);
                }
                None
            }
            ClientMessage::CreatePane => {
                match self.create_pane() {
                    Ok(id) => Some(ServerMessage::PaneCreated {
                        pane_id: id,
                        column_idx: self.workspace.active_column_idx,
                    }),
                    Err(e) => {
                        log::error!("failed to create pane: {e}");
                        None
                    }
                }
            }
            ClientMessage::SplitDown => {
                match self.create_pane_new_column() {
                    Ok(id) => Some(ServerMessage::PaneCreated {
                        pane_id: id,
                        column_idx: self.workspace.active_column_idx,
                    }),
                    Err(e) => {
                        log::error!("failed to split: {e}");
                        None
                    }
                }
            }
            ClientMessage::ClosePane { pane_id } => {
                self.close_pane(pane_id);
                Some(ServerMessage::PaneClosed { pane_id })
            }
            ClientMessage::FocusLeft => {
                self.workspace.focus_left();
                Some(ServerMessage::LayoutUpdate { layout: self.layout_state() })
            }
            ClientMessage::FocusRight => {
                self.workspace.focus_right();
                Some(ServerMessage::LayoutUpdate { layout: self.layout_state() })
            }
            ClientMessage::FocusUp => {
                self.workspace.focus_left();
                Some(ServerMessage::LayoutUpdate { layout: self.layout_state() })
            }
            ClientMessage::FocusDown => {
                self.workspace.focus_right();
                Some(ServerMessage::LayoutUpdate { layout: self.layout_state() })
            }
            ClientMessage::MovePaneLeft => {
                self.workspace.move_pane_left();
                Some(ServerMessage::LayoutUpdate { layout: self.layout_state() })
            }
            ClientMessage::MovePaneRight => {
                self.workspace.move_pane_right();
                Some(ServerMessage::LayoutUpdate { layout: self.layout_state() })
            }
            ClientMessage::Resize { width, height } => {
                self.workspace.resize_view(ViewSize {
                    width: width as f32,
                    height: height as f32,
                });
                Some(ServerMessage::LayoutUpdate { layout: self.layout_state() })
            }
            ClientMessage::SetColumnWidth { proportion } => {
                self.workspace.set_active_column_width(ColumnWidth::Proportion(proportion));
                Some(ServerMessage::LayoutUpdate { layout: self.layout_state() })
            }
            ClientMessage::Attach | ClientMessage::SyncRequest => {
                Some(ServerMessage::StateSync {
                    layout: self.layout_state(),
                    panes: vec![], // Full pane state sync would go here
                })
            }
            ClientMessage::Detach => None,
        }
    }
}

pub async fn run_daemon(session_name: &str) -> Result<()> {
    let sock_path = transport::socket_path(session_name);
    if let Some(parent) = sock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Remove stale socket
    if sock_path.exists() {
        std::fs::remove_file(&sock_path)?;
    }

    let listener = UnixListener::bind(&sock_path)?;
    listener.set_nonblocking(true)?;
    log::info!("ciri-server listening on {}", sock_path.display());

    let state = Arc::new(Mutex::new(ServerState::new(session_name)));

    // Create initial pane
    {
        let mut s = state.lock().unwrap();
        s.create_pane()?;
    }

    // Accept connections
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let state = state.clone();
                std::thread::spawn(move || {
                    log::info!("client connected");
                    loop {
                        let msg: ClientMessage = match codec::decode_from(&mut stream) {
                            Ok(m) => m,
                            Err(_) => {
                                log::info!("client disconnected");
                                break;
                            }
                        };

                        let mut s = state.lock().unwrap();
                        s.process_pty_all();
                        if let Some(response) = s.handle_message(msg)
                            && let Ok(data) = codec::encode(&response)
                                && stream.write_all(&data).is_err() {
                                    break;
                                }
                    }
                });
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // Process PTY output periodically
                let mut s = state.lock().unwrap();
                s.process_pty_all();
                drop(s);
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(e) => {
                log::error!("accept error: {e}");
                break;
            }
        }
    }

    // Cleanup
    let s = state.lock().unwrap();
    let _ = s.save_session();
    drop(s);
    let _ = std::fs::remove_file(&sock_path);

    Ok(())
}
