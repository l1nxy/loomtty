use std::path::PathBuf;

/// Resolve the user's home directory.
/// Prefers $HOME, falls back to getpwuid_r on Unix.
#[cfg(unix)]
fn home_dir() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("HOME")
        && !home.is_empty()
    {
        return Some(PathBuf::from(home));
    }
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
    if ret == 0 && !result.is_null() {
        let dir = unsafe { std::ffi::CStr::from_ptr(pwd.pw_dir) };
        if let Ok(s) = dir.to_str() {
            return Some(PathBuf::from(s));
        }
    }
    None
}

/// XDG_RUNTIME_DIR with proper fallback.
#[cfg(unix)]
pub fn runtime_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(dir);
    }
    // Fallback: /run/user/<uid> (systemd convention)
    let uid = unsafe { libc::getuid() };
    let candidate = PathBuf::from(format!("/run/user/{uid}"));
    if candidate.is_dir() {
        return candidate;
    }
    // Last resort: ~/.cache as a per-user writable directory
    if let Some(home) = home_dir() {
        return home.join(".cache");
    }
    PathBuf::from("/tmp")
}

/// Runtime dir on Windows — uses LOCALAPPDATA.
#[cfg(windows)]
pub fn runtime_dir() -> PathBuf {
    let appdata = std::env::var("LOCALAPPDATA")
        .unwrap_or_else(|_| r"C:\Users\Default\AppData\Local".to_string());
    PathBuf::from(appdata)
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
    #[cfg(unix)]
    {
        let state_home = std::env::var("XDG_STATE_HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|h| h.join(".local/state")));
        match state_home {
            Some(p) => p.join("ciri").join("sessions"),
            None => runtime_dir().join("ciri").join("sessions"),
        }
    }
    #[cfg(windows)]
    {
        let appdata = std::env::var("LOCALAPPDATA")
            .unwrap_or_else(|_| r"C:\Users\Default\AppData\Local".to_string());
        PathBuf::from(appdata).join("ciri").join("sessions")
    }
}
