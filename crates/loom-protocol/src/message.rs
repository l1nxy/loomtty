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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageDisplayMode {
    Cells,
    Pixels,
}

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

// ─── PackedCell (16 bytes, POD, SIMD-friendly) ─────────────────────
//
// Layout: [0..4] char UTF-8, [4..8] fg, [8..12] bg, [12..14] flags LE, [14..16] pad
// 16 bytes = 128-bit = one SSE register.  `slice::fill` and bulk copy
// compile to vectorised 128-bit stores.  Four cells per cache line.
// `#[repr(C)]` with explicit padding — no `packed` needed at 16 bytes.

pub const PACKED_CELL_SIZE: usize = 16;
pub const DEFAULT_CELL_CHAR: char = ' ';
pub const DEFAULT_CELL_FLAGS: u16 = 0;
pub const DEFAULT_FOREGROUND: PackedColor = PackedColor::named(NAMED_FOREGROUND);
pub const DEFAULT_BACKGROUND: PackedColor = PackedColor::named(NAMED_BACKGROUND);

#[derive(Debug, Clone, Copy, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C, align(16))]
pub struct PackedCell {
    pub ch_bytes: [u8; 4],
    pub fg: PackedColor,
    pub bg: PackedColor,
    pub flags: [u8; 2], // u16 LE
    pub(crate) _pad: [u8; 2],
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

const _: () = assert!(size_of::<PackedCell>() == PACKED_CELL_SIZE);
const _: () = assert!(align_of::<PackedCell>() <= PACKED_CELL_SIZE);

impl Default for PackedCell {
    fn default() -> Self {
        let mut cell = PackedCell {
            ch_bytes: [0; 4],
            fg: DEFAULT_FOREGROUND,
            bg: DEFAULT_BACKGROUND,
            flags: DEFAULT_CELL_FLAGS.to_le_bytes(),
            _pad: [0; 2],
        };
        cell.set_ch(DEFAULT_CELL_CHAR);
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
        self.build_lookup_with_offset(cells, 0)
    }

    /// Build a lookup for a slice that starts at `start_index` within the
    /// full cell stream represented by these extras. Returned indices are
    /// rebased to the local slice (0-based within `cells`).
    pub fn build_lookup_with_offset(
        &self,
        cells: &[PackedCell],
        start_index: usize,
    ) -> std::collections::HashMap<u32, String> {
        let mut map = std::collections::HashMap::with_capacity(self.0.len());
        for (idx, extra) in &self.0 {
            let i = *idx as usize;
            if i < start_index {
                continue;
            }
            let local = i - start_index;
            if local < cells.len() {
                let mut s = String::new();
                let ch = cells[local].ch();
                s.push(ch);
                s.push_str(extra);
                map.insert(local as u32, s);
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

// ─── Zerocopy wire headers (fixed-layout decode targets) ───────────

/// CellDelta fixed header (43 bytes). Matches the wire layout exactly.
#[derive(Debug, Clone, Copy, zerocopy::FromBytes, zerocopy::KnownLayout, zerocopy::Immutable)]
#[repr(C, packed)]
pub struct CellDeltaHeader {
    pub pane_id: zerocopy::little_endian::U64,
    pub generation: zerocopy::little_endian::U64,
    pub cursor_line: zerocopy::little_endian::I16,
    pub cursor_col: zerocopy::little_endian::U16,
    pub cursor_shape: u8,
    pub mode_flags: zerocopy::little_endian::U16,
    pub received_ack: zerocopy::little_endian::U64,
    pub echo_ack: zerocopy::little_endian::U64,
    pub cols: zerocopy::little_endian::U16,
    pub num_regions: zerocopy::little_endian::U16,
}

const _: () = assert!(size_of::<CellDeltaHeader>() == 43);

/// CellDelta per-region header (10 bytes).
#[derive(Debug, Clone, Copy, zerocopy::FromBytes, zerocopy::KnownLayout, zerocopy::Immutable)]
#[repr(C, packed)]
pub struct CellDeltaRegionHeader {
    pub line: zerocopy::little_endian::U16,
    pub left: zerocopy::little_endian::U16,
    pub right: zerocopy::little_endian::U16,
    pub sm_data_len: zerocopy::little_endian::U32,
}

const _: () = assert!(size_of::<CellDeltaRegionHeader>() == 10);

/// FullPaneSync fixed header (45 bytes). Everything before the variable-length title.
#[derive(Debug, Clone, Copy, zerocopy::FromBytes, zerocopy::KnownLayout, zerocopy::Immutable)]
#[repr(C, packed)]
pub struct FullPaneSyncHeader {
    pub pane_id: zerocopy::little_endian::U64,
    pub generation: zerocopy::little_endian::U64,
    pub cols: zerocopy::little_endian::U16,
    pub rows: zerocopy::little_endian::U16,
    pub cursor_line: zerocopy::little_endian::I16,
    pub cursor_col: zerocopy::little_endian::U16,
    pub cursor_shape: u8,
    pub mode_flags: zerocopy::little_endian::U16,
    pub received_ack: zerocopy::little_endian::U64,
    pub echo_ack: zerocopy::little_endian::U64,
    pub title_len: zerocopy::little_endian::U16,
}

const _: () = assert!(size_of::<FullPaneSyncHeader>() == 45);

// ─── Wire messages ─���─────────────────────��──────────────────────────

/// Messages sent from client to server (msgpack encoded).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    /// Keyboard/paste input to forward to a pane's PTY.
    Input {
        pane_id: u64,
        data: Vec<u8>,
        /// Monotonic sequence number for echo-ack tracking.
        #[serde(default)]
        input_seq: u64,
    },
    /// Request to create a new pane (column right of active).
    CreatePane,
    /// Request to split the active column vertically (new row).
    SplitDown,
    /// Add a new pane as a stacked tile in the active column.
    NewTileBelow,
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
    ListSessions {
        #[serde(default)]
        all: bool,
    },
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
    FocusPane {
        pane_id: u64,
    },
    /// IPC: Send keystrokes to a specific pane in a named session.
    SendKeys {
        session_name: String,
        pane_id: u64,
        keys: Vec<u8>,
    },
    /// IPC: Run a command in a new pane in the named session.
    RunCommand {
        session_name: String,
        command: String,
        cwd: Option<String>,
    },
    /// IPC: Get detailed info about a session.
    GetSessionInfo {
        session_name: String,
    },
    /// IPC: List all panes in a session.
    ListPanes {
        session_name: String,
    },
    /// IPC: Focus a specific pane by ID.
    FocusPaneById {
        session_name: String,
        pane_id: u64,
    },
    /// IPC: Close a specific pane by ID.
    ClosePaneById {
        session_name: String,
        pane_id: u64,
    },
    /// IPC: Create a new pane in the named session.
    CreatePaneIn {
        session_name: String,
    },
    /// IPC: Get the full layout state of a session.
    GetLayout {
        session_name: String,
    },
    /// Apply a layout template to create/recreate a session.
    ApplyTemplate {
        template_name: String,
        session_name: String,
    },
    /// List available templates.
    ListTemplates,
    /// Save current session layout as a template.
    SaveTemplate {
        template_name: String,
        session_name: String,
    },
    /// RTT measurement ping (echoed back as Pong).
    Ping {
        seq: u64,
        client_time_us: u64,
    },
    /// IPC: Capture a pane's grid (and optionally scrollback) as plain text.
    ///
    /// Wire-format note: rmp-serde encodes enum variants by NAME (not by
    /// position), so variant order here is for human readability only.
    /// Renaming the variant breaks every old peer, however. Inner-struct
    /// fields are positional (msgpack array); never reorder, only append.
    CapturePane {
        session_name: String,
        pane_id: u64,
        #[serde(default)]
        opts: CapturePaneOpts,
    },
    /// IPC: list OSC 133 prompt boundaries for a pane.
    ///
    /// Returns one [`PromptMarkInfo`] per recorded command (oldest first).
    /// Empty when the pane never observed OSC 133 — that's the signal to the
    /// caller that shell integration is not active. See [`PromptMarkInfo`]
    /// for the line-numbering convention.
    ListPrompts {
        session_name: String,
        pane_id: u64,
    },
    /// Jump the requesting client's viewport to a neighboring OSC 133 prompt
    /// boundary. The server resolves direction against its own prompt ring,
    /// translates the target absolute line to a client scroll offset, and
    /// replies (to this client only) with [`ServerMessage::SetScrollOffset`].
    ///
    /// `from_offset` is the client's current `scroll_offset` (lines into
    /// history from the live bottom). `direction` is `-1` for "previous"
    /// (older, scroll up) and `+1` for "next" (newer, scroll down).
    ///
    /// When no prompt exists in the requested direction (already at the
    /// oldest known mark, or no mark below the current position) the server
    /// stays silent — clients should not block on a reply.
    JumpToPrompt {
        session_name: String,
        pane_id: u64,
        from_offset: u32,
        direction: i8,
    },
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
    /// Desktop notification from a pane (OSC 9 or OSC 777).
    Notification {
        pane_id: u64,
        title: String,
        body: String,
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
        display_mode: ImageDisplayMode,
        format: String,
        data: Vec<u8>,
    },
    /// Pane title changed (OSC 0 / OSC 2).
    TitleChanged { pane_id: u64, title: String },
    /// Detected AI agent for a pane changed (server runs procinfo on a
    /// slow timer; broadcasts when the resulting `AgentKind` flips).
    /// `agent` is the kebab-case agent kind (`"claude-code"`, `"codex"`,
    /// `"open-code"`, `"droid"`) or `None` when no agent is running in
    /// the pane (idle shell, vim, etc.). Sent as a burst on client
    /// connect so a fresh client gets the current state without
    /// waiting for the next change.
    PaneAgentChanged {
        pane_id: u64,
        agent: Option<String>,
    },
    /// Inline image deletion/invalidation for a pane.
    ImageDeleted { pane_id: u64 },
    /// IPC response: session detail info.
    SessionInfoReply { info: SessionDetailInfo },
    /// IPC response: list of panes.
    PaneListReply { panes: Vec<PaneDetailInfo> },
    /// IPC response: command result.
    CommandResult {
        success: bool,
        message: String,
        pane_id: Option<u64>,
    },
    /// IPC response: full layout state.
    LayoutReply {
        layout: LayoutState,
        session_name: String,
    },
    /// Template was applied successfully.
    TemplateApplied { session_name: String },
    /// List of available templates.
    TemplateList { templates: Vec<TemplateInfo> },
    /// Template was saved successfully.
    TemplateSaved { template_name: String },
    /// Focus hit an edge boundary (for rubber-band bounce animation).
    BounceEdge { direction: BounceDirection },
    /// RTT measurement pong (echo of client Ping).
    Pong { seq: u64, client_time_us: u64 },
    /// IPC response: captured pane text (response to `CapturePane`).
    ///
    /// See `ClientMessage::CapturePane` for the wire-format note about
    /// variant naming vs positional fields.
    PaneCapture {
        session_name: String,
        pane_id: u64,
        text: String,
        /// `true` when the server returned fewer rows than the caller
        /// could have used. Fires in two cases:
        /// 1. **Pre-loop clamp**: requested scrollback was clipped by
        ///    the byte budget or hard-cap before iteration; `text`
        ///    contains no marker because the cutoff happened before any
        ///    content was produced.
        /// 2. **Mid-loop truncation**: iteration hit the byte budget
        ///    mid-capture; trailing rows are dropped and `text` ends
        ///    with a printable marker line.
        #[serde(default)]
        truncated: bool,
    },
    /// IPC response: list of OSC 133 prompt boundaries (response to
    /// `ListPrompts`). Oldest first. See [`PromptMarkInfo`].
    PromptListReply {
        session_name: String,
        pane_id: u64,
        marks: Vec<PromptMarkInfo>,
    },
    /// Set the requesting client's `scroll_offset` for `pane_id` directly.
    /// Sent as the reply to [`ClientMessage::JumpToPrompt`] — single-client
    /// unicast, not a broadcast. `offset` is in the same units as
    /// `ClientPaneGrid::scroll_offset` (lines into history from the live
    /// bottom).
    SetScrollOffset { pane_id: u64, offset: u32 },
}

/// Direction of the edge bounce.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum BounceDirection {
    Left,
    Right,
    Up,
    Down,
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

/// Options for `ClientMessage::CapturePane`.
///
/// Default: capture the visible viewport of the *active* grid (alt-screen
/// when an alt-screen TUI is foregrounded, primary otherwise), trim trailing
/// ASCII spaces per row, preserve a `\n` after every row (including
/// soft-wrapped ones).
///
/// **Wire-format invariant.** rmp-serde serialises structs as positional
/// msgpack arrays. New fields MUST be appended at the end of this struct,
/// and each MUST carry `#[serde(default)]` so older senders that emit a
/// shorter array still deserialise into the new layout. Re-ordering or
/// removing existing fields breaks the wire format for any peer built
/// against an older revision.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapturePaneOpts {
    /// Include this many rows of scrollback above the viewport (`0` =
    /// viewport only). Clamped to the pane's actual history size, and
    /// further capped by the server (see
    /// `loom_term::pane::capture::MAX_CAPTURE_SCROLLBACK_ROWS`). On
    /// alt-screen panes, history is effectively 0 — the primary buffer's
    /// scrollback is not accessible via this call.
    #[serde(default)]
    pub scrollback_rows: u32,
    /// When a row's WRAPLINE flag indicates it soft-wrapped into the next
    /// row, omit the newline so the two rows render as one logical line.
    #[serde(default)]
    pub join_wrapped: bool,
    /// Keep trailing ASCII-space cells on each row. Default trims them
    /// (matches tmux's no-flag behavior on the visible buffer).
    #[serde(default)]
    pub preserve_trailing_spaces: bool,
}

/// One entry in `ServerMessage::PromptListReply`. Captured from OSC 133
/// shell integration.
///
/// Line numbers are in **absolute-line** space: `scrollback_total +
/// cursor.line` at the moment the OSC 133 sequence was observed. The
/// value is monotonic on the primary screen, so an old mark still
/// identifies the same content after the pane has accumulated more
/// output. A mark whose `prompt_line` is below
/// `scrollback_total - history_size` has aged out of the scrollback
/// ring and its content is no longer reachable via `CapturePane`.
///
/// **Wire-format invariant.** rmp-serde serialises this struct as a
/// positional msgpack array. Appending fields is safe (older senders
/// emit a shorter array that fills with defaults); reordering or
/// removing fields breaks every peer built against an older revision.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptMarkInfo {
    /// Absolute line where OSC 133;A was observed (prompt start).
    pub prompt_line: u64,
    /// Absolute line where OSC 133;C was observed (command output start).
    /// `None` when the user submitted an empty command or D arrived first.
    #[serde(default)]
    pub output_line: Option<u64>,
    /// Absolute line where OSC 133;D was observed (command done).
    /// `None` while the command is still running.
    #[serde(default)]
    pub done_line: Option<u64>,
    /// Exit code from OSC 133;D parameters, when present.
    #[serde(default)]
    pub exit_code: Option<i32>,
    /// Milliseconds between OSC 133;C and OSC 133;D.
    /// `None` when either bookend wasn't observed.
    #[serde(default)]
    pub duration_ms: Option<u64>,
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
    ///
    /// No `skip_serializing_if` here: the TS client decoder is
    /// schema-driven on top of `serde-reflection`, which doesn't see
    /// per-field skip annotations and would expect a 4-element wire
    /// tuple even when this field is None. Keeping the field always
    /// on the wire costs one byte (`c0` for `nil`) per ColumnState
    /// with None — negligible since LayoutState is sent only on
    /// connect / focus / resize.
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

impl DamageRegion {
    pub fn cell_count(&self) -> usize {
        self.right
            .checked_sub(self.left)
            .map(|width| width as usize + 1)
            .unwrap_or(0)
    }
}

/// Incremental cell update for a pane (tag 0x20).
#[derive(Debug, Clone, PartialEq)]
pub struct CellDelta {
    pub pane_id: u64,
    pub generation: u64,
    pub cursor_line: i16,
    pub cursor_col: u16,
    pub cursor_shape: u8,
    /// Terminal mode flags (mouse mode, alt screen, kitty keyboard levels, etc.)
    pub mode_flags: u16,
    pub regions: Vec<DamageRegion>,
}

/// Per-frame pane metadata shared by FullPaneSync and CellDelta.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PaneFrameMeta {
    pub pane_id: u64,
    pub generation: u64,
    pub cursor_line: i16,
    pub cursor_col: u16,
    pub cursor_shape: u8,
    /// Terminal mode flags (mouse mode, alt screen, kitty keyboard levels, etc.)
    pub mode_flags: u16,
    /// Highest input_seq the server has *received* for this pane, regardless of
    /// whether the PTY has produced output yet. Bumped synchronously inside
    /// `handle_input`. Used by the client to validate cursor predictions early
    /// (Overwatch/Quake-style packet-level ack) — particularly the case where
    /// the shell silently rejects a Backspace at the prompt boundary, producing
    /// no PTY output. Without this, late_ack never advances and a hold-Backspace
    /// session predicts unboundedly past column 0.
    pub received_ack: u64,
    /// Highest input_seq that has been *late-acked* — i.e. the PTY has drained
    /// output following that input. Used to validate cell predictions, since
    /// only after PTY echo can the client compare predicted characters against
    /// authoritative server cells. Coalesced bursts may over-ack a suffix; the
    /// client treats that as ordinary misprediction.
    pub echo_ack: u64,
}

/// Full pane snapshot (tag 0x21).
#[derive(Debug, Clone, PartialEq)]
pub struct FullPaneSync {
    pub meta: PaneFrameMeta,
    pub cols: u16,
    pub rows: u16,
    pub title: String,
    /// New scrollback lines (oldest first).
    /// When `scrollback_replace` is false, the client appends these to its buffer.
    /// When `scrollback_replace` is true, the client clears its buffer first.
    pub scrollback: Vec<PackedCell>, // row-major, scrollback_rows * cols
    pub scrollback_rows: u32,
    /// When true, the client should replace its entire scrollback buffer
    /// with these rows instead of appending. Used when the server's ring buffer
    /// has rotated past what the client has.
    pub scrollback_replace: bool,
    pub cells: Vec<PackedCell>, // row-major, rows * cols (viewport)
    /// Sparse grapheme overflow for multi-codepoint clusters (emoji, etc.).
    /// Indices are over the concatenated FullPaneSync cell stream:
    /// `scrollback` first, then `cells`.
    pub grapheme_extras: GraphemeExtras,
    /// Sparse hyperlink data from OSC 8 sequences.
    /// Empty unless the terminal application uses explicit hyperlinks.
    pub hyperlink_extras: HyperlinkExtras,
    /// Current working directory from OSC 7 (if reported by the shell).
    pub cwd: Option<String>,
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

impl BorrowedRegionMeta {
    pub fn cell_count(&self) -> usize {
        self.right
            .checked_sub(self.left)
            .map(|width| width as usize + 1)
            .unwrap_or(0)
    }
}

/// Borrowed variant of `CellDelta`. Owns the raw payload `Vec<u8>` and stores
/// parsed region metadata (offsets into SM opcode streams). Cell data is decoded
/// on demand via `decode_sm_cells` rather than zero-copy cast.
#[derive(Debug)]
pub struct CellDeltaBorrowed {
    pub meta: PaneFrameMeta,
    pub cols: u16,
    pub regions: Vec<BorrowedRegionMeta>,
    /// Raw payload bytes — SM opcode streams are read from here.
    payload: Vec<u8>,
}

impl CellDeltaBorrowed {
    /// Construct from pre-parsed metadata and the raw payload.
    pub fn new(
        meta: PaneFrameMeta,
        cols: u16,
        regions: Vec<BorrowedRegionMeta>,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            meta,
            cols,
            regions,
            payload,
        }
    }

    /// Access the raw SM opcode stream for a given region index.
    /// Returns an empty slice if `region_idx` is out of bounds or the
    /// stored offset/length would exceed the payload buffer.
    pub fn sm_data(&self, region_idx: usize) -> &[u8] {
        let Some(meta) = self.regions.get(region_idx) else {
            return &[];
        };
        let end = meta.sm_offset.saturating_add(meta.sm_len);
        if end > self.payload.len() {
            return &[];
        }
        &self.payload[meta.sm_offset..end]
    }

    /// Reclaim the owned payload buffer for reuse.
    pub fn into_payload(self) -> Vec<u8> {
        self.payload
    }
}

// ─── Zero-copy borrowed FullPaneSync ────────────────────────────────

/// Borrowed variant of `FullPaneSync`. Owns the raw payload and stores
/// offsets into SM opcode streams. Cells are decoded on demand, allowing
/// the client to decode directly into its viewport buffer (zero intermediate alloc).
#[derive(Debug)]
pub struct FullPaneSyncBorrowed {
    pub meta: PaneFrameMeta,
    pub cols: u16,
    pub rows: u16,
    pub title: String,
    pub scrollback_rows: u32,
    pub scrollback_replace: bool,
    /// Grapheme extras over the concatenated scrollback + viewport cell stream.
    pub grapheme_extras: GraphemeExtras,
    /// Hyperlink extras (sparse, typically empty).
    pub hyperlink_extras: HyperlinkExtras,
    /// Current working directory.
    pub cwd: Option<String>,
    /// SM data offset + length for scrollback cells.
    scrollback_sm_offset: usize,
    scrollback_sm_len: usize,
    /// SM data offset + length for viewport cells.
    viewport_sm_offset: usize,
    viewport_sm_len: usize,
    /// Raw payload bytes.
    payload: Vec<u8>,
}

impl FullPaneSyncBorrowed {
    /// Construct from pre-parsed metadata and the raw payload.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        meta: PaneFrameMeta,
        cols: u16,
        rows: u16,
        title: String,
        scrollback_rows: u32,
        scrollback_replace: bool,
        grapheme_extras: GraphemeExtras,
        hyperlink_extras: HyperlinkExtras,
        cwd: Option<String>,
        scrollback_sm_offset: usize,
        scrollback_sm_len: usize,
        viewport_sm_offset: usize,
        viewport_sm_len: usize,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            meta,
            cols,
            rows,
            title,
            scrollback_rows,
            scrollback_replace,
            grapheme_extras,
            hyperlink_extras,
            cwd,
            scrollback_sm_offset,
            scrollback_sm_len,
            viewport_sm_offset,
            viewport_sm_len,
            payload,
        }
    }

    /// Access the raw SM opcode stream for scrollback cells.
    /// Returns an empty slice if the stored offset/length would exceed the payload.
    pub fn scrollback_sm_data(&self) -> &[u8] {
        let end = self
            .scrollback_sm_offset
            .saturating_add(self.scrollback_sm_len);
        if end > self.payload.len() {
            return &[];
        }
        &self.payload[self.scrollback_sm_offset..end]
    }

    /// Access the raw SM opcode stream for viewport cells.
    /// Returns an empty slice if the stored offset/length would exceed the payload.
    pub fn viewport_sm_data(&self) -> &[u8] {
        let end = self.viewport_sm_offset.saturating_add(self.viewport_sm_len);
        if end > self.payload.len() {
            return &[];
        }
        &self.payload[self.viewport_sm_offset..end]
    }

    /// Reclaim the owned payload buffer for reuse.
    pub fn into_payload(self) -> Vec<u8> {
        self.payload
    }
}

// ─── Terminal mode flags ────────────────────────────────────────────

/// Pane has mouse reporting enabled (any mouse mode).
pub const MODE_MOUSE_REPORT: u16 = 0x0001;
/// Pane is in alternate screen buffer (e.g. TUI app).
pub const MODE_ALT_SCREEN: u16 = 0x0002;
/// Shell integration is active (OSC 133 detected).
pub const MODE_SHELL_INTEGRATION: u16 = 0x0004;
/// Kitty keyboard protocol level 1: disambiguate escape codes (CSI u encoding).
pub const MODE_KITTY_KEYBOARD: u16 = 0x0008;
/// Bracketed paste mode (DECSET 2004) is active.
pub const MODE_BRACKETED_PASTE: u16 = 0x0010;
/// Focus event reporting (DECSET 1004) is active.
pub const MODE_FOCUS_EVENT: u16 = 0x0020;
/// Synchronized output (DEC 2026) is active — terminal buffers updates.
pub const MODE_SYNCHRONIZED_OUTPUT: u16 = 0x0040;
/// Kitty keyboard protocol level 2: report event types (press/repeat/release).
pub const MODE_KITTY_REPORT_EVENTS: u16 = 0x0100;
/// Kitty keyboard protocol level 3: report alternate keys (shifted/base layout).
pub const MODE_KITTY_REPORT_ALTERNATES: u16 = 0x0200;
/// Kitty keyboard protocol level 4: report all keys as escape codes (no legacy).
pub const MODE_KITTY_REPORT_ALL: u16 = 0x0400;
/// Kitty keyboard protocol level 5: report associated text as codepoints.
pub const MODE_KITTY_REPORT_TEXT: u16 = 0x0800;
/// Password input detected (PTY ECHO disabled in canonical mode).
pub const MODE_PASSWORD_INPUT: u16 = 0x1000;
/// Application cursor keys (DECCKM / DECSET 1) — arrows/Home/End use SS3.
pub const MODE_APP_CURSOR: u16 = 0x2000;
/// Application keypad mode (DECKPAM / DECSET 66) — numpad sends SS3 sequences.
pub const MODE_APP_KEYPAD: u16 = 0x4000;
/// Alternate scroll mode (DECSET 1007) — mouse scroll in alt screen sends arrow keys.
pub const MODE_ALTERNATE_SCROLL: u16 = 0x8000;

/// Mask covering all kitty keyboard protocol flags.
pub const MODE_KITTY_ALL: u16 = MODE_KITTY_KEYBOARD
    | MODE_KITTY_REPORT_EVENTS
    | MODE_KITTY_REPORT_ALTERNATES
    | MODE_KITTY_REPORT_ALL
    | MODE_KITTY_REPORT_TEXT;

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
        assert_eq!(std::mem::size_of::<PackedCell>(), 16);
    }

    #[test]
    fn packed_cell_roundtrip() {
        let mut cell = PackedCell {
            ch_bytes: [0; 4],
            fg: PackedColor::rgb(255, 128, 0),
            bg: PackedColor::named(5),
            flags: (FLAG_BOLD | FLAG_WIDE_CHAR).to_le_bytes(),
            _pad: [0; 2],
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
            _pad: [0; 2],
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

    #[test]
    fn packed_cell_default_layout_stays_stable() {
        let cell = PackedCell::default();
        assert_eq!(cell.ch(), DEFAULT_CELL_CHAR);
        assert_eq!(cell.fg, DEFAULT_FOREGROUND);
        assert_eq!(cell.bg, DEFAULT_BACKGROUND);
        assert_eq!(cell.flags_u16(), DEFAULT_CELL_FLAGS);
    }

    #[test]
    fn damage_region_cell_count_uses_inclusive_bounds() {
        let region = DamageRegion {
            line: 3,
            left: 4,
            right: 6,
            cells: vec![PackedCell::default(); 3],
        };
        assert_eq!(region.cell_count(), 3);
    }

    #[test]
    fn borrowed_region_cell_count_uses_inclusive_bounds() {
        let region = BorrowedRegionMeta {
            line: 7,
            left: 10,
            right: 12,
            sm_offset: 0,
            sm_len: 5,
        };
        assert_eq!(region.cell_count(), 3);
    }
}
