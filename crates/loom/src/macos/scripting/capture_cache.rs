//! Exact inputs to the automation object tree. Terminal cell changes do not
//! change that tree; compare borrowed metadata before rebuilding its owned IDs,
//! strings and maps. No hash collisions or time-based staleness are allowed.
use super::Source;
use crate::app::App;
use crossbeam_channel::Sender;
use loom_protocol::message::ClientMessage;
use objc2::rc::Retained;
use objc2_app_kit::NSWindow;
use objc2_foundation::{NSCopying, NSString};
use winit::window::{Window, WindowId};

pub(super) struct CaptureState {
    enabled: bool,
    active: Option<WindowId>,
    windows: Vec<WindowState>,
}

struct WindowState {
    id: WindowId,
    title: Retained<NSString>,
    tabs: Vec<isize>,
    selected: Option<isize>,
    app: AppState,
}

struct AppState {
    session: String,
    remote: Option<(String, u16, u16)>,
    sender: Option<Sender<ClientMessage>>,
    connected: bool,
    pending: bool,
    focused: Option<u64>,
    panes: Vec<(u64, String, String)>,
}

fn windows(
    apps: &[App],
    source: Source,
) -> impl Iterator<Item = (&App, &Window, Retained<NSWindow>)> {
    apps.iter()
        .filter(move |app| !app.native_quick_terminal && source.enabled(&app.core.config.window))
        .filter_map(|app| {
            let window = app.window.as_ref()?;
            Some((
                app,
                window.as_ref(),
                super::super::native::native_window(window)?,
            ))
        })
}

fn panes(app: &App) -> impl Iterator<Item = (u64, &str, &str)> {
    app.core
        .workspaces
        .workspaces
        .iter()
        .flat_map(|workspace| &workspace.columns)
        .flat_map(|column| &column.tiles)
        .filter(|_| app.core.connected && app.core.pending_session_name.is_none())
        .filter_map(|tile| {
            let grid = app.core.pane_grids.get(&tile.pane_id)?;
            Some((
                tile.pane_id,
                grid.title.as_str(),
                grid.cwd.as_deref().unwrap_or_default(),
            ))
        })
}

impl AppState {
    fn capture(app: &App) -> Self {
        Self {
            session: app.core.session_name.clone(),
            remote: app
                .core
                .remote_config
                .as_ref()
                .map(|r| (r.host.clone(), r.port, r.ssh_port)),
            sender: app.core.server_tx.clone(),
            connected: app.core.connected,
            pending: app.core.pending_session_name.is_some(),
            focused: app.core.workspaces.active().active_pane_id(),
            panes: panes(app)
                .map(|(id, title, cwd)| (id, title.to_owned(), cwd.to_owned()))
                .collect(),
        }
    }

    fn matches(&self, app: &App) -> bool {
        self.session == app.core.session_name
            && self
                .remote
                .as_ref()
                .map(|(host, port, ssh)| (host.as_str(), *port, *ssh))
                == app
                    .core
                    .remote_config
                    .as_ref()
                    .map(|r| (r.host.as_str(), r.port, r.ssh_port))
            && match (&self.sender, &app.core.server_tx) {
                (Some(a), Some(b)) => a.same_channel(b),
                (None, None) => true,
                _ => false,
            }
            && self.connected == app.core.connected
            && self.pending == app.core.pending_session_name.is_some()
            && self.focused == app.core.workspaces.active().active_pane_id()
            && self
                .panes
                .iter()
                .map(|(id, title, cwd)| (*id, title.as_str(), cwd.as_str()))
                .eq(panes(app))
    }
}

impl CaptureState {
    pub(super) fn capture(
        apps: &[App],
        active: Option<WindowId>,
        enabled: bool,
        source: Source,
    ) -> Self {
        Self {
            enabled,
            active,
            windows: windows(apps, source)
                .map(|(app, window, native)| {
                    let group = native.tabGroup();
                    WindowState {
                        id: window.id(),
                        // Copy rather than retain a potentially mutable title.
                        title: native.title().copy(),
                        tabs: group
                            .as_ref()
                            .map(|g| g.windows().iter().map(|w| w.windowNumber()).collect())
                            .unwrap_or_default(),
                        selected: group
                            .and_then(|g| g.selectedWindow())
                            .map(|w| w.windowNumber()),
                        app: AppState::capture(app),
                    }
                })
                .collect(),
        }
    }

    pub(super) fn matches(
        &self,
        apps: &[App],
        active: Option<WindowId>,
        enabled: bool,
        source: Source,
    ) -> bool {
        if self.enabled != enabled || self.active != active {
            return false;
        }
        let mut previous = self.windows.iter();
        for (app, window, native) in windows(apps, source) {
            let Some(saved) = previous.next() else {
                return false;
            };
            if saved.id != window.id()
                || !saved.app.matches(app)
                || !saved.title.isEqualToString(&native.title())
            {
                return false;
            }
            // AppKit can change native tab order/selection without a loom
            // command. Observe it even when all terminal metadata is stable.
            let group = native.tabGroup();
            let tabs = group.as_ref().map(|g| g.windows());
            if !saved.tabs.iter().copied().eq(tabs
                .iter()
                .flat_map(|tabs| tabs.iter())
                .map(|w| w.windowNumber()))
                || saved.selected
                    != group
                        .and_then(|g| g.selectedWindow())
                        .map(|w| w.windowNumber())
            {
                return false;
            }
        }
        previous.next().is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::ClientPaneGrid;
    use loom_layout::column::ColumnWidth;

    fn app() -> App {
        let mut app = App::new(loom_config::LoomConfig::default(), "test");
        app.core.connected = true;
        app.core.server_tx = Some(crossbeam_channel::unbounded().0);
        for id in [1, 2] {
            app.core
                .workspaces
                .active_mut()
                .add_column_right(id, ColumnWidth::default());
            app.core.pane_grids.insert(id, ClientPaneGrid::new(8, 4, 0));
        }
        app
    }

    #[test]
    fn cell_cursor_and_scroll_updates_do_not_rebuild_automation_objects() {
        let mut app = app();
        let state = AppState::capture(&app);
        let grid = app.core.pane_grids.get_mut(&1).unwrap();
        grid.viewport[0].set_ch('中');
        grid.cursor_col = 3;
        grid.mark_row_dirty(0);
        grid.scroll_offset = 1;
        assert!(state.matches(&app));
        app.core.pane_grids.get_mut(&1).unwrap().title = "nvim".into();
        assert!(!state.matches(&app));
        let state = AppState::capture(&app);
        app.core.pane_grids.get_mut(&1).unwrap().cwd = Some("/tmp/project".into());
        assert!(!state.matches(&app));
    }

    #[test]
    fn connection_and_layout_changes_invalidate_before_commands_reuse_ids() {
        let mut app = app();
        let state = AppState::capture(&app);
        app.core.server_tx = Some(crossbeam_channel::unbounded().0);
        assert!(!state.matches(&app));
        let state = AppState::capture(&app);
        app.core.connected = false;
        assert!(!state.matches(&app));
        app.core.connected = true;
        let state = AppState::capture(&app);
        app.core.pending_session_name = Some("next".into());
        assert!(!state.matches(&app));
        app.core.pending_session_name = None;
        let state = AppState::capture(&app);
        app.core.session_name = "next".into();
        assert!(!state.matches(&app));
        let state = AppState::capture(&app);
        app.core.workspaces.active_mut().focus_column(0);
        assert!(!state.matches(&app));
        let state = AppState::capture(&app);
        app.core.workspaces.active_mut().columns.swap(0, 1);
        assert!(!state.matches(&app));
        let state = AppState::capture(&app);
        app.core.pane_grids.remove(&1);
        assert!(!state.matches(&app));
    }
}
