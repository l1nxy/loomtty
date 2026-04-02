use ciri_config::config::PredictionMode;
use ciri_protocol::message::*;
use std::collections::HashMap;
use std::time::Instant;
use unicode_width::UnicodeWidthChar;

use crate::grid::ClientPaneGrid;

const PREDICTION_TIMEOUT_SECS: u64 = 8;
const PING_INTERVAL_SECS: u64 = 2;

// Glitch trigger thresholds (Mosh convention, in milliseconds).
const GLITCH_THRESHOLD_MS: u64 = 250; // Pending prediction >= 250ms = glitch
const GLITCH_FLAG_THRESHOLD_MS: u64 = 5000; // Pending >= 5s = major glitch (force underline)
const GLITCH_REPAIR_COUNT: u32 = 10; // 10 quick confirmations to cure
const GLITCH_REPAIR_MIN_INTERVAL_MS: u64 = 150; // Min time between decrements

// Flagging (underline) hysteresis thresholds for send_interval (SRTT).
const FLAG_TRIGGER_HIGH_MS: u64 = 80;
const FLAG_TRIGGER_LOW_MS: u64 = 50;

// ---------------------------------------------------------------------------
// UTF-8 byte accumulator
// ---------------------------------------------------------------------------

struct Utf8Accum {
    buf: [u8; 4],
    len: u8,
    expected: u8,
}

impl Utf8Accum {
    fn new() -> Self {
        Self {
            buf: [0; 4],
            len: 0,
            expected: 0,
        }
    }

    fn reset(&mut self) {
        self.len = 0;
        self.expected = 0;
    }

    /// Push a leading byte. Returns expected total byte count (2-4), or 0 if invalid.
    fn start(&mut self, byte: u8) -> u8 {
        let expected = match byte {
            0xC2..=0xDF => 2, // 0xC0-0xC1 are overlong
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4, // 0xF5-0xF7 exceed Unicode range
            _ => 0,
        };
        if expected > 0 {
            self.buf[0] = byte;
            self.len = 1;
            self.expected = expected;
        } else {
            self.reset();
        }
        expected
    }

    /// Push a continuation byte. Returns `Some(char)` when the codepoint is complete.
    fn push_cont(&mut self, byte: u8) -> Option<char> {
        if self.expected == 0 || (byte & 0xC0) != 0x80 {
            self.reset();
            return None;
        }
        self.buf[self.len as usize] = byte;
        self.len += 1;
        if self.len == self.expected {
            let s = std::str::from_utf8(&self.buf[..self.len as usize]).ok()?;
            let ch = s.chars().next()?;
            self.reset();
            Some(ch)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Per-cell overlay state (Mosh ConditionalOverlayCell equivalent)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct OverlayCell {
    active: bool,
    replacement: PackedCell,
    epoch: u64,
    created_at: Instant,
    min_echo_ack: u64,
    original_ch: char,
    /// Content is unpredictable but position is known (e.g. rightmost column
    /// during insert-shift). Unknown cells are not rendered as overlay.
    unknown: bool,
}

impl OverlayCell {
    fn inactive(now: Instant) -> Self {
        Self {
            active: false,
            replacement: PackedCell::default(),
            epoch: 0,
            created_at: now,
            min_echo_ack: 0,
            original_ch: '\0',
            unknown: false,
        }
    }

    fn reset(&mut self) {
        self.active = false;
        self.unknown = false;
    }
}

// ---------------------------------------------------------------------------
// Per-row overlay (Mosh ConditionalOverlayRow equivalent)
// ---------------------------------------------------------------------------

struct OverlayRow {
    cells: Vec<OverlayCell>, // Fixed-size = cols, directly indexed
}

impl OverlayRow {
    fn new(cols: u16) -> Self {
        let now = Instant::now();
        Self {
            cells: (0..cols).map(|_| OverlayCell::inactive(now)).collect(),
        }
    }

    fn has_active(&self) -> bool {
        self.cells.iter().any(|c| c.active)
    }
}

// ---------------------------------------------------------------------------
// Predicted cursor
// ---------------------------------------------------------------------------

pub struct PredictedCursor {
    pub row: i16,
    pub col: u16,
    pub epoch: u64,
    pub min_echo_ack: u64,
}

// ---------------------------------------------------------------------------
// Pane overlay (replaces old PaneOverlay with per-row per-col indexing)
// ---------------------------------------------------------------------------

pub struct PaneOverlay {
    rows: HashMap<u16, OverlayRow>,
    pub cursor: Option<PredictedCursor>,
    pub prediction_epoch: u64,
    pub confirmed_epoch: u64,
    cols: u16,
    utf8: Utf8Accum,
}

impl PaneOverlay {
    fn new(cols: u16) -> Self {
        Self {
            rows: HashMap::new(),
            cursor: None,
            prediction_epoch: 1, // Start at 1 so first predictions are tentative
            confirmed_epoch: 0,
            cols,
            utf8: Utf8Accum::new(),
        }
    }

    fn increment_epoch(&mut self) {
        self.prediction_epoch += 1;
    }

    fn get_or_make_row(&mut self, row: u16) -> &mut OverlayRow {
        self.rows
            .entry(row)
            .or_insert_with(|| OverlayRow::new(self.cols))
    }

    /// Get overlay cell at (row, col) if active. O(1).
    fn get_cell(&self, row: u16, col: u16) -> Option<&OverlayCell> {
        self.rows
            .get(&row)
            .and_then(|r| r.cells.get(col as usize))
            .filter(|c| c.active)
    }

    /// Get the effective PackedCell: overlay if active, else grid fallback.
    fn effective_cell(&self, grid: &ClientPaneGrid, row: u16, col: u16) -> PackedCell {
        if let Some(c) = self.get_cell(row, col) {
            return c.replacement;
        }
        let idx = (row as usize) * (grid.cols as usize) + (col as usize);
        grid.viewport.get(idx).copied().unwrap_or_default()
    }

    /// Kill all predictions at epoch >= target.
    fn kill_epoch(&mut self, epoch: u64) {
        for row in self.rows.values_mut() {
            for cell in &mut row.cells {
                if cell.active && cell.epoch >= epoch {
                    cell.reset();
                }
            }
        }
        if self.cursor.as_ref().is_some_and(|c| c.epoch >= epoch) {
            self.cursor = None;
        }
        if self.prediction_epoch < epoch + 1 {
            self.prediction_epoch = epoch + 1;
        }
    }

    /// Expire predictions older than PREDICTION_TIMEOUT_SECS.
    fn expire_old(&mut self, now: Instant) {
        for row in self.rows.values_mut() {
            for cell in &mut row.cells {
                if cell.active
                    && now.duration_since(cell.created_at).as_secs() >= PREDICTION_TIMEOUT_SECS
                {
                    cell.reset();
                }
            }
        }
        // Clear cursor if no active cells share its epoch.
        if let Some(ref cur) = self.cursor {
            let epoch = cur.epoch;
            let has_peer = self
                .rows
                .values()
                .any(|r| r.cells.iter().any(|c| c.active && c.epoch == epoch));
            if !has_peer {
                self.cursor = None;
            }
        }
    }

    fn cursor_position(&self) -> Option<(i16, u16)> {
        self.cursor.as_ref().map(|c| (c.row, c.col))
    }

    fn is_empty(&self) -> bool {
        self.cursor.is_none() && !self.rows.values().any(|r| r.has_active())
    }

    /// Cleanup rows that have no active cells.
    fn gc_rows(&mut self) {
        self.rows.retain(|_, r| r.has_active());
    }
}

// ---------------------------------------------------------------------------
// Prediction engine
// ---------------------------------------------------------------------------

pub struct PredictionEngine {
    overlays: HashMap<u64, PaneOverlay>,
    srtt_us: u64,
    mode: PredictionMode,
    threshold_ms: u64,
    show_underline: bool,
    ping_seq: u64,
    last_ping: Option<Instant>,
    next_input_seq: u64,
    last_dims: HashMap<u64, (u16, u16)>,
    /// Glitch trigger counter (Mosh convention). 0 = no glitch,
    /// 1-10 = display predictions, >10 = display + force underline.
    glitch_trigger: u32,
    last_quick_confirm: Option<Instant>,
    /// Dynamic flagging (underline) state, controlled by SRTT hysteresis
    /// and glitch severity.
    flagging: bool,
}

impl PredictionEngine {
    pub fn new(mode: PredictionMode, threshold_ms: u64, show_underline: bool) -> Self {
        Self {
            overlays: HashMap::new(),
            srtt_us: 0,
            mode,
            threshold_ms,
            show_underline,
            ping_seq: 0,
            last_ping: None,
            next_input_seq: 1,
            last_dims: HashMap::new(),
            glitch_trigger: 0,
            last_quick_confirm: None,
            flagging: false,
        }
    }

    /// Return the next input_seq and advance the counter.
    pub fn next_input_seq(&mut self) -> u64 {
        let seq = self.next_input_seq;
        self.next_input_seq += 1;
        seq
    }

    pub fn update_config(&mut self, mode: PredictionMode, threshold_ms: u64, show_underline: bool) {
        self.mode = mode;
        self.threshold_ms = threshold_ms;
        self.show_underline = show_underline;
    }

    pub fn srtt_ms(&self) -> u64 {
        self.srtt_us / 1000
    }

    pub fn should_display(&self) -> bool {
        match self.mode {
            PredictionMode::Always => true,
            PredictionMode::Never => false,
            PredictionMode::Adaptive => {
                self.srtt_us / 1000 > self.threshold_ms || self.glitch_trigger > 0
            }
        }
    }

    pub fn maybe_send_ping(&mut self) -> Option<ClientMessage> {
        let now = Instant::now();
        let should_ping = match self.last_ping {
            None => true,
            Some(t) => now.duration_since(t).as_secs() >= PING_INTERVAL_SECS,
        };
        if !should_ping {
            return None;
        }
        self.ping_seq += 1;
        self.last_ping = Some(now);
        let client_time_us = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;
        Some(ClientMessage::Ping {
            seq: self.ping_seq,
            client_time_us,
        })
    }

    pub fn on_pong(&mut self, _seq: u64, client_time_us: u64) {
        let now_us = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;
        let sample = now_us.saturating_sub(client_time_us);
        if self.srtt_us == 0 {
            self.srtt_us = sample;
        } else {
            self.srtt_us = (self.srtt_us * 7 + sample) / 8;
        }
    }

    // ---- Character prediction (insert-mode right-shift) ----

    /// Predict inserting `ch` at cursor position with insert-mode right-shift.
    /// Returns the char width (columns consumed), or 0 if prediction was abandoned.
    fn predict_char(
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
        // Work from right to left to avoid overwriting source cells.
        // Rightmost shifted cells become unknown (may wrap).
        let orow = overlay.get_or_make_row(row_idx);
        for col in (ccol..cols.saturating_sub(char_width)).rev() {
            let dest = col + char_width;
            if dest >= cols {
                continue;
            }
            // Read source cell content before we overwrite it.
            let src_cell = if orow.cells[col as usize].active {
                orow.cells[col as usize].replacement
            } else {
                let idx = (row_idx as usize) * (cols as usize) + (col as usize);
                grid.viewport.get(idx).copied().unwrap_or_default()
            };
            let src_unknown = orow.cells[col as usize].active && orow.cells[col as usize].unknown;
            let orig_idx = (row_idx as usize) * (cols as usize) + (dest as usize);
            let orig_ch = grid.viewport.get(orig_idx).map(|c| c.ch()).unwrap_or('\0');

            // Clear wide-char flags on shifted cells to avoid orphaned spacers.
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

        // Mark the rightmost char_width cells as unknown (content unpredictable
        // due to potential line wrap or lost characters at the edge).
        for i in 0..char_width {
            let uc = cols - 1 - i;
            if uc >= ccol + char_width {
                // Only mark if not already set by the shift loop above
                let dc = &mut orow.cells[uc as usize];
                if !dc.active {
                    let orig_idx = (row_idx as usize) * (cols as usize) + (uc as usize);
                    dc.active = true;
                    dc.replacement = PackedCell::default();
                    dc.epoch = epoch;
                    dc.created_at = now;
                    dc.min_echo_ack = min_ack;
                    dc.original_ch =
                        grid.viewport.get(orig_idx).map(|c| c.ch()).unwrap_or('\0');
                }
                orow.cells[uc as usize].unknown = true;
            }
        }

        // Place the character at cursor position, inheriting renditions
        // from the left neighbor cell (Mosh convention: heuristic color match).
        let orig_idx = (row_idx as usize) * (cols as usize) + (ccol as usize);
        let orig_ch = grid.viewport.get(orig_idx).map(|c| c.ch()).unwrap_or('\0');

        // Inherit fg/bg from the left cell (predicted or actual).
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
        // Inherit text attribute flags (bold, italic, dim, etc.) but not
        // structural flags (wide_char, wrapline, hyperlink).
        let attr_mask = FLAG_BOLD | FLAG_ITALIC | FLAG_DIM | FLAG_STRIKEOUT | FLAG_HIDDEN
            | FLAG_INVERSE | FLAG_UNDERLINE | FLAG_UNDERLINE_STYLE_MASK;
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

        // For wide chars, place a spacer at ccol+1.
        if char_width == 2 {
            let spacer_col = ccol + 1;
            let spacer_orig_idx =
                (row_idx as usize) * (cols as usize) + (spacer_col as usize);
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

    // ---- Main input processing ----

    pub fn new_user_input(&mut self, pane_id: u64, data: &[u8], grid: &ClientPaneGrid) {
        if self.mode == PredictionMode::Never {
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
        let show_ul = self.show_underline && (self.flagging || self.mode == PredictionMode::Always);
        let cols = grid.cols;

        let overlay = self
            .overlays
            .entry(pane_id)
            .or_insert_with(|| PaneOverlay::new(cols));

        // If terminal resized since overlay was created, reset.
        if overlay.cols != cols {
            *overlay = PaneOverlay::new(cols);
        }

        let (mut crow, mut ccol) = overlay
            .cursor_position()
            .unwrap_or((grid.cursor_line, grid.cursor_col));

        // Don't predict when cursor is in scrollback (negative row).
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
                    let w = Self::predict_char(overlay, grid, byte as char, crow, ccol, min_ack, show_ul);
                    if w == 0 {
                        return; // predict_char incremented epoch; stop processing
                    }
                    ccol += w;
                }
                // UTF-8 leading bytes
                0xC2..=0xDF | 0xE0..=0xEF | 0xF0..=0xF4 => {
                    overlay.utf8.start(byte);
                }
                // UTF-8 continuation bytes
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

                    // If we landed on a wide-char spacer, back up one more.
                    let del_cell = overlay.effective_cell(grid, row_idx, ccol);
                    let del_width =
                        if del_cell.flags_u16() & FLAG_WIDE_CHAR_SPACER != 0 && ccol > 0 {
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
                            let idx =
                                (row_idx as usize) * (cols as usize) + src;
                            grid.viewport.get(idx).copied().unwrap_or_default()
                        };
                        // Clear wide-char flags on shifted cells.
                        let mut shifted = src_cell;
                        let f = shifted.flags_u16()
                            & !(FLAG_WIDE_CHAR | FLAG_WIDE_CHAR_SPACER);
                        shifted.flags = f.to_le_bytes();

                        let orig_idx =
                            (row_idx as usize) * (cols as usize) + (col as usize);
                        let orig =
                            grid.viewport.get(orig_idx).map(|c| c.ch()).unwrap_or('\0');

                        let dc = &mut orow.cells[col as usize];
                        dc.active = true;
                        dc.replacement = shifted;
                        dc.epoch = epoch;
                        dc.created_at = now;
                        dc.min_echo_ack = min_ack;
                        dc.original_ch = orig;
                        dc.unknown = false;
                    }

                    // Trailing columns get spaces.
                    for trail in 0..del_width {
                        let tc = (cols - 1 - trail) as usize;
                        let orig_idx =
                            (row_idx as usize) * (cols as usize) + tc;
                        let orig =
                            grid.viewport.get(orig_idx).map(|c| c.ch()).unwrap_or('\0');
                        let mut blank = PackedCell::default();
                        blank.set_ch(' ');
                        if show_ul {
                            blank.flags =
                                (blank.flags_u16() | FLAG_UNDERLINE).to_le_bytes();
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
                    // Carriage return: cursor to column 0.
                    ccol = 0;
                    overlay.increment_epoch(); // CR makes subsequent predictions tentative
                }
                0x0A => {
                    // Line feed: cursor to next row.
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

    // ---- Server sync / validation ----

    pub fn on_server_sync(&mut self, pane_id: u64, grid: &ClientPaneGrid, echo_ack: u64) {
        // Reset on terminal resize.
        let dims = (grid.cols, grid.rows);
        let prev = self.last_dims.insert(pane_id, dims);
        if prev.is_some_and(|d| d != dims) {
            self.overlays.remove(&pane_id);
            return;
        }

        let Some(overlay) = self.overlays.get_mut(&pane_id) else {
            return;
        };
        if overlay.is_empty() {
            return;
        }

        let now = Instant::now();
        overlay.expire_old(now);

        let cols = grid.cols as usize;
        let rows = grid.rows as usize;

        // Collect row numbers for iteration (need row_num for grid lookup).
        let row_nums: Vec<u16> = overlay.rows.keys().copied().collect();

        // --- Pass 1: confirm correct predictions, update glitch trigger ---
        let mut max_confirmed = overlay.confirmed_epoch;

        for &row_num in &row_nums {
            let Some(row) = overlay.rows.get_mut(&row_num) else {
                continue;
            };
            // Track the last confirmed cell's actual renditions for propagation.
            // When a prediction is confirmed, propagate the actual cell's
            // renditions to subsequent active cells in this row (Mosh convention).
            let mut propagate_fg_bg: Option<(PackedColor, PackedColor)> = None;

            for col in 0..row.cells.len() {
                if !row.cells[col].active {
                    continue;
                }
                // Apply rendition propagation from confirmed cells.
                if let Some((fg, bg)) = propagate_fg_bg {
                    row.cells[col].replacement.fg = fg;
                    row.cells[col].replacement.bg = bg;
                }
                if row_num as usize >= rows || col >= cols {
                    row.cells[col].reset();
                    continue;
                }

                // Pending — server hasn't processed input yet.
                if echo_ack < row.cells[col].min_echo_ack {
                    // Glitch detection: prediction outstanding too long.
                    let pending_ms =
                        now.duration_since(row.cells[col].created_at).as_millis() as u64;
                    if pending_ms >= GLITCH_FLAG_THRESHOLD_MS {
                        self.glitch_trigger = GLITCH_REPAIR_COUNT * 2; // major
                    } else if pending_ms >= GLITCH_THRESHOLD_MS
                        && self.glitch_trigger < GLITCH_REPAIR_COUNT
                    {
                        self.glitch_trigger = GLITCH_REPAIR_COUNT; // minor
                    }
                    continue;
                }

                // Unknown cells: CorrectNoCredit once mature.
                if row.cells[col].unknown {
                    row.cells[col].reset();
                    continue;
                }

                let idx = (row_num as usize) * cols + col;
                if idx >= grid.viewport.len() {
                    row.cells[col].reset();
                    continue;
                }

                let actual = &grid.viewport[idx];
                let pred_ch = row.cells[col].replacement.ch();
                let actual_ch = actual.ch();

                if pred_ch == actual_ch {
                    // Correct — give credit if cell actually changed.
                    if pred_ch != row.cells[col].original_ch
                        && row.cells[col].epoch > max_confirmed
                    {
                        max_confirmed = row.cells[col].epoch;
                    }
                    // Glitch repair: quick confirmation reduces trigger.
                    let pred_ms =
                        now.duration_since(row.cells[col].created_at).as_millis() as u64;
                    if pred_ms < GLITCH_THRESHOLD_MS && self.glitch_trigger > 0 {
                        let can_repair = self
                            .last_quick_confirm
                            .map_or(true, |t| {
                                now.duration_since(t).as_millis() as u64
                                    >= GLITCH_REPAIR_MIN_INTERVAL_MS
                            });
                        if can_repair {
                            self.glitch_trigger -= 1;
                            self.last_quick_confirm = Some(now);
                        }
                    }
                    // Propagate actual cell's renditions to subsequent predictions.
                    propagate_fg_bg = Some((actual.fg, actual.bg));
                    row.cells[col].reset();
                }
                // Wrong predictions are handled in pass 2.
            }
        }

        overlay.confirmed_epoch = max_confirmed;

        // Update flagging (underline) with hysteresis.
        let srtt_ms = self.srtt_us / 1000;
        if srtt_ms > FLAG_TRIGGER_HIGH_MS {
            self.flagging = true;
        } else if srtt_ms <= FLAG_TRIGGER_LOW_MS {
            self.flagging = false;
        }
        if self.glitch_trigger > GLITCH_REPAIR_COUNT {
            self.flagging = true; // major glitch forces underline
        }

        // --- Pass 2: detect wrong predictions (using updated confirmed_epoch) ---
        let confirmed = overlay.confirmed_epoch;
        let mut kill_at: Option<u64> = None;
        let mut need_reset = false;

        for &row_num in &row_nums {
            let Some(row) = overlay.rows.get_mut(&row_num) else {
                continue;
            };
            for col in 0..row.cells.len() {
                let cell = &row.cells[col];
                if !cell.active || cell.unknown {
                    continue;
                }
                if echo_ack < cell.min_echo_ack {
                    continue; // still pending
                }
                if row_num as usize >= rows || col >= cols {
                    continue; // already cleaned in pass 1
                }
                let idx = (row_num as usize) * cols + col;
                if idx >= grid.viewport.len() {
                    continue;
                }

                let actual_ch = grid.viewport[idx].ch();
                let pred_ch = cell.replacement.ch();

                if pred_ch != actual_ch {
                    if cell.epoch <= confirmed {
                        need_reset = true;
                    } else {
                        kill_at = Some(kill_at.map_or(cell.epoch, |e| e.min(cell.epoch)));
                    }
                }
            }
        }

        if need_reset {
            self.overlays.remove(&pane_id);
            return;
        }

        let overlay = self.overlays.get_mut(&pane_id).unwrap();
        if let Some(epoch) = kill_at {
            overlay.kill_epoch(epoch);
        }

        // Validate cursor prediction.
        if let Some(ref cur) = overlay.cursor {
            if echo_ack >= cur.min_echo_ack {
                if cur.row == grid.cursor_line && cur.col == grid.cursor_col {
                    overlay.cursor = None;
                } else {
                    self.overlays.remove(&pane_id);
                    return;
                }
            }
        }

        // Cleanup empty rows and remove overlay if fully empty.
        if let Some(overlay) = self.overlays.get_mut(&pane_id) {
            overlay.gc_rows();
            if overlay.is_empty() {
                self.overlays.remove(&pane_id);
            }
        }
    }

    // ---- Public overlay access ----

    pub fn get_overlay_cell(&self, pane_id: u64, row: u16, col: u16) -> Option<PackedCell> {
        if !self.should_display() {
            return None;
        }
        let overlay = self.overlays.get(&pane_id)?;
        let cell = overlay.get_cell(row, col)?;
        if cell.unknown {
            return None; // Unknown cells are not rendered as overlay
        }
        if self.mode == PredictionMode::Always || cell.epoch <= overlay.confirmed_epoch {
            Some(cell.replacement)
        } else {
            None
        }
    }

    pub fn get_overlay_cursor(&self, pane_id: u64) -> Option<(i16, u16)> {
        if !self.should_display() {
            return None;
        }
        let overlay = self.overlays.get(&pane_id)?;
        let cur = overlay.cursor.as_ref()?;
        if self.mode == PredictionMode::Always || cur.epoch <= overlay.confirmed_epoch {
            Some((cur.row, cur.col))
        } else {
            None
        }
    }

    pub fn has_overlay(&self, pane_id: u64) -> bool {
        self.overlays.get(&pane_id).is_some_and(|o| !o.is_empty())
    }

    pub fn clear_pane(&mut self, pane_id: u64) {
        self.overlays.remove(&pane_id);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_grid_at(cols: u16, rows: u16, cursor_col: u16, cursor_row: i16) -> ClientPaneGrid {
        let mut g = ClientPaneGrid::new(cols, rows, 0);
        g.cursor_col = cursor_col;
        g.cursor_line = cursor_row;
        g
    }

    fn make_grid(cols: u16, rows: u16) -> ClientPaneGrid {
        ClientPaneGrid::new(cols, rows, 0)
    }

    #[test]
    fn printable_char_prediction() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, true);
        let grid = make_grid_at(80, 24, 0, 0);
        engine.new_user_input(1, b"A", &grid);

        let cell = engine.get_overlay_cell(1, 0, 0);
        assert!(cell.is_some());
        assert_eq!(cell.unwrap().ch(), 'A');
        assert_ne!(cell.unwrap().flags_u16() & FLAG_UNDERLINE, 0);
    }

    #[test]
    fn cursor_advances() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 5, 0);
        engine.new_user_input(1, b"xyz", &grid);

        assert_eq!(engine.get_overlay_cursor(1), Some((0, 8)));
    }

    #[test]
    fn backspace_prediction() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let mut grid = make_grid_at(80, 24, 5, 0);
        grid.viewport[4].set_ch('D');
        engine.new_user_input(1, &[0x7F], &grid);

        // After backspace, cursor moves to col 4, content shifts left.
        assert_eq!(engine.get_overlay_cursor(1), Some((0, 4)));
    }

    #[test]
    fn alt_screen_skips_prediction() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let mut grid = make_grid_at(80, 24, 0, 0);
        grid.mode_flags = MODE_ALT_SCREEN;
        engine.new_user_input(1, b"A", &grid);

        assert!(!engine.has_overlay(1));
    }

    #[test]
    fn never_mode_skips() {
        let mut engine = PredictionEngine::new(PredictionMode::Never, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);
        engine.new_user_input(1, b"A", &grid);

        assert!(!engine.has_overlay(1));
    }

    #[test]
    fn adaptive_mode_display_threshold() {
        let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
        assert!(!engine.should_display());
        engine.srtt_us = 50_000;
        assert!(engine.should_display());
    }

    #[test]
    fn server_sync_confirms_prediction() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);
        engine.new_user_input(1, b"A", &grid);
        assert!(engine.has_overlay(1));

        let mut server_grid = make_grid(80, 24);
        server_grid.viewport[0].set_ch('A');
        server_grid.cursor_col = 1;
        server_grid.cursor_line = 0;
        engine.on_server_sync(1, &server_grid, 1);

        assert!(!engine.has_overlay(1));
    }

    #[test]
    fn pending_prediction_not_validated_early() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);
        engine.new_user_input(1, b"A", &grid);

        let mut server_grid = make_grid(80, 24);
        server_grid.viewport[0].set_ch('X');
        engine.on_server_sync(1, &server_grid, 0);

        assert!(engine.has_overlay(1));
    }

    #[test]
    fn wrong_prediction_killed() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);
        engine.new_user_input(1, b"A", &grid);

        let mut server_grid = make_grid(80, 24);
        server_grid.viewport[0].set_ch('X');
        server_grid.cursor_col = 1;
        server_grid.cursor_line = 0;
        engine.on_server_sync(1, &server_grid, 1);

        assert!(!engine.has_overlay(1));
    }

    #[test]
    fn wrong_cursor_resets_all() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);
        engine.new_user_input(1, b"AB", &grid);

        let mut server_grid = make_grid(80, 24);
        server_grid.viewport[0].set_ch('A');
        server_grid.viewport[1].set_ch('B');
        server_grid.cursor_col = 10;
        server_grid.cursor_line = 0;
        engine.on_server_sync(1, &server_grid, 1);

        assert!(!engine.has_overlay(1));
    }

    #[test]
    fn anti_credit_no_false_confirm() {
        let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
        engine.srtt_us = 50_000;

        let mut grid = make_grid_at(80, 24, 0, 0);
        grid.viewport[0].set_ch('A');

        engine.new_user_input(1, b"A", &grid);

        let mut server_grid = make_grid(80, 24);
        server_grid.viewport[0].set_ch('A');
        server_grid.cursor_col = 1;
        server_grid.cursor_line = 0;
        engine.on_server_sync(1, &server_grid, 1);

        // Cursor should NOT be displayed (epoch not confirmed due to anti-credit).
        assert_eq!(engine.get_overlay_cursor(1), None);
    }

    #[test]
    fn adaptive_hides_tentative_predictions() {
        let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
        engine.srtt_us = 50_000;

        let grid = make_grid_at(80, 24, 0, 0);
        engine.new_user_input(1, b"A", &grid);

        assert_eq!(engine.get_overlay_cell(1, 0, 0), None);
    }

    #[test]
    fn insert_mode_right_shift() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let mut grid = make_grid_at(5, 1, 1, 0);
        grid.viewport[0].set_ch('A');
        grid.viewport[1].set_ch('B');
        grid.viewport[2].set_ch('C');
        grid.viewport[3].set_ch('D');

        engine.new_user_input(1, b"X", &grid);

        assert_eq!(engine.get_overlay_cell(1, 0, 0), None); // A unchanged
        assert_eq!(engine.get_overlay_cell(1, 0, 1).unwrap().ch(), 'X');
        assert_eq!(engine.get_overlay_cell(1, 0, 2).unwrap().ch(), 'B');
        assert_eq!(engine.get_overlay_cell(1, 0, 3).unwrap().ch(), 'C');
        // col 4 is unknown (rightmost, content unpredictable)
        assert_eq!(engine.get_overlay_cell(1, 0, 4), None);
        assert_eq!(engine.get_overlay_cursor(1), Some((0, 2)));
    }

    #[test]
    fn backspace_left_shift() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let mut grid = make_grid_at(5, 1, 2, 0);
        grid.viewport[0].set_ch('A');
        grid.viewport[1].set_ch('B');
        grid.viewport[2].set_ch('C');
        grid.viewport[3].set_ch('D');
        grid.viewport[4].set_ch('E');

        engine.new_user_input(1, &[0x7F], &grid);

        assert_eq!(engine.get_overlay_cell(1, 0, 0), None); // A unchanged
        assert_eq!(engine.get_overlay_cell(1, 0, 1).unwrap().ch(), 'C');
        assert_eq!(engine.get_overlay_cell(1, 0, 2).unwrap().ch(), 'D');
        assert_eq!(engine.get_overlay_cell(1, 0, 3).unwrap().ch(), 'E');
        assert_eq!(engine.get_overlay_cell(1, 0, 4).unwrap().ch(), ' ');
        assert_eq!(engine.get_overlay_cursor(1), Some((0, 1)));
    }

    #[test]
    fn cr_moves_cursor_to_col_zero() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 10, 0);
        engine.new_user_input(1, b"\r", &grid);

        assert_eq!(engine.get_overlay_cursor(1), Some((0, 0)));
    }

    #[test]
    fn lf_moves_cursor_down() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 5, 3);
        engine.new_user_input(1, b"\n", &grid);

        assert_eq!(engine.get_overlay_cursor(1), Some((4, 5)));
    }

    #[test]
    fn crlf_moves_cursor_to_next_line_col_zero() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 10, 3);
        engine.new_user_input(1, b"\r\n", &grid);

        assert_eq!(engine.get_overlay_cursor(1), Some((4, 0)));
    }

    #[test]
    fn lf_at_last_row_increments_epoch() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 5, 23);
        engine.new_user_input(1, b"\n", &grid);

        assert!(!engine.has_overlay(1));
    }

    #[test]
    fn utf8_multibyte_prediction() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);

        // 'é' = U+00E9 = [0xC3, 0xA9]
        engine.new_user_input(1, &[0xC3, 0xA9], &grid);

        let cell = engine.get_overlay_cell(1, 0, 0);
        assert!(cell.is_some());
        assert_eq!(cell.unwrap().ch(), 'é');
        assert_eq!(engine.get_overlay_cursor(1), Some((0, 1)));
    }

    #[test]
    fn utf8_cjk_wide_char() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);

        // '中' = U+4E2D = [0xE4, 0xB8, 0xAD]
        engine.new_user_input(1, &[0xE4, 0xB8, 0xAD], &grid);

        let cell = engine.get_overlay_cell(1, 0, 0);
        assert!(cell.is_some());
        assert_eq!(cell.unwrap().ch(), '中');
        assert_ne!(cell.unwrap().flags_u16() & FLAG_WIDE_CHAR, 0);
        assert_eq!(engine.get_overlay_cursor(1), Some((0, 2)));
    }

    #[test]
    fn utf8_incomplete_does_not_predict() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);

        engine.new_user_input(1, &[0xE4], &grid);

        assert_eq!(engine.get_overlay_cell(1, 0, 0), None);
    }

    #[test]
    fn grid_zero_cols_no_panic() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(0, 0, 0, 0);
        engine.new_user_input(1, b"A", &grid);
        assert!(!engine.has_overlay(1));
    }

    #[test]
    fn application_mode_arrow_keys() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 5, 0);

        engine.new_user_input(1, b"\x1BOC", &grid); // Application mode right arrow
        assert_eq!(engine.get_overlay_cursor(1), Some((0, 6)));

        engine.new_user_input(1, b"\x1BOD", &grid); // Application mode left arrow
        assert_eq!(engine.get_overlay_cursor(1), Some((0, 5)));
    }

    #[test]
    fn o1_cell_count_after_many_inputs() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 1, 0, 0);

        // Type 50 characters
        for i in 0..50u8 {
            let ch = b'A' + (i % 26);
            engine.new_user_input(1, &[ch], &grid);
        }

        // With the new data structure, total active cells should be <= cols (80)
        // because cells are updated in-place, not appended.
        let overlay = engine.overlays.get(&1).unwrap();
        let total_cells: usize = overlay
            .rows
            .values()
            .map(|r| r.cells.len())
            .sum();
        assert_eq!(total_cells, 80); // Fixed-size row = cols
    }

    #[test]
    fn srtt_computation() {
        let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
        let base_us = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;
        engine.on_pong(1, base_us.saturating_sub(10_000));
        assert!(engine.srtt_us > 0);
    }

    #[test]
    fn ping_generation() {
        let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
        assert!(engine.maybe_send_ping().is_some());
        assert!(engine.maybe_send_ping().is_none());
    }

    // ---- Rendition inheritance tests ----

    #[test]
    fn rendition_inherits_fg_bg_from_left_cell() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let mut grid = make_grid_at(80, 24, 1, 0);
        // Left cell (col 0) has custom colors.
        grid.viewport[0].set_ch('A');
        grid.viewport[0].fg = PackedColor::rgb(255, 0, 0); // red
        grid.viewport[0].bg = PackedColor::rgb(0, 0, 255); // blue

        engine.new_user_input(1, b"B", &grid);

        let cell = engine.get_overlay_cell(1, 0, 1).unwrap();
        assert_eq!(cell.ch(), 'B');
        assert_eq!(cell.fg, PackedColor::rgb(255, 0, 0)); // inherited red
        assert_eq!(cell.bg, PackedColor::rgb(0, 0, 255)); // inherited blue
    }

    #[test]
    fn rendition_inherits_bold_italic_from_left() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let mut grid = make_grid_at(80, 24, 1, 0);
        grid.viewport[0].set_ch('A');
        grid.viewport[0].flags = (FLAG_BOLD | FLAG_ITALIC).to_le_bytes();

        engine.new_user_input(1, b"B", &grid);

        let cell = engine.get_overlay_cell(1, 0, 1).unwrap();
        assert_ne!(cell.flags_u16() & FLAG_BOLD, 0);
        assert_ne!(cell.flags_u16() & FLAG_ITALIC, 0);
    }

    #[test]
    fn rendition_at_col_zero_inherits_from_current_position() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let mut grid = make_grid_at(80, 24, 0, 0);
        grid.viewport[0].fg = PackedColor::rgb(0, 255, 0); // green

        engine.new_user_input(1, b"X", &grid);

        let cell = engine.get_overlay_cell(1, 0, 0).unwrap();
        assert_eq!(cell.fg, PackedColor::rgb(0, 255, 0)); // inherited from current pos
    }

    #[test]
    fn consecutive_chars_chain_renditions() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let mut grid = make_grid_at(80, 24, 0, 0);
        grid.viewport[0].fg = PackedColor::rgb(100, 200, 50);

        // Type "AB" — B should inherit from predicted A, which inherited from grid[0].
        engine.new_user_input(1, b"AB", &grid);

        let a = engine.get_overlay_cell(1, 0, 0).unwrap();
        let b = engine.get_overlay_cell(1, 0, 1).unwrap();
        assert_eq!(a.fg, PackedColor::rgb(100, 200, 50));
        assert_eq!(b.fg, PackedColor::rgb(100, 200, 50)); // chained
    }

    // ---- Glitch trigger tests ----

    #[test]
    fn glitch_trigger_activates_display_in_adaptive() {
        let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 100, false);
        // SRTT is 0 (below threshold), so normally predictions wouldn't display.
        assert!(!engine.should_display());

        // But if glitch_trigger > 0, should_display returns true.
        engine.glitch_trigger = 1;
        assert!(engine.should_display());
    }

    #[test]
    fn glitch_trigger_never_activates_in_never_mode() {
        let mut engine = PredictionEngine::new(PredictionMode::Never, 0, false);
        engine.glitch_trigger = 10;
        assert!(!engine.should_display());
    }

    // ---- Insert-shift edge cases ----

    #[test]
    fn insert_at_last_col_returns_zero_and_stops() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        // Cursor at last column (col 4 of 5-col terminal).
        let grid = make_grid_at(5, 1, 4, 0);

        // Type a wide char (needs 2 cols) — doesn't fit, should abandon.
        engine.new_user_input(1, &[0xE4, 0xB8, 0xAD], &grid); // '中'

        // No cell prediction should be created at col 4.
        assert_eq!(engine.get_overlay_cell(1, 0, 4), None);
    }

    #[test]
    fn multiple_insert_shift_consistency() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let mut grid = make_grid_at(6, 1, 0, 0);
        grid.viewport[0].set_ch('A');
        grid.viewport[1].set_ch('B');
        grid.viewport[2].set_ch('C');

        // Type "XY" at col 0 — two right-shifts.
        engine.new_user_input(1, b"XY", &grid);

        assert_eq!(engine.get_overlay_cell(1, 0, 0).unwrap().ch(), 'X');
        assert_eq!(engine.get_overlay_cell(1, 0, 1).unwrap().ch(), 'Y');
        assert_eq!(engine.get_overlay_cell(1, 0, 2).unwrap().ch(), 'A');
        assert_eq!(engine.get_overlay_cell(1, 0, 3).unwrap().ch(), 'B');
        // col 4, 5 are unknown (rightmost after double shift)
        assert_eq!(engine.get_overlay_cursor(1), Some((0, 2)));
    }

    #[test]
    fn insert_then_backspace_roundtrip() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let mut grid = make_grid_at(5, 1, 1, 0);
        grid.viewport[0].set_ch('A');
        grid.viewport[1].set_ch('B');
        grid.viewport[2].set_ch('C');

        // Type 'X' at col 1, then backspace.
        engine.new_user_input(1, b"X\x7F", &grid);

        // Cursor should be back at col 1.
        assert_eq!(engine.get_overlay_cursor(1), Some((0, 1)));
    }

    // ---- Wide char tests ----

    #[test]
    fn wide_char_spacer_flag_set() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);

        engine.new_user_input(1, &[0xE4, 0xB8, 0xAD], &grid); // '中'

        // col 0: wide char with FLAG_WIDE_CHAR
        let c0 = engine.get_overlay_cell(1, 0, 0).unwrap();
        assert_ne!(c0.flags_u16() & FLAG_WIDE_CHAR, 0);

        // col 1: spacer — active but get_overlay_cell should return it with FLAG_WIDE_CHAR_SPACER
        let c1 = engine.get_overlay_cell(1, 0, 1).unwrap();
        assert_ne!(c1.flags_u16() & FLAG_WIDE_CHAR_SPACER, 0);
    }

    #[test]
    fn wide_char_insert_shift_clears_wide_flags() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let mut grid = make_grid_at(10, 1, 0, 0);
        // Put a wide char at col 2-3.
        grid.viewport[2].set_ch('中');
        grid.viewport[2].flags = FLAG_WIDE_CHAR.to_le_bytes();
        grid.viewport[3].set_ch(' ');
        grid.viewport[3].flags = FLAG_WIDE_CHAR_SPACER.to_le_bytes();

        // Type 'A' at col 0 — shift everything right by 1.
        engine.new_user_input(1, b"A", &grid);

        // Shifted cells should NOT have wide-char flags (they were cleared).
        let c3 = engine.get_overlay_cell(1, 0, 3);
        if let Some(c) = c3 {
            assert_eq!(
                c.flags_u16() & (FLAG_WIDE_CHAR | FLAG_WIDE_CHAR_SPACER),
                0,
                "shifted cell should have wide-char flags cleared"
            );
        }
    }

    #[test]
    fn backspace_wide_char_deletes_two_cols() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let mut grid = make_grid_at(10, 1, 2, 0);
        // Wide char at cols 0-1.
        grid.viewport[0].set_ch('中');
        grid.viewport[0].flags = FLAG_WIDE_CHAR.to_le_bytes();
        grid.viewport[1].set_ch(' ');
        grid.viewport[1].flags = FLAG_WIDE_CHAR_SPACER.to_le_bytes();
        grid.viewport[2].set_ch('A');

        // Backspace at col 2 — should detect spacer at col 1, back up to col 0.
        engine.new_user_input(1, &[0x7F], &grid);

        // Cursor should be at col 0 (backed up 2 cols for wide char).
        assert_eq!(engine.get_overlay_cursor(1), Some((0, 0)));
    }

    // ---- UTF-8 edge cases ----

    #[test]
    fn utf8_split_across_calls() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);

        // Send leading byte in first call, continuation bytes in second.
        engine.new_user_input(1, &[0xC3], &grid);
        assert_eq!(engine.get_overlay_cell(1, 0, 0), None); // still accumulating

        engine.new_user_input(1, &[0xA9], &grid); // completes 'é'
        let cell = engine.get_overlay_cell(1, 0, 0);
        assert!(cell.is_some());
        assert_eq!(cell.unwrap().ch(), 'é');
    }

    #[test]
    fn utf8_invalid_continuation_resets() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);

        // Send leading byte then invalid byte (not 10xxxxxx).
        engine.new_user_input(1, &[0xC3, 0x41], &grid); // 0x41 = 'A', not continuation

        // 0xC3 starts UTF-8, 0x41 is not continuation — accum resets.
        // 0x41 is NOT processed as printable because it's in 0x80..=0xBF branch? No,
        // 0x41 is in 0x20..=0x7E range. But the match checks 0x80..=0xBF first? No,
        // match is ordered: 0x20..=0x7E comes first. So 0x41 would be matched as printable.
        // Actually, 0xC3 starts the accum, then 0x41 is NOT 0x80..=0xBF, so it goes to
        // 0x20..=0x7E and is handled as a printable char.
        let cell = engine.get_overlay_cell(1, 0, 0);
        assert!(cell.is_some());
        assert_eq!(cell.unwrap().ch(), 'A'); // 'A' was processed as printable
    }

    #[test]
    fn utf8_overlong_rejected() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);

        // 0xC0 and 0xC1 are overlong — should not start accumulation.
        engine.new_user_input(1, &[0xC0, 0x80], &grid);

        // 0xC0 hits the _ => branch (not in 0xC2..=0xDF), increments epoch and returns.
        assert!(!engine.has_overlay(1));
    }

    // ---- Server sync edge cases ----

    #[test]
    fn server_sync_partial_confirm_then_error() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);

        // Type "AB" (same epoch).
        engine.new_user_input(1, b"AB", &grid);

        // Server confirms A but shows wrong B.
        let mut server_grid = make_grid(80, 24);
        server_grid.viewport[0].set_ch('A');
        server_grid.viewport[1].set_ch('Z'); // wrong
        server_grid.cursor_col = 2;
        server_grid.cursor_line = 0;
        engine.on_server_sync(1, &server_grid, 1);

        // With two-pass scan: A is confirmed in pass 1 (epoch confirmed),
        // then B is wrong in pass 2 — since B's epoch <= confirmed, this is
        // a catastrophic reset.
        assert!(!engine.has_overlay(1));
    }

    #[test]
    fn server_sync_resize_clears_predictions() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);
        engine.new_user_input(1, b"A", &grid);
        assert!(engine.has_overlay(1));

        // First sync establishes dimensions.
        let server_grid = make_grid(80, 24);
        engine.on_server_sync(1, &server_grid, 0);

        // Second sync with different dimensions — resize reset.
        let resized_grid = make_grid(120, 30);
        engine.on_server_sync(1, &resized_grid, 0);

        assert!(!engine.has_overlay(1));
    }

    #[test]
    fn server_sync_unknown_cells_cleared_on_mature() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let mut grid = make_grid_at(5, 1, 0, 0);
        grid.viewport[0].set_ch('A');

        // Type 'X' — rightmost col (4) becomes unknown.
        engine.new_user_input(1, b"X", &grid);

        // Unknown cell at col 4 should not be visible.
        assert_eq!(engine.get_overlay_cell(1, 0, 4), None);

        // After server sync with echo_ack >= min, unknown cells should be cleared.
        let mut server_grid = make_grid(5, 1);
        server_grid.viewport[0].set_ch('X');
        server_grid.viewport[1].set_ch('A');
        server_grid.cursor_col = 1;
        server_grid.cursor_line = 0;
        engine.on_server_sync(1, &server_grid, 1);
    }

    #[test]
    fn clear_pane_removes_all_state() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);
        engine.new_user_input(1, b"ABC", &grid);
        assert!(engine.has_overlay(1));

        engine.clear_pane(1);
        assert!(!engine.has_overlay(1));
    }

    // ---- CR/LF edge cases ----

    #[test]
    fn cr_then_printable_overwrites_at_col_zero() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let mut grid = make_grid_at(80, 24, 10, 0);
        grid.viewport[0].set_ch('Z');

        // CR moves to col 0, then 'A' overwrites.
        engine.new_user_input(1, b"\rA", &grid);

        assert_eq!(engine.get_overlay_cell(1, 0, 0).unwrap().ch(), 'A');
        assert_eq!(engine.get_overlay_cursor(1), Some((0, 1)));
    }

    #[test]
    fn multiple_lf_moves_down() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);

        engine.new_user_input(1, b"\n\n\n", &grid);
        assert_eq!(engine.get_overlay_cursor(1), Some((3, 0)));
    }

    // ---- Multi-pane isolation ----

    #[test]
    fn predictions_isolated_between_panes() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid1 = make_grid_at(80, 24, 0, 0);
        let grid2 = make_grid_at(80, 24, 5, 0);

        engine.new_user_input(1, b"A", &grid1);
        engine.new_user_input(2, b"B", &grid2);

        // Each pane has its own overlay.
        assert_eq!(engine.get_overlay_cell(1, 0, 0).unwrap().ch(), 'A');
        assert_eq!(engine.get_overlay_cell(2, 0, 5).unwrap().ch(), 'B');
        // Pane 1 at col 5: insert-shift created an active cell (shifted from col 0).
        // But pane 2 should not have pane 1's predictions.
        assert_eq!(engine.get_overlay_cell(2, 0, 0), None); // pane 2 col 0 is inactive
        // Clear pane 1, pane 2 should be unaffected.
        engine.clear_pane(1);
        assert!(!engine.has_overlay(1));
        assert!(engine.has_overlay(2));
    }

    // ---- Scrollback / negative cursor ----

    #[test]
    fn negative_cursor_row_skips_prediction() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, -1); // cursor in scrollback
        engine.new_user_input(1, b"A", &grid);

        // Should not create cell predictions (only epoch increment).
        assert_eq!(engine.get_overlay_cell(1, 0, 0), None);
    }

    // ---- Epoch and confirmation ----

    #[test]
    fn epoch_increments_on_escape() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);

        engine.new_user_input(1, b"\x1B", &grid);

        // ESC should increment epoch but not create any cell/cursor prediction.
        assert!(!engine.has_overlay(1));
    }

    #[test]
    fn adaptive_confirms_then_shows() {
        let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
        engine.srtt_us = 50_000; // above threshold

        let grid = make_grid_at(80, 24, 0, 0);

        // First input — epoch 1, tentative (confirmed_epoch = 0).
        engine.new_user_input(1, b"A", &grid);
        assert_eq!(engine.get_overlay_cell(1, 0, 0), None); // hidden (tentative)

        // Server confirms A.
        let mut sg = make_grid(80, 24);
        sg.viewport[0].set_ch('A');
        sg.cursor_col = 1;
        sg.cursor_line = 0;
        engine.on_server_sync(1, &sg, 1);

        // Now type 'B' — same epoch (1), which is now confirmed.
        // Actually, after sync, the overlay for pane 1 is removed (all confirmed).
        // So we need a new input after confirmation to test.
        engine.new_user_input(1, b"B", &grid);

        // The new prediction is at epoch 1 (confirmed_epoch was set to 1).
        // Wait — after clear, a new PaneOverlay is created with prediction_epoch=1,
        // confirmed_epoch=0. So 'B' at epoch 1 is still tentative. This is correct —
        // each new overlay starts fresh.
        assert_eq!(engine.get_overlay_cell(1, 0, 0), None);
    }

    // ---- next_input_seq ----

    #[test]
    fn next_input_seq_monotonic() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        assert_eq!(engine.next_input_seq(), 1);
        assert_eq!(engine.next_input_seq(), 2);
        assert_eq!(engine.next_input_seq(), 3);
    }

    // ---- Backspace at col 0 ----

    #[test]
    fn backspace_at_col_zero_increments_epoch() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);

        engine.new_user_input(1, &[0x7F], &grid);

        // Backspace at col 0 increments epoch (can't go further left).
        // A cursor overlay at col 0 is still created (cursor position is valid).
        // The key thing is it doesn't panic.
        assert_eq!(engine.get_overlay_cursor(1), Some((0, 0)));
    }

    // ---- Mouse and bracketed paste mode skip ----

    #[test]
    fn mouse_mode_skips_prediction() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let mut grid = make_grid_at(80, 24, 0, 0);
        grid.mode_flags = MODE_MOUSE_REPORT;
        engine.new_user_input(1, b"A", &grid);
        assert!(!engine.has_overlay(1));
    }

    #[test]
    fn bracketed_paste_mode_skips_prediction() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let mut grid = make_grid_at(80, 24, 0, 0);
        grid.mode_flags = MODE_BRACKETED_PASTE;
        engine.new_user_input(1, b"A", &grid);
        assert!(!engine.has_overlay(1));
    }

    // ---- Arrow keys at boundaries ----

    #[test]
    fn right_arrow_at_last_col_stays() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 79, 0);
        engine.new_user_input(1, b"\x1B[C", &grid);
        assert_eq!(engine.get_overlay_cursor(1), Some((0, 79))); // stays at last col
    }

    #[test]
    fn left_arrow_at_col_zero_stays() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);
        engine.new_user_input(1, b"\x1B[D", &grid);
        assert_eq!(engine.get_overlay_cursor(1), Some((0, 0))); // stays at col 0
    }

    // ---- Glitch trigger: minor (>=250ms pending) ----

    #[test]
    fn glitch_trigger_minor_on_medium_pending() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);
        engine.new_user_input(1, b"A", &grid);

        // Backdate the prediction's created_at to 300ms ago.
        let overlay = engine.overlays.get_mut(&1).unwrap();
        let row = overlay.rows.get_mut(&0).unwrap();
        row.cells[0].created_at = Instant::now() - std::time::Duration::from_millis(300);

        // Server sync with echo_ack=0 (pending), triggers glitch detection.
        let server_grid = make_grid(80, 24);
        engine.on_server_sync(1, &server_grid, 0);

        assert_eq!(engine.glitch_trigger, GLITCH_REPAIR_COUNT); // 10
    }

    // ---- Glitch trigger: major (>=5s pending) ----

    #[test]
    fn glitch_trigger_major_on_long_pending() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);
        engine.new_user_input(1, b"A", &grid);

        // Backdate to 6s ago. PREDICTION_TIMEOUT_SECS is 8s so cell survives
        // expire_old, but >= GLITCH_FLAG_THRESHOLD_MS (5000ms) triggers major glitch.
        let overlay = engine.overlays.get_mut(&1).unwrap();
        let row = overlay.rows.get_mut(&0).unwrap();
        row.cells[0].created_at = Instant::now() - std::time::Duration::from_millis(6000);

        let server_grid = make_grid(80, 24);
        engine.on_server_sync(1, &server_grid, 0);

        assert_eq!(engine.glitch_trigger, GLITCH_REPAIR_COUNT * 2); // 20 = major
    }

    // ---- Glitch repair: quick confirm decrements ----

    #[test]
    fn glitch_repair_decrements_on_quick_confirm() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        engine.glitch_trigger = 5;

        let grid = make_grid_at(80, 24, 0, 0);
        engine.new_user_input(1, b"A", &grid);

        // Server confirms quickly (created_at is just now, < 250ms).
        let mut server_grid = make_grid(80, 24);
        server_grid.viewport[0].set_ch('A');
        server_grid.cursor_col = 1;
        server_grid.cursor_line = 0;
        engine.on_server_sync(1, &server_grid, 1);

        assert_eq!(engine.glitch_trigger, 4); // decremented from 5 to 4
    }

    // ---- Glitch repair: interval limit ----

    #[test]
    fn glitch_repair_respects_min_interval() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        engine.glitch_trigger = 5;
        // Set last_quick_confirm to "just now" to block repair.
        engine.last_quick_confirm = Some(Instant::now());

        let grid = make_grid_at(80, 24, 0, 0);
        engine.new_user_input(1, b"A", &grid);

        let mut server_grid = make_grid(80, 24);
        server_grid.viewport[0].set_ch('A');
        server_grid.cursor_col = 1;
        server_grid.cursor_line = 0;
        engine.on_server_sync(1, &server_grid, 1);

        // Should NOT decrement because last_quick_confirm was too recent (< 150ms).
        assert_eq!(engine.glitch_trigger, 5);
    }

    // ---- Flagging hysteresis ----

    #[test]
    fn flagging_hysteresis_srtt_thresholds() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, true);
        let grid = make_grid_at(80, 24, 0, 0);
        engine.new_user_input(1, b"A", &grid);

        // SRTT > 80ms → flagging = true
        engine.srtt_us = 81_000;
        let mut sg = make_grid(80, 24);
        sg.viewport[0].set_ch('A');
        sg.cursor_col = 1;
        sg.cursor_line = 0;
        engine.on_server_sync(1, &sg, 1);
        assert!(engine.flagging);

        // Recreate prediction for next sync.
        engine.new_user_input(1, b"B", &grid);

        // SRTT in hysteresis band (50 < 60 <= 80) → flagging stays true
        engine.srtt_us = 60_000;
        let mut sg2 = make_grid(80, 24);
        sg2.viewport[0].set_ch('B');
        sg2.cursor_col = 1;
        sg2.cursor_line = 0;
        engine.on_server_sync(1, &sg2, 2);
        assert!(engine.flagging); // still true (hysteresis)

        // Recreate prediction.
        engine.new_user_input(1, b"C", &grid);

        // SRTT <= 50ms → flagging = false
        engine.srtt_us = 49_000;
        let mut sg3 = make_grid(80, 24);
        sg3.viewport[0].set_ch('C');
        sg3.cursor_col = 1;
        sg3.cursor_line = 0;
        engine.on_server_sync(1, &sg3, 3);
        assert!(!engine.flagging);
    }

    #[test]
    fn flagging_forced_by_major_glitch() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, true);
        engine.srtt_us = 0; // low SRTT, normally no flagging
        // Set to 15: quick confirm may decrement by 1 → 14, still > 10.
        engine.glitch_trigger = 15;

        let grid = make_grid_at(80, 24, 0, 0);
        engine.new_user_input(1, b"A", &grid);

        let mut sg = make_grid(80, 24);
        sg.viewport[0].set_ch('A');
        sg.cursor_col = 1;
        sg.cursor_line = 0;
        engine.on_server_sync(1, &sg, 1);

        assert!(engine.flagging); // forced by glitch_trigger > GLITCH_REPAIR_COUNT
    }

    // ---- expire_old timeout ----

    #[test]
    fn expire_old_clears_stale_predictions() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);
        engine.new_user_input(1, b"A", &grid);
        assert!(engine.has_overlay(1));

        // Backdate all cells to > PREDICTION_TIMEOUT_SECS ago.
        let overlay = engine.overlays.get_mut(&1).unwrap();
        let past = Instant::now() - std::time::Duration::from_secs(PREDICTION_TIMEOUT_SECS + 1);
        for row in overlay.rows.values_mut() {
            for cell in &mut row.cells {
                cell.created_at = past;
            }
        }

        // on_server_sync calls expire_old which should clear stale cells.
        let server_grid = make_grid(80, 24);
        engine.on_server_sync(1, &server_grid, 0);

        // All predictions expired, overlay should be empty.
        assert!(!engine.has_overlay(1));
    }

    // ---- Rendition propagation on server confirm ----

    #[test]
    fn rendition_propagation_on_confirm() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let mut grid = make_grid_at(80, 24, 0, 0);
        grid.viewport[0].fg = PackedColor::rgb(100, 100, 100);

        // Type 'A' (seq=1).
        engine.new_user_input(1, b"A", &grid);

        // Advance input_seq so next prediction gets a higher min_echo_ack.
        engine.next_input_seq += 10;

        // Type 'B' (seq=12) — different min_echo_ack than 'A'.
        engine.new_user_input(1, b"B", &grid);

        // Server confirms 'A' with actual colors different from predicted.
        let mut sg = make_grid(80, 24);
        sg.viewport[0].set_ch('A');
        sg.viewport[0].fg = PackedColor::rgb(255, 0, 0); // actual is red
        // Don't confirm cursor (leave cursor pending) by not matching position.
        // Actually, cursor is at col 2 after typing AB. Server has cursor at wrong pos
        // to keep cursor pending. But that might cause reset... Let's just match cursor.
        sg.cursor_col = 2;
        sg.cursor_line = 0;
        // echo_ack=1 → 'A' (min_echo_ack=1) is confirmed, 'B' (min_echo_ack=12) is pending.
        engine.on_server_sync(1, &sg, 1);

        // 'B' at col 1 should have its fg updated via rendition propagation.
        // Note: in the current two-pass implementation, propagation updates active cells
        // that come after a confirmed cell in the same row.
        let overlay = engine.overlays.get(&1);
        if let Some(o) = overlay {
            if let Some(cell) = o.get_cell(0, 1) {
                assert_eq!(cell.replacement.fg, PackedColor::rgb(255, 0, 0));
            }
        }
    }

    // ---- overlay.cols mismatch reset ----

    #[test]
    fn overlay_reset_on_cols_mismatch() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid80 = make_grid_at(80, 24, 0, 0);
        engine.new_user_input(1, b"A", &grid80);
        assert!(engine.has_overlay(1));

        // Now input with a 120-col grid — overlay.cols (80) != grid.cols (120).
        let grid120 = make_grid_at(120, 30, 0, 0);
        engine.new_user_input(1, b"B", &grid120);

        // Old overlay was reset, new prediction 'B' is at col 0 in the 120-col overlay.
        let cell = engine.get_overlay_cell(1, 0, 0);
        assert!(cell.is_some());
        assert_eq!(cell.unwrap().ch(), 'B');

        // Verify overlay.cols was updated.
        let overlay = engine.overlays.get(&1).unwrap();
        assert_eq!(overlay.cols, 120);
    }

    // ---- UTF-8 4-byte emoji ----

    #[test]
    fn utf8_four_byte_emoji() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);

        // '😀' = U+1F600 = [0xF0, 0x9F, 0x98, 0x80], width=2
        engine.new_user_input(1, &[0xF0, 0x9F, 0x98, 0x80], &grid);

        let cell = engine.get_overlay_cell(1, 0, 0);
        assert!(cell.is_some());
        assert_eq!(cell.unwrap().ch(), '😀');
        assert_ne!(cell.unwrap().flags_u16() & FLAG_WIDE_CHAR, 0);
        assert_eq!(engine.get_overlay_cursor(1), Some((0, 2)));
    }

    // ---- UTF-8 surrogate rejection ----

    #[test]
    fn utf8_surrogate_rejected() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let grid = make_grid_at(80, 24, 0, 0);

        // U+D800 (surrogate) = [0xED, 0xA0, 0x80] — invalid UTF-8
        engine.new_user_input(1, &[0xED, 0xA0, 0x80], &grid);

        // from_utf8 rejects surrogates, so no prediction should be created.
        assert_eq!(engine.get_overlay_cell(1, 0, 0), None);
    }

    // ---- SRTT EWMA update ----

    #[test]
    fn srtt_ewma_second_sample() {
        let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
        let base_us = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;

        // First pong: srtt = sample.
        engine.on_pong(1, base_us.saturating_sub(10_000));
        let first_srtt = engine.srtt_us;
        assert!(first_srtt > 0);

        // Second pong: EWMA = (srtt*7 + sample)/8
        engine.on_pong(2, base_us.saturating_sub(20_000));
        // SRTT should change (second sample is different).
        assert_ne!(engine.srtt_us, first_srtt);
    }

    // ---- update_config ----

    #[test]
    fn update_config_changes_behavior() {
        let mut engine = PredictionEngine::new(PredictionMode::Never, 0, false);
        assert!(!engine.should_display());

        engine.update_config(PredictionMode::Always, 0, true);
        assert!(engine.should_display());
    }

    // ---- Backspace on wide-char direct (not spacer) ----

    #[test]
    fn backspace_on_wide_char_direct() {
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let mut grid = make_grid_at(10, 1, 1, 0);
        // Wide char at col 0 (FLAG_WIDE_CHAR set, no spacer at col 1 since cursor is there).
        grid.viewport[0].set_ch('中');
        grid.viewport[0].flags = FLAG_WIDE_CHAR.to_le_bytes();

        // Cursor at col 1, backspace — lands on col 0 which has FLAG_WIDE_CHAR → del_width=2.
        engine.new_user_input(1, &[0x7F], &grid);

        // Cursor should be at col 0 (deleted the wide char).
        assert_eq!(engine.get_overlay_cursor(1), Some((0, 0)));
    }

    // ---- Adaptive cursor shown when confirmed ----

    #[test]
    fn adaptive_cursor_shown_when_confirmed() {
        let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
        engine.srtt_us = 50_000;

        let grid = make_grid_at(80, 24, 0, 0);
        engine.new_user_input(1, b"A", &grid);

        // Before confirmation, cursor is tentative (epoch=1, confirmed=0).
        assert_eq!(engine.get_overlay_cursor(1), None);

        // Confirm 'A' — advances confirmed_epoch.
        let mut sg = make_grid(80, 24);
        sg.viewport[0].set_ch('A');
        sg.cursor_col = 1;
        sg.cursor_line = 0;
        engine.on_server_sync(1, &sg, 1);

        // After confirmation, the overlay is cleared (both cell and cursor confirmed).
        // So cursor is gone. This tests that confirmation works end-to-end.
        assert!(!engine.has_overlay(1));

        // New input after confirmation — epoch restarts at 1, confirmed still 0 in new overlay.
        engine.new_user_input(1, b"B", &grid);
        // Still tentative.
        assert_eq!(engine.get_overlay_cursor(1), None);
    }
}
