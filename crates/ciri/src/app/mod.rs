pub(crate) mod action;
pub(crate) mod ciri_ui_adapter;
pub(crate) mod context_menu;
pub(crate) mod event;
pub(crate) mod ime;
pub(crate) mod key_encode;
pub(crate) mod keyboard;
pub(crate) mod mouse;
pub(crate) mod notification;
pub(crate) mod open;
pub(crate) mod overview;
pub(crate) mod background_image;
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
use ciri_render::glyph_cache::{GlyphCache, GlyphEntry, GlyphInstance, PaneGlyphRange};
use ciri_render::rect::{PaneRectRange, Rect};
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
    ContextMenuItem, GestureState, HoveredLink, PaletteEntryKind, PasteButton, PendingPaste,
    PendingPasteTarget, ReconnectPlan, RemoteConnectionConfig, ResizeDragState, ScrollbarDragInfo,
    SearchMatch, Selection, ServerEvent, TopBarHoverRegion,
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
    pub bg_rect_ranges: Vec<PaneRectRange>,
    pub glyphs: Vec<GlyphInstance>,
    pub color_glyphs: Vec<GlyphInstance>,
    pub dirty_bg_ranges: Vec<(usize, usize)>,
    pub dirty_glyph_ranges: Vec<(usize, usize)>,
    pub dirty_color_ranges: Vec<(usize, usize)>,
    pub glyph_batches: Vec<PaneGlyphRange>,
    pub color_glyph_batches: Vec<PaneGlyphRange>,
    pub active_glyph_batches: Vec<PaneGlyphRange>,
    pub active_color_glyph_batches: Vec<PaneGlyphRange>,
    pub pane_order: Vec<u64>,
    pub pane_regions: HashMap<u64, PaneSceneRegion>,
    pub pane_glyph_end: usize,
    pub pane_color_glyph_end: usize,
    /// Reused per-frame storage for the assembled SDF rect stream
    /// (pane focus rings + cached chrome + transient overlays).
    /// Without this, `assemble_scene` allocated a fresh `Vec` every
    /// frame while every other buffer cycled via `mem::take` — a
    /// small but consistent ~96 KB / frame heap churn at the 1024-rect
    /// cap.
    pub ui_sdf_rects: Vec<ciri_render::sdf_rect::SdfRect>,
}

#[derive(Default)]
pub(crate) struct CachedUiScene {
    pub key: Option<u64>,
    pub glyphs: Vec<GlyphInstance>,
    pub color_glyphs: Vec<GlyphInstance>,
    /// SDF-shader chrome rects emitted by ciri-ui-painted widgets
    /// (rounded / bordered / shadowed).
    pub sdf_rects: Vec<ciri_render::sdf_rect::SdfRect>,
    /// Index into `glyphs` where the Overlay layer begins. Anything
    /// before it is `ChromeLayer::Base`. Set by `frame.paint` after
    /// the base pass completes; Overlay primitives append onto the same
    /// Vec, so `[..base_glyph_end]` is base, `[base_glyph_end..]` is
    /// overlay. The renderer issues two GPU passes against these
    /// ranges so overlay rects cover base glyphs.
    pub base_glyph_end: usize,
    pub base_color_glyph_end: usize,
    pub base_sdf_end: usize,
}

impl CachedUiScene {
    pub fn clear(&mut self) {
        self.key = None;
        self.glyphs.clear();
        self.color_glyphs.clear();
        self.sdf_rects.clear();
        self.base_glyph_end = 0;
        self.base_color_glyph_end = 0;
        self.base_sdf_end = 0;
    }
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
    pub pane_origin: [f32; 2],
    pub pane_size: [f32; 2],
    pub pane_radii: [f32; 4],
    pub is_active: bool,
    pub snapshot: u64,
}

impl RenderBuffers {
    pub fn clear_retained_scene(&mut self) {
        self.bg_rects.clear();
        self.bg_rect_ranges.clear();
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
    /// Resolved theme tokens — pre-parsed once per config change so paint
    /// paths can read colors by token instead of hex-parsing every frame.
    pub cached_resolved_theme: ciri_ui::ResolvedTheme,
    /// Per-pane cached glyph instances to skip redundant transformation in build_tiles.
    pub cached_tile_glyphs: HashMap<u64, CachedTileGlyphs>,
    pub cached_tile_backgrounds: HashMap<u64, CachedTileBackgrounds>,
    pub image_atlas_entries: HashMap<(u64, u64), GlyphEntry>,
    pub cached_ui_scene: CachedUiScene,
    /// Hash of the last successfully rendered visual state.
    pub last_render_snapshot: Option<u64>,
    /// Whether the window currently has input focus.
    pub window_focused: bool,
    pub config_watcher: Option<notify::RecommendedWatcher>,
    pub config_change_rx: Option<crossbeam_channel::Receiver<()>>,
    /// In-flight background-image decode. The worker thread spawned by
    /// `reload_background_image` decodes a (possibly multi-MB) image
    /// off the main thread, then sends the result back here + wakes the
    /// event loop. `apply_pending_background_image` drains it next iteration
    /// and pushes the RGBA bytes to the renderer. Tagged with the path
    /// the decode was started for so a stale result (user changed the
    /// path mid-decode) gets discarded instead of overwriting a newer
    /// upload.
    pub pending_background_image_decode: Option<(
        String,
        crossbeam_channel::Receiver<anyhow::Result<Option<crate::app::background_image::DecodedImage>>>,
    )>,
    /// Latest pending resize event and its timestamp.
    pub pending_resize: Option<(winit::dpi::PhysicalSize<u32>, Instant)>,
    /// Deferred DPI change — applied when resize settles to avoid atlas churn.
    pub pending_dpi: Option<f64>,
    /// Proxy to wake the event loop from background IO threads.
    pub event_loop_proxy: Option<EventLoopProxy<()>>,
    /// Coalesced redraw request latched until the event loop reaches AboutToWait.
    pub pending_redraw: bool,
    /// UI-only ephemeral hover state for overview tiles (workspace index +
    /// pane id under the cursor). Was on `AppModel.overview.hovered_pane`;
    /// moved here so the platform-agnostic core stays free of UI state.
    pub overview_hovered_pane: Option<(usize, u64)>,
    /// `hit_id` captured at the most recent mouse-down on chrome,
    /// retained until mouse-up. Threaded through `UiContext` →
    /// `PaintCtx::active_hit_id` so `.active(|s| ...)` refinements
    /// fire while a button is held on a hit-id'd chrome element.
    /// `None` outside of a press. Sticky on drag — moving the cursor
    /// off the press target keeps the value set; only mouse-up clears
    /// it.
    pub active_hit_id: Option<u64>,
    /// Active pane-resize / column-resize / scrollbar-drag state. UI
    /// input bookkeeping with no model meaning — only mouse handlers in
    /// this crate read or mutate it. Was on `AppModel`; moved out to
    /// keep the platform-agnostic core free of UI input state.
    pub drag: ciri_app::app::ResizeDragState,
    /// Touchpad gesture tracking (per-row swipe + scroll accumulator).
    /// Pure UI input state — only mouse-wheel / pan handlers in this
    /// crate touch it. Was on `AppModel`.
    pub gestures: ciri_app::app::GestureState,
    /// Whether the terminal cursor is currently in its visible blink
    /// half. Pure UI animation state — toggled by the event loop tick
    /// (`event.rs`) and reset on key input (`keyboard.rs`); read by
    /// the renderer to gate the cursor primitive. Was on `AppModel`.
    pub cursor_blink_visible: bool,
    /// Timestamp of the last cursor-blink toggle. Pairs with
    /// `cursor_blink_visible` to drive the blink interval. UI-only.
    pub cursor_blink_timer: Instant,
    /// Most recent focus-follows-mouse trigger: `(pane_id,
    /// timestamp_of_switch)`. The mouse handler reads this to debounce
    /// repeated switches inside the configured cooldown window. Pure
    /// input gesture state, no model meaning. Was on `AppModel`.
    pub last_focus_follows_mouse: Option<(u64, Instant)>,
    /// Timestamp of the previous frame, used to compute `dt` for the
    /// animation tick at the top of `render()`. Pure render-loop
    /// bookkeeping with no model meaning. Was on `AppModel`.
    pub last_frame: Instant,
    /// Horizontal scroll offset (px) for the inline pane-tab strip in
    /// the top bar. Pure UI ephemera — readers/writers all live in this
    /// crate: mouse wheel handler (mouse.rs), top-bar visibility helper
    /// (top_bar.rs::ensure_active_pane_tab_visible — re-clamps to the
    /// current workspace's max each paint), top-bar paint
    /// (ui/top_bar/mod.rs), and the chrome cache hash (render.rs).
    ///
    /// Note: workspace switches don't explicitly reset this — same
    /// behaviour as when the field lived on `AppModel`. The next paint
    /// re-clamps to the new workspace's max scroll, which is enough to
    /// keep the strip visually correct, though scroll position can
    /// "carry over" between workspaces with similar tab counts.
    pub pane_tab_scroll: f32,
    /// Notify handle shared with the active connection's IO thread so the UI
    /// can trigger `Cancelled` mid-connect. Exists only while the active slot
    /// is still in a transient `!connected` state — slot switches drop it.
    pub connection_cancel: Option<Arc<tokio::sync::Notify>>,

    /// Redraw-gating ticker for the new `ciri-ui` animation layer.
    ///
    /// `ciri-motion::AnimProp` instances call `wake()` / `sleep()` on
    /// this shared counter as they transition, so `advance_animations`
    /// can OR `motion_ticker.is_animating()` with `AnimationManager`'s
    /// bool to decide whether to schedule another frame. Distinct from
    /// `core.anim_mgr` (which animates pane compositor state); the two
    /// tickers coexist while widgets migrate over one at a time.
    pub motion_ticker: Arc<ciri_motion::Ticker>,

    /// Persistent Taffy layout tree shared by every `ciri-ui` paint /
    /// hit-test call this frame. The tree is `clear()`ed and rebuilt
    /// each call but its SlotMap allocator is preserved, so layout
    /// nodes don't bounce through the system allocator on every chrome
    /// paint. See `ciri_ui::paint_tree_into_with` for the mechanism.
    pub ui_taffy_tree: std::cell::RefCell<crate::app::ui::types::UiTaffyTree>,

    /// Bump arena for ciri-ui Element trees. Cleared at the start of
    /// every chrome / transient paint pass so the per-frame element
    /// tree (every `Div`, `Text`, etc.) allocates by pointer-bump
    /// instead of `Box::new`. The bin crate publishes this arena via
    /// `ElementArenaScope` around each paint call so `AnyElement::new`
    /// inside `Div::child` lands here. See `ciri_ui::arena`.
    pub ui_arena: std::cell::RefCell<ciri_ui::Arena>,

    /// Cross-frame state for stateful UI elements (scroll offsets,
    /// virtual list anchors, hover timers). Keyed by
    /// `(ElementId, TypeId)` and queried via `cx.states.use_state(id)`
    /// inside `Element::paint`. Survives across frames as long as the
    /// element's id remains stable; entries can be flushed via
    /// `ElementStates::clear_id` when an element is permanently gone.
    pub ui_states: std::cell::RefCell<ciri_ui::ElementStates>,
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

    /// Resolve the UI font path / id / pixel size from `config`.
    pub(crate) fn resolve_ui_font_init(config: &CiriConfig, dpi_scale: f64) -> UiFontInit {
        let ui_override = config.font.ui.as_ref();

        // Resolve the family name. When no `[font.ui]` is configured, use
        // the system default sans-serif (Segoe UI on Windows, system sans
        // on Linux, SF Pro on macOS) so UI chrome gets a proportional font.
        let family = ui_override.map(|u| u.family.as_str()).unwrap_or("");
        let size_pt = ui_override.map(|u| u.size).unwrap_or(config.font.size);

        let (path, id) = match ciri_render::ui_shaper::resolve_ui_font(family) {
            Some((p, idx, fid)) => {
                log::info!(
                    "UI font resolved: '{}' → {}",
                    if family.is_empty() {
                        "(system sans-serif)"
                    } else {
                        family
                    },
                    p
                );
                (Some((p, idx)), Some(fid))
            }
            None => {
                if !family.is_empty() {
                    log::warn!(
                        "UI font '{}' not found, falling back to terminal font",
                        family
                    );
                }
                (None, None)
            }
        };
        // Round to integer ppem so DWrite's NATURAL_SYMMETRIC hinting
        // grid lands on whole pixels — fractional ppem (e.g. 10pt → 13.33px)
        // makes proportional UI glyphs render as if hinting were disabled,
        // softening edges of W/M and similar dense-stroke characters.
        let ui_px = (size_pt * (96.0 * dpi_scale as f32) / 72.0).round();
        UiFontInit {
            path,
            id,
            pixel_size: Some(ui_px),
        }
    }

    /// Build a [`UiTextShaper`] from a resolved [`UiFontInit`]. When `init`
    /// has no path (no override or unknown family), the shaper falls back to
    /// the terminal font so UI text still shapes.
    ///
    /// CJK/emoji fallback fonts and the font resolver are always taken from
    /// the terminal shaper — they're the same system fonts regardless of
    /// whether the UI uses a proportional or the terminal's monospaced font.
    pub(crate) fn build_ui_shaper(
        init: &UiFontInit,
        terminal_shaper: &TextShaper,
        config_font_size_pt: f32,
        dpi_scale: f64,
        cell_width: f32,
        cell_height: f32,
    ) -> ciri_render::ui_shaper::UiTextShaper {
        use ciri_render::ui_shaper::UiFontData;

        let id = init.id.or_else(|| terminal_shaper.primary_font_id());

        // Prefer shared font data from the terminal shaper (avoids re-reading
        // multi-MB font files from disk). Fall back to file path for UI font
        // overrides whose data isn't in the terminal shaper.
        let primary = if init.path.is_some() {
            // UI font override — may differ from terminal font.
            // Try to share data if it's the terminal font, otherwise read path.
            id.and_then(|fid| terminal_shaper.font_data_arc(fid))
                .map(|(data, idx)| UiFontData::Shared(data, idx))
                .or_else(|| init.path.clone().map(|(p, i)| UiFontData::Path(p, i)))
        } else {
            // No override — use terminal primary font data.
            id.and_then(|fid| terminal_shaper.font_data_arc(fid))
                .map(|(data, idx)| UiFontData::Shared(data, idx))
        };

        let cjk = terminal_shaper
            .cjk_font_id()
            .and_then(|fid| terminal_shaper.font_data_arc(fid))
            .map(|(data, idx)| UiFontData::Shared(data, idx));

        let emoji = terminal_shaper
            .emoji_font_id()
            .and_then(|fid| terminal_shaper.font_data_arc(fid))
            .map(|(data, idx)| UiFontData::Shared(data, idx));

        let pixel_size = init
            .pixel_size
            .unwrap_or_else(|| config_font_size_pt * (96.0 * dpi_scale as f32) / 72.0);
        // Terminal primary font as last-resort fallback — covers Braille,
        // box drawing, Nerd Font icons that the proportional UI font lacks.
        let terminal_primary = terminal_shaper
            .primary_font_id()
            .and_then(|fid| terminal_shaper.font_data_arc(fid))
            .map(|(data, idx)| UiFontData::Shared(data, idx));

        ciri_render::ui_shaper::UiTextShaper::new(ciri_render::ui_shaper::UiShaperParams {
            primary,
            primary_id: id,
            terminal_primary,
            terminal_primary_id: terminal_shaper.primary_font_id(),
            cjk,
            cjk_id: terminal_shaper.cjk_font_id(),
            emoji,
            emoji_id: terminal_shaper.emoji_font_id(),
            resolver: Some(terminal_shaper.font_resolver()),
            pixel_size,
            fallback_advance: cell_width,
            fallback_line_height: cell_height,
        })
    }

    pub fn new(config: CiriConfig, session_name: impl Into<String>) -> Self {
        let cached_color_table = ColorTable::new(&config);
        let cached_resolved_theme = ciri_ui::ResolvedTheme::from_config(&config.theme);
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
                bg_rect_ranges: Vec::new(),
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
                ui_sdf_rects: Vec::new(),
            },
            clipboard: arboard::Clipboard::new().ok(),
            mouse_left_held: false,
            mouse_left_passthrough: false,
            cached_color_table,
            cached_resolved_theme,
            cached_tile_glyphs: HashMap::new(),
            cached_tile_backgrounds: HashMap::new(),
            image_atlas_entries: HashMap::new(),
            cached_ui_scene: CachedUiScene::default(),
            last_render_snapshot: None,
            window_focused: true,
            config_watcher: None,
            config_change_rx: None,
            pending_background_image_decode: None,
            pending_resize: None,
            pending_dpi: None,
            event_loop_proxy: None,
            pending_redraw: false,
            overview_hovered_pane: None,
            active_hit_id: None,
            pane_tab_scroll: 0.0,
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
            gestures: ciri_app::app::GestureState {
                scroll_accum: 0.0,
                row_active: false,
                row_start: 0,
            },
            cursor_blink_visible: true,
            cursor_blink_timer: Instant::now(),
            last_focus_follows_mouse: None,
            last_frame: Instant::now(),
            connection_cancel: None,
            motion_ticker: Arc::new(ciri_motion::Ticker::new()),
            ui_taffy_tree: std::cell::RefCell::new(taffy::TaffyTree::new()),
            // 256 KB initial chunk — enough for a typical chrome paint
            // (~150 elements × ~100 bytes each fits in well under that),
            // grows automatically if a frame outsizes it.
            ui_arena: std::cell::RefCell::new(ciri_ui::Arena::new(256 * 1024)),
            ui_states: std::cell::RefCell::new(ciri_ui::ElementStates::new()),
        }
    }

    /// Connect to the server, either locally or via remote SSH tunnel.
    ///
    /// Returns the message/event channels and a `Notify` handle the caller
    /// should stash in `App::connection_cancel`. Signalling that handle tears
    /// the tunnel down and surfaces `DisconnectReason::Cancelled`.
    pub fn connect(
        &self,
        viewport: ciri_protocol::codec::ClientHello,
    ) -> std::io::Result<(
        Sender<ClientMessage>,
        Receiver<ServerEvent>,
        Arc<tokio::sync::Notify>,
    )> {
        let proxy = self.event_loop_proxy.clone();
        let cancel = Arc::new(tokio::sync::Notify::new());
        let (tx, rx) = if let Some(ref rc) = self.core.remote_config {
            crate::connection::connect_remote(
                &rc.host,
                rc.port,
                rc.ssh_port,
                viewport,
                proxy,
                cancel.clone(),
            )?
        } else {
            crate::connection::connect_or_spawn(
                &self.core.session_name,
                viewport,
                proxy,
                cancel.clone(),
            )?
        };
        Ok((tx, rx, cancel))
    }

    /// Save the current per-connection state into a ConnectionSlot and reset App fields.
    fn save_current_to_slot(&mut self) -> Option<ConnectionSlot> {
        let server_tx = self.core.server_tx.take()?;
        let server_rx = self.core.server_rx.take()?;
        // Cancel handle is tied to the active slot only — if the user
        // backgrounds a still-connecting slot, they can't Esc-cancel it from
        // the background. Dropping the handle is safe because the IO thread
        // keeps its own clone.
        self.connection_cancel = None;

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
        // Same rationale as finalize_authoritative_session_switch: the
        // incoming anim_mgr's col_widths reflect the restored slot's own
        // layout, but switching slots is visually a full re-layout from
        // the user's perspective — trigger the same equalize animation.
        self.core.anim_mgr.col_widths_equalize_pending = true;
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
        // Restore the active pane's pre-search scroll BEFORE we save
        // the slot — `close_search_restore_scroll` mutates the live
        // pane grid in-place. Bare `search_state = None` would freeze
        // the slot's saved scroll at the last match position with no
        // way to recover when the slot is restored. Same shape as the
        // R12 `finalize_authoritative_session_switch` fix.
        self.close_search_restore_scroll();
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
        self.overview_hovered_pane = None;
        self.core.overview.dragging = false;
        self.core.overview.drag_last_pos = None;
        self.core.anim_mgr.overview_zoom.jump_to(1.0);
        // If the saved slot had a live search session, restore its
        // pre-search scroll on the just-loaded pane grid before
        // clearing — same shape as the pre-save restore above.
        self.close_search_restore_scroll();
        self.core.command_palette = None;
        self.core.context_menu = ContextMenu::default();
        self.core.pending_paste = None;
        self.drag = ResizeDragState {
            col_dragging: None,
            col_right_idx: None,
            col_start_x: 0.0,
            col_start_width: 0.0,
            col_delta: 0.0,
            tile_dragging: None,
            tile_start_y: 0.0,
            scrollbar_dragging: None,
        };
        self.gestures = GestureState {
            scroll_accum: 0.0,
            row_active: false,
            row_start: 0,
        };
        self.pane_tab_scroll = 0.0;
        self.core.last_left_click = None;
        self.core.hovered_link = None;
        self.last_focus_follows_mouse = None;
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
        // Defence-in-depth: every upstream caller is supposed to have
        // validated already, but bypassing this gate would reach `ssh` with
        // untrusted bytes, so we re-check before spawning.
        if let Err(e) = crate::remote_validate::validate_fields(&host, port, ssh_port) {
            log::error!("refusing to connect to {host:?}: {e}");
            if let Some(palette) = &mut self.core.command_palette {
                palette.remote_error = Some(("Remote".to_string(), e.to_string()));
            }
            return;
        }

        let slot_id = format!("remote:{}:{}", host, port);

        // Fresh attempt starts with no stale error hanging over the UI.
        self.core.last_disconnect_reason = None;

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
            Ok((tx, rx, cancel)) => {
                self.core.server_tx = Some(tx);
                self.core.server_rx = Some(rx);
                self.connection_cancel = Some(cancel);
                // Record only after connection was successfully initiated.
                // session_name was moved into self.core above, so read it back.
                let attached = self.core.session_name.clone();
                self.core
                    .record_recent_host(&host, port, ssh_port, &attached);
                crate::recent_hosts::save(&self.core.recent_hosts);
            }
            Err(e) => {
                log::error!("remote connection failed: {e}");
                // The IO error text from `connect()` already carries the
                // validation / spawn detail; preserve it as the reason so the
                // banner can show it instead of a generic "disconnected".
                let reason = if e.kind() == std::io::ErrorKind::InvalidInput {
                    ciri_app::app::DisconnectReason::InvalidTarget(e.to_string())
                } else if e.kind() == std::io::ErrorKind::NotFound {
                    ciri_app::app::DisconnectReason::SshNotFound
                } else {
                    ciri_app::app::DisconnectReason::SshSpawnFailed(e.to_string())
                };
                self.mark_disconnected_for_reconnect(reason);
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

    /// Parse a `[user@]host[:ssh_port]` string and trigger the auto-connect
    /// flow: fire an async session query, then attach to whichever session
    /// `pick_auto_connect_session` picks (preferring last-remembered, then
    /// most-recent, then `"default"`). The palette stays open in loading
    /// state until the query result arrives.
    pub fn connect_remote_from_input(&mut self, input: &str) {
        let target = match crate::remote_validate::parse_input(input, 22) {
            Ok(t) => t,
            Err(e) => {
                log::warn!("invalid remote host input: {input:?} — {e}");
                if let Some(palette) = &mut self.core.command_palette {
                    palette.remote_error = Some(("Input".to_string(), e.to_string()));
                }
                return;
            }
        };

        // Preserve the user@ prefix when the user supplied one, otherwise pass
        // the bare host to ssh (it will use the current OS username).
        let host_arg = match target.user {
            Some(u) => format!("{u}@{}", target.host),
            None => target.host,
        };
        let port = ciri_protocol::transport::DEFAULT_REMOTE_PORT;
        self.start_remote_auto_connect(host_arg, port, target.ssh_port);
    }

    /// Common entry for `DirectConnect` palette rows and text-prompt input.
    /// Sets `pending_auto_connect`, fires `query_remote_sessions`, and
    /// flips the palette into loading state. The result handler picks
    /// the right session and calls `connect_remote_session`.
    pub fn start_remote_auto_connect(&mut self, host: String, port: u16, ssh_port: u16) {
        let preferred = self.core.last_session_for(&host, port);
        if let Some(palette) = &mut self.core.command_palette {
            palette.remote_loading = Some(host.clone());
            palette.remote_error = None;
        }
        let (tx, rx) = crossbeam_channel::bounded(1);
        // host doubles as the display name here — the prompt has no other label.
        crate::connection::query_remote_sessions(&host, &host, port, ssh_port, tx);
        self.core.remote_query_rx = Some(rx);
        self.core.pending_auto_connect = Some(ciri_app::app::PendingAutoConnect {
            host,
            port,
            ssh_port,
            preferred_session: preferred,
        });
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
                let mut names: Vec<String> = sessions.iter().map(|s| s.name.clone()).collect();
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
            self.core
                .cached_local_sessions
                .iter()
                .map(|s| &s.name)
                .collect::<Vec<_>>(),
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
        let ui_line_h = self
            .ui_shaper
            .as_ref()
            .map(|s| s.borrow().line_height())
            .unwrap_or(ch);
        let row_h = ui_line_h + 4.0;
        // Must match the input row height used by PaletteComponent::paint
        // (`tokens::control_height_md(ui_line_h)`), plus the 1px separator below it.
        let input_row_h = crate::app::ui::tokens::control_height_md(ui_line_h) + 1.0;
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

    /// Which command-palette filtered-row index the cursor currently sits on,
    /// derived from `last_mouse_pos` + the live palette layout. `None` when
    /// the palette is closed, the cursor is outside the row strip, or the
    /// hit row is non-selectable (a section header). Used by the chrome
    /// cache hash so hover-state changes invalidate the cache without
    /// requiring a stored field on `AppModel` that needs reset bookkeeping
    /// at every palette-close site.
    pub(crate) fn current_palette_hover(&self) -> Option<usize> {
        let palette = self.core.command_palette.as_ref()?;
        let layout = self.command_palette_layout()?;
        let (mx, my) = self.last_mouse_pos?;
        if mx < layout.panel_x || mx >= layout.panel_x + layout.panel_w {
            return None;
        }
        if my < layout.sep_y {
            return None;
        }
        let row_strip_offset = my - layout.sep_y;
        if row_strip_offset < 0.0 {
            return None;
        }
        let row_idx_in_visible = (row_strip_offset / layout.row_h).floor() as usize;
        if row_idx_in_visible >= layout.visible_rows {
            return None;
        }
        let scroll = self.command_palette_scroll_offset(layout.visible_rows);
        let row_idx = scroll + row_idx_in_visible;
        if row_idx >= palette.filtered.len() {
            return None;
        }
        let entry_idx = palette.filtered[row_idx];
        if !palette.entries[entry_idx].kind.is_selectable() {
            return None;
        }
        Some(row_idx)
    }

    /// Which context-menu row the cursor currently sits on. Returns
    /// `None` when the menu is hidden, the cursor is outside the menu
    /// rect, or the row is disabled. Used by the chrome cache hash so a
    /// row-boundary crossing invalidates the cache without a stored
    /// field.
    ///
    /// Routed through `ContextMenuComponent::capture` + `hover_index`
    /// so cache invalidation, paint, and hit-test all share one layout
    /// snapshot. This matters after Step 42: capture stores raw click
    /// coordinates and `anchored()` may edge-flip the menu at paint
    /// time when the click lands near the bottom or right viewport
    /// edge, so the pre-flip clamp this function used to do would
    /// hash against the wrong rectangle and leave stale hover.
    pub(crate) fn current_context_menu_hover(&self) -> Option<usize> {
        if !self.core.context_menu.visible {
            return None;
        }
        let (mx, my) = self.last_mouse_pos?;
        let cx = self.ui_context();
        let component = crate::app::ui::context_menu::ContextMenuComponent::capture(self, &cx)?;
        component.hover_index(mx, my, &cx)
    }

    /// Hit-id under the cursor for the settings panel, used by the
    /// chrome cache hash so cursor moves between interactive elements
    /// (close button / dropdown trigger / opacity steppers / "Open
    /// settings.toml" link) invalidate the cache and the declarative
    /// `.hover()` styles repaint each frame.
    ///
    /// Mirrors `current_context_menu_hover` /
    /// `current_paste_dialog_hover` / `current_palette_hover`. Returns
    /// the raw u64 hit_id rather than a typed enum because settings
    /// hits are flat (no per-item-index payload to thread through).
    /// `active_hit_id` projected through the same overlay gate as
    /// `current_settings_hover`. When an Overlay-tier popup is visible
    /// the Base layer's press tint must not light up — same shape as
    /// the hover suppression. Used by the cache hash and by the paint
    /// context so both are in lockstep.
    pub(crate) fn effective_active_hit_id(&self) -> Option<u64> {
        if self.core.context_menu.visible || self.core.command_palette.is_some() {
            return None;
        }
        self.active_hit_id
    }

    pub(crate) fn current_settings_hover(&self) -> Option<u64> {
        if !self.core.settings_panel_visible {
            return None;
        }
        // Suppress base-layer hover when an Overlay-layer popup is
        // visible: the popup occludes the panel, so styling a
        // settings element as hovered while the cursor is actually
        // over a popup row produces a phantom highlight underneath
        // the popup. Same gate also kills cache-thrashing — without
        // it every cursor move over the overlap area mutates the
        // hover hit_id and invalidates the chrome cache, causing a
        // full repaint per pointer-move event.
        if self.core.context_menu.visible || self.core.command_palette.is_some() {
            return None;
        }
        let (mx, my) = self.last_mouse_pos?;
        let cx = self.ui_context();
        let component =
            crate::app::ui::settings_panel::SettingsPanelComponent::capture(self, &cx)?;
        component.hover_hit_id(mx, my, &cx)
    }

    /// Which paste-dialog button (`Paste` / `Cancel`) the cursor is over.
    /// Derived from `last_mouse_pos` + the dialog geometry the capture
    /// step would compute. Returns `None` when no paste is pending or
    /// the cursor is outside both buttons.
    ///
    /// Same pattern as `current_palette_hover` and
    /// `current_context_menu_hover`. Used by the chrome cache hash so
    /// hover changes invalidate the cache; display reads hover via the
    /// declarative `.hover()` style on each button.
    pub(crate) fn current_paste_dialog_hover(&self) -> Option<ciri_app::app::PasteButton> {
        if self.core.pending_paste.is_none() {
            return None;
        }
        let (mx, my) = self.last_mouse_pos?;
        let (vw, vh) = self.command_palette_viewport_size();
        let (_, ch) = self.cell_dimensions();
        // Mirror `PasteDialogComponent::capture` + `build_tree` exactly:
        // dialog 60% × 40% centered, panel `.p(pad)` on all sides, body
        // is a `flex_col` whose last children are a `.flex_1()` spacer,
        // the button_row, then a `SPACE_2` trailing pad. So the
        // button_row sits at content_bottom − SPACE_2 − btn_h, full
        // content_w wide, with the two `btn_w` buttons centered with a
        // `pad` gap (`.justify_center().gap(pad)`).
        let dialog_w = vw * 0.6;
        let dialog_h = vh * 0.4;
        let dx = (vw - dialog_w) / 2.0;
        let dy = (vh - dialog_h) / 2.0;
        let pad = crate::app::ui::tokens::SPACE_4;
        let trailing = crate::app::ui::tokens::SPACE_2;
        let btn_w = 100.0_f32;
        let btn_h = crate::app::ui::tokens::control_height_lg(ch);
        let row_y = dy + dialog_h - pad - trailing - btn_h;
        if my < row_y || my >= row_y + btn_h {
            return None;
        }
        let content_w = dialog_w - pad * 2.0;
        let content_x = dx + pad;
        let row_total_w = btn_w * 2.0 + pad;
        let row_x = content_x + (content_w - row_total_w) / 2.0;
        let paste_left = row_x;
        let paste_right = row_x + btn_w;
        let cancel_left = paste_right + pad;
        let cancel_right = cancel_left + btn_w;
        if mx >= paste_left && mx < paste_right {
            Some(ciri_app::app::PasteButton::Paste)
        } else if mx >= cancel_left && mx < cancel_right {
            Some(ciri_app::app::PasteButton::Cancel)
        } else {
            None
        }
    }

    /// Which overview-action-bar button (`Close` / `Focus`) the cursor
    /// is over. Derived from `last_mouse_pos` + the bar geometry that
    /// `OverviewActionBarComponent` would build at paint time. Returns
    /// `None` when overview is closed, no pane is hovered, or the
    /// cursor is outside both buttons.
    ///
    /// Same pattern as `current_palette_hover` / `current_context_menu_hover`
    /// / `current_paste_dialog_hover` — the chrome cache hash hashes
    /// this value so a row-boundary crossing invalidates the cache,
    /// and display reads the live hover via `cx.is_hovered(hit_id)`.
    pub(crate) fn current_overview_action_hover(
        &self,
    ) -> Option<ciri_app::app::OverviewActionHover> {
        if !self.core.overview.active {
            return None;
        }
        let bar =
            crate::app::ui::overview::overview_action_bar_data(self, self.overview_hovered_pane)?;
        let (mx, my) = self.last_mouse_pos?;
        if my < bar.bar_y || my >= bar.bar_y + bar.bar_h {
            return None;
        }
        // Layout: close_button (close_w) | 1px separator (no hit_id —
        // routes to HIT_ACTION_BAR via parent) | focus_button
        // (focus_w - 1.0). The separator pixel is intentionally
        // excluded from both Close and Focus ranges so the cache key
        // matches what the hit-test tree would return for that strip
        // (HIT_ACTION_BAR / `Background`, neither button).
        let close_left = bar.pane_x;
        let close_right = bar.pane_x + bar.close_w;
        let separator_right = close_right + 1.0;
        let focus_right = separator_right + (bar.focus_w - 1.0).max(0.0);
        if mx >= close_left && mx < close_right {
            Some(ciri_app::app::OverviewActionHover::Close)
        } else if mx >= separator_right && mx < focus_right {
            Some(ciri_app::app::OverviewActionHover::Focus)
        } else {
            None
        }
    }

    /// Which inline pane-tab (top-bar integrated mode) the cursor is
    /// over. Derived from `last_mouse_pos` + the pane-tab layout the
    /// top bar would compute. Returns `None` outside the tabs strip,
    /// or when tabs aren't rendered (side-bar mode, no panes).
    ///
    /// Same pattern as the other derive helpers — replaces the stored
    /// `App.hovered_pane_tab` field that was consumed only by the
    /// chrome cache key (display now reads hover declaratively via
    /// `cx.is_hovered(hit_id)` after Step 28).
    pub(crate) fn current_pane_tab_hover(&self) -> Option<u64> {
        if !matches!(
            self.core.config.tabbar.position,
            ciri_config::config::TabBarPosition::Integrated,
        ) {
            return None;
        }
        let (mx, my) = self.last_mouse_pos?;
        let (vw, vh) = self.command_palette_viewport_size();
        let (cw, ch) = self.cell_dimensions();
        let layout = self.top_bar_layout(vw, vh, cw, ch, self.ui_shaper.as_ref());
        if my < layout.bar_y || my >= layout.bar_y + layout.bar_height {
            return None;
        }
        // Clamp to the painted wrapper bounds — `pane_tabs::into_children`
        // clips each tab to `[visible_left, visible_left + visible_w)`
        // where visible_left = tab.x.max(tabs_start_x) and visible_w >
        // 0 is the visible portion. Matching the same bounds keeps the
        // cache hash and the painted hit_id in lockstep at the bar edge.
        let tabs = self.pane_tab_layouts(cw, layout.tabs_area_px, self.ui_shaper.as_ref());
        // Tabs strip starts at the session label's right edge; matches
        // what `top_bar::row_slots` computes.
        let tabs_start_x = layout.session_w;
        for tab in &tabs {
            let visible_left = tab.x.max(tabs_start_x);
            let visible_right = tab.x + tab.w;
            if visible_right <= visible_left {
                continue;
            }
            if mx >= visible_left && mx < visible_right {
                return Some(tab.pane_id);
            }
        }
        None
    }

    /// Which top-bar region the cursor is over. Same pattern as
    /// `current_pane_tab_hover` — derived at hash time so the
    /// display-side `cx.is_hovered(hit_id)` and the cache invalidation
    /// signal stay in lockstep without a stored field on `App`.
    pub(crate) fn current_top_bar_region_hover(&self) -> Option<ciri_app::app::TopBarHoverRegion> {
        let (mx, my) = self.last_mouse_pos?;
        let (vw, vh) = self.command_palette_viewport_size();
        let (cw, ch) = self.cell_dimensions();
        let layout = self.top_bar_layout(vw, vh, cw, ch, self.ui_shaper.as_ref());
        if my < layout.bar_y || my >= layout.bar_y + layout.bar_height {
            return None;
        }
        // Top bar lays out: session label | pane-tabs strip |
        // workspace label | mode label. Walk from the right — fixed-
        // width slots first — and fall back to None if the cursor
        // sits on the pane-tabs middle.
        let mode_label = self.current_mode_label().0;
        let mode_w = crate::app::top_bar::measure(self.ui_shaper.as_ref(), &mode_label, cw);
        let mode_left = vw - mode_w;
        if mx >= mode_left {
            return Some(ciri_app::app::TopBarHoverRegion::Mode);
        }
        let ws_label = self.workspace_indicator_label();
        if !ws_label.is_empty() {
            let ws_w = crate::app::top_bar::measure(self.ui_shaper.as_ref(), &ws_label, cw);
            let ws_left = mode_left - ws_w;
            if mx >= ws_left {
                return Some(ciri_app::app::TopBarHoverRegion::Workspace);
            }
        }
        if mx < layout.session_w {
            return Some(ciri_app::app::TopBarHoverRegion::Session);
        }
        None
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
        let y = screen_y - self.content_origin_y();
        let content_h = self.core.workspaces.view_size.height;
        (y >= 0.0 && y < content_h).then_some(y)
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
    #[cfg(test)]
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

    pub fn mark_disconnected_for_reconnect(&mut self, reason: ciri_app::app::DisconnectReason) {
        self.core.mark_disconnected_for_reconnect(reason);
        // The IO thread is gone — its cancel endpoint has no listener.
        self.connection_cancel = None;
        self.clear_render_caches();
    }

    /// User pressed Esc while a connection is still pending ("Connecting…"
    /// or "Reconnecting…"). Signals the IO thread to stop, clears any
    /// outstanding retry schedule, and falls back to a background slot if
    /// one exists so the app doesn't exit out from under the user.
    pub fn cancel_connection_attempt(&mut self) {
        if let Some(cancel) = &self.connection_cancel {
            cancel.notify_one();
        }
        // Whatever Disconnected(Cancelled) the IO thread will eventually
        // send lands on the old slot's channel — harmless either way — but
        // we short-circuit the retry loop now so the user doesn't see
        // "Reconnecting (2/10)" flash after they cancelled.
        self.core.reconnect_state = None;
        self.dismiss_halted_connection();
    }

    /// User pressed Esc on the halted-connection banner. Prefer to fall back
    /// to an existing background slot so a bad remote attempt doesn't take
    /// the whole app with it; only let the event loop wind down when there's
    /// nothing else to show.
    pub fn dismiss_halted_connection(&mut self) {
        self.core.last_disconnect_reason = None;
        if self.core.background_slots.is_empty() {
            return;
        }
        // Pick the most-recently-seen background slot. `background_slots` is
        // a HashMap so we sort the ids for determinism across runs, matching
        // how `flat_session_list` already orders them.
        let mut ids: Vec<String> = self.core.background_slots.keys().cloned().collect();
        ids.sort();
        if let Some(id) = ids.first() {
            let id = id.clone();
            self.switch_to_slot(&id);
        }
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
            Arc<tokio::sync::Notify>,
        )>,
    ) {
        match result {
            Ok((tx, rx, cancel)) => {
                self.core.finish_reconnect_ok(tx, rx);
                self.connection_cancel = Some(cancel);
            }
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

    /// Invalidate cached rendering state for a pane (view + per-tile
    /// glyph and background caches). Cheap enough to call on every
    /// scroll / resize / selection change.
    ///
    /// Does NOT touch `image_atlas_entries` — those entries map atlas
    /// slots that are still valid as long as the underlying image data
    /// is. Dropping them on every routine invalidation would force
    /// re-uploads on every scroll. Use [`invalidate_pane_images`] when
    /// the pane or its images are actually being torn down.
    pub fn invalidate_pane_cache(&mut self, pane_id: u64) {
        self.cached_views.remove(&pane_id);
        self.cached_tile_glyphs.remove(&pane_id);
        self.cached_tile_backgrounds.remove(&pane_id);
    }

    /// Drop atlas entries for inline images owned by `pane_id`. Called
    /// when a pane closes or its image set is invalidated upstream.
    /// `image_atlas_entries` is keyed `(pane_id, image_id)` so this
    /// only evicts entries for the targeted pane.
    pub fn invalidate_pane_images(&mut self, pane_id: u64) {
        self.image_atlas_entries
            .retain(|(pid, _), _| *pid != pane_id);
    }

    pub fn clear_render_caches(&mut self) {
        self.cached_views.clear();
        self.cached_tile_glyphs.clear();
        self.cached_tile_backgrounds.clear();
        self.image_atlas_entries.clear();
        self.cached_ui_scene.clear();
        self.render_bufs.clear_retained_scene();
        self.last_render_snapshot = None;
    }

    /// Tear down any in-flight mouse interaction (text selection,
    /// column / tile / scrollbar drag) before opening a modal that
    /// will visually occlude the surface where the drag started.
    /// Without this, `mouse_left_held` and the various `drag.*`
    /// states stay live: subsequent `CursorMoved` events extend the
    /// selection or commit a resize through the modal, finalising on
    /// mouse-release. Called from every modal-open site that runs the
    /// close-others discipline.
    pub fn cancel_pending_mouse_interactions(&mut self) {
        self.mouse_left_held = false;
        self.mouse_left_passthrough = false;
        self.drag.col_dragging = None;
        self.drag.tile_dragging = None;
        self.drag.scrollbar_dragging = None;
    }

    pub fn schedule_redraw(&mut self) {
        self.pending_redraw = true;
    }

    pub fn flush_pending_redraw(&mut self) {
        if !self.pending_redraw {
            return;
        }
        self.pending_redraw = false;
        if let Some(window) = &self.window {
            window.request_redraw();
        }
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
        self.schedule_redraw();
    }
}

#[cfg(test)]
mod tests_app_layout {
    use super::App;
    use ciri_config::config::{CiriConfig, StatusBarPosition};
    use winit::dpi::PhysicalSize;

    fn make_app(statusbar_position: StatusBarPosition) -> App {
        let mut config = CiriConfig::default();
        config.window.width = 900.0;
        config.window.height = 700.0;
        config.statusbar.position = statusbar_position;

        let mut app = App::new(config, "test-session");
        app.preview_resize(PhysicalSize::new(900, 700));
        app
    }

    #[test]
    fn content_y_from_screen_excludes_top_and_bottom_chrome() {
        let app = make_app(StatusBarPosition::Top);
        let top_bar_h = app.status_bar_height();
        let content_h = app.core.workspaces.view_size.height;

        assert_eq!(app.content_y_from_screen(top_bar_h + 10.0), Some(10.0));
        assert_eq!(app.content_y_from_screen(top_bar_h - 1.0), None);
        assert_eq!(
            app.content_y_from_screen(top_bar_h + content_h - 1.0),
            Some(content_h - 1.0)
        );
        assert_eq!(app.content_y_from_screen(top_bar_h + content_h + 1.0), None);
    }

    #[test]
    fn content_y_from_screen_excludes_bottom_status_and_hints_bars() {
        let app = make_app(StatusBarPosition::Bottom);
        let content_h = app.core.workspaces.view_size.height;

        assert_eq!(app.content_y_from_screen(10.0), Some(10.0));
        assert_eq!(
            app.content_y_from_screen(content_h - 1.0),
            Some(content_h - 1.0)
        );
        assert_eq!(app.content_y_from_screen(content_h + 1.0), None);
    }
}
