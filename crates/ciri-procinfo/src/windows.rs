//! Windows foreground process detection via Toolhelp32 snapshots.
//!
//! Windows has no `tcgetpgrp` equivalent. Strategy (from Wezterm):
//! 1. Snapshot all processes via `CreateToolhelp32Snapshot`
//! 2. Walk the process tree from `shell_pid` to find descendants
//! 3. Pick the youngest child (most recently started) as the "foreground" process
//!
//! TODO: Read command line and CWD from the PEB via `NtQueryInformationProcess` +
//! `ReadProcessMemory`. Without this, `argv` is always empty and `is_non_interactive`
//! checks cannot reject non-interactive agent invocations.

use crate::{ProcessInfo, RawHandle};

use std::ffi::OsString;
use std::mem;
use std::os::windows::ffi::OsStringExt;
use windows::Win32::Foundation::{CloseHandle, HANDLE, MAX_PATH};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, CREATE_TOOLHELP_SNAPSHOT_FLAGS,
    PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
    PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
};

pub fn foreground_process(shell_pid: u32, _master_fd: RawHandle) -> Option<ProcessInfo> {
    let snapshot = Snapshot::new()?;
    let entries = snapshot.entries();

    // Build parent→children map
    let children: Vec<&ProcEntry> = find_descendants(&entries, shell_pid);

    if children.is_empty() {
        return None; // Shell is idle
    }

    // Find the youngest child (most recently started)
    let youngest = children
        .into_iter()
        .filter_map(|e| {
            let start = process_start_time(e.pid)?;
            Some((e, start))
        })
        .max_by_key(|(_, start)| *start)
        .map(|(e, _)| e)?;

    let pid = youngest.pid;
    let exe_path = read_exe_path(pid).unwrap_or_else(|| youngest.exe_name.clone());
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

struct ProcEntry {
    pid: u32,
    ppid: u32,
    exe_name: String,
}

struct Snapshot {
    handle: HANDLE,
}

impl Snapshot {
    fn new() -> Option<Self> {
        let handle = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()? };
        Some(Snapshot { handle })
    }

    fn entries(&self) -> Vec<ProcEntry> {
        let mut entries = Vec::new();
        let mut entry: PROCESSENTRY32W = unsafe { mem::zeroed() };
        entry.dwSize = mem::size_of::<PROCESSENTRY32W>() as u32;

        let ok = unsafe { Process32FirstW(self.handle, &mut entry).is_ok() };
        if !ok {
            return entries;
        }

        loop {
            let exe = wide_to_string(&entry.szExeFile);
            entries.push(ProcEntry {
                pid: entry.th32ProcessID,
                ppid: entry.th32ParentProcessID,
                exe_name: exe,
            });

            if unsafe { Process32NextW(self.handle, &mut entry).is_err() } {
                break;
            }
        }

        entries
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.handle) };
    }
}

/// Find all descendants of `root_pid` in the process list.
fn find_descendants<'a>(entries: &'a [ProcEntry], root_pid: u32) -> Vec<&'a ProcEntry> {
    let mut result = Vec::new();
    let mut pids_to_check = vec![root_pid];
    let mut visited = std::collections::HashSet::new();
    visited.insert(root_pid);

    // BFS through process tree (visited set prevents cycles from PID reuse)
    while let Some(parent) = pids_to_check.pop() {
        for entry in entries {
            if entry.ppid == parent && visited.insert(entry.pid) {
                result.push(entry);
                pids_to_check.push(entry.pid);
            }
        }
    }

    result
}

/// Get process start time via `GetProcessTimes`.
fn process_start_time(pid: u32) -> Option<u64> {
    let handle = open_process(pid)?;
    let mut creation = unsafe { mem::zeroed() };
    let mut exit = unsafe { mem::zeroed() };
    let mut kernel = unsafe { mem::zeroed() };
    let mut user = unsafe { mem::zeroed() };

    let ok = unsafe {
        GetProcessTimes(handle.0, &mut creation, &mut exit, &mut kernel, &mut user).is_ok()
    };

    if ok {
        Some((creation.dwHighDateTime as u64) << 32 | creation.dwLowDateTime as u64)
    } else {
        None
    }
}

/// Get the executable path via `QueryFullProcessImageNameW`.
fn read_exe_path(pid: u32) -> Option<String> {
    let handle = open_process(pid)?;
    let mut buf = [0u16; MAX_PATH as usize];
    let mut size = buf.len() as u32;

    let ok = unsafe {
        QueryFullProcessImageNameW(handle.0, PROCESS_NAME_FORMAT(0), &mut buf, &mut size).is_ok()
    };

    if ok && size > 0 {
        let path = OsString::from_wide(&buf[..size as usize]);
        path.to_str().map(|s| s.to_string())
    } else {
        None
    }
}

/// Read command line from the process PEB.
///
/// TODO: Implement full PEB reading via ReadProcessMemory for command line extraction.
/// Without this, `argv` is always empty on Windows, which means `is_non_interactive`
/// checks in agent detection cannot reject non-interactive invocations (e.g. `codex exec`).
fn read_cmdline(_pid: u32) -> Option<Vec<String>> {
    None
}

/// Read CWD from the process PEB.
fn read_cwd(pid: u32) -> Option<String> {
    // TODO: Implement via PEB ProcessParameters.CurrentDirectory.DosPath
    log::debug!("Windows CWD reading not yet fully implemented for pid {pid}");
    let _ = pid;
    None
}

struct ProcessHandle(HANDLE);

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn open_process(pid: u32) -> Option<ProcessHandle> {
    let handle =
        unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid).ok()? };
    Some(ProcessHandle(handle))
}

fn wide_to_string(wide: &[u16]) -> String {
    let nul_pos = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    OsString::from_wide(&wide[..nul_pos])
        .to_string_lossy()
        .into_owned()
}
