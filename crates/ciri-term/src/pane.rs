use alacritty_terminal::event::Event;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::Config as TermConfig;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape, NamedColor, Processor};
use anyhow::Result;
use ciri_protocol::message::*;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use crate::event::PtyEventListener;
use crate::pty::Pty;

pub type PaneId = u64;

/// Semantic zone type from OSC 133 shell integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticZone {
    /// After prompt start (OSC 133;A) — the prompt region.
    Prompt,
    /// After command start (OSC 133;B) — user is typing a command.
    Input,
    /// After command executed (OSC 133;C) — command output region.
    Output,
}

/// An inline image placement in the terminal grid.
#[derive(Debug, Clone)]
pub struct ImagePlacement {
    /// Unique ID for this image.
    pub id: u64,
    /// Row (viewport-relative) where the image starts.
    pub row: u16,
    /// Column where the image starts.
    pub col: u16,
    /// Width in cells.
    pub width_cells: u16,
    /// Height in cells.
    pub height_cells: u16,
    /// Image width in pixels (from protocol).
    pub pixel_width: u32,
    /// Image height in pixels (from protocol).
    pub pixel_height: u32,
    /// Image format: "png", "rgb", "rgba", "sixel".
    pub format: String,
    /// Raw image data (PNG/RGB/RGBA bytes, or decoded sixel).
    pub data: Vec<u8>,
}

/// Kitty image metadata parsed from the first chunk of a transmission.
#[derive(Debug, Clone)]
struct KittyImageMeta {
    format: String,     // "png", "rgb", "rgba"
    width: u32,
    height: u32,
    cols: u16,
    rows: u16,
}

/// Shell integration state tracked via OSC 133.
#[derive(Debug, Clone)]
pub struct ShellState {
    /// Current semantic zone.
    pub zone: SemanticZone,
    /// Last command exit code (from OSC 133;D;exitcode).
    pub last_exit_code: Option<i32>,
    /// Line where the current prompt started.
    pub prompt_line: Option<i32>,
    /// Line where command output started.
    pub output_line: Option<i32>,
}

struct TermSize {
    cols: usize,
    rows: usize,
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize { self.rows }
    fn screen_lines(&self) -> usize { self.rows }
    fn columns(&self) -> usize { self.cols }
}

pub struct Pane {
    pub id: PaneId,
    pub(crate) term: Arc<Mutex<Term<PtyEventListener>>>,
    dirty: bool,
    exited: bool,
    pty: Pty,
    processor: Processor,
    event_rx: mpsc::Receiver<Event>,
    cols: u16,
    rows: u16,
    pub title: String,
    /// Pending clipboard writes from OSC 52 (drained by server each tick).
    clipboard_pending: Vec<String>,
    /// Bell fired since last drain (BEL / \x07).
    bell_pending: bool,
    /// Shell integration state (OSC 133).
    pub shell_state: ShellState,
    /// Inline image placements (Kitty graphics / Sixel).
    pub image_placements: Vec<ImagePlacement>,
    /// Next image ID counter.
    next_image_id: u64,
    /// Kitty graphics: partial payload accumulator for multi-chunk transmissions.
    kitty_image_buf: Vec<u8>,
    /// Kitty graphics: metadata from the first chunk (a=T transmit-and-display).
    kitty_image_meta: Option<KittyImageMeta>,
    /// Partial APC frame buffer: holds bytes from an unterminated `ESC _ G ...`
    /// sequence that was split across PTY reads.
    kitty_apc_partial: Vec<u8>,
}

impl Pane {
    pub fn new(id: PaneId, cols: u16, rows: u16, shell: &str) -> Result<Self> {
        let pty = Pty::spawn(cols, rows, shell)?;

        let size = TermSize { cols: cols as usize, rows: rows as usize };
        let mut config = TermConfig::default();
        config.kitty_keyboard = true;
        let (event_listener, event_rx) = PtyEventListener::new();
        let term = Term::new(config, &size, event_listener);

        Ok(Pane {
            id,
            term: Arc::new(Mutex::new(term)),
            dirty: true,
            exited: false,
            pty,
            processor: Processor::new(),
            event_rx,
            cols,
            rows,
            title: String::new(),
            clipboard_pending: Vec::new(),
            bell_pending: false,
            shell_state: ShellState {
                zone: SemanticZone::Prompt,
                last_exit_code: None,
                prompt_line: None,
                output_line: None,
            },
            image_placements: Vec::new(),
            next_image_id: 1,
            kitty_image_buf: Vec::new(),
            kitty_image_meta: None,
            kitty_apc_partial: Vec::new(),
        })
    }

    /// Read PTY output, feed to terminal emulator, and handle terminal events.
    pub fn process_pty_output(&mut self) -> bool {
        if self.exited {
            return false;
        }

        let mut processed = false;

        // Drain all available output from the background reader thread
        let chunks = self.pty.drain_output();
        if !chunks.is_empty() {
            // Scan for OSC 133 shell integration sequences before VT parsing
            // (alacritty_terminal ignores these).
            for chunk in &chunks {
                self.scan_osc133(chunk);
            }

            // VT-parse all chunks first so the cursor reflects any preceding
            // movement sequences (CSI H, etc.) in the same read batch.
            let mut term = self.term.lock().unwrap_or_else(|e| e.into_inner());
            for chunk in &chunks {
                self.processor.advance(&mut *term, chunk);
            }

            // Now read cursor position for image placement — after parsing.
            let (cursor_col, cursor_row) = {
                let cursor = term.grid().cursor.point;
                (cursor.column.0 as u16, cursor.line.0.max(0) as u16)
            };
            drop(term);

            // Scan for Kitty graphics sequences with the post-parse cursor position.
            for chunk in &chunks {
                self.scan_kitty_graphics(chunk, cursor_col, cursor_row);
            }
            processed = true;
        }

        if self.pty.reader_eof() {
            self.exited = true;
        }

        // Process terminal events (PtyWrite for DA1/DA2 responses, etc.)
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                Event::PtyWrite(text) => {
                    self.write_to_pty(text.as_bytes());
                }
                Event::Title(t) => {
                    self.title = t;
                }
                Event::ResetTitle => {
                    self.title.clear();
                }
                Event::ClipboardStore(_, text) => {
                    // OSC 52: TUI app wants to write to system clipboard
                    self.clipboard_pending.push(text);
                }
                Event::ClipboardLoad(_, formatter) => {
                    // OSC 52: TUI app wants to read clipboard.
                    // We can't access the client clipboard from the server,
                    // so respond with empty string (common fallback).
                    let response = formatter("");
                    self.write_to_pty(response.as_bytes());
                }
                Event::Bell => {
                    self.bell_pending = true;
                }
                Event::Exit | Event::ChildExit(_) => {
                    self.exited = true;
                }
                _ => {}
            }
        }

        if !self.exited && self.pty.try_wait() {
            self.exited = true;
        }

        if processed {
            self.dirty = true;
        }
        processed
    }

    /// Drain pending OSC 52 clipboard writes.
    pub fn drain_clipboard(&mut self) -> Vec<String> {
        std::mem::take(&mut self.clipboard_pending)
    }

    /// Check and clear the bell pending flag.
    pub fn drain_bell(&mut self) -> bool {
        std::mem::take(&mut self.bell_pending)
    }

    /// Get the current shell semantic zone (from OSC 133).
    pub fn shell_zone(&self) -> SemanticZone {
        self.shell_state.zone
    }

    /// Get the last command exit code (from OSC 133;D).
    pub fn last_exit_code(&self) -> Option<i32> {
        self.shell_state.last_exit_code
    }

    pub fn write_to_pty(&mut self, data: &[u8]) {
        if self.exited { return; }
        if let Err(e) = self.pty.write(data) {
            log::warn!("pty write failed (pane {}): {e}", self.id);
        }
    }

    /// Check if the terminal has mouse reporting mode enabled.
    pub fn has_mouse_mode(&self) -> bool {
        use alacritty_terminal::term::TermMode;
        let term = self.term.lock().unwrap_or_else(|e| e.into_inner());
        let mode = term.mode();
        mode.contains(TermMode::MOUSE_REPORT_CLICK)
            || mode.contains(TermMode::MOUSE_DRAG)
            || mode.contains(TermMode::MOUSE_MOTION)
    }

    /// Get terminal mode flags for the protocol (mouse mode, alt screen).
    pub fn mode_flags(&self) -> u8 {
        use alacritty_terminal::term::TermMode;
        use ciri_protocol::message::{MODE_MOUSE_REPORT, MODE_ALT_SCREEN};
        let term = self.term.lock().unwrap_or_else(|e| e.into_inner());
        let mode = term.mode();
        let mut flags = 0u8;
        if mode.contains(TermMode::MOUSE_REPORT_CLICK)
            || mode.contains(TermMode::MOUSE_DRAG)
            || mode.contains(TermMode::MOUSE_MOTION) {
            flags |= MODE_MOUSE_REPORT;
        }
        if mode.contains(TermMode::ALT_SCREEN) {
            flags |= MODE_ALT_SCREEN;
        }
        flags
    }

    /// Forward mouse input as SGR escape sequence to the PTY.
    /// Does NOT reset viewport state (TUI apps manage their own scrolling).
    pub fn send_mouse_input(&self, button: u8, col: u16, row: u16, pressed: bool, modifiers: u8) {
        let btn_with_mods = button as u32 | ((modifiers as u32) << 2);
        let suffix = if pressed { 'M' } else { 'm' };
        let seq = format!("\x1b[<{};{};{}{}", btn_with_mods, col + 1, row + 1, suffix);
        if let Err(e) = self.pty.write(seq.as_bytes()) {
            log::warn!("pty write failed (pane {}): {e}", self.id);
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if cols == self.cols && rows == self.rows { return; }
        self.cols = cols;
        self.rows = rows;
        self.pty.resize(cols, rows);
        let size = TermSize { cols: cols as usize, rows: rows as usize };
        let mut term = self.term.lock().unwrap_or_else(|e| e.into_inner());
        term.resize(size);
        self.dirty = true;
    }

    pub fn grid_cols(&self) -> u16 { self.cols }
    pub fn grid_rows(&self) -> u16 { self.rows }

    pub fn is_dirty(&self) -> bool { self.dirty }
    pub fn is_exited(&self) -> bool { self.exited }
    pub fn set_dirty(&mut self, dirty: bool) { self.dirty = dirty; }

    /// Extract damage regions from the terminal. Returns None if no damage.
    /// Resets damage tracking after extraction.
    pub fn extract_damage(&mut self) -> Option<Vec<DamageRegion>> {
        let mut term = self.term.lock().unwrap_or_else(|e| e.into_inner());
        let grid = term.grid();
        let cols = grid.columns();
        let total_rows = grid.screen_lines();

        if cols == 0 || total_rows == 0 {
            term.reset_damage();
            return None;
        }

        use alacritty_terminal::term::TermDamage;
        let damage = term.damage();
        let regions = match damage {
            TermDamage::Full => {
                let mut regions = Vec::with_capacity(total_rows);
                for row in 0..total_rows {
                    let right = cols.saturating_sub(1);
                    let cells = self.read_line_cells(&term, row, 0, right);
                    regions.push(DamageRegion {
                        line: row as u16,
                        left: 0,
                        right: right as u16,
                        cells,
                    });
                }
                regions
            }
            TermDamage::Partial(iter) => {
                let bounds: Vec<_> = iter.collect();
                if bounds.is_empty() {
                    term.reset_damage();
                    return None;
                }
                // Expand each damaged line to full width to catch cleared cells
                // (e.g., PSReadLine prediction text that was erased)
                let right = cols.saturating_sub(1);
                let mut seen_lines = std::collections::HashSet::new();
                let mut regions = Vec::with_capacity(bounds.len());
                for b in bounds {
                    if seen_lines.insert(b.line) {
                        let cells = self.read_line_cells(&term, b.line, 0, right);
                        regions.push(DamageRegion {
                            line: b.line as u16,
                            left: 0,
                            right: right as u16,
                            cells,
                        });
                    }
                }
                regions
            }
        };
        term.reset_damage();
        Some(regions)
    }

    /// Read cells for a specific line range (for CellDelta).
    pub fn read_cells(&self, line: u16, left: u16, right: u16) -> Vec<PackedCell> {
        let term = self.term.lock().unwrap_or_else(|e| e.into_inner());
        let grid = term.grid();
        let mut cells = Vec::with_capacity((right - left + 1) as usize);
        for col in left..=right {
            let point = Point::new(Line(line as i32), Column(col as usize));
            cells.push(pack_cell(&grid[point]));
        }
        cells
    }

    /// Read cursor position, shape, and mode flags.
    pub fn cursor_info(&self) -> (i16, u16, u8, u8) {
        let term = self.term.lock().unwrap_or_else(|e| e.into_inner());
        let content = term.renderable_content();
        let cursor_line = content.cursor.point.line.0 as i16;
        let cursor_col = content.cursor.point.column.0 as u16;
        let cursor_shape = match content.cursor.shape {
            CursorShape::Block => CURSOR_BLOCK,
            CursorShape::Underline => CURSOR_UNDERLINE,
            CursorShape::Beam => CURSOR_BEAM,
            CursorShape::HollowBlock => CURSOR_HOLLOW_BLOCK,
            CursorShape::Hidden => CURSOR_HIDDEN,
        };
        let mode_flags = self.mode_flags_from_term(&term);
        (cursor_line, cursor_col, cursor_shape, mode_flags)
    }

    /// Current history size (number of scrollback lines).
    pub fn history_size(&self) -> usize {
        let term = self.term.lock().unwrap_or_else(|e| e.into_inner());
        term.grid().history_size()
    }

    /// Create a full pane snapshot with incremental scrollback (only new lines since `history_sent`).
    pub fn snapshot_incremental(&self, generation: u64, history_sent: usize) -> FullPaneSync {
        let term = self.term.lock().unwrap_or_else(|e| e.into_inner());
        let grid = term.grid();
        let cols = grid.columns();
        let rows = grid.screen_lines();
        let current_history = grid.history_size();
        let content = term.renderable_content();

        // Read viewport cells
        let mut cells = Vec::with_capacity(cols * rows);
        for row in 0..rows {
            for col in 0..cols {
                let point = Point::new(Line(row as i32), Column(col));
                cells.push(pack_cell(&grid[point]));
            }
        }

        // Read new history lines (only lines not yet sent)
        let last_sent = history_sent.min(current_history);
        let new_lines = current_history - last_sent;
        let mut sb_cells = Vec::new();
        if new_lines > 0 {
            sb_cells.reserve(new_lines * cols);
            for i in (1..=new_lines).rev() {
                for col in 0..cols {
                    let point = Point::new(Line(-(i as i32)), Column(col));
                    sb_cells.push(pack_cell(&grid[point]));
                }
            }
        }

        let cursor_shape = match content.cursor.shape {
            CursorShape::Block => CURSOR_BLOCK,
            CursorShape::Underline => CURSOR_UNDERLINE,
            CursorShape::Beam => CURSOR_BEAM,
            CursorShape::HollowBlock => CURSOR_HOLLOW_BLOCK,
            CursorShape::Hidden => CURSOR_HIDDEN,
        };

        let mode_flags = self.mode_flags_from_term(&term);

        FullPaneSync {
            pane_id: self.id,
            generation,
            cols: cols as u16,
            rows: rows as u16,
            cursor_line: content.cursor.point.line.0 as i16,
            cursor_col: content.cursor.point.column.0 as u16,
            cursor_shape,
            mode_flags,
            title: self.title.clone(),
            scrollback: sb_cells,
            scrollback_rows: new_lines as u16,
            cells,
        }
    }

    /// Helper: compute mode flags from a locked term reference (avoids double-locking).
    fn mode_flags_from_term(&self, term: &Term<PtyEventListener>) -> u8 {
        use alacritty_terminal::term::TermMode;
        let mode = term.mode();
        let mut flags = 0u8;
        if mode.contains(TermMode::MOUSE_REPORT_CLICK)
            || mode.contains(TermMode::MOUSE_DRAG)
            || mode.contains(TermMode::MOUSE_MOTION) {
            flags |= MODE_MOUSE_REPORT;
        }
        if mode.contains(TermMode::ALT_SCREEN) {
            flags |= MODE_ALT_SCREEN;
        }
        // Shell integration detected if we've seen OSC 133 sequences
        if self.shell_state.prompt_line.is_some() {
            flags |= MODE_SHELL_INTEGRATION;
        }
        // Kitty keyboard protocol: at minimum, disambiguate escape codes
        if mode.contains(TermMode::DISAMBIGUATE_ESC_CODES) {
            flags |= MODE_KITTY_KEYBOARD;
        }
        flags
    }

    /// Scan raw PTY output for OSC 133 shell integration sequences.
    /// Pattern: ESC ] 133 ; <cmd> [; params] BEL   or   ESC ] 133 ; <cmd> [; params] ESC \
    fn scan_osc133(&mut self, data: &[u8]) {
        let mut i = 0;
        while i + 6 < data.len() {
            // Look for ESC ] 1 3 3 ;
            if data[i] == 0x1b && data[i + 1] == b']'
                && data[i + 2] == b'1'
                && data[i + 3] == b'3'
                && data[i + 4] == b'3'
                && data[i + 5] == b';'
            {
                let cmd = data[i + 6];
                // Find the string terminator and collect params
                let mut end = i + 7;
                let mut params = String::new();
                while end < data.len() {
                    if data[end] == 0x07 {
                        break;
                    }
                    if data[end] == 0x1b && data.get(end + 1) == Some(&b'\\') {
                        break;
                    }
                    if data[end] == b';' && params.is_empty() {
                        let rest_start = end + 1;
                        let mut rest_end = rest_start;
                        while rest_end < data.len() {
                            if data[rest_end] == 0x07 || (data[rest_end] == 0x1b && data.get(rest_end + 1) == Some(&b'\\')) {
                                break;
                            }
                            rest_end += 1;
                        }
                        params = String::from_utf8_lossy(&data[rest_start..rest_end]).to_string();
                        end = rest_end;
                        break;
                    }
                    end += 1;
                }

                match cmd {
                    b'A' => {
                        self.shell_state.zone = SemanticZone::Prompt;
                        self.shell_state.prompt_line = Some(0); // exact line resolved at snapshot time
                        log::debug!("OSC 133;A prompt start");
                    }
                    b'B' => {
                        self.shell_state.zone = SemanticZone::Input;
                        log::debug!("OSC 133;B command input");
                    }
                    b'C' => {
                        self.shell_state.zone = SemanticZone::Output;
                        self.shell_state.output_line = Some(0);
                        log::debug!("OSC 133;C command output");
                    }
                    b'D' => {
                        self.shell_state.zone = SemanticZone::Prompt;
                        let exit_code = params.trim().parse::<i32>().ok();
                        self.shell_state.last_exit_code = exit_code;
                        log::debug!("OSC 133;D command done, exit={exit_code:?}");
                    }
                    _ => {
                        log::trace!("OSC 133;{} unknown subcommand", cmd as char);
                    }
                }
                i = end + 1;
            } else {
                i += 1;
            }
        }
    }

    /// Scan for Kitty graphics protocol sequences (APC: ESC _ G ... ESC \).
    /// Handles frames split across PTY reads by buffering partial sequences.
    fn scan_kitty_graphics(&mut self, data: &[u8], cursor_col: u16, cursor_row: u16) {
        use base64::Engine;

        // If we have a partial APC from a previous read, prepend it
        let working_data;
        let data = if !self.kitty_apc_partial.is_empty() {
            self.kitty_apc_partial.extend_from_slice(data);
            working_data = std::mem::take(&mut self.kitty_apc_partial);
            &working_data[..]
        } else {
            data
        };

        let mut i = 0;
        while i + 3 < data.len() {
            // Look for ESC _ G (APC for Kitty graphics)
            if data[i] == 0x1b && data[i + 1] == b'_' && data[i + 2] == b'G' {
                // Find the string terminator (ESC \)
                let start = i + 3;
                let mut end = start;
                while end + 1 < data.len() {
                    if data[end] == 0x1b && data[end + 1] == b'\\' {
                        break;
                    }
                    end += 1;
                }
                if end + 1 >= data.len() {
                    // Incomplete sequence — buffer from the APC start for next read
                    self.kitty_apc_partial = data[i..].to_vec();
                    return;
                }

                let payload = &data[start..end];

                // Split at first ';' into control and data parts
                let (control, img_data) = if let Some(sep) = payload.iter().position(|&b| b == b';') {
                    (&payload[..sep], &payload[sep + 1..])
                } else {
                    (payload, &[][..])
                };

                // Parse key=value pairs from control
                let control_str = String::from_utf8_lossy(control);
                let mut action = 'T'; // default: transmit and display
                let mut format_val = 32u32; // 32=PNG, 24=RGB, 32=RGBA
                let mut width = 0u32;
                let mut height = 0u32;
                let mut cols = 0u16;
                let mut rows = 0u16;
                let mut more_chunks = false;

                for pair in control_str.split(',') {
                    if let Some((k, v)) = pair.split_once('=') {
                        match k {
                            "a" => action = v.chars().next().unwrap_or('T'),
                            "f" => format_val = v.parse().unwrap_or(32),
                            "s" => width = v.parse().unwrap_or(0),
                            "v" => height = v.parse().unwrap_or(0),
                            "c" => cols = v.parse().unwrap_or(0),
                            "r" => rows = v.parse().unwrap_or(0),
                            "m" => more_chunks = v == "1",
                            _ => {}
                        }
                    }
                }

                // Decode base64 image data
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(img_data)
                    .unwrap_or_default();

                match action {
                    'T' | 't' => {
                        // Transmit (and display if 'T')
                        if more_chunks {
                            // First/middle chunk: accumulate
                            if self.kitty_image_meta.is_none() {
                                let fmt = match format_val {
                                    24 => "rgb",
                                    32 => "rgba",
                                    _ => "png",
                                };
                                self.kitty_image_meta = Some(KittyImageMeta {
                                    format: fmt.to_string(),
                                    width,
                                    height,
                                    cols: if cols > 0 { cols } else { 10 },
                                    rows: if rows > 0 { rows } else { 5 },
                                });
                            }
                            self.kitty_image_buf.extend_from_slice(&decoded);
                        } else {
                            // Final (or only) chunk
                            let mut full_data = std::mem::take(&mut self.kitty_image_buf);
                            full_data.extend_from_slice(&decoded);

                            let meta = self.kitty_image_meta.take().unwrap_or(KittyImageMeta {
                                format: match format_val {
                                    24 => "rgb".to_string(),
                                    32 => "rgba".to_string(),
                                    _ => "png".to_string(),
                                },
                                width,
                                height,
                                cols: if cols > 0 { cols } else { 10 },
                                rows: if rows > 0 { rows } else { 5 },
                            });

                            if !full_data.is_empty() {
                                let id = self.next_image_id;
                                self.next_image_id += 1;
                                log::info!(
                                    "kitty image #{id}: {}x{} pixels, {} cells, {}x{} grid, {} bytes",
                                    meta.width, meta.height, meta.format,
                                    meta.cols, meta.rows, full_data.len()
                                );
                                self.image_placements.push(ImagePlacement {
                                    id,
                                    row: cursor_row,
                                    col: cursor_col,
                                    width_cells: meta.cols,
                                    height_cells: meta.rows,
                                    pixel_width: meta.width,
                                    pixel_height: meta.height,
                                    format: meta.format,
                                    data: full_data,
                                });
                            }
                        }
                    }
                    'd' => {
                        // Delete images (we clear all for now)
                        self.image_placements.clear();
                    }
                    _ => {}
                }

                i = end + 2; // skip past ESC \
            } else {
                i += 1;
            }
        }
    }

    /// Drain new image placements since last call.
    pub fn drain_images(&mut self) -> Vec<ImagePlacement> {
        std::mem::take(&mut self.image_placements)
    }

    /// Read cells from a line range and pack them (live viewport only).
    fn read_line_cells(
        &self,
        term: &Term<PtyEventListener>,
        line: usize,
        left: usize,
        right: usize,
    ) -> Vec<PackedCell> {
        let grid = term.grid();
        let mut cells = Vec::with_capacity(right - left + 1);
        for col in left..=right {
            let point = Point::new(Line(line as i32), Column(col));
            let cell = &grid[point];
            cells.push(pack_cell(cell));
        }
        cells
    }

    /// Create a full pane snapshot for StateSync / reattach.
    pub fn snapshot(&self, generation: u64) -> FullPaneSync {
        let term = self.term.lock().unwrap_or_else(|e| e.into_inner());
        let grid = term.grid();
        let cols = grid.columns();
        let rows = grid.screen_lines();
        let content = term.renderable_content();

        let mut cells = Vec::with_capacity(cols * rows);
        for row in 0..rows {
            for col in 0..cols {
                let point = Point::new(Line(row as i32), Column(col));
                let cell = &grid[point];
                cells.push(pack_cell(cell));
            }
        }

        // Include recent scrollback for attach/reattach (capped to avoid huge syncs)
        let history_size = grid.history_size();
        let max_scrollback = 1000.min(history_size);
        let mut sb_cells = Vec::new();
        if max_scrollback > 0 {
            sb_cells.reserve(max_scrollback * cols);
            // Line(-max_scrollback) = oldest, Line(-1) = newest
            for i in (1..=max_scrollback).rev() {
                for col in 0..cols {
                    let point = Point::new(Line(-(i as i32)), Column(col));
                    sb_cells.push(pack_cell(&grid[point]));
                }
            }
        }

        let cursor_shape = match content.cursor.shape {
            CursorShape::Block => CURSOR_BLOCK,
            CursorShape::Underline => CURSOR_UNDERLINE,
            CursorShape::Beam => CURSOR_BEAM,
            CursorShape::HollowBlock => CURSOR_HOLLOW_BLOCK,
            CursorShape::Hidden => CURSOR_HIDDEN,
        };

        let mode_flags = self.mode_flags_from_term(&term);

        FullPaneSync {
            pane_id: self.id,
            generation,
            cols: cols as u16,
            rows: rows as u16,
            cursor_line: content.cursor.point.line.0 as i16,
            cursor_col: content.cursor.point.column.0 as u16,
            cursor_shape,
            mode_flags,
            title: self.title.clone(),
            scrollback: sb_cells,
            scrollback_rows: max_scrollback as u16,
            cells,
        }
    }
}

/// Pack an alacritty cell into our wire format.
pub fn pack_cell(cell: &alacritty_terminal::term::cell::Cell) -> PackedCell {
    let fg = pack_color(cell.fg);
    let bg = pack_color(cell.bg);
    let mut flags = 0u16;
    if cell.flags.contains(CellFlags::WIDE_CHAR) { flags |= FLAG_WIDE_CHAR; }
    if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) { flags |= FLAG_WIDE_CHAR_SPACER; }
    if cell.flags.contains(CellFlags::BOLD) { flags |= FLAG_BOLD; }
    if cell.flags.contains(CellFlags::ITALIC) { flags |= FLAG_ITALIC; }
    if cell.flags.contains(CellFlags::ALL_UNDERLINES) { flags |= FLAG_UNDERLINE; }
    if cell.flags.contains(CellFlags::INVERSE) { flags |= FLAG_INVERSE; }
    if cell.flags.contains(CellFlags::DIM) { flags |= FLAG_DIM; }
    if cell.flags.contains(CellFlags::STRIKEOUT) { flags |= FLAG_STRIKEOUT; }
    if cell.flags.contains(CellFlags::HIDDEN) { flags |= FLAG_HIDDEN; }
    let mut packed = PackedCell {
        ch_bytes: [0; 4],
        fg, bg,
        flags: flags.to_le_bytes(),
    };
    packed.set_ch(cell.c);
    packed
}

/// Convert alacritty AnsiColor to PackedColor.
pub fn pack_color(color: AnsiColor) -> PackedColor {
    match color {
        AnsiColor::Named(n) => PackedColor::named(named_color_to_compact(n)),
        AnsiColor::Spec(rgb) => PackedColor::rgb(rgb.r, rgb.g, rgb.b),
        AnsiColor::Indexed(i) => PackedColor::indexed(i),
    }
}

fn named_color_to_compact(n: NamedColor) -> u8 {
    use alacritty_terminal::vte::ansi::NamedColor::*;
    #[allow(unreachable_patterns)]
    match n {
        Black => 0, Red => 1, Green => 2, Yellow => 3,
        Blue => 4, Magenta => 5, Cyan => 6, White => 7,
        BrightBlack => 8, BrightRed => 9, BrightGreen => 10, BrightYellow => 11,
        BrightBlue => 12, BrightMagenta => 13, BrightCyan => 14, BrightWhite => 15,
        Foreground => 16, Background => 17, Cursor => 18,
        DimBlack => 19, DimRed => 20, DimGreen => 21, DimYellow => 22,
        DimBlue => 23, DimMagenta => 24, DimCyan => 25, DimWhite => 26,
        BrightForeground => 27, DimForeground => 28,
        _ => 16, // fallback to Foreground
    }
}
