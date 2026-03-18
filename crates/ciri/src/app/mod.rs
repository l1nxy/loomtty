pub(crate) mod input_handler;
pub(crate) mod render;
pub(crate) mod sync;

use ciri_anim::animation::ViewOffset;
use ciri_config::config::CiriConfig;
use ciri_input::keybind::KeybindMap;
use ciri_input::leader::InputHandler;
use ciri_layout::geometry::ViewSize;
use ciri_layout::workspace_set::WorkspaceSet;
use ciri_protocol::message::*;
use ciri_render::glyph_cache::{GlyphAtlas, GlyphInstance};
use ciri_render::rect::Rect;
use ciri_render::renderer::Renderer;
use ciri_render::terminal::TerminalView;
use crossbeam_channel::{Receiver, Sender};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::keyboard::ModifiersState;
use winit::window::Window;

use crate::connection::ServerEvent;
use crate::grid::ClientPaneGrid;

/// Text selection state with absolute buffer coordinates.
pub(crate) struct Selection {
    pub pane_id: u64,
    pub start: (u16, usize), // (col, buffer_row)
    pub end: (u16, usize),
    pub active: bool, // true while mouse is held
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
    pub frame_interval: Duration,
    pub window: Option<Arc<Window>>,
    pub renderer: Option<Renderer>,
    pub glyph_atlas: Option<GlyphAtlas>,
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
    pub overview_active: bool,
    pub overview_zoom: ViewOffset,
    pub overview_dragging: bool,
    pub drag_last_pos: Option<(f32, f32)>,
    pub bg_rects_buf: Vec<Rect>,
    pub glyph_buf: Vec<GlyphInstance>,
    pub ime_preedit_active: bool,
    pub last_ime_pos: Option<(i32, i32)>,
    pub overview_keybinds: KeybindMap,
    pub resize_dragging: Option<usize>,
    pub resize_drag_start_x: f32,
    pub resize_drag_start_width: f32,
    pub connected: bool,
    pub cursor_blink_visible: bool,
    pub cursor_blink_timer: Instant,
    pub clipboard: Option<arboard::Clipboard>,
    pub selection: Option<Selection>,
    pub mouse_left_held: bool,
    pub reconnect_state: Option<ReconnectState>,
    pub should_exit: bool,
    #[allow(dead_code)]
    pub config_watcher: Option<notify::RecommendedWatcher>,
    pub config_change_rx: Option<crossbeam_channel::Receiver<()>>,
}

impl App {
    pub fn new(config: CiriConfig) -> Self {
        let frame_interval = Duration::from_millis(config.render.frame_interval_ms);
        let initial_view = ViewSize {
            width: config.window.width as f32,
            height: config.window.height as f32,
        };
        let mut input = InputHandler::new(
            Duration::from_millis(config.input.leader_timeout_ms),
            Duration::from_millis(config.input.double_tap_window_ms),
        );
        input.keybinds = KeybindMap::from_config(&config.keys.bindings);
        let leader_str = &config.keys.leader;
        if let Some(rest) = leader_str.strip_prefix("ctrl+") {
            input.leader_ctrl_key = rest.to_string();
        }
        let overview_keybinds = KeybindMap::from_overview_config(&config.keys.overview_bindings);
        let column_gap = config.appearance.column_gap;

        App {
            config,
            frame_interval,
            window: None,
            renderer: None,
            glyph_atlas: None,
            dpi_scale: 1.0,
            workspaces: WorkspaceSet::new_with_gaps(
                initial_view,
                column_gap,
                column_gap,
            ),
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
            overview_active: false,
            overview_dragging: false,
            drag_last_pos: None,
            overview_zoom: {
                let mut v = ViewOffset::new();
                v.jump_to(1.0);
                v
            },
            bg_rects_buf: Vec::new(),
            glyph_buf: Vec::new(),
            ime_preedit_active: false,
            last_ime_pos: None,
            overview_keybinds,
            resize_dragging: None,
            resize_drag_start_x: 0.0,
            resize_drag_start_width: 0.0,
            connected: false,
            cursor_blink_visible: true,
            cursor_blink_timer: Instant::now(),
            clipboard: arboard::Clipboard::new().ok(),
            selection: None,
            mouse_left_held: false,
            reconnect_state: None,
            should_exit: false,
            config_watcher: None,
            config_change_rx: None,
        }
    }

    /// Send a critical message to the server (blocks if queue full).
    /// Used for: Input, Resize, ClosePane, CreatePane, SplitDown, Focus*, MovePane*, SetColumnWidth, SwitchWorkspace.
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

    pub fn cell_dimensions(&self) -> (f32, f32) {
        if let Some(atlas) = &self.glyph_atlas {
            (atlas.cell_width, atlas.cell_height)
        } else {
            (8.0, 16.0)
        }
    }

    pub fn compute_grid_size(&self) -> (u16, u16) {
        if let Some(atlas) = &self.glyph_atlas {
            let pad = self.total_inset();
            let vw = self.workspaces.view_size.width - pad;
            let vh = self.workspaces.view_size.height - pad;
            atlas.grid_size(vw, vh)
        } else {
            (80, 24)
        }
    }

    pub fn total_inset(&self) -> f32 {
        (self.config.appearance.padding + self.config.appearance.border_width) * 2.0
    }

    pub fn status_bar_height(&self) -> f32 {
        let cell_h = self.glyph_atlas.as_ref()
            .map(|a| a.cell_height)
            .unwrap_or(self.config.font.size * 1.2);
        cell_h + self.config.statusbar.height_padding
    }
}
