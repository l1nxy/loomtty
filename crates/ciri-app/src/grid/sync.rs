use std::collections::HashMap;

use ciri_protocol::message::*;

use super::ClientPaneGrid;
use super::types::ScrollbackRow;

fn rebase_grapheme_lookup(
    old_map: &HashMap<u32, String>,
    old_viewport_len: usize,
    old_cols: usize,
    new_scrollback_rows: usize,
    trimmed_rows: usize,
) -> HashMap<u32, String> {
    if old_map.is_empty() || old_cols == 0 {
        return HashMap::new();
    }

    let mut rebased = HashMap::with_capacity(old_map.len());
    for (&idx, grapheme) in old_map {
        let idx = idx as usize;
        if idx < old_viewport_len {
            let row = idx / old_cols;
            let col = idx % old_cols;
            let new_buffer_row = new_scrollback_rows + row;
            if new_buffer_row >= trimmed_rows {
                let rebased_row = new_buffer_row - trimmed_rows;
                let new_idx = rebased_row * old_cols + col;
                rebased.insert(new_idx as u32, grapheme.clone());
            }
        } else {
            let row = idx / old_cols;
            let col = idx % old_cols;
            if row >= trimmed_rows {
                let rebased_row = row - trimmed_rows;
                let new_idx = rebased_row * old_cols + col;
                rebased.insert(new_idx as u32, grapheme.clone());
            }
        }
    }
    rebased
}

impl ClientPaneGrid {
    /// Apply a FullPaneSync from the server.
    ///
    /// The server sends:
    /// - `scrollback` + `scrollback_rows`: new history lines since last sync (oldest first)
    /// - `cells` + `rows`: the current live viewport
    ///
    /// We add scrollback rows to VecDeque, then memcpy cells directly into viewport flat buffer.
    pub fn apply_full_sync(&mut self, sync: &FullPaneSync) {
        let old_cols = self.cols as usize;
        let old_viewport_len = self.viewport.len();
        let old_grapheme_map = std::mem::take(&mut self.grapheme_map);
        let new_cols = sync.cols as usize;
        let new_rows = sync.rows as usize;

        // If dimensions changed, reflow scrollback and resize viewport.
        // rows==0 means scrollback-only sync — don't resize viewport.
        let cols_changed = sync.cols != self.cols;
        if sync.rows > 0 && (cols_changed || sync.rows != self.rows) {
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

        let appended_scrollback_rows = sync.scrollback_rows as usize;
        let trim_count = if sync.scrollback_replace {
            appended_scrollback_rows.saturating_sub(self.max_scrollback)
        } else {
            self.scrollback
                .len()
                .saturating_add(appended_scrollback_rows)
                .saturating_sub(self.max_scrollback)
        };

        // Step 1: Handle scrollback — replace or append
        if sync.scrollback_replace {
            self.scrollback.clear();
        }
        let mut rebased_grapheme_map = if cols_changed || sync.scrollback_replace {
            HashMap::new()
        } else {
            rebase_grapheme_lookup(
                &old_grapheme_map,
                old_viewport_len,
                old_cols,
                self.scrollback.len(),
                trim_count,
            )
        };
        for r in 0..appended_scrollback_rows {
            let start = r * new_cols;
            let end = (start + new_cols).min(sync.scrollback.len());
            if end <= start {
                continue;
            }
            self.scrollback
                .push_back(ScrollbackRow::from_cells(&sync.scrollback[start..end]));
        }

        if !cols_changed {
            let scrollback_base = self
                .scrollback
                .len()
                .saturating_sub(appended_scrollback_rows);
            for (idx, extra) in &sync.grapheme_extras.0 {
                let idx = *idx as usize;
                if idx < sync.scrollback.len() {
                    let ch = sync.scrollback[idx].ch();
                    let mut grapheme = String::new();
                    grapheme.push(ch);
                    grapheme.push_str(extra);
                    rebased_grapheme_map
                        .insert((scrollback_base * new_cols + idx) as u32, grapheme);
                }
            }
        }

        // Step 2: Memcpy cells directly into viewport flat buffer.
        // When rows==0, this is a scrollback-only sync — skip viewport update.
        let vp_cells = new_cols * new_rows;
        if vp_cells > 0 {
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
        self.kitty_flags = sync.meta.mode_flags & MODE_KITTY_ALL;
        self.password_input = sync.meta.mode_flags & MODE_PASSWORD_INPUT != 0;
        self.title = sync.title.clone();
        self.grapheme_map = rebased_grapheme_map;
        let sync_grapheme_map = sync.grapheme_extras.build_lookup(&sync.cells);
        for (idx, grapheme) in sync_grapheme_map {
            self.grapheme_map.insert(idx, grapheme);
        }
        self.hyperlink_map.clear();
        for &(id, ref uri) in &sync.hyperlink_extras.link_map {
            self.hyperlink_map.insert(id, uri.clone());
        }
        self.hyperlink_cell_map.clear();
        for &(cell_idx, link_id) in &sync.hyperlink_extras.cell_links {
            self.hyperlink_cell_map.insert(cell_idx, link_id);
        }
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
        self.kitty_flags = delta.mode_flags & MODE_KITTY_ALL;
        self.password_input = delta.mode_flags & MODE_PASSWORD_INPUT != 0;

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
                // Evict stale hyperlink entries for overwritten cells
                for idx in dst_start..dst_end {
                    self.hyperlink_cell_map.remove(&(idx as u32));
                }
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
        self.kitty_flags = delta.meta.mode_flags & MODE_KITTY_ALL;
        self.password_input = delta.meta.mode_flags & MODE_PASSWORD_INPUT != 0;

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
                // Evict stale hyperlink entries for overwritten cells
                for idx in dst_start..dst_end {
                    self.hyperlink_cell_map.remove(&(idx as u32));
                }
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
