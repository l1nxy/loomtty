use ciri_protocol::message::ClientMessage;
use winit::event::Ime;

use super::App;

impl App {
    pub(crate) fn handle_ime(&mut self, ime: Ime) {
        match ime {
            Ime::Commit(text) => {
                self.ime.preedit_active = false;
                self.ime.preedit_text.clear();
                self.ime.preedit_cursor = None;
                if let Some(pid) = self.workspaces.active_mut().active_pane_id() {
                    self.send(ClientMessage::Input {
                        pane_id: pid,
                        data: text.into_bytes(),
                    });
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            Ime::Preedit(text, cursor) => {
                self.ime.preedit_active = !text.is_empty();
                self.ime.preedit_text = text;
                self.ime.preedit_cursor = cursor.map(|(start, _)| start);
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            Ime::Enabled | Ime::Disabled => {}
        }
    }
}
