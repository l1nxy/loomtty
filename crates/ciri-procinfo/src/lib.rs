//! Cross-platform foreground process detection for PTY panes.
//!
//! Given a shell PID and (on Unix) the master PTY fd, returns structured
//! information about the foreground process running in that PTY.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// Handle type for the master PTY fd.
/// On Unix this is a raw file descriptor; on Windows it is unused.
#[cfg(unix)]
pub type RawHandle = std::os::unix::io::RawFd;
#[cfg(windows)]
pub type RawHandle = u64;

/// Information about a running process.
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    /// Process ID.
    pub pid: u32,
    /// Executable file name (basename, e.g. "claude", "codex").
    pub exe_name: String,
    /// Full argument vector.
    pub argv: Vec<String>,
    /// Current working directory (if available).
    pub cwd: Option<String>,
}

/// Get the foreground process of a PTY.
///
/// On Unix: uses `tcgetpgrp(master_fd)` to get the foreground process group,
/// then reads platform-specific APIs to get process info.
///
/// On Windows: walks the process tree from `shell_pid` to find the youngest
/// child process.
///
/// Returns `None` if:
/// - The shell itself is the foreground process (idle at prompt)
/// - Any syscall fails (process may have exited)
/// - The platform is not supported
pub fn foreground_process(shell_pid: u32, master_fd: RawHandle) -> Option<ProcessInfo> {
    #[cfg(target_os = "linux")]
    return linux::foreground_process(shell_pid, master_fd);

    #[cfg(target_os = "macos")]
    return macos::foreground_process(shell_pid, master_fd);

    #[cfg(windows)]
    return windows::foreground_process(shell_pid, master_fd);

    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (shell_pid, master_fd);
        None
    }
}

/// Extract the basename from an executable path.
/// Strips directory components (both `/` and `\`) and `.exe` suffix.
pub fn exe_basename(path: &str) -> &str {
    // Handle both Unix and Windows path separators regardless of platform
    let name = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .rsplit('\\')
        .next()
        .unwrap_or(path);
    name.strip_suffix(".exe").unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exe_basename() {
        assert_eq!(exe_basename("/usr/bin/claude"), "claude");
        assert_eq!(exe_basename("/opt/claude-code/bin/claude"), "claude");
        assert_eq!(exe_basename("claude"), "claude");
        assert_eq!(exe_basename("claude.exe"), "claude");
        assert_eq!(exe_basename(r"C:\Users\me\AppData\codex.exe"), "codex");
        assert_eq!(exe_basename(""), "");
    }
}
