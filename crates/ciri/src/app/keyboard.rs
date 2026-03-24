use ciri_input::action::Action;
use ciri_input::keybind::KeyCombo;
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

        if self.handle_priority_keyboard_input(event, modifiers) {
            self.request_redraw();
            return;
        }

        self.clear_selection_on_typing(event);
        let key_name = self.resolve_key_name(event, modifiers.ctrl);

        if self.overview.active {
            self.handle_overview_keyboard_input(key_name, modifiers);
        } else {
            self.handle_regular_keyboard_input(event, key_name, modifiers);
        }

        self.request_redraw();
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

    fn handle_priority_keyboard_input(
        &mut self,
        event: &winit::event::KeyEvent,
        modifiers: KeyModifiers,
    ) -> bool {
        self.try_enter_search_mode(event, modifiers)
            || self.handle_pending_paste_key(event)
            || self.handle_search_mode_key(event, modifiers)
            || self.handle_command_palette_mode_key(event, modifiers)
            || self.handle_clipboard_shortcuts(event, modifiers)
    }

    fn try_enter_search_mode(
        &mut self,
        event: &winit::event::KeyEvent,
        modifiers: KeyModifiers,
    ) -> bool {
        use winit::keyboard::{KeyCode, PhysicalKey};

        if !(modifiers.ctrl && modifiers.shift) {
            return false;
        }
        if event.physical_key != PhysicalKey::Code(KeyCode::KeyF) {
            return false;
        }

        let Some(pane_id) = self.workspaces.active().active_pane_id() else {
            return true;
        };
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
        true
    }

    fn handle_pending_paste_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        if self.pending_paste.is_none() {
            return false;
        }
        if matches!(&event.logical_key, Key::Named(NamedKey::Escape)) {
            self.pending_paste = None;
        }
        true
    }

    fn handle_search_mode_key(
        &mut self,
        event: &winit::event::KeyEvent,
        modifiers: KeyModifiers,
    ) -> bool {
        if self.search_state.is_none() {
            return false;
        }
        self.handle_search_key(event, modifiers.ctrl, modifiers.shift);
        true
    }

    fn handle_command_palette_mode_key(
        &mut self,
        event: &winit::event::KeyEvent,
        modifiers: KeyModifiers,
    ) -> bool {
        if self.command_palette.is_none() {
            return false;
        }
        self.handle_command_palette_key(event, modifiers.ctrl, modifiers.shift, modifiers.alt);
        true
    }

    fn handle_clipboard_shortcuts(
        &mut self,
        event: &winit::event::KeyEvent,
        modifiers: KeyModifiers,
    ) -> bool {
        use winit::keyboard::{KeyCode, PhysicalKey};

        if !(modifiers.ctrl && modifiers.shift) {
            return false;
        }

        log::info!("ctrl+shift detected, physical={:?}", event.physical_key);
        match event.physical_key {
            PhysicalKey::Code(KeyCode::KeyV) => {
                self.handle_clipboard_paste();
                true
            }
            PhysicalKey::Code(KeyCode::KeyC) => {
                self.handle_clipboard_copy();
                true
            }
            _ => false,
        }
    }

    fn handle_clipboard_paste(&mut self) {
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

    fn handle_clipboard_copy(&mut self) {
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

    fn handle_overview_keyboard_input(&mut self, key_name: &str, modifiers: KeyModifiers) {
        let combo = KeyCombo::from_modifiers(
            &key_name.to_lowercase(),
            modifiers.ctrl,
            modifiers.shift,
            modifiers.alt,
            modifiers.super_key,
        );
        if let Some(action) = self.overview_keybinds.lookup(&combo) {
            match action {
                Action::ExitOverview => self.exit_overview(),
                other => self.handle_action(other),
            }
        }
    }

    fn handle_regular_keyboard_input(
        &mut self,
        event: &winit::event::KeyEvent,
        key_name: &str,
        modifiers: KeyModifiers,
    ) {
        use ciri_input::leader::InputResult;

        let result = if key_name.is_empty() {
            InputResult::PassThrough
        } else {
            self.input.process_key(
                key_name,
                modifiers.ctrl,
                modifiers.shift,
                modifiers.alt,
                modifiers.super_key,
            )
        };

        match result {
            InputResult::Action(action) => self.handle_action(action),
            InputResult::Consumed => {}
            InputResult::PassThrough => self.send_key_input(event, modifiers),
        }
    }

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
            let vox = self.view_offset_x.value() as f32;
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
