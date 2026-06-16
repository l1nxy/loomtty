//! IME (input method editor) preedit/commit handling.
//!
//! `handle_ime` owns the preedit state (`preedit_active`/`preedit_text`/
//! `preedit_cursor`). A committed string is routed in precedence order: overlay
//! input first (e.g. the command-palette query), swallowed if a modal captures
//! the keyboard, otherwise sent to the PTY — broadcast to every visible pane in
//! `broadcast_mode` (suppressed to the active pane when it is in password
//! mode), else just the active pane.

use loom_protocol::message::ClientMessage;
use winit::event::Ime;

use super::App;

impl App {
    pub(crate) fn handle_ime(&mut self, ime: Ime) {
        match ime {
            Ime::Commit(text) => {
                self.core.ime.preedit_active = false;
                self.core.ime.preedit_text.clear();
                self.core.ime.preedit_cursor = None;
                if self.append_text_to_overlay_input(&text) || self.modal_captures_keyboard() {
                    self.schedule_redraw();
                    return;
                }

                let data = text.into_bytes();
                // Mirror send_key_input: a committed string (which may be a
                // password typed via IME) must never fan out to peer panes
                // while the active pane is in password-input mode.
                let password_mode = self
                    .core
                    .workspaces
                    .active()
                    .active_pane_id()
                    .and_then(|pid| self.core.pane_grids.get(&pid))
                    .is_some_and(|g| g.password_input);
                if self.core.broadcast_mode && !password_mode {
                    let vox = self.core.anim_mgr.view_offset_x.value() as f32;
                    let pids: Vec<u64> = self
                        .core
                        .workspaces
                        .active()
                        .visible_tiles(vox)
                        .iter()
                        .map(|(pid, _, _)| *pid)
                        .collect();
                    for pid in pids {
                        self.send(ClientMessage::Input {
                            pane_id: pid,
                            data: data.clone(),
                            input_seq: 0,
                        });
                    }
                } else if let Some(pid) = self.core.workspaces.active().active_pane_id() {
                    self.send(ClientMessage::Input {
                        pane_id: pid,
                        data,
                        input_seq: 0,
                    });
                }
                self.schedule_redraw();
            }
            Ime::Preedit(text, cursor) => {
                self.core.ime.preedit_active = !text.is_empty();
                self.core.ime.preedit_text = text;
                self.core.ime.preedit_cursor = cursor.map(|(start, _)| start);
                self.schedule_redraw();
            }
            Ime::Enabled | Ime::Disabled => {}
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
