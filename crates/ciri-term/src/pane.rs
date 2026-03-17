use alacritty_terminal::event::Event;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::Config as TermConfig;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::Processor;
use anyhow::Result;
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
    pub dirty: bool,
    pub exited: bool,
    pty: Pty,
    processor: Processor,
    event_rx: mpsc::Receiver<Event>,
    cols: u16,
    rows: u16,
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
            let mut term = self.term.lock().unwrap();
            for chunk in &chunks {
                for byte in chunk {
                    self.processor.advance(&mut *term, *byte);
                }
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
        let _ = self.pty.write(data);
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if cols == self.cols && rows == self.rows { return; }
        self.cols = cols;
        self.rows = rows;
        self.pty.resize(cols, rows);
        let size = TermSize { cols: cols as usize, rows: rows as usize };
        let mut term = self.term.lock().unwrap();
        term.resize(size);
        self.dirty = true;
    }
}
