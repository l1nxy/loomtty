//! macOS foreground process detection via `proc_*` APIs and `sysctl`.
//!
//! Uses:
//! - `tcgetpgrp(master_fd)` → foreground PGID (POSIX, works on macOS)
//! - `proc_pidpath()` → executable path
//! - `sysctl(KERN_PROCARGS2)` → executable + argv
//! - `proc_pidinfo(PROC_PIDVNODEPATHINFO)` → cwd

use crate::{ProcessInfo, RawHandle};

pub fn foreground_process(shell_pid: u32, master_fd: RawHandle) -> Option<ProcessInfo> {
    let fg_pgid = tcgetpgrp(master_fd)?;

    if fg_pgid as u32 == shell_pid {
        return None;
    }

    let pid = fg_pgid as u32;
    let exe_path = read_exe_path(pid)?;
    let exe_name = crate::exe_basename(&exe_path).to_string();
    let argv = read_argv(pid).unwrap_or_default();
    let cwd = read_cwd(pid);

    Some(ProcessInfo {
        pid,
        exe_name,
        argv,
        cwd,
    })
}

fn tcgetpgrp(fd: RawHandle) -> Option<libc::pid_t> {
    let pgid = unsafe { libc::tcgetpgrp(fd) };
    if pgid > 0 { Some(pgid) } else { None }
}

/// Get executable path via `proc_pidpath`.
fn read_exe_path(pid: u32) -> Option<String> {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let ret = unsafe {
        libc::proc_pidpath(
            pid as libc::c_int,
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len() as u32,
        )
    };
    if ret <= 0 {
        log::debug!("proc_pidpath({pid}) failed");
        return None;
    }
    let path = &buf[..ret as usize];
    String::from_utf8(path.to_vec()).ok()
}

/// Get argv via `sysctl(KERN_PROCARGS2)`.
///
/// Buffer layout: `argc (c_int) | exe_path\0 | \0...padding | argv[0]\0 | argv[1]\0 | ...`
fn read_argv(pid: u32) -> Option<Vec<String>> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as libc::c_int];
    let mut size: libc::size_t = 0;

    // First call: get buffer size
    let ret = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret != 0 || size == 0 {
        log::debug!("sysctl KERN_PROCARGS2 size query failed for pid {pid}");
        return None;
    }

    let mut buf = vec![0u8; size];
    let ret = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            buf.as_mut_ptr() as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret != 0 {
        log::debug!("sysctl KERN_PROCARGS2 data read failed for pid {pid}");
        return None;
    }
    buf.truncate(size);

    parse_procargs2(&buf)
}

/// Parse the KERN_PROCARGS2 buffer into argv.
fn parse_procargs2(buf: &[u8]) -> Option<Vec<String>> {
    if buf.len() < std::mem::size_of::<libc::c_int>() {
        return None;
    }

    // Read argc (guard against negative/corrupted values)
    let argc_raw = i32::from_ne_bytes(buf[..4].try_into().ok()?);
    if argc_raw < 0 {
        return None;
    }
    let argc = argc_raw as usize;
    let mut pos = 4;

    // Skip exe_path (NUL-terminated)
    while pos < buf.len() && buf[pos] != 0 {
        pos += 1;
    }
    // Skip NUL terminator(s) / padding between exe and argv
    while pos < buf.len() && buf[pos] == 0 {
        pos += 1;
    }

    // Read argc arguments
    let mut args = Vec::with_capacity(argc);
    for _ in 0..argc {
        if pos >= buf.len() {
            break;
        }
        let start = pos;
        while pos < buf.len() && buf[pos] != 0 {
            pos += 1;
        }
        let arg = String::from_utf8_lossy(&buf[start..pos]).into_owned();
        args.push(arg);
        // Skip NUL terminator
        if pos < buf.len() {
            pos += 1;
        }
    }

    Some(args)
}

/// Get CWD via `proc_pidinfo(PROC_PIDVNODEPATHINFO)`.
fn read_cwd(pid: u32) -> Option<String> {
    // proc_vnodepathinfo contains two vnode_info_path structs (cdir and rdir).
    // Each vnode_info_path is vnode_info (152 bytes) + path (MAXPATHLEN = 1024 bytes).
    // Total struct size = 2 * (152 + 1024) = 2352 bytes.
    const PROC_PIDVNODEPATHINFO: libc::c_int = 9;
    const VNODE_INFO_PATH_SIZE: usize = 152 + 1024; // vnode_info + path
    const BUF_SIZE: usize = 2 * VNODE_INFO_PATH_SIZE;

    let mut buf = vec![0u8; BUF_SIZE];
    let ret = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            PROC_PIDVNODEPATHINFO,
            0,
            buf.as_mut_ptr() as *mut libc::c_void,
            BUF_SIZE as libc::c_int,
        )
    };
    if ret <= 0 {
        log::debug!("proc_pidinfo PROC_PIDVNODEPATHINFO failed for pid {pid}");
        return None;
    }

    // cdir path starts at offset 152 (after vnode_info for current directory)
    let path_offset = 152;
    if buf.len() < path_offset + 1 {
        return None;
    }
    let path_bytes = &buf[path_offset..path_offset + 1024];
    let nul_pos = path_bytes.iter().position(|&b| b == 0).unwrap_or(1024);
    let path = std::str::from_utf8(&path_bytes[..nul_pos]).ok()?;
    if path.is_empty() {
        return None;
    }
    Some(path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_procargs2_basic() {
        // Simulate: argc=2, exe="/bin/ls\0", padding "\0\0", "ls\0", "-la\0"
        let mut buf = Vec::new();
        buf.extend_from_slice(&2i32.to_ne_bytes()); // argc
        buf.extend_from_slice(b"/bin/ls\0"); // exe path
        buf.extend_from_slice(b"\0\0"); // padding
        buf.extend_from_slice(b"ls\0"); // argv[0]
        buf.extend_from_slice(b"-la\0"); // argv[1]

        let args = parse_procargs2(&buf).unwrap();
        assert_eq!(args, vec!["ls", "-la"]);
    }

    #[test]
    fn test_parse_procargs2_empty() {
        assert!(parse_procargs2(&[]).is_none());
        assert!(parse_procargs2(&[0, 0, 0]).is_none());
    }
}
