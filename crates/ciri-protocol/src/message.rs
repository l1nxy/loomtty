use serde::{Deserialize, Serialize};

/// Messages sent from client to server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    /// Keyboard input to forward to the active pane's PTY.
    Input { pane_id: u64, data: Vec<u8> },
    /// Request to create a new pane (column right of active).
    CreatePane,
    /// Request to split the active column vertically.
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
    Resize { width: u32, height: u32 },
    /// Set column width.
    SetColumnWidth { proportion: f64 },
    /// Client is attaching to the session.
    Attach,
    /// Client is detaching.
    Detach,
    /// Request a full state sync.
    SyncRequest,
}

/// Messages sent from server to client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    /// Full state snapshot for initial sync or after reconnect.
    StateSync {
        layout: LayoutState,
        panes: Vec<PaneState>,
    },
    /// Incremental cell updates for a pane.
    CellUpdate {
        pane_id: u64,
        cells: Vec<CellChange>,
    },
    /// A pane was created.
    PaneCreated { pane_id: u64, column_idx: usize },
    /// A pane was closed.
    PaneClosed { pane_id: u64 },
    /// Layout changed (focus, column widths, etc.).
    LayoutUpdate { layout: LayoutState },
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

/// Serializable pane state (for full sync).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneState {
    pub pane_id: u64,
    pub cols: u16,
    pub rows: u16,
    pub cells: Vec<Vec<CellData>>,
    pub cursor_line: i32,
    pub cursor_col: usize,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellData {
    pub c: char,
    pub fg: [u8; 3],
    pub bg: [u8; 3],
    pub flags: u32,
}

/// An incremental cell change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellChange {
    pub line: i32,
    pub col: usize,
    pub cell: CellData,
}
