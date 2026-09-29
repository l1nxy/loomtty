//! macOS application shell: one client core/renderer per native window or tab,
//! a single AppKit menu bar, and process-wide input/lifecycle integration.
mod menu;
mod native;

use crossbeam_channel::Receiver;
use loom_config::config::LoomConfig;
use loom_input::action::Action;
use loom_protocol::message::ClientMessage;
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::window::WindowId;

use crate::app::{App, RemoteConnectionConfig, Selection};

#[derive(Clone, Debug)]
enum Command {
    Menu(muda::MenuId),
    Action(Action),
    NewWindow,
    NewTab,
    CloseWindow,
    Reopen,
    Quit,
    SystemQuit,
    Reload,
    SelectAll,
    FontSize(i8),
    NextTab,
    PreviousTab,
    MergeWindows,
    MoveTabToWindow,
    ToggleTabBar,
}

pub(crate) struct MacApplication {
    windows: Vec<App>,
    active: Option<WindowId>,
    proxy: EventLoopProxy<()>,
    commands: Receiver<Command>,
    menu: Option<menu::MenuBar>,
    _delegate: native::Delegate,
    secure_input: native::SecureInput,
    last_config: LoomConfig,
    last_session: String,
    last_remote: Option<RemoteConnectionConfig>,
    launched_sessions: Vec<String>,
    directories: std::collections::HashMap<WindowId, Option<String>>,
    system_terminate_pending: bool,
}

impl MacApplication {
    pub fn new(app: App, proxy: EventLoopProxy<()>) -> Self {
        let (tx, commands) = crossbeam_channel::unbounded();
        let delegate = native::Delegate::install(tx.clone(), proxy.clone());
        let menu_proxy = proxy.clone();
        muda::MenuEvent::set_event_handler(Some(move |event: muda::MenuEvent| {
            let _ = tx.send(Command::Menu(event.id));
            let _ = menu_proxy.send_event(());
        }));
        Self {
            last_config: app.core.config.clone(),
            last_session: app.core.session_name.clone(),
            last_remote: app.core.remote_config.clone(),
            launched_sessions: vec![app.core.session_name.clone()],
            directories: std::collections::HashMap::new(),
            system_terminate_pending: false,
            windows: vec![app],
            active: None,
            proxy,
            commands,
            menu: None,
            _delegate: delegate,
            secure_input: native::SecureInput::default(),
        }
    }

    /// Complete a Dock/logout termination after winit releases its event
    /// handler. Replying inside user_event synchronously calls AppKit's
    /// applicationWillTerminate and would re-enter winit's borrowed handler.
    pub fn finish_termination(&self) {
        if self.system_terminate_pending {
            let mtm = objc2::MainThreadMarker::new().expect("macOS main thread");
            let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
            // winit already delivered exiting and released every window.
            app.setDelegate(None);
            app.replyToApplicationShouldTerminate(true);
        }
    }

    fn active_index(&self) -> Option<usize> {
        self.active.and_then(|id| {
            self.windows
                .iter()
                .position(|app| app.window.as_ref().is_some_and(|window| window.id() == id))
        })
    }

    fn create_window(&mut self, event_loop: &ActiveEventLoop, tab: bool, reopen: bool) {
        let parent = self.active_index().map(|i| &self.windows[i]);
        let mut config = LoomConfig::load().unwrap_or_else(|_| self.last_config.clone());
        crate::sanitize_remote_hosts(&mut config);
        let remote = parent
            .map(|app| app.core.remote_config.clone())
            .unwrap_or_else(|| self.last_remote.clone());
        let name = if reopen {
            self.last_session.clone()
        } else {
            let mut names = self.launched_sessions.clone();
            for app in &self.windows {
                names.extend(
                    app.core
                        .cached_local_sessions
                        .iter()
                        .map(|s| s.name.clone()),
                );
            }
            loom_session::names::unique_name(&names)
        };
        let parent_window = parent.and_then(|app| app.window.clone());
        let mut app = App::new(config, name.clone());
        app.event_loop_proxy = Some(self.proxy.clone());
        app.core.remote_config = remote;
        app.core.recent_hosts = crate::recent_hosts::load();
        app.resumed(event_loop);
        if let Some(window) = &app.window {
            if tab && let Some(parent) = parent_window {
                native::join_tab(&parent, window);
            }
            self.active = Some(window.id());
        }
        self.launched_sessions.push(name);
        self.windows.push(app);
    }

    fn close_window(&mut self, id: WindowId, event_loop: &ActiveEventLoop) {
        let Some(index) = self
            .windows
            .iter()
            .position(|app| app.window.as_ref().is_some_and(|window| window.id() == id))
        else {
            return;
        };
        let mut app = self.windows.remove(index);
        self.directories.remove(&id);
        self.last_config = app.core.config.clone();
        self.last_session = app.core.session_name.clone();
        self.last_remote = app.core.remote_config.clone();
        // A red close button detaches the client. It never closes PTYs or
        // kills a server session. Drop GPU resources before their NSWindow.
        app.send(ClientMessage::Detach);
        if let Some(cancel) = app.connection_cancel.take() {
            cancel.notify_one();
        }
        app.destroy_gpu_resources();
        app.renderer = None;
        app.window = None;
        if self.active == Some(id) {
            self.active = self
                .windows
                .last()
                .and_then(|app| app.window.as_ref())
                .map(|w| w.id());
        }
        self.secure_input.update(false);
        if self.windows.is_empty() && self.last_config.window.macos_quit_after_last_window_closed {
            event_loop.exit();
        }
    }

    fn prune_closed(&mut self, event_loop: &ActiveEventLoop) {
        let closed: Vec<_> = self
            .windows
            .iter()
            .filter(|app| app.core.should_exit)
            .filter_map(|app| app.window.as_ref().map(|w| w.id()))
            .collect();
        for id in closed {
            self.close_window(id, event_loop);
        }
    }

    fn dispatch(&mut self, command: Command, event_loop: &ActiveEventLoop) {
        match command {
            Command::Menu(id) => {
                if let Some(command) = self.menu.as_ref().and_then(|menu| menu.command(&id)) {
                    self.dispatch(command, event_loop);
                }
            }
            Command::NewWindow | Command::NewTab => {
                self.create_window(event_loop, matches!(command, Command::NewTab), false);
            }
            Command::Reopen => {
                if self.windows.is_empty() {
                    self.create_window(event_loop, false, true);
                } else if let Some(i) = self.active_index()
                    && let Some(window) = &self.windows[i].window
                {
                    window.set_minimized(false);
                    window.set_visible(true);
                    window.focus_window();
                }
            }
            Command::CloseWindow => {
                if let Some(id) = self.active {
                    self.close_window(id, event_loop);
                }
            }
            Command::Quit | Command::SystemQuit => {
                let ids: Vec<_> = self
                    .windows
                    .iter()
                    .filter_map(|app| app.window.as_ref().map(|w| w.id()))
                    .collect();
                for id in ids {
                    self.close_window(id, event_loop);
                }
                self.secure_input.update(false);
                event_loop.exit();
                self.system_terminate_pending = matches!(command, Command::SystemQuit);
            }
            Command::Reload => {
                for app in &mut self.windows {
                    app.reload_config();
                    app.schedule_redraw();
                }
                if let Ok(config) = LoomConfig::load() {
                    self.last_config = config;
                }
            }
            Command::Action(Action::ToggleSettings | Action::ToggleHelp)
                if self.windows.is_empty() =>
            {
                self.create_window(event_loop, false, true);
                self.dispatch(command, event_loop);
            }
            _ => {
                let Some(i) = self.active_index() else { return };
                let app = &mut self.windows[i];
                match command {
                    Command::Action(action) => app.handle_action(action),
                    Command::SelectAll if !app.modal_captures_keyboard() => {
                        if let Some(pid) = app.core.workspaces.active().active_pane_id()
                            && let Some(grid) = app.core.pane_grids.get(&pid)
                        {
                            app.core.selection = Some(Selection {
                                pane_id: pid,
                                start: (0, 0),
                                end: (
                                    grid.cols.saturating_sub(1),
                                    grid.total_lines().saturating_sub(1),
                                ),
                                active: false,
                            });
                        }
                    }
                    Command::FontSize(delta) => {
                        let size = if delta == 0 {
                            LoomConfig::load()
                                .map(|c| c.font.size)
                                .unwrap_or(self.last_config.font.size)
                        } else {
                            (app.core.config.font.size + delta as f32).clamp(1.0, 200.0)
                        };
                        app.core.config.font.size = size;
                        app.apply_font_config_change();
                    }
                    Command::MergeWindows | Command::MoveTabToWindow | Command::ToggleTabBar => {
                        if let Some(window) =
                            app.window.as_ref().and_then(|w| native::native_window(w))
                        {
                            match command {
                                Command::MergeWindows => window.mergeAllWindows(None),
                                Command::MoveTabToWindow => window.moveTabToNewWindow(None),
                                _ => window.toggleTabBar(None),
                            }
                        }
                    }
                    Command::NextTab | Command::PreviousTab => {
                        if let Some(window) = &app.window {
                            native::select_tab(window, matches!(command, Command::NextTab));
                        }
                    }
                    _ => {}
                }
                app.schedule_redraw();
            }
        }
    }

    fn update_native_state(&mut self) {
        // Ask AppKit which tab is key; native tab switching and window-menu
        // selection can change it without going through a loom action.
        let focused = self.windows.iter().position(|app| {
            app.window
                .as_ref()
                .and_then(|window| native::native_window(window))
                .is_some_and(|window| window.isKeyWindow())
        });
        if let Some(i) = focused {
            self.active = self.windows[i].window.as_ref().map(|w| w.id());
        }
        let mtm = objc2::MainThreadMarker::new().expect("macOS main thread");
        let application_active = objc2_app_kit::NSApplication::sharedApplication(mtm).isActive();
        let password = focused.is_some_and(|i| {
            let app = &self.windows[i];
            application_active
                && app.window_focused
                && app.core.config.window.macos_secure_input
                && !app.modal_captures_keyboard()
                && app
                    .core
                    .workspaces
                    .active()
                    .active_pane_id()
                    .and_then(|pid| app.core.pane_grids.get(&pid))
                    .is_some_and(|grid| grid.password_input)
        });
        self.secure_input.update(password);
        for app in &self.windows {
            if let Some(window) = &app.window {
                // A remote cwd must never be represented as a local file URL.
                let cwd = if app.core.remote_config.is_none() {
                    app.core
                        .workspaces
                        .active()
                        .active_pane_id()
                        .and_then(|pid| app.core.pane_grids.get(&pid))
                        .and_then(|grid| grid.cwd.clone())
                } else {
                    None
                };
                if self.directories.get(&window.id()) != Some(&cwd) {
                    native::set_directory(window, cwd.as_deref());
                    self.directories.insert(window.id(), cwd);
                }
            }
        }
        let index = self.active_index();
        let app = index.map(|i| &self.windows[i]);
        if let Some(menu) = &mut self.menu {
            menu.update(
                &app.map(|app| &app.core.config)
                    .unwrap_or(&self.last_config)
                    .keys,
                app.is_some(),
                app.is_some_and(|app| app.core.selection.is_some()),
                app.is_some_and(|app| app.modal_captures_keyboard()),
            );
        }
    }
}

// Each window computes its next deadline. Combining them prevents the last
// idle window from putting an animating sibling to sleep (or busy polling).
fn merge_control_flow(a: ControlFlow, b: ControlFlow) -> ControlFlow {
    match (a, b) {
        (ControlFlow::Poll, _) | (_, ControlFlow::Poll) => ControlFlow::Poll,
        (ControlFlow::Wait, b) => b,
        (a, ControlFlow::Wait) => a,
        (ControlFlow::WaitUntil(a), ControlFlow::WaitUntil(b)) => ControlFlow::WaitUntil(a.min(b)),
    }
}

impl ApplicationHandler for MacApplication {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.menu = Some(menu::MenuBar::new().expect("create macOS menu bar"));
        for app in &mut self.windows {
            app.resumed(event_loop);
            self.active = app.window.as_ref().map(|w| w.id());
        }
        self.update_native_state();
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        self.prune_closed(event_loop);
        let mut flow = ControlFlow::Wait;
        for app in &mut self.windows {
            app.new_events(event_loop, cause);
            flow = merge_control_flow(flow, event_loop.control_flow());
        }
        event_loop.set_control_flow(flow);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: ()) {
        // All IO connections share a proxy; draining every window is cheap
        // and ensures a background tab's output never starves the active one.
        for app in &mut self.windows {
            app.user_event(event_loop, event);
        }
        self.update_native_state();
        while let Ok(command) = self.commands.try_recv() {
            self.dispatch(command, event_loop);
            if event_loop.exiting() {
                break;
            }
        }
        self.prune_closed(event_loop);
        self.update_native_state();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if matches!(event, WindowEvent::CloseRequested) {
            self.close_window(id, event_loop);
        } else if let Some(app) = self
            .windows
            .iter_mut()
            .find(|app| app.window.as_ref().is_some_and(|w| w.id() == id))
        {
            if matches!(event, WindowEvent::Focused(true)) {
                self.active = Some(id);
            }
            app.window_event(event_loop, id, event);
        }
        self.update_native_state();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        for app in &mut self.windows {
            app.about_to_wait(event_loop);
        }
        self.update_native_state();
    }

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        let ids: Vec<_> = self
            .windows
            .iter()
            .filter_map(|app| app.window.as_ref().map(|w| w.id()))
            .collect();
        for id in ids {
            self.close_window(id, event_loop);
        }
        self.secure_input.update(false);
    }
}

/// Launch Services starts bundled applications in `/`. Preserve explicit CLI
/// working directories while making a Finder launch start in the user's home.
pub(crate) fn prepare_launch_directory() {
    let bundled = std::env::current_exe().ok().is_some_and(|exe| {
        exe.parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .is_some_and(|p| p.extension().is_some_and(|ext| ext == "app"))
    });
    if bundled
        && std::env::current_dir().ok().as_deref() == Some(std::path::Path::new("/"))
        && let Some(home) = std::env::var_os("HOME")
        && let Err(error) = std::env::set_current_dir(home)
    {
        log::warn!("could not set Finder launch directory: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn idle_window_cannot_delay_a_sibling_deadline() {
        let now = Instant::now();
        let soon = ControlFlow::WaitUntil(now);
        let later = ControlFlow::WaitUntil(now + Duration::from_secs(1));
        assert_eq!(merge_control_flow(soon, ControlFlow::Wait), soon);
        assert_eq!(merge_control_flow(ControlFlow::Wait, soon), soon);
        assert_eq!(merge_control_flow(later, soon), soon);
        assert_eq!(
            merge_control_flow(soon, ControlFlow::Poll),
            ControlFlow::Poll
        );
    }
}
