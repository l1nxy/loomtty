use std::path::PathBuf;

/// Default TCP port for remote connections.
pub const DEFAULT_REMOTE_PORT: u16 = 7890;

/// Secure fallback directory under /tmp with a user-specific subdirectory.
///
/// Uses the same pattern as tmux (`/tmp/tmux-<uid>/`):
/// 1. `mkdir(2)` is atomic and does NOT follow symlinks — safe against pre-creation.
/// 2. On EEXIST, open with `O_DIRECTORY | O_NOFOLLOW` to get an fd that is
///    guaranteed to refer to a real directory (not a symlink).
/// 3. `fstat(fd)` + `fchmod(fd)` operate on the fd, eliminating any TOCTOU gap
///    between checking and modifying.
#[cfg(unix)]
fn secure_tmp_fallback() -> PathBuf {
    use nix::fcntl::{OFlag, open};
    use nix::sys::stat::{Mode, fchmod, fstat};
    use nix::unistd::close;

    let uid = unsafe { libc::getuid() };
    let dir = PathBuf::from(format!("/tmp/loom-{uid}"));

    // Step 1: Attempt atomic creation with restrictive permissions.
    // mkdir(2) does not follow symlinks, so a pre-planted symlink causes EEXIST.
    match std::fs::create_dir(&dir) {
        Ok(()) => {
            // Freshly created — permissions set by umask; force 0o700 below via fd.
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // Something exists at this path — verify via fd below.
        }
        Err(e) => {
            log::error!("failed to create fallback dir {}: {e}", dir.display());
            return PathBuf::from("/nonexistent-loom-fallback");
        }
    }

    // Step 2: Open with O_DIRECTORY | O_NOFOLLOW.
    // - O_NOFOLLOW: open(2) fails with ELOOP if the path is a symlink.
    // - O_DIRECTORY: open(2) fails with ENOTDIR if the path is not a directory.
    // Together these guarantee the fd refers to a real, non-symlink directory.
    let fd = match open(
        &dir,
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(e) => {
            log::error!(
                "cannot open fallback dir {} (symlink or not a directory?): {e}",
                dir.display()
            );
            return PathBuf::from("/nonexistent-loom-fallback");
        }
    };

    // Step 3: fstat(fd) — verify ownership on the actual opened directory.
    // No TOCTOU: fstat operates on the fd, not the path.
    let stat = match fstat(fd) {
        Ok(s) => s,
        Err(e) => {
            let _ = close(fd);
            log::error!("fstat failed on fallback dir fd: {e}");
            return PathBuf::from("/nonexistent-loom-fallback");
        }
    };
    if stat.st_uid != uid {
        let _ = close(fd);
        log::error!(
            "fallback directory {} is owned by uid {} (expected {}), refusing to use",
            dir.display(),
            stat.st_uid,
            uid
        );
        return PathBuf::from("/nonexistent-loom-fallback");
    }

    // Step 4: fchmod(fd) — set restrictive permissions without TOCTOU.
    if let Err(e) = fchmod(fd, Mode::S_IRWXU) {
        log::warn!("fchmod 0700 failed on fallback dir: {e}");
    }

    let _ = close(fd);
    dir
}

#[cfg(not(unix))]
fn secure_tmp_fallback() -> PathBuf {
    PathBuf::from(if cfg!(windows) {
        r"C:\Users\Default\AppData\Local"
    } else {
        "/tmp"
    })
}

/// XDG_RUNTIME_DIR with proper fallback.
pub fn runtime_dir() -> PathBuf {
    if let Some(rd) = dirs::runtime_dir() {
        return rd;
    }

    // macOS: use $TMPDIR (per-user session temp, like tmux uses /tmp)
    #[cfg(target_os = "macos")]
    {
        if let Ok(tmpdir) = std::env::var("TMPDIR") {
            let p = PathBuf::from(tmpdir);
            if p.exists() {
                return p;
            }
        }
    }

    // Linux: probe /run/user/<uid> when XDG_RUNTIME_DIR is unset
    #[cfg(target_os = "linux")]
    {
        let uid = unsafe { libc::getuid() };
        let probe = PathBuf::from(format!("/run/user/{}", uid));
        if probe.exists() {
            return probe;
        }
    }

    dirs::cache_dir().unwrap_or_else(|| {
        if cfg!(windows) {
            PathBuf::from(r"C:\Users\Default\AppData\Local")
        } else {
            secure_tmp_fallback()
        }
    })
}

/// Derive a localhost TCP port from a session name (for Windows IPC).
/// Returns a port in the range 49152..65535 (ephemeral range).
pub fn port_for_session(session_name: &str) -> u16 {
    let mut hash: u32 = 5381;
    for b in session_name.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(b as u32);
    }
    // Map to ephemeral port range 49152..65535
    49152 + (hash % (65535 - 49152)) as u16
}

/// Fixed socket path for the single loom-server process.
pub fn server_socket_path() -> PathBuf {
    runtime_dir().join("loom").join("loom.sock")
}

/// Fixed TCP port for the single loom-server on Windows (legacy fallback).
pub fn server_port() -> u16 {
    port_for_session("__loom_server__")
}

/// Named pipe path for the loom server on Windows.
#[cfg(windows)]
pub fn server_pipe_name() -> String {
    r"\\.\pipe\loom-server".to_string()
}

/// Get the directory for session state files.
pub fn state_dir() -> PathBuf {
    #[cfg(debug_assertions)]
    if let Some(sd) = std::env::var_os("LOOM_TEST_STATE_DIR") {
        return PathBuf::from(sd);
    }

    if let Some(sd) = dirs::state_dir() {
        return sd.join("loom").join("sessions");
    }

    // macOS: preserve old behavior (~/.local/state) for backward compat
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            let legacy = home.join(".local").join("state");
            if legacy.exists() {
                return legacy.join("loom").join("sessions");
            }
        }
    }

    dirs::data_local_dir()
        .unwrap_or_else(|| {
            if cfg!(windows) {
                PathBuf::from(r"C:\Users\Default\AppData\Roaming")
            } else {
                secure_tmp_fallback()
            }
        })
        .join("loom")
        .join("sessions")
}
