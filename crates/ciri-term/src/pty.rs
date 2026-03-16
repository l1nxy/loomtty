use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc;
use std::sync::Mutex;

/// Cross-platform PTY wrapper using portable-pty.
/// Uses a background reader thread because portable-pty's reader is blocking.
pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
    /// Channel receiving PTY output from the reader thread.
    output_rx: mpsc::Receiver<Vec<u8>>,
    /// Set to true when the reader thread detects EOF.
    reader_done: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Pty {
    pub fn spawn(cols: u16, rows: u16) -> Result<Self> {
        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .context("openpty failed")?;

        let shell = if cfg!(windows) {
            CommandBuilder::new("cmd.exe")
        } else {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
            CommandBuilder::new(shell)
        };

        let child = pair.slave.spawn_command(shell).context("spawn failed")?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().context("clone reader failed")?;
        let writer = pair.master.take_writer().context("take writer failed")?;

        // Spawn background reader thread
        let (output_tx, output_rx) = mpsc::channel();
        let reader_done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let reader_done_clone = reader_done.clone();

        std::thread::spawn(move || {
            let mut buf = [0u8; 65536];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        reader_done_clone.store(true, std::sync::atomic::Ordering::Relaxed);
                        break;
                    }
                    Ok(n) => {
                        if output_tx.send(buf[..n].to_vec()).is_err() {
                            break; // Receiver dropped
                        }
                    }
                    Err(_) => {
                        reader_done_clone.store(true, std::sync::atomic::Ordering::Relaxed);
                        break;
                    }
                }
            }
        });

        Ok(Pty {
            master: pair.master,
            writer: Mutex::new(writer),
            child: Mutex::new(child),
            output_rx,
            reader_done,
        })
    }

    /// Drain all available output from the reader thread (non-blocking).
    pub fn drain_output(&self) -> Vec<Vec<u8>> {
        let mut chunks = Vec::new();
        while let Ok(chunk) = self.output_rx.try_recv() {
            chunks.push(chunk);
        }
        chunks
    }

    /// Check if the reader thread has finished (EOF).
    pub fn reader_eof(&self) -> bool {
        self.reader_done.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Write data to the PTY.
    pub fn write(&self, data: &[u8]) -> std::io::Result<usize> {
        self.writer.lock().unwrap().write(data)
    }

    /// Check if child has exited (non-blocking).
    pub fn try_wait(&self) -> bool {
        self.child.lock().unwrap().try_wait().ok().flatten().is_some()
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let _ = self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
    }
}
