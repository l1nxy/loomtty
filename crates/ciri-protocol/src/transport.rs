use std::path::PathBuf;

/// Get the socket path for a session (Unix only for now).
pub fn socket_path(session_name: &str) -> PathBuf {
    #[cfg(unix)]
    {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
        PathBuf::from(runtime_dir).join("ciri").join(format!("{session_name}.sock"))
    }
    #[cfg(windows)]
    {
        let appdata = std::env::var("LOCALAPPDATA")
            .unwrap_or_else(|_| r"C:\Users\Default\AppData\Local".to_string());
        PathBuf::from(appdata).join("ciri").join(format!("{session_name}.pipe"))
    }
}

/// Get the directory for session state files.
pub fn state_dir() -> PathBuf {
    #[cfg(unix)]
    {
        let state_home = std::env::var("XDG_STATE_HOME")
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
                format!("{home}/.local/state")
            });
        PathBuf::from(state_home).join("ciri").join("sessions")
    }
    #[cfg(windows)]
    {
        let appdata = std::env::var("LOCALAPPDATA")
            .unwrap_or_else(|_| r"C:\Users\Default\AppData\Local".to_string());
        PathBuf::from(appdata).join("ciri").join("sessions")
    }
}
