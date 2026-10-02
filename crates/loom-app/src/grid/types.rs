use loom_protocol::message::*;

/// A single scrollback row with wrap metadata for reflow on resize.
#[derive(Clone)]
pub struct ScrollbackRow {
    pub cells: Vec<PackedCell>,
    /// True if this row's content continues on the next row (soft wrap).
    pub wrapped: bool,
}

impl ScrollbackRow {
    /// Take a decoded row without copying its cells; preserve its wrap flag.
    pub(super) fn from_vec(cells: Vec<PackedCell>) -> Self {
        let wrapped = cells
            .last()
            .is_some_and(|c| c.flags_u16() & FLAG_WRAPLINE != 0);
        ScrollbackRow { cells, wrapped }
    }
}

/// A detected link and the screen cells it occupies. `start`/`end` are
/// inclusive; the span may cross soft-wrapped rows (`start_row < end_row`),
/// in which case it covers `start_col..` on the first row, every column of
/// the rows between, and `..=end_col` on the last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkMatch {
    pub url: String,
    pub start_col: u16,
    /// Absolute buffer row of the first cell.
    pub start_row: usize,
    pub end_col: u16,
    /// Absolute buffer row of the last cell.
    pub end_row: usize,
}

#[derive(Clone, Copy)]
pub(super) struct RowChar {
    pub(super) start_col: u16,
    pub(super) end_col: u16,
    pub(super) ch: char,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum WordClass {
    Whitespace,
    Word,
    Symbol,
}

pub(super) fn classify_word_cell(cell: &PackedCell) -> WordClass {
    let ch = cell.ch();
    if ch == '\0' || ch.is_whitespace() {
        WordClass::Whitespace
    } else if ch.is_alphanumeric() || ch == '_' {
        WordClass::Word
    } else {
        WordClass::Symbol
    }
}
