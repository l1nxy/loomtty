use anyhow::{Context, Result};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

/// Cross-platform PTY wrapper using portable-pty.
/// Uses a background reader thread because portable-pty's reader is blocking.
pub struct Pty {
    /// Option so Drop can take it and control shutdown order on Windows.
    /// On Windows, MasterPty::drop calls ClosePseudoConsole which blocks until
    /// the ConPTY output pipe is fully consumed — we must drain the channel
    /// concurrently to prevent deadlock (see Drop impl).
    master: Option<Box<dyn MasterPty + Send>>,
    /// PTY input handle. Must stay alive until after ClosePseudoConsole —
    /// closing it early destroys the console and force-kills the child.
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
        Self::spawn_with_opts(cols, rows, shell, None, None)
    }

    /// Spawn a PTY with optional command override and working directory.
    pub fn spawn_with_opts(
        cols: u16,
        rows: u16,
        shell: &str,
        command: Option<&str>,
        cwd: Option<&std::path::Path>,
    ) -> Result<Self> {
        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("openpty failed")?;

        // Determine the program to run:
        // 1. If command is provided and non-empty, use it
        // 2. Otherwise fall back to shell / $SHELL / getpwuid
        let mut cmd = if let Some(c) = command.filter(|c| !c.is_empty()) {
            let builder = if cfg!(windows) {
                let mut b = CommandBuilder::new("cmd.exe");
                b.arg("/C");
                b.arg(c);
                b
            } else {
                let mut b = CommandBuilder::new("sh");
                b.arg("-c");
                b.arg(c);
                b
            };
            builder
        } else if !shell.is_empty() {
            CommandBuilder::new(shell)
        } else if cfg!(windows) {
            CommandBuilder::new("cmd.exe")
        } else {
            let shell_path = std::env::var("SHELL")
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
            CommandBuilder::new(shell_path)
        };

        // Ensure child knows its terminal type.
        cmd.env("TERM", "xterm-256color");
        if std::env::var_os("COLORTERM").is_none() {
            cmd.env("COLORTERM", "truecolor");
        }

        // Set TERM_PROGRAM so shells can detect they are inside Ciri.
        cmd.env("TERM_PROGRAM", "ciri");
        cmd.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));

        // Shell integration: set env vars so shells auto-source integration scripts.
        if let Ok(integration_dir) = std::env::var("CIRI_SHELL_INTEGRATION_DIR") {
            cmd.env("CIRI_SHELL_INTEGRATION_DIR", &integration_dir);

            // Bash: BASH_ENV is only sourced by non-interactive bash (scripts,
            // subshells). For interactive shells, users should add to .bashrc:
            //   [[ -n "$CIRI_SHELL_INTEGRATION_DIR" ]] && source "$CIRI_SHELL_INTEGRATION_DIR/ciri.bash"
            let bash_script = format!("{}/ciri.bash", integration_dir);
            cmd.env("BASH_ENV", &bash_script);

            // Fish: XDG_DATA_DIRS-based vendor_conf.d is complex; rely on
            // CIRI_SHELL_INTEGRATION_DIR env var + TERM_PROGRAM detection
            // for manual sourcing or use the fish integration event.
        }

        // Set working directory if provided.
        if let Some(dir) = cwd
            && dir.is_dir()
        {
            cmd.cwd(dir);
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
            master: Some(pair.master),
            writer: Mutex::new(writer),
            child: Mutex::new(child),
            output_rx,
            reader_done,
        })
    }

    /// Spawn with just a CWD override (convenience for OSC 7 CWD inheritance).
    pub fn spawn_with_cwd(
        cols: u16,
        rows: u16,
        shell: &str,
        cwd: Option<&std::path::Path>,
    ) -> Result<Self> {
        Self::spawn_with_opts(cols, rows, shell, None, cwd)
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

    /// Get the child shell's PID.
    pub fn child_pid(&self) -> Option<u32> {
        self.child
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .process_id()
    }

    /// Get the master PTY file descriptor (for `tcgetpgrp` on Unix).
    #[cfg(unix)]
    pub fn master_raw_fd(&self) -> Option<std::os::unix::io::RawFd> {
        self.master.as_ref().and_then(|m| m.as_raw_fd())
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        if let Some(ref master) = self.master
            && let Err(e) = master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
        {
            log::warn!("pty resize failed: {e}");
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // Kill child process to avoid orphans.
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }

        if let Some(master) = self.master.take() {
            if cfg!(windows) {
                // On Windows, MasterPty::drop calls ClosePseudoConsole which blocks
                // until the ConPTY output pipe is fully consumed. Our reader thread
                // reads that pipe and sends chunks through a bounded channel. If the
                // channel is full and nobody drains it, the reader blocks on send(),
                // the pipe stalls, and ClosePseudoConsole deadlocks.
                //
                // Fix: drop the master on a background thread (so ClosePseudoConsole
                // runs there) while we drain the channel here, keeping the reader
                // thread unblocked so it can finish consuming the pipe.
                //
                // Note: the writer (PTY input handle) must outlive ClosePseudoConsole.
                // Since writer is a later struct field, it drops after this fn returns,
                // and we wait for ClosePseudoConsole below — so ordering is correct.
                let close_done = Arc::new(AtomicBool::new(false));
                let close_done2 = close_done.clone();
                match std::thread::Builder::new()
                    .name("pty-close".into())
                    .spawn(move || {
                        drop(master);
                        close_done2.store(true, Ordering::Release);
                    }) {
                    Ok(_handle) => {
                        // Drain the channel while ClosePseudoConsole runs.
                        // Safety timeout prevents infinite hang if something goes wrong.
                        let deadline =
                            std::time::Instant::now() + std::time::Duration::from_secs(5);
                        while !close_done.load(Ordering::Acquire)
                            && std::time::Instant::now() < deadline
                        {
                            while self.output_rx.try_recv().is_ok() {}
                            std::thread::sleep(std::time::Duration::from_millis(1));
                        }
                        // If timeout expired, the close thread is leaked — OS will
                        // clean up on process exit. This is better than deadlocking.
                    }
                    Err(_) => {
                        // Thread spawn failed — master was consumed by the closure and
                        // dropped with it, so ClosePseudoConsole runs on this thread.
                        // Best-effort drain to reduce blocking time.
                        while self.output_rx.try_recv().is_ok() {}
                    }
                }
            } else {
                // On Unix, closing the master fd is instant — no deadlock risk.
                drop(master);
            }
        }
        // Reader thread exits once the pipe returns EOF/error after
        // ClosePseudoConsole completes (or fd close on Unix).
    }
}
