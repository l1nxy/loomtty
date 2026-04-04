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
                    start: (link.start_col, buffer_row),
                    end: (link.end_col, buffer_row),
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
pub(crate) fn link_activation_modifier_active(
    modifiers: winit::keyboard::ModifiersState,
) -> bool {
    modifiers.super_key()
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn link_activation_modifier_active(
    modifiers: winit::keyboard::ModifiersState,
) -> bool {
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
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("ShellExecuteW failed with code {result}"),
            ));
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

/// Check if a link string looks like a file path rather than a URL.
fn is_file_path_link(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("www.") {
        return false;
    }
    // Strip :line:col suffix for path analysis
    let (path, _, _) = parse_file_location(s);
    // Unix absolute, relative, home
    if path.starts_with('/')
        || path.starts_with("./")
        || path.starts_with("../")
        || path.starts_with("~/")
    {
        return true;
    }
    // Windows absolute: C:\ or C:/
    if path.len() >= 3
        && path.as_bytes()[0].is_ascii_alphabetic()
        && path.as_bytes()[1] == b':'
        && (path.as_bytes()[2] == b'\\' || path.as_bytes()[2] == b'/')
    {
        return true;
    }
    // Bare path with separator — must also have a file extension to avoid
    // false positives (consistent with detect_file_path in grid.rs).
    if (path.contains('/') || path.contains('\\')) && !path.contains("://") {
        // Require a dot in the last path component (file extension)
        let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        return file_name.contains('.');
    }
    false
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

    // Try $EDITOR first (supports line numbers)
    if let Ok(editor) = std::env::var("EDITOR") {
        let editor_lower = editor.to_ascii_lowercase();
        if editor_lower.contains("code") || editor_lower.contains("cursor") {
            let mut loc = resolved.to_string();
            if let Some(l) = line {
                loc = format!("{loc}:{l}");
            }
            return Command::new(&editor)
                .args(["--goto", &loc])
                .spawn()
                .map(|_| ());
        }
        if editor_lower.contains("vim")
            || editor_lower.contains("nvim")
            || editor_lower.contains("hx")
        {
            let mut args = Vec::new();
            if let Some(l) = line {
                args.push(format!("+{l}"));
            }
            args.push(resolved.to_string());
            return Command::new(&editor).args(&args).spawn().map(|_| ());
        }
        return Command::new(&editor)
            .arg(resolved.as_ref())
            .spawn()
            .map(|_| ());
    }

    // No $EDITOR set — refuse to open file paths via OS handler.
    // OS handlers (cmd /C start, open, xdg-open) can execute arbitrary
    // binaries (.exe, .bat, .app), which is unsafe for untrusted paths
    // from terminal output. URLs are fine (browser is sandboxed), but
    // file paths need an explicit editor.
    Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "set $EDITOR to open file paths (OS handler refused for safety)",
    ))
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
