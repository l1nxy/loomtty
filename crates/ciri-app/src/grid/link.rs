use super::ClientPaneGrid;
use super::types::{LinkMatch, RowChar};

impl ClientPaneGrid {
    /// Check if a cell has an explicit OSC 8 hyperlink.
    /// Returns the link match with the full span of contiguous cells sharing the same link ID.
    fn osc8_link_at(&self, col: u16, buffer_row: usize) -> Option<LinkMatch> {
        // OSC 8 data is viewport-only; convert buffer_row to viewport row
        let viewport_row = buffer_row.checked_sub(self.scrollback.len())?;
        if viewport_row >= self.rows as usize {
            return None;
        }
        let cols = self.cols as usize;
        let cell_idx = (viewport_row * cols + col as usize) as u32;
        let &link_id = self.hyperlink_cell_map.get(&cell_idx)?;

        // Look up URI from hyperlink_map
        let uri = self.hyperlink_map.get(&link_id)?;

        // Expand span: walk left
        let mut start_col = col;
        while start_col > 0 {
            let prev_idx = (viewport_row * cols + (start_col - 1) as usize) as u32;
            match self.hyperlink_cell_map.get(&prev_idx) {
                Some(&id) if id == link_id => start_col -= 1,
                _ => break,
            }
        }

        // Walk right
        let mut end_col = col;
        while (end_col + 1) < self.cols {
            let next_idx = (viewport_row * cols + (end_col + 1) as usize) as u32;
            match self.hyperlink_cell_map.get(&next_idx) {
                Some(&id) if id == link_id => end_col += 1,
                _ => break,
            }
        }

        Some(LinkMatch {
            url: uri.to_string(),
            start_col,
            end_col,
        })
    }

    pub fn link_at(&self, col: u16, buffer_row: usize) -> Option<LinkMatch> {
        // OSC 8 fast path: check if this cell has an explicit hyperlink
        if let Some(link) = self.osc8_link_at(col, buffer_row) {
            return Some(link);
        }

        if buffer_row >= self.buffer_len() {
            return None;
        }
        let row = self.row(buffer_row);
        let chars = self.row_chars(row);
        let target_idx = chars
            .iter()
            .position(|cell| col >= cell.start_col && col <= cell.end_col)?;

        let mut start_idx = target_idx;
        while start_idx > 0 && !chars[start_idx - 1].ch.is_whitespace() {
            start_idx -= 1;
        }

        let mut end_idx = target_idx;
        while end_idx + 1 < chars.len() && !chars[end_idx + 1].ch.is_whitespace() {
            end_idx += 1;
        }

        let (start_idx, end_idx) = trim_link_token(&chars, start_idx, end_idx)?;
        if target_idx < start_idx || target_idx > end_idx {
            return None;
        }

        let token: String = chars[start_idx..=end_idx]
            .iter()
            .map(|cell| cell.ch)
            .collect();
        let url = normalize_link_token(&token)?;
        Some(LinkMatch {
            url,
            start_col: chars[start_idx].start_col,
            end_col: chars[end_idx].end_col,
        })
    }
}

fn trim_link_token(
    chars: &[RowChar],
    mut start_idx: usize,
    mut end_idx: usize,
) -> Option<(usize, usize)> {
    while start_idx <= end_idx && is_leading_link_punctuation(chars[start_idx].ch) {
        start_idx += 1;
    }
    while start_idx <= end_idx && is_trailing_link_punctuation(chars[end_idx].ch) {
        let trailing = chars[end_idx].ch;

        // Preserve trailing colon when it's part of a port/line number
        if trailing == ':' && should_preserve_trailing_colon(chars, end_idx, end_idx) {
            break;
        }

        // Bracket-aware trimming: check if the trailing closer is balanced within the token
        if let Some(opener) = closer_to_opener(trailing) {
            let open_count = chars[start_idx..=end_idx]
                .iter()
                .filter(|c| c.ch == opener)
                .count();
            let close_count = chars[start_idx..=end_idx]
                .iter()
                .filter(|c| c.ch == trailing)
                .count();

            if close_count > open_count {
                // Excess trailing closers — strip one
                if end_idx == 0 {
                    return None;
                }
                end_idx -= 1;
            } else {
                // Balanced or more openers than closers — the closer is part of the URL
                break;
            }
        } else {
            if end_idx == 0 {
                return None;
            }
            end_idx -= 1;
        }
    }
    if start_idx > end_idx {
        None
    } else {
        Some((start_idx, end_idx))
    }
}

fn normalize_link_token(token: &str) -> Option<String> {
    let lower = token.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        let scheme_len = if lower.starts_with("https://") { 8 } else { 7 };
        token[scheme_len..]
            .chars()
            .any(|ch| ch.is_alphanumeric())
            .then(|| token.to_string())
    } else if lower.starts_with("www.") {
        token[4..]
            .chars()
            .any(|ch| ch.is_alphanumeric())
            .then(|| format!("https://{token}"))
    } else {
        detect_file_path(token)
    }
}

/// Detect file path patterns in a token. Returns the path (with optional :line:col).
///
/// Recognized patterns:
/// - Absolute Unix: `/foo/bar.rs`, `/foo/bar.rs:42:10`
/// - Absolute Windows: `C:\foo\bar.rs`, `C:/foo/bar.rs`
/// - Relative: `./foo.rs`, `../foo.rs`
/// - Home: `~/foo/bar.rs`
/// - Bare paths with extension: `src/main.rs`, `Cargo.toml`
pub(super) fn detect_file_path(token: &str) -> Option<String> {
    // Strip trailing `:line:col` or `:line` suffix, keeping it for the result
    let (path_part, _suffix) = split_path_line_col(token);

    if path_part.is_empty() {
        return None;
    }

    let is_file_path = if path_part.starts_with('/')
        || path_part.starts_with("~/")
        || path_part.starts_with("./")
        || path_part.starts_with("../")
    {
        // Unix absolute, home, or relative path
        true
    } else if path_part.len() >= 3
        && path_part.as_bytes()[0].is_ascii_alphabetic()
        && (path_part.as_bytes()[1] == b':')
        && (path_part.as_bytes()[2] == b'\\' || path_part.as_bytes()[2] == b'/')
    {
        // Windows absolute path: C:\foo or C:/foo
        true
    } else if path_part.contains('/') || path_part.contains('\\') {
        // Bare path with separator: e.g. "src/main.rs", "crates/foo/lib.rs"
        has_file_extension(path_part)
    } else {
        // Bare filename without separator: must have a recognized extension
        // or be a known special filename
        is_bare_filename(path_part)
    };

    if !is_file_path {
        return None;
    }

    // Sanity check: path should have at least one alphanumeric char
    if !path_part.chars().any(|c| c.is_alphanumeric()) {
        return None;
    }

    // Reject paths that look like they're just punctuation or too short
    if path_part.len() < 2 {
        return None;
    }

    Some(token.to_string())
}

/// Split `path:line:col` into `(path, ":line:col")`.
/// Returns `(token, "")` if no line/col suffix is found.
pub(super) fn split_path_line_col(token: &str) -> (&str, &str) {
    // Walk backwards looking for `:digits` patterns
    // Handle: path:42 or path:42:10
    let bytes = token.as_bytes();
    let end = bytes.len();

    // Try to strip :col
    if let Some(pos) = rfind_colon_digits(bytes, end) {
        let after_first_strip = pos;
        // Try to strip another :line
        if let Some(pos2) = rfind_colon_digits(bytes, after_first_strip) {
            return (&token[..pos2], &token[pos2..]);
        }
        return (&token[..after_first_strip], &token[after_first_strip..]);
    }

    (token, "")
}

/// Find the position of `:` in `bytes[..end]` where `:` is followed by only digits.
fn rfind_colon_digits(bytes: &[u8], end: usize) -> Option<usize> {
    if end < 2 {
        return None;
    }
    // Find the last `:` before `end`
    let slice = &bytes[..end];
    let colon_pos = slice.iter().rposition(|&b| b == b':')?;
    // Everything after the colon must be digits (at least one)
    let after = &slice[colon_pos + 1..];
    if after.is_empty() || !after.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(colon_pos)
}

/// Check if a path has a recognizable file extension.
fn has_file_extension(path: &str) -> bool {
    // Get the last path component
    let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    // Must contain a dot that's not at the start (not hidden files alone)
    if let Some(dot_pos) = file_name.rfind('.') {
        let ext = &file_name[dot_pos + 1..];
        // Extension must be 1-12 chars, all alphanumeric
        !ext.is_empty() && ext.len() <= 12 && ext.chars().all(|c| c.is_alphanumeric())
    } else {
        false
    }
}

/// Recognized file extensions for bare filenames (no path separator).
const BARE_FILENAME_EXTENSIONS: &[&str] = &[
    "rs",
    "toml",
    "go",
    "py",
    "js",
    "ts",
    "tsx",
    "jsx",
    "md",
    "yaml",
    "yml",
    "json",
    "lock",
    "sh",
    "bash",
    "zsh",
    "fish",
    "c",
    "cpp",
    "h",
    "hpp",
    "java",
    "kt",
    "rb",
    "lua",
    "css",
    "html",
    "xml",
    "svg",
    "txt",
    "cfg",
    "ini",
    "conf",
    "dockerfile",
    "makefile",
];

/// Special filenames without extensions that are recognized as files.
const SPECIAL_FILENAMES: &[&str] = &[
    "Makefile",
    "Dockerfile",
    "Cargo.lock",
    "README",
    "LICENSE",
    "CHANGELOG",
    "Gemfile",
    "Rakefile",
    "Vagrantfile",
    "Justfile",
    ".gitignore",
    ".gitmodules",
    ".editorconfig",
];

/// Check if a bare token (no path separators) looks like a filename.
fn is_bare_filename(name: &str) -> bool {
    // Check special filenames first (case-insensitive)
    let name_lower = name.to_ascii_lowercase();
    if SPECIAL_FILENAMES
        .iter()
        .any(|s| s.to_ascii_lowercase() == name_lower)
    {
        return true;
    }

    // Check for recognized extension
    if let Some(dot_pos) = name.rfind('.') {
        if dot_pos == 0 {
            return false; // hidden file like ".foo" — not a bare filename
        }
        // Reject tokens where the part before the dot is all digits (e.g. "2.rs", "10.py")
        let stem = &name[..dot_pos];
        if stem.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
        let ext = &name[dot_pos + 1..];
        if ext.is_empty() {
            return false;
        }
        let ext_lower = ext.to_ascii_lowercase();
        BARE_FILENAME_EXTENSIONS
            .iter()
            .any(|&e| e == ext_lower.as_str())
    } else {
        false
    }
}

fn is_leading_link_punctuation(ch: char) -> bool {
    matches!(ch, '(' | '[' | '{' | '<' | '"' | '\'')
}

fn is_trailing_link_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '>' | '"' | '\''
    )
}

/// Check if a trailing `:` should be preserved (e.g. port number like `:8080`).
/// Returns true if the colon at `idx` is followed by digits within the token,
/// meaning it's likely a port number or line number — not sentence punctuation.
fn should_preserve_trailing_colon(chars: &[RowChar], idx: usize, end_idx: usize) -> bool {
    if idx == end_idx {
        // Colon is the very last character — it's sentence punctuation, strip it.
        return false;
    }
    // If followed by digits, it's a port number or line number — preserve.
    // This case doesn't normally reach here since `:8080` is not trailing,
    // but guard for future changes.
    chars.get(idx + 1).is_some_and(|c| c.ch.is_ascii_digit())
}

/// Map a closing bracket to its opening bracket, or None if not a bracket closer.
fn closer_to_opener(ch: char) -> Option<char> {
    match ch {
        ')' => Some('('),
        ']' => Some('['),
        '}' => Some('{'),
        '>' => Some('<'),
        _ => None,
    }
}
