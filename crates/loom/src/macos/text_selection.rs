//! Bounded terminal text exported to native macOS features.
use loom_protocol::message::{FLAG_HIDDEN, FLAG_WIDE_CHAR_SPACER};

use crate::grid::ClientPaneGrid;

pub(super) type SelectionRange = ((u16, usize), (u16, usize));

/// Preflight the selection before allocating, and never export concealed cells.
/// Use the shared copy path after checking so Services and Cmd+C agree on text.
pub(super) fn selected_text(
    grid: &ClientPaneGrid,
    range: SelectionRange,
    max_bytes: usize,
) -> Option<String> {
    if grid.password_input {
        return None;
    }
    let (a, b) = range;
    let (a, b) = if (a.1, a.0) <= (b.1, b.0) {
        (a, b)
    } else {
        (b, a)
    };
    if b.1 >= grid.total_lines() || grid.cols == 0 {
        return None;
    }
    let mut bytes = b.1.checked_sub(a.1)?;
    if bytes > max_bytes {
        return None;
    }
    for row in a.1..=b.1 {
        let left = if row == a.1 { a.0 } else { 0 };
        let right = if row == b.1 { b.0 } else { grid.cols - 1 };
        let (left, right) = grid.snap_selection_to_wide_chars(row, left, right);
        for (col, cell) in grid
            .buffer_row(row)
            .iter()
            .enumerate()
            .take(right as usize + 1)
            .skip(left as usize)
        {
            if cell.flags_u16() & FLAG_HIDDEN != 0 {
                return None;
            }
            if cell.flags_u16() & FLAG_WIDE_CHAR_SPACER != 0 {
                continue;
            }
            let grapheme = row
                .checked_mul(grid.cols as usize)
                .and_then(|index| index.checked_add(col))
                .and_then(|index| u32::try_from(index).ok())
                .and_then(|index| grid.grapheme_map.get(&index));
            bytes =
                bytes.checked_add(grapheme.map_or_else(|| cell.ch().len_utf8(), String::len))?;
            if bytes > max_bytes {
                return None;
            }
        }
    }
    let text = grid.text_in_range(a, b);
    (!text.is_empty() && text.len() <= max_bytes).then_some(text)
}
