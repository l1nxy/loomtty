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
    pub term: Arc<Mutex<Term<PtyEventListener>>>,
    dirty: bool,
    exited: bool,
    pty: Pty,
    processor: Processor,
    event_rx: mpsc::Receiver<Event>,
    cols: u16,
    rows: u16,
    pub title: String,
}

impl Pane {
    pub fn new(id: PaneId, cols: u16, rows: u16, shell: &str) -> Result<Self> {
        let pty = Pty::spawn(cols, rows, shell)?;

        let size = TermSize { cols: cols as usize, rows: rows as usize };
        let config = TermConfig::default();
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
            let mut term = self.term.lock().unwrap_or_else(|e| e.into_inner());
            for chunk in &chunks {
                self.processor.advance(&mut *term, chunk);
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

    pub fn write_to_pty(&self, data: &[u8]) {
        if self.exited { return; }
        if let Err(e) = self.pty.write(data) {
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
                let mut regions = Vec::with_capacity(bounds.len());
                for b in bounds {
                    let cells = self.read_line_cells(&term, b.line, b.left, b.right);
                    regions.push(DamageRegion {
                        line: b.line as u16,
                        left: b.left as u16,
                        right: b.right as u16,
                        cells,
                    });
                }
                regions
            }
        };
        term.reset_damage();
        Some(regions)
    }

    /// Read cells from a line range and pack them.
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

        let cursor_shape = match content.cursor.shape {
            CursorShape::Block => CURSOR_BLOCK,
            CursorShape::Underline => CURSOR_UNDERLINE,
            CursorShape::Beam => CURSOR_BEAM,
            CursorShape::HollowBlock => CURSOR_HOLLOW_BLOCK,
            CursorShape::Hidden => CURSOR_HIDDEN,
        };

        FullPaneSync {
            pane_id: self.id,
            generation,
            cols: cols as u16,
            rows: rows as u16,
            cursor_line: content.cursor.point.line.0 as i16,
            cursor_col: content.cursor.point.column.0 as u16,
            cursor_shape,
            title: self.title.clone(),
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
