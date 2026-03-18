use serde::{Deserialize, Serialize};

// ─── Compact named color IDs for wire format ────────────────────────
// Maps from alacritty NamedColor discriminants to compact u8 IDs.
// Standard ANSI 0-15 map directly; extended named colors are compacted
// so that all values fit in u8 (alacritty Foreground=256 etc. would not).

pub const NAMED_BLACK: u8 = 0;
pub const NAMED_RED: u8 = 1;
pub const NAMED_GREEN: u8 = 2;
pub const NAMED_YELLOW: u8 = 3;
pub const NAMED_BLUE: u8 = 4;
pub const NAMED_MAGENTA: u8 = 5;
pub const NAMED_CYAN: u8 = 6;
pub const NAMED_WHITE: u8 = 7;
pub const NAMED_BRIGHT_BLACK: u8 = 8;
pub const NAMED_BRIGHT_RED: u8 = 9;
pub const NAMED_BRIGHT_GREEN: u8 = 10;
pub const NAMED_BRIGHT_YELLOW: u8 = 11;
pub const NAMED_BRIGHT_BLUE: u8 = 12;
pub const NAMED_BRIGHT_MAGENTA: u8 = 13;
pub const NAMED_BRIGHT_CYAN: u8 = 14;
pub const NAMED_BRIGHT_WHITE: u8 = 15;
pub const NAMED_FOREGROUND: u8 = 16;
pub const NAMED_BACKGROUND: u8 = 17;
pub const NAMED_CURSOR: u8 = 18;
pub const NAMED_DIM_BLACK: u8 = 19;
pub const NAMED_DIM_RED: u8 = 20;
pub const NAMED_DIM_GREEN: u8 = 21;
pub const NAMED_DIM_YELLOW: u8 = 22;
pub const NAMED_DIM_BLUE: u8 = 23;
pub const NAMED_DIM_MAGENTA: u8 = 24;
pub const NAMED_DIM_CYAN: u8 = 25;
pub const NAMED_DIM_WHITE: u8 = 26;
pub const NAMED_BRIGHT_FOREGROUND: u8 = 27;
pub const NAMED_DIM_FOREGROUND: u8 = 28;

// ─── PackedColor (4 bytes) ──────────────────────────────────────────

/// Compact color representation: transmits semantic color (Named/Indexed)
/// so the client can resolve using its own theme.
///
/// `Named(u8)` uses our compact mapping constants (`NAMED_*`), NOT raw
/// alacritty NamedColor discriminants (which exceed u8 range).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackedColor {
    Named(u8),       // Compact named color ID (see NAMED_* constants)
    Rgb(u8, u8, u8),
    Indexed(u8),
}

impl PackedColor {
    pub fn to_bytes(self) -> [u8; 4] {
        match self {
            PackedColor::Named(n) => [0, n, 0, 0],
            PackedColor::Rgb(r, g, b) => [1, r, g, b],
            PackedColor::Indexed(i) => [2, i, 0, 0],
        }
    }

    pub fn from_bytes(b: [u8; 4]) -> Self {
        match b[0] {
            0 => PackedColor::Named(b[1]),
            1 => PackedColor::Rgb(b[1], b[2], b[3]),
            2 => PackedColor::Indexed(b[1]),
            _ => PackedColor::Named(0), // fallback to black
        }
    }
}

// ─── PackedCell (14 bytes) ──────────────────────────────────────────

/// Compact cell representation for wire transfer.
/// Layout: [0..4] char UTF-8, [4..8] fg, [8..12] bg, [12..14] flags
pub const PACKED_CELL_SIZE: usize = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedCell {
    pub ch: char,
    pub fg: PackedColor,
    pub bg: PackedColor,
    pub flags: u16,
}

impl Default for PackedCell {
    fn default() -> Self {
        PackedCell {
            ch: ' ',
            fg: PackedColor::Named(NAMED_FOREGROUND),
            bg: PackedColor::Named(NAMED_BACKGROUND),
            flags: 0,
        }
    }
}

impl PackedCell {
    pub fn to_bytes(self) -> [u8; PACKED_CELL_SIZE] {
        let mut buf = [0u8; PACKED_CELL_SIZE];
        let mut ch_buf = [0u8; 4];
        self.ch.encode_utf8(&mut ch_buf);
        buf[0..4].copy_from_slice(&ch_buf);
        buf[4..8].copy_from_slice(&self.fg.to_bytes());
        buf[8..12].copy_from_slice(&self.bg.to_bytes());
        buf[12..14].copy_from_slice(&self.flags.to_le_bytes());
        buf
    }

    pub fn from_bytes(buf: &[u8; PACKED_CELL_SIZE]) -> Self {
        let ch = {
            let s = std::str::from_utf8(&buf[0..4]).unwrap_or("\0");
            s.chars().next().unwrap_or('\0')
        };
        let fg = PackedColor::from_bytes([buf[4], buf[5], buf[6], buf[7]]);
        let bg = PackedColor::from_bytes([buf[8], buf[9], buf[10], buf[11]]);
        let flags = u16::from_le_bytes([buf[12], buf[13]]);
        PackedCell { ch, fg, bg, flags }
    }
}

// ─── Cell flag constants (mirrors alacritty CellFlags) ──────────────

pub const FLAG_WIDE_CHAR: u16 = 1 << 0;
pub const FLAG_WIDE_CHAR_SPACER: u16 = 1 << 1;
pub const FLAG_BOLD: u16 = 1 << 2;
pub const FLAG_ITALIC: u16 = 1 << 3;
pub const FLAG_UNDERLINE: u16 = 1 << 4;
pub const FLAG_INVERSE: u16 = 1 << 5;
pub const FLAG_DIM: u16 = 1 << 6;
pub const FLAG_STRIKEOUT: u16 = 1 << 7;
pub const FLAG_HIDDEN: u16 = 1 << 8;

// ─── Wire messages ──────────────────────────────────────────────────

/// Messages sent from client to server (msgpack encoded).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    /// Keyboard/paste input to forward to a pane's PTY.
    Input { pane_id: u64, data: Vec<u8> },
    /// Request to create a new pane (column right of active).
    CreatePane,
    /// Request to split the active column vertically (new row).
    SplitDown,
    /// Close a pane.
    ClosePane { pane_id: u64 },
    /// Focus navigation.
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    /// Move pane.
    MovePaneLeft,
    MovePaneRight,
    /// Resize the viewport.
    Resize { cols: u16, rows: u16, width: u32, height: u32 },
    /// Set column width.
    SetColumnWidth { proportion: f64 },
    /// Client is attaching to the session.
    Attach { cols: u16, rows: u16 },
    /// Client is detaching.
    Detach,
    /// Acknowledge received generation.
    Ack { generation: u64 },
}

/// Control messages from server to client (msgpack encoded, tags 0x10-0x1F).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    /// Full state snapshot for initial sync or after reconnect.
    StateSync {
        layout: LayoutState,
        pane_ids: Vec<u64>,
    },
    /// Layout changed (focus, column widths, etc.).
    LayoutUpdate { layout: LayoutState },
    /// A pane was created.
    PaneCreated { pane_id: u64, column_idx: usize },
    /// A pane was closed.
    PaneClosed { pane_id: u64 },
    /// Server is shutting down.
    ServerShutdown,
}

/// Serializable layout state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutState {
    pub columns: Vec<ColumnState>,
    pub active_column_idx: usize,
    pub view_offset_x: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnState {
    pub tiles: Vec<TileState>,
    pub active_tile_idx: usize,
    pub width_proportion: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileState {
    pub pane_id: u64,
    pub weight: f32,
}

// ─── Binary hot-path messages (CellDelta, FullPaneSync) ─────────────

/// Damage region for a single line.
#[derive(Debug, Clone, PartialEq)]
pub struct DamageRegion {
    pub line: u16,
    pub left: u16,
    pub right: u16, // inclusive
    pub cells: Vec<PackedCell>,
}

/// Incremental cell update for a pane (tag 0x20).
#[derive(Debug, Clone, PartialEq)]
pub struct CellDelta {
    pub pane_id: u64,
    pub generation: u64,
    pub regions: Vec<DamageRegion>,
}

/// Full pane snapshot (tag 0x21).
#[derive(Debug, Clone, PartialEq)]
pub struct FullPaneSync {
    pub pane_id: u64,
    pub generation: u64,
    pub cols: u16,
    pub rows: u16,
    pub cursor_line: i16,
    pub cursor_col: u16,
    pub cursor_shape: u8,
    pub title: String,
    pub cells: Vec<PackedCell>, // row-major, cols * rows
}

// ─── Cursor shape encoding ──────────────────────────────────────────

pub const CURSOR_BLOCK: u8 = 0;
pub const CURSOR_UNDERLINE: u8 = 1;
pub const CURSOR_BEAM: u8 = 2;
pub const CURSOR_HIDDEN: u8 = 3;
pub const CURSOR_HOLLOW_BLOCK: u8 = 4;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_cell_size() {
        assert_eq!(std::mem::size_of::<[u8; PACKED_CELL_SIZE]>(), 14);
    }

    #[test]
    fn packed_cell_roundtrip() {
        let cell = PackedCell {
            ch: 'A',
            fg: PackedColor::Rgb(255, 128, 0),
            bg: PackedColor::Named(5),
            flags: FLAG_BOLD | FLAG_WIDE_CHAR,
        };
        let bytes = cell.to_bytes();
        let decoded = PackedCell::from_bytes(&bytes);
        assert_eq!(cell, decoded);
    }

    #[test]
    fn packed_cell_cjk() {
        let cell = PackedCell {
            ch: '中',
            fg: PackedColor::Indexed(196),
            bg: PackedColor::Named(0),
            flags: FLAG_WIDE_CHAR,
        };
        let bytes = cell.to_bytes();
        let decoded = PackedCell::from_bytes(&bytes);
        assert_eq!(decoded.ch, '中');
        assert_eq!(decoded.fg, PackedColor::Indexed(196));
    }

    #[test]
    fn packed_color_roundtrip() {
        for color in [
            PackedColor::Named(7),
            PackedColor::Rgb(1, 2, 3),
            PackedColor::Indexed(200),
        ] {
            let bytes = color.to_bytes();
            assert_eq!(PackedColor::from_bytes(bytes), color);
        }
    }
}
