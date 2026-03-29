use ciri_protocol::message::*;

use super::types::ScrollbackRow;
use super::ClientPaneGrid;

impl ClientPaneGrid {
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
    #[cfg_attr(not(test), allow(dead_code))]
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

    /// Reflow scrollback history to a new column width.
    ///
    /// Joins consecutive wrapped rows into logical lines, then re-splits them
    /// at `new_cols`. This is the standard terminal reflow algorithm used by
    /// alacritty, ghostty, kitty, etc.
    pub(super) fn reflow_scrollback(&mut self, new_cols: usize) {
        if self.scrollback.is_empty() || new_cols == 0 {
            return;
        }
        let old_len = self.scrollback.len();
        let wrapped_count = self.scrollback.iter().filter(|r| r.wrapped).count();
        log::debug!(
            "reflow_scrollback: {} rows, {} wrapped, old_cols={} → new_cols={}",
            old_len,
            wrapped_count,
            self.cols,
            new_cols
        );

        // Phase 1: join wrapped rows into logical lines
        let mut logical_lines: Vec<Vec<PackedCell>> = Vec::new();
        let mut current_line: Vec<PackedCell> = Vec::new();

        for row in self.scrollback.drain(..) {
            // Strip trailing default cells (blank padding) before joining
            let mut cells = row.cells;
            if row.wrapped {
                // For wrapped rows, keep all cells (they filled the full width)
                current_line.append(&mut cells);
            } else {
                // End of logical line: trim trailing blank cells before joining
                let blank = PackedCell::default();
                while cells.last() == Some(&blank) {
                    cells.pop();
                }
                current_line.append(&mut cells);
                logical_lines.push(std::mem::take(&mut current_line));
            }
        }
        // Flush any remaining (last row was wrapped — shouldn't normally happen
        // but handle gracefully)
        if !current_line.is_empty() {
            logical_lines.push(current_line);
        }

        // Phase 2: re-split logical lines at new_cols, respecting wide chars
        let n_logical = logical_lines.len();
        for logical in logical_lines {
            if logical.is_empty() {
                // Preserve empty lines
                self.scrollback.push_back(ScrollbackRow {
                    cells: vec![PackedCell::default(); new_cols],
                    wrapped: false,
                });
                continue;
            }

            // Split at new_cols boundaries, but avoid splitting wide chars.
            // If the last cell of a chunk is the first half of a wide char,
            // move it to the next row to prevent rendering corruption.
            let mut pos = 0;
            while pos < logical.len() {
                // Skip over orphan WIDE_CHAR_SPACER cells (shouldn't happen
                // in well-formed data, but be defensive).
                if logical[pos].flags_u16() & FLAG_WIDE_CHAR_SPACER != 0 {
                    pos += 1;
                    continue;
                }
                // If this cell is a wide char and new_cols < 2, the wide char
                // can't fit — skip the wide+spacer pair entirely.
                if new_cols < 2
                    && logical[pos].flags_u16() & FLAG_WIDE_CHAR != 0
                {
                    pos += 1;
                    // Skip the spacer too if present
                    if pos < logical.len()
                        && logical[pos].flags_u16() & FLAG_WIDE_CHAR_SPACER != 0
                    {
                        pos += 1;
                    }
                    continue;
                }
                let mut end = (pos + new_cols).min(logical.len());
                // Check if we'd split a wide character: the cell at end-1
                // is WIDE_CHAR but its spacer at end would be on the next row.
                if end < logical.len() && end > pos + 1 {
                    let last = &logical[end - 1];
                    if last.flags_u16() & FLAG_WIDE_CHAR != 0 {
                        end -= 1;
                    }
                }
                let is_last = end >= logical.len();
                let mut cells = logical[pos..end].to_vec();
                cells.resize(new_cols, PackedCell::default());
                self.scrollback.push_back(ScrollbackRow {
                    cells,
                    wrapped: !is_last,
                });
                pos = end;
            }
        }
        log::debug!(
            "reflow_scrollback: {} logical lines → {} physical rows",
            n_logical,
            self.scrollback.len()
        );
    }
}
