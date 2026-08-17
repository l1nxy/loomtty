use super::ClientPaneGrid;
use super::types::LinkMatch;
use loom_protocol::message::FLAG_WRAPLINE;

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
            start_row: buffer_row,
            end_col,
            end_row: buffer_row,
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

        // Both detectors run over the *logical* line — soft-wrapped rows
        // joined back together — so a long URL or path that wraps onto
        // the next row is opened whole instead of truncated at the edge.
        let (line, row_offset) = self.logical_line_chars(buffer_row);
        let target_idx = row_offset
            + line[row_offset..]
                .iter()
                .take_while(|c| c.row == buffer_row)
                .position(|c| col >= c.start_col && col <= c.end_col)?;
        let chars: Vec<char> = line.iter().map(|c| c.ch).collect();

        let (start, end, url) = url_span_containing(&chars, target_idx)
            .map(|(s, e)| {
                let mut url: String = chars[s..=e].iter().collect();
                if url.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("www.")) {
                    url = format!("https://{url}");
                }
                (s, e, url)
            })
            .or_else(|| path_span_containing(&chars, target_idx))?;

        Some(LinkMatch {
            url,
            start_col: line[start].start_col,
            start_row: line[start].row,
            end_col: line[end].end_col,
            end_row: line[end].row,
        })
    }

    /// True when `buffer_row` soft-wraps onto the next row (the emulator
    /// set `WRAPLINE` on its last cell).
    fn row_is_wrapped(&self, buffer_row: usize) -> bool {
        self.row(buffer_row)
            .last()
            .is_some_and(|c| c.flags_u16() & FLAG_WRAPLINE != 0)
    }

    /// Collect the logical line containing `buffer_row`: walk up through
    /// rows whose predecessor is wrapped, then down while the current
    /// row is wrapped, and flatten every row's characters in order. Each
    /// entry remembers its (row, col span) so a match can be mapped back
    /// to screen cells. Bounded by `MAX_JOINED_ROWS` in each direction so
    /// a pathological single-line blob can't turn every mouse move into a
    /// scan of the whole scrollback.
    fn logical_line_chars(&self, buffer_row: usize) -> (Vec<LineChar>, usize) {
        let mut first = buffer_row;
        let mut walked = 0;
        while first > 0 && walked < MAX_JOINED_ROWS && self.row_is_wrapped(first - 1) {
            first -= 1;
            walked += 1;
        }
        let mut last = buffer_row;
        walked = 0;
        while last + 1 < self.buffer_len() && walked < MAX_JOINED_ROWS && self.row_is_wrapped(last)
        {
            last += 1;
            walked += 1;
        }

        let mut out = Vec::new();
        let mut target_offset = 0;
        for r in first..=last {
            if r == buffer_row {
                target_offset = out.len();
            }
            for rc in self.row_chars(self.row(r)) {
                out.push(LineChar {
                    row: r,
                    start_col: rc.start_col,
                    end_col: rc.end_col,
                    ch: rc.ch,
                });
            }
        }
        (out, target_offset)
    }
}

/// Upper bound on soft-wrapped rows joined above/below the hovered row
/// when scanning for URLs.
const MAX_JOINED_ROWS: usize = 32;

/// One character of a logical (wrap-joined) line, tagged with the screen
/// cell(s) it occupies.
struct LineChar {
    row: usize,
    start_col: u16,
    end_col: u16,
    ch: char,
}

// ── Plain-text URL scanning ─────────────────────────────────────
//
// Modelled on VS Code's `linkComputer`: find a scheme, then extend
// forward until a character that can't continue the URL. Brackets are
// tracked while extending — a `)` / `]` / `}` with no matching opener
// *inside* the URL ends it, which is what makes markdown `[t](https://…)`
// and prose `(see https://…)` come out clean while Wikipedia-style
// `…/Rust_(language)` keeps its balanced pair. Quote characters end
// the URL unless it is wrapped in a *different* quote, and `*` / `|`
// end it when the URL itself was introduced by that character
// (`**https://…**`, `|https://…|`).
//
// Wide (East Asian) characters and CJK punctuation always terminate:
// in Chinese/Japanese prose URLs are routinely glued to the text around
// them (`详见https://…。`), and raw CJK inside a URL is far rarer in a
// terminal than CJK right after one.

/// Locate the URL that contains `target_idx`, returning inclusive
/// `(start, end)` indices into `chars`.
fn url_span_containing(chars: &[char], target_idx: usize) -> Option<(usize, usize)> {
    let mut i = 0;
    while i <= target_idx {
        if let Some(scheme_len) = url_scheme_at(chars, i) {
            match url_end(chars, i, scheme_len) {
                Some(end) => {
                    if target_idx <= end {
                        return Some((i, end));
                    }
                    // Skip past this URL — nothing inside it can start
                    // a URL that also contains the target.
                    i = end + 1;
                    continue;
                }
                None => {
                    i += scheme_len;
                    continue;
                }
            }
        }
        i += 1;
    }
    None
}

/// If a supported URL scheme starts at `i`, return its length.
/// Only `http://`, `https://` and bare `www.` are recognised — these are
/// the forms the client will actually open, so anything else would show
/// a link that refuses to activate.
fn url_scheme_at(chars: &[char], i: usize) -> Option<usize> {
    fn matches_at(chars: &[char], i: usize, pat: &str) -> bool {
        let n = pat.len();
        i + n <= chars.len()
            && chars[i..i + n]
                .iter()
                .zip(pat.chars())
                .all(|(c, p)| c.eq_ignore_ascii_case(&p))
    }
    if matches_at(chars, i, "https://") {
        return Some(8);
    }
    if matches_at(chars, i, "http://") {
        return Some(7);
    }
    if matches_at(chars, i, "www.") {
        // `www.` must start a token-ish thing: not `foo.www.` / `awww.`.
        let prev_ok = i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '.');
        if prev_ok {
            return Some(4);
        }
    }
    None
}

/// Extend a URL starting at `start` (whose scheme is `scheme_len` chars)
/// and return the inclusive index of its last character, or `None` if
/// nothing usable follows the scheme.
fn url_end(chars: &[char], start: usize, scheme_len: usize) -> Option<usize> {
    let intro = if start > 0 {
        Some(chars[start - 1])
    } else {
        None
    };
    let (mut parens, mut brackets, mut braces) = (0usize, 0usize, 0usize);
    let mut j = start + scheme_len;
    while j < chars.len() {
        let ch = chars[j];
        let terminate = match ch {
            '(' => {
                parens += 1;
                false
            }
            ')' => {
                if parens > 0 {
                    parens -= 1;
                    false
                } else {
                    true
                }
            }
            '[' => {
                brackets += 1;
                false
            }
            ']' => {
                if brackets > 0 {
                    brackets -= 1;
                    false
                } else {
                    true
                }
            }
            '{' => {
                braces += 1;
                false
            }
            '}' => {
                if braces > 0 {
                    braces -= 1;
                    false
                } else {
                    true
                }
            }
            '\'' | '"' | '`' => match intro {
                Some(q) if q == ch => true,
                Some('\'' | '"' | '`') => false,
                _ => true,
            },
            '*' | '|' => intro == Some(ch),
            '<' | '>' | '\\' | '^' => true,
            c if c.is_whitespace() || c.is_control() => true,
            c if !c.is_ascii() && terminates_url_non_ascii(c) => true,
            _ => false,
        };
        if terminate {
            break;
        }
        j += 1;
    }

    // Trailing sentence punctuation is almost never part of the URL.
    let body_start = start + scheme_len;
    let mut end = j;
    while end > body_start && matches!(chars[end - 1], '.' | ',' | ';' | ':' | '!' | '?') {
        end -= 1;
    }
    if !chars[body_start..end].iter().any(|c| c.is_alphanumeric()) {
        return None;
    }
    Some(end - 1)
}

/// Non-ASCII characters that end a URL. Two classes:
/// - anything that isn't a letter/digit — typographic quotes, dashes,
///   ellipsis, arrows, guillemets, emoji, non-breaking space, and every
///   fullwidth/CJK punctuation mark;
/// - anything East-Asian-wide (CJK ideographs, kana, hangul).
///
/// Narrow non-ASCII letters (`café`, Cyrillic, Greek…) stay part of the
/// URL, matching how they appear unescaped in practice.
fn terminates_url_non_ascii(c: char) -> bool {
    use unicode_width::UnicodeWidthChar;
    !c.is_alphanumeric() || c.width().is_some_and(|w| w >= 2)
}

// ── Plain-text file-path scanning ───────────────────────────────
//
// Paths have no scheme to anchor on, so the candidate is the run of
// "path characters" around the hovered cell — everything except
// whitespace, shell/markup delimiters (`<>|"'\`*=^`), and non-ASCII
// punctuation. Non-ASCII *letters* stay in, because Chinese/Japanese
// directory and file names are common (`C:\Users\x\桌面\a.rs`,
// `src/中文.md`). The run is then trimmed of wrapping ASCII punctuation
// and unmatched brackets and, because CJK prose is routinely glued to a
// path with no space (`请看src/main.rs文件`), of a leading wide-character
// run that sits in front of an ASCII path start and a trailing
// wide-character run after the extension. Each trimmed candidate is
// validated by `detect_file_path`, most-trimmed first, so `中文.md`
// (which only validates untrimmed) still works.
//
// A path quoted with `"…"` / `'…'` is tried first, quotes excluded, so
// Windows paths with spaces (`"C:\Program Files\x\y.rs"`) open whole.

/// Locate the file path that contains `target_idx`. Returns inclusive
/// `(start, end)` indices into `chars` plus the normalised
/// `path[:line[:col]]` string.
fn path_span_containing(chars: &[char], target_idx: usize) -> Option<(usize, usize, String)> {
    if let Some(hit) = quoted_path_span(chars, target_idx) {
        return Some(hit);
    }
    if !is_path_char(chars[target_idx]) {
        return None;
    }

    let mut start = target_idx;
    while start > 0 && is_path_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = target_idx;
    while end + 1 < chars.len() && is_path_char(chars[end + 1]) {
        end += 1;
    }

    let (mut start, mut end) = trim_path_token(chars, start, end, target_idx)?;

    loop {
        for (s, e) in path_candidates(chars, start, end) {
            if target_idx < s || target_idx > e {
                continue;
            }
            let token: String = chars[s..=e].iter().collect();
            if let Some(url) = detect_file_path(&token) {
                return Some((s, e, url));
            }
        }
        // Nothing validated: if the target sits inside a balanced bracket
        // pair (`include(src/main.rs)`, `foo[docs/a.md]`), retry with just
        // the bracketed interior — the outer text was a wrapper, not path.
        let (open, close) = innermost_enclosing_pair(chars, start, end, target_idx)?;
        if open + 1 > close - 1 {
            return None;
        }
        (start, end) = trim_path_token(chars, open + 1, close - 1, target_idx)?;
    }
}

/// Innermost matched bracket pair inside `chars[start..=end]` that
/// strictly encloses `target`, as `(open_idx, close_idx)`.
fn innermost_enclosing_pair(
    chars: &[char],
    start: usize,
    end: usize,
    target: usize,
) -> Option<(usize, usize)> {
    let mut stack: Vec<(usize, char)> = Vec::new();
    let mut best: Option<(usize, usize)> = None;
    for (i, &ch) in chars.iter().enumerate().take(end + 1).skip(start) {
        match ch {
            c @ ('(' | '[' | '{') => stack.push((i, c)),
            c @ (')' | ']' | '}') => {
                if let Some(&(open, oc)) = stack.last()
                    && Some(oc) == closer_to_opener(c)
                {
                    stack.pop();
                    if open < target && target < i && best.is_none_or(|(bo, _)| open > bo) {
                        best = Some((open, i));
                    }
                }
            }
            _ => {}
        }
    }
    best
}

/// If `target_idx` lies inside a `"…"` or `'…'` pair on the line whose
/// contents validate as a path, return that span (quotes excluded).
/// Spaces are allowed inside — that is the point — but the span is
/// capped so a stray quote far away can't pull in half the line.
fn quoted_path_span(chars: &[char], target_idx: usize) -> Option<(usize, usize, String)> {
    const MAX_QUOTED: usize = 512;
    for quote in ['"', '\''] {
        if chars[target_idx] == quote {
            continue;
        }
        let Some(open) = chars[..target_idx]
            .iter()
            .rposition(|&c| c == quote)
            .filter(|&o| target_idx - o <= MAX_QUOTED)
        else {
            continue;
        };
        let Some(close) = chars[target_idx + 1..]
            .iter()
            .position(|&c| c == quote)
            .map(|p| p + target_idx + 1)
            .filter(|&c| c - target_idx <= MAX_QUOTED)
        else {
            continue;
        };
        let (s, e) = (open + 1, close - 1);
        if s > e {
            continue;
        }
        // The quoted text must itself look like one path token — spaces
        // are the only extra freedom quoting buys.
        if chars[s..=e]
            .iter()
            .any(|&c| c.is_control() || (c != ' ' && !is_path_char(c)))
        {
            continue;
        }
        let token: String = chars[s..=e].iter().collect();
        if let Some(url) = detect_file_path(token.trim()) {
            // Trim surrounding spaces off the reported span too.
            let mut ts = s;
            while ts < e && chars[ts] == ' ' {
                ts += 1;
            }
            let mut te = e;
            while te > ts && chars[te] == ' ' {
                te -= 1;
            }
            if target_idx >= ts && target_idx <= te {
                return Some((ts, te, url));
            }
        }
    }
    None
}

/// Characters that can be part of a file-path token.
fn is_path_char(c: char) -> bool {
    if c.is_ascii() {
        !(c.is_ascii_whitespace()
            || c.is_ascii_control()
            || matches!(c, '<' | '>' | '|' | '"' | '\'' | '`' | '*' | '=' | '^'))
    } else {
        // Letters/digits of any script (CJK file names are common);
        // fullwidth/typographic punctuation is prose, not path.
        c.is_alphanumeric()
    }
}

/// East-Asian-wide character (CJK ideographs, kana, hangul, emoji…).
fn is_wide(c: char) -> bool {
    use unicode_width::UnicodeWidthChar;
    c.width().is_some_and(|w| w >= 2)
}

/// Candidate `(start, end)` spans for a path token, most-trimmed first:
/// with a leading wide run stripped when it fronts an ASCII path start
/// (`请看src/main.rs` → `src/main.rs`, but `桌面/main.rs` is kept), with a
/// trailing wide run stripped (`main.rs文件` → `main.rs`), both, and the
/// token as-is. Duplicates are removed.
fn path_candidates(chars: &[char], start: usize, end: usize) -> Vec<(usize, usize)> {
    let mut lead = start;
    while lead <= end && is_wide(chars[lead]) {
        lead += 1;
    }
    let lead_ok = lead > start
        && lead <= end
        && (chars[lead].is_ascii_alphanumeric() || matches!(chars[lead], '.' | '~'));
    let lead = if lead_ok { lead } else { start };

    let mut trail = end;
    while trail > start && is_wide(chars[trail]) {
        trail -= 1;
    }
    let trail_ok = trail < end && trail >= lead && chars[trail].is_ascii_alphanumeric();
    let trail = if trail_ok { trail } else { end };

    let mut out = Vec::with_capacity(4);
    for cand in [(lead, trail), (lead, end), (start, trail), (start, end)] {
        if cand.0 <= cand.1 && !out.contains(&cand) {
            out.push(cand);
        }
    }
    out
}

/// Strip wrapping punctuation from a path token around `target`:
/// leading openers/quotes, then any bracket that has no partner inside
/// the token — an unmatched closer/opener before the target moves the
/// start past it, one after the target ends the token there — and
/// finally trailing sentence punctuation. Balanced pairs are kept, so
/// `Program.cs(12,5)`, `foo (1).txt` and `src/[id].tsx` survive while
/// `[text](src/main.rs)` and `[src/a.rs](src/a.rs)` yield just the path.
fn trim_path_token(
    chars: &[char],
    mut start: usize,
    mut end: usize,
    target: usize,
) -> Option<(usize, usize)> {
    loop {
        while start <= end && is_leading_link_punctuation(chars[start]) {
            start += 1;
        }
        if start > end || target < start || target > end {
            return None;
        }
        let base = start;
        let unmatched = unmatched_brackets(&chars[start..=end]);
        if unmatched.is_empty() {
            break;
        }
        let mut changed = false;
        for k in unmatched.into_iter().map(|k| k + base) {
            if k == target {
                return None;
            }
            if k < target && k + 1 > start {
                start = k + 1;
                changed = true;
            } else if k > target && k < end + 1 {
                end = k - 1;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    while start <= end && is_trailing_link_punctuation(chars[end]) {
        let trailing = chars[end];
        if let Some(opener) = closer_to_opener(trailing) {
            let open_count = chars[start..=end].iter().filter(|&&c| c == opener).count();
            let close_count = chars[start..=end]
                .iter()
                .filter(|&&c| c == trailing)
                .count();
            if close_count > open_count {
                if end == 0 {
                    return None;
                }
                end -= 1;
            } else {
                break;
            }
        } else {
            if end == 0 {
                return None;
            }
            end -= 1;
        }
    }
    if start > end || target < start || target > end {
        None
    } else {
        Some((start, end))
    }
}

/// Indices (into `chars`) of brackets with no partner inside `chars`:
/// closers with nothing open, closers of the wrong kind, and openers
/// never closed.
fn unmatched_brackets(chars: &[char]) -> Vec<usize> {
    let mut stack: Vec<(usize, char)> = Vec::new();
    let mut out = Vec::new();
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '(' | '[' | '{' => stack.push((i, c)),
            ')' | ']' | '}' => {
                let want = closer_to_opener(c);
                match stack.last() {
                    Some(&(_, open)) if Some(open) == want => {
                        stack.pop();
                    }
                    _ => out.push(i),
                }
            }
            _ => {}
        }
    }
    out.extend(stack.into_iter().map(|(i, _)| i));
    out.sort_unstable();
    out
}

/// True if `s` is something the file-path detector would produce —
/// used by the opener to route a link to `$EDITOR` rather than the
/// browser. Kept next to `detect_file_path` so the two can't drift.
pub fn looks_like_file_path(s: &str) -> bool {
    detect_file_path(s).is_some()
}

/// Detect file path patterns in a token. Returns the path with any
/// location suffix normalised to `path:line[:col]`.
///
/// Recognized patterns:
/// - Absolute Unix: `/foo/bar.rs`, `/foo/bar.rs:42:10`
/// - Absolute Windows: `C:\foo\bar.rs`, `C:/foo/bar.rs`
/// - Relative: `./foo.rs`, `../foo.rs`
/// - Home: `~/foo/bar.rs`
/// - Bare paths with extension: `src/main.rs`, `Cargo.toml`
/// - Location suffixes: `:line`, `:line:col`, and the MSVC / tsc / dotnet
///   form `(line,col)` / `(line)`
pub(super) fn detect_file_path(token: &str) -> Option<String> {
    // A token carrying a scheme separator is a URL (or URL-ish junk the
    // URL scanner already rejected), never a file path. Without this,
    // `链接：https://x.com` would be offered as a "path" whose activation
    // the opener then refuses — a link that highlights but never opens.
    if token.contains("://") {
        return None;
    }

    // Strip trailing `:line:col` / `(line,col)` suffix, keeping it for the result
    let (path_part, suffix) = match split_path_paren_loc(token) {
        Some((path, line, col)) => (
            path,
            match col {
                Some(c) => format!(":{line}:{c}"),
                None => format!(":{line}"),
            },
        ),
        None => {
            let (path, suffix) = split_path_line_col(token);
            (path, suffix.to_string())
        }
    };

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

    Some(format!("{path_part}{suffix}"))
}

/// Split the MSVC / tsc / dotnet location form `path(line,col)` or
/// `path(line)` into `(path, line, col)`.
fn split_path_paren_loc(token: &str) -> Option<(&str, u32, Option<u32>)> {
    let inner = token.strip_suffix(')')?;
    let open = inner.rfind('(')?;
    let (path, loc) = (&inner[..open], &inner[open + 1..]);
    if path.is_empty() || loc.is_empty() {
        return None;
    }
    let (line, col) = match loc.split_once(',') {
        Some((l, c)) => (l, Some(c)),
        None => (loc, None),
    };
    let line: u32 = line.parse().ok()?;
    let col = match col {
        Some(c) => Some(c.parse::<u32>().ok()?),
        None => None,
    };
    Some((path, line, col))
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
/// Deliberately a fixed list: a bare `word.ext` is only offered as a
/// link when `ext` is something people actually open in an editor, so
/// prose like `e.g.`, `a.m.` or `v1.2` never lights up. Single-letter
/// extensions other than `c`/`h` are left out for the same reason.
const BARE_FILENAME_EXTENSIONS: &[&str] = &[
    // systems / general
    "rs",
    "go",
    "c",
    "cc",
    "cpp",
    "cxx",
    "h",
    "hh",
    "hpp",
    "hxx",
    "mm",
    "asm",
    "zig",
    "nim",
    "cs",
    "fs",
    "fsx",
    "vb",
    "java",
    "kt",
    "kts",
    "scala",
    "groovy",
    "swift",
    "dart",
    "rb",
    "py",
    "pyi",
    "php",
    "pl",
    "pm",
    "lua",
    "jl",
    "hs",
    "ml",
    "mli",
    "ex",
    "exs",
    "erl",
    "clj",
    "cljs",
    "elm",
    "cr",
    "gd", // scripting / web
    "js",
    "mjs",
    "cjs",
    "jsx",
    "ts",
    "mts",
    "cts",
    "tsx",
    "vue",
    "svelte",
    "astro",
    "html",
    "htm",
    "css",
    "scss",
    "sass",
    "less",
    "sh",
    "bash",
    "zsh",
    "fish",
    "ps1",
    "psm1",
    "bat",
    "cmd",
    "nu",
    // shaders
    "wgsl",
    "glsl",
    "hlsl",
    "vert",
    "frag", // data / config / docs
    "md",
    "mdx",
    "rst",
    "adoc",
    "txt",
    "toml",
    "yaml",
    "yml",
    "json",
    "jsonc",
    "json5",
    "xml",
    "svg",
    "csv",
    "tsv",
    "ini",
    "cfg",
    "conf",
    "env",
    "lock",
    "log",
    "sql",
    "proto",
    "graphql",
    "gql",
    "tex",
    "bib",
    "ipynb",
    "ron",
    "nix",
    "tf",
    "hcl",
    "gradle",
    "cmake",
    "mk",
    "ninja",
    "sln",
    "csproj",
    "fsproj",
    "vcxproj",
    "props",
    "targets",
    "xaml",
    "plist",
    "dockerfile",
    "makefile",
    "mod",
    "sum",
    "work",
    "editorconfig",
    "gitignore",
];

/// Special filenames without extensions that are recognized as files.
const SPECIAL_FILENAMES: &[&str] = &[
    "Makefile",
    "Dockerfile",
    "Cargo.lock",
    "README",
    "LICENSE",
    "CHANGELOG",
    "CODEOWNERS",
    "Gemfile",
    "Rakefile",
    "Vagrantfile",
    "Justfile",
    "Procfile",
    "Brewfile",
    "Podfile",
    "Fastfile",
    "Appfile",
    "Cartfile",
    ".gitignore",
    ".gitattributes",
    ".gitmodules",
    ".editorconfig",
    ".env",
    ".envrc",
    ".npmrc",
    ".nvmrc",
    ".prettierrc",
    ".eslintrc",
    ".bashrc",
    ".zshrc",
    ".profile",
    ".vimrc",
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
        BARE_FILENAME_EXTENSIONS.contains(&ext_lower.as_str())
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
