use loom_protocol::message::*;
use std::borrow::Cow;

/// Immutable history cells. Rows with identical attributes share one template;
/// character bytes remain exact, including malformed or padded UTF-8 input.
#[derive(Clone)]
pub enum ScrollbackCells {
    Dense(Vec<PackedCell>),
    Uniform {
        template: PackedCell,
        characters: Box<[[u8; 4]]>,
    },
}

impl ScrollbackCells {
    fn uniform_template(cells: &[PackedCell]) -> Option<PackedCell> {
        // Avoid adding representation overhead to tiny rows.
        let template = *cells.first().filter(|_| cells.len() >= 8)?;
        cells
            .iter()
            .all(|cell| {
                let mut attributes = *cell;
                attributes.ch_bytes = template.ch_bytes;
                attributes == template
            })
            .then_some(template)
    }

    pub fn as_cells(&self) -> Cow<'_, [PackedCell]> {
        match self {
            Self::Dense(cells) => Cow::Borrowed(cells),
            Self::Uniform {
                template,
                characters,
            } => Cow::Owned(
                characters
                    .iter()
                    .map(|&ch_bytes| {
                        let mut cell = *template;
                        cell.ch_bytes = ch_bytes;
                        cell
                    })
                    .collect(),
            ),
        }
    }

    /// Append a viewport-width row without allocating an intermediate row.
    /// Resize gaps are padded with default cells; excess columns are truncated.
    pub fn append_to(&self, output: &mut Vec<PackedCell>, columns: usize) {
        let take = match self {
            Self::Dense(cells) => {
                let take = cells.len().min(columns);
                output.extend_from_slice(&cells[..take]);
                take
            }
            Self::Uniform {
                template,
                characters,
            } => {
                let take = characters.len().min(columns);
                output.extend(characters[..take].iter().map(|&ch_bytes| {
                    let mut cell = *template;
                    cell.ch_bytes = ch_bytes;
                    cell
                }));
                take
            }
        };
        output.resize(output.len() + columns - take, PackedCell::default());
    }

    pub fn cell(&self, index: usize) -> Option<PackedCell> {
        match self {
            Self::Dense(cells) => cells.get(index).copied(),
            Self::Uniform {
                template,
                characters,
            } => characters.get(index).map(|&ch_bytes| {
                let mut cell = *template;
                cell.ch_bytes = ch_bytes;
                cell
            }),
        }
    }

    pub fn into_cells(self) -> Vec<PackedCell> {
        match self {
            Self::Dense(cells) => cells,
            compact => compact.as_cells().into_owned(),
        }
    }
}

impl From<&[PackedCell]> for ScrollbackCells {
    fn from(cells: &[PackedCell]) -> Self {
        if let Some(template) = Self::uniform_template(cells) {
            Self::Uniform {
                template,
                characters: cells.iter().map(|cell| cell.ch_bytes).collect(),
            }
        } else {
            Self::Dense(cells.to_vec())
        }
    }
}

impl From<Vec<PackedCell>> for ScrollbackCells {
    fn from(cells: Vec<PackedCell>) -> Self {
        if let Some(template) = Self::uniform_template(&cells) {
            Self::Uniform {
                template,
                characters: cells.iter().map(|cell| cell.ch_bytes).collect(),
            }
        } else {
            Self::Dense(cells)
        }
    }
}

/// A single scrollback row with wrap metadata for reflow on resize.
#[derive(Clone)]
pub struct ScrollbackRow {
    pub cells: ScrollbackCells,
    /// True if this row's content continues on the next row (soft wrap).
    pub wrapped: bool,
}

impl ScrollbackRow {
    /// Compact a decoded scratch row and preserve its wrap flag.
    pub(super) fn from_slice(cells: &[PackedCell]) -> Self {
        let wrapped = cells
            .last()
            .is_some_and(|c| c.flags_u16() & FLAG_WRAPLINE != 0);
        ScrollbackRow {
            cells: cells.into(),
            wrapped,
        }
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

#[cfg(test)]
mod compact_tests {
    use super::*;

    #[test]
    fn history_attributes_and_character_bytes_roundtrip_exactly() {
        for flags in [
            0,
            FLAG_BOLD,
            FLAG_INVERSE,
            FLAG_HIDDEN,
            FLAG_WIDE_CHAR,
            u16::MAX,
        ] {
            for fg in [
                PackedColor::named(16),
                PackedColor::indexed(123),
                PackedColor::rgb(7, 31, 255),
            ] {
                let mut row = vec![PackedCell::default(); 100];
                for (i, cell) in row.iter_mut().enumerate() {
                    cell.set_ch(['x', '中', '😀', '\0'][i % 4]);
                    cell.fg = fg;
                    cell.bg = PackedColor::rgb(19, 42, 87);
                    cell.flags = flags.to_le_bytes();
                }
                row[17].ch_bytes = [255, 0, 1, 0];
                let compact = ScrollbackCells::from(row.as_slice());
                assert!(matches!(compact, ScrollbackCells::Uniform { .. }));
                assert_eq!(compact.as_cells().as_ref(), row);
                for (i, cell) in row.iter().enumerate() {
                    assert_eq!(compact.cell(i), Some(*cell));
                }
                assert!(compact.cell(row.len()).is_none());
                assert_eq!(compact.clone().into_cells(), row);
                row[37].bg = PackedColor::indexed(29);
                let dense = ScrollbackCells::from(row.clone());
                assert!(matches!(dense, ScrollbackCells::Dense(_)));
                assert_eq!(dense.into_cells(), row);
            }
        }
    }

    #[test]
    fn wrap_metadata_and_mixed_width_rows_are_preserved() {
        let mut row = vec![PackedCell::with_ch('a'); 100];
        row[99].flags = FLAG_WRAPLINE.to_le_bytes();
        row[40].set_ch('界');
        row[40].flags = FLAG_WIDE_CHAR.to_le_bytes();
        row[41].flags = FLAG_WIDE_CHAR_SPACER.to_le_bytes();
        let history = ScrollbackRow::from_slice(&row);
        assert!(history.wrapped);
        assert!(matches!(history.cells, ScrollbackCells::Dense(_)));
        assert_eq!(history.cells.as_cells().as_ref(), row);
    }

    #[test]
    fn append_preserves_prefix_and_truncates_or_pads_both_representations() {
        let mut row = vec![PackedCell::with_ch('界'); 100];
        for mixed in [false, true] {
            if mixed {
                row[5].fg = PackedColor::rgb(1, 2, 3);
            }
            let stored = ScrollbackCells::from(row.as_slice());
            for columns in [0, 50, 100, 120] {
                let prefix = vec![PackedCell::with_ch('p'); 3];
                let mut actual = prefix.clone();
                stored.append_to(&mut actual, columns);
                let mut expected = prefix;
                expected.extend_from_slice(&row[..columns.min(row.len())]);
                expected.resize(3 + columns, PackedCell::default());
                assert_eq!(actual, expected);
            }
        }
    }
}
