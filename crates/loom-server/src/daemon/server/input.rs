use loom_protocol::message::*;

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
                let (pane_exists, pane_in_alt_screen) =
                    if let Some(session) = self.sessions.get_mut(session_name)
                        && let Some(pane) = session.panes.get_mut(&pane_id)
                    {
                        pane.write_to_pty(&data);
                        (true, pane.is_alt_screen())
                    } else {
                        (false, false)
                    };
                // Stash the seq as *received*; promotion to `max_input_seq`
                // happens later in `process_pty_and_damage` once the PTY
                // drain proves the framebuffer reflects the input.
                //
                // Off alt-screen, also poke `cursor_dirty` so the tick loop
                // emits a frame carrying the new `received_ack` even when
                // the shell produced no PTY output (Backspace at the prompt
                // boundary). Without this poke the client never learns the
                // input was rejected and `cap_floor` never fires, letting
                // the predicted cursor walk past the boundary indefinitely.
                //
                // In alt-screen we skip the poke: predictions are disabled
                // there (see `prediction/input.rs` mode gates), so
                // `received_ack` has no consumer, and forcing a frame per
                // keystroke would sample alacritty mid-redraw in TUI apps
                // that hide/show the cursor each frame, flickering it off.
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
