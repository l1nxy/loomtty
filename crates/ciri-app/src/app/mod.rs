mod animation;
mod input_helpers;
mod overview;
mod palette;
mod sync;
mod types;

pub use types::*;

use ciri_config::config::CiriConfig;
use ciri_input::leader::InputHandler;
use ciri_layout::geometry::ViewSize;
use ciri_layout::workspace_set::WorkspaceSet;
use ciri_protocol::message::ClientMessage;
use crossbeam_channel::{Receiver, Sender};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

use ciri_anim::manager::AnimationManager;

use crate::grid::ClientPaneGrid;
use crate::prediction::PredictionEngine;

// ---------------------------------------------------------------------------
// AppModel — all pure-logic application state
// ---------------------------------------------------------------------------

pub struct AppModel {
    pub config: CiriConfig,
    pub session_name: String,
    pub pending_session_name: Option<String>,
    pub frame_interval: Duration,

    pub workspaces: WorkspaceSet,
    pub pane_grids: HashMap<u64, ClientPaneGrid>,
    pub expected_pane_ids: HashSet<u64>,

    pub server_tx: Option<Sender<ClientMessage>>,
    pub server_rx: Option<Receiver<ServerEvent>>,

    pub connected: bool,
    pub reconnect_state: Option<ReconnectState>,

    pub input: InputHandler,
    pub anim_mgr: AnimationManager,

    pub overview: OverviewState,
    pub overview_action_hover: Option<OverviewActionHover>,
    pub search_state: Option<SearchState>,
    pub command_palette: Option<CommandPaletteState>,
    pub context_menu: ContextMenu,
    pub ime: ImeState,

    pub selection: Option<Selection>,
    pub pending_paste: Option<PendingPaste>,
    pub broadcast_mode: bool,
    pub should_exit: bool,

    pub last_frame: Instant,
    pub cursor_blink_visible: bool,
    pub cursor_blink_timer: Instant,

    pub drag: ResizeDragState,
    pub gestures: GestureState,

    pub pane_tab_scroll: f32,
    pub hovered_top_bar_region: Option<TopBarHoverRegion>,
    pub hovered_pane_tab: Option<u64>,

    pub workspace_last_pane_ids: HashMap<usize, u64>,
    pub last_left_click: Option<LastLeftClick>,
    pub hovered_link: Option<HoveredLink>,
    pub last_focus_follows_mouse: Option<(u64, Instant)>,

    pub background_slots: HashMap<String, ConnectionSlot>,
    pub active_slot_id: String,

    pub remote_config: Option<RemoteConnectionConfig>,
    pub remote_query_rx: Option<Receiver<RemoteQueryResult>>,

    pub image_placements: HashMap<u64, Vec<ClientImagePlacement>>,
    pub prediction: PredictionEngine,

    /// Events buffered from a restored slot's `pending_events`.
    /// Drained before reading `server_rx` in `process_server_events`.
    pub buffered_events: std::collections::VecDeque<ServerEvent>,
    /// Slot IDs with in-flight `ListSessions` requests for the session palette.
    pub slot_session_pending: std::collections::HashSet<String>,
    /// When the current batch of slot session queries was started.
    pub slot_session_query_start: Option<Instant>,

    /// Cached local sessions from the most recent SessionList response.
    pub cached_local_sessions: Vec<ciri_protocol::message::SessionInfo>,
    /// Cached per-slot sessions from background slot queries.
    /// BTreeMap ensures deterministic iteration order by slot ID.
    pub cached_slot_sessions: BTreeMap<String, Vec<ciri_protocol::message::SessionInfo>>,
    /// Cached remote host probe results, keyed by host address.
    pub cached_remote_probes: BTreeMap<String, RemoteProbeResult>,
    /// Hosts the user has previously connected to. Loaded once at startup,
    /// updated on successful remote connect, persisted by the platform layer.
    pub recent_hosts: Vec<RecentHost>,
}

impl AppModel {
    pub fn new(config: CiriConfig, session_name: impl Into<String>) -> Self {
        let prediction = PredictionEngine::new(
            config.prediction.mode,
            config.prediction.threshold_ms,
            config.prediction.show_underline,
        );
        let frame_interval = Duration::from_millis(config.render.frame_interval_ms);
        let initial_view = ViewSize {
            width: config.window.width as f32,
            height: config.window.height as f32,
        };
        let session_name = session_name.into();
        let mut input = InputHandler::new(
            Duration::from_millis(config.input.leader_timeout_ms),
            Duration::from_millis(config.input.double_tap_window_ms),
        );
        input.reload_bindings(
            &config.keys.leader,
            match config.input.mode {
                ciri_config::config::InputMode::Prefix => "prefix",
                ciri_config::config::InputMode::Sticky => "sticky",
            },
            &config.keys.bindings,
            &config.keys.modes,
            &config.keys.direct_bindings,
        );
        {
            use ciri_input::keybind::{BindingSet, KeybindMap};
            input.set_binding_set(BindingSet::from_legacy(
                &input.keybinds,
                &input.direct_keybinds,
                &input.mode_keybinds,
                &KeybindMap::from_overview_config(&config.keys.overview_bindings),
                &config.keys.search_bindings,
                &config.keys.palette_bindings,
                &config.keys.paste_confirm_bindings,
            ));
        }
        let column_gap = config.appearance.column_gap;

        AppModel {
            config,
            session_name,
            pending_session_name: None,
            frame_interval,
            workspaces: WorkspaceSet::new_with_gaps(initial_view, column_gap, column_gap),
            pane_grids: HashMap::new(),
            expected_pane_ids: HashSet::new(),
            server_tx: None,
            server_rx: None,
            connected: false,
            reconnect_state: None,
            input,
            anim_mgr: AnimationManager::new(),
            overview: OverviewState {
                active: false,
                dragging: false,
                drag_last_pos: None,
                hovered_pane: None,
            },
            overview_action_hover: None,
            search_state: None,
            command_palette: None,
            context_menu: ContextMenu::default(),
            ime: ImeState {
                preedit_active: false,
                preedit_text: String::new(),
                preedit_cursor: None,
                last_pos: None,
            },
            selection: None,
            pending_paste: None,
            broadcast_mode: false,
            should_exit: false,
            last_frame: Instant::now(),
            cursor_blink_visible: true,
            cursor_blink_timer: Instant::now(),
            drag: ResizeDragState {
                col_dragging: None,
                col_right_idx: None,
                col_start_x: 0.0,
                col_start_width: 0.0,
                col_delta: 0.0,
                tile_dragging: None,
                tile_start_y: 0.0,
                scrollbar_dragging: None,
            },
            gestures: GestureState {
                scroll_accum: 0.0,
                row_active: false,
                row_start: 0,
            },
            pane_tab_scroll: 0.0,
            hovered_top_bar_region: None,
            hovered_pane_tab: None,
            workspace_last_pane_ids: HashMap::new(),
            last_left_click: None,
            hovered_link: None,
            last_focus_follows_mouse: None,
            background_slots: HashMap::new(),
            active_slot_id: "local".to_string(),
            remote_config: None,
            remote_query_rx: None,
            image_placements: HashMap::new(),
            prediction,
            buffered_events: std::collections::VecDeque::new(),
            slot_session_pending: std::collections::HashSet::new(),
            slot_session_query_start: None,
            cached_local_sessions: Vec::new(),
            cached_slot_sessions: BTreeMap::new(),
            cached_remote_probes: BTreeMap::new(),
            recent_hosts: Vec::new(),
        }
    }

    /// Send a message to the server.
    pub fn send(&self, msg: ClientMessage) {
        if let Some(tx) = &self.server_tx
            && let Err(e) = tx.send(msg)
        {
            log::warn!("server channel closed: {e}");
        }
    }

    /// Send a non-critical message (drops if queue full).
    /// Used for: Ack, MouseInput.
    pub fn send_lossy(&self, msg: ClientMessage) {
        if let Some(tx) = &self.server_tx
            && let Err(e) = tx.try_send(msg)
        {
            log::debug!("dropped non-critical message: {e}");
        }
    }

    pub fn total_inset(&self) -> f32 {
        (self.config.appearance.padding + self.config.appearance.border_width) * 2.0
    }

    /// Convert config preset_widths to layout ColumnWidth values.
    pub fn preset_widths(&self) -> Vec<ciri_layout::column::ColumnWidth> {
        use ciri_config::config::PresetWidth;
        use ciri_layout::column::ColumnWidth;
        self.config
            .layout
            .preset_widths
            .iter()
            .map(|pw| match pw {
                PresetWidth::Proportion { proportion } => ColumnWidth::Proportion(*proportion),
                PresetWidth::Fixed { fixed } => ColumnWidth::Fixed(*fixed),
            })
            .collect()
    }

    pub fn remember_workspace_pane(&mut self, workspace_idx: usize, pane_id: u64) {
        self.workspace_last_pane_ids.insert(workspace_idx, pane_id);
    }

    pub fn focus_workspace_pane_local(&mut self, workspace_idx: usize, pane_id: u64) -> bool {
        let Some(ws) = self.workspaces.workspaces.get_mut(workspace_idx) else {
            return false;
        };
        for (col_idx, col) in ws.columns.iter_mut().enumerate() {
            if let Some(tile_idx) = col.tiles.iter().position(|t| t.pane_id == pane_id) {
                ws.active_column_idx = col_idx;
                col.active_tile_idx = tile_idx;
                return true;
            }
        }
        false
    }

    pub fn sync_workspace_pane_memory(&mut self) {
        self.workspace_last_pane_ids.clear();
        for (ws_idx, ws) in self.workspaces.workspaces.iter().enumerate() {
            if let Some(pane_id) = ws.active_pane_id() {
                self.workspace_last_pane_ids.insert(ws_idx, pane_id);
            }
        }
    }

    pub fn write_last_session(&self) {
        let path = Self::last_session_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, &self.session_name);
    }

    pub fn mark_disconnected_for_reconnect(&mut self) {
        log::warn!("disconnected from server");
        self.connected = false;
        // Clear session caches — server state is unknown after disconnect
        self.cached_local_sessions.clear();
        self.cached_slot_sessions.clear();
        self.cached_remote_probes.clear();
        for grid in self.pane_grids.values_mut() {
            grid.dirty = true;
        }
        self.server_tx = None;
        self.server_rx = None;
        self.reconnect_state = Some(ReconnectState {
            attempt: 0,
            max_attempts: 10,
            next_retry: Instant::now() + Duration::from_millis(500),
            backoff: Duration::from_millis(500),
        });
    }

    pub fn prepare_reconnect_plan(&self) -> Option<ReconnectPlanDecision> {
        if self.connected || self.server_rx.is_some() {
            return None;
        }

        let gave_up = self
            .reconnect_state
            .as_ref()
            .is_some_and(|state| state.attempt >= state.max_attempts);
        if gave_up {
            log::error!("max reconnect attempts reached, exiting");
            return Some(ReconnectPlanDecision::GaveUp);
        }

        let should_try = self
            .reconnect_state
            .as_ref()
            .is_some_and(|state| Instant::now() >= state.next_retry);
        if !should_try {
            return None;
        }

        Some(ReconnectPlanDecision::Try)
    }

    pub fn bump_reconnect_attempt(&mut self) {
        if let Some(state) = &mut self.reconnect_state {
            state.attempt += 1;
        }
    }

    pub fn finish_reconnect_ok(&mut self, tx: Sender<ClientMessage>, rx: Receiver<ServerEvent>) {
        log::info!("reconnected to session '{}'", self.session_name);
        self.server_tx = Some(tx);
        self.server_rx = Some(rx);
        self.reconnect_state = None;
    }

    pub fn finish_reconnect_err(&mut self) {
        if let Some(state) = &mut self.reconnect_state {
            state.backoff = (state.backoff * 2).min(Duration::from_secs(10));
            state.next_retry = Instant::now() + state.backoff;
        }
    }

    pub fn invalidate_pane_grid(&mut self, pane_id: u64) {
        // Mark grid as dirty — cache invalidation is a shell concern.
        if let Some(grid) = self.pane_grids.get_mut(&pane_id) {
            grid.dirty = true;
        }
    }

    pub fn last_session_path() -> std::path::PathBuf {
        ciri_protocol::transport::state_dir().join("last-session")
    }
}
