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
                //
                // Critical: also mark the pane's damage accumulator as
                // `cursor_dirty`. The tick loop only sends frames for panes
                // whose damage is non-empty (`tick.rs:218`), so without this
                // poke a shell-rejected Backspace at the prompt boundary
                // produces no PTY drain → no damage → the new `received_ack`
                // would never reach the client, and the client's predicted
                // cursor would keep walking past the prompt. The frame this
                // triggers carries the up-to-date `received_ack` plus the
                // server's true (un-moved) cursor — the client uses both to
                // cap the bad prediction inside `on_server_sync`.
                if pane_exists {
                    if let Some(client) = self.clients.get_mut(&_client_id) {
                        let entry = client.received_input_seq.entry(pane_id).or_insert(0);
                        if input_seq > *entry {
                            *entry = input_seq;
                            client
                                .damage
                                .entry(pane_id)
                                .or_default()
                                .cursor_dirty = true;
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
