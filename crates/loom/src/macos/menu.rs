use std::collections::HashMap;

use loom_config::keys::KeybindConfig;
use loom_input::action::Action;
use muda::{
    AboutMetadata, CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu,
    accelerator::Accelerator,
};
use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;
use objc2_foundation::ns_string;

use super::Command;
use crate::app::App;
use winit::window::WindowId;

struct Entry {
    item: MenuItem,
    command: Command,
    /// Config-driven shortcuts track the active window's direct bindings.
    action: Option<Action>,
}

pub struct MenuBar {
    _menu: Menu,
    entries: Vec<Entry>,
    secure_input: CheckMenuItem,
    bindings: Option<HashMap<String, String>>,
    panes: Submenu,
    pane_entries: Vec<(CheckMenuItem, Command)>,
    pane_context: Option<(WindowId, String)>,
}

impl MenuBar {
    pub fn new() -> muda::Result<Self> {
        let menu = Menu::new();
        let mut this = Self {
            _menu: menu.clone(),
            entries: Vec::new(),
            secure_input: CheckMenuItem::new("Secure Keyboard Entry", true, false, None),
            bindings: None,
            panes: Submenu::new("Go to Pane (1–9)", false),
            pane_entries: Vec::new(),
            pane_context: None,
        };
        let app = Submenu::new("loomtty", true);
        app.append(&PredefinedMenuItem::about(
            Some("About loomtty"),
            Some(AboutMetadata {
                name: Some("loomtty".into()),
                version: Some(env!("CARGO_PKG_VERSION").into()),
                copyright: Some("© loomtty contributors. AGPL-3.0-only.".into()),
                ..Default::default()
            }),
        ))?;
        app.append(&PredefinedMenuItem::separator())?;
        this.add(
            &app,
            "Settings…",
            Command::Action(Action::ToggleSettings),
            Some("Super+Comma"),
        )?;
        this.add(
            &app,
            "Reload Configuration",
            Command::Reload,
            Some("Super+Shift+Comma"),
        )?;
        app.append(&this.secure_input)?;
        app.append_items(&[
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::services(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
        ])?;
        this.add(&app, "Quit loomtty", Command::Quit, Some("Super+Q"))?;

        let file = Submenu::new("File", true);
        this.add(&file, "New Pane", Command::NewPane, Some("Super+N"))?;
        this.add(&file, "New Pane Tab", Command::NewPane, Some("Super+T"))?;
        this.add(&file, "New Window", Command::NewWindow, None)?;
        this.add(&file, "New Session in Window Tab", Command::NewTab, None)?;
        file.append(&PredefinedMenuItem::separator())?;
        this.add(
            &file,
            "New Column",
            Command::Action(Action::NewColumnRight),
            None,
        )?;
        this.add(
            &file,
            "New Workspace Below",
            Command::Action(Action::NewWorkspaceBelow),
            None,
        )?;
        this.add(
            &file,
            "New Stacked Tile",
            Command::Action(Action::NewTileBelow),
            None,
        )?;
        file.append(&PredefinedMenuItem::separator())?;
        this.add(
            &file,
            "Close Pane",
            Command::Action(Action::ClosePane),
            None,
        )?;
        this.add(
            &file,
            "Close Tab / Window",
            Command::CloseWindow,
            Some("Super+Shift+W"),
        )?;

        let edit = Submenu::new("Edit", true);
        // Custom items deliberately use loom's selection and guarded paste
        // pipeline. AppKit's standard copy:/paste: do not know our GPU view.
        this.add(&edit, "Copy", Command::Action(Action::ClipboardCopy), None)?;
        this.add(
            &edit,
            "Paste",
            Command::Action(Action::ClipboardPaste),
            None,
        )?;
        this.add(&edit, "Select All", Command::SelectAll, Some("Super+A"))?;
        edit.append(&PredefinedMenuItem::separator())?;
        this.add(&edit, "Find…", Command::Action(Action::OpenSearch), None)?;
        this.add(&edit, "Look Up", Command::LookUp, Some("Control+Super+D"))?;

        let view = Submenu::new("View", true);
        this.add(&view, "Quick Terminal", Command::ToggleQuickTerminal, None)?;
        this.add(
            &view,
            "Command Palette…",
            Command::Action(Action::ToggleCommandPalette),
            None,
        )?;
        this.add(
            &view,
            "Overview",
            Command::Action(Action::ToggleOverview),
            None,
        )?;
        view.append(&PredefinedMenuItem::separator())?;
        this.add(
            &view,
            "Increase Font Size",
            Command::FontSize(1),
            Some("Super+Equal"),
        )?;
        this.add(
            &view,
            "Decrease Font Size",
            Command::FontSize(-1),
            Some("Super+Minus"),
        )?;
        this.add(
            &view,
            "Reset Font Size",
            Command::FontSize(0),
            Some("Super+Digit0"),
        )?;
        view.append(&PredefinedMenuItem::separator())?;
        this.add(
            &view,
            "Toggle Full Screen",
            Command::ToggleFullscreen,
            Some("Control+Super+F"),
        )?;

        let pane = Submenu::new("Pane", true);
        for (label, action) in [
            ("Split Right", Action::NewColumnRight),
            ("Split Below", Action::NewTileBelow),
            ("Focus Left", Action::FocusLeft),
            ("Focus Right", Action::FocusRight),
            ("Focus Above", Action::FocusUp),
            ("Focus Below", Action::FocusDown),
            ("Close Pane", Action::ClosePane),
        ] {
            this.add(&pane, label, Command::Action(action), None)?;
        }
        pane.append(&PredefinedMenuItem::separator())?;
        pane.append(&this.panes)?;
        this.add(
            &view,
            "Sessions…",
            Command::Action(Action::ToggleSessionPalette),
            None,
        )?;

        let window = Submenu::new("Window", true);
        window.append_items(&[
            &PredefinedMenuItem::minimize(None),
            &PredefinedMenuItem::maximize(None),
        ])?;
        this.add(
            &window,
            "Previous Session Window Tab",
            Command::PreviousTab,
            Some("Super+Shift+BracketLeft"),
        )?;
        this.add(
            &window,
            "Next Session Window Tab",
            Command::NextTab,
            Some("Super+Shift+BracketRight"),
        )?;
        window.append_items(&[
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::bring_all_to_front(None),
        ])?;

        this.add(
            &window,
            "Move Tab to New Window",
            Command::MoveTabToWindow,
            None,
        )?;
        this.add(&window, "Merge All Windows", Command::MergeWindows, None)?;
        this.add(&window, "Show / Hide Tab Bar", Command::ToggleTabBar, None)?;

        let help = Submenu::new("Help", true);
        this.add(
            &help,
            "Keybindings",
            Command::Action(Action::ToggleHelp),
            None,
        )?;
        menu.append_items(&[&app, &file, &edit, &view, &pane, &window, &help])?;
        menu.init_for_nsapp();
        // muda 0.15 creates a separate NSMenu when attaching each submenu.
        // Its registration helpers point at the unattached template instead,
        // so AppKit's window list and Help search never reach the menu bar.
        let mtm = MainThreadMarker::new().expect("menus are created on the main thread");
        let application = NSApplication::sharedApplication(mtm);
        let native_menu = application.mainMenu().expect("installed main menu");
        let windows = native_menu
            .itemWithTitle(ns_string!("Window"))
            .and_then(|item| item.submenu())
            .expect("attached Window menu");
        let help = native_menu
            .itemWithTitle(ns_string!("Help"))
            .and_then(|item| item.submenu())
            .expect("attached Help menu");
        application.setWindowsMenu(Some(&windows));
        application.setHelpMenu(Some(&help));
        Ok(this)
    }

    fn add(
        &mut self,
        menu: &Submenu,
        title: &str,
        command: Command,
        shortcut: Option<&str>,
    ) -> muda::Result<()> {
        let accelerator =
            shortcut.map(|s| s.parse::<Accelerator>().expect("valid native shortcut"));
        let item = MenuItem::new(title, true, accelerator);
        menu.append(&item)?;
        let action = match &command {
            Command::Action(action) if shortcut.is_none() => Some(action.clone()),
            _ => None,
        };
        self.entries.push(Entry {
            item,
            command,
            action,
        });
        Ok(())
    }

    /// Pane navigation belongs in AppKit when the GPU tab strip is absent.
    /// Requests retain window/session identity so a queued click cannot focus
    /// a same-numbered pane after the active window or session changes.
    pub fn update_panes(&mut self, app: Option<&App>, settings: bool) {
        let context = app.and_then(|app| {
            app.window
                .as_ref()
                .map(|window| (window.id(), app.core.session_name.clone()))
        });
        let mut panes = app.map(App::pane_tab_entries).unwrap_or_default();
        // Shortcut menu stays bounded; the scrolling pane strip exposes all panes.
        panes.truncate(9);
        let active = app.and_then(|app| app.core.workspaces.active().active_pane_id());
        let enabled = !settings && app.is_some_and(|app| !app.modal_captures_keyboard());
        let menu_enabled = enabled && !panes.is_empty();
        if self.panes.is_enabled() != menu_enabled {
            self.panes.set_enabled(menu_enabled);
        }
        let changed = self.pane_context != context || self.pane_entries.len() != panes.len()
            || self.pane_entries.iter().zip(&panes).any(|((_, command), (id, _))|
                !matches!(command, Command::FocusPane { pane, .. } if pane == id));
        if changed {
            for (item, _) in self.pane_entries.drain(..) {
                let _ = self.panes.remove(&item);
            }
            if let Some((window, session)) = &context {
                for (index, (id, label)) in panes.iter().enumerate() {
                    let shortcut = (index < 9).then(|| {
                        format!("Super+Alt+Digit{}", index + 1)
                            .parse::<Accelerator>()
                            .expect("valid pane shortcut")
                    });
                    let item = CheckMenuItem::new(label, enabled, active == Some(*id), shortcut);
                    if let Err(error) = self.panes.append(&item) {
                        log::warn!("could not add native pane item: {error}");
                        break;
                    }
                    self.pane_entries.push((
                        item,
                        Command::FocusPane {
                            window: *window,
                            session: session.clone(),
                            pane: *id,
                        },
                    ));
                }
            }
            self.pane_context = context;
        }
        for ((item, _), (id, label)) in self.pane_entries.iter().zip(&panes) {
            if item.text() != *label {
                item.set_text(label);
            }
            if item.is_checked() != (active == Some(*id)) {
                item.set_checked(active == Some(*id));
            }
            if item.is_enabled() != enabled {
                item.set_enabled(enabled);
            }
        }
    }

    pub fn update_quick_terminal(&self, shortcut: Option<&str>, error: Option<&str>) {
        if let Some(entry) = self
            .entries
            .iter()
            .find(|entry| matches!(entry.command, Command::ToggleQuickTerminal))
        {
            let title = match (shortcut, error) {
                (Some(shortcut), Some(_)) => format!("Quick Terminal ({shortcut}; reload failed)"),
                (Some(shortcut), None) => format!("Quick Terminal ({shortcut})"),
                (None, Some(_)) => "Quick Terminal (shortcut unavailable)".to_string(),
                (None, None) => "Quick Terminal".to_string(),
            };
            if entry.item.text() != title {
                entry.item.set_text(title);
            }
        }
    }

    pub fn update_secure_input(&self, manual: bool, enabled: bool) {
        let text = if enabled && !manual {
            "Secure Keyboard Entry (Automatic)"
        } else if manual && !enabled {
            "Secure Keyboard Entry (Inactive)"
        } else {
            "Secure Keyboard Entry"
        };
        if self.secure_input.text() != text {
            self.secure_input.set_text(text);
        }
        if self.secure_input.is_checked() != (manual || enabled) {
            self.secure_input.set_checked(manual || enabled);
        }
        // Automatic protection is a status indicator during password input.
        // The user can manually enable it before/after that prompt.
        let selectable = manual || !enabled;
        if self.secure_input.is_enabled() != selectable {
            self.secure_input.set_enabled(selectable);
        }
    }

    pub fn command(&self, id: &muda::MenuId) -> Option<Command> {
        if self.secure_input.id() == id {
            return Some(Command::ToggleSecureInput);
        }
        if let Some((_, command)) = self.pane_entries.iter().find(|(item, _)| item.id() == id) {
            return Some(command.clone());
        }
        self.entries
            .iter()
            .find(|entry| entry.item.id() == id)
            .map(|entry| entry.command.clone())
    }

    pub fn update(
        &mut self,
        keys: &KeybindConfig,
        has_window: bool,
        can_copy: bool,
        modal: bool,
        quick: bool,
        settings: bool,
    ) {
        let changed = self.bindings.as_ref() != Some(&keys.direct_bindings);
        for entry in &self.entries {
            if matches!(entry.command, Command::Action(Action::ClosePane)) {
                let title = if settings {
                    "Close Settings"
                } else {
                    "Close Pane"
                };
                if entry.item.text() != title {
                    entry.item.set_text(title);
                }
            }
            if matches!(entry.command, Command::ToggleFullscreen) {
                let title = if quick {
                    "Toggle Fill Screen"
                } else {
                    "Toggle Full Screen"
                };
                if entry.item.text() != title {
                    entry.item.set_text(title);
                }
            }
            if changed && let Some(action) = &entry.action {
                let accelerator = action_accelerator(&keys.direct_bindings, action);
                let _ = entry.item.set_accelerator(accelerator);
            }
            let enabled = match entry.command {
                Command::CloseWindow
                | Command::SelectAll
                | Command::Action(
                    Action::ClosePane | Action::ClipboardCopy | Action::ClipboardPaste,
                ) if settings => true,
                Command::Action(Action::ToggleSettings) => true,
                Command::Action(_)
                | Command::NewPane
                | Command::NextTab
                | Command::PreviousTab
                | Command::MergeWindows
                | Command::MoveTabToWindow
                | Command::ToggleTabBar
                | Command::FontSize(_)
                | Command::ToggleFullscreen
                    if settings =>
                {
                    false
                }
                Command::NewWindow
                | Command::NewTab
                | Command::Quit
                | Command::Reload
                | Command::ToggleQuickTerminal => true,
                Command::NextTab
                | Command::PreviousTab
                | Command::MoveTabToWindow
                | Command::MergeWindows
                | Command::ToggleTabBar => has_window && !quick,
                Command::Action(Action::ToggleHelp) => true,
                Command::Action(Action::ClipboardCopy) => has_window && can_copy && !modal,
                Command::Action(Action::ClipboardPaste) => has_window,
                Command::Action(_) | Command::NewPane | Command::SelectAll | Command::LookUp => {
                    has_window && !modal
                }
                _ => has_window,
            };
            if entry.item.is_enabled() != enabled {
                entry.item.set_enabled(enabled);
            }
        }
        if changed {
            self.bindings = Some(keys.direct_bindings.clone());
        }
    }
}

fn action_accelerator(bindings: &HashMap<String, String>, action: &Action) -> Option<Accelerator> {
    let mut matches: Vec<_> = bindings
        .iter()
        .filter(|(_, name)| Action::from_name(name).as_ref() == Some(action))
        .collect();
    matches.sort_by_key(|(key, _)| *key);
    matches.into_iter().find_map(|(key, _)| {
        // Non-Command bindings must pass through loom's mode-aware handler.
        if !key
            .split('+')
            .any(|part| part.eq_ignore_ascii_case("super") || part.eq_ignore_ascii_case("cmd"))
        {
            return None;
        }
        key.parse::<Accelerator>().ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_tracks_remapped_and_removed_direct_shortcuts() {
        let mut bindings = HashMap::from([
            ("super+c".into(), "new_column_right".into()),
            ("super+shift+c".into(), "clipboard_copy".into()),
            ("alt+c".into(), "clipboard_copy".into()),
        ]);
        assert_eq!(
            action_accelerator(&bindings, &Action::ClipboardCopy),
            Some("Super+Shift+C".parse().unwrap())
        );
        bindings.remove("super+shift+c");
        assert_eq!(action_accelerator(&bindings, &Action::ClipboardCopy), None);
        assert_eq!(
            action_accelerator(&bindings, &Action::NewColumnRight),
            Some("Super+C".parse().unwrap())
        );
    }
}
