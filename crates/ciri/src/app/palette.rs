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

    /// Execute a palette entry — some entries require shell access (clipboard, window, connection).
    pub(crate) fn execute_palette_entry(&mut self, entry_idx: usize) {
        let Some(palette) = &self.core.command_palette else {
            return;
        };
        let Some(entry) = palette.entries.get(entry_idx) else {
            return;
        };
        match entry.kind.clone() {
            PaletteEntryKind::SectionHeader(_) => {
                // Non-selectable — should never be reached
            }
            PaletteEntryKind::Action(action) => {
                self.handle_action(action);
            }
            PaletteEntryKind::GoToSession {
                slot_id,
                session_name,
            } => {
                let is_current_slot = slot_id == self.core.active_slot_id;
                if !is_current_slot {
                    self.switch_to_slot(&slot_id);
                }
                if self.core.session_name != session_name {
                    self.send(ClientMessage::SwitchSession { session_name });
                }
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
                // The command is sent as a shell string that the server will
                // eventually feed to `sh -c`. A host that passed validation
                // is drawn from [A-Za-z0-9._-] (or a bracketed IPv6 literal),
                // none of which are shell-active — but we re-check here so a
                // poisoned config entry can't reach the shell.
                if let Err(e) = crate::remote_validate::validate_fields(
                    &host,
                    ciri_protocol::transport::DEFAULT_REMOTE_PORT,
                    ssh_port,
                ) {
                    log::warn!("refusing SSH shell to {host:?}: {e}");
                    if let Some(palette) = &mut self.core.command_palette {
                        palette.remote_error = Some(("SSH".to_string(), e.to_string()));
                    }
                    return;
                }
                let command = if ssh_port != 22 {
                    format!("ssh -p {} -- {}", ssh_port, host)
                } else {
                    format!("ssh -- {}", host)
                };
                self.send(ClientMessage::RunCommand {
                    session_name: self.core.session_name.clone(),
                    command,
                    cwd: None,
                });
            }
            PaletteEntryKind::DirectConnect {
                name: _,
                host,
                port,
                ssh_port,
            } => {
                self.connect_remote_session(host, port, ssh_port, "default".to_string());
            }
            PaletteEntryKind::ConnectRemotePrompt => {
                // Switch palette to remote input mode
                if let Some(palette) = &mut self.core.command_palette {
                    palette.remote_input_mode = true;
                    palette.query.clear();
                    palette.entries.clear();
                    palette.filtered.clear();
                    palette.selected_idx = 0;
                }
            }
        }
    }

    /// Delegate: handle remote query result.
    pub fn handle_remote_query_result(&mut self, result: RemoteQueryResult) {
        self.core.handle_remote_query_result(result);
    }

    /// Poll background slot `server_rx` channels for `SessionList` responses.
    /// Non-SessionList events are buffered into `slot.pending_events` for replay.
    /// Returns `true` if any results were processed (palette needs redraw).
    pub fn poll_slot_sessions(&mut self) -> bool {
        use crate::connection::ServerEvent;
        use ciri_protocol::message::{ServerMessage, SessionInfo};

        // Collect results first to avoid borrow conflicts
        let mut results: Vec<(String, Vec<SessionInfo>)> = Vec::new();
        let mut dead_slots: Vec<String> = Vec::new();
        let pending: Vec<String> = self.core.slot_session_pending.iter().cloned().collect();

        for slot_id in &pending {
            let Some(slot) = self.core.background_slots.get_mut(slot_id) else {
                // Slot was removed (e.g. via switch_to_slot) — stop tracking it
                dead_slots.push(slot_id.clone());
                continue;
            };

            // Drain available events from this slot's channel
            loop {
                match slot.server_rx.try_recv() {
                    Ok(ServerEvent::Control(ServerMessage::SessionList { sessions })) => {
                        results.push((slot_id.clone(), sessions));
                        break;
                    }
                    Ok(event) => {
                        // Buffer non-SessionList events for replay on restore
                        slot.pending_events.push_back(event);
                    }
                    Err(crossbeam_channel::TryRecvError::Empty) => break,
                    Err(crossbeam_channel::TryRecvError::Disconnected) => {
                        // Connection is dead — stop waiting for a response
                        log::debug!("slot {slot_id} disconnected during session query");
                        dead_slots.push(slot_id.clone());
                        break;
                    }
                }
            }
        }

        let changed = !results.is_empty() || !dead_slots.is_empty();

        // Clean up dead/removed slots and their cached session data
        for slot_id in dead_slots {
            self.core.slot_session_pending.remove(&slot_id);
            self.core.cached_slot_sessions.remove(&slot_id);
        }

        // Apply collected results
        for (slot_id, sessions) in results {
            self.core.slot_session_pending.remove(&slot_id);
            self.core.apply_slot_session_result(&slot_id, sessions);
        }
        changed
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
