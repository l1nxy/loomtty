use ciri_input::action::Action;
use ciri_layout::geometry::Rect as GeoRect;
use ciri_protocol::message::*;
use std::process::Command;
use std::time::{Duration, Instant};
use winit::keyboard::{Key, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

use super::App;

impl App {
    pub fn handle_action(&mut self, action: Action) {
        match action {
            Action::NewColumnRight => {
                self.send(ClientMessage::CreatePane);
            }
            Action::NewWorkspaceBelow => {
                self.send(ClientMessage::SplitDown);
            }
            Action::ClosePane => {
                if let Some(pane_id) = self.workspaces.active_mut().active_pane_id() {
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
                    .workspaces
                    .active_mut()
                    .cycle_preset_width(&presets, reverse)
                {
                    let (proportion, fixed_px) = match w {
                        ciri_layout::column::ColumnWidth::Proportion(p) => (p, None),
                        ciri_layout::column::ColumnWidth::Fixed(px) => {
                            let vw = self.workspaces.view_size.width as f64;
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
                self.broadcast_mode = !self.broadcast_mode;
                log::info!("broadcast mode: {}", self.broadcast_mode);
            }
            Action::ConsumeIntoColumn => {
                self.send(ClientMessage::ConsumeIntoColumn);
            }
            Action::ExpelFromColumn => {
                self.send(ClientMessage::ExpelFromColumn);
            }
            Action::ExitOverview => {
                self.overview.active = false;
                self.overview
                    .zoom
                    .animate_to(1.0, self.config.animation.speed);
                self.animate_to_active();
            }
            Action::SwitchWorkspace(idx) => {
                self.send(ClientMessage::SwitchWorkspace { workspace_idx: idx });
            }
            Action::ToggleOverview => {
                self.overview.active = !self.overview.active;
                let omega = self.config.animation.speed;
                if self.overview.active {
                    self.refresh_overview_zoom();
                    self.view_offset_x.animate_to(0.0, omega);
                    self.view_offset_y.animate_to(0.0, omega);
                } else {
                    self.overview.zoom.animate_to(1.0, omega);
                    self.animate_to_active();
                }
            }
            Action::SendLeaderKey => {
                if let Some(pid) = self.workspaces.active_mut().active_pane_id() {
                    self.send(ClientMessage::Input {
                        pane_id: pid,
                        data: vec![0x17],
                    });
                }
            }
            Action::ScrollPageUp => {
                let rows = self
                    .pane_grids
                    .values()
                    .next()
                    .map(|g| g.rows as usize)
                    .unwrap_or(24);
                self.scroll_active_up(rows);
            }
            Action::ScrollPageDown => {
                let rows = self
                    .pane_grids
                    .values()
                    .next()
                    .map(|g| g.rows as usize)
                    .unwrap_or(24);
                self.scroll_active_down(rows);
            }
            Action::ScrollTop => {
                if let Some(pid) = self.workspaces.active().active_pane_id() {
                    if let Some(grid) = self.pane_grids.get_mut(&pid) {
                        grid.scroll_up(grid.max_scroll_offset());
                        self.invalidate_pane_cache(pid);
                    }
                }
            }
            Action::ScrollBottom => {
                self.scroll_active_to_bottom();
            }
            Action::Detach => {
                self.send(ClientMessage::Detach);
                self.should_exit = true;
            }
        }
    }

    pub fn hit_test_overview(&self, mx: f32, my: f32) -> Option<(usize, u64)> {
        let zoom = self.overview.zoom.value() as f32;
        let zoom_threshold = self.config.animation.zoom_threshold;
        let vox = self.view_offset_x.value() as f32;
        let voy = self.view_offset_y.value() as f32;
        let tiles = if self.overview.active || zoom < zoom_threshold {
            self.workspaces.all_tiles_2d(vox, voy)
        } else {
            self.workspaces.visible_tiles_2d(vox, voy)
        };
        let (vw, vh) = self
            .renderer
            .as_ref()
            .map(|r| {
                let (w, h) = r.surface_size();
                (w as f32, h as f32)
            })
            .unwrap_or((
                self.config.window.width as f32,
                self.config.window.height as f32,
            ));
        let cx = vw / 2.0;
        let cy = vh / 2.0;

        for (pane_id, tile_rect, _) in &tiles {
            let tr = if zoom < zoom_threshold {
                GeoRect::new(
                    cx + (tile_rect.x - cx) * zoom,
                    cy + (tile_rect.y - cy) * zoom,
                    tile_rect.w * zoom,
                    tile_rect.h * zoom,
                )
            } else {
                *tile_rect
            };
            if tr.contains(mx, my) {
                for (ws_idx, ws) in self.workspaces.workspaces.iter().enumerate() {
                    if ws.columns.iter().any(|c| c.contains_pane(*pane_id)) {
                        return Some((ws_idx, *pane_id));
                    }
                }
            }
        }
        None
    }

    /// Convert pixel coordinates to (pane_id, col, buffer_row) using absolute buffer indices.
    pub fn pixel_to_cell(&self, mx: f32, my: f32) -> Option<(u64, u16, usize)> {
        let (cw, ch) = self.cell_dimensions();
        if cw <= 0.0 || ch <= 0.0 {
            return None;
        }
        let border_w = self.config.appearance.border_width;
        let padding = self.config.appearance.padding;
        let vox = self.view_offset_x.value() as f32;
        let tiles = self.workspaces.active().visible_tiles(vox);
        for (pane_id, rect, _) in &tiles {
            if rect.contains(mx, my) {
                let inner_x = rect.x + border_w + padding;
                let inner_y = rect.y + border_w + padding;
                let col = ((mx - inner_x) / cw).floor().max(0.0) as u16;
                let viewport_row = ((my - inner_y) / ch).floor().max(0.0) as u16;
                if let Some(grid) = self.pane_grids.get(pane_id) {
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
        let border_w = self.config.appearance.border_width;
        let padding = self.config.appearance.padding;
        let vox = self.view_offset_x.value() as f32;
        let tiles = self.workspaces.active().visible_tiles(vox);
        for (pane_id, rect, _) in &tiles {
            if rect.contains(mx, my) {
                let inner_x = rect.x + border_w + padding;
                let inner_y = rect.y + border_w + padding;
                let col = ((mx - inner_x) / cw).floor().max(0.0) as u16;
                let row = ((my - inner_y) / ch).floor().max(0.0) as u16;
                if let Some(grid) = self.pane_grids.get(pane_id) {
                    let col = col.min(grid.cols.saturating_sub(1));
                    let row = row.min(grid.rows.saturating_sub(1));
                    return Some((*pane_id, col, row));
                }
            }
        }
        None
    }

    /// Extract selected text from the pane grid using absolute buffer coordinates.
    pub fn extract_selected_text(&self) -> Option<String> {
        let sel = self.selection.as_ref()?;
        let grid = self.pane_grids.get(&sel.pane_id)?;
        Some(grid.text_in_range(sel.start, sel.end))
    }

    pub fn is_double_left_click(
        &self,
        pane_id: u64,
        col: u16,
        buffer_row: usize,
        now: Instant,
    ) -> bool {
        let threshold = Duration::from_millis(self.config.input.double_tap_window_ms);
        self.last_left_click.as_ref().is_some_and(|last| {
            last.pane_id == pane_id
                && last.buffer_row == buffer_row
                && last.col.abs_diff(col) <= 1
                && now.duration_since(last.at) <= threshold
        })
    }

    pub fn remember_left_click(&mut self, pane_id: u64, col: u16, buffer_row: usize, now: Instant) {
        self.last_left_click = Some(super::LastLeftClick {
            pane_id,
            col,
            buffer_row,
            at: now,
        });
    }

    pub fn select_word_at(&mut self, pane_id: u64, col: u16, buffer_row: usize) -> bool {
        let Some(grid) = self.pane_grids.get(&pane_id) else {
            return false;
        };
        let Some((start_col, end_col)) = grid.word_bounds_at(col, buffer_row) else {
            return false;
        };
        self.selection = Some(super::Selection {
            pane_id,
            start: (start_col, buffer_row),
            end: (end_col, buffer_row),
            active: true,
        });
        true
    }

    pub fn update_hovered_link(&mut self, mx: f32, my: f32) -> bool {
        let next = self
            .pixel_to_cell(mx, my)
            .and_then(|(pane_id, col, buffer_row)| {
                let link = self.pane_grids.get(&pane_id)?.link_at(col, buffer_row)?;
                Some(super::HoveredLink {
                    pane_id,
                    url: link.url,
                    start: (link.start_col, buffer_row),
                    end: (link.end_col, buffer_row),
                })
            });
        if self.hovered_link == next {
            return false;
        }
        self.hovered_link = next;
        true
    }

    pub fn clear_hovered_link(&mut self) -> bool {
        self.hovered_link.take().is_some()
    }

    pub fn hovered_link_url_at(&self, pane_id: u64, col: u16, buffer_row: usize) -> Option<String> {
        self.hovered_link.as_ref().and_then(|link| {
            (link.pane_id == pane_id
                && link.start.1 == buffer_row
                && col >= link.start.0
                && col <= link.end.0)
                .then(|| link.url.clone())
        })
    }

    pub fn link_activation_modifier_active(&self) -> bool {
        link_activation_modifier_active(self.modifiers)
    }

    pub fn open_url(&self, url: &str) {
        if let Err(e) = open_url(url) {
            log::warn!("failed to open url '{url}': {e}");
        }
    }

    pub fn handle_search_key(&mut self, event: &winit::event::KeyEvent, ctrl: bool, shift: bool) {
        let Some(search) = &mut self.search_state else {
            return;
        };
        let pane_id = search.pane_id;

        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                // Restore original scroll position
                let orig = search.original_scroll_offset;
                if let Some(grid) = self.pane_grids.get_mut(&pane_id) {
                    grid.scroll_offset = orig;
                    grid.dirty = true;
                    self.invalidate_pane_cache(pane_id);
                }
                self.search_state = None;
            }
            Key::Named(NamedKey::Enter) if shift => {
                // Previous match
                self.jump_to_match(true);
            }
            Key::Named(NamedKey::Enter) => {
                if search.query.is_empty() {
                    // Exit search, keep current position
                    self.search_state = None;
                } else {
                    // Next match
                    self.jump_to_match(false);
                }
            }
            Key::Named(NamedKey::Backspace) => {
                search.query.pop();
                self.update_search_results();
            }
            Key::Character(c) if !ctrl => {
                let s: &str = c.as_str();
                search.query.push_str(s);
                self.update_search_results();
            }
            _ => {}
        }
    }

    fn update_search_results(&mut self) {
        let Some(search) = &mut self.search_state else {
            return;
        };
        let pane_id = search.pane_id;
        let query = search.query.clone();

        if let Some(grid) = self.pane_grids.get(&pane_id) {
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
        let Some(search) = &mut self.search_state else {
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
        let Some(search) = &self.search_state else {
            return;
        };
        let Some(m) = search.matches.get(match_idx) else {
            return;
        };
        let pane_id = search.pane_id;
        let target_row = m.buffer_row;

        if let Some(grid) = self.pane_grids.get_mut(&pane_id) {
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
        if let Some(pid) = self.workspaces.active().active_pane_id() {
            if let Some(grid) = self.pane_grids.get_mut(&pid) {
                grid.scroll_up(lines);
                self.invalidate_pane_cache(pid);
            }
        }
    }

    pub fn scroll_active_down(&mut self, lines: usize) {
        if let Some(pid) = self.workspaces.active().active_pane_id() {
            if let Some(grid) = self.pane_grids.get_mut(&pid) {
                grid.scroll_down(lines);
                self.invalidate_pane_cache(pid);
            }
        }
    }

    pub fn scroll_active_to_bottom(&mut self) {
        if let Some(pid) = self.workspaces.active().active_pane_id() {
            if let Some(grid) = self.pane_grids.get_mut(&pid) {
                grid.scroll_to_bottom();
                self.invalidate_pane_cache(pid);
            }
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
    if let Key::Character(c) = &event.logical_key {
        let text = c.as_str();
        // For kitty protocol, use the base key character's Unicode codepoint.
        // With modifiers, the raw character may be a control char, so use
        // key_without_modifiers to get the base letter.
        if let Some(ch) = text.chars().next() {
            let codepoint = if ctrl && (ch as u32) < 0x20 {
                // Recover the original letter from physical key
                use winit::keyboard::{KeyCode, PhysicalKey};
                match event.physical_key {
                    PhysicalKey::Code(code) => {
                        let base = match code {
                            KeyCode::KeyA => 'a',
                            KeyCode::KeyB => 'b',
                            KeyCode::KeyC => 'c',
                            KeyCode::KeyD => 'd',
                            KeyCode::KeyE => 'e',
                            KeyCode::KeyF => 'f',
                            KeyCode::KeyG => 'g',
                            KeyCode::KeyH => 'h',
                            KeyCode::KeyI => 'i',
                            KeyCode::KeyJ => 'j',
                            KeyCode::KeyK => 'k',
                            KeyCode::KeyL => 'l',
                            KeyCode::KeyM => 'm',
                            KeyCode::KeyN => 'n',
                            KeyCode::KeyO => 'o',
                            KeyCode::KeyP => 'p',
                            KeyCode::KeyQ => 'q',
                            KeyCode::KeyR => 'r',
                            KeyCode::KeyS => 's',
                            KeyCode::KeyT => 't',
                            KeyCode::KeyU => 'u',
                            KeyCode::KeyV => 'v',
                            KeyCode::KeyW => 'w',
                            KeyCode::KeyX => 'x',
                            KeyCode::KeyY => 'y',
                            KeyCode::KeyZ => 'z',
                            _ => ch,
                        };
                        base as u32
                    }
                    _ => ch as u32,
                }
            } else {
                ch as u32
            };
            return csi_u(codepoint);
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

fn open_url(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(url).spawn()?;
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("cmd").args(["/C", "start", "", url]).spawn()?;
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open").arg(url).spawn()?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Ok(())
}
