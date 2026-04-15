//! Shape-based text measurement and truncation helpers for UI chrome.
//!
//! All non-terminal text in the UI is shaped through `cx.ui_shaper`. The
//! legacy "cell-grid" assumption (`cell_w * unicode_width(s)`) does not hold
//! for proportional UI fonts — it over-estimates narrow glyphs and
//! under-estimates wide ones, which desyncs layout slots from actual glyph
//! advance. The consequence is either clipped text or, when the difference
//! makes a slot too narrow for its own label, a `UiBuilder::label()` call
//! that fails to allocate and silently emits nothing (the bug that left
//! the session/mode indicators blank on the top bar).
//!
//! This module centralises the three primitives every bar/panel needs:
//! * [`measure`] — shape-aware pixel width
//! * [`prefix_fit`] — byte-cut and fitted width for a max pixel budget
//! * [`truncate_with_ellipsis`] — safe truncation with "…" suffix
//!
//! Every helper falls back to a cell-grid estimate when the shaper has no
//! face available (headless tests, very early startup). The fallback
//! matches what `UiBuilder::text_width` does so measurement stays
//! internally consistent within a single frame.

use unicode_width::UnicodeWidthStr;

use super::types::UiContext;

/// Shape-aware pixel width of `text` using `cx.ui_shaper`, or a
/// `cell_w * unicode_width` fallback when the shaper has no face yet.
pub(super) fn measure(cx: &UiContext<'_>, text: &str) -> f32 {
    if let Some(cell) = cx.ui_shaper {
        let mut shaper = cell.borrow_mut();
        if shaper.has_face() {
            return shaper.measure(text);
        }
    }
    UnicodeWidthStr::width(text) as f32 * cx.cell_w
}

/// Return the byte index of the longest prefix of `text` that fits in
/// `max_w` pixels, plus the fitted width. Falls back to a cell-grid
/// estimate when the shaper is unavailable.
pub(super) fn prefix_fit(cx: &UiContext<'_>, text: &str, max_w: f32) -> (usize, f32) {
    if let Some(cell) = cx.ui_shaper {
        let mut shaper = cell.borrow_mut();
        if shaper.has_face() {
            let (bytes, _) = shaper.prefix_fit(text, max_w);
            // `prefix_fit` may return a cluster boundary; snap to a char
            // boundary so subsequent `&text[..bytes]` indexing is safe for
            // combining sequences.
            let bytes = text.floor_char_boundary(bytes.min(text.len()));
            let w = shaper.measure(&text[..bytes]);
            return (bytes, w);
        }
    }
    // Cell-grid fallback: greedy char-by-char fit.
    use unicode_width::UnicodeWidthChar;
    let cw = cx.cell_w.max(0.0001);
    let max_cols = (max_w / cw).floor().max(0.0) as usize;
    let mut col = 0usize;
    let mut last_byte = 0usize;
    for (i, ch) in text.char_indices() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if col + w > max_cols {
            return (last_byte, col as f32 * cw);
        }
        col += w;
        last_byte = i + ch.len_utf8();
    }
    (text.len(), col as f32 * cw)
}

/// Truncate `text` to fit in `max_w` pixels, appending "…" when any
/// characters had to be dropped. Returns an empty string when `max_w`
/// cannot hold even the ellipsis.
pub(super) fn truncate_with_ellipsis(cx: &UiContext<'_>, text: &str, max_w: f32) -> String {
    if max_w <= 0.0 {
        return String::new();
    }
    let full_w = measure(cx, text);
    if full_w <= max_w {
        return text.to_string();
    }
    let ellipsis = "…";
    let ellipsis_w = measure(cx, ellipsis);
    if ellipsis_w > max_w {
        return String::new();
    }
    let budget = (max_w - ellipsis_w).max(0.0);
    let (cut, _) = prefix_fit(cx, text, budget);
    let cut = text.floor_char_boundary(cut.min(text.len()));
    let mut out = String::with_capacity(cut + ellipsis.len());
    out.push_str(&text[..cut]);
    out.push_str(ellipsis);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ciri_config::config::CiriConfig;

    fn mk_cx(cw: f32) -> (UiContext<'static>, CiriConfig) {
        // Allocate a static-ish config via Box::leak so the lifetime
        // matches `UiContext<'static>` in tests. This is fine for the
        // shape-less fallback path we exercise here.
        let cfg: &'static CiriConfig = Box::leak(Box::new(CiriConfig::default()));
        let cx = UiContext {
            config: cfg,
            viewport_w: 800.0,
            viewport_h: 600.0,
            cell_w: cw,
            cell_h: cw * 2.0,
            baseline: cw * 1.6,
            ui_shaper: None,
        };
        (cx, CiriConfig::default())
    }

    #[test]
    fn measure_fallback_uses_unicode_width() {
        let (cx, _cfg) = mk_cx(8.0);
        assert_eq!(measure(&cx, "abc"), 24.0);
        assert_eq!(measure(&cx, ""), 0.0);
    }

    #[test]
    fn prefix_fit_fallback_stops_at_budget() {
        let (cx, _cfg) = mk_cx(10.0);
        let (bytes, w) = prefix_fit(&cx, "hello", 25.0);
        // 2 chars at 10px each fits in 25px; 3rd would exceed.
        assert_eq!(bytes, 2);
        assert_eq!(w, 20.0);
    }

    #[test]
    fn truncate_fallback_appends_ellipsis_when_overflow() {
        let (cx, _cfg) = mk_cx(10.0);
        // Ellipsis is 1 char wide in the cell-grid fallback.
        let out = truncate_with_ellipsis(&cx, "abcdef", 30.0);
        assert!(out.ends_with('…'));
        assert!(measure(&cx, &out) <= 30.0 + 0.001);
    }

    #[test]
    fn truncate_fallback_returns_full_when_fits() {
        let (cx, _cfg) = mk_cx(10.0);
        let out = truncate_with_ellipsis(&cx, "abc", 100.0);
        assert_eq!(out, "abc");
    }

    #[test]
    fn truncate_fallback_empty_when_ellipsis_too_wide() {
        let (cx, _cfg) = mk_cx(10.0);
        let out = truncate_with_ellipsis(&cx, "abcdef", 5.0);
        assert_eq!(out, "");
    }
}
