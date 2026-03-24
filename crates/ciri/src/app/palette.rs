use ciri_input::action::Action;
use ciri_protocol::message::ClientMessage;
use winit::keyboard::{Key, NamedKey};

use super::{App, PaletteEntry, PaletteEntryKind};

impl App {
    pub fn open_command_palette(&mut self) {
        let entries: Vec<PaletteEntry> = Action::all_with_labels()
            .into_iter()
            .map(|(action, label)| PaletteEntry {
                label: label.to_string(),
                kind: PaletteEntryKind::Action(action),
            })
            .collect();
        let filtered: Vec<usize> = (0..entries.len()).collect();
        self.command_palette = Some(super::CommandPaletteState {
            query: String::new(),
            entries,
            filtered,
            selected_idx: 0,
            hovered_idx: None,
            sessions_only: false,
            sessions_show_all: true,
        });
        self.send(ClientMessage::ListSessions { all: true });
    }

    pub fn open_session_palette(&mut self) {
        self.command_palette = Some(super::CommandPaletteState {
            query: String::new(),
            entries: Vec::new(),
            filtered: Vec::new(),
            selected_idx: 0,
            hovered_idx: None,
            sessions_only: true,
            sessions_show_all: false,
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

    pub fn handle_command_palette_key(
        &mut self,
        event: &winit::event::KeyEvent,
        ctrl: bool,
        _shift: bool,
        _alt: bool,
    ) {
        let Some(palette) = &mut self.command_palette else {
            return;
        };

        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                self.command_palette = None;
            }
            Key::Named(NamedKey::ArrowUp) => {
                if !palette.filtered.is_empty() {
                    palette.selected_idx = if palette.selected_idx == 0 {
                        palette.filtered.len() - 1
                    } else {
                        palette.selected_idx - 1
                    };
                }
            }
            Key::Named(NamedKey::ArrowDown) => {
                if !palette.filtered.is_empty() {
                    palette.selected_idx = (palette.selected_idx + 1) % palette.filtered.len();
                }
            }
            Key::Named(NamedKey::Enter) => {
                if let Some(&entry_idx) = palette.filtered.get(palette.selected_idx) {
                    self.execute_palette_entry(entry_idx);
                }
                self.command_palette = None;
            }
            Key::Named(NamedKey::Backspace) => {
                palette.query.pop();
                self.filter_palette();
            }
            Key::Character(c) if !ctrl => {
                let s: &str = c.as_str();
                let Some(palette) = &mut self.command_palette else {
                    return;
                };
                palette.query.push_str(s);
                self.filter_palette();
            }
            _ => {}
        }
    }

    pub(crate) fn execute_palette_entry(&mut self, entry_idx: usize) {
        let Some(palette) = &self.command_palette else {
            return;
        };
        let Some(entry) = palette.entries.get(entry_idx) else {
            return;
        };
        match &entry.kind {
            PaletteEntryKind::Action(action) => {
                let action = *action;
                self.handle_action(action);
            }
            PaletteEntryKind::SwitchSession(name) => {
                self.send(ClientMessage::SwitchSession {
                    session_name: name.clone(),
                });
            }
            PaletteEntryKind::KillSession(name) => {
                self.send(ClientMessage::KillSession {
                    session_name: name.clone(),
                });
            }
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

    pub(crate) fn command_palette_scroll_offset(&self, visible_rows: usize) -> usize {
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
