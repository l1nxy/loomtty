use ciri_protocol::message::*;
use std::time::Instant;
use unicode_width::UnicodeWidthChar;

use super::PredictionEngine;
use super::overlay::{PaneOverlay, PredictedCursor};
use crate::grid::ClientPaneGrid;

impl PredictionEngine {
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
        }

        char_width
    }

    /// Process user keyboard input and generate predictions.
    pub fn new_user_input(&mut self, pane_id: u64, data: &[u8], grid: &ClientPaneGrid) {
        if self.mode == ciri_config::config::PredictionMode::Never {
            return;
        }
        if grid.cols == 0 || grid.rows == 0 {
            return;
        }
        if grid.mode_flags & MODE_ALT_SCREEN != 0
            || grid.mode_flags & MODE_MOUSE_REPORT != 0
            || grid.mode_flags & MODE_BRACKETED_PASTE != 0
        {
            return;
        }

        let min_ack = self.next_input_seq;
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
                        let w = Self::predict_char(overlay, grid, ch, crow, ccol, min_ack, show_ul);
                        if w == 0 {
                            return;
                        }
                        ccol += w;
                    }
                }
                0x08 | 0x7F => {
                    if ccol == 0 {
                        overlay.increment_epoch();
                        continue;
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
                    }
                }
                0x0D => {
                    ccol = 0;
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
                    overlay.increment_epoch();
                    return;
                }
                _ => {
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
            });
        }
    }
}
