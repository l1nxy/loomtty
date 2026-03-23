pub(crate) mod event;
pub(crate) mod ime;
pub(crate) mod input_handler;
pub(crate) mod keyboard;
pub(crate) mod mouse;
pub(crate) mod paste_guard;
pub(crate) mod notification;
pub(crate) mod render;
pub(crate) mod status_bar;
pub(crate) mod sync;

use ciri_anim::animation::ViewOffset;
use ciri_config::config::CiriConfig;
use ciri_input::keybind::KeybindMap;
use ciri_input::leader::InputHandler;
use ciri_layout::geometry::Rect as GeoRect;
use ciri_layout::geometry::ViewSize;
use ciri_layout::workspace_set::WorkspaceSet;
use ciri_protocol::message::*;
use ciri_gpu::{GlyphAtlasGpu, Renderer};
use ciri_render::glyph_cache::{GlyphCache, GlyphInstance, ScissoredRange};
use ciri_render::rect::Rect;
use ciri_render::shaper::TextShaper;
use ciri_render::terminal::{ColorTable, TerminalView};
use crossbeam_channel::{Receiver, Sender};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::keyboard::ModifiersState;
use winit::window::Window;

use ciri_input::action::Action;

use crate::connection::ServerEvent;
use crate::grid::ClientPaneGrid;

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
    pub col_start_x: f32,
    pub col_start_width: f32,
    pub col_delta: f64,
    pub tile_dragging: Option<(usize, usize)>,
    pub tile_start_y: f32,
    pub scrollbar_dragging: Option<ScrollbarDragInfo>,
}

/// Overview zoom mode state.
pub(crate) struct OverviewState {
    pub active: bool,
    pub zoom: ViewOffset,
    pub dragging: bool,
    pub drag_last_pos: Option<(f32, f32)>,
}

/// Per-pane animation state (open/close/focus/bell).
pub(crate) struct PaneAnimations {
    pub open_opacity: HashMap<u64, f32>,
    pub open_slides: HashMap<u64, f32>,
    pub focus_opacity: HashMap<u64, ViewOffset>,
    pub prev_focused: Option<u64>,
    pub closing: Vec<ClosingPaneState>,
    pub bell_flash: Option<(u64, Instant)>,
}

/// State for a pane that is being animated out (fade-to-close).
pub(crate) struct ClosingPaneState {
    pub rect: GeoRect,
    pub opacity: f32,
    pub started: Instant,
    pub duration_ms: u64,
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
    pub warning: paste_guard::PasteWarning,
    pub preview: String, // first N chars for display
}

/// Auto-reconnection state.
pub(crate) struct ReconnectState {
    pub attempt: u32,
    pub max_attempts: u32,
    pub next_retry: Instant,
    pub backoff: Duration,
}

pub(crate) struct App {
    pub config: CiriConfig,
    pub session_name: String,
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
    pub render_bufs: RenderBuffers,
    pub ime: ImeState,
    pub overview_keybinds: KeybindMap,
    pub drag: ResizeDragState,
    pub connected: bool,
    pub cursor_blink_visible: bool,
    pub cursor_blink_timer: Instant,
    pub clipboard: Option<arboard::Clipboard>,
    pub selection: Option<Selection>,
    pub last_left_click: Option<LastLeftClick>,
    pub hovered_link: Option<HoveredLink>,
    pub mouse_left_held: bool,
    pub reconnect_state: Option<ReconnectState>,
    pub search_state: Option<SearchState>,
    pub command_palette: Option<CommandPaletteState>,
    pub pending_paste: Option<PendingPaste>,
    pub broadcast_mode: bool,
    pub pane_anims: PaneAnimations,
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
        input.keybinds = KeybindMap::from_config(&config.keys.bindings);
        input.leader_key = ciri_input::leader::LeaderKey::parse(&config.keys.leader);
        input.input_mode = match config.input.mode.as_str() {
            "sticky" => ciri_input::leader::InputMode::Sticky,
            _ => ciri_input::leader::InputMode::Prefix,
        };
        let overview_keybinds = KeybindMap::from_overview_config(&config.keys.overview_bindings);
        let column_gap = config.appearance.column_gap;

        let cached_color_table = ColorTable::new(&config);
        App {
            config,
            session_name,
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
                zoom: {
                    let mut v = ViewOffset::new();
                    v.jump_to(1.0);
                    v
                },
            },
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
            clipboard: arboard::Clipboard::new().ok(),
            selection: None,
            last_left_click: None,
            hovered_link: None,
            mouse_left_held: false,
            reconnect_state: None,
            search_state: None,
            command_palette: None,
            pending_paste: None,
            broadcast_mode: false,
            pane_anims: PaneAnimations {
                open_opacity: HashMap::new(),
                open_slides: HashMap::new(),
                focus_opacity: HashMap::new(),
                prev_focused: None,
                closing: Vec::new(),
                bell_flash: None,
            },
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
            remote_config: None,
            last_focus_follows_mouse: None,
            context_menu: ContextMenu::default(),
        }
    }

    /// Connect to the server, either locally or via remote SSH tunnel.
    pub fn connect(&self, viewport: ciri_protocol::codec::ClientHello) -> std::io::Result<(Sender<ClientMessage>, Receiver<ServerEvent>)> {
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

    /// Invalidate all cached rendering state for a pane (view + glyph cache).
    pub fn invalidate_pane_cache(&mut self, pane_id: u64) {
        self.cached_views.remove(&pane_id);
        self.cached_tile_glyphs.remove(&pane_id);
    }

    /// Update client-side viewport/layout state immediately for interactive window resize.
    /// This keeps the UI visually in sync while deferring the expensive PTY resize.
    pub fn preview_resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        log::debug!("preview_resize: {}x{}", size.width, size.height);
        let bar_h = self.status_bar_height();
        self.workspaces.resize_view(ViewSize {
            width: size.width as f32,
            height: size.height as f32 - bar_h,
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
