pub(crate) mod action;
pub(crate) mod context_menu;
pub(crate) mod event;
pub(crate) mod ime;
pub(crate) mod key_encode;
pub(crate) mod keyboard;
pub(crate) mod mouse;
pub(crate) mod notification;
pub(crate) mod open;
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
    ContextMenuItem, GestureState, HoveredLink, OverviewActionHover, PaletteEntryKind, PasteButton,
    PendingPaste, PendingPasteTarget, ReconnectPlan, RemoteConnectionConfig, ResizeDragState,
    ScrollbarDragInfo, SearchMatch, SearchState, Selection, ServerEvent, TopBarHoverRegion,
};
use ciri_layout::geometry::Rect as GeoRect;

/// Cached pre-transformed glyph instances for a pane tile.
/// Avoids redundant pixel-position computation when rows/position haven't changed.
#[derive(Clone, Default)]
pub(crate) struct CachedTileRow {
    pub epoch: u64,
    pub glyphs: Vec<GlyphInstance>,
    pub color_glyphs: Vec<GlyphInstance>,
}

pub(crate) struct CachedTileGlyphs {
    pub key: (u32, u32, u32, u32), // (inner_x_bits, inner_y_bits, zoom_bits, dim_bits)
    pub rows: Vec<CachedTileRow>,
}

#[derive(Clone, Default)]
pub(crate) struct CachedTileBackgroundRow {
    pub epoch: u64,
    pub bg_rects: Vec<Rect>,
}

pub(crate) struct CachedTileBackgrounds {
    pub key: (u32, u32, u32, u32),
    pub rows: Vec<CachedTileBackgroundRow>,
}

#[derive(Clone, Copy)]
pub(crate) struct CommandPaletteLayout {
    pub panel_x: f32,
    pub panel_y: f32,
    pub panel_w: f32,
    pub panel_h: f32,
    pub row_h: f32,
    pub visible_rows: usize,
    pub text_x: f32,
    pub text_y: f32,
    pub sep_y: f32,
}

/// Reusable render buffers (cleared each frame).
pub(crate) struct RenderBuffers {
    pub bg_rects: Vec<Rect>,
    pub glyphs: Vec<GlyphInstance>,
    pub color_glyphs: Vec<GlyphInstance>,
    pub dirty_bg_ranges: Vec<(usize, usize)>,
    pub dirty_glyph_ranges: Vec<(usize, usize)>,
    pub dirty_color_ranges: Vec<(usize, usize)>,
    pub glyph_batches: Vec<ScissoredRange>,
    pub color_glyph_batches: Vec<ScissoredRange>,
    pub active_glyph_batches: Vec<ScissoredRange>,
    pub active_color_glyph_batches: Vec<ScissoredRange>,
    pub pane_order: Vec<u64>,
    pub pane_regions: HashMap<u64, PaneSceneRegion>,
    pub pane_glyph_end: usize,
    pub pane_color_glyph_end: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PaneSceneRegion {
    pub glyph_offset: usize,
    pub glyph_len: usize,
    pub glyph_cap: usize,
    pub color_offset: usize,
    pub color_len: usize,
    pub color_cap: usize,
    pub scissor: (u32, u32, u32, u32),
    pub is_active: bool,
    pub snapshot: u64,
}

impl RenderBuffers {
    pub fn clear_retained_scene(&mut self) {
        self.bg_rects.clear();
        self.glyphs.clear();
        self.color_glyphs.clear();
        self.dirty_bg_ranges.clear();
        self.dirty_glyph_ranges.clear();
        self.dirty_color_ranges.clear();
        self.glyph_batches.clear();
        self.color_glyph_batches.clear();
        self.active_glyph_batches.clear();
        self.active_color_glyph_batches.clear();
        self.pane_order.clear();
        self.pane_regions.clear();
        self.pane_glyph_end = 0;
        self.pane_color_glyph_end = 0;
    }
}

/// Resolved UI-font inputs shared by `FontInitParams` (atlas) and the
/// `UiTextShaper`. Produced by [`App::resolve_ui_font_init`].
pub(crate) struct UiFontInit {
    pub path: Option<(String, u32)>,
    pub id: Option<ciri_render::fontdb::ID>,
    pub pixel_size: Option<f32>,
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
    /// Shaper for UI chrome text (palette, tab bar, status bar, …). Separate
    /// from `text_shaper` so UI can use a proportional font while the
    /// terminal grid stays monospaced. Stored in a `RefCell` so paint paths
    /// that borrow `UiContext` immutably can still drive the LRU cache.
    pub ui_shaper: Option<std::cell::RefCell<ciri_render::ui_shaper::UiTextShaper>>,
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
    pub cached_tile_backgrounds: HashMap<u64, CachedTileBackgrounds>,
    pub image_atlas_entries: HashMap<(u64, u64), GlyphEntry>,
    /// Hash of the last successfully rendered visual state.
    pub last_render_snapshot: Option<u64>,
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

    /// Resolve the UI font path / id / pixel size from `config`. Returns
    /// `path: None` when no `[font.ui]` override is set (UI then shares the
    /// terminal font and uses `FontClass::Primary` in the atlas).
    ///
    /// On Windows the `[font.ui]` override is silently dropped — the DWrite
    /// rasterizer doesn't yet have a per-face glyph_id path for the UI
    /// class, so honoring the override would paint empty glyphs.
    pub(crate) fn resolve_ui_font_init(config: &CiriConfig, dpi_scale: f64) -> UiFontInit {
        #[cfg(windows)]
        if config.font.ui.is_some() {
            log::warn!(
                "[font.ui] override ignored on Windows (DWrite UI-font path \
                 not yet implemented); using terminal font for UI text"
            );
        }
        #[cfg(windows)]
        let ui_override = None::<&ciri_config::schema::UiFontConfig>;
        #[cfg(not(windows))]
        let ui_override = config.font.ui.as_ref();

        let Some(ui_font) = ui_override else {
            return UiFontInit { path: None, id: None, pixel_size: None };
        };

        let (path, id) = match ciri_render::ui_shaper::resolve_ui_font(&ui_font.family) {
            Some((p, idx, fid)) => (Some((p, idx)), Some(fid)),
            None => {
                log::warn!(
                    "UI font '{}' not found, falling back to terminal font",
                    ui_font.family
                );
                (None, None)
            }
        };
        let ui_px = ui_font.size * (96.0 * dpi_scale as f32) / 72.0;
        UiFontInit { path, id, pixel_size: Some(ui_px) }
    }

    /// Build a [`UiTextShaper`] from a resolved [`UiFontInit`]. When `init`
    /// has no path (no override or unknown family), the shaper falls back to
    /// the terminal font so UI text still shapes.
    pub(crate) fn build_ui_shaper(
        init: &UiFontInit,
        terminal_shaper: &TextShaper,
        config_font_size_pt: f32,
        dpi_scale: f64,
        cell_width: f32,
        cell_height: f32,
    ) -> ciri_render::ui_shaper::UiTextShaper {
        let path = init.path.clone().or_else(|| terminal_shaper.primary_font_path());
        let id = init.id.or_else(|| terminal_shaper.primary_font_id());
        let pixel_size = init
            .pixel_size
            .unwrap_or_else(|| config_font_size_pt * (96.0 * dpi_scale as f32) / 72.0);
        ciri_render::ui_shaper::UiTextShaper::new(path, id, pixel_size, cell_width, cell_height)
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
            ui_shaper: None,
            dpi_scale: 1.0,
            modifiers: ModifiersState::empty(),
            cached_views: HashMap::new(),
            last_mouse_pos: None,
            render_bufs: RenderBuffers {
                bg_rects: Vec::new(),
                glyphs: Vec::new(),
                color_glyphs: Vec::new(),
                dirty_bg_ranges: Vec::new(),
                dirty_glyph_ranges: Vec::new(),
                dirty_color_ranges: Vec::new(),
                glyph_batches: Vec::new(),
                color_glyph_batches: Vec::new(),
                active_glyph_batches: Vec::new(),
                active_color_glyph_batches: Vec::new(),
                pane_order: Vec::new(),
                pane_regions: HashMap::new(),
                pane_glyph_end: 0,
                pane_color_glyph_end: 0,
            },
            clipboard: arboard::Clipboard::new().ok(),
            mouse_left_held: false,
            mouse_left_passthrough: false,
            cached_color_table,
            cached_tile_glyphs: HashMap::new(),
            cached_tile_backgrounds: HashMap::new(),
            image_atlas_entries: HashMap::new(),
            last_render_snapshot: None,
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
        // Clean up cached session data for this slot
        self.core.cached_slot_sessions.remove(target_id);

        // Close transient UI state before saving — these are interactive
        // overlays that don't belong to a specific connection.
        if self.core.overview.active {
            self.core.exit_overview();
        }
        self.core.search_state = None;
        self.core.command_palette = None;
        self.core.context_menu = ContextMenu::default();
        self.core.pending_paste = None;

        // Save current state to background
        if let Some(current) = self.save_current_to_slot() {
            self.core
                .background_slots
                .insert(current.id.clone(), current);
        }

        // Restore target
        self.restore_from_slot(target);

        // Reset transient UI/interaction state that doesn't belong to the
        // restored slot — overview, drag, hover, gestures, etc.
        self.core.overview.active = false;
        self.core.overview.hovered_pane = None;
        self.core.overview.dragging = false;
        self.core.overview.drag_last_pos = None;
        self.core.overview_action_hover = None;
        self.core.anim_mgr.overview_zoom.jump_to(1.0);
        self.core.search_state = None;
        self.core.command_palette = None;
        self.core.context_menu = ContextMenu::default();
        self.core.pending_paste = None;
        self.core.drag = ResizeDragState {
            col_dragging: None,
            col_right_idx: None,
            col_start_x: 0.0,
            col_start_width: 0.0,
            col_delta: 0.0,
            tile_dragging: None,
            tile_start_y: 0.0,
            scrollbar_dragging: None,
        };
        self.core.gestures = GestureState {
            scroll_accum: 0.0,
            row_active: false,
            row_start: 0,
        };
        self.core.pane_tab_scroll = 0.0;
        self.core.hovered_top_bar_region = None;
        self.core.hovered_pane_tab = None;
        self.core.last_left_click = None;
        self.core.hovered_link = None;
        self.core.last_focus_follows_mouse = None;
        self.core.cached_local_sessions.clear();
        self.core.cached_remote_probes.clear();
        self.core.prediction.reset();

        // The window may have been resized while this slot was in the background.
        // Use apply_resize (not preview_resize) so we also send the Resize
        // message to the server — otherwise PTY dimensions stay at the slot's
        // save-time size and panels don't reflow.
        if let Some(renderer) = &self.renderer {
            let (w, h) = renderer.surface_size();
            self.apply_resize(winit::dpi::PhysicalSize::new(w, h));
        }

        // Sync window focus state — the server tracks this per-client for
        // DECSET 1004 PTY focus reporting, and it missed any focus changes
        // while this slot was in the background.
        self.core.send(ClientMessage::FocusChange {
            focused: self.window_focused,
        });

        // Pre-fetch session list for the new connection, and refresh all
        // other slot caches so cross-slot cycling has full visibility into
        // every slot's session list (otherwise we'd fall back to "last-active
        // session only" for the slots we're not currently on, and the cycle
        // would skip everything else).
        self.core.send(ClientMessage::ListSessions { all: false });
        self.core.refresh_all_slot_session_caches();

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
                // Record only after connection was successfully initiated.
                self.core.record_recent_host(&host, port, ssh_port);
                crate::recent_hosts::save(&self.core.recent_hosts);
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

    /// Parse a `user@host[:ssh_port]` string and connect.
    /// Uses default remote port (7890) and session name "default".
    pub fn connect_remote_from_input(&mut self, input: &str) {
        let input = input.trim();
        // Parse optional :port suffix (ssh port)
        let (host, ssh_port) = if let Some(colon) = input.rfind(':') {
            if let Ok(port) = input[colon + 1..].parse::<u16>() {
                (&input[..colon], port)
            } else {
                (input, 22)
            }
        } else {
            (input, 22)
        };

        if host.is_empty() {
            return;
        }

        if let Err(reason) = validate_remote_host(host) {
            log::warn!("invalid remote host input: {host:?} — {reason}");
            if let Some(palette) = &mut self.core.command_palette {
                palette.remote_error = Some(("Input".to_string(), reason));
            }
            return;
        }

        let remote_port = 7890;
        self.connect_remote_session(
            host.to_string(),
            remote_port,
            ssh_port,
            "default".to_string(),
        );
    }

    /// Cycle to the next background connection slot.
    /// Build a flat ordered list of `(slot_id, session_name)` across all
    /// connection slots (current + backgrounded). Sessions are first-class
    /// regardless of which connection they live on — local and remote are
    /// equal citizens. Used by `cycle_session`.
    ///
    /// For background slots without a cached session list (palette never
    /// opened to query them), falls back to a single entry with that slot's
    /// last-active session name.
    fn flat_session_list(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();

        let current_slot = self.core.active_slot_id.clone();
        let mut bg_ids: Vec<String> = self.core.background_slots.keys().cloned().collect();
        bg_ids.sort();

        // Current slot — use cached_local_sessions if populated. Sort by name
        // for a stable cycle order: the server's SessionList response is sorted
        // by last_attached, which means the active session bubbles to the
        // front after each switch and the cycle would oscillate between the
        // last two sessions instead of advancing.
        if !self.core.cached_local_sessions.is_empty() {
            let mut names: Vec<String> = self
                .core
                .cached_local_sessions
                .iter()
                .map(|s| s.name.clone())
                .collect();
            names.sort();
            for n in names {
                out.push((current_slot.clone(), n));
            }
        } else {
            out.push((current_slot.clone(), self.core.session_name.clone()));
        }

        // Background slots — use cached_slot_sessions if populated, else the
        // slot's last-active session name as a single fallback entry. Same
        // stable-order treatment.
        for id in &bg_ids {
            if let Some(sessions) = self.core.cached_slot_sessions.get(id)
                && !sessions.is_empty()
            {
                let mut names: Vec<String> =
                    sessions.iter().map(|s| s.name.clone()).collect();
                names.sort();
                for n in names {
                    out.push((id.clone(), n));
                }
            } else if let Some(slot) = self.core.background_slots.get(id) {
                out.push((id.clone(), slot.session_name.clone()));
            }
        }

        out
    }

    /// Cycle to the next (`+1`) or previous (`-1`) session across **all**
    /// connection slots. Local and remote sessions are treated equally —
    /// the cycle visits every session in turn, transparently switching
    /// connections behind the scenes when crossing slot boundaries.
    pub fn cycle_session(&mut self, direction: i32) {
        let list = self.flat_session_list();
        log::debug!(
            "cycle_session(dir={direction}): list={:?}\n  active_slot={} session={} pending={:?}\n  cached_local_sessions={:?}\n  bg_slots={:?}",
            list,
            self.core.active_slot_id,
            self.core.session_name,
            self.core.pending_session_name,
            self.core.cached_local_sessions.iter().map(|s| &s.name).collect::<Vec<_>>(),
            self.core.background_slots.keys().collect::<Vec<_>>(),
        );
        if list.len() < 2 {
            log::debug!("cycle_session: list len < 2, no-op");
            return;
        }
        let current_slot = self.core.active_slot_id.clone();
        // Honor any in-flight session switch the user already requested but
        // the server hasn't synced back yet. Otherwise rapid `i` presses keep
        // computing the cycle position from the stale, pre-switch session_name
        // and re-send SwitchSession to the same target indefinitely.
        let current_session = self
            .core
            .pending_session_name
            .clone()
            .unwrap_or_else(|| self.core.session_name.clone());
        let cur_idx = list
            .iter()
            .position(|(sid, sname)| sid == &current_slot && sname == &current_session)
            .unwrap_or(0);
        let n = list.len() as i32;
        let next_idx = (cur_idx as i32 + direction).rem_euclid(n) as usize;
        let (target_slot, target_session) = list[next_idx].clone();
        if target_slot == current_slot && target_session == current_session {
            return;
        }

        if target_slot != current_slot {
            self.switch_to_slot(&target_slot);
            // After restore, the slot resumes its own last-active session.
            // If the cycle target is a *different* session on that slot, ask
            // the server to switch within it.
            if self.core.session_name != target_session {
                self.core.pending_session_name = Some(target_session.clone());
                self.send(ClientMessage::SwitchSession {
                    session_name: target_session,
                });
            }
        } else {
            // Within the same slot — just switch the session. Mark the target
            // as pending so a subsequent press cycles forward instead of
            // re-asking the server for the same session.
            self.core.pending_session_name = Some(target_session.clone());
            self.send(ClientMessage::SwitchSession {
                session_name: target_session,
            });
        }
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
        // Must match the input row height used by PaletteComponent::paint
        // (`tokens::control_height_md(cell_h)`), plus the 1px separator below it.
        let input_row_h = crate::app::ui::tokens::control_height_md(ch) + 1.0;
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
            visible_rows,
            text_x,
            text_y,
            sep_y,
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

    /// Total horizontal space consumed by the side tab bar, if any.
    /// Matches the `Fixed` size hint used by `TabBarComponent` so that
    /// `Border`'s `left` / `right` slot width equals this value.
    pub fn total_chrome_width(&self) -> f32 {
        match self.core.config.tabbar.position {
            ciri_config::config::TabBarPosition::Integrated => 0.0,
            ciri_config::config::TabBarPosition::Left
            | ciri_config::config::TabBarPosition::Right => self.core.config.tabbar.width,
        }
    }

    pub fn status_bar_y(&self, window_height: f32) -> f32 {
        match self.core.config.statusbar.position {
            StatusBarPosition::Top => 0.0,
            StatusBarPosition::Bottom => window_height - self.status_bar_height(),
        }
    }

    pub fn content_origin_y(&self) -> f32 {
        match self.core.config.statusbar.position {
            StatusBarPosition::Top => self.status_bar_height(),
            StatusBarPosition::Bottom => 0.0,
        }
    }

    /// X origin of the terminal viewport — shifted right by the side
    /// tab bar's width when the tab bar is on the left, zero otherwise.
    pub fn content_origin_x(&self) -> f32 {
        match self.core.config.tabbar.position {
            ciri_config::config::TabBarPosition::Left => self.core.config.tabbar.width,
            _ => 0.0,
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

    /// Map an absolute screen X coordinate into terminal-local X, or
    /// `None` if the point is inside the side tab bar (or to its left/right).
    pub fn content_x_from_screen(&self, screen_x: f32, window_width: f32) -> Option<f32> {
        match self.core.config.tabbar.position {
            ciri_config::config::TabBarPosition::Integrated => Some(screen_x),
            ciri_config::config::TabBarPosition::Left => {
                let x = screen_x - self.core.config.tabbar.width;
                (x >= 0.0).then_some(x)
            }
            ciri_config::config::TabBarPosition::Right => {
                let bar_x = window_width - self.core.config.tabbar.width;
                (screen_x < bar_x).then_some(screen_x)
            }
        }
    }

    /// Rect of the side tab bar, if there is one, given the window size.
    ///
    /// Computed to exactly match what `Border::layout` produces when
    /// `build_ui` builds the chrome tree — i.e. the side bar starts
    /// below any top edge and ends above any bottom edge. Returns
    /// `None` when tabs are integrated into the status bar.
    pub fn side_tab_bar_rect(
        &self,
        window_width: f32,
        window_height: f32,
    ) -> Option<(f32, f32, f32, f32)> {
        use ciri_config::config::TabBarPosition;
        let w = match self.core.config.tabbar.position {
            TabBarPosition::Integrated => return None,
            TabBarPosition::Left | TabBarPosition::Right => self.core.config.tabbar.width,
        };
        // Vertical extent: `Border` resolves top then bottom first, so
        // the side edge starts after the top edge and ends before the
        // bottom edge — exactly matching the `total_chrome_height` split.
        let (y, h) = match self.core.config.statusbar.position {
            StatusBarPosition::Top => {
                let y = self.status_bar_height();
                let h = (window_height - self.total_chrome_height()).max(0.0);
                (y, h)
            }
            StatusBarPosition::Bottom => {
                let h = (window_height - self.total_chrome_height()).max(0.0);
                (0.0, h)
            }
        };
        let x = match self.core.config.tabbar.position {
            TabBarPosition::Left => 0.0,
            TabBarPosition::Right => (window_width - w).max(0.0),
            TabBarPosition::Integrated => unreachable!(),
        };
        Some((x, y, w, h))
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
        self.cached_tile_backgrounds.remove(&pane_id);
    }

    pub fn clear_render_caches(&mut self) {
        self.cached_views.clear();
        self.cached_tile_glyphs.clear();
        self.cached_tile_backgrounds.clear();
        self.image_atlas_entries.clear();
        self.render_bufs.clear_retained_scene();
        self.last_render_snapshot = None;
    }

    /// Update client-side viewport/layout state immediately for interactive window resize.
    /// This keeps the UI visually in sync while deferring the expensive PTY resize.
    pub fn preview_resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        log::debug!("preview_resize: {}x{}", size.width, size.height);
        let chrome_h = self.total_chrome_height();
        let chrome_w = self.total_chrome_width();
        self.core.workspaces.resize_view(ViewSize {
            width: (size.width as f32 - chrome_w).max(0.0),
            height: (size.height as f32 - chrome_h).max(0.0),
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

/// Validate a remote host string (`user@hostname` or `user@ip`).
/// Returns `Ok(())` if the input looks reasonable, or `Err(reason)` with a
/// user-facing error message.
fn validate_remote_host(host: &str) -> Result<(), String> {
    // Must contain exactly one '@' separating user and hostname.
    let Some(at) = host.find('@') else {
        return Err("expected user@host format".to_string());
    };
    let user = &host[..at];
    let hostname = &host[at + 1..];

    if user.is_empty() {
        return Err("username cannot be empty".to_string());
    }
    if hostname.is_empty() {
        return Err("hostname cannot be empty".to_string());
    }

    // Hostname must only contain valid characters (alphanumeric, '.', '-', ':' for IPv6, '_').
    if !hostname
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '_' | '[' | ']'))
    {
        return Err(format!("hostname contains invalid characters: {hostname}"));
    }

    // Hostname shouldn't start or end with '-' or '.'.
    if hostname.starts_with('-') || hostname.starts_with('.') {
        return Err(format!("hostname cannot start with '{}'", &hostname[..1]));
    }

    Ok(())
}

#[cfg(test)]
mod tests_validate_remote_host {
    use super::validate_remote_host;

    #[test]
    fn valid_hosts() {
        assert!(validate_remote_host("user@example.com").is_ok());
        assert!(validate_remote_host("root@192.168.1.1").is_ok());
        assert!(validate_remote_host("deploy@my-server.local").is_ok());
        assert!(validate_remote_host("user@[::1]").is_ok());
    }

    #[test]
    fn missing_at() {
        assert!(validate_remote_host("ffff").is_err());
        assert!(validate_remote_host("just-a-hostname").is_err());
    }

    #[test]
    fn empty_parts() {
        assert!(validate_remote_host("@host").is_err());
        assert!(validate_remote_host("user@").is_err());
        assert!(validate_remote_host("@").is_err());
    }

    #[test]
    fn invalid_hostname_chars() {
        assert!(validate_remote_host("user@host name").is_err());
        assert!(validate_remote_host("user@host/path").is_err());
        assert!(validate_remote_host("user@host;rm -rf").is_err());
    }

    #[test]
    fn hostname_start() {
        assert!(validate_remote_host("user@-bad").is_err());
        assert!(validate_remote_host("user@.bad").is_err());
    }
}
