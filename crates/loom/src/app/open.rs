//! Open URLs and file paths from terminal link clicks.
//!
//! Security: file paths are canonicalized and existence-checked before opening.
//! Only http/https URLs are passed to the OS handler; file paths use $EDITOR.

use std::process::Command;

use super::App;

// ── App methods for link hover/click ──

impl App {
    pub fn update_hovered_link(&mut self, mx: f32, my: f32) -> bool {
        let next = self
            .pixel_to_cell(mx, my)
            .and_then(|(pane_id, col, buffer_row)| {
                let link = self
                    .core
                    .pane_grids
                    .get(&pane_id)?
                    .link_at(col, buffer_row)?;
                Some(super::HoveredLink {
                    pane_id,
                    url: link.url,
                    start: (link.start_col, link.start_row),
                    end: (link.end_col, link.end_row),
                })
            });
        if self.core.hovered_link == next {
            return false;
        }
        self.core.hovered_link = next;
        true
    }

    pub fn clear_hovered_link(&mut self) -> bool {
        self.core.clear_hovered_link()
    }

    pub fn hovered_link_url_at(&self, pane_id: u64, col: u16, buffer_row: usize) -> Option<String> {
        self.core.hovered_link_url_at(pane_id, col, buffer_row)
    }

    pub fn link_activation_modifier_active(&self) -> bool {
        link_activation_modifier_active(self.modifiers)
    }

    pub fn open_url(&self, url: &str) {
        let pane_id = self
            .core
            .hovered_link
            .as_ref()
            .map(|link| link.pane_id)
            .or(self.core.context_menu.target_pane_id)
            .or_else(|| self.core.workspaces.active().active_pane_id());
        let pane_cwd = pane_id
            .and_then(|id| self.core.pane_grids.get(&id))
            .and_then(|grid| grid.cwd.as_deref());
        if let Err(e) = open_url_impl(url, pane_cwd) {
            log::warn!("failed to open url '{url}': {e}");
        }
    }
}

// ── Free functions ──

#[cfg(target_os = "macos")]
pub(crate) fn link_activation_modifier_active(modifiers: winit::keyboard::ModifiersState) -> bool {
    modifiers.super_key()
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn link_activation_modifier_active(modifiers: winit::keyboard::ModifiersState) -> bool {
    modifiers.control_key()
}

fn open_url_impl(url: &str, pane_cwd: Option<&str>) -> std::io::Result<()> {
    // Check if this is a file path (not a URL)
    if is_file_path_link(url) {
        return open_file_path(url, pane_cwd);
    }

    // Only allow http/https URLs for OS handler — other schemes (file://, data:,
    // javascript:, etc.) could be exploited by malicious terminal output.
    let lower = url.to_ascii_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("refusing to open URL with untrusted scheme: {url}"),
        ));
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(url).spawn()?;
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        // Use ShellExecuteW directly instead of `cmd /C start`.
        // cmd.exe interprets metacharacters (& | > < ^ ( ) % ! ") in the URL,
        // and a blocklist can never be exhaustive.  ShellExecuteW is the Windows
        // API designed for this purpose and avoids cmd.exe entirely.
        // This is the same approach used by WezTerm and the `open` crate.
        use std::os::windows::ffi::OsStrExt;
        unsafe extern "system" {
            fn ShellExecuteW(
                hwnd: *mut std::ffi::c_void,
                operation: *const u16,
                file: *const u16,
                parameters: *const u16,
                directory: *const u16,
                show_cmd: i32,
            ) -> isize;
        }
        let wide_open: Vec<u16> = std::ffi::OsStr::new("open")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let wide_url: Vec<u16> = std::ffi::OsStr::new(url)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                wide_open.as_ptr(),
                wide_url.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1, // SW_SHOWNORMAL
            )
        };
        // ShellExecuteW returns > 32 on success.
        if result <= 32 {
            return Err(std::io::Error::other(format!(
                "ShellExecuteW failed with code {result}"
            )));
        }
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open").arg(url).spawn()?;
    }

    #[allow(unreachable_code)]
    Ok(())
}

/// Open a trusted first-party file or directory in the OS default
/// handler. Bypasses the `$EDITOR`-or-refuse safety gate that
/// `open_file_path` enforces for terminal-output-derived paths —
/// this entry point is for paths the app itself produced (e.g.
/// `config_path()`), where there's no risk of malicious terminal
/// content forging the input.
pub(crate) fn open_trusted_path(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(path).spawn()?;
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        // Same `ShellExecuteW` shape as `open_url_impl`'s URL path —
        // copy-paste rather than refactor to keep the unsafe surface
        // tightly scoped. `path` here is OsStr so we encode to UTF-16
        // with the standard Windows extension.
        use std::os::windows::ffi::OsStrExt;
        unsafe extern "system" {
            fn ShellExecuteW(
                hwnd: *mut std::ffi::c_void,
                operation: *const u16,
                file: *const u16,
                parameters: *const u16,
                directory: *const u16,
                show_cmd: i32,
            ) -> isize;
        }
        let wide_open: Vec<u16> = std::ffi::OsStr::new("open")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let wide_path: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                wide_open.as_ptr(),
                wide_path.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1, // SW_SHOWNORMAL
            )
        };
        if result <= 32 {
            return Err(std::io::Error::other(format!(
                "ShellExecuteW failed with code {result}"
            )));
        }
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open").arg(path).spawn()?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Ok(())
}

/// Check if a link string looks like a file path rather than a URL.
///
/// Delegates to the same detector that produced the link, so whatever
/// `link_at` highlights as a path is routed to `$EDITOR` here. The old
/// hand-rolled copy of the rules had drifted: it never accepted bare
/// filenames (`Cargo.toml`, `main.rs`), so those highlighted but were
/// then refused as an "untrusted scheme" URL.
fn is_file_path_link(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("www.") {
        return false;
    }
    loom_app::grid::looks_like_file_path(s)
}

/// Open a file path, optionally at a specific line:col.
///
/// Security: resolves the path to an absolute canonical form and verifies
/// that the file exists before opening. This prevents:
/// - Opening non-existent paths crafted by malicious terminal output
/// - Path traversal via unresolved `..` components
/// - Uncontrolled tilde/env-var expansion
///
/// Uses $EDITOR with line number support when available, otherwise falls
/// back to the OS default handler. All arguments are passed as argv
/// elements (not through a shell) to prevent command injection.
fn open_file_path(path_with_loc: &str, pane_cwd: Option<&str>) -> std::io::Result<()> {
    let (path, line, _col) = parse_file_location(path_with_loc);

    // Resolve ~ to home directory
    let expanded = if path.starts_with("~/") {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            format!("{}{}", home.to_string_lossy(), &path[1..])
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "cannot expand ~: HOME not set",
            ));
        }
    } else {
        path.to_string()
    };

    // For relative paths, resolve against the pane's CWD (from OSC 7).
    // Without a known CWD, relative paths cannot be safely resolved.
    let to_resolve = if !std::path::Path::new(&expanded).is_absolute() {
        match pane_cwd {
            Some(cwd) => {
                let joined = std::path::Path::new(cwd).join(&expanded);
                joined.to_string_lossy().into_owned()
            }
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("cannot resolve relative path without pane CWD: {expanded}"),
                ));
            }
        }
    } else {
        expanded
    };

    // Canonicalize: resolve `..`, symlinks, and convert to absolute path.
    // This also serves as an existence check — canonicalize fails if the
    // file doesn't exist.
    let canonical = std::fs::canonicalize(&to_resolve).map_err(|e| {
        log::debug!("file path not found or inaccessible: {to_resolve}: {e}");
        e
    })?;
    let resolved = canonical.to_string_lossy();

    // Try $VISUAL, then $EDITOR (supports line numbers). $VISUAL is the
    // conventional override for a full-screen/GUI editor and is what many
    // users set instead of $EDITOR — honouring only the latter left their
    // file-path links refusing to open with no visible reason.
    let editor = std::env::var("VISUAL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            std::env::var("EDITOR")
                .ok()
                .filter(|v| !v.trim().is_empty())
        });
    if let Some(editor) = editor {
        // `$EDITOR` is frequently "code --wait" or a quoted path with
        // spaces, and on Windows the VS Code / Cursor launchers on PATH
        // are `code.cmd` / `cursor.cmd`, which `Command::new("code")`
        // cannot find. Split into program + leading args and resolve the
        // program through PATH/PATHEXT so those setups work too.
        let (program, mut args) = split_editor_command(&editor);
        let program = resolve_program(&program);
        let editor_lower = editor.to_ascii_lowercase();
        if editor_lower.contains("code") || editor_lower.contains("cursor") {
            let mut loc = resolved.to_string();
            if let Some(l) = line {
                loc = format!("{loc}:{l}");
            }
            args.push("--goto".to_string());
            args.push(loc);
        } else if editor_lower.contains("vim")
            || editor_lower.contains("nvim")
            || editor_lower.contains("hx")
        {
            if let Some(l) = line {
                args.push(format!("+{l}"));
            }
            args.push(resolved.to_string());
        } else {
            args.push(resolved.to_string());
        }
        return Command::new(&program)
            .args(&args)
            .spawn()
            .map(|_| ())
            .map_err(|e| {
                std::io::Error::new(
                    e.kind(),
                    format!("failed to launch editor '{program}' (from $VISUAL/$EDITOR): {e}"),
                )
            });
    }

    // No $VISUAL/$EDITOR set — refuse to open file paths via OS handler.
    // OS handlers (cmd /C start, open, xdg-open) can execute arbitrary
    // binaries (.exe, .bat, .app), which is unsafe for untrusted paths
    // from terminal output. URLs are fine (browser is sandboxed), but
    // file paths need an explicit editor.
    Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "set $VISUAL or $EDITOR to open file paths (OS handler refused for safety)",
    ))
}

/// Split an `$EDITOR`-style command line into `(program, leading args)`.
///
/// If the whole string names an existing file (e.g. a full path with
/// spaces) it is the program. Otherwise the string is split on whitespace,
/// honouring double quotes, so `code --wait` and
/// `"C:\Program Files\Editor\ed.exe" -n` both work. Args are only ever
/// passed as argv elements — never through a shell.
fn split_editor_command(editor: &str) -> (String, Vec<String>) {
    let trimmed = editor.trim();
    if std::path::Path::new(trimmed).is_file() {
        return (trimmed.to_string(), Vec::new());
    }
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut has_word = false;
    for ch in trimmed.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                has_word = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if has_word {
                    words.push(std::mem::take(&mut cur));
                    has_word = false;
                }
            }
            c => {
                cur.push(c);
                has_word = true;
            }
        }
    }
    if has_word {
        words.push(cur);
    }
    let mut it = words.into_iter();
    let program = it.next().unwrap_or_else(|| trimmed.to_string());
    (program, it.collect())
}

/// Resolve a bare program name through `PATH`, honouring `PATHEXT` on
/// Windows. Needed because `Command::new` only appends `.exe` when
/// searching PATH, so `code` / `cursor` — installed as `code.cmd` /
/// `cursor.cmd` — would fail with "program not found". Returning the full
/// path to the `.cmd` lets std spawn it (via its batch-file handling, which
/// escapes arguments safely). Non-Windows and already-qualified programs
/// are returned unchanged.
fn resolve_program(program: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        use std::path::Path;
        let p = Path::new(program);
        if p.extension().is_some() || p.components().count() > 1 {
            return program.to_string();
        }
        let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        let exts: Vec<String> = pathext
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|e| e.to_string())
            .collect();
        if let Some(paths) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&paths) {
                for ext in &exts {
                    let candidate = dir.join(format!("{program}{ext}"));
                    if candidate.is_file() {
                        return candidate.to_string_lossy().into_owned();
                    }
                }
            }
        }
    }
    program.to_string()
}

/// Parse `path:line:col` into `(path, Option<line>, Option<col>)`.
fn parse_file_location(s: &str) -> (&str, Option<u32>, Option<u32>) {
    // Try path:line:col
    if let Some((rest, col_s)) = s.rsplit_once(':')
        && let Ok(col) = col_s.parse::<u32>()
    {
        if let Some((path, line_s)) = rest.rsplit_once(':')
            && let Ok(line) = line_s.parse::<u32>()
        {
            // Don't split on Windows drive letter (C:)
            if !(path.is_empty() || path.len() == 1 && path.as_bytes()[0].is_ascii_alphabetic()) {
                return (path, Some(line), Some(col));
            }
        }
        // path:line only
        if !(rest.is_empty() || rest.len() == 1 && rest.as_bytes()[0].is_ascii_alphabetic()) {
            return (rest, Some(col), None);
        }
    }
    (s, None, None)
}

#[cfg(test)]
mod tests {
    use super::{is_file_path_link, parse_file_location, split_editor_command};

    #[test]
    fn split_editor_command_bare_program() {
        assert_eq!(split_editor_command("nvim"), ("nvim".to_string(), vec![]));
        assert_eq!(split_editor_command("  hx  "), ("hx".to_string(), vec![]));
    }

    #[test]
    fn split_editor_command_program_with_args() {
        assert_eq!(
            split_editor_command("code --wait"),
            ("code".to_string(), vec!["--wait".to_string()])
        );
        assert_eq!(
            split_editor_command("nvim -u NONE   -n"),
            (
                "nvim".to_string(),
                vec!["-u".to_string(), "NONE".to_string(), "-n".to_string()]
            )
        );
    }

    #[test]
    fn split_editor_command_honours_double_quotes() {
        assert_eq!(
            split_editor_command(r#""C:\Program Files\Editor\ed.exe" -n"#),
            (
                r"C:\Program Files\Editor\ed.exe".to_string(),
                vec!["-n".to_string()]
            )
        );
        // Quotes may wrap only part of a word.
        assert_eq!(
            split_editor_command(r#"ed --title="a b""#),
            ("ed".to_string(), vec!["--title=a b".to_string()])
        );
    }

    #[test]
    fn parse_file_location_variants() {
        assert_eq!(
            parse_file_location("src/main.rs"),
            ("src/main.rs", None, None)
        );
        assert_eq!(
            parse_file_location("src/main.rs:12"),
            ("src/main.rs", Some(12), None)
        );
        assert_eq!(
            parse_file_location("src/main.rs:12:3"),
            ("src/main.rs", Some(12), Some(3))
        );
        // Drive letters are not line numbers.
        assert_eq!(
            parse_file_location(r"C:\x\y.rs:7"),
            (r"C:\x\y.rs", Some(7), None)
        );
    }

    #[test]
    fn urls_are_not_file_paths() {
        assert!(!is_file_path_link("https://example.com/a.rs"));
        assert!(!is_file_path_link("HTTPS://EXAMPLE.COM/x"));
        assert!(!is_file_path_link("www.example.com/x.html"));
        assert!(is_file_path_link("src/main.rs:12"));
        assert!(is_file_path_link(r"C:\x\y.rs"));
        // Bare filenames are links too — previously refused at open time.
        assert!(is_file_path_link("Cargo.toml"));
        assert!(is_file_path_link("main.rs"));
        assert!(is_file_path_link("Program.cs:12:5"));
    }
}
