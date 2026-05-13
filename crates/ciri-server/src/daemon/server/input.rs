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
                // Capture alt-screen state while we have pane access; used
                // below to decide whether to poke `cursor_dirty` for the
                // received_ack flow.
                let (pane_exists, pane_in_alt_screen) =
                    if let Some(session) = self.sessions.get_mut(session_name)
                        && let Some(pane) = session.panes.get_mut(&pane_id)
                    {
                        pane.write_to_pty(&data);
                        (true, pane.is_alt_screen())
                    } else {
                        (false, false)
                    };
                // Stash the seq as *received*. It only gets promoted to the
                // ack-able `max_input_seq` once the pane's PTY produces output
                // (see `Session::promote_received_input_seqs`), so clients
                // don't see an echo_ack until the framebuffer reflects this
                // input. Gated on pane existence to prevent malicious clients
                // from bloating the HashMap.
                //
                // We also mark the pane's damage accumulator as `cursor_dirty`
                // so the tick loop (which only sends frames for panes with
                // non-empty damage, see `tick.rs:218`) actually pushes a frame
                // carrying the new `received_ack`. Without that poke, a
                // shell-rejected Backspace at the prompt boundary produces no
                // PTY drain → no damage → no frame → received_ack never
                // reaches the client and the predicted cursor keeps walking
                // past the prompt.
                //
                // **But skip the poke in alt-screen mode.** The prediction
                // engine doesn't speculate in alt-screen (see
                // `prediction/input.rs` mode gates), so received_ack has no
                // consumer there. And — crucial — TUI apps like Codex sit in
                // alt-screen and redraw continuously with hide/show cursor
                // bracketing each frame. Forcing an extra frame on every
                // keystroke causes the server to sample alacritty mid-redraw,
                // catching transient CURSOR_HIDDEN states and flickering the
                // client cursor off.
                if pane_exists {
                    if let Some(client) = self.clients.get_mut(&_client_id) {
                        let entry = client.received_input_seq.entry(pane_id).or_insert(0);
                        if input_seq > *entry {
                            *entry = input_seq;
                            if !pane_in_alt_screen {
                                client
                                    .damage
                                    .entry(pane_id)
                                    .or_default()
                                    .cursor_dirty = true;
                            }
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
