use ciri_input::action::Action;
use ciri_protocol::message::ClientMessage;

use super::{
    AppModel, CommandPaletteState, ConnectionKind, ConnectionSlot, PaletteEntry, PaletteEntryKind,
    RecentHost, RemoteProbeResult, RemoteQueryResult,
};

impl AppModel {
    pub fn open_command_palette(&mut self) {
        // Clear stale probe results so hosts are re-probed on selection
        self.cached_remote_probes.clear();
        self.command_palette = Some(CommandPaletteState {
            query: String::new(),
            entries: self.build_palette_entries(false),
            filtered: Vec::new(),
            selected_idx: 0,
            hovered_idx: None,
            sessions_only: false,
            remote_loading: None,
            remote_error: None,
            remote_input_mode: false,
        });
        self.filter_palette();
        self.send(ClientMessage::ListSessions { all: false });
    }

    pub fn open_session_palette(&mut self) {
        self.cached_remote_probes.clear();
        self.command_palette = Some(CommandPaletteState {
            query: String::new(),
            entries: self.build_palette_entries(true),
            filtered: Vec::new(),
            selected_idx: 0,
            hovered_idx: None,
            sessions_only: true,
            remote_loading: None,
            remote_error: None,
            remote_input_mode: false,
        });
        self.filter_palette();
        self.send(ClientMessage::ListSessions { all: false });
        self.refresh_all_slot_session_caches();
    }

    /// Send `ListSessions` to every connected background slot so
    /// `cached_slot_sessions` stays populated. Without this, cycling between
    /// slots cannot see *which* sessions live on the other slot — the cycle
    /// list falls back to a single entry per slot using its last-active
    /// session name, which makes the cycle skip every other session.
    pub fn refresh_all_slot_session_caches(&mut self) {
        self.slot_session_pending.clear();
        self.slot_session_query_start = None;
        for (id, slot) in &self.background_slots {
            if slot.connected
                && slot
                    .server_tx
                    .try_send(ClientMessage::ListSessions { all: false })
                    .is_ok()
            {
                self.slot_session_pending.insert(id.clone());
            }
        }
        if !self.slot_session_pending.is_empty() {
            self.slot_session_query_start = Some(std::time::Instant::now());
        }
    }

    /// Build the full palette entry list from cached data, config, and slots.
    /// Produces entries in a deterministic, grouped order with section headers.
    fn build_palette_entries(&self, sessions_only: bool) -> Vec<PaletteEntry> {
        let mut entries = Vec::new();

        if sessions_only {
            self.build_session_palette_entries(&mut entries);
        } else {
            self.build_command_palette_entries(&mut entries);
        }

        entries
    }

    /// Maximum number of recent hosts to keep persisted.
    pub const RECENT_HOSTS_MAX: usize = 20;

    /// Record a successful remote connection in `recent_hosts`. Dedupes by
    /// (host, port) and bumps `last_used` to now. Caller is responsible for
    /// persisting the updated list.
    pub fn record_recent_host(&mut self, host: &str, port: u16, ssh_port: u16) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Dedupe — replace existing entry if the (host, port) tuple matches.
        if let Some(existing) = self
            .recent_hosts
            .iter_mut()
            .find(|h| h.host == host && h.port == port)
        {
            existing.ssh_port = ssh_port;
            existing.last_used = now;
        } else {
            self.recent_hosts.push(RecentHost {
                host: host.to_string(),
                port,
                ssh_port,
                last_used: now,
            });
        }

        // Sort by recency (most recent first) and cap.
        self.recent_hosts
            .sort_by(|a, b| b.last_used.cmp(&a.last_used));
        self.recent_hosts.truncate(Self::RECENT_HOSTS_MAX);
    }

    /// Location label for the active connection (used in GoToSession rows).
    fn current_location_label(&self) -> String {
        match &self.remote_config {
            Some(rc) => rc.host.clone(),
            None => "local".to_string(),
        }
    }

    /// Location label for a background slot.
    fn slot_location_label(&self, slot: &ConnectionSlot) -> String {
        match &slot.kind {
            ConnectionKind::Local => "local".to_string(),
            ConnectionKind::Remote { host, .. } => host.clone(),
        }
    }

    /// Session palette layout (flat, no connection-concept leak):
    ///   ── Sessions ──
    ///     work                  [local]    ● (current)
    ///     notes                 [local]
    ///     dev                   [host-a]
    ///     + New Session
    ///   ── Remote Hosts ──  (configured but not connected, optional)
    ///     RemoteHost entries
    ///   + Connect to New Host...
    fn build_session_palette_entries(&self, entries: &mut Vec<PaletteEntry>) {
        entries.push(PaletteEntry {
            label: "Sessions".to_string(),
            kind: PaletteEntryKind::SectionHeader("Sessions".to_string()),
        });
        self.push_flat_session_entries(entries);

        // ── New Session ── (always available, inline at end of sessions)
        entries.push(PaletteEntry {
            label: "+ New Session".to_string(),
            kind: PaletteEntryKind::Action(Action::NewSession),
        });

        // ── Remote Hosts ── (configured in config but not connected yet)
        let remote_host_entries = self.build_remote_host_entries(true);
        if !remote_host_entries.is_empty() {
            entries.push(PaletteEntry {
                label: "Remote Hosts".to_string(),
                kind: PaletteEntryKind::SectionHeader("Remote Hosts".to_string()),
            });
            entries.extend(remote_host_entries);
        }

        // ── Connect to New Host... ── always last.
        entries.push(PaletteEntry {
            label: "+ Connect to New Host...".to_string(),
            kind: PaletteEntryKind::ConnectRemotePrompt,
        });
    }

    /// Shared flat session list: every session on every slot (current + bg),
    /// each tagged with its location. Connection identity is surfaced only
    /// as a `[location]` suffix — no separate "Connections" section.
    fn push_flat_session_entries(&self, entries: &mut Vec<PaletteEntry>) {
        let active_slot = &self.active_slot_id;
        let active_session = &self.session_name;

        // Current slot sessions
        let current_loc = self.current_location_label();
        if !self.cached_local_sessions.is_empty() {
            for s in &self.cached_local_sessions {
                let marker = if &s.name == active_session {
                    "● "
                } else {
                    "  "
                };
                entries.push(PaletteEntry {
                    label: format!("{}{}  [{}]", marker, s.name, current_loc),
                    kind: PaletteEntryKind::GoToSession {
                        slot_id: active_slot.clone(),
                        session_name: s.name.clone(),
                    },
                });
            }
        } else {
            // No cache yet (palette opened before list arrived): show at least
            // the active session so the user sees *something* useful.
            entries.push(PaletteEntry {
                label: format!("● {}  [{}]", active_session, current_loc),
                kind: PaletteEntryKind::GoToSession {
                    slot_id: active_slot.clone(),
                    session_name: active_session.clone(),
                },
            });
        }

        // Background slot sessions (sorted by slot id for stable order)
        let mut slot_ids: Vec<&String> = self.background_slots.keys().collect();
        slot_ids.sort();
        for slot_id in slot_ids {
            let slot = &self.background_slots[slot_id];
            let loc = self.slot_location_label(slot);
            if let Some(sessions) = self.cached_slot_sessions.get(slot_id)
                && !sessions.is_empty()
            {
                for s in sessions {
                    entries.push(PaletteEntry {
                        label: format!("  {}  [{}]", s.name, loc),
                        kind: PaletteEntryKind::GoToSession {
                            slot_id: slot_id.clone(),
                            session_name: s.name.clone(),
                        },
                    });
                }
            } else {
                // No cached sessions for this slot — at least offer a single
                // entry for the slot's last-active session.
                entries.push(PaletteEntry {
                    label: format!("  {}  [{}]", slot.session_name, loc),
                    kind: PaletteEntryKind::GoToSession {
                        slot_id: slot_id.clone(),
                        session_name: slot.session_name.clone(),
                    },
                });
            }
        }
    }

    /// Command palette layout:
    ///   ── Sessions ──
    ///     GoToSession entries (flat, across all slots) + Kill for current slot
    ///     + New Session
    ///   ── Remote Hosts ──  (configured but unconnected, if any)
    ///     RemoteHost entries + Connect to New Host...
    ///   ── Actions ──
    ///     All action entries
    fn build_command_palette_entries(&self, entries: &mut Vec<PaletteEntry>) {
        // ── Sessions ── (flat, tagged by location; no separate Connections)
        entries.push(PaletteEntry {
            label: "Sessions".to_string(),
            kind: PaletteEntryKind::SectionHeader("Sessions".to_string()),
        });
        self.push_flat_session_entries(entries);

        // Kill entries only for current-slot sessions (remote kill unsupported).
        for s in &self.cached_local_sessions {
            entries.push(PaletteEntry {
                label: format!("Kill: {}", s.name),
                kind: PaletteEntryKind::KillSession(s.name.clone()),
            });
        }

        entries.push(PaletteEntry {
            label: "+ New Session".to_string(),
            kind: PaletteEntryKind::Action(Action::NewSession),
        });

        // ── Remote Hosts ── (configured but not connected — still probe-based
        // from command palette, contrast with session palette which skips probe)
        let remote_host_entries = self.build_remote_host_entries(false);
        if !remote_host_entries.is_empty() {
            entries.push(PaletteEntry {
                label: "Remote Hosts".to_string(),
                kind: PaletteEntryKind::SectionHeader("Remote Hosts".to_string()),
            });
            entries.extend(remote_host_entries);
        }
        entries.push(PaletteEntry {
            label: "+ Connect to New Host...".to_string(),
            kind: PaletteEntryKind::ConnectRemotePrompt,
        });

        // ── Actions ──
        let action_entries: Vec<PaletteEntry> = Action::all_with_labels()
            .into_iter()
            .map(|(action, label)| PaletteEntry {
                label: label.to_string(),
                kind: PaletteEntryKind::Action(action),
            })
            .collect();
        if !action_entries.is_empty() {
            entries.push(PaletteEntry {
                label: "Actions".to_string(),
                kind: PaletteEntryKind::SectionHeader("Actions".to_string()),
            });
            entries.extend(action_entries);
        }
    }

    /// Build remote host entries from config, excluding hosts that already
    /// have an active background slot or are the current connection.
    /// Incorporates cached probe results when available.
    fn build_remote_host_entries(&self, sessions_only: bool) -> Vec<PaletteEntry> {
        let active_remote_hosts: std::collections::HashSet<String> = self
            .background_slots
            .values()
            .filter_map(|slot| match &slot.kind {
                ConnectionKind::Remote { host, .. } => Some(host.clone()),
                _ => None,
            })
            .collect();
        let current_remote_host = self.remote_config.as_ref().map(|rc| rc.host.clone());

        let mut entries = Vec::new();
        for rh in &self.config.remote.hosts {
            if active_remote_hosts.contains(&rh.host)
                || current_remote_host.as_deref() == Some(&rh.host)
            {
                continue;
            }

            // Check if we have a cached probe result for this host
            if let Some(probe) = self.cached_remote_probes.get(&rh.host) {
                match probe {
                    RemoteProbeResult::Sessions(sessions) => {
                        if sessions.is_empty() {
                            entries.push(PaletteEntry {
                                label: format!("{} > (new session)", rh.name),
                                kind: PaletteEntryKind::RemoteSession {
                                    host: rh.host.clone(),
                                    port: rh.port,
                                    ssh_port: rh.ssh_port,
                                    session_name: "default".to_string(),
                                },
                            });
                        } else {
                            for s in sessions {
                                entries.push(PaletteEntry {
                                    label: format!("{} > {}", rh.name, s.name),
                                    kind: PaletteEntryKind::RemoteSession {
                                        host: rh.host.clone(),
                                        port: rh.port,
                                        ssh_port: rh.ssh_port,
                                        session_name: s.name.clone(),
                                    },
                                });
                            }
                        }
                    }
                    RemoteProbeResult::NoServer => {
                        entries.push(PaletteEntry {
                            label: format!("SSH: {} (no ciritty-server)", rh.name),
                            kind: PaletteEntryKind::SshShell {
                                name: rh.name.clone(),
                                host: rh.host.clone(),
                                ssh_port: rh.ssh_port,
                            },
                        });
                    }
                    RemoteProbeResult::Error(_) => {
                        // Error results are shown via palette.remote_error, not as entries
                    }
                }
                continue;
            }

            if sessions_only {
                entries.push(PaletteEntry {
                    label: format!("Connect: {} ({}) [remote]", rh.name, rh.host),
                    kind: PaletteEntryKind::DirectConnect {
                        name: rh.name.clone(),
                        host: rh.host.clone(),
                        port: rh.port,
                        ssh_port: rh.ssh_port,
                    },
                });
            } else {
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
        }

        // ── Recent Hosts ── (persisted across runs)
        // Skip those already covered by a configured RemoteHostConfig (matched
        // above), the active connection, or an open background slot.
        let configured_hosts: std::collections::HashSet<(String, u16)> = self
            .config
            .remote
            .hosts
            .iter()
            .map(|rh| (rh.host.clone(), rh.port))
            .collect();
        let active_remote_pairs: std::collections::HashSet<(String, u16)> = self
            .background_slots
            .values()
            .filter_map(|slot| match &slot.kind {
                ConnectionKind::Remote { host, port, .. } => Some((host.clone(), *port)),
                _ => None,
            })
            .collect();
        let current_pair = self
            .remote_config
            .as_ref()
            .map(|rc| (rc.host.clone(), rc.port));

        for rh in &self.recent_hosts {
            let key = (rh.host.clone(), rh.port);
            if configured_hosts.contains(&key)
                || active_remote_pairs.contains(&key)
                || current_pair.as_ref() == Some(&key)
            {
                continue;
            }
            // Render as DirectConnect (skip probing — user already knows it works).
            let label = if sessions_only {
                format!("Recent: {}", rh.host)
            } else {
                format!("Recent: {} ({}:{})", rh.host, rh.host, rh.port)
            };
            entries.push(PaletteEntry {
                label,
                kind: PaletteEntryKind::DirectConnect {
                    name: rh.host.clone(),
                    host: rh.host.clone(),
                    port: rh.port,
                    ssh_port: rh.ssh_port,
                },
            });
        }

        entries
    }

    /// Rebuild palette entries from current cached data and re-filter.
    /// Called when cached data changes while the palette is open.
    /// Preserves the user's current selection when possible.
    pub fn rebuild_palette_entries(&mut self) {
        // Remember the currently selected entry's label to restore after rebuild
        let prev_selected_label = self.command_palette.as_ref().and_then(|p| {
            p.filtered
                .get(p.selected_idx)
                .and_then(|&i| p.entries.get(i))
                .filter(|e| e.kind.is_selectable())
                .map(|e| e.label.clone())
        });

        let sessions_only = self
            .command_palette
            .as_ref()
            .is_some_and(|p| p.sessions_only);
        let entries = self.build_palette_entries(sessions_only);
        if let Some(palette) = &mut self.command_palette {
            palette.entries = entries;
        }
        self.filter_palette();

        // Try to restore the previous selection by matching label
        if let Some(label) = prev_selected_label {
            if let Some(palette) = &mut self.command_palette {
                if let Some(pos) = palette.filtered.iter().position(|&i| {
                    palette.entries[i].kind.is_selectable() && palette.entries[i].label == label
                }) {
                    palette.selected_idx = pos;
                }
            }
        }
    }

    pub fn filter_palette(&mut self) {
        let Some(palette) = &mut self.command_palette else {
            return;
        };
        let needle = palette.query.to_lowercase();

        if needle.is_empty() {
            // No filter: include all entries
            palette.filtered = (0..palette.entries.len()).collect();
        } else {
            // Two-pass filter: match selectable entries, then include their headers
            let mut matched = vec![false; palette.entries.len()];
            for (i, e) in palette.entries.iter().enumerate() {
                if e.kind.is_selectable() && fuzzy_match(&e.label.to_lowercase(), &needle) {
                    matched[i] = true;
                }
            }

            // Include section headers that have at least one matched child after them
            for i in 0..palette.entries.len() {
                if matches!(palette.entries[i].kind, PaletteEntryKind::SectionHeader(_)) {
                    // Check if any selectable entry between this header and the next header matched
                    let has_match = palette.entries[i + 1..]
                        .iter()
                        .take_while(|e| !matches!(e.kind, PaletteEntryKind::SectionHeader(_)))
                        .enumerate()
                        .any(|(j, _)| matched[i + 1 + j]);
                    if has_match {
                        matched[i] = true;
                    }
                }
            }

            palette.filtered = matched
                .iter()
                .enumerate()
                .filter(|(_, m)| **m)
                .map(|(i, _)| i)
                .collect();
        }

        // Ensure selected_idx points to a selectable entry
        palette.selected_idx = palette
            .filtered
            .iter()
            .position(|&i| palette.entries[i].kind.is_selectable())
            .unwrap_or(0);
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
    /// Stores result in cache and rebuilds palette entries.
    pub fn handle_remote_query_result(&mut self, result: RemoteQueryResult) {
        if let Some(palette) = &mut self.command_palette {
            palette.remote_loading = None;
        }

        match &result.result {
            RemoteProbeResult::Error(e) => {
                if let Some(palette) = &mut self.command_palette {
                    palette.remote_error = Some((result.host_name.clone(), e.clone()));
                }
            }
            _ => {
                // Cache the probe result keyed by host address
                self.cached_remote_probes
                    .insert(result.host.clone(), result.result);
            }
        }

        if self.command_palette.is_some() {
            self.rebuild_palette_entries();
        }
    }

    /// Update cached slot sessions and rebuild palette if open.
    pub fn apply_slot_session_result(
        &mut self,
        slot_id: &str,
        sessions: Vec<ciri_protocol::message::SessionInfo>,
    ) {
        // Only cache sessions from slots that still exist
        if self.background_slots.contains_key(slot_id) {
            self.cached_slot_sessions
                .insert(slot_id.to_string(), sessions);
        }
        if self.command_palette.is_some() {
            self.rebuild_palette_entries();
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{
        ConnectionKind, ConnectionSlot, PaletteEntryKind, RemoteConnectionConfig, ServerEvent,
    };
    use ciri_anim::manager::AnimationManager;
    use ciri_config::config::{CiriConfig, RemoteHostConfig};
    use ciri_layout::workspace_set::WorkspaceSet;
    use ciri_protocol::message::{ClientMessage, SessionInfo};

    /// 创建一个最小的 AppModel 用于纯逻辑测试
    fn make_model() -> AppModel {
        AppModel::new(CiriConfig::default(), "test-session")
    }

    /// 创建一个带 server channel 的 AppModel
    fn make_model_with_tx() -> (AppModel, crossbeam_channel::Receiver<ClientMessage>) {
        let mut model = make_model();
        let (tx, rx) = crossbeam_channel::unbounded();
        model.server_tx = Some(tx);
        (model, rx)
    }

    /// 创建一个 mock ConnectionSlot
    fn make_slot(
        id: &str,
        kind: ConnectionKind,
        session_name: &str,
        connected: bool,
    ) -> (
        ConnectionSlot,
        crossbeam_channel::Sender<ServerEvent>,
        crossbeam_channel::Receiver<ClientMessage>,
    ) {
        let (event_tx, event_rx) = crossbeam_channel::unbounded();
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
        let view = ciri_layout::geometry::ViewSize {
            width: 800.0,
            height: 600.0,
        };
        let slot = ConnectionSlot {
            id: id.to_string(),
            kind,
            session_name: session_name.to_string(),
            server_tx: cmd_tx,
            server_rx: event_rx,
            pane_grids: std::collections::HashMap::new(),
            workspaces: WorkspaceSet::new(view),
            expected_pane_ids: std::collections::HashSet::new(),
            connected,
            reconnect_state: None,
            pending_session_name: None,
            anim_mgr: AnimationManager::new(),
            workspace_last_pane_ids: std::collections::HashMap::new(),
            selection: None,
            broadcast_mode: false,
            image_placements: std::collections::HashMap::new(),
            pending_events: std::collections::VecDeque::new(),
        };
        (slot, event_tx, cmd_rx)
    }

    fn session_info(name: &str) -> SessionInfo {
        SessionInfo {
            name: name.to_string(),
            running: true,
            pane_count: 1,
            client_count: 1,
        }
    }

    /// 统计 palette 中特定 kind 类型的条目数
    fn count_entries<F>(model: &AppModel, pred: F) -> usize
    where
        F: Fn(&PaletteEntryKind) -> bool,
    {
        model
            .command_palette
            .as_ref()
            .map(|p| p.entries.iter().filter(|e| pred(&e.kind)).count())
            .unwrap_or(0)
    }

    // -----------------------------------------------------------------------
    // open_session_palette 相关测试
    // -----------------------------------------------------------------------

    #[test]
    fn open_session_palette_sends_query_to_connected_slots() {
        let (mut model, _rx) = make_model_with_tx();

        let (slot_a, _ev_a, cmd_rx_a) = make_slot("slot-a", ConnectionKind::Local, "sess-a", true);
        let (slot_b, _ev_b, cmd_rx_b) = make_slot("slot-b", ConnectionKind::Local, "sess-b", false);
        model.background_slots.insert("slot-a".into(), slot_a);
        model.background_slots.insert("slot-b".into(), slot_b);

        model.open_session_palette();

        let msg = cmd_rx_a.try_recv().expect("connected slot 应收到查询");
        assert!(matches!(msg, ClientMessage::ListSessions { all: false }));
        assert!(
            cmd_rx_b.try_recv().is_err(),
            "disconnected slot 不应收到查询"
        );
    }

    #[test]
    fn open_session_palette_sets_query_start_time() {
        let (mut model, _rx) = make_model_with_tx();

        let (slot, _ev, _cmd) = make_slot("slot-a", ConnectionKind::Local, "s", true);
        model.background_slots.insert("slot-a".into(), slot);

        model.open_session_palette();

        assert!(model.slot_session_query_start.is_some());
        assert!(model.slot_session_pending.contains("slot-a"));
    }

    #[test]
    fn open_session_palette_no_background_slots_no_query_start() {
        let (mut model, _rx) = make_model_with_tx();
        model.open_session_palette();
        assert!(model.slot_session_query_start.is_none());
        assert!(model.slot_session_pending.is_empty());
    }

    #[test]
    fn open_session_palette_resets_pending_on_repeated_call() {
        let (mut model, _rx) = make_model_with_tx();

        let (slot, _ev, _cmd) = make_slot("slot-a", ConnectionKind::Local, "s", true);
        model.background_slots.insert("slot-a".into(), slot);

        model.open_session_palette();
        assert_eq!(model.slot_session_pending.len(), 1);

        model.slot_session_pending.insert("stale-slot".into());
        assert_eq!(model.slot_session_pending.len(), 2);

        model.open_session_palette();
        assert!(!model.slot_session_pending.contains("stale-slot"));
        assert!(model.slot_session_pending.contains("slot-a"));
    }

    #[test]
    fn open_session_palette_all_slots_disconnected() {
        let (mut model, _rx) = make_model_with_tx();

        let (slot_a, _ev_a, _cmd_a) = make_slot("slot-a", ConnectionKind::Local, "s1", false);
        let (slot_b, _ev_b, _cmd_b) = make_slot("slot-b", ConnectionKind::Local, "s2", false);
        model.background_slots.insert("slot-a".into(), slot_a);
        model.background_slots.insert("slot-b".into(), slot_b);

        model.open_session_palette();

        assert!(model.slot_session_pending.is_empty());
        assert!(model.slot_session_query_start.is_none());
    }

    // -----------------------------------------------------------------------
    // build_palette_entries 相关测试
    // -----------------------------------------------------------------------

    #[test]
    fn build_entries_includes_section_headers() {
        let (mut model, _rx) = make_model_with_tx();
        model.cached_local_sessions = vec![session_info("main")];

        model.open_session_palette();

        let headers = count_entries(&model, |k| matches!(k, PaletteEntryKind::SectionHeader(_)));
        assert!(headers >= 1, "应至少有 Local section header");
    }

    #[test]
    fn build_entries_deterministic_slot_order() {
        let (mut model, _rx) = make_model_with_tx();

        // Insert slots in non-alphabetical order
        let (slot_c, _ev_c, _cmd_c) = make_slot("slot-c", ConnectionKind::Local, "sc", true);
        let (slot_a, _ev_a, _cmd_a) = make_slot("slot-a", ConnectionKind::Local, "sa", true);
        let (slot_b, _ev_b, _cmd_b) = make_slot("slot-b", ConnectionKind::Local, "sb", true);
        model.background_slots.insert("slot-c".into(), slot_c);
        model.background_slots.insert("slot-a".into(), slot_a);
        model.background_slots.insert("slot-b".into(), slot_b);

        model.open_session_palette();

        // Collect background-slot GoToSession entries in order. Current slot
        // (sentinel "default") is filtered out; we only check ordering of bg slots.
        let slot_order: Vec<String> = model
            .command_palette
            .as_ref()
            .unwrap()
            .entries
            .iter()
            .filter_map(|e| match &e.kind {
                PaletteEntryKind::GoToSession { slot_id, .. }
                    if slot_id != &model.active_slot_id =>
                {
                    Some(slot_id.clone())
                }
                _ => None,
            })
            .collect();

        assert_eq!(
            slot_order,
            vec!["slot-a", "slot-b", "slot-c"],
            "Slots 应按 ID 排序"
        );
    }

    #[test]
    fn build_entries_skips_active_remote_host() {
        let (mut model, _rx) = make_model_with_tx();

        model.remote_config = Some(RemoteConnectionConfig {
            host: "active.example.com".into(),
            port: 7890,
            ssh_port: 22,
        });
        model.config.remote.hosts.push(RemoteHostConfig {
            name: "active".into(),
            host: "active.example.com".into(),
            port: 7890,
            ssh_port: 22,
        });

        model.open_session_palette();

        let direct_count = count_entries(&model, |k| {
            matches!(k, PaletteEntryKind::DirectConnect { .. })
        });
        assert_eq!(direct_count, 0, "不应为当前活跃连接创建 DirectConnect 条目");
    }

    #[test]
    fn build_entries_skips_background_slot_host() {
        let (mut model, _rx) = make_model_with_tx();

        let (slot, _ev, _cmd) = make_slot(
            "remote:bg.example.com:7890",
            ConnectionKind::Remote {
                host: "bg.example.com".into(),
                port: 7890,
                ssh_port: 22,
            },
            "sess",
            true,
        );
        model
            .background_slots
            .insert("remote:bg.example.com:7890".into(), slot);
        model.config.remote.hosts.push(RemoteHostConfig {
            name: "bg".into(),
            host: "bg.example.com".into(),
            port: 7890,
            ssh_port: 22,
        });

        model.open_session_palette();

        let direct_count = count_entries(&model, |k| {
            matches!(k, PaletteEntryKind::DirectConnect { .. })
        });
        assert_eq!(
            direct_count, 0,
            "不应为已有 background slot 的 host 创建条目"
        );
    }

    #[test]
    fn build_entries_sessions_only_uses_direct_connect() {
        let (mut model, _rx) = make_model_with_tx();

        model.config.remote.hosts.push(RemoteHostConfig {
            name: "dev".into(),
            host: "dev.example.com".into(),
            port: 7890,
            ssh_port: 22,
        });

        model.open_session_palette();

        let dc = count_entries(&model, |k| {
            matches!(k, PaletteEntryKind::DirectConnect { .. })
        });
        let rh = count_entries(&model, |k| matches!(k, PaletteEntryKind::RemoteHost { .. }));
        assert_eq!(dc, 1, "sessions_only 应有 DirectConnect");
        assert_eq!(rh, 0, "sessions_only 不应有 RemoteHost");
    }

    #[test]
    fn build_entries_command_palette_uses_remote_host() {
        let (mut model, _rx) = make_model_with_tx();

        model.config.remote.hosts.push(RemoteHostConfig {
            name: "dev".into(),
            host: "dev.example.com".into(),
            port: 7890,
            ssh_port: 22,
        });

        model.open_command_palette();

        let dc = count_entries(&model, |k| {
            matches!(k, PaletteEntryKind::DirectConnect { .. })
        });
        let rh = count_entries(&model, |k| matches!(k, PaletteEntryKind::RemoteHost { .. }));
        assert_eq!(dc, 0, "command palette 不应有 DirectConnect");
        assert_eq!(rh, 1, "command palette 应有 RemoteHost");
    }

    #[test]
    fn build_entries_no_remote_hosts_config() {
        let (mut model, _rx) = make_model_with_tx();
        assert!(model.config.remote.hosts.is_empty());
        model.open_session_palette();

        let dc = count_entries(&model, |k| {
            matches!(k, PaletteEntryKind::DirectConnect { .. })
        });
        assert_eq!(dc, 0);
    }

    #[test]
    fn build_entries_always_has_connect_prompt() {
        let (mut model, _rx) = make_model_with_tx();
        model.open_session_palette();

        let prompt = count_entries(&model, |k| {
            matches!(k, PaletteEntryKind::ConnectRemotePrompt)
        });
        assert_eq!(prompt, 1, "应始终有 ConnectRemotePrompt 条目");
    }

    #[test]
    fn build_entries_only_cached_sessions() {
        // The cache only contains running sessions (filtered on arrival in sync.rs).
        // Verify that all cached sessions appear in the palette.
        let (mut model, _rx) = make_model_with_tx();

        model.cached_local_sessions = vec![session_info("running1"), session_info("running2")];

        model.open_session_palette();

        // Names of sessions on the current slot
        let switch_names: Vec<String> = model
            .command_palette
            .as_ref()
            .unwrap()
            .entries
            .iter()
            .filter_map(|e| match &e.kind {
                PaletteEntryKind::GoToSession {
                    slot_id,
                    session_name,
                } if slot_id == &model.active_slot_id => Some(session_name.clone()),
                _ => None,
            })
            .collect();

        assert_eq!(switch_names, vec!["running1", "running2"]);
    }

    // -----------------------------------------------------------------------
    // apply_slot_session_result 相关测试
    // -----------------------------------------------------------------------

    #[test]
    fn apply_slot_session_result_updates_cache_and_rebuilds() {
        let (mut model, _rx) = make_model_with_tx();

        let (slot, _ev, _cmd) = make_slot(
            "remote:dev:7890",
            ConnectionKind::Remote {
                host: "dev.example.com".into(),
                port: 7890,
                ssh_port: 22,
            },
            "current",
            true,
        );
        model
            .background_slots
            .insert("remote:dev:7890".into(), slot);

        model.open_session_palette();
        model.apply_slot_session_result(
            "remote:dev:7890",
            vec![session_info("alpha"), session_info("beta")],
        );

        // Verify cache was updated
        assert_eq!(
            model.cached_slot_sessions["remote:dev:7890"].len(),
            2,
            "缓存应有 2 个 session"
        );

        // Verify palette entries were rebuilt — bg slot now contributes
        // GoToSession entries with that slot's id.
        let active = model.active_slot_id.clone();
        let slot_sessions: Vec<_> = model
            .command_palette
            .as_ref()
            .unwrap()
            .entries
            .iter()
            .filter(|e| matches!(&e.kind, PaletteEntryKind::GoToSession { slot_id, .. } if slot_id != &active))
            .collect();
        assert_eq!(
            slot_sessions.len(),
            2,
            "应有 2 个 background GoToSession 条目"
        );
        assert!(
            slot_sessions
                .iter()
                .all(|e| e.label.contains("[dev.example.com]")),
            "远程 slot 的条目应包含主机名标签"
        );
    }

    #[test]
    fn apply_slot_session_result_local_slot_no_remote_tag() {
        let (mut model, _rx) = make_model_with_tx();

        let (slot, _ev, _cmd) = make_slot("local-bg", ConnectionKind::Local, "sess", true);
        model.background_slots.insert("local-bg".into(), slot);

        model.open_session_palette();
        model.apply_slot_session_result("local-bg", vec![session_info("main")]);

        let active = model.active_slot_id.clone();
        let entry = model
            .command_palette
            .as_ref()
            .unwrap()
            .entries
            .iter()
            .find(|e| matches!(&e.kind, PaletteEntryKind::GoToSession { slot_id, .. } if slot_id != &active))
            .expect("应有 background GoToSession 条目");

        assert!(entry.label.contains("[local]"), "本地 slot 应标记 [local]");
    }

    #[test]
    fn apply_slot_session_result_replaces_old_entries() {
        let (mut model, _rx) = make_model_with_tx();

        let (slot, _ev, _cmd) = make_slot("slot-x", ConnectionKind::Local, "s", true);
        model.background_slots.insert("slot-x".into(), slot);

        model.open_session_palette();
        model.apply_slot_session_result("slot-x", vec![session_info("old1"), session_info("old2")]);
        assert_eq!(
            count_entries(&model, |k| matches!(
                k,
                PaletteEntryKind::GoToSession { slot_id, .. } if slot_id == "slot-x"
            )),
            2
        );

        model.apply_slot_session_result("slot-x", vec![session_info("new1")]);
        let ss: Vec<_> = model
            .command_palette
            .as_ref()
            .unwrap()
            .entries
            .iter()
            .filter_map(|e| match &e.kind {
                PaletteEntryKind::GoToSession {
                    slot_id,
                    session_name,
                } if slot_id == "slot-x" => Some(session_name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(ss, vec!["new1"], "旧条目应被替换");
    }

    #[test]
    fn apply_slot_session_result_nonexistent_slot_no_panic() {
        let (mut model, _rx) = make_model_with_tx();
        model.open_session_palette();
        model.apply_slot_session_result("nonexistent", vec![session_info("x")]);

        assert_eq!(
            count_entries(&model, |k| matches!(
                k,
                PaletteEntryKind::GoToSession { slot_id, .. } if slot_id == "nonexistent"
            )),
            0
        );
    }

    #[test]
    fn apply_slot_session_result_empty_session_list() {
        let (mut model, _rx) = make_model_with_tx();

        let (slot, _ev, _cmd) = make_slot("slot-e", ConnectionKind::Local, "s", true);
        model.background_slots.insert("slot-e".into(), slot);

        model.open_session_palette();
        model.apply_slot_session_result("slot-e", vec![]);

        // Empty cached list → fallback to slot.session_name as a single entry.
        assert_eq!(
            count_entries(&model, |k| matches!(
                k,
                PaletteEntryKind::GoToSession { slot_id, .. } if slot_id == "slot-e"
            )),
            1,
            "空 cache 时回退到 slot 的当前 session（一个条目）"
        );
    }

    // -----------------------------------------------------------------------
    // filter_palette 相关测试
    // -----------------------------------------------------------------------

    #[test]
    fn filter_palette_matches_slot_session_entries() {
        let (mut model, _rx) = make_model_with_tx();

        let (slot, _ev, _cmd) = make_slot("slot-f", ConnectionKind::Local, "s", true);
        model.background_slots.insert("slot-f".into(), slot);

        model.open_session_palette();
        model.apply_slot_session_result("slot-f", vec![session_info("my-special-session")]);

        if let Some(palette) = &mut model.command_palette {
            palette.query = "special".to_string();
        }
        model.filter_palette();

        let palette = model.command_palette.as_ref().unwrap();
        assert!(
            palette.filtered.iter().any(|&i| {
                matches!(
                    &palette.entries[i].kind,
                    PaletteEntryKind::GoToSession { session_name, slot_id, .. } if session_name == "my-special-session" && slot_id == "slot-f"
                )
            }),
            "应能通过模糊搜索匹配到 background slot 的 GoToSession 条目"
        );
    }

    #[test]
    fn filter_palette_matches_direct_connect_entries() {
        let (mut model, _rx) = make_model_with_tx();

        model.config.remote.hosts.push(RemoteHostConfig {
            name: "production".into(),
            host: "prod.example.com".into(),
            port: 7890,
            ssh_port: 22,
        });

        model.open_session_palette();

        if let Some(palette) = &mut model.command_palette {
            palette.query = "prod".to_string();
        }
        model.filter_palette();

        let palette = model.command_palette.as_ref().unwrap();
        assert!(
            palette.filtered.iter().any(|&i| {
                matches!(&palette.entries[i].kind, PaletteEntryKind::DirectConnect { name, .. } if name == "production")
            }),
            "应能通过模糊搜索匹配到 DirectConnect 条目"
        );
    }

    #[test]
    fn filter_palette_hides_empty_section_headers() {
        let (mut model, _rx) = make_model_with_tx();

        model.cached_local_sessions = vec![session_info("alpha"), session_info("beta")];
        model.open_session_palette();

        // Filter for something that only matches one session
        if let Some(palette) = &mut model.command_palette {
            palette.query = "alpha".to_string();
        }
        model.filter_palette();

        let palette = model.command_palette.as_ref().unwrap();
        // The Local header should be present (has matching child)
        let filtered_headers: Vec<_> = palette
            .filtered
            .iter()
            .filter(|&&i| matches!(palette.entries[i].kind, PaletteEntryKind::SectionHeader(_)))
            .collect();
        assert!(
            !filtered_headers.is_empty(),
            "匹配条目所在的 section header 应可见"
        );
    }

    #[test]
    fn filter_palette_selected_idx_skips_headers() {
        let (mut model, _rx) = make_model_with_tx();

        model.cached_local_sessions = vec![session_info("main")];
        model.open_session_palette();

        let palette = model.command_palette.as_ref().unwrap();
        // selected_idx should point to a selectable entry, not a header
        if !palette.filtered.is_empty() {
            let selected_entry = &palette.entries[palette.filtered[palette.selected_idx]];
            assert!(
                selected_entry.kind.is_selectable(),
                "selected_idx 应指向可选条目，而不是 SectionHeader"
            );
        }
    }

    // -----------------------------------------------------------------------
    // rebuild_palette_entries (cache → rebuild) 测试
    // -----------------------------------------------------------------------

    #[test]
    fn rebuild_preserves_sessions_and_slots() {
        let (mut model, _rx) = make_model_with_tx();

        let (slot, _ev, _cmd) = make_slot("slot-g", ConnectionKind::Local, "s", true);
        model.background_slots.insert("slot-g".into(), slot);

        model.open_session_palette();

        // Simulate SessionList + slot sessions arriving
        model.cached_local_sessions = vec![session_info("local-1"), session_info("local-2")];
        model.apply_slot_session_result("slot-g", vec![session_info("bg-session")]);

        // Both current-slot sessions and background slot sessions should
        // appear as GoToSession entries (distinguished by slot_id).
        let active = model.active_slot_id.clone();
        assert!(
            count_entries(&model, |k| matches!(
                k,
                PaletteEntryKind::GoToSession { slot_id, .. } if slot_id == "slot-g"
            )) >= 1,
            "rebuild 后应保留 background slot 的 GoToSession 条目"
        );
        assert!(
            count_entries(&model, |k| matches!(
                k,
                PaletteEntryKind::GoToSession { slot_id, .. } if slot_id == &active
            )) >= 1,
            "rebuild 后应有 current slot 的 GoToSession 条目"
        );
    }

    #[test]
    fn rebuild_no_duplicate_entries() {
        let (mut model, _rx) = make_model_with_tx();

        let (slot, _ev, _cmd) = make_slot("slot-i", ConnectionKind::Local, "s", true);
        model.background_slots.insert("slot-i".into(), slot);

        model.open_session_palette();

        // Rebuild twice
        model.cached_local_sessions = vec![session_info("main")];
        model.rebuild_palette_entries();
        let count1 = count_entries(&model, |k| {
            matches!(k, PaletteEntryKind::GoToSession { .. })
        });

        model.rebuild_palette_entries();
        let count2 = count_entries(&model, |k| {
            matches!(k, PaletteEntryKind::GoToSession { .. })
        });

        assert_eq!(count1, count2, "多次 rebuild 不应产生重复条目");
    }
}
