// Session management helpers for the server daemon.
// Used by the CLI commands (ciri list, ciri attach, etc.)

use ciri_protocol::transport;

#[allow(dead_code)]
pub fn is_session_running(_session_name: &str) -> bool {
    let _sock_path = transport::server_socket_path();

    #[cfg(unix)]
    {
        if !_sock_path.exists() {
            return false;
        }
        // Try connecting to verify the server is actually alive
        std::os::unix::net::UnixStream::connect(&_sock_path).is_ok()
    }

    #[cfg(windows)]
    {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(transport::server_pipe_name())
            .is_ok()
    }
}

#[allow(dead_code)]
pub fn list_running_sessions() -> Vec<String> {
    let ciri_dir = transport::runtime_dir().join("ciri");

    let mut sessions = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&ciri_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            #[cfg(unix)]
            let ext_match = path.extension().is_some_and(|ext| ext == "sock");
            #[cfg(windows)]
            let ext_match = path.extension().is_some_and(|ext| ext == "pipe");

            if ext_match {
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
