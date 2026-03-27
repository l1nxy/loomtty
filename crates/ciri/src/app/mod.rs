pub(crate) mod context_menu;
pub(crate) mod event;
pub(crate) mod ime;
pub(crate) mod input_handler;
pub(crate) mod keyboard;
pub(crate) mod mouse;
pub(crate) mod notification;
pub(crate) mod overview;
pub(crate) mod palette;
pub(crate) mod paste_dialog;
pub(crate) mod paste_guard;
pub(crate) mod render;
pub(crate) mod resize;
pub(crate) mod status_bar;
pub(crate) mod sync;
pub(crate) mod top_bar;
pub(crate) mod ui;

use ciri_anim::animation::ViewOffset;
use ciri_anim::manager::{AnimParams, AnimationManager};
use ciri_config::config::{CiriConfig, PaneOpenStyle, StatusBarPosition};
use ciri_gpu::{GlyphAtlasGpu, Renderer};
use ciri_input::keybind::KeybindMap;
use ciri_input::leader::InputHandler;
use ciri_layout::geometry::ViewSize;
use ciri_layout::workspace_set::WorkspaceSet;
use ciri_protocol::message::*;
use ciri_render::glyph_cache::{GlyphCache, GlyphInstance, ScissoredRange};
use ciri_render::rect::Rect;
use ciri_render::shaper::TextShaper;
use ciri_render::terminal::{ColorTable, TerminalView};
use crossbeam_channel::{Receiver, Sender};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::keyboard::ModifiersState;
use winit::window::Window;

use ciri_input::action::Action;

use crate::connection::ServerEvent;
use crate::grid::ClientPaneGrid;
use std::path::PathBuf;

/// A context menu item.
#[derive(Debug, Clone)]
pub(crate) struct ContextMenuItem {
    pub label: String,
    pub action: ContextMenuAction,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum ContextMenuAction {
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
pub(crate) struct ContextMenu {
    pub visible: bool,
    pub x: f32,
    pub y: f32,
    pub target_pane_id: Option<u64>,
    pub items: Vec<ContextMenuItem>,
    pub hovered_index: Option<usize>,
}

/// Text selection state with absolute buffer coordinates.
pub(crate) struct Selection {
    pub pane_id: u64,
    pub start: (u16, usize), // (col, buffer_row)
    pub end: (u16, usize),
    pub active: bool, // true while mouse is held
}

pub(crate) struct LastLeftClick {
    pub pane_id: u64,
    pub col: u16,
    pub buffer_row: usize,
    pub at: Instant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HoveredLink {
    pub pane_id: u64,
    pub url: String,
    pub start: (u16, usize),
    pub end: (u16, usize),
}

/// Active search session state.
pub(crate) struct SearchState {
    pub query: String,
    pub matches: Vec<SearchMatch>,
    pub current_match_idx: usize,
    pub pane_id: u64,
    pub original_scroll_offset: usize,
}

pub(crate) struct SearchMatch {
    pub buffer_row: usize,
    pub start_col: u16,
    pub end_col: u16,
}

/// IME composition state.
pub(crate) struct ImeState {
    pub preedit_active: bool,
    pub preedit_text: String,
    pub preedit_cursor: Option<usize>,
    pub last_pos: Option<(i32, i32)>,
}

/// Command palette state.
pub(crate) struct CommandPaletteState {
    pub query: String,
    pub entries: Vec<PaletteEntry>,
    pub filtered: Vec<usize>, // indices into entries
    pub selected_idx: usize,
    pub hovered_idx: Option<usize>,
    pub sessions_only: bool,
    pub sessions_show_all: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct CommandPaletteLayout {
    pub panel_x: f32,
    pub panel_y: f32,
    pub panel_w: f32,
    pub panel_h: f32,
    pub row_h: f32,
    pub input_row_h: f32,
    pub visible_rows: usize,
    pub entry_count: usize,
    pub text_x: f32,
    pub text_y: f32,
    pub sep_y: f32,
}

#[derive(Clone, Copy)]
pub(crate) struct PaletteToggleLayout {
    pub bg_x: f32,
    pub bg_y: f32,
    pub bg_w: f32,
    pub bg_h: f32,
    pub label_x: f32,
}

pub(crate) struct PaletteEntry {
    pub label: String,
    pub kind: PaletteEntryKind,
}

pub(crate) enum PaletteEntryKind {
    Action(Action),
    SwitchSession(String),
    KillSession(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TopBarHoverRegion {
    Session,
    Workspace,
    Mode,
}

/// Reusable render buffers (cleared each frame).
pub(crate) struct RenderBuffers {
    pub bg_rects: Vec<Rect>,
    pub glyphs: Vec<GlyphInstance>,
    pub color_glyphs: Vec<GlyphInstance>,
    pub glyph_batches: Vec<ScissoredRange>,
    pub color_glyph_batches: Vec<ScissoredRange>,
}

/// Touchpad gesture tracking state.
pub(crate) struct GestureState {
    pub scroll_accum: f64,
    pub row_offset: ViewOffset,
    pub row_active: bool,
    pub row_start: usize,
}

/// Scrollbar drag tracking state.
pub(crate) struct ScrollbarDragInfo {
    pub pane_id: u64,
    pub pane_inner_y: f32,
    pub pane_inner_h: f32,
    pub total_lines: usize,
    pub visible_rows: u16,
}

/// Column/tile border drag resize state.
pub(crate) struct ResizeDragState {
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
pub(crate) enum OverviewActionHover {
    Close,
    Focus,
}

pub(crate) struct OverviewState {
    pub active: bool,
    pub zoom: ViewOffset,
    pub dragging: bool,
    pub drag_last_pos: Option<(f32, f32)>,
    pub hovered_pane: Option<(usize, u64)>,
}


/// Cached pre-transformed glyph instances for a pane tile.
/// Avoids redundant pixel-position computation when content/position haven't changed.
pub(crate) struct CachedTileGlyphs {
    pub generation: u64,
    pub key: (u32, u32, u32, u32), // (inner_x_bits, inner_y_bits, zoom_bits, dim_bits)
    pub glyphs: Vec<GlyphInstance>,
    pub color_glyphs: Vec<GlyphInstance>,
}

/// Client-side image placement for rendering.
#[allow(dead_code)]
pub(crate) struct ClientImagePlacement {
    pub image_id: u64,
    pub col: u16,
    pub row: u16,
    pub width_cells: u16,
    pub height_cells: u16,
    pub pixel_width: u32,
    pub pixel_height: u32,
}

/// Pending paste that needs user confirmation.
#[derive(Debug, Clone)]
pub(crate) struct PendingPaste {
    pub info: paste_guard::PasteInfo,
    pub preview: String,
    pub hovered_button: Option<PasteButton>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PasteButton {
    Paste,
    Cancel,
}

/// Auto-reconnection state.
pub(crate) struct ReconnectState {
    pub attempt: u32,
    pub max_attempts: u32,
    pub next_retry: Instant,
    pub backoff: Duration,
}

pub(crate) struct ReconnectPlan {
    pub viewport: ciri_protocol::codec::ClientHello,
    pub should_exit: bool,
}

pub(crate) struct App {
    pub config: CiriConfig,
    pub session_name: String,
    pub pending_session_name: Option<String>,
    pub frame_interval: Duration,
    pub window: Option<Arc<Window>>,
    pub renderer: Option<Renderer>,
    pub glyph_cache: Option<GlyphCache>,
    pub glyph_atlas_gpu: Option<GlyphAtlasGpu>,
    pub text_shaper: Option<TextShaper>,
    pub dpi_scale: f64,
    pub workspaces: WorkspaceSet,
    pub pane_grids: HashMap<u64, ClientPaneGrid>,
    pub input: InputHandler,
    pub server_tx: Option<Sender<ClientMessage>>,
    pub server_rx: Option<Receiver<ServerEvent>>,
    pub view_offset_x: ViewOffset,
    pub view_offset_y: ViewOffset,
    pub col_widths: Vec<ViewOffset>,
    pub last_frame: Instant,
    pub modifiers: ModifiersState,
    pub cached_views: HashMap<u64, TerminalView>,
    pub last_mouse_pos: Option<(f32, f32)>,
    pub overview: OverviewState,
    pub overview_action_hover: Option<OverviewActionHover>,
    pub render_bufs: RenderBuffers,
    pub ime: ImeState,
    pub overview_keybinds: KeybindMap,
    pub drag: ResizeDragState,
    pub connected: bool,
    pub cursor_blink_visible: bool,
    pub cursor_blink_timer: Instant,
    pub pane_tab_scroll: f32,
    pub hovered_top_bar_region: Option<TopBarHoverRegion>,
    pub hovered_pane_tab: Option<u64>,
    pub workspace_last_pane_ids: HashMap<usize, u64>,
    pub clipboard: Option<arboard::Clipboard>,
    pub selection: Option<Selection>,
    pub last_left_click: Option<LastLeftClick>,
    pub hovered_link: Option<HoveredLink>,
    pub mouse_left_held: bool,
    pub reconnect_state: Option<ReconnectState>,
    pub expected_pane_ids: std::collections::HashSet<u64>,
    pub search_state: Option<SearchState>,
    pub command_palette: Option<CommandPaletteState>,
    pub pending_paste: Option<PendingPaste>,
    pub broadcast_mode: bool,
    pub anim_mgr: AnimationManager,
    /// Inline image placements per pane.
    pub image_placements: HashMap<u64, Vec<ClientImagePlacement>>,
    pub cached_color_table: ColorTable,
    /// Per-pane cached glyph instances to skip redundant transformation in build_tiles.
    pub cached_tile_glyphs: HashMap<u64, CachedTileGlyphs>,
    /// Whether the window currently has input focus.
    pub window_focused: bool,
    pub should_exit: bool,
    #[allow(dead_code)]
    pub config_watcher: Option<notify::RecommendedWatcher>,
    pub config_change_rx: Option<crossbeam_channel::Receiver<()>>,
    pub gestures: GestureState,
    /// Latest pending resize event and its timestamp.
    /// Local layout preview is immediate; PTY/server resize is committed once
    /// after the window size settles.
    pub pending_resize: Option<(winit::dpi::PhysicalSize<u32>, Instant)>,
    /// Deferred DPI change — applied when resize settles to avoid atlas churn.
    pub pending_dpi: Option<f64>,
    /// Remote connection parameters, if connecting via SSH tunnel.
    pub remote_config: Option<RemoteConnectionConfig>,
    /// Last pane focused by focus-follows-mouse and the time it was set (for debouncing).
    pub last_focus_follows_mouse: Option<(u64, Instant)>,
    /// Right-click context menu state.
    pub context_menu: ContextMenu,
}

/// Parameters for a remote SSH tunnel connection.
pub(crate) struct RemoteConnectionConfig {
    pub host: String,
    pub port: u16,
    pub ssh_port: u16,
}

impl App {
    const COMMAND_PALETTE_MIN_WIDTH: f32 = 300.0;
    const COMMAND_PALETTE_EDGE_MARGIN: f32 = 20.0;
    const COMMAND_PALETTE_TOP_RATIO: f32 = 0.15;
    const COMMAND_PALETTE_MAX_HEIGHT_RATIO: f32 = 0.6;
    const COMMAND_PALETTE_INPUT_PAD_X: f32 = 8.0;
    const COMMAND_PALETTE_INPUT_PAD_Y: f32 = 4.0;
    const COMMAND_PALETTE_BOTTOM_PAD: f32 = 4.0;
    const COMMAND_PALETTE_TOGGLE_RIGHT_PAD: f32 = 16.0;
    const COMMAND_PALETTE_TOGGLE_TOP_PAD: f32 = 3.0;
    const COMMAND_PALETTE_TOGGLE_SIDE_PAD: f32 = 4.0;

    pub fn new(config: CiriConfig, session_name: impl Into<String>) -> Self {
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
            &config.input.mode,
            &config.keys.bindings,
            &config.keys.modes,
            &config.keys.direct_bindings,
        );
        let overview_keybinds = KeybindMap::from_overview_config(&config.keys.overview_bindings);
        let column_gap = config.appearance.column_gap;

        let cached_color_table = ColorTable::new(&config);
        App {
            config,
            session_name,
            pending_session_name: None,
            frame_interval,
            window: None,
            renderer: None,
            glyph_cache: None,
            glyph_atlas_gpu: None,
            text_shaper: None,
            dpi_scale: 1.0,
            workspaces: WorkspaceSet::new_with_gaps(initial_view, column_gap, column_gap),
            pane_grids: HashMap::new(),
            input,
            server_tx: None,
            server_rx: None,
            view_offset_x: ViewOffset::new(),
            view_offset_y: ViewOffset::new(),
            col_widths: Vec::new(),
            last_frame: Instant::now(),
            modifiers: ModifiersState::empty(),
            cached_views: HashMap::new(),
            last_mouse_pos: None,
            overview: OverviewState {
                active: false,
                dragging: false,
                drag_last_pos: None,
                hovered_pane: None,
                zoom: {
                    let mut v = ViewOffset::new();
                    v.jump_to(1.0);
                    v
                },
            },
            overview_action_hover: None,
            render_bufs: RenderBuffers {
                bg_rects: Vec::new(),
                glyphs: Vec::new(),
                color_glyphs: Vec::new(),
                glyph_batches: Vec::new(),
                color_glyph_batches: Vec::new(),
            },
            ime: ImeState {
                preedit_active: false,
                preedit_text: String::new(),
                preedit_cursor: None,
                last_pos: None,
            },
            overview_keybinds,
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
            connected: false,
            cursor_blink_visible: true,
            cursor_blink_timer: Instant::now(),
            pane_tab_scroll: 0.0,
            hovered_top_bar_region: None,
            hovered_pane_tab: None,
            workspace_last_pane_ids: HashMap::new(),
            clipboard: arboard::Clipboard::new().ok(),
            selection: None,
            last_left_click: None,
            hovered_link: None,
            mouse_left_held: false,
            reconnect_state: None,
            expected_pane_ids: HashSet::new(),
            search_state: None,
            command_palette: None,
            pending_paste: None,
            broadcast_mode: false,
            anim_mgr: AnimationManager::new(),
            image_placements: HashMap::new(),
            cached_color_table,
            cached_tile_glyphs: HashMap::new(),
            window_focused: true,
            should_exit: false,
            config_watcher: None,
            config_change_rx: None,
            gestures: GestureState {
                scroll_accum: 0.0,
                row_offset: ViewOffset::new(),
                row_active: false,
                row_start: 0,
            },
            pending_resize: None,
            pending_dpi: None,
            remote_config: None,
            last_focus_follows_mouse: None,
            context_menu: ContextMenu::default(),
        }
    }

    /// Connect to the server, either locally or via remote SSH tunnel.
    pub fn connect(
        &self,
        viewport: ciri_protocol::codec::ClientHello,
    ) -> std::io::Result<(Sender<ClientMessage>, Receiver<ServerEvent>)> {
        if let Some(ref rc) = self.remote_config {
            crate::connection::connect_remote(&rc.host, rc.port, rc.ssh_port, viewport)
        } else {
            crate::connection::connect_or_spawn(&self.session_name, viewport)
        }
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

    /// Send a message to the server.
    pub fn send(&self, msg: ClientMessage) {
        if let Some(tx) = &self.server_tx {
            if let Err(e) = tx.send(msg) {
                log::warn!("server channel closed: {e}");
            }
        }
    }

    /// Send a non-critical message (drops if queue full).
    /// Used for: Ack, MouseInput.
    pub fn send_lossy(&self, msg: ClientMessage) {
        if let Some(tx) = &self.server_tx {
            if let Err(e) = tx.try_send(msg) {
                log::debug!("dropped non-critical message: {e}");
            }
        }
    }

    /// Destroy GPU resources (atlas, etc.) before dropping the renderer.
    pub fn destroy_gpu_resources(&mut self) {
        if let (Some(atlas_gpu), Some(renderer)) =
            (self.glyph_atlas_gpu.as_mut(), self.renderer.as_ref())
        {
            renderer.destroy_atlas(atlas_gpu);
        }
        self.glyph_cache = None;
        self.glyph_atlas_gpu = None;
    }

    pub fn cell_dimensions(&self) -> (f32, f32) {
        if let Some(cache) = &self.glyph_cache {
            (cache.cell_width, cache.cell_height)
        } else {
            (8.0, 16.0)
        }
    }

    pub(crate) fn command_palette_viewport_size(&self) -> (f32, f32) {
        self.renderer
            .as_ref()
            .map(|r| {
                let (w, h) = r.surface_size();
                (w as f32, h as f32)
            })
            .unwrap_or((
                self.config.window.width as f32,
                self.config.window.height as f32,
            ))
    }

    pub(crate) fn command_palette_layout(&self) -> Option<CommandPaletteLayout> {
        let palette = self.command_palette.as_ref()?;
        let (_, vh) = self.command_palette_viewport_size();
        let (_, ch) = self.cell_dimensions();
        let (vw, _) = self.command_palette_viewport_size();

        let panel_w = (vw * 0.5)
            .max(Self::COMMAND_PALETTE_MIN_WIDTH)
            .min(vw - Self::COMMAND_PALETTE_EDGE_MARGIN);
        let panel_max_h = vh * Self::COMMAND_PALETTE_MAX_HEIGHT_RATIO;
        let panel_x = (vw - panel_w) / 2.0;
        let panel_y = vh * Self::COMMAND_PALETTE_TOP_RATIO;
        let row_h = ch + 4.0;
        let input_row_h = ch + 8.0;
        let visible_rows = ((panel_max_h - input_row_h) / row_h).floor().max(1.0) as usize;
        let entry_count = palette.filtered.len().min(visible_rows);
        let panel_h = input_row_h + entry_count as f32 * row_h + Self::COMMAND_PALETTE_BOTTOM_PAD;
        let text_x = panel_x + Self::COMMAND_PALETTE_INPUT_PAD_X;
        let text_y = panel_y + Self::COMMAND_PALETTE_INPUT_PAD_Y;
        let sep_y = panel_y + input_row_h;

        Some(CommandPaletteLayout {
            panel_x,
            panel_y,
            panel_w,
            panel_h,
            row_h,
            input_row_h,
            visible_rows,
            entry_count,
            text_x,
            text_y,
            sep_y,
        })
    }

    pub(crate) fn command_palette_toggle_layout(
        &self,
        layout: CommandPaletteLayout,
    ) -> Option<PaletteToggleLayout> {
        let palette = self.command_palette.as_ref()?;
        if !palette.sessions_only {
            return None;
        }

        let (cw, ch) = self.cell_dimensions();
        let active_label_w = " ACTIVE ".len() as f32 * cw;
        let label = if palette.sessions_show_all {
            " ALL "
        } else {
            " ACTIVE "
        };
        let label_w = label.len() as f32 * cw;
        let bg_w = active_label_w + Self::COMMAND_PALETTE_TOGGLE_SIDE_PAD * 2.0;
        let bg_x = layout.panel_x + layout.panel_w - bg_w - Self::COMMAND_PALETTE_TOGGLE_RIGHT_PAD;
        let bg_y = layout.panel_y + Self::COMMAND_PALETTE_TOGGLE_TOP_PAD;
        let label_x =
            bg_x + Self::COMMAND_PALETTE_TOGGLE_SIDE_PAD + (active_label_w - label_w) * 0.5;

        Some(PaletteToggleLayout {
            bg_x,
            bg_y,
            bg_w,
            bg_h: ch + 6.0,
            label_x,
        })
    }

    pub fn compute_grid_size(&self) -> (u16, u16) {
        if let Some(cache) = &self.glyph_cache {
            let pad = self.total_inset();
            let vw = self.workspaces.view_size.width - pad;
            let vh = self.workspaces.view_size.height - pad;
            cache.grid_size(vw, vh)
        } else {
            (80, 24)
        }
    }

    pub fn total_inset(&self) -> f32 {
        (self.config.appearance.padding + self.config.appearance.border_width) * 2.0
    }

    pub(crate) fn anim_params(&self) -> AnimParams {
        use ciri_anim::manager::{CloseStyle, OpenStyle};
        AnimParams {
            omega: self.config.animation.speed,
            epsilon: self.config.animation.epsilon,
            focus_speed: self.config.animation.focus_transition_speed,
            open_style: match self.config.animation.pane_open_style {
                PaneOpenStyle::Fade => OpenStyle::Fade,
                PaneOpenStyle::SlideUp => OpenStyle::SlideUp,
                PaneOpenStyle::SlideDown => OpenStyle::SlideDown,
                PaneOpenStyle::SlideLeft => OpenStyle::SlideLeft,
                PaneOpenStyle::FadeSlideUp => OpenStyle::FadeSlideUp,
            },
            open_duration_secs: self.config.animation.pane_open_duration_ms.max(1) as f64 / 1000.0,
            close_style: CloseStyle::Fade,
            close_duration_secs: self.config.animation.pane_close_duration_ms.max(1) as f64
                / 1000.0,
            bell_duration_secs: 0.15,
            inactive_opacity: self.config.appearance.inactive_opacity,
        }
    }

    pub fn status_bar_height(&self) -> f32 {
        let cell_h = self
            .glyph_cache
            .as_ref()
            .map(|c| c.cell_height)
            .unwrap_or(self.config.font.size * 1.2);
        let padding = if let Some(px) = self.config.statusbar.height_padding {
            px
        } else {
            cell_h * self.config.statusbar.padding_ratio
        };
        cell_h + padding
    }

    /// Height of the bottom hints bar (same size as the status bar).
    pub fn hints_bar_height(&self) -> f32 {
        self.status_bar_height()
    }

    /// Total vertical space occupied by chrome (status bar + hints bar).
    pub fn total_chrome_height(&self) -> f32 {
        self.status_bar_height() + self.hints_bar_height()
    }

    pub fn status_bar_y(&self, window_height: f32) -> f32 {
        match self.config.statusbar.position {
            StatusBarPosition::Top => 0.0,
            StatusBarPosition::Bottom => window_height - self.status_bar_height(),
        }
    }

    /// Y position of the bottom hints bar.
    pub fn hints_bar_y(&self, window_height: f32) -> f32 {
        match self.config.statusbar.position {
            StatusBarPosition::Top => window_height - self.hints_bar_height(),
            StatusBarPosition::Bottom => {
                window_height - self.status_bar_height() - self.hints_bar_height()
            }
        }
    }

    pub fn content_origin_y(&self) -> f32 {
        match self.config.statusbar.position {
            StatusBarPosition::Top => self.status_bar_height(),
            StatusBarPosition::Bottom => 0.0,
        }
    }

    pub fn content_y_from_screen(&self, screen_y: f32) -> Option<f32> {
        match self.config.statusbar.position {
            StatusBarPosition::Top => {
                let y = screen_y - self.status_bar_height();
                (y >= 0.0).then_some(y)
            }
            StatusBarPosition::Bottom => Some(screen_y),
        }
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
        for grid in self.pane_grids.values_mut() {
            grid.dirty = true;
        }
        self.cached_views.clear();
        self.cached_tile_glyphs.clear();
        self.server_tx = None;
        self.server_rx = None;
        self.reconnect_state = Some(ReconnectState {
            attempt: 0,
            max_attempts: 10,
            next_retry: Instant::now() + Duration::from_millis(500),
            backoff: Duration::from_millis(500),
        });
    }

    pub fn prepare_reconnect(&mut self) -> Option<ReconnectPlan> {
        if self.connected || self.server_rx.is_some() {
            return None;
        }

        let should_try = self
            .reconnect_state
            .as_ref()
            .is_some_and(|state| Instant::now() >= state.next_retry);
        let gave_up = self
            .reconnect_state
            .as_ref()
            .is_some_and(|state| state.attempt >= state.max_attempts);
        if gave_up {
            log::error!("max reconnect attempts reached, exiting");
            return Some(ReconnectPlan {
                viewport: self.current_viewport(),
                should_exit: true,
            });
        }
        if !should_try {
            return None;
        }

        if let Some(state) = &mut self.reconnect_state {
            state.attempt += 1;
        }

        Some(ReconnectPlan {
            viewport: self.current_viewport(),
            should_exit: false,
        })
    }

    pub fn finish_reconnect_attempt(
        &mut self,
        result: std::io::Result<(
            crossbeam_channel::Sender<ClientMessage>,
            crossbeam_channel::Receiver<crate::connection::ServerEvent>,
        )>,
    ) {
        match result {
            Ok((tx, rx)) => {
                log::info!("reconnected to session '{}'", self.session_name);
                self.server_tx = Some(tx);
                self.server_rx = Some(rx);
                self.reconnect_state = None;
            }
            Err(e) => {
                log::warn!("reconnect failed: {e}");
                if let Some(state) = &mut self.reconnect_state {
                    state.backoff = (state.backoff * 2).min(Duration::from_secs(10));
                    state.next_retry = Instant::now() + state.backoff;
                }
            }
        }
    }

    pub fn current_viewport(&self) -> ciri_protocol::codec::ClientHello {
        let (cell_width, cell_height) = self.cell_dimensions();
        let view = &self.workspaces.view_size;
        ciri_protocol::codec::ClientHello {
            session_name: self.session_name.clone(),
            width: view.width as u32,
            height: view.height as u32,
            cell_width,
            cell_height,
        }
    }

    fn last_session_path() -> PathBuf {
        ciri_protocol::transport::state_dir().join("last-session")
    }

    /// Invalidate all cached rendering state for a pane (view + glyph cache).
    pub fn invalidate_pane_cache(&mut self, pane_id: u64) {
        self.cached_views.remove(&pane_id);
        self.cached_tile_glyphs.remove(&pane_id);
    }

    /// Update client-side viewport/layout state immediately for interactive window resize.
    /// This keeps the UI visually in sync while deferring the expensive PTY resize.
    pub fn preview_resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        log::debug!("preview_resize: {}x{}", size.width, size.height);
        let chrome_h = self.total_chrome_height();
        self.workspaces.resize_view(ViewSize {
            width: size.width as f32,
            height: size.height as f32 - chrome_h,
        });
        self.snap_all_col_widths();
        let center_strategy = match self.config.layout.center_focused_column {
            ciri_config::config::CenterStrategy::Always => {
                ciri_layout::workspace::CenterStrategy::Always
            }
            ciri_config::config::CenterStrategy::OnOverflow => {
                ciri_layout::workspace::CenterStrategy::OnOverflow
            }
            ciri_config::config::CenterStrategy::Never => {
                ciri_layout::workspace::CenterStrategy::Never
            }
        };
        let current_vox = self.view_offset_x.value() as f32;
        let t = self
            .workspaces
            .active_mut()
            .target_offset_for_active_with_strategy(center_strategy, current_vox);
        self.view_offset_x.jump_to(t as f64);
    }

    /// Apply a deferred resize. Called once per frame from `new_events` so
    /// that multiple `WindowEvent::Resized` events within one frame are
    /// coalesced into a single expensive layout + PTY resize pass.
    pub fn apply_resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        use ciri_protocol::message::ClientMessage;

        log::debug!("apply_resize: {}x{}", size.width, size.height);
        if let Some(renderer) = &mut self.renderer {
            let (surface_w, surface_h) = renderer.surface_size();
            if surface_w != size.width || surface_h != size.height {
                renderer.resize(size.width, size.height);
            }
        }
        self.preview_resize(size);
        for grid in self.pane_grids.values_mut() {
            grid.dirty = true;
        }
        self.cached_views.clear();
        self.cached_tile_glyphs.clear();
        let (cols, rows) = self.compute_grid_size();
        let (cw, ch) = self.cell_dimensions();
        let view = &self.workspaces.view_size;
        log::debug!("  sending Resize: {cols}x{rows} cells, {cw:.1}x{ch:.1} cell_px");
        self.send(ClientMessage::Resize {
            cols,
            rows,
            width: view.width as u32,
            height: view.height as u32,
            cell_width: cw,
            cell_height: ch,
        });
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}
