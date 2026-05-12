use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Flags as CellFlags;
use ciri_protocol::message::CapturePaneOpts;

use super::Pane;

/// Hard cap on the number of **scrollback** rows we'll consider for a
/// single capture response. The viewport rows are always included on top.
/// A second, narrower clamp based on the actual control-frame byte budget
/// is applied per-call (see `capture_text` body), since `MAX_CONTROL_FRAME_LEN`
/// is only 1 MiB and a wide pane could blow past that long before reaching
/// this row count.
pub const MAX_CAPTURE_SCROLLBACK_ROWS: usize = 100_000;

/// Soft byte budget for the captured text payload. Sits below the
/// `MAX_CONTROL_FRAME_LEN = 1 MiB` ceiling for control-channel frames,
/// leaving headroom for the rmp-serde envelope (variant name, session
/// name, pane_id, msgpack framing). A capture that would exceed this
/// budget is truncated row-wise to fit.
const MAX_CAPTURE_TEXT_BYTES: usize = 900 * 1024;

impl Pane {
    /// Render the pane's *active* grid (plus optional scrollback) as plain text.
    ///
    /// Returns the rendered text and a `truncated` flag set when the
    /// server returned fewer rows than the caller could have used. Two
    /// paths can flip it:
    /// 1. **Pre-loop clamp** — requested scrollback was clipped by the
    ///    byte budget (`MAX_CAPTURE_TEXT_BYTES`) or hard-cap
    ///    (`MAX_CAPTURE_SCROLLBACK_ROWS`) before iteration began.
    ///    `text` is *not* marker-suffixed in this case.
    /// 2. **Mid-loop truncation** — iteration crossed the byte budget;
    ///    trailing rows are dropped and `text` ends with a printable
    ///    marker line so even non-`truncated`-aware consumers see a
    ///    visible sentinel.
    ///
    /// Wide chars are emitted once (the spacer cell is skipped, but its
    /// WRAPLINE bit is still respected — see WRAPLINE handling below).
    /// Combining marks attached to a cell are appended after the base
    /// char — including when the base cell is a "trailing space" that the
    /// trim pass would otherwise drop.
    ///
    /// Trailing **ASCII** space cells (`' '`, U+0020) are trimmed per row
    /// unless `preserve_trailing_spaces` is set; NBSP, U+3000 and other
    /// non-ASCII whitespace are preserved verbatim.
    ///
    /// Cell content is emitted as-is from `cell.c`. In practice that
    /// excludes most C0 controls (NUL, BEL, BS, ESC, …): alacritty's
    /// `Term::input` rejects them via `unicode_width::UnicodeWidthChar`
    /// (returns `None` for non-printable). Two known exceptions:
    /// `\t`, which `Term::put_tab` writes into otherwise-empty space
    /// cells (so rows can contain literal tabs, and since `\t != ' '`,
    /// the trim pass leaves them in place); and any control bytes that
    /// reach the grid through a non-`input` path (escape-sequence
    /// parameters, paste handlers, etc.). Callers piping the output
    /// to a terminal should consider scrubbing if untrusted programs
    /// can run inside the pane.
    ///
    /// # Alt-screen
    ///
    /// `term.grid()` returns the currently-displayed grid. While an alt-screen
    /// TUI (vim, less, htop, fzf …) is foregrounded, this captures the alt
    /// buffer and `history_size()` is normally 0 — the primary buffer's
    /// scrollback is not reachable through this call. Callers that need
    /// "what the user can see right now" want exactly this; callers that
    /// want "the shell's history" should capture when no alt-screen TUI is
    /// active.
    pub fn capture_text(&self, opts: &CapturePaneOpts) -> CaptureText {
        let grid = self.term.grid();
        let cols = grid.columns();
        let visible = grid.screen_lines();
        if cols == 0 || visible == 0 {
            return CaptureText::default();
        }

        // Clamp the **scrollback** request in three steps:
        //   1) by what the buffer actually has,
        //   2) by the explicit row cap (defense-in-depth),
        //   3) by `MAX_CAPTURE_TEXT_BYTES` (900 KiB, deliberately sized
        //      below the codec's `MAX_CONTROL_FRAME_LEN = 1 MiB` ceiling
        //      so the response fits in a single control frame even after
        //      the envelope and a worst-case last row).
        //
        // Step 3 estimates worst-case bytes per row as `cols * 4 + 1`
        // (every cell a 4-byte UTF-8 codepoint plus one newline). It does
        // NOT account for cells with combining marks, which can extend a
        // single cell to ~17 bytes; the post-row safety net inside the
        // iteration loop catches that case and bails with a marker.
        //
        // The viewport itself is iterated unconditionally; if cols are so
        // wide that the viewport alone would blow the budget, the same
        // post-row guard truncates mid-viewport.
        // `cols == 0` already returned early, so `bytes_per_row >= 5`.
        let bytes_per_row = cols.saturating_mul(4).saturating_add(1);
        let rows_for_byte_budget = MAX_CAPTURE_TEXT_BYTES / bytes_per_row;
        // Scrollback gets whatever rows the byte budget has left after the
        // viewport. If the viewport alone meets or exceeds the budget,
        // scrollback is denied here and the post-row safety net inside the
        // loop caps the viewport itself.
        let scrollback_row_budget = rows_for_byte_budget.saturating_sub(visible);

        let requested = opts.scrollback_rows as usize;
        // What we *could* have returned without any cap: bound only by
        // the actual pane history.
        let history_available = requested.min(grid.history_size());
        let scrollback = history_available
            .min(MAX_CAPTURE_SCROLLBACK_ROWS)
            .min(scrollback_row_budget);
        // Pre-loop truncation signal: the user asked for content the
        // byte-budget or hard-cap forced us to drop *before* we even
        // started iterating. Without this, a 200×1200 pane that
        // perfectly fills the budget would silently return 0 scrollback
        // rows with `truncated == false`.
        let mut truncated = scrollback < history_available;

        // The marker must itself fit within the byte budget; pre-compute
        // the cutoff at which we stop adding rows. Using a printable
        // marker (no ESC) keeps the non-`--json` path safe to dump to a
        // user's terminal.
        const TRUNC_MARKER: &str =
            "--- ciritty: capture truncated to fit control-frame budget ---\n";
        let safe_budget = MAX_CAPTURE_TEXT_BYTES.saturating_sub(TRUNC_MARKER.len());

        let total_rows = scrollback + visible;
        // `with_capacity` takes bytes. Match the per-row buffer estimate
        // (`cols * 4 + 1`) so non-ASCII captures don't grow this string
        // mid-loop. Capped at the byte budget so we don't try to allocate
        // gigabytes for a 10k-col pane.
        let cap_hint = total_rows
            .saturating_mul(cols.saturating_mul(4).saturating_add(1))
            .min(MAX_CAPTURE_TEXT_BYTES);
        let mut out = String::with_capacity(cap_hint);

        // Iterate oldest-first: scrollback rows live at Line(-scrollback ..= -1),
        // viewport rows at Line(0 ..= visible-1). Both bounds are pre-clamped
        // above, so `i32` casts are safe.
        let first_line = -(scrollback as i32);
        let last_line = visible as i32;
        for line_idx in first_line..last_line {
            let line = Line(line_idx);

            // `with_capacity` takes BYTES, not chars. For CJK / emoji
            // content each cell is 3-4 bytes, so a `cols`-byte hint would
            // force per-row reallocation. Round up to `cols * 4` (worst-
            // case UTF-8) to avoid that without over-allocating on the
            // common ASCII path (still capped by frame budget upstream).
            let mut row_text = String::with_capacity(cols.saturating_mul(4));
            let mut last_significant_len = 0;
            let mut wraps = false;

            for col in 0..cols {
                let cell = &grid[Point::new(line, Column(col))];
                // Skip BOTH spacer kinds: WIDE_CHAR_SPACER (the second
                // half of a wide glyph, follows its base cell) and
                // LEADING_WIDE_CHAR_SPACER (the placeholder space
                // written by alacritty's `Term::input` at the last
                // column of a row when the next wide glyph won't fit
                // and has to wrap to the next line). The placeholder
                // is not visible to the user, so emitting it would
                // corrupt output under `preserve_trailing_spaces` and
                // the row's WRAPLINE bit can ride on the spacer cell
                // — alacritty's `Cell::is_empty` treats both spacer
                // flags as non-content for the same reason. Honour
                // WRAPLINE before `continue`-ing.
                if cell.flags.intersects(
                    CellFlags::WIDE_CHAR_SPACER | CellFlags::LEADING_WIDE_CHAR_SPACER,
                ) {
                    if cell.flags.contains(CellFlags::WRAPLINE) {
                        wraps = true;
                    }
                    continue;
                }
                if cell.flags.contains(CellFlags::WRAPLINE) {
                    wraps = true;
                }

                row_text.push(cell.c);
                // `zerowidth()` returns `Some(&[])` when the cell was
                // probed but no marks are attached — treat empty exactly
                // like `None`. (Matches the convention in `pack_cell` /
                // `collect_*_cells`.)
                let has_marks = match cell.zerowidth() {
                    Some(marks) if !marks.is_empty() => {
                        for c in marks {
                            row_text.push(*c);
                        }
                        true
                    }
                    _ => false,
                };
                // A cell is "significant" (not trailing-trimmable) when it
                // is not an ASCII space OR carries non-empty combining marks.
                if cell.c != ' ' || has_marks {
                    last_significant_len = row_text.len();
                }
            }

            // Trim trailing ASCII spaces UNLESS:
            //   - the caller asked us to preserve them, or
            //   - this row soft-wraps into the next AND the caller asked
            //     us to join — those trailing spaces are grid artifacts
            //     bridging to the next row, NOT user-visible padding;
            //     dropping them and then suppressing the newline would
            //     glue the next row's content into the previous one.
            //
            // (This matches tmux's `-J` flag, which both joins wrapped
            // rows AND preserves their trailing whitespace.)
            let preserve_for_wrap = opts.join_wrapped && wraps;
            if !opts.preserve_trailing_spaces && !preserve_for_wrap {
                row_text.truncate(last_significant_len);
            }
            out.push_str(&row_text);

            // Emit a newline unless this row soft-wrapped AND the caller
            // asked for wrapped rows to be joined.
            if !(opts.join_wrapped && wraps) {
                out.push('\n');
            }

            // Post-row byte-budget check. Done AFTER the push so we use
            // the actual produced length, not a worst-case estimate.
            //
            // `safe_budget = MAX_CAPTURE_TEXT_BYTES - TRUNC_MARKER.len()`
            // reserves room for the marker; after appending it we
            // satisfy `out.len() ≤ MAX_CAPTURE_TEXT_BYTES + max_row_bytes`,
            // where `max_row_bytes` is the worst single-row size. The
            // soft `MAX_CAPTURE_TEXT_BYTES` (900 KiB) is intentionally
            // below the hard `MAX_CONTROL_FRAME_LEN` (1 MiB), so even
            // a pathological combining-mark row can't push the frame
            // past the codec's ceiling.
            //
            // Skip the marker on the final iteration: there are no more
            // rows to drop, so claiming "truncated" would be misleading
            // and the marker noise wouldn't carry any information.
            let is_last_row = line_idx + 1 == last_line;
            if out.len() >= safe_budget && !is_last_row {
                out.push_str(TRUNC_MARKER);
                truncated = true;
                break;
            }
        }

        CaptureText {
            text: out,
            truncated,
        }
    }
}

/// Return value of [`Pane::capture_text`].
#[derive(Debug, Clone, Default)]
pub struct CaptureText {
    pub text: String,
    /// True when the capture was clipped by `MAX_CAPTURE_TEXT_BYTES`
    /// (trailing rows omitted; the text ends with a printable marker).
    pub truncated: bool,
}
