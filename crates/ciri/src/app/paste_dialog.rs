use ciri_protocol::message::ClientMessage;
use super::App;

impl App {
    pub(crate) fn confirm_pending_paste(&mut self) {
        let text = self.pending_paste.as_ref().unwrap().info.text.clone();
        self.pending_paste = None;
        if let Some(pid) = self.workspaces.active_mut().active_pane_id() {
            let bracketed = self
                .pane_grids
                .get(&pid)
                .is_some_and(|g| g.mode_flags & ciri_protocol::message::MODE_BRACKETED_PASTE != 0);
            let mut data = Vec::with_capacity(text.len() + if bracketed { 12 } else { 0 });
            if bracketed {
                data.extend_from_slice(b"\x1b[200~");
            }
            data.extend_from_slice(text.as_bytes());
            if bracketed {
                data.extend_from_slice(b"\x1b[201~");
            }
            self.send(ClientMessage::Input { pane_id: pid, data });
        }
    }
}
