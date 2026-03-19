use ciri_protocol::message::ClientMessage;
use winit::event::Ime;

use super::App;

impl App {
    pub(crate) fn handle_ime(&mut self, ime: Ime) {
        match ime {
            Ime::Commit(text) => {
                self.ime_preedit_active = false;
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
            Ime::Preedit(text, _cursor) => {
                self.ime_preedit_active = !text.is_empty();
            }
            Ime::Enabled | Ime::Disabled => {}
        }
    }
}
