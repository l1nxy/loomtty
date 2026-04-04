use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};
use ciri_protocol::message::*;

pub fn pack_cell(cell: &alacritty_terminal::term::cell::Cell) -> PackedCell {
    let fg = pack_color(cell.fg);
    let bg = pack_color(cell.bg);
    let mut flags = 0u16;
    if cell.flags.contains(CellFlags::WIDE_CHAR) {
        flags |= FLAG_WIDE_CHAR;
    }
    if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
        flags |= FLAG_WIDE_CHAR_SPACER;
    }
    if cell.flags.contains(CellFlags::BOLD) {
        flags |= FLAG_BOLD;
    }
    if cell.flags.contains(CellFlags::ITALIC) {
        flags |= FLAG_ITALIC;
    }
    if cell.flags.contains(CellFlags::DOUBLE_UNDERLINE) {
        flags |= FLAG_UNDERLINE | FLAG_UNDERLINE_DOUBLE;
    } else if cell.flags.contains(CellFlags::UNDERCURL) {
        flags |= FLAG_UNDERLINE | FLAG_UNDERLINE_CURLY;
    } else if cell.flags.contains(CellFlags::DOTTED_UNDERLINE) {
        flags |= FLAG_UNDERLINE | FLAG_UNDERLINE_DOTTED;
    } else if cell.flags.contains(CellFlags::DASHED_UNDERLINE) {
        flags |= FLAG_UNDERLINE | FLAG_UNDERLINE_DASHED;
    } else if cell.flags.contains(CellFlags::ALL_UNDERLINES) {
        flags |= FLAG_UNDERLINE;
    }
    if cell.flags.contains(CellFlags::INVERSE) {
        flags |= FLAG_INVERSE;
    }
    if cell.flags.contains(CellFlags::DIM) {
        flags |= FLAG_DIM;
    }
    if cell.flags.contains(CellFlags::STRIKEOUT) {
        flags |= FLAG_STRIKEOUT;
    }
    if cell.flags.contains(CellFlags::HIDDEN) {
        flags |= FLAG_HIDDEN;
    }
    if cell.flags.contains(CellFlags::WRAPLINE) {
        flags |= FLAG_WRAPLINE;
    }
    let mut packed = PackedCell {
        ch_bytes: [0; 4],
        fg,
        bg,
        flags: flags.to_le_bytes(),
    };
    packed.set_ch(cell.c);
    packed
}

pub fn pack_color(color: AnsiColor) -> PackedColor {
    match color {
        AnsiColor::Named(n) => PackedColor::named(named_color_to_compact(n)),
        AnsiColor::Spec(rgb) => PackedColor::rgb(rgb.r, rgb.g, rgb.b),
        AnsiColor::Indexed(i) => PackedColor::indexed(i),
    }
}

fn named_color_to_compact(n: NamedColor) -> u8 {
    use alacritty_terminal::vte::ansi::NamedColor::*;
    match n {
        Black => 0,
        Red => 1,
        Green => 2,
        Yellow => 3,
        Blue => 4,
        Magenta => 5,
        Cyan => 6,
        White => 7,
        BrightBlack => 8,
        BrightRed => 9,
        BrightGreen => 10,
        BrightYellow => 11,
        BrightBlue => 12,
        BrightMagenta => 13,
        BrightCyan => 14,
        BrightWhite => 15,
        Foreground => 16,
        Background => 17,
        Cursor => 18,
        DimBlack => 19,
        DimRed => 20,
        DimGreen => 21,
        DimYellow => 22,
        DimBlue => 23,
        DimMagenta => 24,
        DimCyan => 25,
        DimWhite => 26,
        BrightForeground => 27,
        DimForeground => 28,
    }
}

pub(super) fn full_damage_rows(total_rows: usize, right: u16) -> Vec<(u16, u16, u16)> {
    (0..total_rows)
        .map(|row| (row as u16, 0u16, right))
        .collect()
}

pub(super) fn partial_damage_rows(
    iter: impl IntoIterator<Item = alacritty_terminal::term::LineDamageBounds>,
    right: u16,
) -> Vec<(u16, u16, u16)> {
    let mut seen = [false; 256];
    let mut ranges = Vec::new();
    for bounds in iter {
        let line = bounds.line;
        if line < 256 {
            if seen[line] {
                continue;
            }
            seen[line] = true;
        }
        ranges.push((line as u16, 0u16, right));
    }
    ranges
}

pub(super) fn collect_viewport_cells(
    grid: &alacritty_terminal::grid::Grid<alacritty_terminal::term::cell::Cell>,
    rows: usize,
    cols: usize,
) -> (Vec<PackedCell>, GraphemeExtras, HyperlinkExtras) {
    let mut cells = Vec::with_capacity(cols * rows);
    let mut grapheme_extras = GraphemeExtras::new();

    // Hyperlink dedup: URI string → link ID (1-based).
    let mut uri_to_id: std::collections::HashMap<String, u16> = std::collections::HashMap::new();
    let mut link_map: Vec<(u16, String)> = Vec::new();
    let mut cell_links: Vec<(u32, u16)> = Vec::new();
    let mut next_link_id: u16 = 1;

    for row in 0..rows {
        for col in 0..cols {
            let point = Point::new(Line(row as i32), Column(col));
            let cell = &grid[point];
            let cell_idx = (row * cols + col) as u32;
            let mut packed = pack_cell(cell);

            // Extract OSC 8 hyperlink if present.
            if let Some(hyperlink) = cell.hyperlink() {
                let uri = hyperlink.uri();
                let link_id = *uri_to_id.entry(uri.to_string()).or_insert_with(|| {
                    let id = next_link_id;
                    next_link_id = next_link_id.wrapping_add(1).max(1);
                    link_map.push((id, uri.to_string()));
                    id
                });
                let flags = packed.flags_u16() | FLAG_HYPERLINK;
                packed.flags = flags.to_le_bytes();
                cell_links.push((cell_idx, link_id));
            }

            cells.push(packed);
            if let Some(zw) = cell.zerowidth()
                && !zw.is_empty()
            {
                let extra: String = zw.iter().collect();
                grapheme_extras.push(cell_idx, &extra);
            }
        }
    }

    let hyperlink_extras = HyperlinkExtras {
        cell_links,
        link_map,
    };

    (cells, grapheme_extras, hyperlink_extras)
}

pub(super) fn collect_scrollback_cells(
    grid: &alacritty_terminal::grid::Grid<alacritty_terminal::term::cell::Cell>,
    cols: usize,
    scrollback_rows: usize,
) -> Vec<PackedCell> {
    if scrollback_rows == 0 {
        return Vec::new();
    }
    let mut cells = Vec::with_capacity(scrollback_rows * cols);
    for row_offset in (1..=scrollback_rows).rev() {
        for col in 0..cols {
            let point = Point::new(Line(-(row_offset as i32)), Column(col));
            cells.push(pack_cell(&grid[point]));
        }
    }
    cells
}

pub(super) fn round_cell_size(value: f32) -> Option<u16> {
    if value.is_finite() && value > 0.0 {
        Some(value.round().clamp(1.0, u16::MAX as f32) as u16)
    } else {
        None
    }
}
