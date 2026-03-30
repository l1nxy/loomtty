use ciri_protocol::message::*;

/// A single scrollback row with wrap metadata for reflow on resize.
#[derive(Clone)]
pub struct ScrollbackRow {
    pub cells: Vec<PackedCell>,
    /// True if this row's content continues on the next row (soft wrap).
    pub wrapped: bool,
}

impl ScrollbackRow {
    /// Build from a slice of cells, extracting the wrap flag from the last cell.
    pub(super) fn from_cells(cells: &[PackedCell]) -> Self {
        let wrapped = cells
            .last()
            .is_some_and(|c| c.flags_u16() & FLAG_WRAPLINE != 0);
        ScrollbackRow {
            cells: cells.to_vec(),
            wrapped,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkMatch {
    pub url: String,
    pub start_col: u16,
    pub end_col: u16,
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
