use ciri_protocol::message::*;
use std::time::Instant;
use unicode_width::UnicodeWidthChar;

use super::PredictionEngine;
use super::overlay::{PaneOverlay, PredictedCursor};
use crate::grid::ClientPaneGrid;

impl PredictionEngine {
    fn starts_local_edit(data: &[u8]) -> bool {
        !data.is_empty() && !data.iter().any(|b| matches!(*b, 0x00..=0x1F | 0x7F))
    }

    fn ends_local_edit(data: &[u8]) -> bool {
        data.iter().any(|b| matches!(*b, 0x03 | 0x04 | 0x0A | 0x0D))
    }

    fn backspace_only(data: &[u8]) -> bool {
        !data.is_empty() && data.iter().all(|b| matches!(*b, 0x08 | 0x7F))
    }

    fn requires_local_edit_start(grid: &ClientPaneGrid) -> bool {
        grid.mode_flags & (MODE_ALT_SCREEN | MODE_MOUSE_REPORT | MODE_BRACKETED_PASTE) != 0
            || grid.kitty_flags != 0
    }

    fn clear_local_edit_state(&mut self, pane_id: u64) {
        self.overlays.remove(&pane_id);
        self.force_visible_panes.remove(&pane_id);
    }

    fn cursor_at_or_before(row: i16, col: u16, start: (i16, u16)) -> bool {
        row < start.0 || (row == start.0 && col <= start.1)
    }

    /// Predict inserting `ch` at cursor position with insert-mode right-shift.
    /// Returns the char width (columns consumed), or 0 if prediction was abandoned.
    pub(super) fn predict_char(
        overlay: &mut PaneOverlay,
        grid: &ClientPaneGrid,
        ch: char,
        crow: i16,
        ccol: u16,
        min_ack: u64,
        show_ul: bool,
        tolerate_mismatch: bool,
    ) -> u16 {
        let char_width = ch.width().unwrap_or(1) as u16;
        if ccol + char_width > grid.cols {
            overlay.increment_epoch();
            return 0;
        }

        let row_idx = crow as u16;
        let cols = grid.cols;
        let epoch = overlay.prediction_epoch;
        let now = Instant::now();

        // Insert-mode: shift cells right by char_width positions.
        let orow = overlay.get_or_make_row(row_idx);
        for col in (ccol..cols.saturating_sub(char_width)).rev() {
            let dest = col + char_width;
            if dest >= cols {
                continue;
            }
            let src_cell = if orow.cells[col as usize].active {
                orow.cells[col as usize].replacement
            } else {
                let idx = (row_idx as usize) * (cols as usize) + (col as usize);
                grid.viewport.get(idx).copied().unwrap_or_default()
            };
            let src_unknown = orow.cells[col as usize].active && orow.cells[col as usize].unknown;
            let orig_idx = (row_idx as usize) * (cols as usize) + (dest as usize);
            let orig_ch = grid.viewport.get(orig_idx).map(|c| c.ch()).unwrap_or('\0');

            let mut shifted = src_cell;
            let f = shifted.flags_u16() & !(FLAG_WIDE_CHAR | FLAG_WIDE_CHAR_SPACER);
            shifted.flags = f.to_le_bytes();

            let dc = &mut orow.cells[dest as usize];
            dc.active = true;
            dc.replacement = shifted;
            dc.epoch = epoch;
            dc.created_at = now;
            dc.min_echo_ack = min_ack;
            dc.original_ch = orig_ch;
            dc.unknown = src_unknown;
            dc.tolerate_mismatch = tolerate_mismatch;
        }

        // Mark rightmost cells as unknown.
        for i in 0..char_width {
            let uc = cols - 1 - i;
            if uc >= ccol + char_width {
                let dc = &mut orow.cells[uc as usize];
                if !dc.active {
                    let orig_idx = (row_idx as usize) * (cols as usize) + (uc as usize);
                    dc.active = true;
                    dc.replacement = PackedCell::default();
                    dc.epoch = epoch;
                    dc.created_at = now;
                    dc.min_echo_ack = min_ack;
                    dc.original_ch = grid.viewport.get(orig_idx).map(|c| c.ch()).unwrap_or('\0');
                }
                orow.cells[uc as usize].unknown = true;
                orow.cells[uc as usize].tolerate_mismatch = tolerate_mismatch;
            }
        }

        // Place the character, inheriting renditions from left neighbor.
        let orig_idx = (row_idx as usize) * (cols as usize) + (ccol as usize);
        let orig_ch = grid.viewport.get(orig_idx).map(|c| c.ch()).unwrap_or('\0');

        let inherit_from = if ccol > 0 {
            let left_col = ccol - 1;
            if orow.cells[left_col as usize].active && !orow.cells[left_col as usize].unknown {
                orow.cells[left_col as usize].replacement
            } else {
                let li = (row_idx as usize) * (cols as usize) + (left_col as usize);
                grid.viewport.get(li).copied().unwrap_or_default()
            }
        } else {
            grid.viewport.get(orig_idx).copied().unwrap_or_default()
        };

        let mut cell = PackedCell::default();
        cell.set_ch(ch);
        cell.fg = inherit_from.fg;
        cell.bg = inherit_from.bg;
        let mut flags = cell.flags_u16();
        let attr_mask = FLAG_BOLD
            | FLAG_ITALIC
            | FLAG_DIM
            | FLAG_STRIKEOUT
            | FLAG_HIDDEN
            | FLAG_INVERSE
            | FLAG_UNDERLINE
            | FLAG_UNDERLINE_STYLE_MASK;
        flags |= inherit_from.flags_u16() & attr_mask;
        if show_ul {
            flags |= FLAG_UNDERLINE;
        }
        if char_width == 2 {
            flags |= FLAG_WIDE_CHAR;
        }
        cell.flags = flags.to_le_bytes();

        let cc = &mut orow.cells[ccol as usize];
        cc.active = true;
        cc.replacement = cell;
        cc.epoch = epoch;
        cc.created_at = now;
        cc.min_echo_ack = min_ack;
        cc.original_ch = orig_ch;
        cc.unknown = false;
        cc.tolerate_mismatch = tolerate_mismatch;

        // Wide char spacer.
        if char_width == 2 {
            let spacer_col = ccol + 1;
            let spacer_orig_idx = (row_idx as usize) * (cols as usize) + (spacer_col as usize);
            let spacer_orig = grid
                .viewport
                .get(spacer_orig_idx)
                .map(|c| c.ch())
                .unwrap_or('\0');
            let mut spacer = PackedCell::default();
            spacer.set_ch(' ');
            spacer.flags = FLAG_WIDE_CHAR_SPACER.to_le_bytes();

            let sc = &mut orow.cells[spacer_col as usize];
            sc.active = true;
            sc.replacement = spacer;
            sc.epoch = epoch;
            sc.created_at = now;
            sc.min_echo_ack = min_ack;
            sc.original_ch = spacer_orig;
            sc.unknown = false;
            sc.tolerate_mismatch = tolerate_mismatch;
        }

        char_width
    }

    /// Process user keyboard input and generate predictions.
    pub fn new_user_input(&mut self, pane_id: u64, data: &[u8], grid: &ClientPaneGrid) {
        self.new_user_input_internal(pane_id, data, grid, self.next_input_seq, false, false);
    }

    /// Process user keyboard input and generate predictions tied to a specific input ack.
    pub fn new_user_input_with_min_ack(
        &mut self,
        pane_id: u64,
        data: &[u8],
        grid: &ClientPaneGrid,
        min_ack: u64,
    ) {
        self.new_user_input_internal(pane_id, data, grid, min_ack, false, false);
    }

    /// Track local editable input even when speculative display is disabled.
    ///
    /// This keeps the prediction cursor/cells coherent for a following
    /// force-visible Backspace without making regular typed text visible in
    /// `PredictionMode::Never`.
    pub fn new_user_input_track_hidden(
        &mut self,
        pane_id: u64,
        data: &[u8],
        grid: &ClientPaneGrid,
        min_ack: u64,
    ) {
        self.new_user_input_internal(pane_id, data, grid, min_ack, false, true);
    }

    /// Process input as an immediately visible local prediction, even when
    /// regular speculative echo is disabled.
    pub fn new_user_input_force_visible(
        &mut self,
        pane_id: u64,
        data: &[u8],
        grid: &ClientPaneGrid,
        min_ack: u64,
    ) {
        self.new_user_input_internal(pane_id, data, grid, min_ack, true, true);
    }

    fn new_user_input_internal(
        &mut self,
        pane_id: u64,
        data: &[u8],
        grid: &ClientPaneGrid,
        min_ack: u64,
        force_visible: bool,
        force_track: bool,
    ) {
        if self.mode == ciri_config::config::PredictionMode::Never && !force_track {
            if Self::ends_local_edit(data) {
                self.clear_local_edit_state(pane_id);
            }
            return;
        }
        if grid.cols == 0 || grid.rows == 0 {
            return;
        }
        if !force_track
            && (grid.mode_flags & MODE_ALT_SCREEN != 0
                || grid.mode_flags & MODE_MOUSE_REPORT != 0
                || grid.mode_flags & MODE_BRACKETED_PASTE != 0)
        {
            if Self::ends_local_edit(data) {
                self.clear_local_edit_state(pane_id);
            }
            return;
        }

        self.bump_visual_serial_for_pane(pane_id);

        let show_ul = self.show_underline
            && (self.flagging || self.mode == ciri_config::config::PredictionMode::Always);
        let cols = grid.cols;

        let overlay = self
            .overlays
            .entry(pane_id)
            .or_insert_with(|| PaneOverlay::new(cols));

        if overlay.cols != cols {
            *overlay = PaneOverlay::new(cols);
        }

        let (mut crow, mut ccol) = overlay
            .cursor_position()
            .unwrap_or((grid.cursor_line, grid.cursor_col));

        if crow < 0 {
            overlay.increment_epoch();
            return;
        }
        if force_track
            && !force_visible
            && overlay.local_edit_start.is_none()
            && Self::starts_local_edit(data)
        {
            overlay.local_edit_start = Some((crow, ccol));
        }
        if force_visible
            && Self::backspace_only(data)
            && overlay.local_edit_start.is_none()
            && Self::requires_local_edit_start(grid)
        {
            if overlay.is_empty() {
                self.overlays.remove(&pane_id);
            }
            self.force_visible_panes.remove(&pane_id);
            return;
        }
        if force_visible {
            self.force_visible_panes.insert(pane_id);
        }

        // Arrow key escape sequences (normal + application mode)
        if data == b"\x1B[C" || data == b"\x1BOC" {
            if ccol < cols.saturating_sub(1) {
                ccol += 1;
            }
            if ccol < cols {
                overlay.cursor = Some(PredictedCursor {
                    row: crow,
                    col: ccol,
                    epoch: overlay.prediction_epoch,
                    min_echo_ack: min_ack,
                    created_at: Instant::now(),
                    tolerate_mismatch: force_track,
                });
            }
            return;
        }
        if data == b"\x1B[D" || data == b"\x1BOD" {
            if ccol > 0 {
                ccol -= 1;
            }
            overlay.cursor = Some(PredictedCursor {
                row: crow,
                col: ccol,
                epoch: overlay.prediction_epoch,
                min_echo_ack: min_ack,
                created_at: Instant::now(),
                tolerate_mismatch: force_track,
            });
            return;
        }

        for &byte in data {
            match byte {
                0x20..=0x7E => {
                    let w = Self::predict_char(
                        overlay,
                        grid,
                        byte as char,
                        crow,
                        ccol,
                        min_ack,
                        show_ul,
                        force_track,
                    );
                    if w == 0 {
                        return;
                    }
                    ccol += w;
                }
                0xC2..=0xDF | 0xE0..=0xEF | 0xF0..=0xF4 => {
                    overlay.utf8.start(byte);
                }
                0x80..=0xBF => {
                    if let Some(ch) = overlay.utf8.push_cont(byte) {
                        let w = Self::predict_char(
                            overlay,
                            grid,
                            ch,
                            crow,
                            ccol,
                            min_ack,
                            show_ul,
                            force_track,
                        );
                        if w == 0 {
                            return;
                        }
                        ccol += w;
                    }
                }
                0x08 | 0x7F => {
                    if force_visible {
                        if let Some(start) = overlay.local_edit_start {
                            if Self::cursor_at_or_before(crow, ccol, start) {
                                crow = start.0;
                                ccol = start.1;
                                overlay.cursor = Some(PredictedCursor {
                                    row: crow,
                                    col: ccol,
                                    epoch: overlay.prediction_epoch,
                                    min_echo_ack: min_ack,
                                    created_at: Instant::now(),
                                    tolerate_mismatch: force_track,
                                });
                                continue;
                            }
                        }
                    }
                    if ccol == 0 {
                        if crow <= 0 {
                            overlay.increment_epoch();
                            continue;
                        }
                        crow -= 1;
                        ccol = cols;
                    }
                    ccol -= 1;
                    let row_idx = crow as u16;

                    let del_cell = overlay.effective_cell(grid, row_idx, ccol);
                    let del_width = if del_cell.flags_u16() & FLAG_WIDE_CHAR_SPACER != 0 && ccol > 0
                    {
                        ccol -= 1;
                        2u16
                    } else if del_cell.flags_u16() & FLAG_WIDE_CHAR != 0 {
                        2
                    } else {
                        1
                    };

                    let epoch = overlay.prediction_epoch;
                    let now = Instant::now();
                    let orow = overlay.get_or_make_row(row_idx);

                    // Shift cells left by del_width.
                    for col in ccol..cols.saturating_sub(del_width) {
                        let src = (col + del_width) as usize;
                        let src_cell = if orow.cells[src].active {
                            orow.cells[src].replacement
                        } else {
                            let idx = (row_idx as usize) * (cols as usize) + src;
                            grid.viewport.get(idx).copied().unwrap_or_default()
                        };
                        let mut shifted = src_cell;
                        let f = shifted.flags_u16() & !(FLAG_WIDE_CHAR | FLAG_WIDE_CHAR_SPACER);
                        shifted.flags = f.to_le_bytes();

                        let orig_idx = (row_idx as usize) * (cols as usize) + (col as usize);
                        let orig = grid.viewport.get(orig_idx).map(|c| c.ch()).unwrap_or('\0');

                        let dc = &mut orow.cells[col as usize];
                        dc.active = true;
                        dc.replacement = shifted;
                        dc.epoch = epoch;
                        dc.created_at = now;
                        dc.min_echo_ack = min_ack;
                        dc.original_ch = orig;
                        dc.unknown = false;
                        dc.tolerate_mismatch = force_track;
                    }

                    for trail in 0..del_width {
                        let tc = (cols - 1 - trail) as usize;
                        let orig_idx = (row_idx as usize) * (cols as usize) + tc;
                        let orig = grid.viewport.get(orig_idx).map(|c| c.ch()).unwrap_or('\0');
                        let mut blank = PackedCell::default();
                        blank.set_ch(' ');
                        if show_ul {
                            blank.flags = (blank.flags_u16() | FLAG_UNDERLINE).to_le_bytes();
                        }
                        let dc = &mut orow.cells[tc];
                        dc.active = true;
                        dc.replacement = blank;
                        dc.epoch = epoch;
                        dc.created_at = now;
                        dc.min_echo_ack = min_ack;
                        dc.original_ch = orig;
                        dc.unknown = false;
                        dc.tolerate_mismatch = force_track;
                    }
                }
                0x0D => {
                    ccol = 0;
                    overlay.local_edit_start = None;
                    overlay.increment_epoch();
                }
                0x0A => {
                    if crow < grid.rows as i16 - 1 {
                        crow += 1;
                    } else {
                        overlay.increment_epoch();
                        return;
                    }
                }
                0x1B => {
                    overlay.local_edit_start = None;
                    overlay.increment_epoch();
                    return;
                }
                _ => {
                    overlay.local_edit_start = None;
                    overlay.increment_epoch();
                    return;
                }
            }
        }

        if ccol < cols {
            overlay.cursor = Some(PredictedCursor {
                row: crow,
                col: ccol,
                epoch: overlay.prediction_epoch,
                min_echo_ack: min_ack,
                created_at: Instant::now(),
                tolerate_mismatch: force_track,
            });
        }
    }
}
