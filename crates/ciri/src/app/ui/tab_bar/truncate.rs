/// Clip `s` to at most `max_cols` display columns, appending "…" if we
/// dropped any characters. Returns an empty string if `max_cols == 0`.
pub(super) fn truncate_to_cols(s: &str, max_cols: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    if max_cols == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut col = 0usize;
    let mut iter = s.chars().peekable();
    while let Some(&ch) = iter.peek() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        // Reserve one column for the ellipsis if more chars follow.
        let remaining_after_this = {
            let mut rest = iter.clone();
            rest.next();
            rest.next().is_some()
        };
        let needed = if remaining_after_this { w + 1 } else { w };
        if col + needed > max_cols {
            if col + 1 <= max_cols {
                out.push('…');
            }
            return out;
        }
        out.push(ch);
        col += w;
        iter.next();
    }
    out
}

#[cfg(test)]
#[path = "truncate_tests.rs"]
mod tests;
