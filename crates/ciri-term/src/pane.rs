use alacritty_terminal::event::Event;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::Config as TermConfig;
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape, NamedColor, Processor};
use anyhow::Result;
use ciri_protocol::message::*;
use std::sync::Arc;
use std::sync::mpsc;

use crate::dec_mode_parser::DecModeParser;
use crate::event::PtyEventListener;
use crate::kitty_graphics::KittyGraphicsParser;
use crate::osc7_parser::Osc7Parser;
use crate::osc8_parser::Osc8Parser;
use crate::pty::Pty;
use crate::shell_integration::Osc133Parser;
use crate::sixel::SixelParser;

pub type PaneId = u64;

/// Maximum number of active image placements retained for reconnecting clients.
const MAX_ACTIVE_IMAGES: usize = 64;

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
    pub data: Arc<Vec<u8>>,
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
    /// Timestamp when the last command started (OSC 133;C).
    pub command_start: Option<std::time::Instant>,
}

struct TermSize {
    cols: usize,
    rows: usize,
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// Map alacritty CursorShape to our wire-format constant.
fn cursor_shape_to_u8(shape: CursorShape) -> u8 {
    match shape {
        CursorShape::Block => CURSOR_BLOCK,
        CursorShape::Underline => CURSOR_UNDERLINE,
        CursorShape::Beam => CURSOR_BEAM,
        CursorShape::HollowBlock => CURSOR_HOLLOW_BLOCK,
        CursorShape::Hidden => CURSOR_HIDDEN,
    }
}

#[derive(Debug, Clone, Copy)]
struct SnapshotScrollback {
    rows: usize,
}

impl SnapshotScrollback {
    fn full(history_size: usize) -> Self {
        Self { rows: history_size }
    }

    fn incremental(current_history: usize, history_sent: usize) -> Self {
        let retained_history = history_sent.min(current_history);
        Self {
            rows: current_history - retained_history,
        }
    }
}

pub struct Pane {
    pub id: PaneId,
    term: Term<PtyEventListener>,
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
    /// Active image placements (persistent — survives drain, used for reconnecting clients).
    active_images: Vec<ImagePlacement>,
    /// Newly added image placements since last drain (broadcast to clients then cleared).
    pending_images: Vec<ImagePlacement>,
    /// Kitty graphics protocol parser.
    kitty_parser: KittyGraphicsParser,
    /// OSC 133 shell integration parser.
    osc133_parser: Osc133Parser,
    /// DEC private mode parser (focus events 1004, sync output 2026).
    dec_mode_parser: DecModeParser,
    /// Sixel image protocol parser.
    sixel_parser: SixelParser,
    /// OSC 8 hyperlink parser.
    osc8_parser: Osc8Parser,
    /// OSC 7 working directory parser.
    osc7_parser: Osc7Parser,
    /// Duration of the last completed command (set on OSC 133;D, drained by server).
    last_command_duration: Option<std::time::Duration>,
}

impl Pane {
    pub fn new(id: PaneId, cols: u16, rows: u16, shell: &str) -> Result<Self> {
        Self::new_with_opts(id, cols, rows, shell, None, None)
    }

    /// Create a new pane with CWD override (convenience for OSC 7 CWD inheritance).
    pub fn new_with_cwd(
        id: PaneId,
        cols: u16,
        rows: u16,
        shell: &str,
        cwd: Option<&std::path::Path>,
    ) -> Result<Self> {
        Self::new_with_opts(id, cols, rows, shell, None, cwd)
    }

    /// Create a new pane with optional command and working directory.
    pub fn new_with_opts(
        id: PaneId,
        cols: u16,
        rows: u16,
        shell: &str,
        command: Option<&str>,
        cwd: Option<&std::path::Path>,
    ) -> Result<Self> {
        let pty = Pty::spawn_with_opts(cols, rows, shell, command, cwd)?;

        let size = TermSize {
            cols: cols as usize,
            rows: rows as usize,
        };
        let config = TermConfig {
            kitty_keyboard: true,
            ..TermConfig::default()
        };
        let (event_listener, event_rx) = PtyEventListener::new();
        let term = Term::new(config, &size, event_listener);

        Ok(Pane {
            id,
            term,
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
                command_start: None,
            },
            active_images: Vec::new(),
            pending_images: Vec::new(),
            kitty_parser: KittyGraphicsParser::new(),
            osc133_parser: Osc133Parser::new(),
            dec_mode_parser: DecModeParser::new(),
            sixel_parser: SixelParser::new(),
            osc8_parser: Osc8Parser::new(),
            osc7_parser: Osc7Parser::new(),
            last_command_duration: None,
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
            // Scan for OSC 133 shell integration, DEC private mode, OSC 8
            // hyperlink, and OSC 7 CWD sequences before VT parsing
            // (alacritty_terminal ignores these).
            for chunk in &chunks {
                self.osc133_parser.scan(
                    chunk,
                    &mut self.shell_state,
                    &mut self.last_command_duration,
                );
                self.dec_mode_parser.scan(chunk);
                self.osc8_parser.scan(chunk);
                self.osc7_parser.scan(chunk);
            }

            // VT-parse each chunk individually, recording the cursor position
            // after each one. This ensures Kitty image placements use the cursor
            // at the time of each sequence, not the final cursor after all chunks.
            let mut per_chunk_cursors: Vec<(u16, u16)> = Vec::with_capacity(chunks.len());
            for chunk in &chunks {
                self.processor.advance(&mut self.term, chunk);
                let cursor = self.term.grid().cursor.point;
                per_chunk_cursors.push((cursor.column.0 as u16, cursor.line.0.max(0) as u16));
            }

            // Scan for Kitty graphics sequences with per-chunk cursor positions.
            for (chunk, (cursor_col, cursor_row)) in chunks.iter().zip(per_chunk_cursors.iter()) {
                let result = self.kitty_parser.scan(
                    chunk,
                    *cursor_col,
                    *cursor_row,
                    &mut self.active_images,
                );
                if result.deleted {
                    // A delete command invalidates everything queued so far.
                    self.pending_images.clear();
                }
                self.pending_images.extend(result.placements);

                // Scan for Sixel graphics sequences (DCS q ... ST).
                let sixel_result = self.sixel_parser.scan(
                    chunk,
                    *cursor_col,
                    *cursor_row,
                    &mut self.active_images,
                );
                self.pending_images.extend(sixel_result.placements);
            }

            // Cap active images to prevent unbounded growth
            if self.active_images.len() > MAX_ACTIVE_IMAGES {
                let excess = self.active_images.len() - MAX_ACTIVE_IMAGES;
                self.active_images.drain(..excess);
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

    /// Drain the last completed command duration (from OSC 133;D).
    pub fn drain_command_completion(&mut self) -> Option<std::time::Duration> {
        self.last_command_duration.take()
    }

    /// Get the current shell semantic zone (from OSC 133).
    pub fn shell_zone(&self) -> SemanticZone {
        self.shell_state.zone
    }

    /// Get the last command exit code (from OSC 133;D).
    pub fn last_exit_code(&self) -> Option<i32> {
        self.shell_state.last_exit_code
    }

    /// Get the current working directory (from OSC 7).
    pub fn cwd(&self) -> Option<&str> {
        self.osc7_parser.cwd()
    }

    pub fn write_to_pty(&mut self, data: &[u8]) {
        if self.exited {
            return;
        }
        if let Err(e) = self.pty.write(data) {
            log::warn!("pty write failed (pane {}): {e}", self.id);
        }
    }

    /// Check if the terminal has mouse reporting mode enabled.
    pub fn has_mouse_mode(&self) -> bool {
        self.mode_flags_from_term(&self.term) & MODE_MOUSE_REPORT != 0
    }

    /// Forward mouse input as SGR escape sequence to the PTY.
    /// Does NOT reset viewport state (TUI apps manage their own scrolling).
    pub fn send_mouse_input(
        &mut self,
        button: u8,
        col: u16,
        row: u16,
        pressed: bool,
        modifiers: u8,
    ) {
        let btn_with_mods = button as u32 | ((modifiers as u32) << 2);
        let suffix = if pressed { b'M' } else { b'm' };
        // Stack-allocated buffer avoids heap allocation for every mouse event
        let mut buf = [0u8; 32];
        let len = {
            use std::io::Write;
            let mut cursor = std::io::Cursor::new(&mut buf[..]);
            write!(
                cursor,
                "\x1b[<{};{};{}{}",
                btn_with_mods,
                col + 1,
                row + 1,
                suffix as char
            )
            .unwrap();
            cursor.position() as usize
        };
        self.write_to_pty(&buf[..len]);
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if cols == self.cols && rows == self.rows {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        self.pty.resize(cols, rows);
        let size = TermSize {
            cols: cols as usize,
            rows: rows as usize,
        };
        self.term.resize(size);
        self.dirty = true;
    }

    pub fn grid_cols(&self) -> u16 {
        self.cols
    }
    pub fn grid_rows(&self) -> u16 {
        self.rows
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
    pub fn is_exited(&self) -> bool {
        self.exited
    }
    pub fn set_dirty(&mut self, dirty: bool) {
        self.dirty = dirty;
    }

    /// Extract damage metadata from the terminal. Returns None if no damage.
    /// Returns (line, left, right) tuples — cells are NOT read here (they are
    /// read at encoding time via `write_cells_into` to avoid intermediate allocations).
    /// Resets damage tracking after extraction.
    pub fn extract_damage(&mut self) -> Option<Vec<(u16, u16, u16)>> {
        let Some((total_rows, right)) = self.damage_bounds() else {
            self.term.reset_damage();
            return None;
        };

        // Determine which lines are damaged, consuming the TermDamage borrow.
        use alacritty_terminal::term::TermDamage;
        let ranges = match self.term.damage() {
            TermDamage::Full => full_damage_rows(total_rows, right),
            TermDamage::Partial(iter) => partial_damage_rows(iter, right),
        };

        self.term.reset_damage();
        if ranges.is_empty() {
            None
        } else {
            Some(ranges)
        }
    }

    /// Read cursor position, shape, and mode flags.
    pub fn cursor_info(&self) -> (i16, u16, u8, u8) {
        let term = &self.term;
        let content = term.renderable_content();
        let cursor_line = content.cursor.point.line.0 as i16;
        let cursor_col = content.cursor.point.column.0 as u16;
        let cursor_shape = cursor_shape_to_u8(content.cursor.shape);
        let mode_flags = self.mode_flags_from_term(term);
        (cursor_line, cursor_col, cursor_shape, mode_flags)
    }

    /// Current history size (number of scrollback lines).
    pub fn history_size(&self) -> usize {
        self.term.grid().history_size()
    }

    /// Create a full pane snapshot with incremental scrollback (only new lines since `history_sent`).
    pub fn snapshot_incremental(&self, generation: u64, history_sent: usize) -> FullPaneSync {
        let scrollback = SnapshotScrollback::incremental(self.term.grid().history_size(), history_sent);
        self.build_snapshot(generation, scrollback)
    }

    /// Create a full pane snapshot for StateSync / reattach.
    /// Sends all available scrollback — the client trims to its own `max_scrollback`.
    pub fn snapshot(&self, generation: u64) -> FullPaneSync {
        let scrollback = SnapshotScrollback::full(self.term.grid().history_size());
        self.build_snapshot(generation, scrollback)
    }

    /// Shared snapshot builder: reads viewport cells, scrollback, cursor, and mode flags.
    fn build_snapshot(&self, generation: u64, scrollback: SnapshotScrollback) -> FullPaneSync {
        let term = &self.term;
        let grid = term.grid();
        let cols = grid.columns();
        let rows = grid.screen_lines();
        let content = term.renderable_content();

        // Read viewport cells + collect grapheme extras for multi-codepoint chars
        let (cells, grapheme_extras) = collect_viewport_cells(grid, rows, cols);

        // Read scrollback lines (oldest first)
        let sb_cells = collect_scrollback_cells(grid, cols, scrollback.rows);

        // Collect hyperlink data from OSC 8 parser
        let hyperlink_extras = {
            let link_map = self.osc8_parser.link_map();
            if link_map.is_empty() {
                ciri_protocol::message::HyperlinkExtras::new()
            } else {
                ciri_protocol::message::HyperlinkExtras {
                    cell_links: Vec::new(), // Per-cell mapping requires terminal-level tracking
                    link_map: link_map.to_vec(),
                }
            }
        };

        FullPaneSync {
            pane_id: self.id,
            generation,
            cols: cols as u16,
            rows: rows as u16,
            cursor_line: content.cursor.point.line.0 as i16,
            cursor_col: content.cursor.point.column.0 as u16,
            cursor_shape: cursor_shape_to_u8(content.cursor.shape),
            mode_flags: self.mode_flags_from_term(term),
            title: self.title.clone(),
            scrollback: sb_cells,
            scrollback_rows: scrollback.rows as u16,
            cells,
            grapheme_extras,
            hyperlink_extras,
        }
    }

    fn damage_bounds(&self) -> Option<(usize, u16)> {
        let cols = self.term.grid().columns();
        let total_rows = self.term.grid().screen_lines();
        if cols == 0 || total_rows == 0 {
            None
        } else {
            Some((total_rows, cols.saturating_sub(1) as u16))
        }
    }

    /// Helper: compute mode flags from a term reference.
    fn mode_flags_from_term(&self, term: &Term<PtyEventListener>) -> u8 {
        use alacritty_terminal::term::TermMode;
        let mode = term.mode();
        let mut flags = 0u8;
        if mode.contains(TermMode::MOUSE_REPORT_CLICK)
            || mode.contains(TermMode::MOUSE_DRAG)
            || mode.contains(TermMode::MOUSE_MOTION)
        {
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
        if mode.contains(TermMode::BRACKETED_PASTE) {
            flags |= MODE_BRACKETED_PASTE;
        }
        if self.dec_mode_parser.focus_event_mode {
            flags |= MODE_FOCUS_EVENT;
        }
        if self.dec_mode_parser.sync_output_mode {
            flags |= MODE_SYNCHRONIZED_OUTPUT;
        }
        flags
    }

    /// Write a focus event escape sequence to the PTY (CSI I / CSI O).
    /// Only call this when the pane has DECSET 1004 enabled.
    pub fn write_focus_event(&mut self, focused: bool) {
        if self.dec_mode_parser.focus_event_mode {
            let seq = if focused { b"\x1b[I" } else { b"\x1b[O" };
            self.write_to_pty(seq);
        }
    }

    /// Whether this pane has focus event reporting (DECSET 1004) enabled.
    pub fn has_focus_event_mode(&self) -> bool {
        self.dec_mode_parser.focus_event_mode
    }

    /// Whether this pane has synchronized output (DEC 2026) enabled.
    pub fn is_sync_output(&self) -> bool {
        self.dec_mode_parser.sync_output_mode
    }

    /// Drain new image placements since last call (for broadcasting to clients).
    /// Active images are preserved for reconnecting clients.
    pub fn drain_images(&mut self) -> Vec<ImagePlacement> {
        std::mem::take(&mut self.pending_images)
    }

    /// Get all currently active image placements (for full sync on client reconnect).
    pub fn active_images(&self) -> &[ImagePlacement] {
        &self.active_images
    }

    /// Push packed cells for a line range into a state-machine encoder.
    /// Cells go from grid → PackedCell → StateEncoder opcode stream.
    pub fn write_cells_into_sm(
        &self,
        line: u16,
        left: u16,
        right: u16,
        encoder: &mut ciri_protocol::codec::StateEncoder,
    ) {
        let grid = self.term.grid();
        for col in left..=right {
            let point = Point::new(Line(line as i32), Column(col as usize));
            let packed = pack_cell(&grid[point]);
            encoder.push_cell(&packed);
        }
    }
}

/// Pack an alacritty cell into our wire format.
pub fn pack_cell(cell: &alacritty_terminal::term::cell::Cell) -> PackedCell {
    let fg = pack_color(cell.fg);
    let bg = pack_color(cell.bg);
    let mut flags = 0u16;
    if cell.flags.contains(CellFlags::WIDE_CHAR) {
        flags |= FLAG_WIDE_CHAR;
    }
    if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
        flags |= FLAG_WIDE_CHAR_SPACER;
    }
    if cell.flags.contains(CellFlags::BOLD) {
        flags |= FLAG_BOLD;
    }
    if cell.flags.contains(CellFlags::ITALIC) {
        flags |= FLAG_ITALIC;
    }
    // Preserve underline style variants
    if cell.flags.contains(CellFlags::DOUBLE_UNDERLINE) {
        flags |= FLAG_UNDERLINE | FLAG_UNDERLINE_DOUBLE;
    } else if cell.flags.contains(CellFlags::UNDERCURL) {
        flags |= FLAG_UNDERLINE | FLAG_UNDERLINE_CURLY;
    } else if cell.flags.contains(CellFlags::DOTTED_UNDERLINE) {
        flags |= FLAG_UNDERLINE | FLAG_UNDERLINE_DOTTED;
    } else if cell.flags.contains(CellFlags::DASHED_UNDERLINE) {
        flags |= FLAG_UNDERLINE | FLAG_UNDERLINE_DASHED;
    } else if cell.flags.contains(CellFlags::ALL_UNDERLINES) {
        flags |= FLAG_UNDERLINE;
    }
    if cell.flags.contains(CellFlags::INVERSE) {
        flags |= FLAG_INVERSE;
    }
    if cell.flags.contains(CellFlags::DIM) {
        flags |= FLAG_DIM;
    }
    if cell.flags.contains(CellFlags::STRIKEOUT) {
        flags |= FLAG_STRIKEOUT;
    }
    if cell.flags.contains(CellFlags::HIDDEN) {
        flags |= FLAG_HIDDEN;
    }
    if cell.flags.contains(CellFlags::WRAPLINE) {
        flags |= FLAG_WRAPLINE;
    }
    let mut packed = PackedCell {
        ch_bytes: [0; 4],
        fg,
        bg,
        flags: flags.to_le_bytes(),
    };
    packed.set_ch(cell.c);
    packed
}

fn full_damage_rows(total_rows: usize, right: u16) -> Vec<(u16, u16, u16)> {
    let mut ranges = Vec::with_capacity(total_rows);
    for row in 0..total_rows {
        ranges.push((row as u16, 0u16, right));
    }
    ranges
}

fn partial_damage_rows(
    iter: impl IntoIterator<Item = alacritty_terminal::term::LineDamageBounds>,
    right: u16,
) -> Vec<(u16, u16, u16)> {
    let mut seen = [false; 256];
    let mut ranges = Vec::new();
    for bounds in iter {
        let line = bounds.line;
        if line < 256 {
            if seen[line] {
                continue;
            }
            seen[line] = true;
        }
        ranges.push((line as u16, 0u16, right));
    }
    ranges
}

fn collect_viewport_cells(
    grid: &alacritty_terminal::grid::Grid<alacritty_terminal::term::cell::Cell>,
    rows: usize,
    cols: usize,
) -> (Vec<PackedCell>, ciri_protocol::message::GraphemeExtras) {
    let mut cells = Vec::with_capacity(cols * rows);
    let mut grapheme_extras = ciri_protocol::message::GraphemeExtras::new();

    for row in 0..rows {
        for col in 0..cols {
            let point = Point::new(Line(row as i32), Column(col));
            let cell = &grid[point];
            let cell_idx = (row * cols + col) as u32;
            cells.push(pack_cell(cell));
            if let Some(zw) = cell.zerowidth()
                && !zw.is_empty()
            {
                let extra: String = zw.iter().collect();
                grapheme_extras.push(cell_idx, &extra);
            }
        }
    }

    (cells, grapheme_extras)
}

fn collect_scrollback_cells(
    grid: &alacritty_terminal::grid::Grid<alacritty_terminal::term::cell::Cell>,
    cols: usize,
    scrollback_rows: usize,
) -> Vec<PackedCell> {
    let mut cells = Vec::new();
    if scrollback_rows > 0 {
        cells.reserve(scrollback_rows * cols);
        for row_offset in (1..=scrollback_rows).rev() {
            for col in 0..cols {
                let point = Point::new(Line(-(row_offset as i32)), Column(col));
                cells.push(pack_cell(&grid[point]));
            }
        }
    }
    cells
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
        Black => 0,
        Red => 1,
        Green => 2,
        Yellow => 3,
        Blue => 4,
        Magenta => 5,
        Cyan => 6,
        White => 7,
        BrightBlack => 8,
        BrightRed => 9,
        BrightGreen => 10,
        BrightYellow => 11,
        BrightBlue => 12,
        BrightMagenta => 13,
        BrightCyan => 14,
        BrightWhite => 15,
        Foreground => 16,
        Background => 17,
        Cursor => 18,
        DimBlack => 19,
        DimRed => 20,
        DimGreen => 21,
        DimYellow => 22,
        DimBlue => 23,
        DimMagenta => 24,
        DimCyan => 25,
        DimWhite => 26,
        BrightForeground => 27,
        DimForeground => 28,
        _ => 16, // fallback to Foreground
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell_path() -> &'static str {
        if std::path::Path::new("/bin/sh").exists() {
            "/bin/sh"
        } else {
            "sh"
        }
    }

    fn new_test_pane() -> Pane {
        Pane::new(7, 4, 3, shell_path()).expect("create test pane")
    }

    #[test]
    fn extract_damage_resets_after_read() {
        let mut pane = new_test_pane();

        let first = pane.extract_damage().expect("new pane should start dirty");
        assert_eq!(first, vec![(0, 0, 3), (1, 0, 3), (2, 0, 3)]);

        let second = pane.extract_damage().expect("alacritty keeps the cursor line dirty");
        assert_eq!(second, vec![(0, 0, 3)]);

        let third = pane.extract_damage().expect("cursor line damage remains stable after reset");
        assert_eq!(third, vec![(0, 0, 3)]);
    }

    #[test]
    fn snapshot_incremental_only_includes_new_scrollback() {
        let pane = new_test_pane();

        let none_sent = pane.snapshot_incremental(11, 0);
        let over_sent = pane.snapshot_incremental(12, 99);

        assert_eq!(none_sent.scrollback_rows, 0);
        assert!(none_sent.scrollback.is_empty());
        assert_eq!(over_sent.scrollback_rows, 0);
        assert!(over_sent.scrollback.is_empty());
    }

    #[test]
    fn drain_images_leaves_active_images_available_for_reconnect() {
        let mut pane = new_test_pane();
        let image = ImagePlacement {
            id: 1,
            row: 2,
            col: 3,
            width_cells: 4,
            height_cells: 5,
            pixel_width: 6,
            pixel_height: 7,
            format: "png".into(),
            data: Arc::new(vec![1, 2, 3]),
        };

        pane.pending_images.push(image.clone());
        pane.active_images.push(image.clone());

        let drained = pane.drain_images();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].id, image.id);
        assert_eq!(pane.active_images().len(), 1);
        assert_eq!(pane.active_images()[0].id, image.id);
        assert!(pane.drain_images().is_empty());
    }
}
