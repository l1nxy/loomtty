use ciri_input::action::Action;
use ciri_protocol::message::ClientMessage;

use super::{
    AppModel, CommandPaletteState, ConnectionKind, PaletteEntry, PaletteEntryKind,
    RemoteProbeResult, RemoteQueryResult,
};

impl AppModel {
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
                ConnectionKind::Local => {
                    format!("Switch to: local ({})", slot.session_name)
                }
                ConnectionKind::Remote { host, .. } => {
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
        self.command_palette = Some(CommandPaletteState {
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
        self.command_palette = Some(CommandPaletteState {
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

    pub fn command_palette_scroll_offset(&self, visible_rows: usize) -> usize {
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
