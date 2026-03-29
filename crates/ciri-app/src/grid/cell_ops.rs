use std::borrow::Cow;

use ciri_protocol::message::*;

use super::types::{RowChar, classify_word_cell};
use super::ClientPaneGrid;

impl ClientPaneGrid {
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

    pub(super) fn normalize_cell_start(&self, row: &[PackedCell], mut idx: usize) -> usize {
        while idx > 0 && row[idx].flags_u16() & FLAG_WIDE_CHAR_SPACER != 0 {
            idx -= 1;
        }
        idx
    }

    pub(super) fn prev_cell_start(&self, row: &[PackedCell], idx: usize) -> Option<usize> {
        if idx == 0 {
            return None;
        }
        let mut prev = idx - 1;
        while prev > 0 && row[prev].flags_u16() & FLAG_WIDE_CHAR_SPACER != 0 {
            prev -= 1;
        }
        Some(prev)
    }

    pub(super) fn next_cell_start(&self, row: &[PackedCell], idx: usize) -> Option<usize> {
        let next = self.cell_end(row, idx) + 1;
        if next < row.len() { Some(next) } else { None }
    }

    pub(super) fn row_chars(&self, row: &[PackedCell]) -> Vec<RowChar> {
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

    pub(super) fn cell_end(&self, row: &[PackedCell], mut idx: usize) -> usize {
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
