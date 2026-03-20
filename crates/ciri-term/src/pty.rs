use anyhow::{Context, Result};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

/// Cross-platform PTY wrapper using portable-pty.
/// Uses a background reader thread because portable-pty's reader is blocking.
pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
    /// Channel receiving PTY output from the reader thread.
    output_rx: mpsc::Receiver<Vec<u8>>,
    /// Set to true when the reader thread detects EOF.
    reader_done: Arc<AtomicBool>,
}

/// Query the user's login shell via getpwuid_r (thread-safe).
#[cfg(unix)]
fn get_pw_shell() -> Option<String> {
    let uid = unsafe { libc::getuid() };
    let mut buf = vec![0u8; 4096];
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result = std::ptr::null_mut();
    let ret = unsafe {
        libc::getpwuid_r(
            uid,
            &mut pwd,
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
            &mut result,
        )
    };
    if ret != 0 || result.is_null() {
        return None;
    }
    let shell = unsafe { std::ffi::CStr::from_ptr(pwd.pw_shell) };
    shell.to_str().ok().map(|s| s.to_string())
}

impl Pty {
    pub fn spawn(cols: u16, rows: u16, shell: &str) -> Result<Self> {
        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("openpty failed")?;

        let mut cmd = if !shell.is_empty() {
            CommandBuilder::new(shell)
        } else if cfg!(windows) {
            CommandBuilder::new("cmd.exe")
        } else {
            let default = std::env::var("SHELL")
                .ok()
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    #[cfg(unix)]
                    {
                        get_pw_shell()
                    }
                    #[cfg(not(unix))]
                    {
                        None
                    }
                })
                .unwrap_or_else(|| "/bin/sh".to_string());
            CommandBuilder::new(default)
        };

        // Ensure child knows its terminal type.
        cmd.env("TERM", "xterm-256color");
        if std::env::var_os("COLORTERM").is_none() {
            cmd.env("COLORTERM", "truecolor");
        }

        // pair.master is the PTY master fd

        let child = pair.slave.spawn_command(cmd).context("spawn failed")?;
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .context("clone reader failed")?;
        let writer = pair.master.take_writer().context("take writer failed")?;

        // Spawn background reader thread with bounded channel to prevent
        // memory spikes when PTY output exceeds processing bandwidth (e.g. cat large_file).
        // 8 slots × 64KB buffer = ~512KB max buffered.
        let (output_tx, output_rx) = mpsc::sync_channel(8);
        let reader_done = Arc::new(AtomicBool::new(false));
        let reader_done_clone = reader_done.clone();

        std::thread::Builder::new()
            .name("pty-reader".into())
            .spawn(move || {
                let mut buf = [0u8; 65536];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => {
                            reader_done_clone.store(true, Ordering::Release);
                            break;
                        }
                        Ok(n) => {
                            if output_tx.send(buf[..n].to_vec()).is_err() {
                                break; // Receiver dropped
                            }
                        }
                        Err(_) => {
                            reader_done_clone.store(true, Ordering::Release);
                            break;
                        }
                    }
                }
            })
            .context("failed to spawn pty reader thread")?;

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
        self.reader_done.load(Ordering::Acquire)
    }

    /// Write data to the PTY.
    pub fn write(&self, data: &[u8]) -> std::io::Result<()> {
        self.writer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .write_all(data)
    }

    /// Check if child has exited (non-blocking).
    pub fn try_wait(&self) -> bool {
        self.child
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .try_wait()
            .ok()
            .flatten()
            .is_some()
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        if let Err(e) = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        }) {
            log::warn!("pty resize failed: {e}");
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // Signal reader thread to stop by dropping the master fd.
        // The reader will get an error/EOF on the next read() and exit.
        // We also kill the child process to avoid orphans.
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
        // The reader thread will exit once the master fd is dropped (which
        // happens when `self.master` is dropped after this method returns).
        // We intentionally do NOT join the thread here to avoid blocking
        // the UI — the thread will exit on its own when read() returns an error.
    }
}
