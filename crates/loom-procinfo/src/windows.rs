//! Windows foreground process detection via Toolhelp32 snapshots.
//!
//! Windows has no `tcgetpgrp` equivalent. Strategy (from Wezterm):
//! 1. Snapshot all processes via `CreateToolhelp32Snapshot`
//! 2. Walk the process tree from `shell_pid` to find descendants
//! 3. Pick the youngest child (most recently started) as the "foreground" process
//!
//! Command line and CWD are recovered from the target's PEB via
//! `NtQueryInformationProcess` + `ReadProcessMemory` (see `read_peb_strings`),
//! which is what lets agent detection reject non-interactive invocations
//! (e.g. `codex exec`).

use crate::{ProcessInfo, RawHandle};

use core::ffi::c_void;
use std::ffi::OsString;
use std::mem;
use std::os::windows::ffi::OsStringExt;
use windows::Wdk::System::Threading::{NtQueryInformationProcess, ProcessBasicInformation};
use windows::Win32::Foundation::{CloseHandle, HANDLE, MAX_PATH};
use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_NAME_FORMAT, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    QueryFullProcessImageNameW,
};
use windows::core::PWSTR;

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
    let (argv, cwd) = read_peb_strings(pid);
    let argv = argv.unwrap_or_default();

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
        QueryFullProcessImageNameW(
            handle.0,
            PROCESS_NAME_FORMAT(0),
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        )
        .is_ok()
    };

    if ok && size > 0 {
        let path = OsString::from_wide(&buf[..size as usize]);
        path.to_str().map(|s| s.to_string())
    } else {
        None
    }
}

// ── PEB reading ────────────────────────────────────────────────────────
//
// The kernel keeps a process's command line and current directory in its
// PEB (Process Environment Block). There is no public API to read them
// for another process, so we do what every tool in this space does:
// `NtQueryInformationProcess(ProcessBasicInformation)` to get the PEB
// address, then `ReadProcessMemory` to walk PEB → ProcessParameters →
// the `CommandLine` / `CurrentDirectory.DosPath` UNICODE_STRINGs.
//
// The structs below are hand-defined with explicit x64 field offsets
// rather than reusing the `windows` crate's types: its
// `RTL_USER_PROCESS_PARAMETERS` stub stops at `CommandLine` and omits
// `CurrentDirectory`, and its `PEB` is a `Reserved*` blob. These are
// 64-bit layouts — we read a 64-bit target from our 64-bit process. A
// 32-bit (WOW64) target has a different layout; the mismatched read just
// yields offsets that fail validation and we return `None` rather than
// wrong data. Every modern agent (codex, claude, …) is 64-bit, so this
// covers the real cases.

/// x64 `UNICODE_STRING`: `{ u16, u16, <4-byte pad>, *mut u16 }` = 16 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}

/// x64 `PROCESS_BASIC_INFORMATION` (48 bytes). Only `peb_base_address` is
/// used; the rest pins the layout so `NtQueryInformationProcess` writes
/// into the right slots.
#[repr(C)]
#[allow(dead_code)]
struct ProcessBasicInformationRaw {
    exit_status: i32,
    peb_base_address: *mut c_void,
    affinity_mask: usize,
    base_priority: i32,
    unique_process_id: usize,
    inherited_from_unique_process_id: usize,
}

/// PEB prefix up to `ProcessParameters` (x64 offset 0x20).
#[repr(C)]
#[allow(dead_code)]
struct PebPrefix {
    reserved: [u8; 0x20],
    process_parameters: *mut c_void,
}

/// `RTL_USER_PROCESS_PARAMETERS` truncated after `CommandLine` (x64
/// offset 0x70). `CurrentDirectory.DosPath` sits at 0x38.
#[repr(C)]
#[allow(dead_code)]
struct RtlUserProcessParameters {
    reserved1: [u8; 0x38],
    current_directory_path: UnicodeString, // CurDir.DosPath @ 0x38
    current_directory_handle: *mut c_void, // CurDir.Handle  @ 0x48
    dll_path: UnicodeString,               //                @ 0x50
    image_path_name: UnicodeString,        //                @ 0x60
    command_line: UnicodeString,           //                @ 0x70
}

/// Copy a POD value of type `T` out of another process's address space.
/// Returns `None` unless the full `size_of::<T>()` bytes were read.
unsafe fn read_remote<T>(handle: HANDLE, addr: *const c_void) -> Option<T> {
    if addr.is_null() {
        return None;
    }
    let mut value = unsafe { mem::zeroed::<T>() };
    let mut bytes_read = 0usize;
    unsafe {
        ReadProcessMemory(
            handle,
            addr,
            &mut value as *mut T as *mut c_void,
            mem::size_of::<T>(),
            Some(&mut bytes_read),
        )
    }
    .ok()?;
    (bytes_read == mem::size_of::<T>()).then_some(value)
}

/// Read the UTF-16 payload a remote `UNICODE_STRING` points at.
unsafe fn read_remote_wstr(handle: HANDLE, s: &UnicodeString) -> Option<String> {
    // `length` is a UTF-16 *byte* count. Reject the cases that would make
    // the read unsafe or pointless:
    //  - odd: a valid UNICODE_STRING length is always even; an odd value
    //    (garbage from a WOW64-layout mismatch, or a target corrupting its
    //    own parameters) would leave `byte_len / 2` u16s one byte short of
    //    the `byte_len`-byte ReadProcessMemory below — an OOB write in *our*
    //    address space.
    //  - zero / null buffer: nothing to read.
    //  - oversized: bound the allocation against a garbage length.
    let byte_len = s.length as usize;
    if byte_len == 0 || byte_len % 2 != 0 || s.buffer.is_null() || byte_len > 64 * 1024 {
        return None;
    }
    let mut buf = vec![0u16; byte_len / 2];
    let mut bytes_read = 0usize;
    unsafe {
        ReadProcessMemory(
            handle,
            s.buffer as *const c_void,
            buf.as_mut_ptr() as *mut c_void,
            byte_len,
            Some(&mut bytes_read),
        )
    }
    .ok()?;
    (bytes_read == byte_len).then(|| String::from_utf16_lossy(&buf))
}

/// Walk `pid`'s PEB once and return its `(argv, cwd)`. Opening the
/// process and traversing the PEB is done a single time for both, since
/// they live in the same `RTL_USER_PROCESS_PARAMETERS`.
fn read_peb_strings(pid: u32) -> (Option<Vec<String>>, Option<String>) {
    let Some(handle) = open_process(pid) else {
        return (None, None);
    };
    unsafe {
        let mut pbi = mem::zeroed::<ProcessBasicInformationRaw>();
        let mut ret_len = 0u32;
        let status = NtQueryInformationProcess(
            handle.0,
            ProcessBasicInformation,
            &mut pbi as *mut _ as *mut c_void,
            mem::size_of::<ProcessBasicInformationRaw>() as u32,
            &mut ret_len,
        );
        // STATUS_SUCCESS == 0.
        if status.0 != 0 {
            return (None, None);
        }

        let Some(peb) = read_remote::<PebPrefix>(handle.0, pbi.peb_base_address) else {
            return (None, None);
        };
        let Some(params) =
            read_remote::<RtlUserProcessParameters>(handle.0, peb.process_parameters)
        else {
            return (None, None);
        };

        let argv = read_remote_wstr(handle.0, &params.command_line)
            .map(|cmdline| split_command_line(&cmdline));
        let cwd = read_remote_wstr(handle.0, &params.current_directory_path).map(|mut s| {
            // Windows stores the cwd with a trailing separator
            // (`C:\proj\`); drop it for parity with the Unix cwd, but
            // keep a bare drive root (`C:\`).
            while s.len() > 3 && s.ends_with('\\') {
                s.pop();
            }
            s
        });
        (argv, cwd)
    }
}

/// Split a Windows command-line string into an argument vector following
/// the same backslash/quote rules as `CommandLineToArgvW` (and therefore
/// the argv the target's own runtime sees). Kept as a pure function so it
/// is unit-testable off-Windows.
fn split_command_line(cmd: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut chars = cmd.chars().peekable();
    let mut in_quotes = false;
    let mut has_token = false;

    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                has_token = true;
                let mut slashes = 1usize;
                while chars.peek() == Some(&'\\') {
                    chars.next();
                    slashes += 1;
                }
                if chars.peek() == Some(&'"') {
                    // Backslashes before a quote: each pair is one literal
                    // backslash; an odd one out escapes the quote.
                    for _ in 0..slashes / 2 {
                        current.push('\\');
                    }
                    if slashes % 2 == 1 {
                        current.push('"');
                        chars.next();
                    }
                    // Even count: leave the quote for the next iteration
                    // to treat as a delimiter.
                } else {
                    for _ in 0..slashes {
                        current.push('\\');
                    }
                }
            }
            '"' => {
                has_token = true;
                if in_quotes && chars.peek() == Some(&'"') {
                    // "" inside quotes is a literal quote; stay quoted.
                    current.push('"');
                    chars.next();
                } else {
                    in_quotes = !in_quotes;
                }
            }
            ' ' | '\t' if !in_quotes => {
                if has_token {
                    args.push(mem::take(&mut current));
                    has_token = false;
                }
            }
            other => {
                has_token = true;
                current.push(other);
            }
        }
    }
    if has_token {
        args.push(current);
    }
    args
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Walk our *own* PEB and check the results against `std`. This is the
    /// real validation that the hand-defined x64 offsets are correct: if
    /// any field were misaligned, the cwd wouldn't match and argv[0]
    /// wouldn't be a path.
    #[test]
    fn reads_own_process_peb() {
        let (argv, cwd) = read_peb_strings(std::process::id());

        let argv = argv.expect("should read own command line");
        assert!(!argv.is_empty(), "argv should contain at least argv[0]");
        assert!(!argv[0].is_empty(), "argv[0] should be the program path");

        let cwd = cwd.expect("should read own current directory");
        let expected = std::env::current_dir().unwrap();
        assert_eq!(
            cwd.to_lowercase().trim_end_matches('\\'),
            expected.to_string_lossy().to_lowercase().trim_end_matches('\\'),
            "PEB cwd should match std::env::current_dir"
        );
    }

    #[test]
    fn split_simple_tokens() {
        assert_eq!(split_command_line("codex exec foo"), ["codex", "exec", "foo"]);
        assert_eq!(split_command_line("   claude   -p  "), ["claude", "-p"]);
        assert_eq!(split_command_line(""), Vec::<String>::new());
    }

    #[test]
    fn split_quoted_path_with_spaces() {
        assert_eq!(
            split_command_line(r#""C:\Program Files\x\codex.exe" exec"#),
            [r"C:\Program Files\x\codex.exe", "exec"]
        );
    }

    #[test]
    fn split_backslash_quote_rules() {
        // 2 backslashes + quote => 1 literal backslash, quote is a delimiter.
        assert_eq!(split_command_line(r#"a\\"b c" d"#), [r"a\b c", "d"]);
        // 3 backslashes + quote => 1 literal backslash + a literal quote.
        assert_eq!(split_command_line(r#"a\\\"b"#), [r#"a\"b"#]);
        // Trailing backslashes (not before a quote) stay literal.
        assert_eq!(split_command_line(r"a\\"), [r"a\\"]);
    }

    #[test]
    fn split_embedded_and_empty_quotes() {
        // "" inside a quoted run is a literal quote.
        assert_eq!(split_command_line(r#""a""b""#), [r#"a"b"#]);
        // An empty quoted string is still an argument.
        assert_eq!(split_command_line(r#"a "" b"#), ["a", "", "b"]);
    }
}
