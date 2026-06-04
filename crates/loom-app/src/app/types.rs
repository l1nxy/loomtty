use loom_anim::manager::AnimationManager;
use loom_layout::workspace_set::WorkspaceSet;
use loom_protocol::message::*;
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
    /// Apply a theme preset by name and live-reload the chrome.
    /// Used by the settings panel's Theme dropdown — context_menu is
    /// the existing popup with a hit-walker / outside-click handler,
    /// so the Theme dropdown reuses it instead of duplicating that
    /// machinery.
    SetThemePreset(String),
    /// Pick a value for one of the settings panel's enum dropdowns.
    /// `field_id` is the [`SettingsField`] discriminant (see the
    /// settings_panel schema in `crates/loom/src/app/ui/settings_panel/
    /// schema.rs`); `value` is the kebab-case variant the user picked.
    /// Carrying an opaque u16 here keeps `loom-app` from depending on
    /// the UI-side schema enum.
    SetSettingsEnum {
        field_id: u16,
        value: String,
    },
}

/// Sidebar category currently selected in the settings panel.
/// `Default` resolves to [`Self::Appearance`] so the panel's first
/// open lands on the most visually-impactful section.
///
/// The actual rows shown for each category, plus the read/write/clamp
/// behaviour for every field, live in the UI-side schema table — this
/// enum is purely the navigation discriminant the panel persists on
/// `AppModel.settings_category`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SettingsCategory {
    #[default]
    Appearance,
    Font,
    Terminal,
    Layout,
    Animation,
    Input,
    StatusBar,
    Prediction,
}

impl SettingsCategory {
    /// Sidebar order — also doubles as the iteration order for tests.
    pub const ALL: &'static [SettingsCategory] = &[
        Self::Appearance,
        Self::Font,
        Self::Terminal,
        Self::Layout,
        Self::Animation,
        Self::Input,
        Self::StatusBar,
        Self::Prediction,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Font => "Font",
            Self::Terminal => "Terminal",
            Self::Layout => "Layout",
            Self::Animation => "Animation",
            Self::Input => "Input",
            Self::StatusBar => "Status Bar",
            Self::Prediction => "Prediction",
        }
    }
}

/// Context menu state.
#[derive(Debug, Default)]
pub struct ContextMenu {
    pub visible: bool,
    pub x: f32,
    pub y: f32,
    pub target_pane_id: Option<u64>,
    pub items: Vec<ContextMenuItem>,
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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
    pub sessions_only: bool,
    /// Remote host name currently being queried (loading state).
    pub remote_loading: Option<String>,
    /// (host_name, error_message) for a failed remote query.
    pub remote_error: Option<(String, String)>,
    /// When true, the palette is in "remote host input" mode:
    /// the query field is used to type a `user@host[:port]` address.
    pub remote_input_mode: bool,
}

impl CommandPaletteState {
    /// Move `selected_idx` by `delta` filtered entries, skipping over
    /// non-selectable items (`SectionHeader`s). Positive `delta` moves
    /// down, negative moves up. `wrap` controls the boundary behavior:
    /// keyboard navigation passes `true` (Up at the top wraps to the
    /// bottom); mouse-wheel scrolling passes `false` (clamps).
    ///
    /// Honors the full magnitude of `delta`: a wheel event reporting
    /// 3 lines moves three selectable entries, not just one. Each unit
    /// step independently skips past `SectionHeader` rows; if a step
    /// can't find any selectable entry within `filtered.len()` skip
    /// iterations the loop bails and `selected_idx` keeps the last
    /// successful position.
    pub fn move_selection(&mut self, delta: i32, wrap: bool) {
        if self.filtered.is_empty() || delta == 0 {
            return;
        }
        let len = self.filtered.len();
        let dir: isize = if delta > 0 { 1 } else { -1 };
        let steps = (delta.unsigned_abs() as usize).min(len);
        for _ in 0..steps {
            if !self.advance_one_selectable(dir, wrap) {
                // No selectable position reachable in the requested
                // direction (e.g. clamped at boundary with all
                // remaining entries non-selectable). Stop early.
                return;
            }
        }
    }

    /// Advance `selected_idx` by exactly one selectable entry in the
    /// direction `dir` (`+1` or `-1`), respecting `wrap`. Returns
    /// `true` if a new position was committed, `false` if no
    /// reachable position is selectable.
    fn advance_one_selectable(&mut self, dir: isize, wrap: bool) -> bool {
        let len = self.filtered.len();
        let mut idx = self.selected_idx;
        for _ in 0..len {
            let next = idx as isize + dir;
            let next_idx = if next < 0 {
                if wrap {
                    len - 1
                } else {
                    return false;
                }
            } else if next as usize >= len {
                if wrap {
                    0
                } else {
                    return false;
                }
            } else {
                next as usize
            };
            idx = next_idx;
            if self.entries[self.filtered[idx]].kind.is_selectable() {
                self.selected_idx = idx;
                return true;
            }
        }
        false
    }
}

pub struct PaletteEntry {
    pub label: String,
    /// Lowercase form of `label` cached at construction so the per-keystroke
    /// fuzzy filter doesn't `to_lowercase()` every entry's label on every
    /// edit. Always derived from `label`; constructors should use
    /// [`PaletteEntry::new`] rather than building this directly.
    pub lowercase_label: String,
    pub kind: PaletteEntryKind,
}

impl PaletteEntry {
    pub fn new(label: impl Into<String>, kind: PaletteEntryKind) -> Self {
        let label = label.into();
        let lowercase_label = label.to_lowercase();
        Self {
            label,
            lowercase_label,
            kind,
        }
    }
}

#[derive(Clone)]
pub enum PaletteEntryKind {
    /// Non-selectable section header for visual grouping.
    SectionHeader(String),
    Action(loom_input::action::Action),
    /// Unified session entry — navigates to the right slot + session in one click.
    /// Replaces the old SwitchSession / SwitchSlot / SlotSession triad: from the
    /// user's perspective there's just one concept — "go to this session".
    GoToSession {
        slot_id: String,
        session_name: String,
    },
    /// Kill a session on the current slot (remote kill not supported via palette).
    KillSession(String),
    /// A configured remote host — selecting it triggers session probing.
    RemoteHost {
        name: String,
        host: String,
        port: u16,
        ssh_port: u16,
    },
    /// A session on a remote loom-server (full remote mode).
    RemoteSession {
        host: String,
        port: u16,
        ssh_port: u16,
        session_name: String,
    },
    /// SSH shell fallback (no loom-server on remote).
    SshShell {
        name: String,
        host: String,
        ssh_port: u16,
    },
    /// Direct-connect to a configured remote host without probing (VSCode SSH style).
    DirectConnect {
        name: String,
        host: String,
        port: u16,
        ssh_port: u16,
    },
    /// Fixed entry: "Connect to New Host" — switches palette to input mode.
    ConnectRemotePrompt,
}

impl PaletteEntryKind {
    /// Whether this entry can be selected/executed by the user.
    /// Section headers are purely visual and cannot be selected.
    pub fn is_selectable(&self) -> bool {
        !matches!(self, PaletteEntryKind::SectionHeader(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

/// Context menu scrollbar drag tracking state. Lives alongside the
/// pane scrollbar drag so the existing mouse-up tear-down covers both.
pub struct ContextMenuScrollbarDrag {
    /// Mouse Y at the moment the drag started — used as the reference
    /// for offset deltas during move events.
    pub start_my: f32,
    /// `context_menu_scroll_offset` at the moment the drag started.
    pub start_offset: usize,
    /// Pixel range the thumb can travel through (`track_h - thumb_h`).
    /// Combined with `max_offset` it gives the rows-per-pixel rate.
    pub thumb_travel: f32,
    /// Maximum legal offset (`total_rows - visible_rows`).
    pub max_offset: usize,
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
    /// Drag state for the context menu's own scrollbar thumb.
    pub context_menu_scrollbar_dragging: Option<ContextMenuScrollbarDrag>,
}

/// Overview zoom mode state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OverviewActionHover {
    Close,
    Focus,
}

pub struct OverviewState {
    pub active: bool,
    pub dragging: bool,
    pub drag_last_pos: Option<(f32, f32)>,
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
    pub display_mode: loom_protocol::message::ImageDisplayMode,
    pub format: String,
    pub data: Arc<Vec<u8>>,
}

/// Pending paste that needs user confirmation.
#[derive(Debug, Clone)]
pub struct PendingPaste {
    pub info: crate::paste_guard::PasteInfo,
    pub preview: String,
    pub target: PendingPasteTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PasteButton {
    Paste,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PendingPasteTarget {
    Terminal,
    CommandPalette,
    Search,
}

/// Auto-reconnection state.
pub struct ReconnectState {
    pub attempt: u32,
    pub max_attempts: u32,
    pub next_retry: Instant,
    pub backoff: Duration,
}

pub struct ReconnectPlan {
    pub viewport: loom_protocol::codec::ClientHello,
    pub should_exit: bool,
}

/// Parameters for a remote SSH tunnel connection.
pub struct RemoteConnectionConfig {
    pub host: String,
    pub port: u16,
    pub ssh_port: u16,
}

/// A remote host the user previously connected to. Persisted across runs so
/// the palette can offer recent connections without re-typing the address.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecentHost {
    pub host: String,
    pub port: u16,
    pub ssh_port: u16,
    /// Unix epoch seconds — when the user last successfully connected.
    pub last_used: u64,
    /// Name of the remote session attached on the last successful connect.
    /// `None` for entries written by older builds (serde default fills in).
    /// Drives "remember last session" auto-connect: revisits prefer this
    /// name if the remote still has it, falling back to the most-recent
    /// session otherwise.
    #[serde(default)]
    pub last_session: Option<String>,
}

/// In-flight intent to auto-connect to a remote after an async session
/// query completes. Set when the user picks a `DirectConnect` palette row
/// or types a host into the prompt; consumed by the query-result handler
/// which picks the right session and triggers the actual connect.
#[derive(Debug, Clone)]
pub struct PendingAutoConnect {
    pub host: String,
    pub port: u16,
    pub ssh_port: u16,
    /// `last_session` from `recent_hosts.json`, if any. Preferred when the
    /// remote still hosts a session of that name.
    pub preferred_session: Option<String>,
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
    FullPaneSync(FullPaneSyncBorrowed),
    Disconnected(DisconnectReason),
}

/// Why a server connection ended. Each emission point in the connection
/// thread classifies the failure so the UI can distinguish "fix your config"
/// from "just reconnect" and skip backoff on permanent errors.
#[derive(Debug, Clone)]
pub enum DisconnectReason {
    // ── Permanent: don't retry ──
    /// `ssh` binary missing from PATH.
    SshNotFound,
    /// `Command::spawn` failed for some other reason (e.g. fork).
    SshSpawnFailed(String),
    /// ssh: Could not resolve hostname …
    DnsFailure(String),
    /// ssh: Permission denied (publickey,…)
    PermissionDenied(String),
    /// known_hosts mismatch / man-in-the-middle warning.
    HostKeyChanged(String),
    /// ssh: connect to host X port Y: Connection refused
    ConnectionRefused(String),
    /// Protocol handshake rejected (version mismatch, bad magic, …).
    HandshakeFailed(String),
    /// Input validation bounced this target.
    InvalidTarget(String),
    /// User cancelled via Esc while connecting.
    Cancelled,

    // ── Transient: reconnect makes sense ──
    /// No bytes received within the startup grace window.
    Timeout,
    /// Stream closed cleanly mid-session.
    RemoteEof,
    /// Unclassified IO error (retry with backoff).
    Other(String),
}

impl DisconnectReason {
    /// Permanent failures short-circuit the reconnect loop so the user sees
    /// the error banner instead of 10 attempts of the same typo.
    pub fn is_permanent(&self) -> bool {
        use DisconnectReason::*;
        matches!(
            self,
            SshNotFound
                | SshSpawnFailed(_)
                | DnsFailure(_)
                | PermissionDenied(_)
                | HostKeyChanged(_)
                | ConnectionRefused(_)
                | HandshakeFailed(_)
                | InvalidTarget(_)
                | Cancelled
        )
    }
}

impl std::fmt::Display for DisconnectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use DisconnectReason::*;
        match self {
            SshNotFound => write!(f, "ssh binary not found in PATH"),
            SshSpawnFailed(e) => write!(f, "failed to launch ssh: {e}"),
            DnsFailure(s) => write!(f, "could not resolve hostname: {s}"),
            PermissionDenied(s) => write!(f, "permission denied: {s}"),
            HostKeyChanged(s) => write!(f, "host key changed: {s}"),
            ConnectionRefused(s) => write!(f, "connection refused: {s}"),
            HandshakeFailed(s) => write!(f, "handshake failed: {s}"),
            InvalidTarget(s) => write!(f, "invalid target: {s}"),
            Cancelled => write!(f, "cancelled"),
            Timeout => write!(f, "connection timed out"),
            RemoteEof => write!(f, "connection closed"),
            Other(s) => write!(f, "{s}"),
        }
    }
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

/// Outcome of probing a remote host for loom-server.
#[derive(Clone)]
pub enum RemoteProbeResult {
    /// loom-server is available; here are its sessions.
    Sessions(Vec<SessionInfo>),
    /// SSH connected but no loom-server (handshake failed / connection refused).
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

#[cfg(test)]
mod move_selection_tests {
    use super::*;
    use loom_input::action::Action;

    fn header(label: &str) -> PaletteEntry {
        PaletteEntry::new(label, PaletteEntryKind::SectionHeader(label.to_string()))
    }

    fn action(label: &str) -> PaletteEntry {
        PaletteEntry::new(label, PaletteEntryKind::Action(Action::ClosePane))
    }

    /// Build a state with `entries`, all included in `filtered`, and the
    /// initial selection on `start_idx`.
    fn state(entries: Vec<PaletteEntry>, start_idx: usize) -> CommandPaletteState {
        let filtered = (0..entries.len()).collect();
        CommandPaletteState {
            query: String::new(),
            entries,
            filtered,
            selected_idx: start_idx,
            sessions_only: false,
            remote_loading: None,
            remote_error: None,
            remote_input_mode: false,
        }
    }

    #[test]
    fn single_step_skips_section_header() {
        let mut s = state(vec![action("a"), header("h"), action("b"), action("c")], 0);
        s.move_selection(1, true);
        assert_eq!(s.selected_idx, 2, "single-step down jumps over the header");
    }

    #[test]
    fn large_delta_clamps_to_filtered_len() {
        // 3 selectable rows, ask for delta = 1000 — should land on the
        // last selectable index without spinning forever or panicking.
        let mut s = state(vec![action("a"), action("b"), action("c")], 0);
        s.move_selection(1000, false);
        assert_eq!(s.selected_idx, 2);
    }

    #[test]
    fn wrap_at_top_when_wrap_true() {
        let mut s = state(vec![action("a"), action("b"), action("c")], 0);
        s.move_selection(-1, true);
        assert_eq!(s.selected_idx, 2, "Up at top with wrap=true wraps to last");
    }

    #[test]
    fn wrap_at_bottom_when_wrap_true() {
        let mut s = state(vec![action("a"), action("b"), action("c")], 2);
        s.move_selection(1, true);
        assert_eq!(
            s.selected_idx, 0,
            "Down at bottom with wrap=true wraps to first"
        );
    }

    #[test]
    fn clamp_at_top_when_wrap_false() {
        let mut s = state(vec![action("a"), action("b"), action("c")], 0);
        s.move_selection(-1, false);
        assert_eq!(s.selected_idx, 0, "Up at top with wrap=false stays put");
    }

    #[test]
    fn clamp_at_bottom_when_wrap_false() {
        let mut s = state(vec![action("a"), action("b"), action("c")], 2);
        s.move_selection(5, false);
        assert_eq!(s.selected_idx, 2, "Down past end with wrap=false clamps");
    }

    #[test]
    fn delta_zero_is_noop() {
        let mut s = state(vec![action("a"), action("b")], 1);
        s.move_selection(0, true);
        assert_eq!(s.selected_idx, 1);
    }

    #[test]
    fn empty_filtered_is_noop() {
        let mut s = state(vec![], 0);
        s.move_selection(3, true);
        assert_eq!(s.selected_idx, 0);
    }

    #[test]
    fn all_headers_no_infinite_loop() {
        // Every entry is non-selectable; advance_one_selectable must bail
        // after `len` iterations rather than spinning forever.
        let mut s = state(vec![header("a"), header("b"), header("c")], 0);
        s.move_selection(1, true);
        assert_eq!(
            s.selected_idx, 0,
            "no selectable target → selection unchanged"
        );
    }

    #[test]
    fn i32_min_does_not_overflow() {
        // Regression: earlier code did `delta.abs() as usize`, which
        // panics for i32::MIN. unsigned_abs() must be used so this is
        // just a (clamped) huge upward step.
        let mut s = state(vec![action("a"), action("b"), action("c")], 2);
        s.move_selection(i32::MIN, false);
        assert_eq!(
            s.selected_idx, 0,
            "huge upward delta clamps to top with wrap=false"
        );
    }

    #[test]
    fn multi_step_skips_repeated_headers() {
        // Three selectable rows with two consecutive headers between the
        // first and second. delta=2 from index 0 must skip both headers
        // and the second selectable, landing on the third (index 4).
        let mut s = state(
            vec![
                action("a"),
                header("h1"),
                header("h2"),
                action("b"),
                action("c"),
            ],
            0,
        );
        s.move_selection(2, true);
        assert_eq!(s.selected_idx, 4);
    }
}
