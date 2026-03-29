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

use crate::event::PtyEventListener;
use crate::image_store::ImageStore;
use crate::parser_suite::ParserSuite;
use crate::pending_events::PendingEvents;
use crate::pty::Pty;

pub type PaneId = u64;

/// Semantic zone type from OSC 133 shell integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticZone {
    Prompt,
    Input,
    Output,
}

/// An inline image placement in the terminal grid.
#[derive(Debug, Clone)]
pub struct ImagePlacement {
    pub id: u64,
    pub row: u16,
    pub col: u16,
    pub width_cells: u16,
    pub height_cells: u16,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub format: String,
    pub data: Arc<Vec<u8>>,
}

/// Shell integration state tracked via OSC 133.
#[derive(Debug, Clone)]
pub struct ShellState {
    pub zone: SemanticZone,
    pub last_exit_code: Option<i32>,
    pub prompt_line: Option<i32>,
    pub output_line: Option<i32>,
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

// ─── Pane ────────────────────────────────────────────────────────────

pub struct Pane {
    pub id: PaneId,
    pub title: String,
    pub shell_state: ShellState,

    // Core terminal
    term: Term<PtyEventListener>,
    processor: Processor,
    event_rx: mpsc::Receiver<Event>,
    pty: Pty,

    // Dimensions
    cols: u16,
    rows: u16,

    // State
    dirty: bool,
    exited: bool,

    // Scrollback tracking
    /// Total number of lines ever added to scrollback (monotonically increasing).
    /// Unlike `history_size()` which is bounded by the ring buffer capacity,
    /// this counter keeps growing when old lines are evicted by new ones.
    scrollback_total: usize,
    /// Previous `history_size()` value, used to detect growth vs rotation.
    prev_history_size: usize,

    // Subsystems
    parsers: ParserSuite,
    images: ImageStore,
    events: PendingEvents,
}

impl Pane {
    pub fn new(id: PaneId, cols: u16, rows: u16, shell: &str) -> Result<Self> {
        Self::new_with_opts(id, cols, rows, shell, None, None)
    }

    pub fn new_with_cwd(
        id: PaneId,
        cols: u16,
        rows: u16,
        shell: &str,
        cwd: Option<&std::path::Path>,
    ) -> Result<Self> {
        Self::new_with_opts(id, cols, rows, shell, None, cwd)
    }

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
            title: String::new(),
            shell_state: ShellState {
                zone: SemanticZone::Prompt,
                last_exit_code: None,
                prompt_line: None,
                output_line: None,
                command_start: None,
            },
            term,
            processor: Processor::new(),
            event_rx,
            pty,
            cols,
            rows,
            dirty: true,
            exited: false,
            scrollback_total: 0,
            prev_history_size: 0,
            parsers: ParserSuite::new(),
            images: ImageStore::new(),
            events: PendingEvents::new(),
        })
    }

    // ── PTY I/O ──────────────────────────────────────────────────────

    pub fn process_pty_output(&mut self) -> bool {
        if self.exited {
            return false;
        }

        // Snapshot Line(-1) row hash before processing to detect ring buffer rotation.
        let prev_sb_hash = if self.prev_history_size > 0 && !self.is_alt_screen() {
            Some(self.hash_scrollback_top())
        } else {
            None
        };

        let data_processed = self.drain_and_parse_pty();
        self.process_terminal_events();
        self.check_pty_exit();

        if data_processed {
            self.dirty = true;
            self.track_scrollback_growth(prev_sb_hash);
        }
        data_processed
    }

    /// Fast hash of the most recent scrollback row (Line(-1)) for rotation detection.
    /// Samples up to 8 evenly-spaced columns to keep cost bounded on wide terminals.
    fn hash_scrollback_top(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let cols = self.term.grid().columns();
        // Sample up to 8 columns across the row for a fast but reliable fingerprint.
        let step = (cols / 8).max(1);
        let mut col = 0;
        while col < cols {
            let cell = &self.term.grid()[Point::new(Line(-1), Column(col))];
            cell.c.hash(&mut hasher);
            col += step;
        }
        hasher.finish()
    }

    /// Detect scrollback growth or ring buffer rotation and update the
    /// monotonic `scrollback_total` counter accordingly.
    fn track_scrollback_growth(&mut self, prev_sb_hash: Option<u64>) {
        if self.is_alt_screen() {
            return;
        }
        let hs = self.term.grid().history_size();
        if hs > self.prev_history_size {
            // Growing phase: exact count available.
            self.scrollback_total += hs - self.prev_history_size;
        } else if hs > 0 && hs == self.prev_history_size {
            // Buffer may be at capacity. Detect rotation by checking
            // whether the most recent scrollback row changed.
            let cur_hash = self.hash_scrollback_top();
            if prev_sb_hash.is_some_and(|prev| prev != cur_hash) {
                // Line(-1) content changed — scrollback rotated.
                // We can't know the exact count from alacritty's API.
                // Increment by 1 per tick — this may under-count rapid output
                // but is self-correcting: each subsequent tick detects the
                // still-changed hash and increments again, eventually catching up.
                // Using a larger estimate (e.g. viewport rows) would over-count
                // and trigger unnecessary full scrollback replacements.
                self.scrollback_total += 1;
            }
        }
        self.prev_history_size = hs;
    }

    fn drain_and_parse_pty(&mut self) -> bool {
        let chunks = self.pty.drain_output();
        if chunks.is_empty() {
            return false;
        }

        // Run non-image parsers on each chunk
        for chunk in &chunks {
            self.parsers.scan_control(
                chunk,
                &mut self.shell_state,
                &mut self.events.command_completion,
            );
        }

        // VT-parse each chunk, recording cursor position after each
        let cursors: Vec<(u16, u16)> = chunks
            .iter()
            .map(|chunk| {
                self.processor.advance(&mut self.term, chunk);
                let cursor = self.term.grid().cursor.point;
                (cursor.column.0 as u16, cursor.line.0.max(0) as u16)
            })
            .collect();

        // Run image parsers with per-chunk cursor positions
        for (chunk, &(cursor_col, cursor_row)) in chunks.iter().zip(cursors.iter()) {
            let (kitty_result, sixel_placements) = self.parsers.scan_images(
                chunk,
                cursor_col,
                cursor_row,
                self.images.active_mut(),
            );
            if kitty_result.deleted {
                self.images.clear_on_delete();
            }
            self.images.add_placements(kitty_result.placements);
            self.images.add_placements(sixel_placements);
        }

        self.images.cap_active();
        true
    }

    fn process_terminal_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                Event::PtyWrite(text) => self.write_to_pty(text.as_bytes()),
                Event::Title(t) => self.title = t,
                Event::ResetTitle => self.title.clear(),
                Event::ClipboardStore(_, text) => self.events.clipboard.push(text),
                Event::ClipboardLoad(_, formatter) => {
                    let response = formatter("");
                    self.write_to_pty(response.as_bytes());
                }
                Event::Bell => self.events.bell = true,
                Event::Exit | Event::ChildExit(_) => self.exited = true,
                _ => {}
            }
        }
    }

    fn check_pty_exit(&mut self) {
        if !self.exited && self.pty.reader_eof() {
            self.exited = true;
        }
        if !self.exited && self.pty.try_wait() {
            self.exited = true;
        }
    }

    // ── Drain methods ────────────────────────────────────────────────

    pub fn drain_clipboard(&mut self) -> Vec<String> {
        self.events.drain_clipboard()
    }

    pub fn drain_bell(&mut self) -> bool {
        self.events.drain_bell()
    }

    pub fn drain_command_completion(&mut self) -> Option<std::time::Duration> {
        self.events.drain_command_completion()
    }

    pub fn drain_images(&mut self) -> Vec<ImagePlacement> {
        self.images.drain_pending()
    }

    pub fn active_images(&self) -> &[ImagePlacement] {
        self.images.active()
    }

    // ── Shell integration ────────────────────────────────────────────

    pub fn shell_zone(&self) -> SemanticZone {
        self.shell_state.zone
    }

    pub fn last_exit_code(&self) -> Option<i32> {
        self.shell_state.last_exit_code
    }

    pub fn cwd(&self) -> Option<&str> {
        self.parsers.osc7.cwd()
    }

    // ── PTY write ────────────────────────────────────────────────────

    pub fn write_to_pty(&mut self, data: &[u8]) {
        if self.exited {
            return;
        }
        if let Err(e) = self.pty.write(data) {
            log::warn!("pty write failed (pane {}): {e}", self.id);
        }
    }

    // ── Mouse ────────────────────────────────────────────────────────

    pub fn has_mouse_mode(&self) -> bool {
        self.mode_flags_from_term(&self.term) & MODE_MOUSE_REPORT != 0
    }

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

    // ── Resize & grid ────────────────────────────────────────────────

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if cols == self.cols && rows == self.rows {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        self.pty.resize(cols, rows);
        let hs_before = self.term.grid().history_size();
        self.term.resize(TermSize {
            cols: cols as usize,
            rows: rows as usize,
        });
        let hs_after = self.term.grid().history_size();
        // Reflow may add or remove scrollback lines. Adjust the monotonic
        // counter so incremental sync stays consistent.
        if hs_after > hs_before {
            self.scrollback_total += hs_after - hs_before;
        } else if hs_after < hs_before {
            self.scrollback_total = self.scrollback_total.saturating_sub(hs_before - hs_after);
        }
        self.prev_history_size = hs_after;
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

    // ── Damage & cursor ──────────────────────────────────────────────

    pub fn extract_damage(&mut self) -> Option<Vec<(u16, u16, u16)>> {
        let Some((total_rows, right)) = self.damage_bounds() else {
            self.term.reset_damage();
            return None;
        };

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

    pub fn cursor_info(&self) -> (i16, u16, u8, u8) {
        let content = self.term.renderable_content();
        let cursor_line = content.cursor.point.line.0 as i16;
        let cursor_col = content.cursor.point.column.0 as u16;
        let cursor_shape = cursor_shape_to_u8(content.cursor.shape);
        let mode_flags = self.mode_flags_from_term(&self.term);
        (cursor_line, cursor_col, cursor_shape, mode_flags)
    }

    pub fn history_size(&self) -> usize {
        self.term.grid().history_size()
    }

    /// Monotonically increasing count of total lines ever added to scrollback.
    /// Unlike `history_size()` which is bounded by the ring buffer capacity,
    /// this counter keeps growing when old lines are evicted by new ones.
    pub fn scrollback_total(&self) -> usize {
        self.scrollback_total
    }

    pub fn is_alt_screen(&self) -> bool {
        use alacritty_terminal::term::TermMode;
        self.term.mode().contains(TermMode::ALT_SCREEN)
    }

    // ── Mode flags ───────────────────────────────────────────────────

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
        if self.shell_state.prompt_line.is_some() {
            flags |= MODE_SHELL_INTEGRATION;
        }
        if mode.contains(TermMode::DISAMBIGUATE_ESC_CODES) {
            flags |= MODE_KITTY_KEYBOARD;
        }
        if mode.contains(TermMode::BRACKETED_PASTE) {
            flags |= MODE_BRACKETED_PASTE;
        }
        if self.parsers.dec_mode.focus_event_mode {
            flags |= MODE_FOCUS_EVENT;
        }
        if self.parsers.dec_mode.sync_output_mode {
            flags |= MODE_SYNCHRONIZED_OUTPUT;
        }
        flags
    }

    pub fn write_focus_event(&mut self, focused: bool) {
        if self.parsers.dec_mode.focus_event_mode {
            let seq = if focused { b"\x1b[I" } else { b"\x1b[O" };
            self.write_to_pty(seq);
        }
    }

    pub fn has_focus_event_mode(&self) -> bool {
        self.parsers.dec_mode.focus_event_mode
    }

    pub fn is_sync_output(&self) -> bool {
        self.parsers.dec_mode.sync_output_mode
    }

    // ── Snapshots ────────────────────────────────────────────────────

    pub fn snapshot_incremental(&self, generation: u64, history_sent: usize) -> FullPaneSync {
        let scrollback =
            SnapshotScrollback::incremental(self.term.grid().history_size(), history_sent);
        self.build_snapshot(generation, scrollback)
    }

    pub fn snapshot(&self, generation: u64) -> FullPaneSync {
        let scrollback = SnapshotScrollback::full(self.term.grid().history_size());
        self.build_snapshot(generation, scrollback)
    }

    fn build_snapshot(&self, generation: u64, scrollback: SnapshotScrollback) -> FullPaneSync {
        let term = &self.term;
        let grid = term.grid();
        let cols = grid.columns();
        let rows = grid.screen_lines();
        let content = term.renderable_content();

        let (cells, grapheme_extras) = collect_viewport_cells(grid, rows, cols);
        let sb_cells = collect_scrollback_cells(grid, cols, scrollback.rows);

        let hyperlink_extras = {
            let link_map = self.parsers.osc8.link_map();
            if link_map.is_empty() {
                HyperlinkExtras::new()
            } else {
                HyperlinkExtras {
                    cell_links: Vec::new(),
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
            scrollback_rows: scrollback.rows as u32,
            scrollback_replace: false,
            cells,
            grapheme_extras,
            hyperlink_extras,
            cwd: self.cwd().map(|s| s.to_string()),
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

    // ── Cell encoding ────────────────────────────────────────────────

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
            encoder.push_cell(&pack_cell(&grid[point]));
        }
    }
}

// ─── Free functions ──────────────────────────────────────────────────

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
    (0..total_rows)
        .map(|row| (row as u16, 0u16, right))
        .collect()
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
) -> (Vec<PackedCell>, GraphemeExtras) {
    let mut cells = Vec::with_capacity(cols * rows);
    let mut grapheme_extras = GraphemeExtras::new();

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
    if scrollback_rows == 0 {
        return Vec::new();
    }
    let mut cells = Vec::with_capacity(scrollback_rows * cols);
    for row_offset in (1..=scrollback_rows).rev() {
        for col in 0..cols {
            let point = Point::new(Line(-(row_offset as i32)), Column(col));
            cells.push(pack_cell(&grid[point]));
        }
    }
    cells
}

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
        _ => 16,
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

        pane.images.add_placements(vec![image.clone()]);
        pane.images.active_mut().push(image.clone());

        let drained = pane.drain_images();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].id, image.id);
        assert_eq!(pane.active_images().len(), 1);
        assert_eq!(pane.active_images()[0].id, image.id);
        assert!(pane.drain_images().is_empty());
    }
}
