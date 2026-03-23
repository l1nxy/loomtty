use std::path::PathBuf;

/// Default TCP port for remote connections.
pub const DEFAULT_REMOTE_PORT: u16 = 7890;

/// XDG_RUNTIME_DIR with proper fallback.
pub fn runtime_dir() -> PathBuf {
    if let Some(dir) = dirs::runtime_dir() {
        return dir;
    }
    // Fallback: cache dir (~/Library/Caches on macOS, ~/.cache on Linux,
    // LOCALAPPDATA on Windows)
    if let Some(dir) = dirs::cache_dir() {
        return dir;
    }
    PathBuf::from(if cfg!(windows) {
        r"C:\Users\Default\AppData\Local"
    } else {
        "/tmp"
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
    // dirs::state_dir() handles XDG_STATE_HOME on Linux, returns None on
    // macOS/Windows where the concept doesn't exist.
    if let Some(dir) = dirs::state_dir() {
        return dir.join("ciri").join("sessions");
    }
    // Fallback: use data_local_dir (LOCALAPPDATA on Windows,
    // ~/Library/Application Support on macOS)
    if let Some(dir) = dirs::data_local_dir() {
        return dir.join("ciri").join("sessions");
    }
    runtime_dir().join("ciri").join("sessions")
}
