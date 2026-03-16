use std::path::PathBuf;

/// Get the socket path for a session.
pub fn socket_path(session_name: &str) -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
    PathBuf::from(runtime_dir).join("ciri").join(format!("{session_name}.sock"))
}

/// Get the directory for session state files.
pub fn state_dir() -> PathBuf {
    let state_home = std::env::var("XDG_STATE_HOME")
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            format!("{home}/.local/state")
        });
    PathBuf::from(state_home).join("ciri").join("sessions")
}
