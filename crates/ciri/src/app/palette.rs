use ciri_input::action::Action;
use ciri_protocol::message::ClientMessage;

use super::{App, PaletteEntry, PaletteEntryKind};
use crate::connection::{RemoteProbeResult, RemoteQueryResult};

impl App {
    pub fn open_command_palette(&mut self) {
        let mut entries: Vec<PaletteEntry> = Action::all_with_labels()
            .into_iter()
            .map(|(action, label)| PaletteEntry {
                label: label.to_string(),
                kind: PaletteEntryKind::Action(action),
            })
            .collect();

        // Background connection slots — quick-switch entries
        for (id, slot) in &self.background_slots {
            let label = match &slot.kind {
                super::ConnectionKind::Local => {
                    format!("Switch to: local ({})", slot.session_name)
                }
                super::ConnectionKind::Remote { host, .. } => {
                    format!("Switch to: {} ({})", host, slot.session_name)
                }
            };
            entries.push(PaletteEntry {
                label,
                kind: PaletteEntryKind::SwitchSlot(id.clone()),
            });
        }

        // Configured remote hosts
        for rh in &self.config.remote.hosts {
            entries.push(PaletteEntry {
                label: format!("Remote: {} ({})", rh.name, rh.host),
                kind: PaletteEntryKind::RemoteHost {
                    name: rh.name.clone(),
                    host: rh.host.clone(),
                    port: rh.port,
                    ssh_port: rh.ssh_port,
                },
            });
        }

        let filtered: Vec<usize> = (0..entries.len()).collect();
        self.command_palette = Some(super::CommandPaletteState {
            query: String::new(),
            entries,
            filtered,
            selected_idx: 0,
            hovered_idx: None,
            sessions_only: false,
            sessions_show_all: true,
            remote_loading: None,
            remote_error: None,
        });
        self.send(ClientMessage::ListSessions { all: true });
    }

    pub fn open_session_palette(&mut self) {
        self.command_palette = Some(super::CommandPaletteState {
            query: String::new(),
            entries: Vec::new(),
            filtered: Vec::new(),
            selected_idx: 0,
            hovered_idx: None,
            sessions_only: true,
            sessions_show_all: false,
            remote_loading: None,
            remote_error: None,
        });
        self.send(ClientMessage::ListSessions { all: false });
    }

    pub fn refresh_session_palette(&mut self) {
        if let Some(palette) = &self.command_palette
            && palette.sessions_only
        {
            self.send(ClientMessage::ListSessions {
                all: palette.sessions_show_all,
            });
        }
    }

    /// Returns true if the palette should stay open after this entry.
    pub(crate) fn execute_palette_entry(&mut self, entry_idx: usize) {
        let Some(palette) = &self.command_palette else {
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
                self.send(ClientMessage::SwitchSession {
                    session_name: name,
                });
            }
            PaletteEntryKind::KillSession(name) => {
                self.send(ClientMessage::KillSession {
                    session_name: name,
                });
            }
            PaletteEntryKind::RemoteHost {
                name,
                host,
                port,
                ssh_port,
            } => {
                // Enter loading state and fire async query
                if let Some(palette) = &mut self.command_palette {
                    palette.remote_loading = Some(name.clone());
                    palette.remote_error = None;
                }
                let (tx, rx) = crossbeam_channel::bounded(1);
                crate::connection::query_remote_sessions(&name, &host, port, ssh_port, tx);
                self.remote_query_rx = Some(rx);
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
                    session_name: self.session_name.clone(),
                    command,
                    cwd: None,
                });
            }
            PaletteEntryKind::SwitchSlot(id) => {
                self.switch_to_slot(&id);
            }
        }
    }

    /// Handle the result of an async remote host probe.
    pub fn handle_remote_query_result(&mut self, result: RemoteQueryResult) {
        let Some(palette) = &mut self.command_palette else {
            return;
        };
        palette.remote_loading = None;

        match result.result {
            RemoteProbeResult::Sessions(sessions) => {
                // Remove any previous remote session/ssh entries for this host
                palette.entries.retain(|e| {
                    !matches!(&e.kind,
                        PaletteEntryKind::RemoteSession { host, .. } if *host == result.host)
                        && !matches!(&e.kind,
                        PaletteEntryKind::SshShell { host, .. } if *host == result.host)
                });

                if sessions.is_empty() {
                    // Server exists but no sessions — offer to create one
                    palette.entries.push(PaletteEntry {
                        label: format!("  {} > (new session)", result.host_name),
                        kind: PaletteEntryKind::RemoteSession {
                            host: result.host.clone(),
                            port: result.port,
                            ssh_port: result.ssh_port,
                            session_name: "default".to_string(),
                        },
                    });
                } else {
                    for s in &sessions {
                        palette.entries.push(PaletteEntry {
                            label: format!("  {} > {}", result.host_name, s.name),
                            kind: PaletteEntryKind::RemoteSession {
                                host: result.host.clone(),
                                port: result.port,
                                ssh_port: result.ssh_port,
                                session_name: s.name.clone(),
                            },
                        });
                    }
                }
            }
            RemoteProbeResult::NoServer => {
                // Remove previous entries for this host
                palette.entries.retain(|e| {
                    !matches!(&e.kind,
                        PaletteEntryKind::RemoteSession { host, .. } if *host == result.host)
                        && !matches!(&e.kind,
                        PaletteEntryKind::SshShell { host, .. } if *host == result.host)
                });

                palette.entries.push(PaletteEntry {
                    label: format!("  SSH: {} (no ciri-server)", result.host_name),
                    kind: PaletteEntryKind::SshShell {
                        name: result.host_name.clone(),
                        host: result.host.clone(),
                        ssh_port: result.ssh_port,
                    },
                });
            }
            RemoteProbeResult::Error(e) => {
                palette.remote_error = Some((result.host_name, e));
            }
        }

        self.filter_palette();
    }

    pub fn filter_palette(&mut self) {
        let Some(palette) = &mut self.command_palette else {
            return;
        };
        let needle = palette.query.to_lowercase();
        palette.filtered = palette
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                if needle.is_empty() {
                    true
                } else {
                    fuzzy_match(&e.label.to_lowercase(), &needle)
                }
            })
            .map(|(i, _)| i)
            .collect();
        palette.selected_idx = 0;
        palette.hovered_idx = None;
    }

    pub(crate) fn command_palette_scroll_offset(&self, visible_rows: usize) -> usize {
        self.command_palette
            .as_ref()
            .map(|palette| {
                if palette.selected_idx >= visible_rows {
                    palette.selected_idx - visible_rows + 1
                } else {
                    0
                }
            })
            .unwrap_or(0)
    }
}

fn fuzzy_match(haystack: &str, needle: &str) -> bool {
    let mut it = needle.chars();
    let mut current = it.next();
    for h in haystack.chars() {
        if let Some(n) = current {
            if h == n {
                current = it.next();
            }
        } else {
            return true;
        }
    }
    current.is_none()
}
