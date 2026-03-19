use serde::{Deserialize, Serialize};

// ─── Compact named color IDs for wire format ────────────────────────

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

// ─── PackedColor (4 bytes, POD) ─────────────────────────────────────
//
// Layout: [tag, b1, b2, b3]
//   tag=0 → Named(b1)
//   tag=1 → Rgb(b1, b2, b3)
//   tag=2 → Indexed(b1)

/// Compact color: 4 bytes, zero-copy safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct PackedColor {
    pub tag: u8,
    pub b1: u8,
    pub b2: u8,
    pub b3: u8,
}

/// Color kind discriminant.
pub const COLOR_NAMED: u8 = 0;
pub const COLOR_RGB: u8 = 1;
pub const COLOR_INDEXED: u8 = 2;

impl PackedColor {
    pub const fn named(n: u8) -> Self { PackedColor { tag: COLOR_NAMED, b1: n, b2: 0, b3: 0 } }
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self { PackedColor { tag: COLOR_RGB, b1: r, b2: g, b3: b } }
    pub const fn indexed(i: u8) -> Self { PackedColor { tag: COLOR_INDEXED, b1: i, b2: 0, b3: 0 } }
}

// ─── PackedCell (14 bytes, POD) ─────────────────────────────────────
//
// Layout: [0..4] char UTF-8, [4..8] fg, [8..12] bg, [12..14] flags LE
// `#[repr(C, packed)]` guarantees no padding → bytemuck::cast_slice works.

pub const PACKED_CELL_SIZE: usize = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C, packed)]
pub struct PackedCell {
    pub ch_bytes: [u8; 4],
    pub fg: PackedColor,
    pub bg: PackedColor,
    pub flags: [u8; 2], // u16 LE
}

impl PackedCell {
    pub fn ch(&self) -> char {
        let s = std::str::from_utf8(&self.ch_bytes).unwrap_or("\0");
        s.chars().next().unwrap_or('\0')
    }

    pub fn set_ch(&mut self, c: char) {
        self.ch_bytes = [0; 4];
        c.encode_utf8(&mut self.ch_bytes);
    }

    pub fn flags_u16(&self) -> u16 {
        u16::from_le_bytes(self.flags)
    }

    /// Convenience: create a cell with a char and default colors.
    pub fn with_ch(c: char) -> Self {
        let mut cell = Self::default();
        cell.set_ch(c);
        cell
    }
}

const _: () = assert!(std::mem::size_of::<PackedCell>() == PACKED_CELL_SIZE);

impl Default for PackedCell {
    fn default() -> Self {
        let mut cell = PackedCell {
            ch_bytes: [0; 4],
            fg: PackedColor::named(NAMED_FOREGROUND),
            bg: PackedColor::named(NAMED_BACKGROUND),
            flags: [0; 2],
        };
        cell.set_ch(' ');
        cell
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
    Resize { cols: u16, rows: u16, width: u32, height: u32, cell_width: f32, cell_height: f32 },
    /// Set column width.
    SetColumnWidth { proportion: f64 },
    /// Adjust the split between the active column and its nearest neighbor.
    AdjustColumnSplit { delta: f64 },
    /// Make the active column and its nearest neighbor 50/50.
    EqualizeColumnSplit,
    /// Client is attaching (kept for backwards compat, viewport now sent in ClientHello).
    Attach,
    /// Client is detaching.
    Detach,
    /// Acknowledge received generation.
    Ack { generation: u64 },
    /// Mouse input forwarded to pane (SGR mouse protocol).
    MouseInput { pane_id: u64, button: u8, col: u16, row: u16, pressed: bool, modifiers: u8 },
    /// Switch to a workspace by index.
    SwitchWorkspace { workspace_idx: usize },
    /// Consume the right neighbor column's active pane into the current column.
    ConsumeIntoColumn,
    /// Expel the current column's active tile into a new column to the right.
    ExpelFromColumn,
    /// Request the list of all sessions (running + saved).
    ListSessions,
    /// Kill a session by name.
    KillSession { session_name: String },
    /// Kill the entire server process.
    KillServer,
    /// Switch this client to a different session (create if needed).
    SwitchSession { session_name: String },
    /// Set absolute weights for two adjacent stacked tiles in a column.
    /// Sent on mouse-up after dragging a tile border.
    SetTileWeights { column_idx: usize, top_tile_idx: usize, top_weight: f64, bottom_weight: f64 },
    /// Adjust the split between a specific column pair (identified by left index).
    AdjustColumnSplitAt { column_idx: usize, delta: f64 },
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
    PaneCreated { pane_id: u64, column_idx: usize, cols: u16, rows: u16 },
    /// A pane was closed.
    PaneClosed { pane_id: u64 },
    /// Server is shutting down.
    ServerShutdown,
    /// OSC 52: TUI app requests clipboard write.
    ClipboardStore { data: String },
    /// Response to ListSessions.
    SessionList { sessions: Vec<SessionInfo> },
    /// Client has been switched to a new session (followed by StateSync + FullPaneSync).
    SessionSwitched { session_name: String },
    /// A session was killed.
    SessionKilled { session_name: String },
    /// Error response.
    Error { message: String },
    /// Bell notification from a pane (BEL / \x07).
    Bell { pane_id: u64 },
    /// Inline image placement from Kitty/Sixel protocol.
    ImagePlacement {
        pane_id: u64,
        image_id: u64,
        col: u16,
        row: u16,
        width_cells: u16,
        height_cells: u16,
        pixel_width: u32,
        pixel_height: u32,
        format: String,
        data: Vec<u8>,
    },
}

/// Session info returned in SessionList.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub name: String,
    /// True if the session is currently live in the server.
    pub running: bool,
    /// Number of panes (0 if saved-only).
    pub pane_count: usize,
    /// Number of attached clients.
    pub client_count: usize,
}

/// Serializable layout state (2D: workspaces × columns).
/// This is the single source of truth shared by protocol, server, and client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutState {
    pub workspaces: Vec<WorkspaceState>,
    pub active_workspace_idx: usize,
}

/// One horizontal workspace of columns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceState {
    pub columns: Vec<ColumnState>,
    pub active_column_idx: usize,
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
    pub cursor_line: i16,
    pub cursor_col: u16,
    pub cursor_shape: u8,
    /// Terminal mode flags (mouse mode, alt screen, etc.)
    pub mode_flags: u8,
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
    /// Terminal mode flags (mouse mode, alt screen, etc.)
    pub mode_flags: u8,
    pub title: String,
    /// New scrollback lines that the client hasn't seen yet (oldest first).
    /// Client should prepend these to its buffer before applying the viewport.
    pub scrollback: Vec<PackedCell>, // row-major, scrollback_rows * cols
    pub scrollback_rows: u16,
    pub cells: Vec<PackedCell>, // row-major, rows * cols (viewport)
}

// ─── Zero-copy borrowed CellDelta ───────────────────────────────────

/// Metadata for a single borrowed damage region (offsets into the payload).
#[derive(Debug, Clone)]
pub struct BorrowedRegionMeta {
    pub line: u16,
    pub left: u16,
    pub right: u16,
    /// Byte offset into `CellDeltaBorrowed::payload` where cell data starts.
    pub cells_offset: usize,
    /// Number of cells in this region.
    pub cell_count: usize,
}

/// Zero-copy variant of `CellDelta`. Owns the raw payload `Vec<u8>` and stores
/// parsed region metadata (offsets), but borrows the cell data in-place via
/// `bytemuck::cast_slice` instead of copying into per-region `Vec<PackedCell>`.
#[derive(Debug)]
pub struct CellDeltaBorrowed {
    pub pane_id: u64,
    pub generation: u64,
    pub cursor_line: i16,
    pub cursor_col: u16,
    pub cursor_shape: u8,
    pub mode_flags: u8,
    pub regions: Vec<BorrowedRegionMeta>,
    /// Raw payload bytes — cell data is read directly from here.
    payload: Vec<u8>,
}

impl CellDeltaBorrowed {
    /// Construct from pre-parsed metadata and the raw payload.
    pub fn new(
        pane_id: u64,
        generation: u64,
        cursor_line: i16,
        cursor_col: u16,
        cursor_shape: u8,
        mode_flags: u8,
        regions: Vec<BorrowedRegionMeta>,
        payload: Vec<u8>,
    ) -> Self {
        Self { pane_id, generation, cursor_line, cursor_col, cursor_shape, mode_flags, regions, payload }
    }

    /// Zero-copy access to the cells for a given region index.
    /// Returns an empty slice if `region_idx` is out of bounds.
    pub fn cells(&self, region_idx: usize) -> &[PackedCell] {
        let Some(meta) = self.regions.get(region_idx) else {
            return &[];
        };
        let end = meta.cells_offset + meta.cell_count * PACKED_CELL_SIZE;
        bytemuck::cast_slice(&self.payload[meta.cells_offset..end])
    }
}

// ─── Terminal mode flags ────────────────────────────────────────────

/// Pane has mouse reporting enabled (any mouse mode).
pub const MODE_MOUSE_REPORT: u8 = 0x01;
/// Pane is in alternate screen buffer (e.g. TUI app).
pub const MODE_ALT_SCREEN: u8 = 0x02;
/// Shell integration is active (OSC 133 detected).
pub const MODE_SHELL_INTEGRATION: u8 = 0x04;
/// Kitty keyboard protocol: disambiguate escape codes (CSI u encoding).
pub const MODE_KITTY_KEYBOARD: u8 = 0x08;

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
        let mut cell = PackedCell {
            ch_bytes: [0; 4],
            fg: PackedColor::rgb(255, 128, 0),
            bg: PackedColor::named(5),
            flags: (FLAG_BOLD | FLAG_WIDE_CHAR).to_le_bytes(),
        };
        cell.set_ch('A');
        let bytes: &[u8] = bytemuck::bytes_of(&cell);
        let decoded: &PackedCell = bytemuck::from_bytes(bytes);
        assert_eq!(&cell, decoded);
    }

    #[test]
    fn packed_cell_cjk() {
        let mut cell = PackedCell {
            ch_bytes: [0; 4],
            fg: PackedColor::indexed(196),
            bg: PackedColor::named(0),
            flags: FLAG_WIDE_CHAR.to_le_bytes(),
        };
        cell.set_ch('中');
        let bytes: &[u8] = bytemuck::bytes_of(&cell);
        let decoded: &PackedCell = bytemuck::from_bytes(bytes);
        assert_eq!(decoded.ch(), '中');
        assert_eq!(decoded.fg, PackedColor::indexed(196));
    }

    #[test]
    fn packed_color_roundtrip() {
        for color in [
            PackedColor::named(7),
            PackedColor::rgb(1, 2, 3),
            PackedColor::indexed(200),
        ] {
            let bytes: &[u8] = bytemuck::bytes_of(&color);
            let decoded: &PackedColor = bytemuck::from_bytes(bytes);
            assert_eq!(*decoded, color);
        }
    }
}
