use alacritty_terminal::event::Event;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::Config as TermConfig;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::Processor;
use anyhow::Result;
use std::os::fd::AsRawFd;
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
    pub fn new(id: PaneId, cols: u16, rows: u16) -> Result<Self> {
        let pty = Pty::spawn(cols, rows)?;

        let size = TermSize { cols: cols as usize, rows: rows as usize };
        let config = TermConfig::default();
        let (event_listener, event_rx) = PtyEventListener::new();
        let term = Term::new(config, &size, event_listener);

        let fd = pty.master_fd().as_raw_fd();
        let flags = nix::fcntl::fcntl(fd, nix::fcntl::FcntlArg::F_GETFL)?;
        let mut oflags = nix::fcntl::OFlag::from_bits_truncate(flags);
        oflags.insert(nix::fcntl::OFlag::O_NONBLOCK);
        nix::fcntl::fcntl(fd, nix::fcntl::FcntlArg::F_SETFL(oflags))?;

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

        let fd = self.pty.master_fd().as_raw_fd();
        let mut buf = [0u8; 65536];
        let mut processed = false;

        loop {
            match nix::unistd::read(fd, &mut buf) {
                Ok(0) => { self.exited = true; break; }
                Ok(n) => {
                    let mut term = self.term.lock().unwrap();
                    for byte in &buf[..n] {
                        self.processor.advance(&mut *term, *byte);
                    }
                    processed = true;
                }
                Err(nix::errno::Errno::EAGAIN) => break,
                Err(nix::errno::Errno::EIO) => { self.exited = true; break; }
                Err(_) => { self.exited = true; break; }
            }
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
                _ => {} // Bell, Title, etc. — handle later
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
        let fd = self.pty.master_fd();
        let _ = nix::unistd::write(fd, data);
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
