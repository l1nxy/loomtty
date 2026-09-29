use std::collections::HashMap;

use loom_config::keys::KeybindConfig;
use loom_input::action::Action;
use muda::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu, accelerator::Accelerator};

use super::Command;

struct Entry {
    item: MenuItem,
    command: Command,
    /// Config-driven shortcuts track the active window's direct bindings.
    action: Option<Action>,
}

pub struct MenuBar {
    _menu: Menu,
    entries: Vec<Entry>,
    bindings: Option<HashMap<String, String>>,
}

impl MenuBar {
    pub fn new() -> muda::Result<Self> {
        let menu = Menu::new();
        let mut this = Self {
            _menu: menu.clone(),
            entries: Vec::new(),
            bindings: None,
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
        this.add(&file, "New Window", Command::NewWindow, Some("Super+N"))?;
        this.add(&file, "New Tab", Command::NewTab, Some("Super+T"))?;
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

        let view = Submenu::new("View", true);
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
        view.append(&PredefinedMenuItem::fullscreen(None))?;

        let window = Submenu::new("Window", true);
        window.append_items(&[
            &PredefinedMenuItem::minimize(None),
            &PredefinedMenuItem::maximize(None),
        ])?;
        this.add(
            &window,
            "Show Previous Tab",
            Command::PreviousTab,
            Some("Super+Shift+BracketLeft"),
        )?;
        this.add(
            &window,
            "Show Next Tab",
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
        menu.append_items(&[&app, &file, &edit, &view, &window, &help])?;
        menu.init_for_nsapp();
        window.set_as_windows_menu_for_nsapp();
        help.set_as_help_menu_for_nsapp();
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

    pub fn command(&self, id: &muda::MenuId) -> Option<Command> {
        self.entries
            .iter()
            .find(|entry| entry.item.id() == id)
            .map(|entry| entry.command.clone())
    }

    pub fn update(&mut self, keys: &KeybindConfig, has_window: bool, can_copy: bool, modal: bool) {
        let changed = self.bindings.as_ref() != Some(&keys.direct_bindings);
        for entry in &self.entries {
            if changed && let Some(action) = &entry.action {
                let accelerator = action_accelerator(&keys.direct_bindings, action);
                let _ = entry.item.set_accelerator(accelerator);
            }
            let enabled = match entry.command {
                Command::NewWindow | Command::NewTab | Command::Quit | Command::Reload => true,
                Command::Action(Action::ToggleSettings | Action::ToggleHelp) => true,
                Command::Action(Action::ClipboardCopy) => has_window && can_copy && !modal,
                Command::Action(Action::ClipboardPaste) => has_window,
                Command::Action(_) | Command::SelectAll => has_window && !modal,
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
