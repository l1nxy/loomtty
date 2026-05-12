use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Flags as CellFlags;
use ciri_protocol::message::CapturePaneOpts;

use super::Pane;

/// Hard cap on the number of **scrollback** rows we'll consider for a
/// single capture response. Defense-in-depth above the byte budget.
pub const MAX_CAPTURE_SCROLLBACK_ROWS: usize = 100_000;

/// Soft byte budget for the captured text payload. Sits below the
/// `MAX_CONTROL_FRAME_LEN = 1 MiB` ceiling for control-channel frames,
/// leaving headroom for the rmp-serde envelope (variant name, session
/// name, pane_id, msgpack framing). A capture that would exceed this
/// budget is truncated row-wise to fit.
const MAX_CAPTURE_TEXT_BYTES: usize = 900 * 1024;

/// Printable marker (no ESC) appended when rows are dropped, so the
/// non-`--json` path stays safe to dump to a user's terminal.
const TRUNC_MARKER: &str =
    "--- ciritty: capture truncated to fit control-frame budget ---\n";

/// Pre-reserves room for the marker inside the byte budget.
const SAFE_BUDGET: usize = MAX_CAPTURE_TEXT_BYTES - TRUNC_MARKER.len();

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
    ///    trailing rows are dropped and `text` ends with `TRUNC_MARKER`.
    ///
    /// Wide chars are emitted once (the spacer cell is skipped). Combining
    /// marks attached to a cell are appended after the base char —
    /// including when the base cell is a "trailing space" that the trim
    /// pass would otherwise drop (the marks are not spaces, so the
    /// trailing-space trim leaves them intact).
    ///
    /// Trailing **ASCII** space cells (`' '`, U+0020) are trimmed per row
    /// unless `preserve_trailing_spaces` is set; NBSP, U+3000 and other
    /// non-ASCII whitespace are preserved verbatim.
    ///
    /// Cell content is emitted as-is from `cell.c`. In practice that
    /// excludes most C0 controls: alacritty's `Term::input` rejects them
    /// via `unicode_width::UnicodeWidthChar`. Known exceptions: `\t`
    /// (written by `Term::put_tab`), and any control bytes that reach
    /// the grid through non-`input` paths (escape-sequence parameters,
    /// paste handlers). Callers piping output to a terminal should
    /// consider scrubbing if untrusted programs run in the pane.
    ///
    /// # Alt-screen
    ///
    /// `term.grid()` returns the currently-displayed grid. While an
    /// alt-screen TUI (vim, less, htop, fzf …) is foregrounded, this
    /// captures the alt buffer and `history_size()` is normally 0 — the
    /// primary buffer's scrollback is not reachable through this call.
    pub fn capture_text(&self, opts: &CapturePaneOpts) -> CaptureText {
        let grid = self.term.grid();
        let cols = grid.columns();
        let visible = grid.screen_lines();
        if cols == 0 || visible == 0 {
            return CaptureText::default();
        }

        // Per-row byte estimate (`cols * 4 + 1`) is worst-case UTF-8 +
        // newline. It does NOT account for combining marks (up to
        // ~17 bytes/cell); the post-row guard inside the loop catches
        // those cases and bails with the marker.
        let bytes_per_row = cols.saturating_mul(4).saturating_add(1);
        let rows_for_byte_budget = MAX_CAPTURE_TEXT_BYTES / bytes_per_row;
        let scrollback_row_budget = rows_for_byte_budget.saturating_sub(visible);

        let requested = opts.scrollback_rows as usize;
        let history_available = requested.min(grid.history_size());
        let scrollback = history_available
            .min(MAX_CAPTURE_SCROLLBACK_ROWS)
            .min(scrollback_row_budget);
        // Pre-loop truncation signal: byte-budget or hard-cap forced us
        // to drop content before iteration began. Without this, a wide
        // pane that perfectly fills the budget would silently return 0
        // scrollback rows with `truncated == false`.
        let mut truncated = scrollback < history_available;

        let total_rows = scrollback + visible;
        let cap_hint = total_rows
            .saturating_mul(bytes_per_row)
            .min(MAX_CAPTURE_TEXT_BYTES);
        let mut out = String::with_capacity(cap_hint);
        // Reused across rows so we don't pay N heap allocations on a
        // 100k-row capture. Capacity is preserved by `clear()`.
        let mut row_text = String::with_capacity(cols.saturating_mul(4));

        // Scrollback rows live at Line(-scrollback ..= -1), viewport at
        // Line(0 ..= visible-1). `i32` casts safe — both pre-clamped.
        let first_line = -(scrollback as i32);
        let last_line = visible as i32;
        let last_col = Column(cols - 1);
        for line_idx in first_line..last_line {
            let line = Line(line_idx);
            row_text.clear();

            for col in 0..cols {
                let cell = &grid[Point::new(line, Column(col))];
                // Skip BOTH spacer kinds: WIDE_CHAR_SPACER (the second
                // half of a wide glyph) and LEADING_WIDE_CHAR_SPACER
                // (placeholder at the last column when a wide glyph
                // wraps). Emitting them would duplicate the wide char
                // or insert phantom padding under
                // `preserve_trailing_spaces`. WRAPLINE on the spacer
                // is still recovered after the loop, since the spacer
                // sits at `last_col` like any other terminal cell.
                if cell.flags.intersects(
                    CellFlags::WIDE_CHAR_SPACER | CellFlags::LEADING_WIDE_CHAR_SPACER,
                ) {
                    continue;
                }

                row_text.push(cell.c);
                // `zerowidth()` returns `Some(&[])` for probed-but-empty;
                // the empty iteration is a no-op.
                if let Some(marks) = cell.zerowidth() {
                    for c in marks {
                        row_text.push(*c);
                    }
                }
            }

            // WRAPLINE rides on whatever cell occupies the last column —
            // regular char or spacer — so a single read recovers wrap
            // state. (`cols >= 1` here: the `cols == 0` early-return
            // above guarantees it.)
            let wraps = grid[Point::new(line, last_col)]
                .flags
                .contains(CellFlags::WRAPLINE);

            // Trim trailing ASCII spaces unless:
            //   - the caller asked to preserve them, or
            //   - the row soft-wraps AND `--join-wrapped` is set —
            //     those trailing spaces bridge to the next row, so
            //     dropping them then suppressing the newline would glue
            //     adjacent rows together. (Matches tmux `-J`.)
            //
            // `trim_end_matches(' ')` is safe over combining marks:
            // marks are non-space, so a "space cell with marks" lands
            // as `' ' + marks` and survives the trim intact.
            let preserve_for_wrap = opts.join_wrapped && wraps;
            if !opts.preserve_trailing_spaces && !preserve_for_wrap {
                let trimmed_len = row_text.trim_end_matches(' ').len();
                row_text.truncate(trimmed_len);
            }
            out.push_str(&row_text);

            if !(opts.join_wrapped && wraps) {
                out.push('\n');
            }

            // Post-row guard. Uses actual produced length, not an
            // estimate. `SAFE_BUDGET` reserves room for the marker so
            // even a worst-case last-row + marker stays under the 1 MiB
            // control-frame ceiling.
            //
            // Skip on the final iteration: no more rows to drop, so the
            // marker would falsely signal data loss.
            let is_last_row = line_idx + 1 == last_line;
            if out.len() >= SAFE_BUDGET && !is_last_row {
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
    /// True when rows were dropped to fit `MAX_CAPTURE_TEXT_BYTES`
    /// (the text ends with [`TRUNC_MARKER`]) or when the requested
    /// scrollback was clipped before iteration began (no marker).
    pub truncated: bool,
}
