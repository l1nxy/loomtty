use anyhow::{Context, Result};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

/// Cross-platform PTY wrapper using portable-pty.
/// Uses a background reader thread because portable-pty's reader is blocking.
/// Callback invoked by the PTY reader thread when output is available.
/// Allows the server tick loop to wake immediately instead of polling.
pub type PtyOutputNotify = Arc<dyn Fn() + Send + Sync>;

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

/// Non-unix platforms have no passwd database, so there's no login shell to
/// query. A `None`-returning stub lets the call site stay a plain `fn` ref.
#[cfg(not(unix))]
fn get_pw_shell() -> Option<String> {
    None
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
        Self::spawn_inner(cols, rows, shell, command, cwd, None)
    }

    /// Spawn a PTY with an output notification callback.
    /// The callback fires from the reader thread whenever PTY output is available,
    /// allowing the server tick loop to wake immediately instead of polling.
    pub fn spawn_with_notify(
        cols: u16,
        rows: u16,
        shell: &str,
        command: Option<&str>,
        cwd: Option<&std::path::Path>,
        notify: PtyOutputNotify,
    ) -> Result<Self> {
        Self::spawn_inner(cols, rows, shell, command, cwd, Some(notify))
    }

    fn spawn_inner(
        cols: u16,
        rows: u16,
        shell: &str,
        command: Option<&str>,
        cwd: Option<&std::path::Path>,
        output_notify: Option<PtyOutputNotify>,
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
            if cfg!(windows) {
                let mut b = CommandBuilder::new("cmd.exe");
                b.arg("/C");
                b.arg(c);
                b
            } else {
                let mut b = CommandBuilder::new("sh");
                b.arg("-c");
                b.arg(c);
                b
            }
        } else if !shell.is_empty() {
            CommandBuilder::new(shell)
        } else if cfg!(windows) {
            CommandBuilder::new("cmd.exe")
        } else {
            let shell_path = std::env::var("SHELL")
                .ok()
                .filter(|s| !s.is_empty())
                // `or_else` keeps the passwd lookup lazy: only fall back to
                // `get_pw_shell()` when `$SHELL` is unset/empty. `or(..)` would
                // run the (possibly NSS/LDAP-blocking) lookup on every pane
                // spawn even when `$SHELL` is already valid.
                .or_else(get_pw_shell)
                .unwrap_or_else(|| "/bin/sh".to_string());
            CommandBuilder::new(shell_path)
        };

        // Ensure child knows its terminal type.
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

        // Terminal identification — used by neofetch/fastfetch, shell integrations, etc.
        let version = env!("CARGO_PKG_VERSION");
        cmd.env("TERM_PROGRAM", "loomtty");
        cmd.env("TERM_PROGRAM_VERSION", version);
        // LC_TERMINAL / LC_TERMINAL_VERSION are respected by many tools as an
        // alternative to TERM_PROGRAM and survive across sudo/ssh boundaries.
        cmd.env("LC_TERMINAL", "loomtty");
        cmd.env("LC_TERMINAL_VERSION", version);

        // Shell integration: set env vars so shells auto-source integration scripts.
        if let Ok(integration_dir) = std::env::var("LOOM_SHELL_INTEGRATION_DIR") {
            cmd.env("LOOM_SHELL_INTEGRATION_DIR", &integration_dir);

            // Bash: BASH_ENV is only sourced by non-interactive bash (scripts,
            // subshells). For interactive shells, users should add to .bashrc:
            //   [[ -n "$LOOM_SHELL_INTEGRATION_DIR" ]] && source "$LOOM_SHELL_INTEGRATION_DIR/loom.bash"
            let bash_script = format!("{}/loom.bash", integration_dir);
            cmd.env("BASH_ENV", &bash_script);

            // Fish: XDG_DATA_DIRS-based vendor_conf.d is complex; rely on
            // LOOM_SHELL_INTEGRATION_DIR env var + TERM_PROGRAM detection
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
        // 64 slots × 64KB buffer = ~4MB max buffered.  The larger queue lets
        // the reader thread keep draining the OS PTY buffer while the tick
        // loop processes previous batches, avoiding pipeline stalls on bulk output.
        let (output_tx, output_rx) = mpsc::sync_channel(64);
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
                            if let Some(ref notify) = output_notify {
                                notify();
                            }
                            break;
                        }
                        Ok(n) => {
                            if output_tx.send(buf[..n].to_vec()).is_err() {
                                break; // Receiver dropped
                            }
                            if let Some(ref notify) = output_notify {
                                notify();
                            }
                        }
                        Err(_) => {
                            reader_done_clone.store(true, Ordering::Release);
                            if let Some(ref notify) = output_notify {
                                notify();
                            }
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

    /// Reap the child if it has exited (non-blocking), returning its exit code.
    /// `Some(code)` = exited (from `portable_pty::ExitStatus`); `None` = still
    /// running or not yet reapable.
    pub fn try_wait(&self) -> Option<i32> {
        self.child
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .try_wait()
            .ok()
            .flatten()
            .map(|status| status.exit_code() as i32)
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

    /// Heuristic check for password input mode: canonical mode (`ICANON`) with
    /// echo disabled (`!ECHO`).
    ///
    /// Programs like `sudo`, `ssh`, and `passwd` disable terminal echo when
    /// reading passwords, which this detects.
    ///
    /// **Known false positives:** `read -s` in bash/zsh, explicit `stty -echo`.
    /// **Known false negatives:** raw-mode password prompts (e.g. some TUI apps),
    /// programs that read directly from `/dev/tty` (e.g. ssh in some
    /// configurations).
    /// **Windows:** Always returns `false` — ConPTY does not expose termios state.
    #[cfg(unix)]
    pub fn is_password_input(&self) -> bool {
        if let Some(ref master) = self.master
            && let Some(termios) = master.get_termios()
        {
            let bits = termios.local_flags.bits();
            let canonical = (bits & libc::ICANON) != 0;
            let echo = (bits & libc::ECHO) != 0;
            return canonical && !echo;
        }
        false
    }

    #[cfg(not(unix))]
    pub fn is_password_input(&self) -> bool {
        false
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
