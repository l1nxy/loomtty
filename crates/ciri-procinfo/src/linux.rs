//! Linux foreground process detection via `/proc` filesystem.

use crate::{ProcessInfo, RawHandle};
use std::fs;
use std::path::PathBuf;

/// Get the foreground process of a PTY on Linux.
///
/// 1. `tcgetpgrp(master_fd)` → foreground process group ID
/// 2. If fg_pgid == shell_pid → shell is idle, return None
/// 3. Read `/proc/<fg_pgid>/exe` → executable path
/// 4. Read `/proc/<fg_pgid>/cmdline` → argv
/// 5. Read `/proc/<fg_pgid>/cwd` → working directory
pub fn foreground_process(shell_pid: u32, master_fd: RawHandle) -> Option<ProcessInfo> {
    let fg_pgid = tcgetpgrp(master_fd)?;

    // Shell is in foreground → idle at prompt
    if fg_pgid as u32 == shell_pid {
        return None;
    }

    let pid = fg_pgid as u32;
    let exe_path = read_exe(pid)?;
    let exe_name = crate::exe_basename(&exe_path).to_string();
    let argv = read_cmdline(pid).unwrap_or_default();
    let cwd = read_cwd(pid);

    Some(ProcessInfo {
        pid,
        exe_name,
        argv,
        cwd,
    })
}

/// Call `tcgetpgrp` to get the foreground process group of the terminal.
fn tcgetpgrp(fd: RawHandle) -> Option<libc::pid_t> {
    let pgid = unsafe { libc::tcgetpgrp(fd) };
    if pgid > 0 { Some(pgid) } else { None }
}

/// Read the executable path via `/proc/<pid>/exe` symlink.
fn read_exe(pid: u32) -> Option<String> {
    let path = format!("/proc/{pid}/exe");
    match fs::read_link(&path) {
        Ok(target) => target.to_str().map(|s| s.to_string()),
        Err(e) => {
            log::debug!("failed to read {path}: {e}");
            None
        }
    }
}

/// Read the command line via `/proc/<pid>/cmdline`.
/// The file contains NUL-separated arguments.
fn read_cmdline(pid: u32) -> Option<Vec<String>> {
    let path = format!("/proc/{pid}/cmdline");
    match fs::read(&path) {
        Ok(data) => parse_cmdline_bytes(&data),
        Err(e) => {
            log::debug!("failed to read {path}: {e}");
            None
        }
    }
}

fn parse_cmdline_bytes(data: &[u8]) -> Option<Vec<String>> {
    if data.is_empty() {
        return None;
    }

    let data = if data.last() == Some(&0) {
        &data[..data.len() - 1]
    } else {
        data
    };

    data.split(|&b| b == 0)
        .map(|arg| String::from_utf8(arg.to_vec()).ok())
        .collect()
}

/// Read the current working directory via `/proc/<pid>/cwd` symlink.
fn read_cwd(pid: u32) -> Option<String> {
    let path = PathBuf::from(format!("/proc/{pid}/cwd"));
    match fs::read_link(&path) {
        Ok(target) => target.to_str().map(|s| s.to_string()),
        Err(e) => {
            log::debug!("failed to read /proc/{pid}/cwd: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::symlink;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "ciri-procinfo-{label}-{nanos}-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn read_self_exe() {
        // /proc/self/exe should always be readable
        let pid = std::process::id();
        let exe = read_exe(pid);
        assert!(exe.is_some(), "should be able to read own exe");
    }

    #[test]
    fn read_self_cmdline() {
        let pid = std::process::id();
        let argv = read_cmdline(pid);
        assert!(argv.is_some(), "should be able to read own cmdline");
        let argv = argv.unwrap();
        assert!(!argv.is_empty(), "cmdline should not be empty");
    }

    #[test]
    fn read_self_cwd() {
        let pid = std::process::id();
        let cwd = read_cwd(pid);
        assert!(cwd.is_some(), "should be able to read own cwd");
    }

    #[test]
    fn nonexistent_pid_returns_none() {
        // PID 4194304 is extremely unlikely to exist
        assert!(read_exe(4194304).is_none());
        assert!(read_cmdline(4194304).is_none());
        assert!(read_cwd(4194304).is_none());
    }

    #[test]
    fn read_cmdline_rejects_invalid_utf8() {
        let data = [b'a', 0xff, 0];
        assert_eq!(
            parse_cmdline_bytes(&data),
            None,
            "invalid utf-8 should fall back cleanly instead of manufacturing replacement characters"
        );
    }

    #[test]
    fn parse_cmdline_bytes_parses_nul_separated_args() {
        let data = b"python\0-c\0print(1)\0";
        let argv = parse_cmdline_bytes(data).unwrap();
        assert_eq!(argv, vec!["python", "-c", "print(1)"]);
    }

    #[test]
    fn read_link_with_invalid_utf8_returns_none() {
        let dir = unique_temp_dir("invalid-link");
        let target = dir.join("target");
        fs::write(&target, []).unwrap();

        let link = dir.join("link");
        #[cfg(target_os = "linux")]
        symlink(std::ffi::OsStr::from_bytes(b"bad-\xff"), &link).unwrap();

        let resolved = fs::read_link(&link)
            .ok()
            .and_then(|target| target.to_str().map(|s| s.to_string()));
        assert!(resolved.is_none());
    }
}
