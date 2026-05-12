use ciri_protocol::message::*;

use super::{Server, ServerResponse};

impl Server {
    pub(super) fn handle_input(
        &mut self,
        msg: ClientMessage,
        _client_id: u64,
        session_name: &str,
        _responses: &mut Vec<ServerResponse>,
    ) {
        match msg {
            ClientMessage::Input {
                pane_id,
                data,
                input_seq,
            } => {
                let pane_exists = if let Some(session) = self.sessions.get_mut(session_name)
                    && let Some(pane) = session.panes.get_mut(&pane_id)
                {
                    pane.write_to_pty(&data);
                    true
                } else {
                    false
                };
                // Stash the seq as *received*. It only gets promoted to the
                // ack-able `max_input_seq` once the pane's PTY produces output
                // (see `Session::promote_received_input_seqs`), so clients
                // don't see an echo_ack until the framebuffer reflects this
                // input. Gated on pane existence to prevent malicious clients
                // from bloating the HashMap.
                if pane_exists {
                    if let Some(client) = self.clients.get_mut(&_client_id) {
                        let entry = client.received_input_seq.entry(pane_id).or_insert(0);
                        if input_seq > *entry {
                            *entry = input_seq;
                        }
                    }
                }
            }
            ClientMessage::MouseInput {
                pane_id,
                button,
                col,
                row,
                pressed,
                modifiers,
            } => {
                if let Some(session) = self.sessions.get_mut(session_name)
                    && let Some(pane) = session.panes.get_mut(&pane_id)
                    && pane.has_mouse_mode()
                {
                    pane.send_mouse_input(button, col, row, pressed, modifiers);
                }
            }
            ClientMessage::FocusChange { focused } => {
                if let Some(session) = self.sessions.get_mut(session_name) {
                    if let Some(pane_id) = session.workspaces.active().active_pane_id()
                        && let Some(pane) = session.panes.get_mut(&pane_id)
                    {
                        pane.write_focus_event(focused);
                    }
                }
            }
            _ => {}
        }
    }
}
