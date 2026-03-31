use ciri_protocol::message::ClientMessage;

use super::{App, PaletteEntryKind};
use crate::connection::RemoteQueryResult;

impl App {
    /// Delegate: open command palette.
    pub fn open_command_palette(&mut self) {
        self.core.open_command_palette();
    }

    /// Delegate: open session palette.
    pub fn open_session_palette(&mut self) {
        self.core.open_session_palette();
    }

    /// Delegate: refresh session palette.
    pub fn refresh_session_palette(&mut self) {
        self.core.refresh_session_palette();
    }

    /// Execute a palette entry — some entries require shell access (clipboard, window, connection).
    pub(crate) fn execute_palette_entry(&mut self, entry_idx: usize) {
        let Some(palette) = &self.core.command_palette else {
            return;
        };
        let Some(entry) = palette.entries.get(entry_idx) else {
            return;
        };
        match entry.kind.clone() {
            PaletteEntryKind::Action(action) => {
                self.handle_action(action);
            }
            PaletteEntryKind::SwitchSession(name) => {
                self.send(ClientMessage::SwitchSession { session_name: name });
            }
            PaletteEntryKind::KillSession(name) => {
                self.send(ClientMessage::KillSession { session_name: name });
            }
            PaletteEntryKind::RemoteHost {
                name,
                host,
                port,
                ssh_port,
            } => {
                // Enter loading state and fire async query
                if let Some(palette) = &mut self.core.command_palette {
                    palette.remote_loading = Some(name.clone());
                    palette.remote_error = None;
                }
                let (tx, rx) = crossbeam_channel::bounded(1);
                crate::connection::query_remote_sessions(&name, &host, port, ssh_port, tx);
                self.core.remote_query_rx = Some(rx);
            }
            PaletteEntryKind::RemoteSession {
                host,
                port,
                ssh_port,
                session_name,
            } => {
                self.connect_remote_session(host, port, ssh_port, session_name);
            }
            PaletteEntryKind::SshShell {
                name: _,
                host,
                ssh_port,
            } => {
                let command = if ssh_port != 22 {
                    format!("ssh -p {} {}", ssh_port, host)
                } else {
                    format!("ssh {}", host)
                };
                self.send(ClientMessage::RunCommand {
                    session_name: self.core.session_name.clone(),
                    command,
                    cwd: None,
                });
            }
            PaletteEntryKind::SwitchSlot(id) => {
                self.switch_to_slot(&id);
            }
        }
    }

    /// Delegate: handle remote query result.
    pub fn handle_remote_query_result(&mut self, result: RemoteQueryResult) {
        self.core.handle_remote_query_result(result);
    }

    /// Delegate: filter palette entries.
    pub fn filter_palette(&mut self) {
        self.core.filter_palette();
    }

    /// Delegate: palette scroll offset.
    pub(crate) fn command_palette_scroll_offset(&self, visible_rows: usize) -> usize {
        self.core.command_palette_scroll_offset(visible_rows)
    }
}
