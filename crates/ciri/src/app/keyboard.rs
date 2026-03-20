use ciri_input::action::Action;
use ciri_input::keybind::KeyCombo;
use ciri_protocol::message::ClientMessage;
use winit::event::ElementState;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};

use super::App;
use super::input_handler::{key_event_to_kitty_bytes, key_event_to_pty_bytes};

impl App {
    pub(crate) fn handle_keyboard_input(
        &mut self,
        event: &winit::event::KeyEvent,
        _event_loop: &ActiveEventLoop,
    ) {
        if event.state != ElementState::Pressed {
            return;
        }
        if self.ime.preedit_active {
            return;
        }

        self.cursor_blink_visible = true;
        self.cursor_blink_timer = std::time::Instant::now();

        let ctrl = self.modifiers.control_key();
        let shift = self.modifiers.shift_key();
        let alt = self.modifiers.alt_key();
        let super_key = self.modifiers.super_key();

        log::debug!(
            "key: ctrl={ctrl} shift={shift} alt={alt} super={super_key} logical={:?} physical={:?}",
            event.logical_key,
            event.physical_key
        );

        // Ctrl+Shift+F: enter search mode
        if ctrl && shift {
            use winit::keyboard::{KeyCode, PhysicalKey};
            if event.physical_key == PhysicalKey::Code(KeyCode::KeyF) {
                if let Some(pane_id) = self.workspaces.active().active_pane_id() {
                    let scroll_offset = self
                        .pane_grids
                        .get(&pane_id)
                        .map(|g| g.scroll_offset)
                        .unwrap_or(0);
                    self.search_state = Some(super::SearchState {
                        query: String::new(),
                        matches: Vec::new(),
                        current_match_idx: 0,
                        pane_id,
                        original_scroll_offset: scroll_offset,
                    });
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
                return;
            }
        }

        // Search mode: intercept all input
        if self.search_state.is_some() {
            self.handle_search_key(event, ctrl, shift);
            if let Some(w) = &self.window {
                w.request_redraw();
            }
            return;
        }

        // Clipboard: Ctrl+Shift+V / Ctrl+Shift+C
        if ctrl && shift {
            use winit::keyboard::{KeyCode, PhysicalKey};
            log::info!("ctrl+shift detected, physical={:?}", event.physical_key);
            match event.physical_key {
                PhysicalKey::Code(KeyCode::KeyV) => {
                    log::info!("clipboard paste triggered");
                    match &mut self.clipboard {
                        None => log::warn!("clipboard not available"),
                        Some(cb) => match cb.get_text() {
                            Err(e) => log::warn!("clipboard read failed: {e}"),
                            Ok(text) => {
                                log::info!("clipboard text: {} bytes", text.len());
                                if let Some(pid) =
                                    self.workspaces.active_mut().active_pane_id()
                                {
                                    let bracketed = self
                                        .pane_grids
                                        .get(&pid)
                                        .is_some_and(|g| {
                                            g.mode_flags
                                                & ciri_protocol::message::MODE_BRACKETED_PASTE
                                                != 0
                                        });
                                    let mut data = Vec::with_capacity(
                                        text.len() + if bracketed { 12 } else { 0 },
                                    );
                                    if bracketed {
                                        data.extend_from_slice(b"\x1b[200~");
                                    }
                                    data.extend_from_slice(text.as_bytes());
                                    if bracketed {
                                        data.extend_from_slice(b"\x1b[201~");
                                    }
                                    self.send(ClientMessage::Input {
                                        pane_id: pid,
                                        data,
                                    });
                                }
                            }
                        },
                    }
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                    return;
                }
                PhysicalKey::Code(KeyCode::KeyC) => {
                    log::info!(
                        "clipboard copy triggered, selection={}",
                        self.selection.is_some()
                    );
                    if let Some(text) = self.extract_selected_text() {
                        log::info!("copying {} bytes", text.len());
                        if let Some(cb) = &mut self.clipboard {
                            let _ = cb.set_text(&text);
                        }
                    }
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                    return;
                }
                _ => {}
            }
        }

        let is_modifier_only = matches!(
            event.logical_key,
            Key::Named(
                NamedKey::Control | NamedKey::Shift | NamedKey::Alt | NamedKey::Super
            )
        );
        if !is_modifier_only {
            self.selection = None;
        }

        // Resolve key name: prefer logical_key, but fall back to
        // key_without_modifiers for Ctrl combos where logical_key
        // becomes a control character (e.g. Ctrl+Space -> '\0').
        let key_name = match &event.logical_key {
            Key::Named(n) => match n {
                NamedKey::Space => "space",
                NamedKey::Tab => "tab",
                NamedKey::Escape => "escape",
                NamedKey::Enter => "enter",
                NamedKey::ArrowLeft => "Left",
                NamedKey::ArrowRight => "Right",
                NamedKey::ArrowUp => "Up",
                NamedKey::ArrowDown => "Down",
                NamedKey::Alt => "Alt",
                NamedKey::Control => "Control",
                NamedKey::Super => "Super",
                _ => "",
            },
            Key::Character(c) => {
                let s = c.as_str();
                // Ctrl turns letters into control chars (e.g. Ctrl+W -> 0x17).
                // Fall back to physical key to recover the original letter.
                if ctrl && s.len() == 1 && s.as_bytes()[0] < 0x20 {
                    use winit::keyboard::{KeyCode, PhysicalKey};
                    match event.physical_key {
                        PhysicalKey::Code(code) => match code {
                            KeyCode::Space => "space",
                            KeyCode::KeyA => "a", KeyCode::KeyB => "b",
                            KeyCode::KeyC => "c", KeyCode::KeyD => "d",
                            KeyCode::KeyE => "e", KeyCode::KeyF => "f",
                            KeyCode::KeyG => "g", KeyCode::KeyH => "h",
                            KeyCode::KeyI => "i", KeyCode::KeyJ => "j",
                            KeyCode::KeyK => "k", KeyCode::KeyL => "l",
                            KeyCode::KeyM => "m", KeyCode::KeyN => "n",
                            KeyCode::KeyO => "o", KeyCode::KeyP => "p",
                            KeyCode::KeyQ => "q", KeyCode::KeyR => "r",
                            KeyCode::KeyS => "s", KeyCode::KeyT => "t",
                            KeyCode::KeyU => "u", KeyCode::KeyV => "v",
                            KeyCode::KeyW => "w", KeyCode::KeyX => "x",
                            KeyCode::KeyY => "y", KeyCode::KeyZ => "z",
                            _ => s,
                        },
                        _ => s,
                    }
                } else {
                    s
                }
            }
            _ => "",
        };

        if self.overview.active {
            let combo = KeyCombo::from_modifiers(
                &key_name.to_lowercase(),
                ctrl,
                shift,
                alt,
                super_key,
            );
            if let Some(action) = self.overview_keybinds.lookup(&combo) {
                match action {
                    Action::ExitOverview => {
                        self.overview.active = false;
                        self.overview.zoom
                            .animate_to(1.0, self.config.animation.speed);
                        self.animate_to_active();
                    }
                    other => self.handle_action(other),
                }
            }
        } else {
            use ciri_input::leader::InputResult;
            let result = if !key_name.is_empty() {
                self.input
                    .process_key(key_name, ctrl, shift, alt, super_key)
            } else {
                InputResult::PassThrough
            };

            match result {
                InputResult::Action(action) => self.handle_action(action),
                InputResult::Consumed => {}
                InputResult::PassThrough => {
                    self.scroll_active_to_bottom();
                    // Use Kitty keyboard encoding if the active pane has it enabled
                    let use_kitty = self.workspaces.active().active_pane_id()
                        .and_then(|pid| self.pane_grids.get(&pid))
                        .is_some_and(|grid| grid.has_kitty_keyboard);
                    let bytes = if use_kitty {
                        key_event_to_kitty_bytes(event, ctrl, shift, alt, super_key)
                    } else {
                        key_event_to_pty_bytes(event, ctrl)
                    };
                    if !bytes.is_empty() {
                        if self.broadcast_mode {
                            // Send to visible panes only — off-screen panes should
                            // not receive destructive commands unexpectedly.
                            let vox = self.view_offset_x.value() as f32;
                            let visible_pids: Vec<u64> = self.workspaces.active()
                                .visible_tiles(vox)
                                .iter()
                                .map(|(pid, _, _)| *pid)
                                .collect();
                            for pid in visible_pids {
                                let pane_kitty = self.pane_grids.get(&pid)
                                    .is_some_and(|g| g.has_kitty_keyboard);
                                let pane_bytes = if pane_kitty == use_kitty {
                                    bytes.clone()
                                } else if pane_kitty {
                                    key_event_to_kitty_bytes(event, ctrl, shift, alt, super_key)
                                } else {
                                    key_event_to_pty_bytes(event, ctrl)
                                };
                                self.send(ClientMessage::Input {
                                    pane_id: pid,
                                    data: pane_bytes,
                                });
                            }
                        } else if let Some(pid) = self.workspaces.active_mut().active_pane_id() {
                            self.send(ClientMessage::Input {
                                pane_id: pid,
                                data: bytes,
                            });
                        }
                    }
                }
            }
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}
