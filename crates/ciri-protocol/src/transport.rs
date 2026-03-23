use std::path::PathBuf;

/// Default TCP port for remote connections.
pub const DEFAULT_REMOTE_PORT: u16 = 7890;

/// XDG_RUNTIME_DIR with proper fallback.
pub fn runtime_dir() -> PathBuf {
    if let Some(rd) = dirs::runtime_dir() {
        return rd;
    }

    // macOS: preserve old behavior (~/.cache) for backward compat
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            let legacy = home.join(".cache");
            if legacy.exists() {
                return legacy;
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
            PathBuf::from("/tmp")
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

/// Fixed socket path for the single ciri-server process.
pub fn server_socket_path() -> PathBuf {
    runtime_dir().join("ciri").join("ciri.sock")
}

/// Fixed TCP port for the single ciri-server on Windows (legacy fallback).
pub fn server_port() -> u16 {
    port_for_session("__ciri_server__")
}

/// Named pipe path for the ciri server on Windows.
#[cfg(windows)]
pub fn server_pipe_name() -> String {
    r"\\.\pipe\ciri-server".to_string()
}

/// Get the directory for session state files.
pub fn state_dir() -> PathBuf {
    if let Some(sd) = dirs::state_dir() {
        return sd.join("ciri").join("sessions");
    }

    // macOS: preserve old behavior (~/.local/state) for backward compat
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            let legacy = home.join(".local").join("state");
            if legacy.exists() {
                return legacy.join("ciri").join("sessions");
            }
        }
    }

    dirs::data_local_dir()
        .unwrap_or_else(|| {
            if cfg!(windows) {
                PathBuf::from(r"C:\Users\Default\AppData\Roaming")
            } else {
                PathBuf::from("/tmp")
            }
        })
        .join("ciri")
        .join("sessions")
}
