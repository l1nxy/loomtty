use ciri_input::action::Action;
use ciri_protocol::message::ClientMessage;

use super::{
    AppModel, CommandPaletteState, ConnectionKind, PaletteEntry, PaletteEntryKind,
    RemoteProbeResult, RemoteQueryResult,
};

impl AppModel {
    pub fn open_command_palette(&mut self) {
        let entries: Vec<PaletteEntry> = Action::all_with_labels()
            .into_iter()
            .map(|(action, label)| PaletteEntry {
                label: label.to_string(),
                kind: PaletteEntryKind::Action(action),
            })
            .collect();

        self.command_palette = Some(CommandPaletteState {
            query: String::new(),
            entries,
            filtered: Vec::new(),
            selected_idx: 0,
            hovered_idx: None,
            sessions_only: false,
            sessions_show_all: true,
            remote_loading: None,
            remote_error: None,
        });
        self.append_connection_entries();
        self.filter_palette();
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
        // Show background slots and remote hosts immediately while waiting
        // for the async SessionList response to populate local sessions.
        self.append_connection_entries();
        self.filter_palette();
        self.send(ClientMessage::ListSessions { all: false });

        // Query sessions from all connected background slots for the unified list.
        self.slot_session_pending.clear();
        self.slot_session_query_start = None;
        for (id, slot) in &self.background_slots {
            if slot.connected
                && slot
                    .server_tx
                    .try_send(ClientMessage::ListSessions { all: true })
                    .is_ok()
            {
                self.slot_session_pending.insert(id.clone());
            }
        }
        if !self.slot_session_pending.is_empty() {
            self.slot_session_query_start = Some(std::time::Instant::now());
        }
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

    /// Append background connection slots and configured remote hosts to the
    /// current palette.  Called after session entries are populated so that both
    /// the session palette and the command palette include connection entries.
    pub fn append_connection_entries(&mut self) {
        // Take the palette out to avoid overlapping borrows with
        // self.background_slots / self.config.
        let Some(mut palette) = self.command_palette.take() else {
            return;
        };

        // Remove previous SwitchSlot/DirectConnect entries to avoid duplicates
        // (this method is called multiple times: on palette open and on each
        // SessionList response).
        palette.entries.retain(|e| {
            !matches!(
                e.kind,
                PaletteEntryKind::SwitchSlot(_)
                    | PaletteEntryKind::DirectConnect { .. }
                    | PaletteEntryKind::RemoteHost { .. }
            )
        });

        // Background connection slots — quick-switch entries
        for (id, slot) in &self.background_slots {
            let label = match &slot.kind {
                ConnectionKind::Local => {
                    format!("Switch to: local ({})", slot.session_name)
                }
                ConnectionKind::Remote { host, .. } => {
                    format!("Switch to: {} ({}) [remote]", host, slot.session_name)
                }
            };
            palette.entries.push(PaletteEntry {
                label,
                kind: PaletteEntryKind::SwitchSlot(id.clone()),
            });
        }

        // Configured remote hosts — direct connect entries (VSCode SSH style)
        // Only show hosts that don't already have an active background slot.
        let active_remote_hosts: std::collections::HashSet<String> = self
            .background_slots
            .values()
            .filter_map(|slot| match &slot.kind {
                ConnectionKind::Remote { host, .. } => Some(host.clone()),
                _ => None,
            })
            .collect();
        // Also exclude the currently active remote connection
        let current_remote_host = self
            .remote_config
            .as_ref()
            .map(|rc| rc.host.clone());

        for rh in &self.config.remote.hosts {
            if active_remote_hosts.contains(&rh.host)
                || current_remote_host.as_deref() == Some(&rh.host)
            {
                continue;
            }
            if palette.sessions_only {
                // Session palette: direct connect (no probe needed)
                palette.entries.push(PaletteEntry {
                    label: format!("Connect: {} ({}) [remote, new session]", rh.name, rh.host),
                    kind: PaletteEntryKind::DirectConnect {
                        name: rh.name.clone(),
                        host: rh.host.clone(),
                        port: rh.port,
                        ssh_port: rh.ssh_port,
                    },
                });
            } else {
                // Command palette: probe first (existing behavior)
                palette.entries.push(PaletteEntry {
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

        self.command_palette = Some(palette);
    }

    /// Handle a `SessionList` response from a background slot.
    /// Inserts per-session entries into the palette so the user can switch
    /// to a specific session on that server.
    pub fn apply_slot_session_result(
        &mut self,
        slot_id: &str,
        sessions: Vec<ciri_protocol::message::SessionInfo>,
    ) {
        // Take palette out to avoid borrow conflict with self.background_slots
        let Some(mut palette) = self.command_palette.take() else {
            return;
        };

        // Remove any previous SlotSession entries for this slot
        palette.entries.retain(|e| {
            !matches!(&e.kind, PaletteEntryKind::SlotSession { slot_id: sid, .. } if sid == slot_id)
        });

        // Look up slot kind for labeling
        if let Some(slot) = self.background_slots.get(slot_id) {
            let host_label = match &slot.kind {
                ConnectionKind::Local => "local".to_string(),
                ConnectionKind::Remote { host, .. } => host.clone(),
            };
            let is_remote = matches!(slot.kind, ConnectionKind::Remote { .. });

            for s in &sessions {
                let tag = if is_remote { " [remote]" } else { "" };
                palette.entries.push(PaletteEntry {
                    label: format!("  {} > {}{}", host_label, s.name, tag),
                    kind: PaletteEntryKind::SlotSession {
                        slot_id: slot_id.to_string(),
                        session_name: s.name.clone(),
                    },
                });
            }
        }

        self.command_palette = Some(palette);
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
    fn make_model_with_tx() -> (
        AppModel,
        crossbeam_channel::Receiver<ClientMessage>,
    ) {
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
        // 向已连接的后台 slot 发送 ListSessions，不向断连 slot 发
        let (mut model, _rx) = make_model_with_tx();

        let (slot_a, _ev_a, cmd_rx_a) =
            make_slot("slot-a", ConnectionKind::Local, "sess-a", true);
        let (slot_b, _ev_b, cmd_rx_b) =
            make_slot("slot-b", ConnectionKind::Local, "sess-b", false);
        model.background_slots.insert("slot-a".into(), slot_a);
        model.background_slots.insert("slot-b".into(), slot_b);

        model.open_session_palette();

        // connected slot 应该收到 ListSessions
        let msg = cmd_rx_a.try_recv().expect("connected slot 应收到查询");
        assert!(matches!(msg, ClientMessage::ListSessions { all: true }));

        // disconnected slot 不应该收到任何消息
        assert!(cmd_rx_b.try_recv().is_err(), "disconnected slot 不应收到查询");
    }

    #[test]
    fn open_session_palette_sets_query_start_time() {
        // 有 pending slot 时应设置 slot_session_query_start
        let (mut model, _rx) = make_model_with_tx();

        let (slot, _ev, _cmd) = make_slot("slot-a", ConnectionKind::Local, "s", true);
        model.background_slots.insert("slot-a".into(), slot);

        model.open_session_palette();

        assert!(model.slot_session_query_start.is_some());
        assert!(model.slot_session_pending.contains("slot-a"));
    }

    #[test]
    fn open_session_palette_no_background_slots_no_query_start() {
        // 没有后台 slot 时 slot_session_query_start 应为 None
        let (mut model, _rx) = make_model_with_tx();

        model.open_session_palette();

        assert!(model.slot_session_query_start.is_none());
        assert!(model.slot_session_pending.is_empty());
    }

    #[test]
    fn open_session_palette_resets_pending_on_repeated_call() {
        // 重复调用时 pending 集合应被正确重置
        let (mut model, _rx) = make_model_with_tx();

        let (slot, _ev, _cmd) = make_slot("slot-a", ConnectionKind::Local, "s", true);
        model.background_slots.insert("slot-a".into(), slot);

        model.open_session_palette();
        assert_eq!(model.slot_session_pending.len(), 1);

        // 手动添加一个假的 pending 条目
        model.slot_session_pending.insert("stale-slot".into());
        assert_eq!(model.slot_session_pending.len(), 2);

        // 重新打开 palette 应清除旧的 pending
        model.open_session_palette();
        assert!(!model.slot_session_pending.contains("stale-slot"));
        assert!(model.slot_session_pending.contains("slot-a"));
    }

    #[test]
    fn open_session_palette_all_slots_disconnected() {
        // 所有 slot 都 disconnected 时不应 panic，也不应设置 query_start
        let (mut model, _rx) = make_model_with_tx();

        let (slot_a, _ev_a, _cmd_a) =
            make_slot("slot-a", ConnectionKind::Local, "s1", false);
        let (slot_b, _ev_b, _cmd_b) =
            make_slot("slot-b", ConnectionKind::Local, "s2", false);
        model.background_slots.insert("slot-a".into(), slot_a);
        model.background_slots.insert("slot-b".into(), slot_b);

        model.open_session_palette();

        assert!(model.slot_session_pending.is_empty());
        assert!(model.slot_session_query_start.is_none());
    }

    // -----------------------------------------------------------------------
    // append_connection_entries 相关测试
    // -----------------------------------------------------------------------

    #[test]
    fn append_connection_entries_no_duplicates_on_double_call() {
        // 调用两次后不应有重复的 SwitchSlot/DirectConnect 条目
        let (mut model, _rx) = make_model_with_tx();

        let (slot, _ev, _cmd) = make_slot("slot-a", ConnectionKind::Local, "sess", true);
        model.background_slots.insert("slot-a".into(), slot);
        model.config.remote.hosts.push(RemoteHostConfig {
            name: "dev".into(),
            host: "dev.example.com".into(),
            port: 7890,
            ssh_port: 22,
        });

        model.open_session_palette();
        let count_before = count_entries(&model, |k| {
            matches!(k, PaletteEntryKind::SwitchSlot(_) | PaletteEntryKind::DirectConnect { .. })
        });

        // 再次调用 append_connection_entries
        model.append_connection_entries();
        let count_after = count_entries(&model, |k| {
            matches!(k, PaletteEntryKind::SwitchSlot(_) | PaletteEntryKind::DirectConnect { .. })
        });

        assert_eq!(count_before, count_after, "不应产生重复条目");
    }

    #[test]
    fn append_connection_entries_skips_active_remote_host() {
        // 跳过当前活跃连接对应的 remote host
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
    fn append_connection_entries_skips_background_slot_host() {
        // 跳过已有 background slot 的 remote host
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
        assert_eq!(direct_count, 0, "不应为已有 background slot 的 host 创建条目");
    }

    #[test]
    fn append_connection_entries_sessions_only_uses_direct_connect() {
        // sessions_only 模式下应添加 DirectConnect 而非 RemoteHost
        let (mut model, _rx) = make_model_with_tx();

        model.config.remote.hosts.push(RemoteHostConfig {
            name: "dev".into(),
            host: "dev.example.com".into(),
            port: 7890,
            ssh_port: 22,
        });

        model.open_session_palette(); // sessions_only = true

        let dc = count_entries(&model, |k| matches!(k, PaletteEntryKind::DirectConnect { .. }));
        let rh = count_entries(&model, |k| matches!(k, PaletteEntryKind::RemoteHost { .. }));
        assert_eq!(dc, 1, "sessions_only 应有 DirectConnect");
        assert_eq!(rh, 0, "sessions_only 不应有 RemoteHost");
    }

    #[test]
    fn append_connection_entries_command_palette_uses_remote_host() {
        // 非 sessions_only（命令面板）模式下应添加 RemoteHost 而非 DirectConnect
        let (mut model, _rx) = make_model_with_tx();

        model.config.remote.hosts.push(RemoteHostConfig {
            name: "dev".into(),
            host: "dev.example.com".into(),
            port: 7890,
            ssh_port: 22,
        });

        model.open_command_palette(); // sessions_only = false

        let dc = count_entries(&model, |k| matches!(k, PaletteEntryKind::DirectConnect { .. }));
        let rh = count_entries(&model, |k| matches!(k, PaletteEntryKind::RemoteHost { .. }));
        assert_eq!(dc, 0, "command palette 不应有 DirectConnect");
        assert_eq!(rh, 1, "command palette 应有 RemoteHost");
    }

    #[test]
    fn append_connection_entries_no_remote_hosts_config() {
        // config 中没有 remote hosts 时不应 panic
        let (mut model, _rx) = make_model_with_tx();

        assert!(model.config.remote.hosts.is_empty());
        model.open_session_palette();

        let dc = count_entries(&model, |k| matches!(k, PaletteEntryKind::DirectConnect { .. }));
        assert_eq!(dc, 0);
    }

    // -----------------------------------------------------------------------
    // apply_slot_session_result 相关测试
    // -----------------------------------------------------------------------

    #[test]
    fn apply_slot_session_result_adds_entries_with_remote_tag() {
        // 为远程 slot 添加 SlotSession 条目并标记 [remote]
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

        let slot_sessions: Vec<_> = model
            .command_palette
            .as_ref()
            .unwrap()
            .entries
            .iter()
            .filter(|e| matches!(&e.kind, PaletteEntryKind::SlotSession { .. }))
            .collect();

        assert_eq!(slot_sessions.len(), 2, "应有 2 个 SlotSession 条目");
        assert!(
            slot_sessions.iter().all(|e| e.label.contains("[remote]")),
            "远程 slot 的条目应标记 [remote]"
        );
    }

    #[test]
    fn apply_slot_session_result_local_slot_no_remote_tag() {
        // 本地 slot 的条目不应有 [remote] 标记
        let (mut model, _rx) = make_model_with_tx();

        let (slot, _ev, _cmd) = make_slot("local-bg", ConnectionKind::Local, "sess", true);
        model.background_slots.insert("local-bg".into(), slot);

        model.open_session_palette();
        model.apply_slot_session_result("local-bg", vec![session_info("main")]);

        let entry = model
            .command_palette
            .as_ref()
            .unwrap()
            .entries
            .iter()
            .find(|e| matches!(&e.kind, PaletteEntryKind::SlotSession { .. }))
            .expect("应有 SlotSession 条目");

        assert!(
            !entry.label.contains("[remote]"),
            "本地 slot 不应有 [remote] 标记"
        );
    }

    #[test]
    fn apply_slot_session_result_replaces_old_entries() {
        // 替换同一 slot 的旧条目
        let (mut model, _rx) = make_model_with_tx();

        let (slot, _ev, _cmd) = make_slot("slot-x", ConnectionKind::Local, "s", true);
        model.background_slots.insert("slot-x".into(), slot);

        model.open_session_palette();
        model.apply_slot_session_result("slot-x", vec![session_info("old1"), session_info("old2")]);
        assert_eq!(
            count_entries(&model, |k| matches!(k, PaletteEntryKind::SlotSession { .. })),
            2
        );

        // 用新列表替换
        model.apply_slot_session_result("slot-x", vec![session_info("new1")]);
        let ss: Vec<_> = model
            .command_palette
            .as_ref()
            .unwrap()
            .entries
            .iter()
            .filter_map(|e| match &e.kind {
                PaletteEntryKind::SlotSession { session_name, .. } => Some(session_name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(ss, vec!["new1"], "旧条目应被替换");
    }

    #[test]
    fn apply_slot_session_result_nonexistent_slot_no_panic() {
        // 对不存在的 slot 调用不应 panic
        let (mut model, _rx) = make_model_with_tx();

        model.open_session_palette();
        model.apply_slot_session_result("nonexistent", vec![session_info("x")]);

        // 不存在的 slot 不会创建条目
        assert_eq!(
            count_entries(&model, |k| matches!(k, PaletteEntryKind::SlotSession { .. })),
            0
        );
    }

    #[test]
    fn apply_slot_session_result_empty_session_list() {
        // 空 session 列表不应 panic
        let (mut model, _rx) = make_model_with_tx();

        let (slot, _ev, _cmd) = make_slot("slot-e", ConnectionKind::Local, "s", true);
        model.background_slots.insert("slot-e".into(), slot);

        model.open_session_palette();
        model.apply_slot_session_result("slot-e", vec![]);

        assert_eq!(
            count_entries(&model, |k| matches!(k, PaletteEntryKind::SlotSession { .. })),
            0,
            "空 session 列表不应创建条目"
        );
    }

    // -----------------------------------------------------------------------
    // filter_palette 相关测试
    // -----------------------------------------------------------------------

    #[test]
    fn filter_palette_matches_slot_session_entries() {
        // filter_palette 能匹配 SlotSession 条目
        let (mut model, _rx) = make_model_with_tx();

        let (slot, _ev, _cmd) = make_slot("slot-f", ConnectionKind::Local, "s", true);
        model.background_slots.insert("slot-f".into(), slot);

        model.open_session_palette();
        model.apply_slot_session_result("slot-f", vec![session_info("my-special-session")]);

        // 用关键词过滤
        if let Some(palette) = &mut model.command_palette {
            palette.query = "special".to_string();
        }
        model.filter_palette();

        let palette = model.command_palette.as_ref().unwrap();
        assert!(
            palette.filtered.iter().any(|&i| {
                matches!(
                    &palette.entries[i].kind,
                    PaletteEntryKind::SlotSession { session_name, .. } if session_name == "my-special-session"
                )
            }),
            "应能通过模糊搜索匹配到 SlotSession 条目"
        );
    }

    #[test]
    fn filter_palette_matches_direct_connect_entries() {
        // filter_palette 能匹配 DirectConnect 条目
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

    // -----------------------------------------------------------------------
    // SessionList 响应处理（模拟 sync.rs 中的逻辑在 AppModel 层）
    // -----------------------------------------------------------------------

    #[test]
    fn session_list_preserves_slot_sessions_in_sessions_only_mode() {
        // sessions_only 模式下 SessionList 保留 SlotSession 条目
        let (mut model, _rx) = make_model_with_tx();

        let (slot, _ev, _cmd) = make_slot("slot-g", ConnectionKind::Local, "s", true);
        model.background_slots.insert("slot-g".into(), slot);

        model.open_session_palette();
        model.apply_slot_session_result("slot-g", vec![session_info("bg-session")]);

        // 模拟 SessionList 响应：sessions_only 模式下保留 SlotSession
        {
            let palette = model.command_palette.as_mut().unwrap();
            assert!(palette.sessions_only);
            // 模拟 sync.rs 中的 SessionList 处理逻辑
            palette
                .entries
                .retain(|e| matches!(e.kind, PaletteEntryKind::SlotSession { .. }));
            for s in &[session_info("local-1"), session_info("local-2")] {
                palette.entries.push(PaletteEntry {
                    label: format!("Switch to: {}", s.name),
                    kind: PaletteEntryKind::SwitchSession(s.name.clone()),
                });
            }
        }
        model.append_connection_entries();
        model.filter_palette();

        // 验证 SlotSession 条目保留
        assert!(
            count_entries(&model, |k| matches!(k, PaletteEntryKind::SlotSession { .. })) >= 1,
            "SessionList 处理后应保留 SlotSession 条目"
        );
        // 验证本地 session 也存在
        assert!(
            count_entries(&model, |k| matches!(k, PaletteEntryKind::SwitchSession(_))) >= 1,
            "SessionList 处理后应有 SwitchSession 条目"
        );
    }

    #[test]
    fn session_list_preserves_slot_sessions_in_command_palette_mode() {
        // 非 sessions_only 模式下 SessionList 也保留 SlotSession
        let (mut model, _rx) = make_model_with_tx();

        let (slot, _ev, _cmd) = make_slot("slot-h", ConnectionKind::Local, "s", true);
        model.background_slots.insert("slot-h".into(), slot);

        model.open_command_palette();
        model.apply_slot_session_result("slot-h", vec![session_info("bg-sess")]);

        // 模拟非 sessions_only 的 SessionList 处理
        {
            let palette = model.command_palette.as_mut().unwrap();
            assert!(!palette.sessions_only);
            palette.entries.retain(|e| {
                matches!(
                    e.kind,
                    PaletteEntryKind::Action(_)
                        | PaletteEntryKind::RemoteSession { .. }
                        | PaletteEntryKind::SshShell { .. }
                        | PaletteEntryKind::SlotSession { .. }
                )
            });
            palette.entries.push(PaletteEntry {
                label: "Switch to: default".to_string(),
                kind: PaletteEntryKind::SwitchSession("default".to_string()),
            });
        }
        model.append_connection_entries();
        model.filter_palette();

        assert!(
            count_entries(&model, |k| matches!(k, PaletteEntryKind::SlotSession { .. })) >= 1,
            "command palette 的 SessionList 处理也应保留 SlotSession"
        );
    }

    #[test]
    fn session_list_then_append_no_duplicate_switch_slot() {
        // SessionList 后 append_connection_entries 不产生重复 SwitchSlot
        let (mut model, _rx) = make_model_with_tx();

        let (slot, _ev, _cmd) = make_slot("slot-i", ConnectionKind::Local, "s", true);
        model.background_slots.insert("slot-i".into(), slot);

        model.open_session_palette();

        let switch_count = count_entries(&model, |k| matches!(k, PaletteEntryKind::SwitchSlot(_)));

        // 模拟 SessionList 后再次 append
        model.append_connection_entries();
        let switch_count_after =
            count_entries(&model, |k| matches!(k, PaletteEntryKind::SwitchSlot(_)));

        assert_eq!(
            switch_count, switch_count_after,
            "SessionList 后 append_connection_entries 不应产生重复 SwitchSlot"
        );
    }
}
