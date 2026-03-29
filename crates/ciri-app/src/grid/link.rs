use super::types::{LinkMatch, RowChar};
use super::ClientPaneGrid;

impl ClientPaneGrid {
    pub fn link_at(&self, col: u16, buffer_row: usize) -> Option<LinkMatch> {
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
        if end_idx == 0 {
            return None;
        }
        end_idx -= 1;
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
    } else {
        // Bare path: must contain a separator AND have a file-like component
        // e.g. "src/main.rs", "crates/foo/lib.rs"
        (path_part.contains('/') || path_part.contains('\\')) && has_file_extension(path_part)
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
    let file_name = path
        .rsplit(|c: char| c == '/' || c == '\\')
        .next()
        .unwrap_or(path);
    // Must contain a dot that's not at the start (not hidden files alone)
    if let Some(dot_pos) = file_name.rfind('.') {
        let ext = &file_name[dot_pos + 1..];
        // Extension must be 1-12 chars, all alphanumeric
        !ext.is_empty() && ext.len() <= 12 && ext.chars().all(|c| c.is_alphanumeric())
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
