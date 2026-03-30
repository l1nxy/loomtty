use ciri_protocol::message::*;

use super::ClientPaneGrid;
use super::types::ScrollbackRow;

impl ClientPaneGrid {
    /// Apply a FullPaneSync from the server.
    ///
    /// The server sends:
    /// - `scrollback` + `scrollback_rows`: new history lines since last sync (oldest first)
    /// - `cells` + `rows`: the current live viewport
    ///
    /// We add scrollback rows to VecDeque, then memcpy cells directly into viewport flat buffer.
    pub fn apply_full_sync(&mut self, sync: &FullPaneSync) {
        let new_cols = sync.cols as usize;
        let new_rows = sync.rows as usize;

        // If dimensions changed, reflow scrollback and resize viewport.
        let cols_changed = sync.cols != self.cols;
        if cols_changed || sync.rows != self.rows {
            if cols_changed {
                self.reflow_scrollback(new_cols);
            }
            self.cols = sync.cols;
            self.rows = sync.rows;
            self.viewport = vec![PackedCell::default(); new_cols * new_rows];
            self.dirty_rows = vec![false; new_rows];
            self.dirty_row_count = 0;
            self.scroll_offset = 0;
        }

        // Step 1: Handle scrollback — replace or append
        if sync.scrollback_replace {
            self.scrollback.clear();
        }
        let sb_rows = sync.scrollback_rows as usize;
        for r in 0..sb_rows {
            let start = r * new_cols;
            let end = (start + new_cols).min(sync.scrollback.len());
            if end <= start {
                continue;
            }
            self.scrollback
                .push_back(ScrollbackRow::from_cells(&sync.scrollback[start..end]));
        }

        // Step 2: Memcpy cells directly into viewport flat buffer
        let vp_cells = new_cols * new_rows;
        if sync.cells.len() >= vp_cells {
            self.viewport[..vp_cells].copy_from_slice(&sync.cells[..vp_cells]);
        } else {
            // Partial: copy what we have, blank the rest
            let have = sync.cells.len();
            self.viewport[..have].copy_from_slice(&sync.cells);
            for cell in &mut self.viewport[have..vp_cells] {
                *cell = PackedCell::default();
            }
        }

        // Trim scrollback if over max
        while self.scrollback.len() > self.max_scrollback {
            self.scrollback.pop_front();
        }
        // Clamp scroll_offset
        let max_off = self.max_scroll_offset();
        if self.scroll_offset > max_off {
            self.scroll_offset = max_off;
        }

        self.cursor_line = sync.meta.cursor_line;
        self.cursor_col = sync.meta.cursor_col;
        self.cursor_shape = sync.meta.cursor_shape;
        self.mode_flags = sync.meta.mode_flags;
        self.has_shell_integration = sync.meta.mode_flags & MODE_SHELL_INTEGRATION != 0;
        self.has_kitty_keyboard = sync.meta.mode_flags & MODE_KITTY_KEYBOARD != 0;
        self.title = sync.title.clone();
        self.grapheme_map = sync.grapheme_extras.build_lookup(&sync.cells);
        self.hyperlink_map = sync.hyperlink_extras.link_map.clone();
        self.cwd = sync.cwd.clone();
        self.dirty = true;
    }

    /// Apply incremental CellDelta: patch the live viewport directly using flat buffer indexing.
    /// Kept for use with non-borrowed CellDelta (e.g. tests, offline replay).
    pub fn apply_delta(&mut self, delta: &CellDelta) {
        self.cursor_line = delta.cursor_line;
        self.cursor_col = delta.cursor_col;
        self.cursor_shape = delta.cursor_shape;
        self.mode_flags = delta.mode_flags;
        self.has_shell_integration = delta.mode_flags & MODE_SHELL_INTEGRATION != 0;
        self.has_kitty_keyboard = delta.mode_flags & MODE_KITTY_KEYBOARD != 0;

        let cols = self.cols as usize;

        for region in &delta.regions {
            let line = region.line as usize;
            if line >= self.rows as usize {
                continue;
            }
            let col_start = region.left as usize;
            let col_end = (col_start + region.cells.len()).min(cols);
            let copy_len = col_end.saturating_sub(col_start);
            if copy_len > 0 {
                let dst_start = line * cols + col_start;
                let dst_end = line * cols + col_end;
                self.viewport[dst_start..dst_end].copy_from_slice(&region.cells[..copy_len]);
                self.mark_row_dirty(line);
            }
        }
        self.dirty = true;
    }

    /// Apply incremental CellDeltaBorrowed (SM-decoded): decode opcode streams
    /// directly into the viewport flat buffer. Only marks individual dirty rows.
    pub fn apply_delta_borrowed(&mut self, delta: &CellDeltaBorrowed) {
        // Track if cursor moved (old and new cursor rows need redraw)
        let old_cursor_line = self.cursor_line;

        self.cursor_line = delta.meta.cursor_line;
        self.cursor_col = delta.meta.cursor_col;
        self.cursor_shape = delta.meta.cursor_shape;
        self.mode_flags = delta.meta.mode_flags;
        self.has_shell_integration = delta.meta.mode_flags & MODE_SHELL_INTEGRATION != 0;
        self.has_kitty_keyboard = delta.meta.mode_flags & MODE_KITTY_KEYBOARD != 0;

        let cols = self.cols as usize;
        let nrows = self.rows as usize;

        for (i, region) in delta.regions.iter().enumerate() {
            let line = region.line as usize;
            if line >= nrows {
                continue;
            }
            let col_start = region.left as usize;
            let cell_count = (region.right - region.left + 1) as usize;
            let col_end = (col_start + cell_count).min(cols);
            let copy_len = col_end.saturating_sub(col_start);
            if copy_len > 0 {
                let dst_start = line * cols + col_start;
                let dst_end = dst_start + copy_len;
                let sm_data = delta.sm_data(i);
                match ciri_protocol::codec::decode_sm_cells(
                    sm_data,
                    &mut self.viewport[dst_start..dst_end],
                ) {
                    Ok(_) => self.mark_row_dirty(line),
                    Err(e) => log::warn!("SM decode error for region {i}: {e}"),
                }
            }
        }
        // Mark old and new cursor rows dirty for cursor movement
        if old_cursor_line >= 0 {
            self.mark_row_dirty(old_cursor_line as usize);
        }
        if delta.meta.cursor_line >= 0 {
            self.mark_row_dirty(delta.meta.cursor_line as usize);
        }
    }
}
