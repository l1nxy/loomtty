use ciri_config::config::CiriConfig;
use ciri_input::action::Action;
use ciri_input::keybind::{BindingSet, KeybindMap};
use ciri_input::leader::InputHandler;
use ciri_protocol::message::*;
use std::process::Command;
use std::time::Instant;
use winit::keyboard::{Key, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

use super::App;

impl App {
    /// Build unified BindingSet from all config sources and install it.
    pub(crate) fn rebuild_binding_set(input: &mut InputHandler, config: &CiriConfig) {
        input.set_binding_set(BindingSet::from_legacy(
            &input.keybinds,
            &input.direct_keybinds,
            &input.mode_keybinds,
            &KeybindMap::from_overview_config(&config.keys.overview_bindings),
            &config.keys.search_bindings,
            &config.keys.palette_bindings,
            &config.keys.paste_confirm_bindings,
        ));
    }

    pub fn handle_action(&mut self, action: Action) {
        match action {
            Action::NewColumnRight => {
                self.send(ClientMessage::CreatePane);
            }
            Action::NewWorkspaceBelow => {
                self.send(ClientMessage::SplitDown);
            }
            Action::ClosePane => {
                if let Some(pane_id) = self.core.workspaces.active_mut().active_pane_id() {
                    self.send(ClientMessage::ClosePane { pane_id });
                }
            }
            Action::FocusLeft => {
                self.send(ClientMessage::FocusLeft);
            }
            Action::FocusRight => {
                self.send(ClientMessage::FocusRight);
            }
            Action::FocusDown => {
                self.send(ClientMessage::FocusDown);
            }
            Action::FocusUp => {
                self.send(ClientMessage::FocusUp);
            }
            Action::MovePaneLeft => {
                self.send(ClientMessage::MovePaneLeft);
            }
            Action::MovePaneRight => {
                self.send(ClientMessage::MovePaneRight);
            }
            Action::CyclePresetWidth | Action::CyclePresetWidthReverse => {
                let reverse = matches!(action, Action::CyclePresetWidthReverse);
                let presets = self.preset_widths();
                if let Some(w) = self
                    .core
                    .workspaces
                    .active_mut()
                    .cycle_preset_width(&presets, reverse)
                {
                    let (proportion, fixed_px) = match w {
                        ciri_layout::column::ColumnWidth::Proportion(p) => (p, None),
                        ciri_layout::column::ColumnWidth::Fixed(px) => {
                            let vw = self.core.workspaces.view_size.width as f64;
                            let p = if vw > 0.0 { px / vw } else { 0.5 };
                            (p, Some(px))
                        }
                    };
                    self.send(ClientMessage::SetColumnWidth {
                        proportion,
                        fixed_px,
                    });
                }
                self.snap_all_col_widths();
                self.animate_to_active();
            }
            Action::ColumnWidthOneThird => {
                self.send(ClientMessage::SetColumnWidth {
                    proportion: 1.0 / 3.0,
                    fixed_px: None,
                });
            }
            Action::ColumnWidthHalf => {
                self.send(ClientMessage::SetColumnWidth {
                    proportion: 0.5,
                    fixed_px: None,
                });
            }
            Action::ColumnWidthTwoThirds => {
                self.send(ClientMessage::SetColumnWidth {
                    proportion: 2.0 / 3.0,
                    fixed_px: None,
                });
            }
            Action::ColumnWidthFull => {
                self.send(ClientMessage::SetColumnWidth {
                    proportion: 1.0,
                    fixed_px: None,
                });
            }
            Action::ColumnWidthIncrease => {
                self.send(ClientMessage::AdjustColumnSplit { delta: 0.05 });
            }
            Action::ColumnWidthDecrease => {
                self.send(ClientMessage::AdjustColumnSplit { delta: -0.05 });
            }
            Action::EqualizeAdjacentColumns => {
                self.send(ClientMessage::EqualizeColumnSplit);
            }
            Action::ToggleBroadcast => {
                self.core.broadcast_mode = !self.core.broadcast_mode;
                log::info!("broadcast mode: {}", self.core.broadcast_mode);
            }
            Action::ConsumeIntoColumn => {
                self.send(ClientMessage::ConsumeIntoColumn);
            }
            Action::ExpelFromColumn => {
                self.send(ClientMessage::ExpelFromColumn);
            }
            Action::ExitOverview => self.exit_overview(),
            Action::SwitchWorkspace(idx) => {
                self.send(ClientMessage::SwitchWorkspace { workspace_idx: idx });
            }
            Action::ToggleOverview => self.toggle_overview(),
            Action::SendLeaderKey => {
                if let Some(pid) = self.core.workspaces.active_mut().active_pane_id() {
                    self.send(ClientMessage::Input {
                        pane_id: pid,
                        data: vec![0x17],
                    });
                }
            }
            Action::ScrollPageUp => {
                let rows = self
                    .core
                    .pane_grids
                    .values()
                    .next()
                    .map(|g| g.rows as usize)
                    .unwrap_or(24);
                self.scroll_active_up(rows);
            }
            Action::ScrollPageDown => {
                let rows = self
                    .core
                    .pane_grids
                    .values()
                    .next()
                    .map(|g| g.rows as usize)
                    .unwrap_or(24);
                self.scroll_active_down(rows);
            }
            Action::ScrollHalfPageUp => {
                let rows = self
                    .core
                    .pane_grids
                    .values()
                    .next()
                    .map(|g| (g.rows as usize) / 2)
                    .unwrap_or(12);
                self.scroll_active_up(rows.max(1));
            }
            Action::ScrollHalfPageDown => {
                let rows = self
                    .core
                    .pane_grids
                    .values()
                    .next()
                    .map(|g| (g.rows as usize) / 2)
                    .unwrap_or(12);
                self.scroll_active_down(rows.max(1));
            }
            Action::ScrollLineUp => {
                self.scroll_active_up(1);
            }
            Action::ScrollLineDown => {
                self.scroll_active_down(1);
            }
            Action::ScrollTop => {
                if let Some(pid) = self.core.workspaces.active().active_pane_id()
                    && let Some(grid) = self.core.pane_grids.get_mut(&pid)
                {
                    grid.scroll_up(grid.max_scroll_offset());
                    self.invalidate_pane_cache(pid);
                }
            }
            Action::ScrollBottom => {
                self.scroll_active_to_bottom();
            }
            Action::Detach => {
                self.send(ClientMessage::Detach);
                self.core.should_exit = true;
            }
            Action::ToggleCommandPalette => {
                if self.core.command_palette.is_some() {
                    self.core.command_palette = None;
                } else {
                    self.open_command_palette();
                }
            }
            Action::EnterMode(_) => {
                // State transition handled by InputHandler::process_key
            }
            Action::ToggleLock => {
                self.core.input.toggle_lock();
            }
            // ── Search ──
            Action::OpenSearch => {
                self.open_search();
            }
            Action::CloseSearch => {
                self.close_search_restore_scroll();
            }
            Action::SearchNextMatch => {
                if self
                    .core
                    .search_state
                    .as_ref()
                    .is_some_and(|s| s.query.is_empty())
                {
                    // Empty query: just exit search
                    self.core.search_state = None;
                } else {
                    self.jump_to_match(false);
                }
            }
            Action::SearchPrevMatch => {
                self.jump_to_match(true);
            }

            // ── Command palette ──
            Action::CloseCommandPalette => {
                self.core.command_palette = None;
            }
            Action::PaletteUp => {
                if let Some(palette) = &mut self.core.command_palette
                    && !palette.filtered.is_empty()
                {
                    palette.selected_idx = if palette.selected_idx == 0 {
                        palette.filtered.len() - 1
                    } else {
                        palette.selected_idx - 1
                    };
                }
            }
            Action::PaletteDown => {
                if let Some(palette) = &mut self.core.command_palette
                    && !palette.filtered.is_empty()
                {
                    palette.selected_idx = (palette.selected_idx + 1) % palette.filtered.len();
                }
            }
            Action::PaletteConfirm => {
                self.execute_palette_selection();
            }

            // ── Clipboard ──
            Action::ClipboardCopy => {
                self.handle_clipboard_copy();
            }
            Action::ClipboardPaste => {
                self.handle_clipboard_paste();
            }

            // ── Paste confirmation ──
            Action::ConfirmPaste => {
                if self.core.pending_paste.take().is_some() {
                    self.handle_clipboard_paste_force();
                }
            }
            Action::DismissPasteConfirm => {
                self.core.pending_paste = None;
            }

            // ── Text input ──
            Action::TextInput => {
                // Handled in keyboard.rs handle_text_input(); should not reach here.
                log::debug!("TextInput action reached handle_action (unexpected)");
            }
            Action::TextBackspace => {
                if let Some(search) = &mut self.core.search_state {
                    search.query.pop();
                    self.update_search_results();
                } else if let Some(palette) = &mut self.core.command_palette {
                    palette.query.pop();
                    self.filter_palette();
                }
            }

            // ── Key table management (handled by InputHandler internally) ──
            Action::ActivateKeyTable(_) | Action::DeactivateKeyTable => {
                // State transitions already handled in process_key_v2.
            }
        }
    }

    /// Delegate: extract selected text.
    pub fn extract_selected_text(&self) -> Option<String> {
        self.core.extract_selected_text()
    }

    /// Delegate: advance click count and return streak (1/2/3).
    pub fn advance_click_count(
        &mut self,
        pane_id: u64,
        col: u16,
        buffer_row: usize,
        now: Instant,
    ) -> u8 {
        self.core.advance_click_count(pane_id, col, buffer_row, now)
    }

    /// Delegate: select word at position.
    pub fn select_word_at(&mut self, pane_id: u64, col: u16, buffer_row: usize) -> bool {
        self.core.select_word_at(pane_id, col, buffer_row)
    }

    /// Delegate: select entire line at position.
    pub fn select_line_at(&mut self, pane_id: u64, buffer_row: usize) {
        self.core.select_line_at(pane_id, buffer_row)
    }

    /// Delegate: clear hovered link.
    pub fn clear_hovered_link(&mut self) -> bool {
        self.core.clear_hovered_link()
    }

    /// Delegate: hovered link URL at position.
    pub fn hovered_link_url_at(&self, pane_id: u64, col: u16, buffer_row: usize) -> Option<String> {
        self.core.hovered_link_url_at(pane_id, col, buffer_row)
    }

    /// Convert pixel coordinates to (pane_id, col, buffer_row) using absolute buffer indices.
    pub fn pixel_to_cell(&self, mx: f32, my: f32) -> Option<(u64, u16, usize)> {
        let (cw, ch) = self.cell_dimensions();
        if cw <= 0.0 || ch <= 0.0 {
            return None;
        }
        let my = self.content_y_from_screen(my)?;
        let border_w = self.core.config.appearance.border_width;
        let padding = self.core.config.appearance.padding;
        let vox = self.core.anim_mgr.view_offset_x.value() as f32;
        let tiles = self.core.workspaces.active().visible_tiles(vox);
        for (pane_id, rect, _) in &tiles {
            if rect.contains(mx, my) {
                let inner_x = rect.x + border_w + padding;
                let inner_y = rect.y + border_w + padding;
                let col = ((mx - inner_x) / cw).floor().max(0.0) as u16;
                let viewport_row = ((my - inner_y) / ch).floor().max(0.0) as u16;
                if let Some(grid) = self.core.pane_grids.get(pane_id) {
                    let col = col.min(grid.cols.saturating_sub(1));
                    let viewport_row = viewport_row.min(grid.rows.saturating_sub(1));
                    let buffer_row = grid.viewport_to_buffer_row(viewport_row);
                    return Some((*pane_id, col, buffer_row));
                }
            }
        }
        None
    }

    /// Convert pixel coordinates to (pane_id, col, viewport_row) for mouse forwarding.
    pub fn pixel_to_viewport_cell(&self, mx: f32, my: f32) -> Option<(u64, u16, u16)> {
        let (cw, ch) = self.cell_dimensions();
        if cw <= 0.0 || ch <= 0.0 {
            return None;
        }
        let my = self.content_y_from_screen(my)?;
        let border_w = self.core.config.appearance.border_width;
        let padding = self.core.config.appearance.padding;
        let vox = self.core.anim_mgr.view_offset_x.value() as f32;
        let tiles = self.core.workspaces.active().visible_tiles(vox);
        for (pane_id, rect, _) in &tiles {
            if rect.contains(mx, my) {
                let inner_x = rect.x + border_w + padding;
                let inner_y = rect.y + border_w + padding;
                let col = ((mx - inner_x) / cw).floor().max(0.0) as u16;
                let row = ((my - inner_y) / ch).floor().max(0.0) as u16;
                if let Some(grid) = self.core.pane_grids.get(pane_id) {
                    let col = col.min(grid.cols.saturating_sub(1));
                    let row = row.min(grid.rows.saturating_sub(1));
                    return Some((*pane_id, col, row));
                }
            }
        }
        None
    }

    pub fn update_hovered_link(&mut self, mx: f32, my: f32) -> bool {
        let next = self
            .pixel_to_cell(mx, my)
            .and_then(|(pane_id, col, buffer_row)| {
                let link = self
                    .core
                    .pane_grids
                    .get(&pane_id)?
                    .link_at(col, buffer_row)?;
                Some(super::HoveredLink {
                    pane_id,
                    url: link.url,
                    start: (link.start_col, buffer_row),
                    end: (link.end_col, buffer_row),
                })
            });
        if self.core.hovered_link == next {
            return false;
        }
        self.core.hovered_link = next;
        true
    }

    pub fn link_activation_modifier_active(&self) -> bool {
        link_activation_modifier_active(self.modifiers)
    }

    pub fn open_url(&self, url: &str) {
        // Look up the CWD of the relevant pane for relative path resolution.
        // Try hovered link pane first, then context menu target pane.
        let pane_id = self
            .core
            .hovered_link
            .as_ref()
            .map(|link| link.pane_id)
            .or(self.core.context_menu.target_pane_id)
            .or_else(|| self.core.workspaces.active().active_pane_id());
        let pane_cwd = pane_id
            .and_then(|id| self.core.pane_grids.get(&id))
            .and_then(|grid| grid.cwd.as_deref());
        if let Err(e) = open_url(url, pane_cwd) {
            log::warn!("failed to open url '{url}': {e}");
        }
    }

    // ── Unified action helpers ──

    fn open_search(&mut self) {
        self.core.open_search();
    }

    fn close_search_restore_scroll(&mut self) {
        let Some(search) = &self.core.search_state else {
            return;
        };
        let pane_id = search.pane_id;
        let orig = search.original_scroll_offset;
        if let Some(grid) = self.core.pane_grids.get_mut(&pane_id) {
            grid.scroll_offset = orig;
            grid.dirty = true;
            self.invalidate_pane_cache(pane_id);
        }
        self.core.search_state = None;
    }

    fn execute_palette_selection(&mut self) {
        let Some(palette) = &self.core.command_palette else {
            return;
        };
        let keep_open = palette
            .filtered
            .get(palette.selected_idx)
            .and_then(|&idx| palette.entries.get(idx))
            .is_some_and(|e| matches!(e.kind, super::PaletteEntryKind::RemoteHost { .. }));
        if let Some(&entry_idx) = palette.filtered.get(palette.selected_idx) {
            self.execute_palette_entry(entry_idx);
        }
        if !keep_open {
            self.core.command_palette = None;
        }
    }

    fn handle_clipboard_paste_force(&mut self) {
        // Re-paste without guard check.
        match &mut self.clipboard {
            None => log::warn!("clipboard not available"),
            Some(cb) => match cb.get_text() {
                Err(e) => log::warn!("clipboard read failed: {e}"),
                Ok(text) => {
                    self.send_paste_to_active_pane(text.as_bytes());
                }
            },
        }
    }

    pub(crate) fn update_search_results(&mut self) {
        let Some(search) = &mut self.core.search_state else {
            return;
        };
        let pane_id = search.pane_id;
        let query = search.query.clone();

        if let Some(grid) = self.core.pane_grids.get(&pane_id) {
            let raw_matches = grid.search(&query);
            search.matches = raw_matches
                .into_iter()
                .map(|(row, sc, ec)| super::SearchMatch {
                    buffer_row: row,
                    start_col: sc,
                    end_col: ec,
                })
                .collect();
            search.current_match_idx = 0;

            // If there are matches, scroll to the first one near current viewport
            if !search.matches.is_empty() {
                let viewport_top = grid.viewport_top();
                let idx = search
                    .matches
                    .iter()
                    .position(|m| m.buffer_row >= viewport_top)
                    .unwrap_or(0);
                search.current_match_idx = idx;
                self.scroll_to_match(idx);
            }
        }
    }

    fn jump_to_match(&mut self, reverse: bool) {
        let Some(search) = &mut self.core.search_state else {
            return;
        };
        if search.matches.is_empty() {
            return;
        }

        if reverse {
            search.current_match_idx = if search.current_match_idx == 0 {
                search.matches.len() - 1
            } else {
                search.current_match_idx - 1
            };
        } else {
            search.current_match_idx = (search.current_match_idx + 1) % search.matches.len();
        }
        let idx = search.current_match_idx;
        self.scroll_to_match(idx);
    }

    fn scroll_to_match(&mut self, match_idx: usize) {
        let Some(search) = &self.core.search_state else {
            return;
        };
        let Some(m) = search.matches.get(match_idx) else {
            return;
        };
        let pane_id = search.pane_id;
        let target_row = m.buffer_row;

        if let Some(grid) = self.core.pane_grids.get_mut(&pane_id) {
            let total = grid.buffer_len();
            let rows = grid.rows as usize;
            // Calculate scroll_offset to put target_row in the middle of viewport
            let desired_top = target_row.saturating_sub(rows / 2);
            let max_scroll = total.saturating_sub(rows);
            let new_offset = max_scroll.saturating_sub(desired_top);
            grid.scroll_offset = new_offset.min(max_scroll);
            grid.dirty = true;
            self.invalidate_pane_cache(pane_id);
        }
    }

    pub fn scroll_active_up(&mut self, lines: usize) {
        if let Some(pid) = self.core.workspaces.active().active_pane_id()
            && let Some(grid) = self.core.pane_grids.get_mut(&pid)
        {
            grid.scroll_up(lines);
            self.invalidate_pane_cache(pid);
        }
    }

    pub fn scroll_active_down(&mut self, lines: usize) {
        if let Some(pid) = self.core.workspaces.active().active_pane_id()
            && let Some(grid) = self.core.pane_grids.get_mut(&pid)
        {
            grid.scroll_down(lines);
            self.invalidate_pane_cache(pid);
        }
    }

    pub fn scroll_active_to_bottom(&mut self) {
        if let Some(pid) = self.core.workspaces.active().active_pane_id()
            && let Some(grid) = self.core.pane_grids.get_mut(&pid)
        {
            grid.scroll_to_bottom();
            self.invalidate_pane_cache(pid);
        }
    }
}

pub(crate) fn key_event_to_pty_bytes(event: &winit::event::KeyEvent, ctrl: bool) -> Vec<u8> {
    if ctrl && let Key::Character(c) = &event.logical_key {
        let ch = c.as_str();
        if ch.len() == 1 {
            let byte = ch.as_bytes()[0];
            if byte.is_ascii_lowercase() {
                return vec![byte - b'a' + 1];
            }
            if byte.is_ascii_uppercase() {
                return vec![byte - b'A' + 1];
            }
            // Some Wayland compositors report the control character directly
            // (e.g. Ctrl+A → '\x01') rather than the base letter.
            if byte < 0x20 {
                return vec![byte];
            }
            return match byte {
                b'[' => vec![0x1b],
                b'\\' => vec![0x1c],
                b']' => vec![0x1d],
                b'^' => vec![0x1e],
                b'_' => vec![0x1f],
                b'@' => vec![0x00],
                _ => vec![],
            };
        }
    }

    if let Key::Named(key) = &event.logical_key {
        match key {
            NamedKey::Enter => return vec![b'\r'],
            NamedKey::Backspace => return vec![0x7f],
            NamedKey::Tab => return vec![b'\t'],
            NamedKey::Escape => return vec![0x1b],
            NamedKey::Space => return vec![b' '],
            NamedKey::ArrowUp => return b"\x1b[A".to_vec(),
            NamedKey::ArrowDown => return b"\x1b[B".to_vec(),
            NamedKey::ArrowRight => return b"\x1b[C".to_vec(),
            NamedKey::ArrowLeft => return b"\x1b[D".to_vec(),
            NamedKey::Home => return b"\x1b[H".to_vec(),
            NamedKey::End => return b"\x1b[F".to_vec(),
            NamedKey::PageUp => return b"\x1b[5~".to_vec(),
            NamedKey::PageDown => return b"\x1b[6~".to_vec(),
            NamedKey::Delete => return b"\x1b[3~".to_vec(),
            NamedKey::Insert => return b"\x1b[2~".to_vec(),
            NamedKey::F1 => return b"\x1bOP".to_vec(),
            NamedKey::F2 => return b"\x1bOQ".to_vec(),
            NamedKey::F3 => return b"\x1bOR".to_vec(),
            NamedKey::F4 => return b"\x1bOS".to_vec(),
            NamedKey::F5 => return b"\x1b[15~".to_vec(),
            NamedKey::F6 => return b"\x1b[17~".to_vec(),
            NamedKey::F7 => return b"\x1b[18~".to_vec(),
            NamedKey::F8 => return b"\x1b[19~".to_vec(),
            NamedKey::F9 => return b"\x1b[20~".to_vec(),
            NamedKey::F10 => return b"\x1b[21~".to_vec(),
            NamedKey::F11 => return b"\x1b[23~".to_vec(),
            NamedKey::F12 => return b"\x1b[24~".to_vec(),
            _ => {}
        }
    }

    // event.text is winit's authoritative text for key presses — derived from
    // xkb_state_key_get_utf8() on Wayland, so it correctly includes Shift and
    // other modifier transformations (e.g. Shift+a → "A", Shift+1 → "!").
    if let Some(text) = &event.text {
        let s: &str = text;
        if !s.is_empty() {
            return s.as_bytes().to_vec();
        }
    }

    // Fallback: logical_key Character — also reliable on most platforms.
    if let Key::Character(c) = &event.logical_key {
        let s = c.as_str();
        if !s.is_empty() {
            return s.as_bytes().to_vec();
        }
    }

    // Last resort: platform extension that can return None on some Wayland setups.
    if let Some(text) = event.text_with_all_modifiers() {
        let s: &str = text;
        if !s.is_empty() {
            return s.as_bytes().to_vec();
        }
    }

    vec![]
}

/// Encode a key event using the Kitty keyboard protocol (CSI u format).
///
/// Format: CSI unicode-key-code [; modifier-value] u
/// Modifier bits: shift=1, alt=2, ctrl=4, super=8 (value = bits + 1)
///
/// For special keys (arrows, function keys, etc.) that have legacy encodings,
/// we use: CSI 1 ; modifier-value <suffix>
pub(crate) fn key_event_to_kitty_bytes(
    event: &winit::event::KeyEvent,
    ctrl: bool,
    shift: bool,
    alt: bool,
    super_key: bool,
) -> Vec<u8> {
    // Compute modifier value (kitty uses modifier_bits + 1)
    let mut modifier_bits: u8 = 0;
    if shift {
        modifier_bits |= 1;
    }
    if alt {
        modifier_bits |= 2;
    }
    if ctrl {
        modifier_bits |= 4;
    }
    if super_key {
        modifier_bits |= 8;
    }
    let modifier_val = modifier_bits + 1; // 1 = no modifiers

    // Helper: format CSI <keycode> [; modifier] u
    let csi_u = |keycode: u32| -> Vec<u8> {
        if modifier_val > 1 {
            format!("\x1b[{};{}u", keycode, modifier_val).into_bytes()
        } else {
            format!("\x1b[{}u", keycode).into_bytes()
        }
    };

    // Helper: format CSI 1 ; modifier <suffix> for special keys
    let csi_special = |suffix: char| -> Vec<u8> {
        if modifier_val > 1 {
            format!("\x1b[1;{}{}", modifier_val, suffix).into_bytes()
        } else {
            // Fall back to legacy encoding when no modifiers
            format!("\x1b[{}", suffix).into_bytes()
        }
    };

    // Helper: format CSI <keycode> ; modifier ~ for tilde keys
    let csi_tilde = |keycode: u32| -> Vec<u8> {
        if modifier_val > 1 {
            format!("\x1b[{};{}~", keycode, modifier_val).into_bytes()
        } else {
            format!("\x1b[{}~", keycode).into_bytes()
        }
    };

    // Named keys first
    if let Key::Named(key) = &event.logical_key {
        return match key {
            NamedKey::Enter => csi_u(13),
            NamedKey::Tab => csi_u(9),
            NamedKey::Backspace => csi_u(127),
            NamedKey::Escape => csi_u(27),
            NamedKey::Space => csi_u(32),
            NamedKey::ArrowUp => csi_special('A'),
            NamedKey::ArrowDown => csi_special('B'),
            NamedKey::ArrowRight => csi_special('C'),
            NamedKey::ArrowLeft => csi_special('D'),
            NamedKey::Home => csi_special('H'),
            NamedKey::End => csi_special('F'),
            NamedKey::PageUp => csi_tilde(5),
            NamedKey::PageDown => csi_tilde(6),
            NamedKey::Insert => csi_tilde(2),
            NamedKey::Delete => csi_tilde(3),
            NamedKey::F1 => csi_tilde(11),
            NamedKey::F2 => csi_tilde(12),
            NamedKey::F3 => csi_tilde(13),
            NamedKey::F4 => csi_tilde(14),
            NamedKey::F5 => csi_tilde(15),
            NamedKey::F6 => csi_tilde(17),
            NamedKey::F7 => csi_tilde(18),
            NamedKey::F8 => csi_tilde(19),
            NamedKey::F9 => csi_tilde(20),
            NamedKey::F10 => csi_tilde(21),
            NamedKey::F11 => csi_tilde(23),
            NamedKey::F12 => csi_tilde(24),
            // Modifier-only keys: don't send in kitty protocol unless explicitly requested
            NamedKey::Control | NamedKey::Shift | NamedKey::Alt | NamedKey::Super => vec![],
            _ => vec![],
        };
    }

    // Character keys: encode as CSI <unicode_codepoint> [; modifier] u
    // Kitty protocol requires the *base* (unshifted) key codepoint.
    // The modifier value separately carries shift/ctrl/alt/super info.
    // e.g. Shift+a → CSI 97;2u (base='a'=97, modifier=2=shift)
    //      Ctrl+a  → CSI 97;5u (base='a'=97, modifier=5=ctrl)
    if let Key::Character(c) = &event.logical_key {
        let text = c.as_str();
        if let Some(ch) = text.chars().next() {
            // Kitty spec: plain text keys (no modifiers, or only shift)
            // should be sent as raw text, not CSI u encoded.
            let has_non_shift_mods = ctrl || alt || super_key;
            if !has_non_shift_mods && ch as u32 >= 0x20 {
                let mut buf = [0u8; 4];
                let s = ch.encode_utf8(&mut buf);
                return s.as_bytes().to_vec();
            }

            let base = physical_key_to_base_char(event.physical_key)
                .map(|c| c as u32)
                .unwrap_or(ch as u32);
            return csi_u(base);
        }
    }

    vec![]
}

#[cfg(target_os = "macos")]
fn link_activation_modifier_active(modifiers: winit::keyboard::ModifiersState) -> bool {
    modifiers.super_key()
}

#[cfg(not(target_os = "macos"))]
fn link_activation_modifier_active(modifiers: winit::keyboard::ModifiersState) -> bool {
    modifiers.control_key()
}

fn open_url(url: &str, pane_cwd: Option<&str>) -> std::io::Result<()> {
    // Check if this is a file path (not a URL)
    if is_file_path_link(url) {
        return open_file_path(url, pane_cwd);
    }

    // Only allow http/https URLs for OS handler — other schemes (file://, data:,
    // javascript:, etc.) could be exploited by malicious terminal output.
    let lower = url.to_ascii_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("refusing to open URL with untrusted scheme: {url}"),
        ));
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(url).spawn()?;
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        // Use ShellExecuteW directly instead of `cmd /C start`.
        // cmd.exe interprets metacharacters (& | > < ^ ( ) % ! ") in the URL,
        // and a blocklist can never be exhaustive.  ShellExecuteW is the Windows
        // API designed for this purpose and avoids cmd.exe entirely.
        // This is the same approach used by WezTerm and the `open` crate.
        use std::os::windows::ffi::OsStrExt;
        unsafe extern "system" {
            fn ShellExecuteW(
                hwnd: *mut std::ffi::c_void,
                operation: *const u16,
                file: *const u16,
                parameters: *const u16,
                directory: *const u16,
                show_cmd: i32,
            ) -> isize;
        }
        let wide_open: Vec<u16> = std::ffi::OsStr::new("open")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let wide_url: Vec<u16> = std::ffi::OsStr::new(url)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                wide_open.as_ptr(),
                wide_url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1, // SW_SHOWNORMAL
            )
        };
        // ShellExecuteW returns > 32 on success.
        if result <= 32 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("ShellExecuteW failed with code {result}"),
            ));
        }
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open").arg(url).spawn()?;
    }

    #[allow(unreachable_code)]
    Ok(())
}

/// Check if a link string looks like a file path rather than a URL.
fn is_file_path_link(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("www.") {
        return false;
    }
    // Strip :line:col suffix for path analysis
    let (path, _, _) = parse_file_location(s);
    // Unix absolute, relative, home
    if path.starts_with('/')
        || path.starts_with("./")
        || path.starts_with("../")
        || path.starts_with("~/")
    {
        return true;
    }
    // Windows absolute: C:\ or C:/
    if path.len() >= 3
        && path.as_bytes()[0].is_ascii_alphabetic()
        && path.as_bytes()[1] == b':'
        && (path.as_bytes()[2] == b'\\' || path.as_bytes()[2] == b'/')
    {
        return true;
    }
    // Bare path with separator — must also have a file extension to avoid
    // false positives (consistent with detect_file_path in grid.rs).
    if (path.contains('/') || path.contains('\\')) && !path.contains("://") {
        // Require a dot in the last path component (file extension)
        let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        return file_name.contains('.');
    }
    false
}

/// Open a file path, optionally at a specific line:col.
///
/// Security: resolves the path to an absolute canonical form and verifies
/// that the file exists before opening. This prevents:
/// - Opening non-existent paths crafted by malicious terminal output
/// - Path traversal via unresolved `..` components
/// - Uncontrolled tilde/env-var expansion
///
/// Uses $EDITOR with line number support when available, otherwise falls
/// back to the OS default handler. All arguments are passed as argv
/// elements (not through a shell) to prevent command injection.
fn open_file_path(path_with_loc: &str, pane_cwd: Option<&str>) -> std::io::Result<()> {
    let (path, line, _col) = parse_file_location(path_with_loc);

    // Resolve ~ to home directory
    let expanded = if path.starts_with("~/") {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            format!("{}{}", home.to_string_lossy(), &path[1..])
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "cannot expand ~: HOME not set",
            ));
        }
    } else {
        path.to_string()
    };

    // For relative paths, resolve against the pane's CWD (from OSC 7).
    // Without a known CWD, relative paths cannot be safely resolved.
    let to_resolve = if !std::path::Path::new(&expanded).is_absolute() {
        match pane_cwd {
            Some(cwd) => {
                let joined = std::path::Path::new(cwd).join(&expanded);
                joined.to_string_lossy().into_owned()
            }
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("cannot resolve relative path without pane CWD: {expanded}"),
                ));
            }
        }
    } else {
        expanded
    };

    // Canonicalize: resolve `..`, symlinks, and convert to absolute path.
    // This also serves as an existence check — canonicalize fails if the
    // file doesn't exist.
    let canonical = std::fs::canonicalize(&to_resolve).map_err(|e| {
        log::debug!("file path not found or inaccessible: {to_resolve}: {e}");
        e
    })?;
    let resolved = canonical.to_string_lossy();

    // Try $EDITOR first (supports line numbers)
    if let Ok(editor) = std::env::var("EDITOR") {
        let editor_lower = editor.to_ascii_lowercase();
        if editor_lower.contains("code") || editor_lower.contains("cursor") {
            let mut loc = resolved.to_string();
            if let Some(l) = line {
                loc = format!("{loc}:{l}");
            }
            return Command::new(&editor)
                .args(["--goto", &loc])
                .spawn()
                .map(|_| ());
        }
        if editor_lower.contains("vim")
            || editor_lower.contains("nvim")
            || editor_lower.contains("hx")
        {
            let mut args = Vec::new();
            if let Some(l) = line {
                args.push(format!("+{l}"));
            }
            args.push(resolved.to_string());
            return Command::new(&editor).args(&args).spawn().map(|_| ());
        }
        return Command::new(&editor)
            .arg(resolved.as_ref())
            .spawn()
            .map(|_| ());
    }

    // No $EDITOR set — refuse to open file paths via OS handler.
    // OS handlers (cmd /C start, open, xdg-open) can execute arbitrary
    // binaries (.exe, .bat, .app), which is unsafe for untrusted paths
    // from terminal output. URLs are fine (browser is sandboxed), but
    // file paths need an explicit editor.
    Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "set $EDITOR to open file paths (OS handler refused for safety)",
    ))
}

/// Parse `path:line:col` into `(path, Option<line>, Option<col>)`.
fn parse_file_location(s: &str) -> (&str, Option<u32>, Option<u32>) {
    // Try path:line:col
    if let Some((rest, col_s)) = s.rsplit_once(':')
        && let Ok(col) = col_s.parse::<u32>()
    {
        if let Some((path, line_s)) = rest.rsplit_once(':')
            && let Ok(line) = line_s.parse::<u32>()
        {
            // Don't split on Windows drive letter (C:)
            if !(path.is_empty() || path.len() == 1 && path.as_bytes()[0].is_ascii_alphabetic()) {
                return (path, Some(line), Some(col));
            }
        }
        // path:line only
        if !(rest.is_empty() || rest.len() == 1 && rest.as_bytes()[0].is_ascii_alphabetic()) {
            return (rest, Some(col), None);
        }
    }
    (s, None, None)
}

/// Map a physical key code to its base (unshifted, unmodified) character.
/// Returns `None` for keys that don't have a simple character mapping.
pub(crate) fn physical_key_to_base_char(key: winit::keyboard::PhysicalKey) -> Option<char> {
    use winit::keyboard::{KeyCode, PhysicalKey};
    match key {
        PhysicalKey::Code(code) => match code {
            KeyCode::KeyA => Some('a'),
            KeyCode::KeyB => Some('b'),
            KeyCode::KeyC => Some('c'),
            KeyCode::KeyD => Some('d'),
            KeyCode::KeyE => Some('e'),
            KeyCode::KeyF => Some('f'),
            KeyCode::KeyG => Some('g'),
            KeyCode::KeyH => Some('h'),
            KeyCode::KeyI => Some('i'),
            KeyCode::KeyJ => Some('j'),
            KeyCode::KeyK => Some('k'),
            KeyCode::KeyL => Some('l'),
            KeyCode::KeyM => Some('m'),
            KeyCode::KeyN => Some('n'),
            KeyCode::KeyO => Some('o'),
            KeyCode::KeyP => Some('p'),
            KeyCode::KeyQ => Some('q'),
            KeyCode::KeyR => Some('r'),
            KeyCode::KeyS => Some('s'),
            KeyCode::KeyT => Some('t'),
            KeyCode::KeyU => Some('u'),
            KeyCode::KeyV => Some('v'),
            KeyCode::KeyW => Some('w'),
            KeyCode::KeyX => Some('x'),
            KeyCode::KeyY => Some('y'),
            KeyCode::KeyZ => Some('z'),
            KeyCode::Digit0 => Some('0'),
            KeyCode::Digit1 => Some('1'),
            KeyCode::Digit2 => Some('2'),
            KeyCode::Digit3 => Some('3'),
            KeyCode::Digit4 => Some('4'),
            KeyCode::Digit5 => Some('5'),
            KeyCode::Digit6 => Some('6'),
            KeyCode::Digit7 => Some('7'),
            KeyCode::Digit8 => Some('8'),
            KeyCode::Digit9 => Some('9'),
            KeyCode::Space => Some(' '),
            _ => None,
        },
        _ => None,
    }
}
