use ciri_protocol::message::*;
use std::collections::VecDeque;

/// Client-side pane grid with scrollback buffer.
///
/// The buffer stores all lines (scrollback + viewport) as a VecDeque of rows.
/// `scroll_offset = 0` means showing the live viewport (bottom of buffer).
/// `scroll_offset > 0` means viewing history (scrolled up N lines from bottom).
pub struct ClientPaneGrid {
    pub cols: u16,
    pub rows: u16,
    /// Ring buffer: each entry is one row of `cols` PackedCells.
    buffer: VecDeque<Vec<PackedCell>>,
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
    pub dirty: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WordClass {
    Whitespace,
    Word,
    Symbol,
}

impl ClientPaneGrid {
    pub fn new(cols: u16, rows: u16, max_scrollback: usize) -> Self {
        let blank_row = vec![PackedCell::default(); cols as usize];
        let mut buffer = VecDeque::with_capacity(rows as usize + max_scrollback);
        for _ in 0..rows as usize {
            buffer.push_back(blank_row.clone());
        }
        ClientPaneGrid {
            cols,
            rows,
            buffer,
            max_scrollback,
            scroll_offset: 0,
            cursor_line: 0,
            cursor_col: 0,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            title: String::new(),
            dirty: true,
        }
    }

    /// Maximum scroll offset (how far up the user can scroll).
    pub fn max_scroll_offset(&self) -> usize {
        self.buffer.len().saturating_sub(self.rows as usize)
    }

    /// The buffer_row index of the top of the current viewport.
    fn viewport_top(&self) -> usize {
        let total = self.buffer.len();
        let bottom = total.saturating_sub(self.rows as usize);
        bottom.saturating_sub(self.scroll_offset)
    }

    /// Apply a FullPaneSync from the server.
    ///
    /// The server sends:
    /// - `scrollback` + `scrollback_rows`: new history lines since last sync (oldest first)
    /// - `cells` + `rows`: the current live viewport
    ///
    /// We prepend scrollback to the buffer, then replace the live viewport.
    pub fn apply_full_sync(&mut self, sync: &FullPaneSync) {
        let new_cols = sync.cols as usize;
        let new_rows = sync.rows as usize;

        // If dimensions changed, reset buffer
        if sync.cols != self.cols || sync.rows != self.rows {
            self.cols = sync.cols;
            self.rows = sync.rows;
            self.buffer.clear();
            self.scroll_offset = 0;
        }

        // Step 1: Remove old live viewport (last `rows` lines)
        let buf_len = self.buffer.len();
        let live_start = buf_len.saturating_sub(self.rows as usize);
        while self.buffer.len() > live_start {
            self.buffer.pop_back();
        }

        // Step 2: Append new scrollback lines (server sends oldest first → natural order)
        let sb_rows = sync.scrollback_rows as usize;
        for r in 0..sb_rows {
            let start = r * new_cols;
            let end = (start + new_cols).min(sync.scrollback.len());
            if end <= start {
                continue;
            }
            self.buffer.push_back(sync.scrollback[start..end].to_vec());
        }

        // Step 3: Append new live viewport
        for r in 0..new_rows {
            let start = r * new_cols;
            let end = (start + new_cols).min(sync.cells.len());
            self.buffer.push_back(sync.cells[start..end].to_vec());
        }

        // Trim if over max scrollback
        let max_total = self.max_scrollback + new_rows;
        while self.buffer.len() > max_total {
            self.buffer.pop_front();
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
        self.title = sync.title.clone();
        self.dirty = true;
    }

    /// Apply incremental CellDelta: patch the live viewport rows in the buffer.
    pub fn apply_delta(&mut self, delta: &CellDelta) {
        self.cursor_line = delta.cursor_line;
        self.cursor_col = delta.cursor_col;
        self.cursor_shape = delta.cursor_shape;
        self.mode_flags = delta.mode_flags;

        let buf_len = self.buffer.len();
        let live_start = buf_len.saturating_sub(self.rows as usize);

        for region in &delta.regions {
            let line = region.line as usize;
            if line >= self.rows as usize {
                continue;
            }
            let buf_row = live_start + line;
            if buf_row >= buf_len {
                continue;
            }
            for (i, &cell) in region.cells.iter().enumerate() {
                let col = region.left as usize + i;
                if col < self.buffer[buf_row].len() {
                    self.buffer[buf_row][col] = cell;
                }
            }
        }
        self.dirty = true;
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

    /// Jump to the live viewport (bottom).
    pub fn scroll_to_bottom(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset = 0;
            self.dirty = true;
        }
    }

    /// Get the cells for the current viewport (respecting scroll_offset).
    /// Returns a flat Vec of `rows * cols` cells.
    pub fn visible_cells(&self) -> Vec<PackedCell> {
        let top = self.viewport_top();
        let rows = self.rows as usize;
        let cols = self.cols as usize;
        let mut cells = Vec::with_capacity(rows * cols);
        for r in 0..rows {
            let buf_row = top + r;
            if buf_row < self.buffer.len() {
                let row = &self.buffer[buf_row];
                cells.extend_from_slice(row);
                // Pad if row is shorter than cols
                for _ in row.len()..cols {
                    cells.push(PackedCell::default());
                }
            } else {
                // Beyond buffer — blank row
                for _ in 0..cols {
                    cells.push(PackedCell::default());
                }
            }
        }
        cells
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
        let row = self.buffer.get(buffer_row)?;
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

    /// Extract text from a buffer range (absolute buffer_rows).
    pub fn text_in_range(&self, start: (u16, usize), end: (u16, usize)) -> String {
        let (start, end) = if start.1 < end.1 || (start.1 == end.1 && start.0 <= end.0) {
            (start, end)
        } else {
            (end, start)
        };
        let mut result = String::new();
        for buf_row in start.1..=end.1 {
            if buf_row >= self.buffer.len() {
                break;
            }
            let row_data = &self.buffer[buf_row];
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
        if next < row.len() {
            Some(next)
        } else {
            None
        }
    }

    fn cell_end(&self, row: &[PackedCell], mut idx: usize) -> usize {
        while idx + 1 < row.len() && row[idx + 1].flags_u16() & FLAG_WIDE_CHAR_SPACER != 0 {
            idx += 1;
        }
        idx
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

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_with_line(text: &str) -> ClientPaneGrid {
        let mut grid = ClientPaneGrid::new(text.chars().count() as u16, 1, 0);
        let row = grid.buffer.get_mut(0).unwrap();
        row.clear();
        row.extend(text.chars().map(PackedCell::with_ch));
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
}
