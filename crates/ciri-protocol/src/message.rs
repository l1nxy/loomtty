use serde::{Deserialize, Serialize};

// ─── Safety limits ──────────────────────────────────────────────────

/// Maximum number of cells (cols × rows) allowed in a single grid.
/// Prevents OOM from malformed messages claiming huge grid dimensions.
pub const MAX_GRID_CELLS: usize = 10_000_000;

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
    pub const fn named(n: u8) -> Self {
        PackedColor {
            tag: COLOR_NAMED,
            b1: n,
            b2: 0,
            b3: 0,
        }
    }
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        PackedColor {
            tag: COLOR_RGB,
            b1: r,
            b2: g,
            b3: b,
        }
    }
    pub const fn indexed(i: u8) -> Self {
        PackedColor {
            tag: COLOR_INDEXED,
            b1: i,
            b2: 0,
            b3: 0,
        }
    }
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

/// Sparse grapheme overflow: extra codepoints for cells that hold multi-char
/// grapheme clusters (flag emoji, ZWJ sequences, combining diacritics).
///
/// `cell_index` is the flat row-major index into the cell grid.
/// `extra` contains the combining/zerowidth characters that follow the primary
/// char stored in `PackedCell::ch_bytes`.
///
/// Sent alongside cell data; typically empty (>99.9% of frames have no emoji).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GraphemeExtras(pub Vec<(u32, String)>);

impl GraphemeExtras {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn push(&mut self, cell_index: u32, extra_chars: &str) {
        self.0.push((cell_index, extra_chars.to_string()));
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Build a lookup: cell_index → full grapheme string (primary char + extras).
    /// Call once per frame, then look up by index in the render loop.
    pub fn build_lookup(&self, cells: &[PackedCell]) -> std::collections::HashMap<u32, String> {
        let mut map = std::collections::HashMap::with_capacity(self.0.len());
        for (idx, extra) in &self.0 {
            let i = *idx as usize;
            if i < cells.len() {
                let mut s = String::new();
                let ch = cells[i].ch();
                s.push(ch);
                s.push_str(extra);
                map.insert(*idx, s);
            }
        }
        map
    }
}

/// Sparse hyperlink data: maps cell indices to link IDs, plus a link ID → URI table.
///
/// Sent alongside cell data in FullPaneSync. Typically empty — most frames have
/// no explicit hyperlinks. Uses the same sparse pattern as GraphemeExtras.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HyperlinkExtras {
    /// Cell index → link ID mapping (sparse).
    pub cell_links: Vec<(u32, u16)>,
    /// Link ID → URI mapping.
    pub link_map: Vec<(u16, String)>,
}

impl HyperlinkExtras {
    pub fn new() -> Self {
        Self {
            cell_links: Vec::new(),
            link_map: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.cell_links.is_empty()
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
/// Underline style variants (3 bits, bits 9-11).
/// 0b000 = single (default when FLAG_UNDERLINE is set)
/// 0b001 = double
/// 0b010 = curly
/// 0b011 = dotted
/// 0b100 = dashed
pub const FLAG_UNDERLINE_STYLE_MASK: u16 = 0b111 << 9;
pub const FLAG_UNDERLINE_DOUBLE: u16 = 0b001 << 9;
pub const FLAG_UNDERLINE_CURLY: u16 = 0b010 << 9;
pub const FLAG_UNDERLINE_DOTTED: u16 = 0b011 << 9;
pub const FLAG_UNDERLINE_DASHED: u16 = 0b100 << 9;
/// Line wrapping marker: set on the last cell of a row whose content continues
/// on the next row (soft wrap). Used by the client for scrollback reflow.
pub const FLAG_WRAPLINE: u16 = 1 << 12;
/// Cell is part of an OSC 8 hyperlink. The link ID is in HyperlinkExtras.
pub const FLAG_HYPERLINK: u16 = 1 << 13;

// ─── Wire messages ──────────────────────────────────────────────────

/// Messages sent from client to server (msgpack encoded).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    /// Keyboard/paste input to forward to a pane's PTY.
    Input {
        pane_id: u64,
        data: Vec<u8>,
    },
    /// Request to create a new pane (column right of active).
    CreatePane,
    /// Request to split the active column vertically (new row).
    SplitDown,
    /// Close a pane.
    ClosePane {
        pane_id: u64,
    },
    /// Focus navigation.
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    /// Move pane.
    MovePaneLeft,
    MovePaneRight,
    /// Resize the viewport.
    Resize {
        cols: u16,
        rows: u16,
        width: u32,
        height: u32,
        cell_width: f32,
        cell_height: f32,
    },
    /// Set column width. If `fixed_px` is Some, use fixed pixel width; otherwise proportion.
    SetColumnWidth {
        proportion: f64,
        fixed_px: Option<f64>,
    },
    /// Adjust the split between the active column and its nearest neighbor.
    AdjustColumnSplit {
        delta: f64,
    },
    /// Make the active column and its nearest neighbor 50/50.
    EqualizeColumnSplit,
    /// Client is attaching (kept for backwards compat, viewport now sent in ClientHello).
    Attach,
    /// Client is detaching.
    Detach,
    /// Acknowledge received generation.
    Ack {
        generation: u64,
    },
    /// Mouse input forwarded to pane (SGR mouse protocol).
    MouseInput {
        pane_id: u64,
        button: u8,
        col: u16,
        row: u16,
        pressed: bool,
        modifiers: u8,
    },
    /// Switch to a workspace by index.
    SwitchWorkspace {
        workspace_idx: usize,
    },
    /// Consume the right neighbor column's active pane into the current column.
    ConsumeIntoColumn,
    /// Expel the current column's active tile into a new column to the right.
    ExpelFromColumn,
    /// Request the list of sessions. If `all` is true, include saved (inactive) sessions.
    ListSessions { all: bool },
    /// Kill a session by name.
    KillSession {
        session_name: String,
    },
    /// Kill the entire server process.
    KillServer,
    /// Switch this client to a different session (create if needed).
    SwitchSession {
        session_name: String,
    },
    /// Set absolute weights for two adjacent stacked tiles in a column.
    /// Sent on mouse-up after dragging a tile border.
    SetTileWeights {
        column_idx: usize,
        top_tile_idx: usize,
        top_weight: f64,
        bottom_weight: f64,
    },
    /// Adjust the split between a specific column pair (identified by left index).
    AdjustColumnSplitAt {
        column_idx: usize,
        delta: f64,
    },
    /// Window focus changed (for DECSET 1004 focus event reporting).
    FocusChange {
        focused: bool,
    },
    /// Focus a specific pane by ID (used for focus-follows-mouse).
    FocusPane { pane_id: u64 },
    /// IPC: Send keystrokes to a specific pane in a named session.
    SendKeys { session_name: String, pane_id: u64, keys: Vec<u8> },
    /// IPC: Run a command in a new pane in the named session.
    RunCommand { session_name: String, command: String, cwd: Option<String> },
    /// IPC: Get detailed info about a session.
    GetSessionInfo { session_name: String },
    /// IPC: List all panes in a session.
    ListPanes { session_name: String },
    /// IPC: Focus a specific pane by ID.
    FocusPaneById { session_name: String, pane_id: u64 },
    /// IPC: Close a specific pane by ID.
    ClosePaneById { session_name: String, pane_id: u64 },
    /// IPC: Create a new pane in the named session.
    CreatePaneIn { session_name: String },
    /// IPC: Get the full layout state of a session.
    GetLayout { session_name: String },
    /// Apply a layout template to create/recreate a session.
    ApplyTemplate { template_name: String, session_name: String },
    /// List available templates.
    ListTemplates,
    /// Save current session layout as a template.
    SaveTemplate { template_name: String, session_name: String },
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
    PaneCreated {
        pane_id: u64,
        column_idx: usize,
        cols: u16,
        rows: u16,
    },
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
    /// A shell command completed (requires shell integration / OSC 133).
    CommandCompleted {
        pane_id: u64,
        duration_secs: u64,
        exit_code: Option<i32>,
    },
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
    /// IPC response: session detail info.
    SessionInfoReply { info: SessionDetailInfo },
    /// IPC response: list of panes.
    PaneListReply { panes: Vec<PaneDetailInfo> },
    /// IPC response: command result.
    CommandResult { success: bool, message: String, pane_id: Option<u64> },
    /// IPC response: full layout state.
    LayoutReply { layout: LayoutState, session_name: String },
    /// Template was applied successfully.
    TemplateApplied { session_name: String },
    /// List of available templates.
    TemplateList { templates: Vec<TemplateInfo> },
    /// Template was saved successfully.
    TemplateSaved { template_name: String },
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

/// Detailed session info returned by GetSessionInfo IPC command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDetailInfo {
    pub name: String,
    pub running: bool,
    pub pane_count: usize,
    pub client_count: usize,
    pub workspace_count: usize,
    pub active_workspace: usize,
}

/// Detailed pane info returned by ListPanes IPC command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneDetailInfo {
    pub pane_id: u64,
    pub cols: u16,
    pub rows: u16,
    pub title: String,
    pub cwd: Option<String>,
    pub is_active: bool,
    pub workspace_idx: usize,
    pub column_idx: usize,
    pub tile_idx: usize,
}

/// Template info returned in TemplateList.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateInfo {
    pub name: String,
    pub description: Option<String>,
    pub workspace_count: usize,
    pub total_panes: usize,
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
    /// If set, column uses a fixed pixel width instead of proportion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_fixed_px: Option<f64>,
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
    /// Sparse grapheme overflow for multi-codepoint clusters (emoji, etc.).
    /// Empty for >99.9% of frames.
    pub grapheme_extras: GraphemeExtras,
    /// Sparse hyperlink data from OSC 8 sequences.
    /// Empty unless the terminal application uses explicit hyperlinks.
    pub hyperlink_extras: HyperlinkExtras,
}

// ─── Zero-copy borrowed CellDelta ───────────────────────────────────

/// Metadata for a single borrowed damage region (SM-encoded, offsets into the payload).
#[derive(Debug, Clone)]
pub struct BorrowedRegionMeta {
    pub line: u16,
    pub left: u16,
    pub right: u16,
    /// Byte offset into `CellDeltaBorrowed::payload` where SM opcode data starts.
    pub sm_offset: usize,
    /// Length of the SM opcode stream for this region.
    pub sm_len: usize,
}

/// Borrowed variant of `CellDelta`. Owns the raw payload `Vec<u8>` and stores
/// parsed region metadata (offsets into SM opcode streams). Cell data is decoded
/// on demand via `decode_sm_cells` rather than zero-copy cast.
#[derive(Debug)]
pub struct CellDeltaBorrowed {
    pub pane_id: u64,
    pub generation: u64,
    pub cursor_line: i16,
    pub cursor_col: u16,
    pub cursor_shape: u8,
    pub mode_flags: u8,
    pub cols: u16,
    pub regions: Vec<BorrowedRegionMeta>,
    /// Raw payload bytes — SM opcode streams are read from here.
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
        cols: u16,
        regions: Vec<BorrowedRegionMeta>,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            pane_id,
            generation,
            cursor_line,
            cursor_col,
            cursor_shape,
            mode_flags,
            cols,
            regions,
            payload,
        }
    }

    /// Access the raw SM opcode stream for a given region index.
    /// Returns an empty slice if `region_idx` is out of bounds.
    pub fn sm_data(&self, region_idx: usize) -> &[u8] {
        let Some(meta) = self.regions.get(region_idx) else {
            return &[];
        };
        &self.payload[meta.sm_offset..meta.sm_offset + meta.sm_len]
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
/// Bracketed paste mode (DECSET 2004) is active.
pub const MODE_BRACKETED_PASTE: u8 = 0x10;
/// Focus event reporting (DECSET 1004) is active.
pub const MODE_FOCUS_EVENT: u8 = 0x20;
/// Synchronized output (DEC 2026) is active — terminal buffers updates.
pub const MODE_SYNCHRONIZED_OUTPUT: u8 = 0x40;

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
