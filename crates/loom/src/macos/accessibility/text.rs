//! UTF-16 offsets used by AppKit mapped to terminal cells and buffer rows.
use crate::grid::ClientPaneGrid;
use loom_protocol::message::{FLAG_HIDDEN, FLAG_WIDE_CHAR_SPACER};
use std::ops::Range;

const MAX_ROWS: usize = 2048;
const MAX_UNITS: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub range: Range<usize>,
    pub col: u16,
    pub width: u16,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub row: usize,
    pub range: Range<usize>,
    pub cells: Vec<Cell>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Text {
    pub utf16: Vec<u16>,
    pub lines: Vec<Line>,
    pub visible: Range<usize>,
    pub selected: Range<usize>,
    pub cursor_line: Option<usize>,
    pub viewport_top: usize,
    pub clipped: bool,
}

impl Text {
    pub fn capture(
        grid: &ClientPaneGrid,
        selection: Option<((u16, usize), (u16, usize))>,
        history: bool,
    ) -> Self {
        let top = grid.viewport_top();
        let mut result = Self {
            viewport_top: top,
            ..Self::default()
        };
        // Secure input and concealed cells must never enter the AX value cache.
        if grid.password_input {
            return result;
        }
        let end = (top + grid.rows as usize).min(grid.total_lines());
        let start = if history {
            end.saturating_sub(MAX_ROWS)
        } else {
            top.max(end.saturating_sub(MAX_ROWS))
        };
        let mut lines = Vec::new();
        let mut units = 0;
        // Build from the visible end backwards, keeping the viewport when a
        // large scrollback reaches the bounded accessibility working set.
        for row in (start..end).rev() {
            let mut line_text = Vec::new();
            let mut cells: Vec<Cell> = Vec::new();
            for (col, cell) in grid.buffer_row(row).iter().enumerate() {
                if cell.flags_u16() & FLAG_WIDE_CHAR_SPACER != 0
                    && let Some(cell) = cells.last_mut()
                {
                    cell.width += 1;
                    continue;
                }
                let begin = line_text.len();
                if cell.flags_u16() & (FLAG_HIDDEN | FLAG_WIDE_CHAR_SPACER) != 0 {
                    line_text.push(b' ' as u16);
                } else if let Some(grapheme) = row
                    .checked_mul(grid.cols as usize)
                    .and_then(|index| index.checked_add(col))
                    .and_then(|index| u32::try_from(index).ok())
                    .and_then(|index| grid.grapheme_map.get(&index))
                {
                    line_text.extend(
                        grapheme
                            .encode_utf16()
                            .take(MAX_UNITS.saturating_sub(line_text.len()) + 1),
                    );
                } else {
                    let ch = if cell.ch() == '\0' { ' ' } else { cell.ch() };
                    line_text.extend_from_slice(ch.encode_utf16(&mut [0; 2]));
                }
                cells.push(Cell {
                    range: begin..line_text.len(),
                    col: col as u16,
                    width: 1,
                });
                if line_text.len() > MAX_UNITS {
                    break;
                }
            }
            line_text.push(b'\n' as u16);
            if units + line_text.len() > MAX_UNITS {
                result.clipped = true;
                break;
            }
            units += line_text.len();
            lines.push((row, line_text, cells));
        }
        for (row, text, mut cells) in lines.into_iter().rev() {
            let start = result.utf16.len();
            result.utf16.extend(text);
            for cell in &mut cells {
                cell.range.start += start;
                cell.range.end += start;
            }
            result.lines.push(Line {
                row,
                range: start..result.utf16.len(),
                cells,
            });
        }
        result.clipped |= result.lines.first().is_some_and(|line| line.row > 0);
        result.visible = result
            .lines
            .iter()
            .find(|line| line.row >= top)
            .map(|line| line.range.start)
            .unwrap_or(result.utf16.len())..result.utf16.len();
        if let Some((col, line)) = grid.cursor_in_viewport().filter(|(_, line)| *line >= 0) {
            let row = top + line as usize;
            result.cursor_line = result.lines.iter().position(|line| line.row == row);
            if let Some(offset) = result.offset(col, row, false) {
                result.selected = offset..offset;
            }
        }
        if let Some((a, b)) = selection {
            let (a, b) = if (a.1, a.0) <= (b.1, b.0) {
                (a, b)
            } else {
                (b, a)
            };
            if let (Some(start), Some(end)) = (
                result.offset(a.0, a.1, false),
                result.offset(b.0, b.1, true),
            ) {
                result.selected = start..end;
            }
        }
        result
    }
    pub fn offset(&self, col: u16, row: usize, after: bool) -> Option<usize> {
        let line = self.lines.iter().find(|line| line.row == row)?;
        Some(
            line.cells
                .iter()
                .find(|cell| col >= cell.col && col < cell.col.saturating_add(cell.width))
                .map(|cell| {
                    if after {
                        cell.range.end
                    } else {
                        cell.range.start
                    }
                })
                .unwrap_or_else(|| line.range.end.saturating_sub(1)),
        )
    }
    pub fn line_for_index(&self, index: usize) -> Option<usize> {
        if index > self.utf16.len() {
            return None;
        }
        self.lines
            .iter()
            .position(|line| line.range.contains(&index))
            .or_else(|| {
                (index == self.utf16.len())
                    .then(|| self.lines.len().checked_sub(1))
                    .flatten()
            })
    }
    pub fn cell_for_index(&self, index: usize) -> Option<(&Line, &Cell)> {
        let line = self.lines.get(self.line_for_index(index)?)?;
        let cell = line
            .cells
            .iter()
            .find(|cell| cell.range.contains(&index))
            .or_else(|| line.cells.last())?;
        Some((line, cell))
    }
    pub fn checked_range(&self, location: usize, length: usize) -> Option<Range<usize>> {
        let end = location.checked_add(length)?;
        (end <= self.utf16.len()).then_some(location..end)
    }
    pub fn selection_cells(&self, range: Range<usize>) -> Option<((u16, usize), (u16, usize))> {
        if range.is_empty() {
            return None;
        }
        let (a_line, a) = self.cell_for_index(range.start)?;
        let (b_line, b) = self.cell_for_index(range.end.checked_sub(1)?)?;
        Some(((a.col, a_line.row), (b.col + b.width - 1, b_line.row)))
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    revision: u64,
    selection: Option<((u16, usize), (u16, usize))>,
    cursor: (u16, i16),
    dimensions: (u16, u16),
    top: usize,
    secure: bool,
    history: bool,
}
impl Stamp {
    fn read(
        grid: &ClientPaneGrid,
        selection: Option<((u16, usize), (u16, usize))>,
        history: bool,
    ) -> Self {
        Self {
            revision: grid.content_revision(),
            selection,
            cursor: (grid.cursor_col, grid.cursor_line),
            dimensions: (grid.cols, grid.rows),
            top: grid.viewport_top(),
            secure: grid.password_input,
            history,
        }
    }
}
/// Coalesce output without reusing or consuming renderer damage flags. In
/// particular, an occluded pane may remain render-dirty indefinitely.
pub struct Cache {
    pub text: std::rc::Rc<Text>,
    pub revision: u64,
    stamp: Stamp,
    captured: std::time::Instant,
    dirty: bool,
}
impl Cache {
    const INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);
    pub fn new(
        grid: &ClientPaneGrid,
        selection: Option<((u16, usize), (u16, usize))>,
        history: bool,
        now: std::time::Instant,
    ) -> Self {
        Self {
            text: std::rc::Rc::new(Text::capture(grid, selection, history)),
            revision: 1,
            stamp: Stamp::read(grid, selection, history),
            captured: now,
            dirty: false,
        }
    }
    pub fn update(
        &mut self,
        grid: &ClientPaneGrid,
        selection: Option<((u16, usize), (u16, usize))>,
        history: bool,
        now: std::time::Instant,
    ) {
        let stamp = Stamp::read(grid, selection, history);
        self.dirty |= stamp != self.stamp;
        let force = stamp.secure != self.stamp.secure
            || stamp.history != self.stamp.history
            || stamp.dimensions != self.stamp.dimensions
            || stamp.top != self.stamp.top;
        self.stamp = stamp;
        if force || (self.dirty && now.duration_since(self.captured) >= Self::INTERVAL) {
            let text = Text::capture(grid, selection, history);
            if text != *self.text {
                self.text = std::rc::Rc::new(text);
                self.revision = self.revision.wrapping_add(1);
            }
            self.captured = now;
            self.dirty = false;
        }
    }
    pub fn deadline(&self) -> Option<std::time::Instant> {
        self.dirty.then_some(self.captured + Self::INTERVAL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_protocol::message::{FLAG_WIDE_CHAR, PackedCell};
    fn grid() -> ClientPaneGrid {
        let mut grid = ClientPaneGrid::new(6, 2, 100);
        for (cell, ch) in grid.viewport.iter_mut().zip("A中 😀 eabcdef".chars()) {
            cell.set_ch(ch);
        }
        grid.viewport[1].flags = FLAG_WIDE_CHAR.to_le_bytes();
        grid.viewport[2].flags = FLAG_WIDE_CHAR_SPACER.to_le_bytes();
        grid.grapheme_map.insert(5, "e\u{301}".into());
        grid
    }
    #[test]
    fn utf16_indices_follow_wide_cells_surrogates_and_combining_graphemes() {
        let text = Text::capture(&grid(), None, false);
        assert_eq!(
            String::from_utf16(&text.utf16).unwrap(),
            "A中😀 e\u{301}\nabcdef\n"
        );
        assert_eq!(text.offset(1, 0, false), text.offset(2, 0, false));
        assert_eq!(text.offset(3, 0, false), Some(2));
        assert_eq!(text.offset(3, 0, true), Some(4));
        assert_eq!(text.offset(5, 0, true), Some(7));
        assert_eq!(text.cell_for_index(3).unwrap().1.col, 3);
        assert_eq!(text.selection_cells(1..4), Some(((1, 0), (3, 0))));
    }
    #[test]
    fn reversed_selection_and_cursor_use_utf16_ranges() {
        let mut grid = grid();
        grid.cursor_col = 5;
        let text = Text::capture(&grid, None, false);
        assert_eq!(text.selected, 5..5);
        let text = Text::capture(&grid, Some(((3, 0), (2, 0))), false);
        assert_eq!(text.selected, 1..4);
    }
    #[test]
    fn scrollback_ranges_keep_absolute_rows_and_track_the_visible_viewport() {
        use loom_protocol::message::*;
        let mut grid = ClientPaneGrid::new(2, 2, 10);
        grid.apply_full_sync_owned(&FullPaneSync {
            meta: PaneFrameMeta::default(),
            cols: 2,
            rows: 2,
            title: String::new(),
            scrollback: "aabbcc".chars().map(PackedCell::with_ch).collect(),
            scrollback_rows: 3,
            scrollback_replace: true,
            cells: "DDEE".chars().map(PackedCell::with_ch).collect(),
            grapheme_extras: GraphemeExtras::new(),
            hyperlink_extras: HyperlinkExtras::new(),
            cwd: None,
        });
        let text = Text::capture(&grid, Some(((0, 1), (1, 3))), true);
        assert_eq!(
            String::from_utf16(&text.utf16).unwrap(),
            "aa\nbb\ncc\nDD\nEE\n"
        );
        assert_eq!(text.visible, 9..15);
        assert_eq!(text.selected, 3..11);
        assert_eq!(
            text.selection_cells(text.selected.clone()),
            Some(((0, 1), (1, 3)))
        );
        grid.scroll_up(2);
        let text = Text::capture(&grid, None, true);
        assert_eq!(String::from_utf16(&text.utf16).unwrap(), "aa\nbb\ncc\n");
        assert_eq!(text.visible, 3..9);
        assert_eq!(text.cursor_line, None);
        let visible = Text::capture(&grid, None, false);
        assert_eq!(visible.lines[0].row, 1);
        assert_eq!(String::from_utf16(&visible.utf16).unwrap(), "bb\ncc\n");
        assert_eq!(visible.visible, 0..6);
    }
    #[test]
    fn password_and_concealed_cells_never_leak_to_accessibility() {
        let mut grid = grid();
        grid.viewport[5].flags = FLAG_HIDDEN.to_le_bytes();
        let text = Text::capture(&grid, None, false);
        assert!(!String::from_utf16(&text.utf16).unwrap().contains('\u{301}'));
        grid.password_input = true;
        let text = Text::capture(&grid, None, true);
        assert!(text.utf16.is_empty());
        assert!(text.lines.is_empty());
        assert_eq!(text.selected, 0..0);
    }
    #[test]
    fn ranges_reject_overflow_and_out_of_bounds_requests() {
        let text = Text::capture(&grid(), None, false);
        assert!(text.checked_range(usize::MAX, 1).is_none());
        assert!(text.checked_range(text.utf16.len(), 1).is_none());
        assert_eq!(
            text.checked_range(text.utf16.len(), 0),
            Some(text.utf16.len()..text.utf16.len())
        );
    }
    #[test]
    fn work_is_bounded_and_visible_rows_are_retained() {
        let mut grid = ClientPaneGrid::new(100, 3000, 0);
        grid.viewport.fill(PackedCell::default());
        let text = Text::capture(&grid, None, true);
        assert_eq!(text.lines.len(), MAX_ROWS);
        assert_eq!(text.lines.last().unwrap().row, 2999);
        assert!(text.utf16.len() <= MAX_UNITS);
        assert!(text.clipped);
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn persistent_render_damage_does_not_poll_or_rebuild_accessibility() {
        let mut grid = ClientPaneGrid::new(2, 1, 0);
        let now = Instant::now();
        let mut cache = Cache::new(&grid, None, false, now);
        let original = cache.text.clone();
        cache.update(&grid, None, false, now + Duration::from_secs(1));
        assert!(grid.is_dirty());
        assert!(cache.deadline().is_none());
        assert!(std::rc::Rc::ptr_eq(&original, &cache.text));
        grid.clear_dirty();
        cache.update(&grid, None, false, now + Duration::from_secs(2));
        assert!(cache.deadline().is_none());
    }
    #[test]
    fn output_is_coalesced_but_secure_input_redacts_immediately() {
        use loom_protocol::message::*;
        let mut grid = ClientPaneGrid::new(2, 1, 0);
        let now = Instant::now();
        let mut cache = Cache::new(&grid, None, true, now);
        grid.apply_delta(&CellDelta {
            pane_id: 1,
            generation: 1,
            cursor_line: 0,
            cursor_col: 1,
            cursor_shape: CURSOR_BLOCK,
            mode_flags: 0,
            regions: vec![DamageRegion {
                line: 0,
                left: 0,
                right: 0,
                cells: vec![PackedCell::with_ch('A')],
            }],
        });
        cache.update(&grid, None, true, now + Duration::from_millis(10));
        assert_eq!(cache.deadline(), Some(now + Cache::INTERVAL));
        assert_eq!(cache.text.utf16[0], b' ' as u16);
        grid.clear_dirty();
        cache.update(&grid, None, true, now + Cache::INTERVAL);
        assert_eq!(cache.text.utf16[0], b'A' as u16);
        assert!(cache.deadline().is_none());
        grid.password_input = true;
        cache.update(&grid, None, true, now + Duration::from_millis(101));
        assert!(cache.text.utf16.is_empty());
        assert!(cache.deadline().is_none());
    }
}
