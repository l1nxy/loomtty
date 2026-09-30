//! macOS application shell: one client core/renderer per native window or tab,
//! a single AppKit menu bar, and process-wide input/lifecycle integration.
mod accessibility;
pub(crate) mod context_menu;
mod lookup;
mod menu;
mod native;
mod restore;
mod scripting;
mod services;
mod text_selection;
mod text_services;
pub(crate) use native::system_dark_appearance;
pub(crate) use scripting::creation_result as scripting_result;
pub(crate) use scripting::metadata as app_intents_metadata;
pub(crate) mod quick_terminal;

use crossbeam_channel::Receiver;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager};
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
    ContextMenu(context_menu::Request),
    ServiceText(text_services::Request),
    Accessibility(accessibility::Action),
    AccessibilityRefresh,
    Script(u64),
    Menu(muda::MenuId),
    Action(Action),
    NewWindow,
    NewTab,
    OpenDirectory {
        directory: std::path::PathBuf,
        tab: bool,
    },
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
    ToggleQuickTerminal,
    ToggleFullscreen,
    ToggleSecureInput,
    LookUp,
    LookUpAt {
        id: WindowId,
        point: objc2_foundation::NSPoint,
    },
    Hotkey(GlobalHotKeyEvent),
}

pub(crate) struct MacApplication {
    windows: Vec<App>,
    active: Option<WindowId>,
    proxy: EventLoopProxy<()>,
    commands: Receiver<Command>,
    menu: Option<menu::MenuBar>,
    _delegate: native::Delegate,
    secure_input: native::SecureInput,
    manual_secure_input: bool,
    view_hooks: std::collections::HashMap<WindowId, lookup::ViewHooks>,
    last_config: LoomConfig,
    last_session: String,
    last_remote: Option<RemoteConnectionConfig>,
    launched_sessions: Vec<String>,
    directories: std::collections::HashMap<WindowId, Option<String>>,
    system_terminate_pending: bool,
    quick_terminal: Option<quick_terminal::QuickTerminal>,
    show_initial_window: bool,
    explicit_window: bool,
    restoration: Option<restore::Store>,
    pending_restore: Option<restore::Snapshot>,
    normal_frames: std::collections::HashMap<WindowId, restore::Frame>,
    hotkey_manager: Option<GlobalHotKeyManager>,
    hotkey_registration: quick_terminal::hotkey::Registration,
    scripting: scripting::Bridge,
    intents: scripting::Bridge,
    accessibility: accessibility::Bridge,
}

impl MacApplication {
    pub fn new(app: App, proxy: EventLoopProxy<()>, explicit_window: bool) -> Self {
        let (tx, commands) = crossbeam_channel::unbounded();
        scripting::configure(app.core.config.window.macos_applescript);
        scripting::start_intents(app.core.config.window.macos_app_intents);
        let delegate = native::Delegate::install(tx.clone(), proxy.clone());
        let menu_proxy = proxy.clone();
        let hotkey_proxy = proxy.clone();
        let hotkey_tx = tx.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            let _ = hotkey_tx.send(Command::Hotkey(event));
            let _ = hotkey_proxy.send_event(());
        }));
        muda::MenuEvent::set_event_handler(Some(move |event: muda::MenuEvent| {
            let command = context_menu::command(&event.id)
                .map(Command::ContextMenu)
                .unwrap_or(Command::Menu(event.id));
            let _ = tx.send(command);
            let _ = menu_proxy.send_event(());
        }));
        let restoration = if !explicit_window
            && app.core.config.window.macos_initial_window
            && app.core.config.window.macos_restore_windows
        {
            match restore::Store::open(&loom_protocol::transport::state_dir()) {
                Ok(store) => store,
                Err(error) => {
                    log::warn!("could not open macOS window restoration: {error}");
                    None
                }
            }
        } else {
            None
        };
        let pending_restore = restoration
            .as_ref()
            .and_then(|store| store.loaded())
            .filter(|state| !state.groups.is_empty());
        Self {
            show_initial_window: explicit_window || app.core.config.window.macos_initial_window,
            explicit_window,
            restoration,
            pending_restore,
            normal_frames: std::collections::HashMap::new(),
            last_config: app.core.config.clone(),
            last_session: app.core.session_name.clone(),
            last_remote: app.core.remote_config.clone(),
            launched_sessions: vec![app.core.session_name.clone()],
            directories: std::collections::HashMap::new(),
            system_terminate_pending: false,
            quick_terminal: None,
            hotkey_manager: match GlobalHotKeyManager::new() {
                Ok(manager) => Some(manager),
                Err(error) => {
                    log::warn!("global hotkey initialization failed: {error}");
                    None
                }
            },
            hotkey_registration: quick_terminal::hotkey::Registration::default(),
            windows: vec![app],
            active: None,
            proxy,
            commands,
            menu: None,
            _delegate: delegate,
            secure_input: native::SecureInput::default(),
            manual_secure_input: false,
            scripting: scripting::Bridge::default(),
            intents: scripting::Bridge::default(),
            accessibility: accessibility::Bridge::default(),
            view_hooks: std::collections::HashMap::new(),
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
            self.windows.iter().position(|app| {
                app.window.as_ref().is_some_and(|window| window.id() == id)
                    && (!app.native_quick_terminal
                        || self
                            .quick_terminal
                            .as_ref()
                            .is_some_and(|quick| quick.visible))
            })
        })
    }

    fn create_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        tab: bool,
        reopen: bool,
        directory: Option<std::path::PathBuf>,
    ) {
        let parent = self.active_index().map(|i| &self.windows[i]);
        let mut config = LoomConfig::load().unwrap_or_else(|_| self.last_config.clone());
        crate::sanitize_remote_hosts(&mut config);
        let remote = if directory.is_some() {
            None
        } else {
            parent
                .map(|app| app.core.remote_config.clone())
                .unwrap_or_else(|| self.last_remote.clone())
        };
        let cwd = directory
            .or_else(|| {
                parent
                    .filter(|app| app.core.remote_config.is_none())
                    .and_then(|app| {
                        app.core
                            .workspaces
                            .active()
                            .active_pane_id()
                            .and_then(|id| app.core.pane_grids.get(&id))
                    })
                    .and_then(|grid| grid.cwd.as_ref())
                    .map(std::path::PathBuf::from)
                    .filter(|path| path.is_dir())
            })
            .or_else(|| std::env::current_dir().ok());
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
            if remote.is_none()
                && let Some(cwd) = cwd
            {
                match services::seed_session(
                    &loom_protocol::transport::state_dir(),
                    &names,
                    &cwd,
                    &config,
                ) {
                    Ok(name) => name,
                    Err(error) => {
                        log::error!("could not create terminal in {}: {error}", cwd.display());
                        return;
                    }
                }
            } else {
                loom_session::names::unique_name(&names)
            }
        };
        let parent_window = parent
            .filter(|app| !app.native_quick_terminal)
            .and_then(|app| app.window.clone());
        let mut app = App::new(config, name.clone());
        app.event_loop_proxy = Some(self.proxy.clone());
        app.core.remote_config = remote;
        app.core.recent_hosts = crate::recent_hosts::load();
        app.resumed(event_loop);
        let window = app.window.clone();
        if let Some(window) = &window {
            if tab && let Some(parent) = parent_window {
                native::join_tab(&parent, window);
            }
            self.active = Some(window.id());
        }
        self.launched_sessions.push(name);
        self.windows.push(app);
        if let Some(window) = window {
            native::focus_window(&window);
        }
    }

    fn close_window(&mut self, id: WindowId, event_loop: &ActiveEventLoop) {
        context_menu::cancel(id);
        let Some(index) = self
            .windows
            .iter()
            .position(|app| app.window.as_ref().is_some_and(|window| window.id() == id))
        else {
            return;
        };
        let mut app = self.windows.remove(index);
        self.accessibility.remove(id);
        self.directories.remove(&id);
        self.view_hooks.remove(&id);
        self.normal_frames.remove(&id);
        if app.native_quick_terminal {
            self.quick_terminal = None;
        } else {
            self.last_config = app.core.config.clone();
            self.last_session = app.core.session_name.clone();
            self.last_remote = app.core.remote_config.clone();
        }
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
        if self.windows.iter().all(|app| app.native_quick_terminal)
            && self.last_config.window.macos_quit_after_last_window_closed
        {
            self.save_restoration(true);
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
            Command::ContextMenu(request) => self.handle_context_menu(request),
            Command::ServiceText(request) => self.handle_service_text(request),
            Command::Accessibility(action) => self.handle_accessibility(action),
            Command::AccessibilityRefresh => {}
            Command::Script(token) => self.execute_script(token, event_loop),
            Command::Menu(id) => {
                if let Some(command) = self.menu.as_ref().and_then(|menu| menu.command(&id)) {
                    self.dispatch(command, event_loop);
                }
            }
            Command::ToggleSecureInput => {
                self.manual_secure_input = !self.manual_secure_input;
            }
            Command::LookUp => {
                if let Some(id) = self.active
                    && let Some(point) = self.view_hooks.get(&id).and_then(|view| view.pointer())
                {
                    self.look_up(id, point, true);
                }
            }
            Command::LookUpAt { id, point } => self.look_up(id, point, false),
            Command::ToggleFullscreen => {
                let Some(i) = self.active_index() else {
                    return;
                };
                let app = &self.windows[i];
                if let Some(window) = &app.window {
                    if app.native_quick_terminal {
                        if let Some(quick) = &mut self.quick_terminal {
                            quick.toggle_fullscreen(
                                window,
                                &app.core.config.window.macos_quick_terminal,
                            );
                        }
                    } else {
                        window.set_fullscreen(if window.fullscreen().is_some() {
                            None
                        } else {
                            Some(winit::window::Fullscreen::Borderless(None))
                        });
                    }
                }
                self.advance_quick_terminal();
            }
            Command::Hotkey(event) => {
                if self.hotkey_registration.pressed(event) {
                    self.toggle_quick_terminal(event_loop);
                }
            }
            Command::ToggleQuickTerminal => self.toggle_quick_terminal(event_loop),
            Command::NewWindow | Command::NewTab => {
                self.create_window(event_loop, matches!(command, Command::NewTab), false, None);
            }
            Command::OpenDirectory { directory, tab } => {
                self.create_window(event_loop, tab, false, Some(directory));
            }
            Command::Reopen => {
                let normal = self
                    .active_index()
                    .filter(|i| !self.windows[*i].native_quick_terminal)
                    .or_else(|| {
                        self.windows
                            .iter()
                            .position(|app| !app.native_quick_terminal)
                    });
                if let Some(i) = normal {
                    if let Some(window) = &self.windows[i].window {
                        native::focus_window(window);
                        self.active = Some(window.id());
                    }
                } else {
                    self.create_window(event_loop, false, true, None);
                }
            }
            Command::CloseWindow => {
                if let Some(id) = self.active {
                    if self
                        .quick_terminal
                        .as_ref()
                        .is_some_and(|quick| quick.id == id)
                    {
                        self.hide_quick_terminal(true);
                    } else {
                        self.close_window(id, event_loop);
                    }
                }
            }
            Command::Quit | Command::SystemQuit => {
                // Capture before teardown; closing windows during Quit must not
                // overwrite the snapshot with an empty application.
                self.save_restoration(true);
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
                if self.active_index().is_none() =>
            {
                self.create_window(event_loop, false, true, None);
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
                    Command::MergeWindows | Command::MoveTabToWindow | Command::ToggleTabBar
                        if !app.native_quick_terminal =>
                    {
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
                    Command::NextTab | Command::PreviousTab if !app.native_quick_terminal => {
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

    fn save_restoration(&mut self, quitting: bool) {
        if self.restoration.as_ref().is_none_or(|store| store.frozen) {
            return;
        }
        let normal_active = self
            .active
            .filter(|id| {
                self.windows.iter().any(|app| {
                    !app.native_quick_terminal
                        && app.window.as_ref().is_some_and(|window| window.id() == *id)
                })
            })
            .or_else(|| {
                self.quick_terminal
                    .as_ref()
                    .and_then(|quick| quick.previous_window)
            });
        let snapshot = restore::capture(&self.windows, normal_active, &mut self.normal_frames);
        if let Some(store) = &mut self.restoration {
            store.observe(snapshot, std::time::Instant::now());
            store.flush(quitting);
            store.frozen = quitting;
        }
    }

    fn next_native_deadline(&self) -> Option<std::time::Instant> {
        let quick = self
            .quick_terminal
            .as_ref()
            .and_then(|quick| quick.next_frame(std::time::Instant::now()));
        let save = self.restoration.as_ref().and_then(|store| store.deadline());
        quick
            .into_iter()
            .chain(save)
            .chain(self.scripting.deadline())
            .chain(self.intents.deadline())
            .chain(self.accessibility.deadline())
            .min()
    }

    fn restore_windows(&mut self, snapshot: restore::Snapshot, event_loop: &ActiveEventLoop) {
        use objc2::{MainThreadMarker, rc::Retained};
        use objc2_app_kit::{NSScreen, NSWindow, NSWindowOrderingMode};
        use std::sync::Arc;
        use winit::window::Window;
        let mtm = MainThreadMarker::new().expect("macOS main thread");
        let screens: Vec<_> = NSScreen::screens(mtm)
            .iter()
            .map(|screen| restore::Frame::from(screen.visibleFrame()))
            .collect();
        let initial = std::mem::take(&mut self.windows);
        let mut selected_windows = Vec::new();
        for (group_index, group) in snapshot.groups.into_iter().enumerate() {
            let frame = group.frame.fit(&screens);
            let mut parent: Option<Retained<NSWindow>> = None;
            let mut selected: Option<Arc<Window>> = None;
            for (tab_index, tab) in group.tabs.into_iter().enumerate() {
                if tab.remote.is_none()
                    && quick_terminal::session::is_quick_session(
                        &loom_protocol::transport::state_dir(),
                        &tab.session,
                    )
                {
                    continue;
                }
                let mut app = App::new(self.last_config.clone(), tab.session.clone());
                app.native_initially_hidden = true;
                app.event_loop_proxy = Some(self.proxy.clone());
                app.core.remote_config = tab.remote.map(|remote| RemoteConnectionConfig {
                    host: remote.host,
                    port: remote.port,
                    ssh_port: remote.ssh_port,
                });
                app.core.recent_hosts = crate::recent_hosts::load();
                app.resumed(event_loop);
                if let Some(window) = &app.window
                    && let Some(native) = native::native_window(window)
                {
                    native.setFrame_display((&frame).into(), false);
                    self.normal_frames.insert(window.id(), frame.clone());
                    if let Some(parent) = &parent {
                        parent.addTabbedWindow_ordered(&native, NSWindowOrderingMode::Above);
                    } else {
                        parent = Some(native);
                    }
                    if selected.is_none() || tab_index == group.selected {
                        selected = Some(window.clone());
                    }
                }
                self.launched_sessions.push(tab.session);
                self.windows.push(app);
            }
            if let Some(window) = selected
                && let Some(native) = native::native_window(&window)
            {
                if let Some(tabs) = native.tabGroup() {
                    tabs.setSelectedWindow(Some(&native));
                }
                window.set_visible(true);
                if group.zoomed && !group.fullscreen {
                    native.zoom(None);
                }
                if group.fullscreen {
                    window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
                }
                if group.minimized && !group.fullscreen {
                    window.set_minimized(true);
                }
                selected_windows.push((group_index, window, group.minimized));
            }
        }
        if self.windows.is_empty() {
            self.windows = initial;
            for app in &mut self.windows {
                app.resumed(event_loop);
            }
        } else if let Some((_, window, _)) = selected_windows
            .iter()
            .find(|(index, _, minimized)| Some(*index) == snapshot.active_group && !minimized)
            .or_else(|| selected_windows.iter().find(|(_, _, minimized)| !minimized))
        {
            native::focus_window(window);
            self.active = Some(window.id());
        }
    }

    fn look_up(&mut self, id: WindowId, point: objc2_foundation::NSPoint, selection: bool) {
        let Some(app) = self.windows.iter().find(|app| {
            app.window_focused && app.window.as_ref().is_some_and(|window| window.id() == id)
        }) else {
            return;
        };
        let Some(text) = lookup::text_at(app, point, selection) else {
            return;
        };
        let point = if selection {
            lookup::selection_point(app).unwrap_or(point)
        } else {
            point
        };
        if let Some(view) = self.view_hooks.get_mut(&id)
            && view.allow_lookup()
        {
            view.show(&text, point);
        }
    }

    fn toggle_quick_terminal(&mut self, event_loop: &ActiveEventLoop) {
        if self
            .quick_terminal
            .as_ref()
            .is_some_and(|quick| quick.visible)
        {
            self.hide_quick_terminal(true);
            return;
        }
        let previous_application = quick_terminal::QuickTerminal::frontmost_application();
        let previous_window = self
            .active_index()
            .and_then(|i| self.windows[i].window.as_ref().map(|w| w.id()));
        if self.quick_terminal.is_none() {
            let mut config = LoomConfig::load().unwrap_or_else(|_| self.last_config.clone());
            crate::sanitize_remote_hosts(&mut config);
            let mut names = self.launched_sessions.clone();
            for app in &self.windows {
                names.extend(
                    app.core
                        .cached_local_sessions
                        .iter()
                        .map(|session| session.name.clone()),
                );
            }
            let state_dir = loom_protocol::transport::state_dir();
            names.extend(loom_session::restore::list_sessions(&state_dir).unwrap_or_default());
            let name = match quick_terminal::session::allocate(&state_dir, &names) {
                Ok(name) => name,
                Err(error) => {
                    log::error!("could not allocate Quick Terminal session: {error}");
                    return;
                }
            };
            let mut app = App::new(config, name.clone());
            app.event_loop_proxy = Some(self.proxy.clone());
            app.native_quick_terminal = true;
            app.core.remembers_last_session = false;
            app.core.recent_hosts = crate::recent_hosts::load();
            app.resumed(event_loop);
            let Some(window) = &app.window else {
                return;
            };
            self.quick_terminal = Some(quick_terminal::QuickTerminal::new(
                window,
                &app.core.config.window.macos_quick_terminal,
            ));
            self.launched_sessions.push(name);
            self.windows.push(app);
        }
        let quick = self
            .quick_terminal
            .as_mut()
            .expect("created Quick Terminal");
        let Some(app) = self.windows.iter_mut().find(|app| {
            app.window
                .as_ref()
                .is_some_and(|window| window.id() == quick.id)
        }) else {
            return;
        };
        if let Some(window) = &app.window {
            quick.show(
                window,
                &app.core.config.window.macos_quick_terminal,
                previous_window,
                previous_application,
            );
            self.active = Some(quick.id);
            // Reversing a hide can reuse an NSWindow that is still key, so
            // AppKit may not emit another Focused(true) event for this show.
            let mtm = objc2::MainThreadMarker::new().expect("macOS main thread");
            if !app.window_focused
                && objc2_app_kit::NSApplication::sharedApplication(mtm).isActive()
                && native::native_window(window).is_some_and(|window| window.isKeyWindow())
            {
                app.handle_window_focus_changed(true);
            }
            app.schedule_redraw();
        }
        self.advance_quick_terminal();
    }

    fn hide_quick_terminal(&mut self, restore_focus: bool) {
        let Some(quick) = &mut self.quick_terminal else {
            return;
        };
        if let Some(app) = self.windows.iter_mut().find(|app| {
            app.window
                .as_ref()
                .is_some_and(|window| window.id() == quick.id)
        }) {
            if let Some(window) = &app.window {
                quick.hide(
                    window,
                    &app.core.config.window.macos_quick_terminal,
                    restore_focus,
                );
            }
            app.handle_window_focus_changed(false);
        }
        self.secure_input.update(false);
        self.advance_quick_terminal();
    }

    fn advance_quick_terminal(&mut self) {
        let Some(quick) = &mut self.quick_terminal else {
            return;
        };
        let Some(window) = self.windows.iter().find_map(|app| {
            app.window
                .as_ref()
                .filter(|window| window.id() == quick.id)
                .cloned()
        }) else {
            return;
        };
        let now = std::time::Instant::now();
        let previous = quick.step(&window, now);
        if quick.take_final_resize(now)
            && let Some(app) = self
                .windows
                .iter_mut()
                .find(|app| app.native_quick_terminal)
        {
            // Commit exactly one final grid/PTY resize; animation frames are
            // compositor previews, never intermediate terminal dimensions.
            let size = window.inner_size();
            // Hidden/intermediate Resized events are deliberately ignored. A
            // zero-duration show after a reversed hide may emit no new event,
            // so explicitly queue the final GPU extent as well as the PTY size.
            if let Some(renderer) = &mut app.renderer {
                renderer.resize(size.width, size.height);
            }
            app.pending_resize = Some((size, now));
            app.schedule_redraw();
        }
        if let Some(previous) = previous
            && let Some(window) = self
                .windows
                .iter()
                .find_map(|app| app.window.as_ref().filter(|window| window.id() == previous))
        {
            window.focus_window();
            self.active = Some(previous);
        }
    }

    fn update_native_state(&mut self) {
        // Ask AppKit which tab is key; native tab switching and window-menu
        // selection can change it without going through a loom action.
        let focused = self.windows.iter().position(|app| {
            (!app.native_quick_terminal
                || self
                    .quick_terminal
                    .as_ref()
                    .is_some_and(|quick| quick.visible))
                && app
                    .window
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
                && !app.modal_captures_keyboard()
                && (self.manual_secure_input
                    || (app.core.config.window.macos_secure_input
                        && app
                            .core
                            .workspaces
                            .active()
                            .active_pane_id()
                            .and_then(|pid| app.core.pane_grids.get(&pid))
                            .is_some_and(|grid| grid.password_input)))
        });
        self.secure_input.update(password);
        self.accessibility.begin_update();
        for app in &self.windows {
            if let Some(window) = &app.window {
                if let std::collections::hash_map::Entry::Vacant(entry) =
                    self.view_hooks.entry(window.id())
                    && let Some(hooks) = lookup::ViewHooks::install(window)
                {
                    if let Some(window) = native::native_window(window) {
                        // Loom restores its session/layout snapshot itself.
                        // Do not also ask AppKit to restore these NSWindows.
                        window.setRestorable(false);
                    }
                    entry.insert(hooks);
                }
                if let Some(hooks) = self.view_hooks.get(&window.id()) {
                    let visible = !app.native_quick_terminal
                        || self
                            .quick_terminal
                            .as_ref()
                            .is_some_and(|quick| quick.visible);
                    self.accessibility
                        .update(app, hooks.view(), visible, application_active);
                    text_services::update(app, hooks.view(), visible);
                }
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
        let config = app.map(|app| &app.core.config).unwrap_or(&self.last_config);
        if let Some(manager) = &self.hotkey_manager {
            self.hotkey_registration
                .update(manager, &config.window.macos_quick_terminal.shortcut);
        }
        if let Some(menu) = &mut self.menu {
            menu.update_secure_input(self.manual_secure_input, self.secure_input.is_enabled());
            menu.update_quick_terminal(
                self.hotkey_registration.label().as_deref(),
                self.hotkey_registration.error.as_deref(),
            );
            menu.update(
                &app.map(|app| &app.core.config)
                    .unwrap_or(&self.last_config)
                    .keys,
                app.is_some(),
                app.is_some_and(|app| app.core.selection.is_some()),
                app.is_some_and(|app| app.modal_captures_keyboard()),
                app.is_some_and(|app| app.native_quick_terminal),
            );
        }
        self.save_restoration(false);
        self.update_scripting();
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
        let queued: Vec<_> = self.commands.try_iter().collect();
        if !self.explicit_window && !native::is_default_launch() {
            // A Service or automation request may arrive *after* resumed().
            // AppKit's launch notification lets it create its own window,
            // instead of first opening/restoring unrelated default windows.
            self.windows.clear();
            self.pending_restore = None;
            // A background query must not replace the saved desktop with an
            // empty snapshot when it later exits.
            self.restoration = None;
        }
        // File/Services launch requests can arrive before didFinishLaunching.
        // They supply their own windows instead of an unrelated initial one.
        if queued
            .iter()
            .any(|command| matches!(command, Command::OpenDirectory { .. }))
        {
            self.windows.clear();
            self.pending_restore = None;
        }
        if !self.show_initial_window {
            self.windows.clear();
        }
        if let Some(snapshot) = self.pending_restore.take() {
            self.restore_windows(snapshot, event_loop);
        } else {
            for app in &mut self.windows {
                app.resumed(event_loop);
                self.active = app.window.as_ref().map(|w| w.id());
            }
        }
        for command in queued {
            self.dispatch(command, event_loop);
        }
        if self.explicit_window || native::is_default_launch() {
            native::activate();
        }
        self.update_native_state();
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        self.prune_closed(event_loop);
        let mut flow = ControlFlow::Wait;
        for app in &mut self.windows {
            if app.native_quick_terminal
                && self
                    .quick_terminal
                    .as_ref()
                    .is_some_and(|quick| quick.defer_resize(std::time::Instant::now()))
            {
                app.pending_resize = None;
            }
            app.new_events(event_loop, cause);
            flow = merge_control_flow(flow, event_loop.control_flow());
        }
        self.advance_quick_terminal();
        if let Some(deadline) = self.next_native_deadline() {
            flow = merge_control_flow(flow, ControlFlow::WaitUntil(deadline));
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
        let is_quick = self
            .quick_terminal
            .as_ref()
            .is_some_and(|quick| quick.id == id);
        if is_quick
            && matches!(event, WindowEvent::Resized(_))
            && self
                .quick_terminal
                .as_ref()
                .is_some_and(|quick| quick.defer_resize(std::time::Instant::now()))
        {
            return;
        }
        if let WindowEvent::TouchpadPressure { stage, .. } = &event {
            let point = self.view_hooks.get_mut(&id).and_then(|view| {
                view.pressure_changed(*stage)
                    .then(|| view.pointer())
                    .flatten()
            });
            if let Some(point) = point {
                self.look_up(id, point, false);
            }
            return;
        }
        if matches!(event, WindowEvent::Focused(false))
            && let Some(view) = self.view_hooks.get_mut(&id)
        {
            view.pressure_changed(0);
        }
        if is_quick && matches!(event, WindowEvent::CloseRequested) {
            self.hide_quick_terminal(true);
        } else if matches!(event, WindowEvent::CloseRequested) {
            self.close_window(id, event_loop);
        } else if let Some(app) = self
            .windows
            .iter_mut()
            .find(|app| app.window.as_ref().is_some_and(|w| w.id() == id))
        {
            if is_quick
                && self
                    .quick_terminal
                    .as_ref()
                    .is_some_and(|quick| !quick.visible)
                && matches!(
                    event,
                    WindowEvent::KeyboardInput { .. }
                        | WindowEvent::Ime(_)
                        | WindowEvent::Focused(true)
                        | WindowEvent::ModifiersChanged(_)
                        | WindowEvent::MouseInput { .. }
                        | WindowEvent::MouseWheel { .. }
                        | WindowEvent::CursorMoved { .. }
                        | WindowEvent::DroppedFile(_)
                )
            {
                return;
            }
            let hide_quick = is_quick
                && matches!(event, WindowEvent::Focused(false))
                && app.core.config.window.macos_quick_terminal.autohide;
            if matches!(event, WindowEvent::Focused(true)) {
                self.active = Some(id);
            }
            app.window_event(event_loop, id, event);
            if hide_quick {
                self.hide_quick_terminal(false);
            }
        }
        self.update_native_state();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        for app in &mut self.windows {
            app.about_to_wait(event_loop);
        }
        self.advance_quick_terminal();
        self.update_native_state();
        if let Some(deadline) = self.next_native_deadline() {
            event_loop.set_control_flow(merge_control_flow(
                event_loop.control_flow(),
                ControlFlow::WaitUntil(deadline),
            ));
        }
    }

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        scripting::cancel_all("loomtty is quitting");
        scripting::stop_intents();
        scripting::configure(false);
        self.save_restoration(true);
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
