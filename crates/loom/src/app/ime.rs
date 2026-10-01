//! IME (input method editor) preedit/commit handling.
//!
//! `handle_ime` owns the preedit state (`preedit_active`/`preedit_text`/
//! `preedit_cursor`). A committed string is routed in precedence order: overlay
//! input first (e.g. the command-palette query), swallowed if a modal captures
//! the keyboard, otherwise sent to the PTY — broadcast to every visible pane in
//! `broadcast_mode` (suppressed to the active pane when it is in password
//! mode), else just the active pane.
//! Composition binds to the original field/session/channel and recipient set;
//! a delayed commit cannot follow a focus, modal, or connection change.

use loom_protocol::message::ClientMessage;
use winit::event::Ime;

use super::App;

#[derive(Debug, PartialEq)]
enum Destination {
    Blocked,
    Palette { remote: bool },
    Search(u64),
    Terminal { active: u64, recipients: Vec<u64> },
}

pub(super) struct Target {
    destination: Destination,
    session: String,
    sender: Option<crossbeam_channel::Sender<ClientMessage>>,
    generation: u64,
}

impl Target {
    fn matches(&self, other: &Self) -> bool {
        self.destination == other.destination
            && self.session == other.session
            && self.generation == other.generation
            && match (&self.sender, &other.sender) {
                (Some(a), Some(b)) => a.same_channel(b),
                (None, None) => true,
                _ => false,
            }
    }
}

impl App {
    fn current_ime_target(&self) -> Target {
        let destination = if !self.window_focused
            || self.core.pending_paste.is_some()
            || self.core.context_menu.visible
            || self.core.settings_panel_visible
            || self.core.help_visible
            || self.core.overview.active
        {
            Destination::Blocked
        } else if let Some(palette) = &self.core.command_palette {
            Destination::Palette {
                remote: palette.remote_input_mode,
            }
        } else if let Some(search) = &self.core.search_state {
            Destination::Search(search.pane_id)
        } else if let Some(active) = self.core.workspaces.active().active_pane_id()
            && self.core.connected
            && self.core.pending_session_name.is_none()
            && let Some(grid) = self.core.pane_grids.get(&active)
        {
            // Password composition must never fan out to peer panes.
            let recipients = if self.core.broadcast_mode && !grid.password_input {
                let vox = self.core.anim_mgr.view_offset_x.value() as f32;
                self.core
                    .workspaces
                    .active()
                    .visible_tiles(vox)
                    .iter()
                    .map(|(pane, _, _)| *pane)
                    .collect()
            } else {
                vec![active]
            };
            Destination::Terminal { active, recipients }
        } else {
            Destination::Blocked
        };
        Target {
            destination,
            session: self.core.session_name.clone(),
            sender: self.core.server_tx.clone(),
            generation: self.ime_generation,
        }
    }

    pub(super) fn ime_target_is_current(&self) -> bool {
        let current = self.current_ime_target();
        current.destination != Destination::Blocked
            && self
                .ime_target
                .as_ref()
                .is_none_or(|target| target.matches(&current))
    }

    pub(super) fn cancel_stale_ime(&mut self) {
        if !self.core.ime.preedit_active || self.ime_target_is_current() {
            return;
        }
        if let Some(window) = &self.window {
            window.set_ime_allowed(false);
            window.set_ime_allowed(self.window_focused);
        }
        self.core.ime.preedit_active = false;
        self.core.ime.preedit_text.clear();
        self.core.ime.preedit_cursor = None;
        self.core.ime.last_area = None;
        // Retain the stale identity until Disabled or a fresh composition.
        // A commit already queued before cancellation must still be rejected.
        self.schedule_redraw();
    }

    pub(crate) fn handle_ime(&mut self, ime: Ime) {
        match ime {
            Ime::Commit(text) => {
                self.core.ime.preedit_active = false;
                self.core.ime.preedit_text.clear();
                self.core.ime.preedit_cursor = None;
                let current = self.current_ime_target();
                let stale = self
                    .ime_target
                    .take()
                    .is_some_and(|target| !target.matches(&current));
                if stale || current.destination == Destination::Blocked {
                    self.schedule_redraw();
                    return;
                }
                if self.append_text_to_overlay_input(&text) || self.modal_captures_keyboard() {
                    self.schedule_redraw();
                    return;
                }

                if let Destination::Terminal { recipients, .. } = current.destination {
                    let data = text.into_bytes();
                    for pane_id in recipients {
                        self.send(ClientMessage::Input {
                            pane_id,
                            data: data.clone(),
                            input_seq: 0,
                        });
                    }
                }
                self.schedule_redraw();
            }
            Ime::Preedit(text, cursor) => {
                if !text.is_empty() && !self.core.ime.preedit_active {
                    self.ime_target = Some(self.current_ime_target());
                }
                // Keep the target across an empty preedit: AppKit sends that
                // immediately before Commit, which must retain its identity.
                self.core.ime.preedit_active = !text.is_empty();
                self.core.ime.preedit_text = text;
                self.core.ime.preedit_cursor = cursor.map(|(start, _)| start);
                self.schedule_redraw();
            }
            Ime::Disabled => {
                self.ime_target = None;
                self.core.ime.preedit_active = false;
                self.core.ime.preedit_text.clear();
                self.core.ime.preedit_cursor = None;
                self.core.ime.last_area = None;
                self.schedule_redraw();
            }
            Ime::Enabled => {
                self.ime_target = None;
                self.core.ime.last_area = None;
                self.schedule_redraw();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use loom_config::config::LoomConfig;

    fn make_app() -> App {
        App::new(LoomConfig::default(), "test-session")
    }

    fn terminal_app() -> (App, crossbeam_channel::Receiver<ClientMessage>) {
        let mut app = make_app();
        for pane in [1, 2] {
            app.core
                .workspaces
                .active_mut()
                .add_column_right(pane, loom_layout::column::ColumnWidth::Proportion(0.5));
            app.core
                .pane_grids
                .insert(pane, crate::grid::ClientPaneGrid::new(80, 24, 0));
        }
        app.core.workspaces.active_mut().focus_column(0);
        app.core.connected = true;
        let (tx, rx) = crossbeam_channel::unbounded();
        app.core.server_tx = Some(tx);
        (app, rx)
    }

    #[test]
    fn composition_does_not_follow_pane_focus_change() {
        let (mut app, rx) = terminal_app();
        app.handle_ime(Ime::Preedit("ni".into(), Some((2, 2))));
        app.core.workspaces.active_mut().focus_column(1);
        // AppKit emits an empty preedit immediately before Commit.
        app.handle_ime(Ime::Preedit(String::new(), None));
        app.handle_ime(Ime::Commit("你".into()));
        assert!(
            rx.is_empty(),
            "old composition must not type in the newly focused pane"
        );
    }

    #[test]
    fn cancelling_stale_composition_unblocks_input_and_preserves_commit_guard() {
        let (mut app, rx) = terminal_app();
        app.handle_ime(Ime::Preedit("ni".into(), Some((2, 2))));
        app.core.workspaces.active_mut().focus_column(1);
        app.cancel_stale_ime();
        assert!(!app.core.ime.preedit_active);
        assert!(app.core.ime.preedit_text.is_empty());
        app.handle_ime(Ime::Commit("旧".into()));
        assert!(rx.is_empty());
        app.handle_ime(Ime::Preedit("xin".into(), Some((3, 3))));
        app.handle_ime(Ime::Commit("新".into()));
        let ClientMessage::Input { pane_id, data, .. } = rx.try_recv().unwrap() else {
            panic!("expected input")
        };
        assert_eq!(pane_id, 2);
        assert_eq!(data, "新".as_bytes());
        assert!(rx.is_empty());
    }

    #[test]
    fn composition_does_not_follow_terminal_into_palette() {
        let (mut app, rx) = terminal_app();
        app.handle_ime(Ime::Preedit("ni".into(), Some((2, 2))));
        app.core.open_command_palette();
        app.handle_ime(Ime::Commit("你".into()));
        assert!(app.core.command_palette.as_ref().unwrap().query.is_empty());
        assert!(
            !rx.try_iter()
                .any(|message| matches!(message, ClientMessage::Input { .. }))
        );
    }

    #[test]
    fn valid_composition_survives_appkit_empty_preedit_and_commits_once() {
        let (mut app, rx) = terminal_app();
        app.handle_ime(Ime::Preedit("ni".into(), Some((2, 2))));
        app.handle_ime(Ime::Preedit(String::new(), None));
        app.handle_ime(Ime::Commit("你👩🏽‍💻e\u{301}".into()));
        let ClientMessage::Input { pane_id, data, .. } = rx.try_recv().unwrap() else {
            panic!("expected input")
        };
        assert_eq!(pane_id, 1);
        assert_eq!(data, "你👩🏽‍💻e\u{301}".as_bytes());
        assert!(rx.is_empty());
    }

    #[test]
    fn composition_rejects_session_replacement_reconnect_and_pending_switch() {
        for change in 0..3 {
            let (mut app, old_rx) = terminal_app();
            app.handle_ime(Ime::Preedit("ni".into(), Some((2, 2))));
            let (new_tx, new_rx) = crossbeam_channel::unbounded();
            match change {
                0 => app.core.session_name = "other-session".into(),
                1 => app.core.server_tx = Some(new_tx),
                _ => app.core.pending_session_name = Some("other-session".into()),
            }
            assert!(!app.ime_target_is_current());
            app.handle_ime(Ime::Preedit(String::new(), None));
            app.handle_ime(Ime::Commit("你".into()));
            assert!(old_rx.is_empty());
            assert!(new_rx.is_empty());
        }
    }

    #[test]
    fn reopened_palette_does_not_inherit_old_composition() {
        let mut app = make_app();
        app.enter_modal_close_peers(super::super::ModalKind::CommandPalette);
        app.core.open_command_palette();
        app.handle_ime(Ime::Preedit("ni".into(), Some((2, 2))));
        app.enter_modal_close_peers(super::super::ModalKind::None);
        app.enter_modal_close_peers(super::super::ModalKind::CommandPalette);
        app.core.open_command_palette();
        app.handle_ime(Ime::Commit("旧".into()));
        assert!(app.core.command_palette.as_ref().unwrap().query.is_empty());
        app.handle_ime(Ime::Preedit("xin".into(), Some((3, 3))));
        app.handle_ime(Ime::Preedit(String::new(), None));
        app.handle_ime(Ime::Commit("新".into()));
        assert_eq!(app.core.command_palette.as_ref().unwrap().query, "新");
    }

    #[test]
    fn ime_commit_does_not_edit_palette_beneath_paste_confirmation() {
        let mut app = make_app();
        app.core.open_command_palette();
        app.core.config.terminal.paste_warn_threshold = 3;
        app.handle_text_paste("large paste".into());
        assert!(app.core.pending_paste.is_some());
        app.handle_ime(Ime::Commit("你".into()));
        assert!(app.core.command_palette.as_ref().unwrap().query.is_empty());
    }

    #[test]
    fn background_window_rejects_ime_commit_and_password_does_not_broadcast() {
        let (mut app, rx) = terminal_app();
        app.handle_window_focus_changed(false);
        app.handle_ime(Ime::Commit("旧".into()));
        assert!(
            !rx.try_iter()
                .any(|message| matches!(message, ClientMessage::Input { .. }))
        );
        app.handle_window_focus_changed(true);
        rx.try_iter().for_each(drop);
        app.core.broadcast_mode = true;
        app.core.pane_grids.get_mut(&1).unwrap().password_input = true;
        app.handle_ime(Ime::Preedit("mi".into(), Some((2, 2))));
        app.handle_ime(Ime::Commit("密".into()));
        let ClientMessage::Input { pane_id, data, .. } = rx.try_recv().unwrap() else {
            panic!("expected input")
        };
        assert_eq!(pane_id, 1);
        assert_eq!(data, "密".as_bytes());
        assert!(rx.is_empty());
    }

    #[test]
    fn candidate_area_refreshes_after_font_scale_or_ime_context_changes() {
        use loom_app::app::ImeCursorArea;
        let mut app = make_app();
        let area = ImeCursorArea {
            x: 100,
            y: 200,
            width: 8.0,
            height: 16.0,
            scale: 1.0,
        };
        assert!(app.core.ime.update_area(area));
        assert!(!app.core.ime.update_area(area));
        let resized = ImeCursorArea {
            height: 20.0,
            ..area
        };
        assert!(app.core.ime.update_area(resized));
        let scaled = ImeCursorArea {
            scale: 2.0,
            ..resized
        };
        assert!(app.core.ime.update_area(scaled));
        assert!(!app.core.ime.update_area(scaled));
        app.handle_ime(Ime::Disabled);
        assert!(app.core.ime.update_area(scaled));
        app.handle_ime(Ime::Enabled);
        assert!(app.core.ime.update_area(scaled));
    }

    #[test]
    fn cancelled_composition_does_not_block_subsequent_input() {
        let mut app = make_app();
        app.handle_ime(Ime::Preedit("ni".into(), Some((2, 2))));
        app.handle_ime(Ime::Disabled);
        assert!(!app.core.ime.preedit_active);
        assert!(app.core.ime.preedit_text.is_empty());
        assert_eq!(app.core.ime.preedit_cursor, None);
        app.core.open_command_palette();
        app.handle_ime(Ime::Commit("你好".into()));
        assert_eq!(app.core.command_palette.unwrap().query, "你好");
    }

    #[test]
    fn focus_loss_clears_composition_and_stale_modifiers() {
        let mut app = make_app();
        app.handle_ime(Ime::Preedit("ni".into(), Some((2, 2))));
        app.modifiers = winit::keyboard::ModifiersState::SUPER;
        app.handle_window_focus_changed(false);
        assert!(!app.core.ime.preedit_active);
        assert!(app.modifiers.is_empty());
    }

    #[test]
    fn ime_commit_routes_to_palette_query() {
        let mut app = make_app();
        app.core.open_command_palette();
        app.core.ime.preedit_active = true;
        app.core.ime.preedit_text = "n".into();

        app.handle_ime(Ime::Commit("你好".into()));

        assert_eq!(
            app.core.command_palette.as_ref().map(|p| p.query.as_str()),
            Some("你好")
        );
        assert!(!app.core.ime.preedit_active);
        assert!(app.core.ime.preedit_text.is_empty());
    }
}
