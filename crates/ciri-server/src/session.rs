// Session management helpers for the server daemon.
// Used by the CLI commands (ciri list, ciri attach, etc.)

use ciri_protocol::transport;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

#[allow(dead_code)]
pub fn is_session_running(session_name: &str) -> bool {
    let sock_path = transport::socket_path(session_name);
    if !sock_path.exists() {
        return false;
    }
    // Try connecting to verify the server is actually alive
    UnixStream::connect(&sock_path).is_ok()
}

#[allow(dead_code)]
pub fn list_running_sessions() -> Vec<String> {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
    let ciri_dir = PathBuf::from(runtime_dir).join("ciri");

    let mut sessions = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&ciri_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "sock") {
                if let Some(name) = path.file_stem() {
                    let name = name.to_string_lossy().to_string();
                    if is_session_running(&name) {
                        sessions.push(name);
                    }
                }
            }
        }
    }
    sessions.sort();
    sessions
}
