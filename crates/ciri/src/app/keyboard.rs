use ciri_input::action::Action;
use ciri_input::keybind::BindingMode;
use ciri_protocol::message::ClientMessage;
use winit::event::ElementState;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};

use super::App;
use super::key_encode::{
    key_event_base_char, key_event_text_for_input, key_event_to_kitty_bytes, key_event_to_pty_bytes,
};

#[derive(Clone, Copy)]
struct KeyModifiers {
    ctrl: bool,
    shift: bool,
    alt: bool,
    super_key: bool,
    caps_lock: bool,
    num_lock: bool,
}

impl KeyModifiers {
    fn from_winit(modifiers: winit::keyboard::ModifiersState) -> Self {
        Self {
            ctrl: modifiers.control_key(),
            shift: modifiers.shift_key(),
            alt: modifiers.alt_key(),
            super_key: modifiers.super_key(),
            caps_lock: modifiers.caps_lock(),
            num_lock: modifiers.num_lock(),
        }
    }
}

impl App {
    fn overlay_paste_target(&self) -> Option<super::PendingPasteTarget> {
        if self.core.pending_paste.is_some() {
            None
        } else if self.core.command_palette.is_some() {
            Some(super::PendingPasteTarget::CommandPalette)
        } else if self.core.search_state.is_some() {
            Some(super::PendingPasteTarget::Search)
        } else {
            None
        }
    }

    fn sanitize_overlay_input(text: &str) -> Option<String> {
        let mut normalized = String::with_capacity(text.len());
        let mut pending_space = false;

        for ch in text.chars() {
            match ch {
                '\r' | '\n' | '\t' => {
                    pending_space = !normalized.is_empty();
                }
                _ if ch.is_control() => {}
                _ => {
                    if pending_space && !normalized.ends_with(' ') {
                        normalized.push(' ');
                    }
                    pending_space = false;
                    normalized.push(ch);
                }
            }
        }

        if normalized.is_empty() {
            None
        } else {
            Some(normalized)
        }
    }

    fn normalize_overlay_paste_text(text: &str) -> Option<String> {
        let normalized = Self::sanitize_overlay_input(text)?;
        let trimmed = normalized.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    fn queue_overlay_paste(&mut self, text: &str) -> bool {
        let Some(target) = self.overlay_paste_target() else {
            return false;
        };
        let Some(normalized) = Self::normalize_overlay_paste_text(text) else {
            return true;
        };

        let threshold = self.core.config.terminal.paste_warn_threshold;
        if let Some(info) = super::paste_guard::check_paste_size(&normalized, threshold) {
            let preview = if normalized.len() > 200 {
                format!("{}...", &normalized[..normalized.floor_char_boundary(200)])
            } else {
                normalized.clone()
            };
            self.core.pending_paste = Some(super::PendingPaste {
                info,
                preview,
                hovered_button: None,
                target,
            });
        } else {
            let _ = self.append_text_to_overlay_input(&normalized);
        }

        true
    }

    pub(crate) fn handle_keyboard_input(
        &mut self,
        event: &winit::event::KeyEvent,
        _event_loop: &ActiveEventLoop,
    ) {
        if self.core.ime.preedit_active {
            return;
        }

        // Handle key release: bare-modifier leader (e.g. Alt) exits leader on release
        if event.state == ElementState::Released {
            let key_name = self.resolve_key_name(event, false);
            if !key_name.is_empty() {
                self.core.input.process_key_release(key_name);
                self.request_redraw();
            }

            if self.modal_captures_keyboard() {
                return;
            }

            // Check if the active pane wants release events (kitty level 2+)
            let kitty_flags = self
                .core
                .workspaces
                .active()
                .active_pane_id()
                .and_then(|pid| self.core.pane_grids.get(&pid))
                .map(|grid| grid.kitty_flags)
                .unwrap_or(0);
            let pane_wants_release =
                kitty_flags & ciri_protocol::message::MODE_KITTY_REPORT_EVENTS != 0;

            if pane_wants_release {
                // Send release event directly to PTY via kitty encoder
                let mods = KeyModifiers::from_winit(self.modifiers);
                let bytes = key_event_to_kitty_bytes(
                    event,
                    mods.ctrl,
                    mods.shift,
                    mods.alt,
                    mods.super_key,
                    mods.caps_lock,
                    mods.num_lock,
                    kitty_flags,
                );
                if !bytes.is_empty() {
                    if self.core.broadcast_mode {
                        let vox = self.core.anim_mgr.view_offset_x.value() as f32;
                        let visible_pids: Vec<u64> = self
                            .core
                            .workspaces
                            .active()
                            .visible_tiles(vox)
                            .iter()
                            .map(|(pid, _, _)| *pid)
                            .collect();
                        for pid in visible_pids {
                            let pane_kitty_flags = self
                                .core
                                .pane_grids
                                .get(&pid)
                                .map(|g| g.kitty_flags)
                                .unwrap_or(0);
                            let pane_wants = pane_kitty_flags
                                & ciri_protocol::message::MODE_KITTY_REPORT_EVENTS
                                != 0;
                            if !pane_wants {
                                continue;
                            }
                            let pane_bytes = if pane_kitty_flags == kitty_flags {
                                bytes.clone()
                            } else {
                                key_event_to_kitty_bytes(
                                    event,
                                    mods.ctrl,
                                    mods.shift,
                                    mods.alt,
                                    mods.super_key,
                                    mods.caps_lock,
                                    mods.num_lock,
                                    pane_kitty_flags,
                                )
                            };
                            self.send(ClientMessage::Input {
                                pane_id: pid,
                                data: pane_bytes,
                                input_seq: 0,
                            });
                        }
                    } else if let Some(pid) = self.core.workspaces.active_mut().active_pane_id() {
                        self.send(ClientMessage::Input {
                            pane_id: pid,
                            data: bytes,
                            input_seq: 0,
                        });
                    }
                }
                self.request_redraw();
            }
            return;
        }

        if self.dismiss_context_menu_on_keypress() {
            return;
        }

        // Esc during any `!connected` state takes priority over every other
        // binding so the user can always bail:
        //   - halted (permanent failure) → dismiss banner, fall back slot
        //   - connecting / reconnecting  → cancel in-flight attempt
        // Other `!connected` shapes (no cancel handle, no reconnect state)
        // mean nothing is in flight — let Esc fall through to the normal
        // binding pipeline.
        if !self.core.connected && matches!(&event.logical_key, Key::Named(NamedKey::Escape)) {
            if self.core.is_halted() {
                self.dismiss_halted_connection();
                self.request_redraw();
                return;
            }
            if self.connection_cancel.is_some() || self.core.reconnect_state.is_some() {
                self.cancel_connection_attempt();
                self.request_redraw();
                return;
            }
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
        let result = self.core.input.process_key_event(
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
            InputResult::PassThrough => {
                if !self.modal_captures_keyboard() {
                    self.send_key_input(event, modifiers);
                }
            }
        }

        self.request_redraw();
    }

    /// Compute the current binding mode from application state.
    fn compute_binding_mode(&self) -> BindingMode {
        let mut mode = self.top_overlay_binding_mode();
        if self.core.input.is_locked() {
            mode |= BindingMode::LOCKED;
        }
        if self.core.input.has_active_table() {
            mode |= BindingMode::KEY_TABLE;
        }
        mode
    }

    fn top_overlay_binding_mode(&self) -> BindingMode {
        if self.core.pending_paste.is_some() {
            BindingMode::PASTE_CONFIRM
        } else if self.core.command_palette.is_some() {
            BindingMode::PALETTE
        } else if self.core.search_state.is_some() {
            BindingMode::SEARCH
        } else if self.core.overview.active {
            BindingMode::OVERVIEW
        } else {
            BindingMode::EMPTY
        }
    }

    pub(crate) fn modal_captures_keyboard(&self) -> bool {
        self.core.context_menu.visible || self.top_overlay_binding_mode() != BindingMode::EMPTY
    }

    pub(crate) fn append_text_to_overlay_input(&mut self, text: &str) -> bool {
        let Some(text) = Self::sanitize_overlay_input(text) else {
            return false;
        };

        if let Some(palette) = &mut self.core.command_palette {
            palette.query.push_str(&text);
            palette.remote_error = None;
            self.filter_palette();
            true
        } else if let Some(search) = &mut self.core.search_state {
            search.query.push_str(&text);
            self.update_search_results();
            true
        } else {
            false
        }
    }

    pub(crate) fn pop_text_from_overlay_input(&mut self) -> bool {
        if let Some(palette) = &mut self.core.command_palette {
            palette.query.pop();
            palette.remote_error = None;
            self.filter_palette();
            true
        } else if let Some(search) = &mut self.core.search_state {
            search.query.pop();
            self.update_search_results();
            true
        } else {
            false
        }
    }

    /// Handle TextInput action: append character to active text buffer (search/palette).
    fn handle_text_input(&mut self, event: &winit::event::KeyEvent, modifiers: KeyModifiers) {
        if modifiers.ctrl {
            return; // Don't accumulate ctrl+key as text
        }
        let Some(text) = key_event_text_for_input(event) else {
            return;
        };
        let _ = self.append_text_to_overlay_input(text);
    }

    fn dismiss_context_menu_on_keypress(&mut self) -> bool {
        if !self.core.context_menu.visible {
            return false;
        }
        self.core.context_menu.visible = false;
        self.request_redraw();
        true
    }

    fn reset_cursor_blink_on_input(&mut self) {
        self.core.cursor_blink_visible = true;
        self.core.cursor_blink_timer = std::time::Instant::now();
    }

    // Priority handlers removed — all keybindings now go through
    // the unified pipeline: compute_binding_mode() + process_key_event().

    pub(crate) fn handle_clipboard_paste(&mut self) {
        log::info!("clipboard paste triggered");
        match &mut self.clipboard {
            None => log::warn!("clipboard not available"),
            Some(cb) => match cb.get_text() {
                Err(e) => log::warn!("clipboard read failed: {e}"),
                Ok(text) => {
                    if self.queue_overlay_paste(&text) {
                        log::info!("clipboard paste routed to overlay flow");
                        return;
                    }
                    if self.modal_captures_keyboard() {
                        log::debug!("clipboard paste swallowed by modal layer");
                        return;
                    }

                    log::info!("clipboard text: {} bytes", text.len());
                    let threshold = self.core.config.terminal.paste_warn_threshold;
                    if let Some(info) = super::paste_guard::check_paste_size(&text, threshold) {
                        let preview = if text.len() > 200 {
                            format!("{}...", &text[..text.floor_char_boundary(200)])
                        } else {
                            text.clone()
                        };
                        let preview = preview.replace('\n', " \\n ").replace('\r', "");
                        self.core.pending_paste = Some(super::PendingPaste {
                            info,
                            preview,
                            hovered_button: None,
                            target: super::PendingPasteTarget::Terminal,
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
            self.core.selection.is_some()
        );
        if let Some(text) = self.extract_selected_text() {
            log::info!("copying {} bytes", text.len());
            if let Some(cb) = &mut self.clipboard {
                let _ = cb.set_text(&text);
            }
        }
    }

    pub(crate) fn send_paste_to_active_pane(&mut self, text: &[u8]) {
        let Some(pid) = self.core.workspaces.active_mut().active_pane_id() else {
            return;
        };
        let bracketed = self
            .core
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
        self.send(ClientMessage::Input {
            pane_id: pid,
            data,
            input_seq: 0,
        });
    }

    fn clear_selection_on_typing(&mut self, event: &winit::event::KeyEvent) {
        let is_modifier_only = matches!(
            event.logical_key,
            Key::Named(NamedKey::Control | NamedKey::Shift | NamedKey::Alt | NamedKey::Super)
        );
        if !is_modifier_only && self.core.config.terminal.clear_selection_on_type {
            self.core.selection = None;
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
                NamedKey::Backspace => "backspace",
                NamedKey::Delete => "delete",
                NamedKey::Home => "home",
                NamedKey::End => "end",
                NamedKey::PageUp => "pageup",
                NamedKey::PageDown => "pagedown",
                NamedKey::Insert => "insert",
                NamedKey::F1 => "f1",
                NamedKey::F2 => "f2",
                NamedKey::F3 => "f3",
                NamedKey::F4 => "f4",
                NamedKey::F5 => "f5",
                NamedKey::F6 => "f6",
                NamedKey::F7 => "f7",
                NamedKey::F8 => "f8",
                NamedKey::F9 => "f9",
                NamedKey::F10 => "f10",
                NamedKey::F11 => "f11",
                NamedKey::F12 => "f12",
                NamedKey::Alt => "Alt",
                NamedKey::Control => "Control",
                NamedKey::Super => "Super",
                _ => "",
            },
            Key::Character(c) => {
                let s = c.as_str();
                if ctrl && s.len() == 1 && s.as_bytes()[0] < 0x20 {
                    key_event_base_char(event)
                        .map(|ch| match ch {
                            ' ' => "space",
                            'a'..='z' => {
                                const LETTERS: [&str; 26] = [
                                    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l",
                                    "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x",
                                    "y", "z",
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
        let active_grid = self
            .core
            .workspaces
            .active()
            .active_pane_id()
            .and_then(|pid| self.core.pane_grids.get(&pid));
        let kitty_flags = active_grid.map(|g| g.kitty_flags).unwrap_or(0);
        let mode_flags = active_grid.map(|g| g.mode_flags).unwrap_or(0);
        let password_mode = active_grid.is_some_and(|g| g.password_input);
        let bytes = self.encode_key_input(event, modifiers, kitty_flags, mode_flags);
        if bytes.is_empty() {
            return;
        }

        self.scroll_active_to_bottom();
        if self.core.broadcast_mode {
            if password_mode {
                // Password input detected on active pane — suppress
                // broadcast to avoid leaking secrets to other panes.
                log::warn!("broadcast suppressed: password input detected on active pane");
                if let Some(pid) = self.core.workspaces.active_mut().active_pane_id() {
                    self.send(ClientMessage::Input {
                        pane_id: pid,
                        data: bytes,
                        input_seq: 0,
                    });
                }
            } else {
                let vox = self.core.anim_mgr.view_offset_x.value() as f32;
                let visible_pids: Vec<u64> = self
                    .core
                    .workspaces
                    .active()
                    .visible_tiles(vox)
                    .iter()
                    .map(|(pid, _, _)| *pid)
                    .collect();
                for pid in visible_pids {
                    let pane_kitty_flags = self
                        .core
                        .pane_grids
                        .get(&pid)
                        .map(|g| g.kitty_flags)
                        .unwrap_or(0);
                    let pane_mode_flags = self
                        .core
                        .pane_grids
                        .get(&pid)
                        .map(|g| g.mode_flags)
                        .unwrap_or(0);
                    let pane_bytes = if pane_kitty_flags == kitty_flags
                        && pane_mode_flags == mode_flags
                    {
                        bytes.clone()
                    } else {
                        self.encode_key_input(event, modifiers, pane_kitty_flags, pane_mode_flags)
                    };
                    self.send(ClientMessage::Input {
                        pane_id: pid,
                        data: pane_bytes,
                        input_seq: 0,
                    });
                }
            }
            return;
        }

        if let Some(pid) = self.core.workspaces.active_mut().active_pane_id() {
            // Skip prediction when the pane is in password input mode
            // to avoid leaking sensitive keystrokes into the prediction engine.
            let seq = self.core.prediction.next_input_seq();
            if !password_mode {
                if let Some(grid) = self.core.pane_grids.get(&pid) {
                    self.core.prediction.new_user_input(pid, &bytes, grid);
                }
            }
            self.send(ClientMessage::Input {
                pane_id: pid,
                data: bytes,
                input_seq: seq,
            });
        }
    }

    fn encode_key_input(
        &self,
        event: &winit::event::KeyEvent,
        modifiers: KeyModifiers,
        kitty_flags: u16,
        mode_flags: u16,
    ) -> Vec<u8> {
        if kitty_flags & ciri_protocol::message::MODE_KITTY_KEYBOARD != 0 {
            key_event_to_kitty_bytes(
                event,
                modifiers.ctrl,
                modifiers.shift,
                modifiers.alt,
                modifiers.super_key,
                modifiers.caps_lock,
                modifiers.num_lock,
                kitty_flags,
            )
        } else {
            let app_cursor = mode_flags & ciri_protocol::message::MODE_APP_CURSOR != 0;
            let app_keypad = mode_flags & ciri_protocol::message::MODE_APP_KEYPAD != 0;
            key_event_to_pty_bytes(
                event,
                modifiers.ctrl,
                modifiers.shift,
                modifiers.alt,
                app_cursor,
                app_keypad,
            )
        }
    }

    fn request_redraw(&mut self) {
        self.schedule_redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, SearchState};
    use ciri_config::config::CiriConfig;

    fn make_app() -> App {
        App::new(CiriConfig::default(), "test-session")
    }

    #[test]
    fn binding_mode_prefers_palette_over_search() {
        let mut app = make_app();
        app.core.search_state = Some(SearchState {
            query: "s".into(),
            matches: Vec::new(),
            current_match_idx: 0,
            pane_id: 1,
            original_scroll_offset: 0,
        });
        app.core.open_command_palette();

        let mode = app.compute_binding_mode();
        assert!(mode.contains(BindingMode::PALETTE));
        assert!(!mode.contains(BindingMode::SEARCH));
    }

    #[test]
    fn overlay_text_input_prefers_palette_over_search() {
        let mut app = make_app();
        app.core.search_state = Some(SearchState {
            query: "search".into(),
            matches: Vec::new(),
            current_match_idx: 0,
            pane_id: 1,
            original_scroll_offset: 0,
        });
        app.core.open_command_palette();

        assert!(app.append_text_to_overlay_input("x"));
        assert_eq!(
            app.core.command_palette.as_ref().map(|p| p.query.as_str()),
            Some("x")
        );
        assert_eq!(
            app.core.search_state.as_ref().map(|s| s.query.as_str()),
            Some("search")
        );
    }

    #[test]
    fn overlay_backspace_prefers_palette_over_search() {
        let mut app = make_app();
        app.core.search_state = Some(SearchState {
            query: "search".into(),
            matches: Vec::new(),
            current_match_idx: 0,
            pane_id: 1,
            original_scroll_offset: 0,
        });
        app.core.open_command_palette();
        if let Some(palette) = &mut app.core.command_palette {
            palette.query = "palette".into();
        }

        assert!(app.pop_text_from_overlay_input());
        assert_eq!(
            app.core.command_palette.as_ref().map(|p| p.query.as_str()),
            Some("palett")
        );
        assert_eq!(
            app.core.search_state.as_ref().map(|s| s.query.as_str()),
            Some("search")
        );
    }

    #[test]
    fn overlay_paste_normalizes_multiline_text() {
        let mut app = make_app();
        app.core.open_command_palette();

        assert!(app.queue_overlay_paste("ssh\nuser@host\r\n"));
        assert_eq!(
            app.core.command_palette.as_ref().map(|p| p.query.as_str()),
            Some("ssh user@host")
        );
        assert!(app.core.pending_paste.is_none());
    }

    #[test]
    fn overlay_large_paste_requires_confirmation() {
        let mut app = make_app();
        app.core.config.terminal.paste_warn_threshold = 3;
        app.core.open_command_palette();

        assert!(app.queue_overlay_paste("ab\ncd"));
        let pending = app.core.pending_paste.as_ref().expect("pending paste");
        assert_eq!(
            pending.target,
            crate::app::PendingPasteTarget::CommandPalette
        );
        assert_eq!(pending.info.text, "ab cd");
        assert_eq!(
            app.core.command_palette.as_ref().map(|p| p.query.as_str()),
            Some("")
        );

        app.confirm_pending_paste();
        assert!(app.core.pending_paste.is_none());
        assert_eq!(
            app.core.command_palette.as_ref().map(|p| p.query.as_str()),
            Some("ab cd")
        );
    }
}
