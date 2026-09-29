//! Cocoa scripting objects, queued commands, and correlated terminal creation.
mod cocoa;
mod execution;
mod model;
pub(super) use cocoa::{cancel_all, install};
use crossbeam_channel::Sender;
pub(crate) use execution::creation_result;
use loom_protocol::message::ClientMessage;
use std::collections::HashMap;
use std::time::Instant;
use winit::window::WindowId;

pub(super) fn configure(enabled: bool) {
    cocoa::publish(model::Snapshot {
        enabled,
        ..model::Snapshot::default()
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verb {
    NewWindow,
    NewTab,
    Split,
    Focus,
    CloseTerminal,
    CloseTab,
    CloseWindow,
    Input,
}
#[derive(Clone, Debug)]
struct Request {
    verb: Verb,
    target: Option<String>,
    input: Option<String>,
    directory: Option<String>,
    direction: String,
}
struct Connection {
    session: String,
    remote: Option<(String, u16, u16)>,
    sender: Option<Sender<ClientMessage>>,
    connected: bool,
}
struct Waiting {
    token: u64,
    tab: String,
    deadline: Instant,
    result: WaitResult,
}
enum WaitResult {
    Tab { window: bool },
    Split { parent: String, pane: Option<u64> },
}
#[derive(Default)]
pub(super) struct Bridge {
    ids: model::Ids,
    connections: HashMap<String, Connection>,
    waiting: Vec<Waiting>,
}
impl Bridge {
    pub fn deadline(&self) -> Option<Instant> {
        self.waiting.iter().map(|wait| wait.deadline).min()
    }
    fn capture(
        &mut self,
        apps: &[crate::app::App],
        active: Option<WindowId>,
        enabled: bool,
    ) -> model::Snapshot {
        use model::{Kind, Object, Snapshot};
        let mut snapshot = Snapshot {
            enabled,
            ..Snapshot::default()
        };
        let windows: Vec<_> = apps
            .iter()
            .filter(|app| !app.native_quick_terminal && app.core.config.window.macos_applescript)
            .filter_map(|app| {
                Some((
                    app,
                    app.window.as_ref()?,
                    super::native::native_window(app.window.as_ref()?)?,
                ))
            })
            .collect();
        for (app, window, native) in &windows {
            let id = self.ids.tab(window.id());
            let connection = Connection {
                session: app.core.session_name.clone(),
                remote: app
                    .core
                    .remote_config
                    .as_ref()
                    .map(|r| (r.host.clone(), r.port, r.ssh_port)),
                sender: app.core.server_tx.clone(),
                connected: app.core.connected,
            };
            if self.connections.get(&id).is_some_and(|old| {
                old.session != connection.session
                    || old.remote != connection.remote
                    || old.connected != connection.connected
                    || match (&old.sender, &connection.sender) {
                        (Some(a), Some(b)) => !a.same_channel(b),
                        (None, None) => false,
                        _ => true,
                    }
            }) {
                self.ids.invalidate_terminals(&id);
            }
            self.connections.insert(id.clone(), connection);
            let pane_ids = app.core.workspaces.all_pane_ids();
            let mut terminals = Vec::new();
            let mut focused = None;
            // Disconnected grids are retained for viewing, but their script IDs
            // expire so a stale reference cannot reach a replacement server.
            if app.core.connected && app.core.pending_session_name.is_none() {
                for pane_id in pane_ids {
                    let Some(grid) = app.core.pane_grids.get(&pane_id) else {
                        continue;
                    };
                    let terminal = self.ids.terminal(&id, pane_id);
                    if app.core.workspaces.active().active_pane_id() == Some(pane_id) {
                        focused = Some(terminal.clone());
                    }
                    snapshot.objects.insert(
                        terminal.clone(),
                        Object {
                            id: terminal.clone(),
                            kind: Kind::Terminal,
                            name: grid.title.clone(),
                            tabs: Vec::new(),
                            terminals: Vec::new(),
                            selected: None,
                            focused: None,
                            cwd: grid.cwd.clone().unwrap_or_default(),
                            session: app.core.session_name.clone(),
                            window_id: window.id(),
                            pane_id: Some(pane_id),
                            ready: true,
                        },
                    );
                    terminals.push(terminal);
                }
            }
            snapshot.terminals.extend(terminals.iter().cloned());
            snapshot.tabs.push(id.clone());
            snapshot.objects.insert(
                id.clone(),
                Object {
                    id,
                    kind: Kind::Tab,
                    name: native.title().to_string(),
                    tabs: Vec::new(),
                    terminals,
                    selected: None,
                    focused,
                    cwd: String::new(),
                    session: app.core.session_name.clone(),
                    window_id: window.id(),
                    pane_id: None,
                    ready: app.core.connected && app.core.pending_session_name.is_none(),
                },
            );
        }
        let mut groups: Vec<(Vec<String>, String)> = Vec::new();
        for (_, window, native) in &windows {
            let id = self.ids.tab(window.id());
            if groups.iter().any(|(tabs, _)| tabs.contains(&id)) {
                continue;
            }
            let group = native.tabGroup();
            let tabs: Vec<_> = group
                .as_ref()
                .map(|g| g.windows())
                .into_iter()
                .flat_map(|tabs| tabs.to_vec())
                .filter_map(|tab| windows.iter().find(|(_, _, native)| **native == *tab))
                .map(|(_, window, _)| self.ids.tab(window.id()))
                .collect();
            let tabs = if tabs.is_empty() {
                vec![id.clone()]
            } else {
                tabs
            };
            let selected = group
                .and_then(|group| group.selectedWindow())
                .and_then(|selected| windows.iter().find(|(_, _, native)| **native == *selected))
                .map(|(_, window, _)| self.ids.tab(window.id()))
                .unwrap_or_else(|| id.clone());
            groups.push((tabs, selected));
        }
        let group_ids = self.ids.groups(
            &groups
                .iter()
                .map(|(tabs, _)| tabs.clone())
                .collect::<Vec<_>>(),
        );
        for (id, (tabs, selected)) in group_ids.into_iter().zip(groups) {
            let selected_object = &snapshot.objects[&selected];
            let terminals = tabs
                .iter()
                .flat_map(|tab| snapshot.objects[tab].terminals.iter().cloned())
                .collect();
            let window = Object {
                id: id.clone(),
                kind: Kind::Window,
                name: selected_object.name.clone(),
                tabs: tabs.clone(),
                terminals,
                selected: Some(selected),
                focused: selected_object.focused.clone(),
                cwd: String::new(),
                session: String::new(),
                window_id: selected_object.window_id,
                pane_id: None,
                ready: selected_object.ready,
            };
            if tabs
                .iter()
                .any(|tab| Some(snapshot.objects[tab].window_id) == active)
            {
                snapshot.front = Some(id.clone());
            }
            snapshot.windows.push(id.clone());
            snapshot.objects.insert(id, window);
        }
        self.ids.prune(&snapshot);
        self.connections
            .retain(|id, _| snapshot.objects.contains_key(id));
        snapshot
    }
}
