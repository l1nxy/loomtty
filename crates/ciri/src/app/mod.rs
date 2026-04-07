pub(crate) mod action;
pub(crate) mod context_menu;
pub(crate) mod event;
pub(crate) mod ime;
pub(crate) mod key_encode;
pub(crate) mod keyboard;
pub(crate) mod mouse;
pub(crate) mod open;
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

use ciri_anim::manager::{AnimConfig, AnimationManager};
use ciri_config::config::{CiriConfig, StatusBarPosition};
use ciri_gpu::{GlyphAtlasGpu, Renderer};
use ciri_layout::geometry::ViewSize;
use ciri_layout::workspace_set::WorkspaceSet;
use ciri_protocol::message::*;
use ciri_render::glyph_cache::{GlyphCache, GlyphEntry, GlyphInstance, ScissoredRange};
use ciri_render::rect::Rect;
use ciri_render::shaper::TextShaper;
use ciri_render::terminal::{ColorTable, TerminalView};
use crossbeam_channel::{Receiver, Sender};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use winit::event_loop::EventLoopProxy;
use winit::keyboard::ModifiersState;
use winit::window::Window;

// Re-export core types so existing `use super::*` in submodules still works.
pub(crate) use ciri_app::app::{
    AppModel, ClientImagePlacement, ConnectionKind, ConnectionSlot, ContextMenu, ContextMenuAction,
    ContextMenuItem, HoveredLink, OverviewActionHover, PaletteEntry, PaletteEntryKind, PasteButton,
    PendingPaste, ReconnectPlan, RemoteConnectionConfig, ScrollbarDragInfo, SearchMatch,
    SearchState, Selection, ServerEvent, TopBarHoverRegion,
};
use ciri_layout::geometry::Rect as GeoRect;

/// Cached pre-transformed glyph instances for a pane tile.
/// Avoids redundant pixel-position computation when content/position haven't changed.
pub(crate) struct CachedTileGlyphs {
    pub generation: u64,
    pub key: (u32, u32, u32, u32), // (inner_x_bits, inner_y_bits, zoom_bits, dim_bits)
    pub glyphs: Vec<GlyphInstance>,
    pub color_glyphs: Vec<GlyphInstance>,
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

/// Reusable render buffers (cleared each frame).
pub(crate) struct RenderBuffers {
    pub bg_rects: Vec<Rect>,
    pub glyphs: Vec<GlyphInstance>,
    pub color_glyphs: Vec<GlyphInstance>,
    pub glyph_batches: Vec<ScissoredRange>,
    pub color_glyph_batches: Vec<ScissoredRange>,
    pub active_glyph_batches: Vec<ScissoredRange>,
    pub active_color_glyph_batches: Vec<ScissoredRange>,
}

pub(crate) struct App {
    /// Core logic state — platform-agnostic.
    pub core: AppModel,

    // --- Shell-only fields (GPU / windowing / platform) ---
    pub window: Option<Arc<Window>>,
    pub renderer: Option<Renderer>,
    pub glyph_cache: Option<GlyphCache>,
    pub glyph_atlas_gpu: Option<GlyphAtlasGpu>,
    pub text_shaper: Option<TextShaper>,
    pub dpi_scale: f64,
    pub modifiers: ModifiersState,
    pub cached_views: HashMap<u64, TerminalView>,
    pub last_mouse_pos: Option<(f32, f32)>,
    pub render_bufs: RenderBuffers,
    pub clipboard: Option<arboard::Clipboard>,
    pub mouse_left_held: bool,
    pub mouse_left_passthrough: bool,
    pub cached_color_table: ColorTable,
    /// Per-pane cached glyph instances to skip redundant transformation in build_tiles.
    pub cached_tile_glyphs: HashMap<u64, CachedTileGlyphs>,
    pub image_atlas_entries: HashMap<(u64, u64), GlyphEntry>,
    /// Whether the window currently has input focus.
    pub window_focused: bool,
    pub config_watcher: Option<notify::RecommendedWatcher>,
    pub config_change_rx: Option<crossbeam_channel::Receiver<()>>,
    /// Latest pending resize event and its timestamp.
    pub pending_resize: Option<(winit::dpi::PhysicalSize<u32>, Instant)>,
    /// Deferred DPI change — applied when resize settles to avoid atlas churn.
    pub pending_dpi: Option<f64>,
    /// Proxy to wake the event loop from background IO threads.
    pub event_loop_proxy: Option<EventLoopProxy<()>>,
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

    fn transformed_tile_rect_for_zoom(
        tile_rect: GeoRect,
        zoom: f32,
        zoom_threshold: f32,
        vw: f32,
        vh: f32,
    ) -> GeoRect {
        if zoom < zoom_threshold {
            let cx = vw / 2.0;
            let cy = vh / 2.0;
            GeoRect::new(
                cx + (tile_rect.x - cx) * zoom,
                cy + (tile_rect.y - cy) * zoom,
                tile_rect.w * zoom,
                tile_rect.h * zoom,
            )
        } else {
            tile_rect
        }
    }

    fn transformed_tile_rect(&self, tile_rect: GeoRect, zoom: f32, vw: f32, vh: f32) -> GeoRect {
        Self::transformed_tile_rect_for_zoom(
            tile_rect,
            zoom,
            self.core.config.animation.zoom_threshold,
            vw,
            vh,
        )
    }

    fn overview_visible_tiles(
        &self,
        zoom: f32,
        view_offset_x: f32,
        view_offset_y: f32,
    ) -> Vec<(u64, GeoRect, bool)> {
        let tiles = self
            .core
            .workspaces
            .all_tiles_2d(view_offset_x, view_offset_y);
        let (vw, vh) = self.command_palette_viewport_size();
        let margin_x = (vw * 0.25).max(96.0);
        let margin_y = (vh * 0.25).max(96.0);
        let clip = GeoRect::new(
            -margin_x,
            -margin_y,
            vw + margin_x * 2.0,
            vh + margin_y * 2.0,
        );
        let zoom_threshold = self.core.config.animation.zoom_threshold;

        tiles
            .into_iter()
            .filter(|(_, rect, _)| {
                Self::transformed_tile_rect_for_zoom(*rect, zoom, zoom_threshold, vw, vh)
                    .intersection(&clip)
                    .is_some()
            })
            .collect()
    }

    pub fn new(config: CiriConfig, session_name: impl Into<String>) -> Self {
        let cached_color_table = ColorTable::new(&config);
        let core = AppModel::new(config, session_name);
        App {
            core,
            window: None,
            renderer: None,
            glyph_cache: None,
            glyph_atlas_gpu: None,
            text_shaper: None,
            dpi_scale: 1.0,
            modifiers: ModifiersState::empty(),
            cached_views: HashMap::new(),
            last_mouse_pos: None,
            render_bufs: RenderBuffers {
                bg_rects: Vec::new(),
                glyphs: Vec::new(),
                color_glyphs: Vec::new(),
                glyph_batches: Vec::new(),
                color_glyph_batches: Vec::new(),
                active_glyph_batches: Vec::new(),
                active_color_glyph_batches: Vec::new(),
            },
            clipboard: arboard::Clipboard::new().ok(),
            mouse_left_held: false,
            mouse_left_passthrough: false,
            cached_color_table,
            cached_tile_glyphs: HashMap::new(),
            image_atlas_entries: HashMap::new(),
            window_focused: true,
            config_watcher: None,
            config_change_rx: None,
            pending_resize: None,
            pending_dpi: None,
            event_loop_proxy: None,
        }
    }

    /// Connect to the server, either locally or via remote SSH tunnel.
    pub fn connect(
        &self,
        viewport: ciri_protocol::codec::ClientHello,
    ) -> std::io::Result<(Sender<ClientMessage>, Receiver<ServerEvent>)> {
        let proxy = self.event_loop_proxy.clone();
        if let Some(ref rc) = self.core.remote_config {
            crate::connection::connect_remote(&rc.host, rc.port, rc.ssh_port, viewport, proxy)
        } else {
            crate::connection::connect_or_spawn(&self.core.session_name, viewport, proxy)
        }
    }

    /// Save the current per-connection state into a ConnectionSlot and reset App fields.
    fn save_current_to_slot(&mut self) -> Option<ConnectionSlot> {
        let server_tx = self.core.server_tx.take()?;
        let server_rx = self.core.server_rx.take()?;

        let kind = if let Some(ref rc) = self.core.remote_config {
            ConnectionKind::Remote {
                host: rc.host.clone(),
                port: rc.port,
                ssh_port: rc.ssh_port,
            }
        } else {
            ConnectionKind::Local
        };

        let initial_view = self.core.workspaces.view_size;
        let column_gap = self.core.config.appearance.column_gap;

        Some(ConnectionSlot {
            id: self.core.active_slot_id.clone(),
            kind,
            session_name: std::mem::take(&mut self.core.session_name),
            server_tx,
            server_rx,
            pane_grids: std::mem::take(&mut self.core.pane_grids),
            workspaces: std::mem::replace(
                &mut self.core.workspaces,
                WorkspaceSet::new_with_gaps(initial_view, column_gap, column_gap),
            ),
            expected_pane_ids: std::mem::take(&mut self.core.expected_pane_ids),
            connected: std::mem::replace(&mut self.core.connected, false),
            reconnect_state: self.core.reconnect_state.take(),
            pending_session_name: self.core.pending_session_name.take(),
            anim_mgr: std::mem::replace(&mut self.core.anim_mgr, AnimationManager::new()),
            workspace_last_pane_ids: std::mem::take(&mut self.core.workspace_last_pane_ids),
            selection: self.core.selection.take(),
            broadcast_mode: std::mem::replace(&mut self.core.broadcast_mode, false),
            image_placements: std::mem::take(&mut self.core.image_placements),
            pending_events: std::mem::take(&mut self.core.buffered_events),
        })
    }

    /// Restore per-connection state from a ConnectionSlot into App fields.
    fn restore_from_slot(&mut self, slot: ConnectionSlot) {
        self.core.active_slot_id = slot.id;
        self.core.session_name = slot.session_name;
        self.core.server_tx = Some(slot.server_tx);
        self.core.server_rx = Some(slot.server_rx);
        self.core.pane_grids = slot.pane_grids;
        self.core.workspaces = slot.workspaces;
        self.core.expected_pane_ids = slot.expected_pane_ids;
        self.core.connected = slot.connected;
        self.core.reconnect_state = slot.reconnect_state;
        self.core.pending_session_name = slot.pending_session_name;
        self.core.anim_mgr = slot.anim_mgr;
        self.core.workspace_last_pane_ids = slot.workspace_last_pane_ids;
        self.core.selection = slot.selection;
        self.core.broadcast_mode = slot.broadcast_mode;
        self.core.image_placements = slot.image_placements;

        // Replay any events that were consumed while the slot was backgrounded
        self.core.buffered_events.extend(slot.pending_events);

        // Set remote_config based on slot kind
        self.core.remote_config = match &slot.kind {
            ConnectionKind::Local => None,
            ConnectionKind::Remote {
                host,
                port,
                ssh_port,
            } => Some(RemoteConnectionConfig {
                host: host.clone(),
                port: *port,
                ssh_port: *ssh_port,
            }),
        };

        // Clear caches — they'll be rebuilt on next render
        self.clear_render_caches();

        // Update window title
        let suffix = match &self.core.remote_config {
            Some(rc) => format!(" (remote: {})", rc.host),
            None => String::new(),
        };
        if let Some(w) = &self.window {
            w.set_title(&format!(
                "{} [{}]{}",
                self.core.config.window.title, self.core.session_name, suffix
            ));
        }
    }

    /// Switch to a background connection slot by ID.
    pub fn switch_to_slot(&mut self, target_id: &str) {
        let target = match self.core.background_slots.remove(target_id) {
            Some(slot) => slot,
            None => {
                log::warn!("no background slot with id: {target_id}");
                return;
            }
        };

        // Save current state to background
        if let Some(current) = self.save_current_to_slot() {
            self.core
                .background_slots
                .insert(current.id.clone(), current);
        }

        // Restore target
        self.restore_from_slot(target);

        // Process any events that accumulated while this slot was in the background
        self.process_server_events();
    }

    /// Create a new remote connection and switch to it.
    pub fn connect_remote_session(
        &mut self,
        host: String,
        port: u16,
        ssh_port: u16,
        session_name: String,
    ) {
        let slot_id = format!("remote:{}:{}", host, port);

        // If a slot already exists for this remote, switch to it instead
        if self.core.background_slots.contains_key(&slot_id) {
            self.switch_to_slot(&slot_id);
            // Once connected, switch session within the remote server
            self.core
                .send(ClientMessage::SwitchSession { session_name });
            return;
        }

        // Save current state to background
        if let Some(current) = self.save_current_to_slot() {
            self.core
                .background_slots
                .insert(current.id.clone(), current);
        }

        // Set up new remote connection
        self.core.remote_config = Some(RemoteConnectionConfig {
            host: host.clone(),
            port,
            ssh_port,
        });
        self.core.session_name = session_name;
        self.core.active_slot_id = slot_id;

        let viewport = self.current_viewport();
        match self.connect(viewport) {
            Ok((tx, rx)) => {
                self.core.server_tx = Some(tx);
                self.core.server_rx = Some(rx);
            }
            Err(e) => {
                log::error!("remote connection failed: {e}");
                self.mark_disconnected_for_reconnect();
            }
        }

        // Update window title
        if let Some(w) = &self.window {
            w.set_title(&format!(
                "{} [{}] (remote: {})",
                self.core.config.window.title, self.core.session_name, host
            ));
        }
    }

    /// Cycle to the next background connection slot.
    /// Order: sort slot IDs lexicographically, pick the one after active_slot_id (wrapping).
    pub fn cycle_next_slot(&mut self) {
        if self.core.background_slots.is_empty() {
            return;
        }
        let mut ids: Vec<String> = self.core.background_slots.keys().cloned().collect();
        ids.sort();
        // Pick the first slot (simplest: just grab the first one in sorted order
        // that differs from current; with only 1 slot this is always it)
        let target = ids
            .iter()
            .find(|id| id.as_str() > self.core.active_slot_id.as_str())
            .unwrap_or(&ids[0])
            .clone();
        self.switch_to_slot(&target);
    }

    /// Delegate: Convert config preset_widths to layout ColumnWidth values.
    pub fn preset_widths(&self) -> Vec<ciri_layout::column::ColumnWidth> {
        self.core.preset_widths()
    }

    /// Delegate: Send a message to the server.
    pub fn send(&self, msg: ClientMessage) {
        self.core.send(msg);
    }

    /// Delegate: Send a non-critical message (drops if queue full).
    pub fn send_lossy(&self, msg: ClientMessage) {
        self.core.send_lossy(msg);
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
                self.core.config.window.width as f32,
                self.core.config.window.height as f32,
            ))
    }

    pub(crate) fn command_palette_layout(&self) -> Option<CommandPaletteLayout> {
        let palette = self.core.command_palette.as_ref()?;
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
        let palette = self.core.command_palette.as_ref()?;
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
            let pad = self.core.total_inset();
            let vw = self.core.workspaces.view_size.width - pad;
            let vh = self.core.workspaces.view_size.height - pad;
            cache.grid_size(vw, vh)
        } else {
            (80, 24)
        }
    }

    /// Delegate: get animation configuration.
    pub(crate) fn anim_config(&self) -> AnimConfig {
        self.core.anim_config()
    }

    pub fn status_bar_height(&self) -> f32 {
        let cell_h = self
            .glyph_cache
            .as_ref()
            .map(|c| c.cell_height)
            .unwrap_or(self.core.config.font.size * 1.2);
        let padding = if let Some(px) = self.core.config.statusbar.height_padding {
            px
        } else {
            cell_h * self.core.config.statusbar.padding_ratio
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
        match self.core.config.statusbar.position {
            StatusBarPosition::Top => 0.0,
            StatusBarPosition::Bottom => window_height - self.status_bar_height(),
        }
    }

    /// Y position of the bottom hints bar.
    pub fn hints_bar_y(&self, window_height: f32) -> f32 {
        match self.core.config.statusbar.position {
            StatusBarPosition::Top => window_height - self.hints_bar_height(),
            StatusBarPosition::Bottom => {
                window_height - self.status_bar_height() - self.hints_bar_height()
            }
        }
    }

    pub fn content_origin_y(&self) -> f32 {
        match self.core.config.statusbar.position {
            StatusBarPosition::Top => self.status_bar_height(),
            StatusBarPosition::Bottom => 0.0,
        }
    }

    pub fn content_y_from_screen(&self, screen_y: f32) -> Option<f32> {
        match self.core.config.statusbar.position {
            StatusBarPosition::Top => {
                let y = screen_y - self.status_bar_height();
                (y >= 0.0).then_some(y)
            }
            StatusBarPosition::Bottom => Some(screen_y),
        }
    }

    /// Delegate: remember workspace pane.
    pub fn remember_workspace_pane(&mut self, workspace_idx: usize, pane_id: u64) {
        self.core.remember_workspace_pane(workspace_idx, pane_id);
    }

    /// Delegate: focus workspace pane local.
    pub fn focus_workspace_pane_local(&mut self, workspace_idx: usize, pane_id: u64) -> bool {
        self.core.focus_workspace_pane_local(workspace_idx, pane_id)
    }

    /// Delegate: write last session.
    pub fn write_last_session(&self) {
        self.core.write_last_session();
    }

    pub fn mark_disconnected_for_reconnect(&mut self) {
        self.core.mark_disconnected_for_reconnect();
        self.clear_render_caches();
    }

    pub fn prepare_reconnect(&mut self) -> Option<ReconnectPlan> {
        use ciri_app::app::ReconnectPlanDecision;
        match self.core.prepare_reconnect_plan()? {
            ReconnectPlanDecision::GaveUp => Some(ReconnectPlan {
                viewport: self.current_viewport(),
                should_exit: true,
            }),
            ReconnectPlanDecision::Try => {
                self.core.bump_reconnect_attempt();
                Some(ReconnectPlan {
                    viewport: self.current_viewport(),
                    should_exit: false,
                })
            }
        }
    }

    pub fn finish_reconnect_attempt(
        &mut self,
        result: std::io::Result<(
            crossbeam_channel::Sender<ClientMessage>,
            crossbeam_channel::Receiver<ServerEvent>,
        )>,
    ) {
        match result {
            Ok((tx, rx)) => self.core.finish_reconnect_ok(tx, rx),
            Err(e) => {
                log::warn!("reconnect failed: {e}");
                self.core.finish_reconnect_err();
            }
        }
    }

    pub fn current_viewport(&self) -> ciri_protocol::codec::ClientHello {
        let (cell_width, cell_height) = self.cell_dimensions();
        let view = &self.core.workspaces.view_size;
        ciri_protocol::codec::ClientHello {
            session_name: self.core.session_name.clone(),
            width: view.width as u32,
            height: view.height as u32,
            cell_width,
            cell_height,
        }
    }

    /// Invalidate all cached rendering state for a pane (view + glyph cache).
    pub fn invalidate_pane_cache(&mut self, pane_id: u64) {
        self.cached_views.remove(&pane_id);
        self.cached_tile_glyphs.remove(&pane_id);
    }

    pub fn clear_render_caches(&mut self) {
        self.cached_views.clear();
        self.cached_tile_glyphs.clear();
        self.image_atlas_entries.clear();
    }

    /// Update client-side viewport/layout state immediately for interactive window resize.
    /// This keeps the UI visually in sync while deferring the expensive PTY resize.
    pub fn preview_resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        log::debug!("preview_resize: {}x{}", size.width, size.height);
        let chrome_h = self.total_chrome_height();
        self.core.workspaces.resize_view(ViewSize {
            width: size.width as f32,
            height: size.height as f32 - chrome_h,
        });
        self.snap_all_col_widths();
        let center_strategy = match self.core.config.layout.center_focused_column {
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
        let current_vox = self.core.anim_mgr.view_offset_x.value() as f32;
        let t = self
            .core
            .workspaces
            .active_mut()
            .target_offset_for_active_with_strategy(center_strategy, current_vox);
        self.core.anim_mgr.view_offset_x.jump_to(t as f64);
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
        for grid in self.core.pane_grids.values_mut() {
            grid.dirty = true;
        }
        self.clear_render_caches();
        let (cols, rows) = self.compute_grid_size();
        let (cw, ch) = self.cell_dimensions();
        let view = &self.core.workspaces.view_size;
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
