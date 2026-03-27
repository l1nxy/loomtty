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
                let data = text.into_bytes();
                if self.broadcast_mode {
                    let vox = self.anim_mgr.view_offset_x.value() as f32;
                    let pids: Vec<u64> = self
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
                        });
                    }
                } else if let Some(pid) = self.workspaces.active_mut().active_pane_id() {
                    self.send(ClientMessage::Input {
                        pane_id: pid,
                        data,
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
