use ciri_protocol::message::*;
use std::borrow::Cow;
use std::collections::VecDeque;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkMatch {
    pub url: String,
    pub start_col: u16,
    pub end_col: u16,
}

#[derive(Clone, Copy)]
struct RowChar {
    start_col: u16,
    end_col: u16,
    ch: char,
}

/// Client-side pane grid with split storage: scrollback VecDeque + flat viewport.
///
/// `scrollback` holds history rows as a VecDeque of Vec<PackedCell>.
/// `viewport` is a contiguous `Vec<PackedCell>` of size `rows * cols`,
/// enabling direct memcpy for delta applies and fast clone for visible_cells.
///
/// `scroll_offset = 0` means showing the live viewport (bottom of buffer).
/// `scroll_offset > 0` means viewing history (scrolled up N lines from bottom).
pub struct ClientPaneGrid {
    pub cols: u16,
    pub rows: u16,
    /// History rows (oldest first). Each entry is one row of PackedCells.
    scrollback: VecDeque<Vec<PackedCell>>,
    /// Flat contiguous viewport buffer: rows * cols PackedCells.
    pub viewport: Vec<PackedCell>,
    max_scrollback: usize,
    /// 0 = live (showing bottom), >0 = scrolled up N lines from bottom.
    pub scroll_offset: usize,
    /// Cursor position in the live viewport (from server).
    pub cursor_line: i16,
    pub cursor_col: u16,
    pub cursor_shape: u8,
    /// Terminal mode flags from server (mouse mode, alt screen, etc.)
    pub mode_flags: u8,
    pub title: String,
    /// True when the entire pane needs full rebuild (resize, full sync, scroll, config reload).
    pub dirty: bool,
    /// Per-row dirty flags for incremental updates. Only meaningful when `dirty` is false.
    pub dirty_rows: Vec<bool>,
    /// Grapheme extras lookup: cell_index → full grapheme (primary + zerowidth).
    pub grapheme_map: std::collections::HashMap<u32, String>,
    /// Number of rows currently marked dirty (avoids O(n) scan in is_dirty).
    dirty_row_count: usize,
    /// True if shell integration (OSC 133) is active for this pane.
    pub has_shell_integration: bool,
    /// True if kitty keyboard protocol is active for this pane.
    pub has_kitty_keyboard: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WordClass {
    Whitespace,
    Word,
    Symbol,
}

impl ClientPaneGrid {
    pub fn new(cols: u16, rows: u16, max_scrollback: usize) -> Self {
        ClientPaneGrid {
            cols,
            rows,
            scrollback: VecDeque::with_capacity(max_scrollback),
            viewport: vec![PackedCell::default(); cols as usize * rows as usize],
            max_scrollback,
            scroll_offset: 0,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            title: String::new(),
            dirty: true,
            dirty_rows: vec![false; rows as usize],
            dirty_row_count: 0,
            grapheme_map: std::collections::HashMap::new(),
            has_shell_integration: false,
            has_kitty_keyboard: false,
        }
    }

    /// Access a row by absolute buffer index (0 = oldest scrollback row).
    /// Returns the row slice from either scrollback or viewport.
    fn row(&self, buf_row: usize) -> &[PackedCell] {
        let sb_len = self.scrollback.len();
        if buf_row < sb_len {
            &self.scrollback[buf_row]
        } else {
            let vp_line = buf_row - sb_len;
            let cols = self.cols as usize;
            let start = vp_line * cols;
            let end = start + cols;
            if end <= self.viewport.len() {
                &self.viewport[start..end]
            } else {
                &[]
            }
        }
    }

    /// Total number of lines in the buffer (scrollback + viewport).
    pub fn total_lines(&self) -> usize {
        self.scrollback.len() + self.rows as usize
    }

    /// True if any content has changed (full rebuild or per-row).
    pub fn is_dirty(&self) -> bool {
        self.dirty || self.dirty_row_count > 0
    }

    /// Mark a single row as dirty (idempotent).
    fn mark_row_dirty(&mut self, line: usize) {
        if line < self.dirty_rows.len() && !self.dirty_rows[line] {
            self.dirty_rows[line] = true;
            self.dirty_row_count += 1;
        }
    }

    /// Swap out the dirty_rows flags and reset the counter.
    /// Returns the old dirty flags; leaves a zeroed vec in place.
    pub fn take_dirty_rows(&mut self) -> Vec<bool> {
        let mut taken = vec![false; self.dirty_rows.len()];
        std::mem::swap(&mut taken, &mut self.dirty_rows);
        self.dirty_row_count = 0;
        taken
    }

    /// Clear all dirty flags after rendering.
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
        if self.dirty_row_count > 0 {
            self.dirty_rows.iter_mut().for_each(|d| *d = false);
            self.dirty_row_count = 0;
        }
    }

    /// Maximum scroll offset (how far up the user can scroll).
    pub fn max_scroll_offset(&self) -> usize {
        self.scrollback.len()
    }

    /// Total number of rows in the buffer (scrollback + viewport).
    pub fn buffer_len(&self) -> usize {
        self.scrollback.len() + self.rows as usize
    }

    /// The buffer_row index of the top of the current viewport.
    pub fn viewport_top(&self) -> usize {
        let total = self.scrollback.len() + self.rows as usize;
        let bottom = total.saturating_sub(self.rows as usize);
        bottom.saturating_sub(self.scroll_offset)
    }

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

        // If dimensions changed, resize viewport but preserve scrollback.
        // Old scrollback rows may have a different column count; visible_cells()
        // handles padding/truncation so they remain viewable.
        if sync.cols != self.cols || sync.rows != self.rows {
            self.cols = sync.cols;
            self.rows = sync.rows;
            self.viewport = vec![PackedCell::default(); new_cols * new_rows];
            self.dirty_rows = vec![false; new_rows];
            self.dirty_row_count = 0;
            self.scroll_offset = 0;
        }

        // Step 1: Append new scrollback lines (server sends oldest first)
        let sb_rows = sync.scrollback_rows as usize;
        for r in 0..sb_rows {
            let start = r * new_cols;
            let end = (start + new_cols).min(sync.scrollback.len());
            if end <= start {
                continue;
            }
            self.scrollback
                .push_back(sync.scrollback[start..end].to_vec());
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

        self.cursor_line = sync.cursor_line;
        self.cursor_col = sync.cursor_col;
        self.cursor_shape = sync.cursor_shape;
        self.mode_flags = sync.mode_flags;
        self.has_shell_integration = sync.mode_flags & MODE_SHELL_INTEGRATION != 0;
        self.has_kitty_keyboard = sync.mode_flags & MODE_KITTY_KEYBOARD != 0;
        self.title = sync.title.clone();
        self.grapheme_map = sync.grapheme_extras.build_lookup(&sync.cells);
        self.dirty = true;
    }

    /// Apply incremental CellDelta: patch the live viewport directly using flat buffer indexing.
    /// Kept for use with non-borrowed CellDelta (e.g. tests, offline replay).
    #[allow(dead_code)]
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

        self.cursor_line = delta.cursor_line;
        self.cursor_col = delta.cursor_col;
        self.cursor_shape = delta.cursor_shape;
        self.mode_flags = delta.mode_flags;
        self.has_shell_integration = delta.mode_flags & MODE_SHELL_INTEGRATION != 0;
        self.has_kitty_keyboard = delta.mode_flags & MODE_KITTY_KEYBOARD != 0;

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
        if delta.cursor_line >= 0 {
            self.mark_row_dirty(delta.cursor_line as usize);
        }
    }

    /// Scroll up (into history). Returns actual lines scrolled.
    pub fn scroll_up(&mut self, lines: usize) -> usize {
        let max = self.max_scroll_offset();
        let old = self.scroll_offset;
        self.scroll_offset = (self.scroll_offset + lines).min(max);
        let scrolled = self.scroll_offset - old;
        if scrolled > 0 {
            self.dirty = true;
        }
        scrolled
    }

    /// Scroll down (toward live). Returns actual lines scrolled.
    pub fn scroll_down(&mut self, lines: usize) -> usize {
        let old = self.scroll_offset;
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
        let scrolled = old - self.scroll_offset;
        if scrolled > 0 {
            self.dirty = true;
        }
        scrolled
    }

    /// Set scroll offset directly (clamped to valid range). Used by scrollbar drag.
    pub fn set_scroll_offset(&mut self, offset: usize) {
        let max = self.max_scroll_offset();
        let new = offset.min(max);
        if new != self.scroll_offset {
            self.scroll_offset = new;
            self.dirty = true;
        }
    }

    /// Jump to the live viewport (bottom).
    pub fn scroll_to_bottom(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset = 0;
            self.dirty = true;
        }
    }

    /// Get the cells for the current viewport (respecting scroll_offset).
    /// Returns a borrowed slice when not scrolled (zero-copy fast path),
    /// or an owned Vec when viewing scrollback history.
    pub fn visible_cells(&self) -> Cow<'_, [PackedCell]> {
        // Fast path: if not scrolled, borrow viewport directly (no clone!)
        if self.scroll_offset == 0 {
            return Cow::Borrowed(&self.viewport);
        }

        let top = self.viewport_top();
        let rows = self.rows as usize;
        let cols = self.cols as usize;
        let mut cells = Vec::with_capacity(rows * cols);
        for r in 0..rows {
            let buf_row = top + r;
            let row = self.row(buf_row);
            // Scrollback rows may have a different column count after a
            // resize/DPI change — truncate or pad to current cols.
            let take = row.len().min(cols);
            cells.extend_from_slice(&row[..take]);
            for _ in take..cols {
                cells.push(PackedCell::default());
            }
        }
        Cow::Owned(cells)
    }

    /// Get cursor position in viewport coordinates, or None if cursor is not visible
    /// (e.g., when scrolled away from live viewport).
    pub fn cursor_in_viewport(&self) -> Option<(u16, i16)> {
        if self.scroll_offset == 0 {
            Some((self.cursor_col, self.cursor_line))
        } else {
            None // cursor is at the live viewport, which is not visible
        }
    }

    /// Convert viewport row to buffer row.
    pub fn viewport_to_buffer_row(&self, viewport_row: u16) -> usize {
        self.viewport_top() + viewport_row as usize
    }

    /// Convert buffer row to viewport row, or None if not visible.
    pub fn buffer_to_viewport_row(&self, buffer_row: usize) -> Option<u16> {
        let top = self.viewport_top();
        if buffer_row >= top && buffer_row < top + self.rows as usize {
            Some((buffer_row - top) as u16)
        } else {
            None
        }
    }

    pub fn word_bounds_at(&self, col: u16, buffer_row: usize) -> Option<(u16, u16)> {
        if buffer_row >= self.buffer_len() {
            return None;
        }
        let row = self.row(buffer_row);
        if row.is_empty() {
            return None;
        }

        let mut idx = (col as usize).min(row.len().saturating_sub(1));
        idx = self.normalize_cell_start(row, idx);

        let class = classify_word_cell(row.get(idx)?);
        let mut left = idx;
        while let Some(prev) = self.prev_cell_start(row, left) {
            if classify_word_cell(&row[prev]) != class {
                break;
            }
            left = prev;
        }

        let mut right = idx;
        while let Some(next) = self.next_cell_start(row, right) {
            if classify_word_cell(&row[next]) != class {
                break;
            }
            right = next;
        }

        Some((left as u16, self.cell_end(row, right) as u16))
    }

    pub fn link_at(&self, col: u16, buffer_row: usize) -> Option<LinkMatch> {
        if buffer_row >= self.buffer_len() {
            return None;
        }
        let row = self.row(buffer_row);
        let chars = self.row_chars(row);
        let target_idx = chars
            .iter()
            .position(|cell| col >= cell.start_col && col <= cell.end_col)?;

        let mut start_idx = target_idx;
        while start_idx > 0 && !chars[start_idx - 1].ch.is_whitespace() {
            start_idx -= 1;
        }

        let mut end_idx = target_idx;
        while end_idx + 1 < chars.len() && !chars[end_idx + 1].ch.is_whitespace() {
            end_idx += 1;
        }

        let (start_idx, end_idx) = trim_link_token(&chars, start_idx, end_idx)?;
        if target_idx < start_idx || target_idx > end_idx {
            return None;
        }

        let token: String = chars[start_idx..=end_idx]
            .iter()
            .map(|cell| cell.ch)
            .collect();
        let url = normalize_link_token(&token)?;
        Some(LinkMatch {
            url,
            start_col: chars[start_idx].start_col,
            end_col: chars[end_idx].end_col,
        })
    }

    /// Extract text from a buffer range (absolute buffer_rows).
    pub fn text_in_range(&self, start: (u16, usize), end: (u16, usize)) -> String {
        let (start, end) = if start.1 < end.1 || (start.1 == end.1 && start.0 <= end.0) {
            (start, end)
        } else {
            (end, start)
        };
        let mut result = String::new();
        for buf_row in start.1..=end.1 {
            if buf_row >= self.buffer_len() {
                break;
            }
            let row_data = self.row(buf_row);
            let left = if buf_row == start.1 {
                start.0 as usize
            } else {
                0
            };
            let right = if buf_row == end.1 {
                end.0 as usize
            } else {
                self.cols.saturating_sub(1) as usize
            };
            let mut line = String::new();
            for col in left..=right {
                if col >= row_data.len() {
                    break;
                }
                if row_data[col].flags_u16() & FLAG_WIDE_CHAR_SPACER != 0 {
                    continue;
                }
                let c = row_data[col].ch();
                if c != '\0' {
                    line.push(c);
                }
            }
            let trimmed = line.trim_end();
            result.push_str(trimmed);
            if buf_row < end.1 {
                result.push('\n');
            }
        }
        result
    }

    fn normalize_cell_start(&self, row: &[PackedCell], mut idx: usize) -> usize {
        while idx > 0 && row[idx].flags_u16() & FLAG_WIDE_CHAR_SPACER != 0 {
            idx -= 1;
        }
        idx
    }

    fn prev_cell_start(&self, row: &[PackedCell], idx: usize) -> Option<usize> {
        if idx == 0 {
            return None;
        }
        let mut prev = idx - 1;
        while prev > 0 && row[prev].flags_u16() & FLAG_WIDE_CHAR_SPACER != 0 {
            prev -= 1;
        }
        Some(prev)
    }

    fn next_cell_start(&self, row: &[PackedCell], idx: usize) -> Option<usize> {
        let next = self.cell_end(row, idx) + 1;
        if next < row.len() { Some(next) } else { None }
    }

    fn row_chars(&self, row: &[PackedCell]) -> Vec<RowChar> {
        let mut chars = Vec::with_capacity(row.len());
        let mut idx = 0;
        while idx < row.len() {
            if row[idx].flags_u16() & FLAG_WIDE_CHAR_SPACER != 0 {
                idx += 1;
                continue;
            }
            let end = self.cell_end(row, idx);
            let ch = row[idx].ch();
            if ch != '\0' {
                chars.push(RowChar {
                    start_col: idx as u16,
                    end_col: end as u16,
                    ch,
                });
            }
            idx = end + 1;
        }
        chars
    }

    fn cell_end(&self, row: &[PackedCell], mut idx: usize) -> usize {
        while idx + 1 < row.len() && row[idx + 1].flags_u16() & FLAG_WIDE_CHAR_SPACER != 0 {
            idx += 1;
        }
        idx
    }

    /// Search all lines in the buffer for `query` (case-insensitive).
    /// Returns list of (buffer_row, start_col, end_col) matches.
    pub fn search(&self, query: &str) -> Vec<(usize, u16, u16)> {
        if query.is_empty() {
            return vec![];
        }
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();

        let query_chars: Vec<char> = query_lower.chars().collect();

        let mut chars: Vec<char> = Vec::with_capacity(self.cols as usize);
        let mut col_positions: Vec<u16> = Vec::with_capacity(self.cols as usize);

        for row_idx in 0..self.buffer_len() {
            let row = self.row(row_idx);
            chars.clear();
            col_positions.clear();

            for (col, cell) in row.iter().enumerate() {
                if cell.flags_u16() & FLAG_WIDE_CHAR_SPACER != 0 {
                    continue;
                }
                let ch = cell.ch();
                let lower_ch = if ch == '\0' { ' ' } else { ch };
                for lc in lower_ch.to_lowercase() {
                    chars.push(lc);
                    col_positions.push(col as u16);
                }
            }

            let mut search_from = 0;
            while search_from + query_chars.len() <= chars.len() {
                if chars[search_from..search_from + query_chars.len()] == query_chars[..] {
                    let char_start = search_from;
                    let char_end = search_from + query_chars.len() - 1;
                    results.push((row_idx, col_positions[char_start], col_positions[char_end]));
                    search_from += 1;
                } else {
                    search_from += 1;
                }
            }
        }
        results
    }
}

fn classify_word_cell(cell: &PackedCell) -> WordClass {
    let ch = cell.ch();
    if ch == '\0' || ch.is_whitespace() {
        WordClass::Whitespace
    } else if ch.is_alphanumeric() || ch == '_' {
        WordClass::Word
    } else {
        WordClass::Symbol
    }
}

fn trim_link_token(
    chars: &[RowChar],
    mut start_idx: usize,
    mut end_idx: usize,
) -> Option<(usize, usize)> {
    while start_idx <= end_idx && is_leading_link_punctuation(chars[start_idx].ch) {
        start_idx += 1;
    }
    while start_idx <= end_idx && is_trailing_link_punctuation(chars[end_idx].ch) {
        if end_idx == 0 {
            return None;
        }
        end_idx -= 1;
    }
    if start_idx > end_idx {
        None
    } else {
        Some((start_idx, end_idx))
    }
}

fn normalize_link_token(token: &str) -> Option<String> {
    let lower = token.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        let scheme_len = if lower.starts_with("https://") { 8 } else { 7 };
        token[scheme_len..]
            .chars()
            .any(|ch| ch.is_alphanumeric())
            .then(|| token.to_string())
    } else if lower.starts_with("www.") {
        token[4..]
            .chars()
            .any(|ch| ch.is_alphanumeric())
            .then(|| format!("https://{token}"))
    } else {
        None
    }
}

fn is_leading_link_punctuation(ch: char) -> bool {
    matches!(ch, '(' | '[' | '{' | '<' | '"' | '\'')
}

fn is_trailing_link_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '>' | '"' | '\''
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_with_line(text: &str) -> ClientPaneGrid {
        let cols = text.chars().count() as u16;
        let mut grid = ClientPaneGrid::new(cols, 1, 0);
        for (i, ch) in text.chars().enumerate() {
            grid.viewport[i] = PackedCell::with_ch(ch);
        }
        grid
    }

    #[test]
    fn word_bounds_select_identifier() {
        let grid = grid_with_line("echo hello_world test");
        assert_eq!(grid.word_bounds_at(7, 0), Some((5, 15)));
    }

    #[test]
    fn word_bounds_select_whitespace_run() {
        let grid = grid_with_line("a   b");
        assert_eq!(grid.word_bounds_at(2, 0), Some((1, 3)));
    }

    #[test]
    fn word_bounds_select_symbol_run() {
        let grid = grid_with_line("foo::bar");
        assert_eq!(grid.word_bounds_at(4, 0), Some((3, 4)));
    }

    #[test]
    fn link_at_detects_https_url() {
        let grid = grid_with_line("go https://example.com/docs now");
        assert_eq!(
            grid.link_at(8, 0),
            Some(LinkMatch {
                url: "https://example.com/docs".to_string(),
                start_col: 3,
                end_col: 26,
            })
        );
    }

    #[test]
    fn link_at_trims_wrapping_punctuation() {
        let grid = grid_with_line("(https://example.com/path).");
        assert_eq!(
            grid.link_at(10, 0),
            Some(LinkMatch {
                url: "https://example.com/path".to_string(),
                start_col: 1,
                end_col: 24,
            })
        );
        assert_eq!(grid.link_at(0, 0), None);
        assert_eq!(grid.link_at(25, 0), None);
    }

    #[test]
    fn link_at_normalizes_www_urls() {
        let grid = grid_with_line("visit www.example.com/test soon");
        assert_eq!(
            grid.link_at(10, 0),
            Some(LinkMatch {
                url: "https://www.example.com/test".to_string(),
                start_col: 6,
                end_col: 25,
            })
        );
    }

    // ─── Helper: build a grid with scrollback for scroll tests ─────
    fn grid_with_scrollback() -> ClientPaneGrid {
        // 4 cols, 2 rows, 10 max_scrollback
        let mut grid = ClientPaneGrid::new(4, 2, 10);
        // Feed 3 full syncs to accumulate scrollback
        for round in 0..3u8 {
            let ch = (b'a' + round) as char;
            let sync = FullPaneSync {
                pane_id: 1,
                generation: round as u64,
                cols: 4,
                rows: 2,
                cursor_line: 0,
                cursor_col: 0,
                cursor_shape: CURSOR_BLOCK,
                mode_flags: 0,
                title: String::new(),
                scrollback: vec![PackedCell::with_ch(ch); 4],
                scrollback_rows: 1,
                cells: vec![PackedCell::with_ch(ch.to_ascii_uppercase()); 8],
                grapheme_extras: ciri_protocol::message::GraphemeExtras::new(),
            };
            grid.apply_full_sync(&sync);
        }
        // scrollback: ['a'x4, 'b'x4, 'c'x4], viewport: ['C'x4, 'C'x4]
        grid
    }

    // ─── row() ──────────────────────────────────────────────────────

    #[test]
    fn row_returns_scrollback_row() {
        let grid = grid_with_scrollback();
        let row = grid.row(0);
        assert_eq!(row.len(), 4);
        assert_eq!(row[0].ch(), 'a');
    }

    #[test]
    fn row_returns_viewport_row() {
        let grid = grid_with_scrollback();
        let sb = grid.scrollback.len(); // 3
        let row = grid.row(sb); // first viewport row
        assert_eq!(row.len(), 4);
        assert_eq!(row[0].ch(), 'C');
    }

    #[test]
    fn row_out_of_bounds_returns_empty() {
        let grid = grid_with_scrollback();
        let row = grid.row(9999);
        assert!(row.is_empty());
    }

    // ─── viewport_top() ─────────────────────────────────────────────

    #[test]
    fn viewport_top_no_scroll() {
        let grid = grid_with_scrollback();
        // scroll_offset=0 → top = scrollback.len()
        assert_eq!(grid.viewport_top(), grid.scrollback.len());
    }

    #[test]
    fn viewport_top_scrolled_up() {
        let mut grid = grid_with_scrollback();
        grid.scroll_up(2);
        // scroll_offset=2 → top = scrollback.len() - 2
        assert_eq!(grid.viewport_top(), grid.scrollback.len() - 2);
    }

    #[test]
    fn viewport_top_scrolled_max() {
        let mut grid = grid_with_scrollback();
        grid.scroll_up(100); // clamped to max_scroll_offset = scrollback.len()
        assert_eq!(grid.viewport_top(), 0);
    }

    // ─── visible_cells() ────────────────────────────────────────────

    #[test]
    fn visible_cells_no_scroll_returns_viewport_clone() {
        let grid = grid_with_scrollback();
        let cells = grid.visible_cells();
        assert_eq!(cells.len(), 8); // 2 rows * 4 cols
        assert_eq!(cells[0].ch(), 'C');
        assert_eq!(cells[7].ch(), 'C');
    }

    #[test]
    fn visible_cells_scrolled_mixes_scrollback_and_viewport() {
        let mut grid = grid_with_scrollback();
        // scrollback: [row0='a', row1='b', row2='c'], viewport: [row3='C', row4='C']
        grid.scroll_up(1);
        let cells = grid.visible_cells();
        assert_eq!(cells.len(), 8);
        // top row should be last scrollback row ('c')
        assert_eq!(cells[0].ch(), 'c');
        // bottom row should be first viewport row ('C')
        assert_eq!(cells[4].ch(), 'C');
    }

    #[test]
    fn visible_cells_scrolled_to_top() {
        let mut grid = grid_with_scrollback();
        grid.scroll_up(100); // clamp to max=3
        let cells = grid.visible_cells();
        // showing scrollback rows 0 and 1 ('a' and 'b')
        assert_eq!(cells[0].ch(), 'a');
        assert_eq!(cells[4].ch(), 'b');
    }

    // ─── scroll clamping ────────────────────────────────────────────

    #[test]
    fn scroll_up_returns_actual_scrolled() {
        let mut grid = grid_with_scrollback(); // 3 scrollback rows
        assert_eq!(grid.scroll_up(2), 2);
        assert_eq!(grid.scroll_up(5), 1); // only 1 more possible
        assert_eq!(grid.scroll_up(1), 0); // already at max
    }

    #[test]
    fn scroll_down_returns_actual_scrolled() {
        let mut grid = grid_with_scrollback();
        grid.scroll_up(3);
        assert_eq!(grid.scroll_down(2), 2);
        assert_eq!(grid.scroll_down(5), 1); // only 1 left
        assert_eq!(grid.scroll_down(1), 0); // already at bottom
    }

    #[test]
    fn scroll_up_no_scrollback_returns_zero() {
        let mut grid = ClientPaneGrid::new(4, 2, 0);
        assert_eq!(grid.scroll_up(10), 0);
    }

    #[test]
    fn scroll_to_bottom_resets_offset() {
        let mut grid = grid_with_scrollback();
        grid.scroll_up(2);
        assert_eq!(grid.scroll_offset, 2);
        grid.scroll_to_bottom();
        assert_eq!(grid.scroll_offset, 0);
    }

    // ─── apply_full_sync edge cases ─────────────────────────────────

    #[test]
    fn full_sync_dimension_change_preserves_scrollback() {
        let mut grid = grid_with_scrollback();
        assert_eq!(grid.scrollback.len(), 3);

        // Resize from 4x2 to 6x3
        let sync = FullPaneSync {
            pane_id: 1,
            generation: 10,
            cols: 6,
            rows: 3,
            cursor_line: 1,
            cursor_col: 2,
            cursor_shape: CURSOR_BEAM,
            mode_flags: 0,
            title: "resized".into(),
            scrollback: vec![],
            scrollback_rows: 0,
            cells: vec![PackedCell::with_ch('X'); 18],
                grapheme_extras: ciri_protocol::message::GraphemeExtras::new(),
        };
        grid.apply_full_sync(&sync);
        assert_eq!(grid.cols, 6);
        assert_eq!(grid.rows, 3);
        assert_eq!(grid.scrollback.len(), 3); // preserved across resize
        assert_eq!(grid.viewport.len(), 18);
        assert_eq!(grid.viewport[0].ch(), 'X');
        assert_eq!(grid.scroll_offset, 0);

        // Old scrollback rows are 4 cols wide; visible_cells() pads to 6
        grid.scroll_up(1);
        let cells = grid.visible_cells();
        assert_eq!(cells.len(), 18); // 3 rows × 6 cols
        // Last scrollback row ('c' × 4) padded to 6 cols
        assert_eq!(cells[0].ch(), 'c');
        assert_eq!(cells[3].ch(), 'c');
        assert_eq!(cells[4].ch(), ' '); // padded
        assert_eq!(cells[5].ch(), ' '); // padded
    }

    #[test]
    fn full_sync_short_cells_blanks_remainder() {
        let mut grid = ClientPaneGrid::new(4, 2, 10);
        let sync = FullPaneSync {
            pane_id: 1,
            generation: 1,
            cols: 4,
            rows: 2,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            title: String::new(),
            scrollback: vec![],
            scrollback_rows: 0,
            cells: vec![PackedCell::with_ch('A'); 3], // only 3 of 8 cells
            grapheme_extras: GraphemeExtras::new(),
        };
        grid.apply_full_sync(&sync);
        assert_eq!(grid.viewport[0].ch(), 'A');
        assert_eq!(grid.viewport[2].ch(), 'A');
        assert_eq!(grid.viewport[3].ch(), ' '); // default blank
        assert_eq!(grid.viewport[7].ch(), ' ');
    }

    #[test]
    fn full_sync_scrollback_trimmed_to_max() {
        let mut grid = ClientPaneGrid::new(2, 1, 3); // max 3 scrollback rows
        for i in 0u8..10 {
            let ch = (b'0' + i) as char;
            let sync = FullPaneSync {
                pane_id: 1,
                generation: i as u64,
                cols: 2,
                rows: 1,
                cursor_line: 0,
                cursor_col: 0,
                cursor_shape: CURSOR_BLOCK,
                mode_flags: 0,
                title: String::new(),
                scrollback: vec![PackedCell::with_ch(ch); 2],
                scrollback_rows: 1,
                cells: vec![PackedCell::with_ch('.'); 2],
                grapheme_extras: ciri_protocol::message::GraphemeExtras::new(),
            };
            grid.apply_full_sync(&sync);
        }
        // Should keep only the last 3 scrollback rows
        assert_eq!(grid.scrollback.len(), 3);
        // Oldest surviving = '7', then '8', then '9'
        assert_eq!(grid.scrollback[0][0].ch(), '7');
        assert_eq!(grid.scrollback[2][0].ch(), '9');
    }

    #[test]
    fn full_sync_clamps_scroll_offset() {
        let mut grid = ClientPaneGrid::new(2, 1, 5);
        // Add some scrollback
        for i in 0..4u8 {
            let sync = FullPaneSync {
                pane_id: 1,
                generation: i as u64,
                cols: 2,
                rows: 1,
                cursor_line: 0,
                cursor_col: 0,
                cursor_shape: CURSOR_BLOCK,
                mode_flags: 0,
                title: String::new(),
                scrollback: vec![PackedCell::with_ch('x'); 2],
                scrollback_rows: 1,
                cells: vec![PackedCell::default(); 2],
                grapheme_extras: ciri_protocol::message::GraphemeExtras::new(),
            };
            grid.apply_full_sync(&sync);
        }
        assert_eq!(grid.scrollback.len(), 4);
        grid.scroll_up(4); // scroll to top
        assert_eq!(grid.scroll_offset, 4);

        // Resize resets scroll_offset to 0, but preserves scrollback
        let sync = FullPaneSync {
            pane_id: 1,
            generation: 10,
            cols: 3,
            rows: 1, // dimension change
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            title: String::new(),
            scrollback: vec![],
            scrollback_rows: 0,
            cells: vec![PackedCell::default(); 3],
                grapheme_extras: ciri_protocol::message::GraphemeExtras::new(),
        };
        grid.apply_full_sync(&sync);
        assert_eq!(grid.scroll_offset, 0); // reset by dimension change
        assert_eq!(grid.scrollback.len(), 4); // scrollback preserved
    }

    // ─── apply_delta edge cases ─────────────────────────────────────

    #[test]
    fn delta_apply_second_row() {
        let mut grid = ClientPaneGrid::new(5, 3, 0);
        let delta = CellDelta {
            pane_id: 1,
            generation: 1,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: 0,
            mode_flags: 0,
            regions: vec![DamageRegion {
                line: 2,
                left: 1,
                right: 3,
                cells: vec![
                    PackedCell::with_ch('X'),
                    PackedCell::with_ch('Y'),
                    PackedCell::with_ch('Z'),
                ],
            }],
        };
        grid.apply_delta(&delta);
        // Row 2 (offset = 2 * 5 = 10), cols 1-3
        assert_eq!(grid.viewport[10].ch(), ' '); // col 0 untouched
        assert_eq!(grid.viewport[11].ch(), 'X');
        assert_eq!(grid.viewport[12].ch(), 'Y');
        assert_eq!(grid.viewport[13].ch(), 'Z');
        assert_eq!(grid.viewport[14].ch(), ' '); // col 4 untouched
    }

    #[test]
    fn delta_out_of_bounds_line_skipped() {
        let mut grid = ClientPaneGrid::new(4, 2, 0);
        let delta = CellDelta {
            pane_id: 1,
            generation: 1,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: 0,
            mode_flags: 0,
            regions: vec![DamageRegion {
                line: 5,
                left: 0,
                right: 0, // line 5 doesn't exist in 2-row grid
                cells: vec![PackedCell::with_ch('X')],
            }],
        };
        grid.apply_delta(&delta);
        // Should not panic, viewport unchanged
        assert_eq!(grid.viewport[0].ch(), ' ');
    }

    #[test]
    fn delta_region_clamped_to_cols() {
        let mut grid = ClientPaneGrid::new(3, 1, 0);
        let delta = CellDelta {
            pane_id: 1,
            generation: 1,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: 0,
            mode_flags: 0,
            regions: vec![DamageRegion {
                line: 0,
                left: 1,
                right: 9, // right extends way past cols=3
                cells: vec![PackedCell::with_ch('X'); 9],
            }],
        };
        grid.apply_delta(&delta);
        // Only cols 1 and 2 should be written (clamped to cols=3)
        assert_eq!(grid.viewport[0].ch(), ' ');
        assert_eq!(grid.viewport[1].ch(), 'X');
        assert_eq!(grid.viewport[2].ch(), 'X');
    }

    #[test]
    fn delta_sets_mode_flags() {
        let mut grid = ClientPaneGrid::new(4, 1, 0);
        let delta = CellDelta {
            pane_id: 1,
            generation: 1,
            cursor_line: 0,
            cursor_col: 3,
            cursor_shape: CURSOR_BEAM,
            mode_flags: MODE_SHELL_INTEGRATION | MODE_ALT_SCREEN,
            regions: vec![],
        };
        grid.apply_delta(&delta);
        assert_eq!(grid.cursor_col, 3);
        assert_eq!(grid.cursor_shape, CURSOR_BEAM);
        assert!(grid.has_shell_integration);
        assert!(!grid.has_kitty_keyboard);
    }

    // ─── text_in_range spanning scrollback + viewport ────────────────

    #[test]
    fn text_in_range_spans_scrollback_and_viewport() {
        let mut grid = ClientPaneGrid::new(3, 1, 10);
        // Add one scrollback row 'abc', viewport row 'XYZ'
        let sync = FullPaneSync {
            pane_id: 1,
            generation: 1,
            cols: 3,
            rows: 1,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            title: String::new(),
            scrollback: vec![
                PackedCell::with_ch('a'),
                PackedCell::with_ch('b'),
                PackedCell::with_ch('c'),
            ],
            scrollback_rows: 1,
            cells: vec![
                PackedCell::with_ch('X'),
                PackedCell::with_ch('Y'),
                PackedCell::with_ch('Z'),
            ],
            grapheme_extras: GraphemeExtras::new(),
        };
        grid.apply_full_sync(&sync);
        // buffer_row 0 = scrollback 'abc', buffer_row 1 = viewport 'XYZ'
        let text = grid.text_in_range((0, 0), (2, 1));
        assert_eq!(text, "abc\nXYZ");
    }

    // ─── search spanning scrollback + viewport ──────────────────────

    #[test]
    fn search_finds_in_scrollback_and_viewport() {
        let mut grid = ClientPaneGrid::new(5, 1, 10);
        let sync = FullPaneSync {
            pane_id: 1,
            generation: 1,
            cols: 5,
            rows: 1,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            title: String::new(),
            scrollback: vec![
                PackedCell::with_ch('h'),
                PackedCell::with_ch('e'),
                PackedCell::with_ch('l'),
                PackedCell::with_ch('l'),
                PackedCell::with_ch('o'),
            ],
            scrollback_rows: 1,
            cells: vec![
                PackedCell::with_ch('h'),
                PackedCell::with_ch('e'),
                PackedCell::with_ch('l'),
                PackedCell::with_ch('l'),
                PackedCell::with_ch('o'),
            ],
            grapheme_extras: GraphemeExtras::new(),
        };
        grid.apply_full_sync(&sync);
        let results = grid.search("hello");
        assert_eq!(results.len(), 2); // found in both scrollback row 0 and viewport row 1
        assert_eq!(results[0].0, 0); // scrollback row
        assert_eq!(results[1].0, 1); // viewport row
    }

    // ─── word_bounds_at / link_at bounds ────────────────────────────

    #[test]
    fn word_bounds_at_out_of_bounds_returns_none() {
        let grid = ClientPaneGrid::new(4, 2, 0);
        assert_eq!(grid.word_bounds_at(0, 999), None);
    }

    #[test]
    fn link_at_out_of_bounds_returns_none() {
        let grid = ClientPaneGrid::new(4, 2, 0);
        assert_eq!(grid.link_at(0, 999), None);
    }

    #[test]
    fn word_bounds_on_viewport_row() {
        let mut grid = ClientPaneGrid::new(5, 1, 10);
        let sync = FullPaneSync {
            pane_id: 1,
            generation: 1,
            cols: 5,
            rows: 1,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            title: String::new(),
            scrollback: vec![PackedCell::with_ch('x'); 5],
            scrollback_rows: 1,
            cells: vec![
                PackedCell::with_ch('a'),
                PackedCell::with_ch('b'),
                PackedCell::with_ch(' '),
                PackedCell::with_ch('c'),
                PackedCell::with_ch('d'),
            ],
            grapheme_extras: GraphemeExtras::new(),
        };
        grid.apply_full_sync(&sync);
        // buffer_row 1 = viewport row → "ab cd"
        assert_eq!(grid.word_bounds_at(0, 1), Some((0, 1))); // "ab"
        assert_eq!(grid.word_bounds_at(3, 1), Some((3, 4))); // "cd"
    }

    // ─── existing tests (unchanged) ─────────────────────────────────

    #[test]
    fn delta_apply_patches_viewport() {
        let mut grid = ClientPaneGrid::new(10, 2, 0);
        let delta = CellDelta {
            pane_id: 1,
            generation: 1,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            regions: vec![DamageRegion {
                line: 0,
                left: 2,
                right: 4,
                cells: vec![
                    PackedCell::with_ch('A'),
                    PackedCell::with_ch('B'),
                    PackedCell::with_ch('C'),
                ],
            }],
        };
        grid.apply_delta(&delta);
        assert_eq!(grid.viewport[2].ch(), 'A');
        assert_eq!(grid.viewport[3].ch(), 'B');
        assert_eq!(grid.viewport[4].ch(), 'C');
    }

    #[test]
    fn full_sync_populates_viewport_and_scrollback() {
        let mut grid = ClientPaneGrid::new(4, 2, 100);
        let sync = FullPaneSync {
            pane_id: 1,
            generation: 1,
            cols: 4,
            rows: 2,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            title: String::new(),
            scrollback: vec![PackedCell::with_ch('S'); 4],
            scrollback_rows: 1,
            cells: vec![
                PackedCell::with_ch('A'),
                PackedCell::with_ch('B'),
                PackedCell::with_ch('C'),
                PackedCell::with_ch('D'),
                PackedCell::with_ch('E'),
                PackedCell::with_ch('F'),
                PackedCell::with_ch('G'),
                PackedCell::with_ch('H'),
            ],
            grapheme_extras: GraphemeExtras::new(),
        };
        grid.apply_full_sync(&sync);
        assert_eq!(grid.scrollback.len(), 1);
        assert_eq!(grid.viewport[0].ch(), 'A');
        assert_eq!(grid.viewport[7].ch(), 'H');
        assert_eq!(grid.total_lines(), 3); // 1 scrollback + 2 viewport
        assert_eq!(grid.max_scroll_offset(), 1);
    }
}
