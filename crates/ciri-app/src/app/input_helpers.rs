use std::time::{Duration, Instant};

use unicode_width::UnicodeWidthChar;

use super::{AppModel, LastLeftClick, SearchState, Selection};

/// Compute the display width of a string, accounting for wide characters (CJK etc.).
fn unicode_display_width(s: &str) -> usize {
    s.chars()
        .map(|c| UnicodeWidthChar::width(c).unwrap_or(0))
        .sum()
}

/// Truncate a string to fit within `max_width` display columns.
fn truncate_to_display_width(s: &str, max_width: usize) -> String {
    let mut width = 0;
    let mut out = String::new();
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if width + cw > max_width {
            break;
        }
        width += cw;
        out.push(c);
    }
    out
}

impl AppModel {
    /// Extract selected text from the pane grid using absolute buffer coordinates.
    pub fn extract_selected_text(&self) -> Option<String> {
        let sel = self.selection.as_ref()?;
        let grid = self.pane_grids.get(&sel.pane_id)?;
        Some(grid.text_in_range(sel.start, sel.end))
    }

    /// Compute the click streak (1 = single, 2 = double, 3 = triple) and
    /// record this click for future multi-click detection.
    ///
    /// The streak advances when the same button is pressed within
    /// `double_tap_window_ms` at roughly the same position (±1 cell).
    /// This follows the Alacritty pattern where click state advances on press.
    pub fn advance_click_count(
        &mut self,
        pane_id: u64,
        col: u16,
        buffer_row: usize,
        now: Instant,
    ) -> u8 {
        let threshold = Duration::from_millis(self.config.input.double_tap_window_ms);
        let count = if let Some(last) = &self.last_left_click
            && last.pane_id == pane_id
            && last.buffer_row == buffer_row
            && last.col.abs_diff(col) <= 1
            && now.duration_since(last.at) <= threshold
        {
            // Advance streak, wrap around after triple (3 -> 1).
            (last.count % 3) + 1
        } else {
            1
        };
        self.last_left_click = Some(LastLeftClick {
            pane_id,
            col,
            buffer_row,
            at: now,
            count,
        });
        count
    }

    /// Select the entire line at the given buffer row (triple-click behavior).
    pub fn select_line_at(&mut self, pane_id: u64, buffer_row: usize) {
        let end_col = self
            .pane_grids
            .get(&pane_id)
            .map(|g| g.cols.saturating_sub(1))
            .unwrap_or(79);
        self.selection = Some(Selection {
            pane_id,
            start: (0, buffer_row),
            end: (end_col, buffer_row),
            active: true,
        });
    }

    pub fn select_word_at(&mut self, pane_id: u64, col: u16, buffer_row: usize) -> bool {
        let Some(grid) = self.pane_grids.get(&pane_id) else {
            return false;
        };
        let Some((start_col, end_col)) = grid.word_bounds_at(col, buffer_row) else {
            return false;
        };
        self.selection = Some(Selection {
            pane_id,
            start: (start_col, buffer_row),
            end: (end_col, buffer_row),
            active: true,
        });
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

    pub fn open_search(&mut self) {
        let Some(pane_id) = self.workspaces.active().active_pane_id() else {
            return;
        };
        let scroll_offset = self
            .pane_grids
            .get(&pane_id)
            .map(|g| g.scroll_offset)
            .unwrap_or(0);
        self.search_state = Some(SearchState {
            query: String::new(),
            matches: Vec::new(),
            current_match_idx: 0,
            pane_id,
            original_scroll_offset: scroll_offset,
        });
    }

    /// Confirm pending paste — sends paste data to active pane.
    pub fn confirm_pending_paste(&mut self) {
        let Some(paste) = self.pending_paste.as_ref() else {
            return;
        };
        let text = paste.info.text.clone();
        self.pending_paste = None;
        if let Some(pid) = self.workspaces.active_mut().active_pane_id() {
            let bracketed = self
                .pane_grids
                .get(&pid)
                .is_some_and(|g| g.mode_flags & ciri_protocol::message::MODE_BRACKETED_PASTE != 0);
            let mut data = Vec::with_capacity(text.len() + if bracketed { 12 } else { 0 });
            if bracketed {
                data.extend_from_slice(b"\x1b[200~");
            }
            data.extend_from_slice(text.as_bytes());
            if bracketed {
                data.extend_from_slice(b"\x1b[201~");
            }
            self.send(ciri_protocol::message::ClientMessage::Input { pane_id: pid, data, input_seq: 0 });
        }
    }

    /// Content-area resize hit test (pure geometry — no shell fields).
    pub fn content_resize_hit_test(&self, mx: f32, my: f32) -> (bool, bool) {
        let ws = self.workspaces.active();
        let vox = self.anim_mgr.view_offset_x.value() as f32;
        let mut near_col_border = false;
        for i in 1..ws.columns.len() {
            let col_x = ws.column_x(i) - vox;
            if (mx - col_x).abs() < 4.0 {
                near_col_border = true;
                break;
            }
        }
        let near_tile_border = ws.hit_test_tile_border(vox, mx, my, 4.0).is_some();
        (near_col_border, near_tile_border)
    }

    /// Session display name for the top bar.
    /// Shows `session` for local, `session@host` for remote connections.
    pub fn session_display_name(&self) -> String {
        match &self.remote_config {
            Some(rc) => format!("{}@{}", self.session_name, rc.host),
            None => self.session_name.clone(),
        }
    }

    /// Workspace indicator label for the top bar.
    pub fn workspace_indicator_label(&self) -> String {
        let idx = self.workspaces.active_workspace_idx + 1;
        let total = self.workspaces.workspaces.len();
        if total <= 1 {
            String::new()
        } else {
            format!("[{}/{}] ", idx, total)
        }
    }

    /// Current mode label and color for the top bar.
    pub fn current_mode_label(&self) -> (String, [f32; 4]) {
        use ciri_config::theme::ThemeConfig;
        let accent = ThemeConfig::parse_color(&self.config.theme.accent);
        let broadcast_color = ThemeConfig::parse_color(&self.config.theme.mode_broadcast);
        let dim = ThemeConfig::parse_color(&self.config.theme.statusbar_dim);
        let warn_color = ThemeConfig::parse_color(&self.config.theme.mode_broadcast);
        if self.input.is_locked() {
            (" LOCKED ".into(), warn_color)
        } else if self.broadcast_mode {
            (" BROADCAST ".into(), broadcast_color)
        } else if self.overview.active {
            (" OVERVIEW ".into(), accent)
        } else if let Some(name) = self.input.current_mode_name() {
            (format!(" {} ", name.to_uppercase()), accent)
        } else if self.input.is_awaiting_action() {
            (" LEADER ".into(), accent)
        } else {
            (" NORMAL ".into(), dim)
        }
    }

    /// Get ordered pane tab entries for the active workspace.
    pub fn pane_tab_entries(&self) -> Vec<(u64, String)> {
        let mut panes = Vec::new();
        let ws = self.workspaces.active();
        for col in &ws.columns {
            for tile in &col.tiles {
                let title = self
                    .pane_grids
                    .get(&tile.pane_id)
                    .map(|g| g.title.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "pane".to_string());
                panes.push((tile.pane_id, title));
            }
        }
        panes
    }

    /// Format a pane tab label with index prefix, respecting display width.
    pub fn format_pane_tab_label(&self, idx: usize, title: &str) -> String {
        const PANE_TAB_WIDTH_CHARS: usize = 20;
        let prefix = format!("{:>2} ", idx + 1);
        let prefix_width = unicode_display_width(&prefix);
        let max_title_width = PANE_TAB_WIDTH_CHARS.saturating_sub(prefix_width);
        let title = truncate_to_display_width(title, max_title_width);
        let label = format!("{}{}", prefix, title);
        let label_width = unicode_display_width(&label);
        let pad = PANE_TAB_WIDTH_CHARS.saturating_sub(label_width);
        format!("{}{}", label, " ".repeat(pad))
    }

    /// Snapshot pane *layout* positions (using resolve_width, not rendered_width).
    /// This gives stable positions unaffected by in-flight column width animations,
    /// so move animations are only triggered by real structural changes.
    pub fn snapshot_pane_positions(&self) -> std::collections::HashMap<u64, (f32, f32)> {
        let mut positions = std::collections::HashMap::new();
        let vw = self.workspaces.view_size.width;
        let vh = self.workspaces.view_size.height;
        for (ws_idx, ws) in self.workspaces.workspaces.iter().enumerate() {
            let wy = self.workspaces.workspace_y(ws_idx);
            let mut col_x = 0.0_f32;
            for col in &ws.columns {
                let col_w = col.resolve_width(vw);
                let tile_count = col.tiles.len();
                let total_weight: f64 = col.tiles.iter().map(|t| t.height.weight() as f64).sum();
                let mut tile_y = 0.0_f32;
                for tile in &col.tiles {
                    let tile_h = if total_weight > 0.0 && tile_count > 1 {
                        (tile.height.weight() as f64 / total_weight * vh as f64) as f32
                    } else {
                        vh / tile_count.max(1) as f32
                    };
                    positions.insert(tile.pane_id, (col_x, wy + tile_y));
                    tile_y += tile_h;
                }
                col_x += col_w + ws.column_gap;
            }
        }
        positions
    }
}
