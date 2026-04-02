use ciri_anim::manager::AnimationManager;
use ciri_layout::workspace_set::WorkspaceSet;
use ciri_protocol::message::*;
use crossbeam_channel::{Receiver, Sender};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::grid::ClientPaneGrid;

/// A context menu item.
#[derive(Debug, Clone)]
pub struct ContextMenuItem {
    pub label: String,
    pub action: ContextMenuAction,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub enum ContextMenuAction {
    Copy,
    Paste,
    SelectAll,
    Search,
    OpenLink(String),
    CopyLink(String),
    SplitRight,
    SplitDown,
    ClosePane,
}

/// Context menu state.
#[derive(Debug, Default)]
pub struct ContextMenu {
    pub visible: bool,
    pub x: f32,
    pub y: f32,
    pub target_pane_id: Option<u64>,
    pub items: Vec<ContextMenuItem>,
    pub hovered_index: Option<usize>,
}

/// Text selection state with absolute buffer coordinates.
pub struct Selection {
    pub pane_id: u64,
    pub start: (u16, usize), // (col, buffer_row)
    pub end: (u16, usize),
    pub active: bool, // true while mouse is held
}

pub struct LastLeftClick {
    pub pane_id: u64,
    pub col: u16,
    pub buffer_row: usize,
    pub at: Instant,
    /// Click streak: 1 = single, 2 = double, 3 = triple.
    /// Advances on each press within the multi-click threshold.
    pub count: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoveredLink {
    pub pane_id: u64,
    pub url: String,
    pub start: (u16, usize),
    pub end: (u16, usize),
}

/// Active search session state.
pub struct SearchState {
    pub query: String,
    pub matches: Vec<SearchMatch>,
    pub current_match_idx: usize,
    pub pane_id: u64,
    pub original_scroll_offset: usize,
}

pub struct SearchMatch {
    pub buffer_row: usize,
    pub start_col: u16,
    pub end_col: u16,
}

/// IME composition state.
pub struct ImeState {
    pub preedit_active: bool,
    pub preedit_text: String,
    pub preedit_cursor: Option<usize>,
    pub last_pos: Option<(i32, i32)>,
}

/// Command palette state.
pub struct CommandPaletteState {
    pub query: String,
    pub entries: Vec<PaletteEntry>,
    pub filtered: Vec<usize>, // indices into entries
    pub selected_idx: usize,
    pub hovered_idx: Option<usize>,
    pub sessions_only: bool,
    pub sessions_show_all: bool,
    /// Remote host name currently being queried (loading state).
    pub remote_loading: Option<String>,
    /// (host_name, error_message) for a failed remote query.
    pub remote_error: Option<(String, String)>,
}

pub struct PaletteEntry {
    pub label: String,
    pub kind: PaletteEntryKind,
}

#[derive(Clone)]
pub enum PaletteEntryKind {
    Action(ciri_input::action::Action),
    SwitchSession(String),
    KillSession(String),
    /// A configured remote host — selecting it triggers session probing.
    RemoteHost {
        name: String,
        host: String,
        port: u16,
        ssh_port: u16,
    },
    /// A session on a remote ciri-server (full remote mode).
    RemoteSession {
        host: String,
        port: u16,
        ssh_port: u16,
        session_name: String,
    },
    /// SSH shell fallback (no ciri-server on remote).
    SshShell {
        name: String,
        host: String,
        ssh_port: u16,
    },
    /// Switch to a background connection slot.
    SwitchSlot(String),
    /// Direct-connect to a configured remote host without probing (VSCode SSH style).
    DirectConnect {
        name: String,
        host: String,
        port: u16,
        ssh_port: u16,
    },
    /// Switch to a background slot and then switch session within it.
    SlotSession {
        slot_id: String,
        session_name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopBarHoverRegion {
    Session,
    Workspace,
    Mode,
}

/// Touchpad gesture tracking state.
pub struct GestureState {
    pub scroll_accum: f64,
    pub row_active: bool,
    pub row_start: usize,
}

/// Scrollbar drag tracking state.
pub struct ScrollbarDragInfo {
    pub pane_id: u64,
    pub pane_inner_y: f32,
    pub pane_inner_h: f32,
    pub total_lines: usize,
    pub visible_rows: u16,
}

/// Column/tile border drag resize state.
pub struct ResizeDragState {
    pub col_dragging: Option<usize>,
    pub col_right_idx: Option<usize>,
    pub col_start_x: f32,
    pub col_start_width: f32,
    pub col_delta: f64,
    pub tile_dragging: Option<(usize, usize)>,
    pub tile_start_y: f32,
    pub scrollbar_dragging: Option<ScrollbarDragInfo>,
}

/// Overview zoom mode state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverviewActionHover {
    Close,
    Focus,
}

pub struct OverviewState {
    pub active: bool,
    pub dragging: bool,
    pub drag_last_pos: Option<(f32, f32)>,
    pub hovered_pane: Option<(usize, u64)>,
}

/// Client-side image placement for rendering.
#[derive(Clone)]
pub struct ClientImagePlacement {
    pub image_id: u64,
    pub col: u16,
    pub row: u16,
    pub width_cells: u16,
    pub height_cells: u16,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub display_mode: ciri_protocol::message::ImageDisplayMode,
    pub format: String,
    pub data: Arc<Vec<u8>>,
}

/// Pending paste that needs user confirmation.
#[derive(Debug, Clone)]
pub struct PendingPaste {
    pub info: crate::paste_guard::PasteInfo,
    pub preview: String,
    pub hovered_button: Option<PasteButton>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteButton {
    Paste,
    Cancel,
}

/// Auto-reconnection state.
pub struct ReconnectState {
    pub attempt: u32,
    pub max_attempts: u32,
    pub next_retry: Instant,
    pub backoff: Duration,
}

pub struct ReconnectPlan {
    pub viewport: ciri_protocol::codec::ClientHello,
    pub should_exit: bool,
}

/// Parameters for a remote SSH tunnel connection.
pub struct RemoteConnectionConfig {
    pub host: String,
    pub port: u16,
    pub ssh_port: u16,
}

/// Identifies what kind of connection a slot represents.
#[derive(Debug, Clone)]
pub enum ConnectionKind {
    Local,
    Remote {
        host: String,
        port: u16,
        ssh_port: u16,
    },
}

/// Snapshot of per-connection state, saved when a connection goes to background.
pub struct ConnectionSlot {
    pub id: String,
    pub kind: ConnectionKind,
    pub session_name: String,
    pub server_tx: Sender<ClientMessage>,
    pub server_rx: Receiver<ServerEvent>,
    pub pane_grids: HashMap<u64, ClientPaneGrid>,
    pub workspaces: WorkspaceSet,
    pub expected_pane_ids: HashSet<u64>,
    pub connected: bool,
    pub reconnect_state: Option<ReconnectState>,
    pub pending_session_name: Option<String>,
    pub anim_mgr: AnimationManager,
    pub workspace_last_pane_ids: HashMap<usize, u64>,
    pub selection: Option<Selection>,
    pub broadcast_mode: bool,
    pub image_placements: HashMap<u64, Vec<ClientImagePlacement>>,
    /// Events consumed from `server_rx` while the slot was backgrounded
    /// (e.g. during background session queries). Replayed on restore.
    pub pending_events: std::collections::VecDeque<ServerEvent>,
}

/// Server event forwarded from the connection thread to the application.
/// Re-exported here so AppModel can reference it without depending on
/// platform-specific connection code.
pub enum ServerEvent {
    Control(ServerMessage),
    CellDelta(CellDeltaBorrowed),
    FullPaneSync(FullPaneSync),
    Disconnected,
}

/// Result returned from an async remote host query.
pub struct RemoteQueryResult {
    /// Display name from config.
    pub host_name: String,
    pub host: String,
    pub port: u16,
    pub ssh_port: u16,
    pub result: RemoteProbeResult,
}

/// Outcome of probing a remote host for ciri-server.
pub enum RemoteProbeResult {
    /// ciri-server is available; here are its sessions.
    Sessions(Vec<SessionInfo>),
    /// SSH connected but no ciri-server (handshake failed / connection refused).
    NoServer,
    /// SSH itself failed or timed out.
    Error(String),
}

/// Decision from `prepare_reconnect_plan`.
pub enum ReconnectPlanDecision {
    Try,
    GaveUp,
}
use std::sync::Arc;
