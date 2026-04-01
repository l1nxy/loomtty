use ciri_protocol::message::*;
use std::collections::VecDeque;

mod cell_ops;
mod link;
mod scrollback;
mod sync;
mod types;

#[cfg(test)]
mod tests;

pub use types::{LinkMatch, ScrollbackRow};

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
    /// History rows (oldest first), each with a wrap flag for reflow.
    pub(super) scrollback: VecDeque<ScrollbackRow>,
    /// Flat contiguous viewport buffer: rows * cols PackedCells.
    pub viewport: Vec<PackedCell>,
    pub(super) max_scrollback: usize,
    /// 0 = live (showing bottom), >0 = scrolled up N lines from bottom.
    pub scroll_offset: usize,
    /// Cursor position in the live viewport (from server).
    pub cursor_line: i16,
    pub cursor_col: u16,
    pub cursor_shape: u8,
    /// Terminal mode flags from server (mouse mode, alt screen, kitty keyboard levels, etc.)
    pub mode_flags: u16,
    pub title: String,
    /// True when the entire pane needs full rebuild (resize, full sync, scroll, config reload).
    pub dirty: bool,
    /// Per-row dirty flags for incremental updates. Only meaningful when `dirty` is false.
    pub dirty_rows: Vec<bool>,
    /// Grapheme extras lookup: cell_index → full grapheme (primary + zerowidth).
    pub grapheme_map: std::collections::HashMap<u32, String>,
    /// Number of rows currently marked dirty (avoids O(n) scan in is_dirty).
    pub(super) dirty_row_count: usize,
    /// True if shell integration (OSC 133) is active for this pane.
    pub has_shell_integration: bool,
    /// Kitty keyboard protocol flags (bitmask of MODE_KITTY_* constants).
    pub kitty_flags: u16,
    /// True if password input detected (PTY ECHO disabled in canonical mode).
    pub password_input: bool,
    /// OSC 8 hyperlink map: link_id → URI (from server's HyperlinkExtras).
    pub hyperlink_map: Vec<(u16, String)>,
    /// Current working directory from OSC 7 (reported by the shell via server).
    pub cwd: Option<String>,
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
            kitty_flags: 0,
            password_input: false,
            hyperlink_map: Vec::new(),
            cwd: None,
        }
    }

    /// Access a row by absolute buffer index (0 = oldest scrollback row).
    /// Returns the row slice from either scrollback or viewport.
    pub(super) fn row(&self, buf_row: usize) -> &[PackedCell] {
        let sb_len = self.scrollback.len();
        if buf_row < sb_len {
            &self.scrollback[buf_row].cells
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
    pub(super) fn mark_row_dirty(&mut self, line: usize) {
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
}
