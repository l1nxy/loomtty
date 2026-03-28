use ciri_input::action::Action;
use ciri_input::keybind::BindingMode;
use ciri_protocol::message::ClientMessage;
use winit::event::ElementState;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};

use super::App;
use super::input_handler::{key_event_to_kitty_bytes, key_event_to_pty_bytes};

#[derive(Clone, Copy)]
struct KeyModifiers {
    ctrl: bool,
    shift: bool,
    alt: bool,
    super_key: bool,
}

impl KeyModifiers {
    fn from_winit(modifiers: winit::keyboard::ModifiersState) -> Self {
        Self {
            ctrl: modifiers.control_key(),
            shift: modifiers.shift_key(),
            alt: modifiers.alt_key(),
            super_key: modifiers.super_key(),
        }
    }
}

impl App {
    pub(crate) fn handle_keyboard_input(
        &mut self,
        event: &winit::event::KeyEvent,
        _event_loop: &ActiveEventLoop,
    ) {
        if self.ime.preedit_active {
            return;
        }

        // Handle key release: bare-modifier leader (e.g. Alt) exits leader on release
        if event.state == ElementState::Released {
            let key_name = self.resolve_key_name(event, false);
            if !key_name.is_empty() {
                self.input.process_key_release(key_name);
                self.request_redraw();
            }
            return;
        }

        if self.dismiss_context_menu_on_keypress() {
            return;
        }

        self.reset_cursor_blink_on_input();
        let modifiers = KeyModifiers::from_winit(self.modifiers);

        log::debug!(
            "key: ctrl={} shift={} alt={} super={} logical={:?} physical={:?}",
            modifiers.ctrl,
            modifiers.shift,
            modifiers.alt,
            modifiers.super_key,
            event.logical_key,
            event.physical_key
        );

        self.clear_selection_on_typing(event);
        let key_name = self.resolve_key_name(event, modifiers.ctrl);
        if key_name.is_empty() {
            self.request_redraw();
            return;
        }

        // ── Unified pipeline: compute mode → process key → handle action ──
        let app_mode = self.compute_binding_mode();
        let result = self.input.process_key_v2(
            key_name,
            modifiers.ctrl,
            modifiers.shift,
            modifiers.alt,
            modifiers.super_key,
            app_mode,
        );

        use ciri_input::leader::InputResult;
        match result {
            InputResult::Action(Action::TextInput) => {
                self.handle_text_input(event, modifiers);
            }
            InputResult::Action(action) => self.handle_action(action),
            InputResult::Consumed => {}
            InputResult::PassThrough => self.send_key_input(event, modifiers),
        }

        self.request_redraw();
    }

    /// Compute the current binding mode from application state.
    fn compute_binding_mode(&self) -> BindingMode {
        let mut mode = BindingMode::EMPTY;
        if self.overview.active {
            mode |= BindingMode::OVERVIEW;
        }
        if self.search_state.is_some() {
            mode |= BindingMode::SEARCH;
        }
        if self.command_palette.is_some() {
            mode |= BindingMode::PALETTE;
        }
        if self.pending_paste.is_some() {
            mode |= BindingMode::PASTE_CONFIRM;
        }
        if self.input.is_locked() {
            mode |= BindingMode::LOCKED;
        }
        if self.input.has_active_table() {
            mode |= BindingMode::KEY_TABLE;
        }
        mode
    }

    /// Handle TextInput action: append character to active text buffer (search/palette).
    fn handle_text_input(&mut self, event: &winit::event::KeyEvent, modifiers: KeyModifiers) {
        if modifiers.ctrl {
            return; // Don't accumulate ctrl+key as text
        }
        let Key::Character(c) = &event.logical_key else {
            return;
        };
        let s: &str = c.as_str();

        if let Some(search) = &mut self.search_state {
            search.query.push_str(s);
            self.update_search_results();
        } else if let Some(palette) = &mut self.command_palette {
            palette.query.push_str(s);
            self.filter_palette();
        }
    }

    fn dismiss_context_menu_on_keypress(&mut self) -> bool {
        if !self.context_menu.visible {
            return false;
        }
        self.context_menu.visible = false;
        self.request_redraw();
        true
    }

    fn reset_cursor_blink_on_input(&mut self) {
        self.cursor_blink_visible = true;
        self.cursor_blink_timer = std::time::Instant::now();
    }

    // Priority handlers removed — all keybindings now go through
    // the unified pipeline: compute_binding_mode() + process_key_v2().

    pub(crate) fn handle_clipboard_paste(&mut self) {
        log::info!("clipboard paste triggered");
        match &mut self.clipboard {
            None => log::warn!("clipboard not available"),
            Some(cb) => match cb.get_text() {
                Err(e) => log::warn!("clipboard read failed: {e}"),
                Ok(text) => {
                    log::info!("clipboard text: {} bytes", text.len());
                    let threshold = self.config.terminal.paste_warn_threshold;
                    if let Some(info) = super::paste_guard::check_paste_size(&text, threshold) {
                        let preview = if text.len() > 200 {
                            format!("{}...", &text[..text.floor_char_boundary(200)])
                        } else {
                            text.clone()
                        };
                        let preview = preview.replace('\n', " \\n ").replace('\r', "");
                        self.pending_paste = Some(super::PendingPaste {
                            info,
                            preview,
                            hovered_button: None,
                        });
                        log::info!("paste guard: showing confirmation ({} bytes)", text.len());
                    } else {
                        self.send_paste_to_active_pane(text.as_bytes());
                    }
                }
            },
        }
    }

    pub(crate) fn handle_clipboard_copy(&mut self) {
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
    }

    pub(crate) fn send_paste_to_active_pane(&mut self, text: &[u8]) {
        let Some(pid) = self.workspaces.active_mut().active_pane_id() else {
            return;
        };
        let bracketed = self
            .pane_grids
            .get(&pid)
            .is_some_and(|g| g.mode_flags & ciri_protocol::message::MODE_BRACKETED_PASTE != 0);
        let mut data = Vec::with_capacity(text.len() + if bracketed { 12 } else { 0 });
        if bracketed {
            data.extend_from_slice(b"\x1b[200~");
        }
        data.extend_from_slice(text);
        if bracketed {
            data.extend_from_slice(b"\x1b[201~");
        }
        self.send(ClientMessage::Input { pane_id: pid, data });
    }

    fn clear_selection_on_typing(&mut self, event: &winit::event::KeyEvent) {
        let is_modifier_only = matches!(
            event.logical_key,
            Key::Named(NamedKey::Control | NamedKey::Shift | NamedKey::Alt | NamedKey::Super)
        );
        if !is_modifier_only && self.config.terminal.clear_selection_on_type {
            self.selection = None;
        }
    }

    fn resolve_key_name<'a>(&self, event: &'a winit::event::KeyEvent, ctrl: bool) -> &'a str {
        match &event.logical_key {
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
                if ctrl && s.len() == 1 && s.as_bytes()[0] < 0x20 {
                    super::input_handler::physical_key_to_base_char(event.physical_key)
                        .map(|ch| match ch {
                            ' ' => "space",
                            'a'..='z' => {
                                const LETTERS: [&str; 26] = [
                                    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k",
                                    "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v",
                                    "w", "x", "y", "z",
                                ];
                                LETTERS[(ch as u8 - b'a') as usize]
                            }
                            '0'..='9' => {
                                const DIGITS: [&str; 10] =
                                    ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
                                DIGITS[(ch as u8 - b'0') as usize]
                            }
                            _ => s,
                        })
                        .unwrap_or(s)
                } else {
                    s
                }
            }
            _ => "",
        }
    }

    // handle_overview_keyboard_input and handle_regular_keyboard_input removed —
    // all key processing now goes through the unified pipeline above.

    fn send_key_input(&mut self, event: &winit::event::KeyEvent, modifiers: KeyModifiers) {
        let use_kitty = self
            .workspaces
            .active()
            .active_pane_id()
            .and_then(|pid| self.pane_grids.get(&pid))
            .is_some_and(|grid| grid.has_kitty_keyboard);
        let bytes = self.encode_key_input(event, modifiers, use_kitty);
        if bytes.is_empty() {
            return;
        }

        self.scroll_active_to_bottom();
        if self.broadcast_mode {
            let vox = self.anim_mgr.view_offset_x.value() as f32;
            let visible_pids: Vec<u64> = self
                .workspaces
                .active()
                .visible_tiles(vox)
                .iter()
                .map(|(pid, _, _)| *pid)
                .collect();
            for pid in visible_pids {
                let pane_kitty = self
                    .pane_grids
                    .get(&pid)
                    .is_some_and(|g| g.has_kitty_keyboard);
                let pane_bytes = if pane_kitty == use_kitty {
                    bytes.clone()
                } else {
                    self.encode_key_input(event, modifiers, pane_kitty)
                };
                self.send(ClientMessage::Input {
                    pane_id: pid,
                    data: pane_bytes,
                });
            }
            return;
        }

        if let Some(pid) = self.workspaces.active_mut().active_pane_id() {
            self.send(ClientMessage::Input {
                pane_id: pid,
                data: bytes,
            });
        }
    }

    fn encode_key_input(
        &self,
        event: &winit::event::KeyEvent,
        modifiers: KeyModifiers,
        use_kitty: bool,
    ) -> Vec<u8> {
        if use_kitty {
            key_event_to_kitty_bytes(
                event,
                modifiers.ctrl,
                modifiers.shift,
                modifiers.alt,
                modifiers.super_key,
            )
        } else {
            key_event_to_pty_bytes(event, modifiers.ctrl)
        }
    }

    fn request_redraw(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}
