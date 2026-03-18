mod connection;
mod grid;

use anyhow::Result;
use ciri_anim::animation::ViewOffset;
use ciri_config::config::CiriConfig;
use ciri_config::theme::ThemeConfig;
use ciri_input::action::Action;
use ciri_input::keybind::{KeyCombo, KeybindMap};
use ciri_input::leader::InputHandler;
use ciri_layout::column::ColumnWidth;
use ciri_layout::geometry::{Rect as GeoRect, ViewSize};
use ciri_layout::workspace_set::WorkspaceSet;
use ciri_render::glyph_cache::{GlyphAtlas, GlyphInstance};
use ciri_render::rect::Rect;
use ciri_render::renderer::Renderer;
use ciri_render::terminal::{self, TerminalView};
use ciri_protocol::message::*;
use connection::ServerEvent;
use crossbeam_channel::{Receiver, Sender};
use grid::ClientPaneGrid;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, Ime, MouseScrollDelta, StartCause, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{Window, WindowAttributes, WindowId};

struct App {
    config: CiriConfig,
    frame_interval: Duration,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    glyph_atlas: Option<GlyphAtlas>,
    dpi_scale: f64,
    workspaces: WorkspaceSet,
    /// Client-side cell grid mirrors (no PTY).
    pane_grids: HashMap<u64, ClientPaneGrid>,
    input: InputHandler,
    /// Server communication channels.
    server_tx: Option<Sender<ClientMessage>>,
    server_rx: Option<Receiver<ServerEvent>>,
    /// Global horizontal scroll offset.
    view_offset_x: ViewOffset,
    view_offset_y: ViewOffset,
    col_widths: Vec<ViewOffset>,
    last_frame: Instant,
    modifiers: ModifiersState,
    cached_views: HashMap<u64, TerminalView>,
    last_mouse_pos: Option<(f32, f32)>,
    overview_active: bool,
    overview_zoom: ViewOffset,
    overview_dragging: bool,
    drag_last_pos: Option<(f32, f32)>,
    bg_rects_buf: Vec<Rect>,
    glyph_buf: Vec<GlyphInstance>,
    ime_preedit_active: bool,
    last_ime_pos: Option<(i32, i32)>,
    overview_keybinds: KeybindMap,
    resize_dragging: Option<usize>,
    resize_drag_start_x: f32,
    resize_drag_start_width: f32,
    /// Whether we've received initial StateSync from server.
    connected: bool,
}

impl App {
    fn new(config: CiriConfig) -> Self {
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

        App {
            config,
            frame_interval,
            window: None,
            renderer: None,
            glyph_atlas: None,
            dpi_scale: 1.0,
            workspaces: WorkspaceSet::new(initial_view),
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
        }
    }

    /// Send a message to the server.
    fn send(&self, msg: ClientMessage) {
        if let Some(tx) = &self.server_tx {
            if let Err(e) = tx.try_send(msg) {
                log::warn!("failed to send to server: {e}");
            }
        }
    }

    /// Process all pending server events.
    fn process_server_events(&mut self) -> bool {
        let Some(rx) = self.server_rx.as_ref() else { return false };
        // Drain all events first to avoid borrow conflicts
        let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        if events.is_empty() { return false; }
        let mut needs_redraw = false;

        for event in events {
            match event {
                ServerEvent::Control(ServerMessage::StateSync { layout, pane_ids }) => {
                    self.apply_layout(&layout);
                    // Grids will be populated by FullPaneSync messages that follow
                    for &id in &pane_ids {
                        self.pane_grids.entry(id).or_insert_with(|| ClientPaneGrid::new(80, 24));
                    }
                    self.connected = true;
                    needs_redraw = true;
                }
                ServerEvent::Control(ServerMessage::LayoutUpdate { layout }) => {
                    self.apply_layout(&layout);
                    needs_redraw = true;
                }
                ServerEvent::Control(ServerMessage::PaneCreated { pane_id, .. }) => {
                    self.pane_grids.entry(pane_id).or_insert_with(|| ClientPaneGrid::new(80, 24));
                    needs_redraw = true;
                }
                ServerEvent::Control(ServerMessage::PaneClosed { pane_id }) => {
                    self.pane_grids.remove(&pane_id);
                    self.cached_views.remove(&pane_id);
                    needs_redraw = true;
                }
                ServerEvent::Control(ServerMessage::ServerShutdown) => {
                    log::info!("server shut down");
                    self.pane_grids.clear();
                    self.cached_views.clear();
                    needs_redraw = true;
                }
                ServerEvent::FullPaneSync(sync) => {
                    let grid = self.pane_grids.entry(sync.pane_id)
                        .or_insert_with(|| ClientPaneGrid::new(sync.cols, sync.rows));
                    grid.apply_full_sync(&sync);
                    self.cached_views.remove(&sync.pane_id);
                    needs_redraw = true;
                }
                ServerEvent::CellDelta(delta) => {
                    if let Some(grid) = self.pane_grids.get_mut(&delta.pane_id) {
                        grid.apply_delta(&delta);
                        self.cached_views.remove(&delta.pane_id);
                        needs_redraw = true;
                    }
                }
                ServerEvent::Disconnected => {
                    log::warn!("disconnected from server, exiting");
                    self.connected = false;
                    self.pane_grids.clear();
                    self.cached_views.clear();
                    self.server_tx = None;
                    self.server_rx = None;
                    return true; // signal caller to exit
                }
            }
        }
        needs_redraw
    }

    fn apply_layout(&mut self, layout: &LayoutState) {
        // Rebuild workspace from server's layout state
        let ws = self.workspaces.active_mut();
        ws.columns.clear();
        for col_state in &layout.columns {
            if let Some(tile) = col_state.tiles.first() {
                use ciri_layout::column::Column;
                let mut col = Column::new(tile.pane_id);
                col.width = ColumnWidth::Proportion(col_state.width_proportion);
                ws.columns.push(col);
            }
        }
        ws.active_column_idx = layout.active_column_idx.min(ws.columns.len().saturating_sub(1));
        self.snap_all_col_widths();
        self.animate_to_active();
    }

    fn hit_test_overview(&self, mx: f32, my: f32) -> Option<(usize, u64)> {
        let zoom = self.overview_zoom.value() as f32;
        let zoom_threshold = self.config.animation.zoom_threshold;
        let tiles = if self.overview_active || zoom < zoom_threshold {
            self.workspaces.all_tiles_2d()
        } else {
            self.workspaces.visible_tiles_2d()
        };
        let (vw, vh) = self.renderer.as_ref()
            .map(|r| { let (w, h) = r.surface_size(); (w as f32, h as f32) })
            .unwrap_or((self.config.window.width as f32, self.config.window.height as f32));
        let cx = vw / 2.0;
        let cy = vh / 2.0;

        for (pane_id, tile_rect, _) in &tiles {
            let tr = if zoom < zoom_threshold {
                GeoRect::new(
                    cx + (tile_rect.x - cx) * zoom,
                    cy + (tile_rect.y - cy) * zoom,
                    tile_rect.w * zoom,
                    tile_rect.h * zoom,
                )
            } else {
                *tile_rect
            };
            if tr.contains(mx, my) {
                for (row_idx, ws) in self.workspaces.rows.iter().enumerate() {
                    if ws.columns.iter().any(|c| c.pane_id == *pane_id) {
                        return Some((row_idx, *pane_id));
                    }
                }
            }
        }
        None
    }

    fn total_inset(&self) -> f32 {
        (self.config.appearance.padding + self.config.appearance.border_width) * 2.0
    }

    fn compute_grid_size(&self) -> (u16, u16) {
        if let Some(atlas) = &self.glyph_atlas {
            let pad = self.total_inset();
            let vw = self.workspaces.view_size.width - pad;
            let vh = self.workspaces.view_size.height - pad;
            atlas.grid_size(vw, vh)
        } else {
            (80, 24)
        }
    }

    fn cell_dimensions(&self) -> (f32, f32) {
        if let Some(atlas) = &self.glyph_atlas {
            (atlas.cell_width, atlas.cell_height)
        } else {
            (8.0, 16.0)
        }
    }

    fn status_bar_height(&self) -> f32 {
        let cell_h = self.glyph_atlas.as_ref()
            .map(|a| a.cell_height)
            .unwrap_or(self.config.font.size * 1.2);
        cell_h + self.config.statusbar.height_padding
    }

    fn snap_all_col_widths(&mut self) {
        let vw = self.workspaces.view_size.width;
        self.col_widths.clear();
        for ws in &mut self.workspaces.rows {
            for col in &mut ws.columns {
                col.snap_width(vw);
            }
        }
    }

    fn refresh_overview_zoom(&mut self) {
        if !self.overview_active { return; }
        let omega = self.config.animation.speed;
        let vw = self.workspaces.view_size.width;
        let vh = self.workspaces.view_size.height;
        let max_w = self.workspaces.rows.iter()
            .map(|ws| ws.total_width())
            .fold(0.0f32, f32::max)
            .max(vw);
        let nrows = self.workspaces.rows.iter().filter(|ws| !ws.is_empty()).count().max(1);
        let total_h = nrows as f32 * vh + (nrows.saturating_sub(1)) as f32 * self.workspaces.row_gap;
        let fit = self.config.animation.overview_zoom_fit;
        let zoom_x = vw / max_w;
        let zoom_y = vh / total_h;
        let zoom = (zoom_x.min(zoom_y).min(1.0) * fit).max(0.15);
        self.overview_zoom.animate_to(zoom as f64, omega);
    }

    fn switch_to_workspace(&mut self, idx: usize) {
        let old = self.workspaces.active_workspace_idx();
        self.workspaces.switch_to(idx);
        if old != idx {
            self.snap_all_col_widths();
            self.cached_views.clear();
            self.animate_to_active();
        }
    }

    fn handle_action(&mut self, action: Action) {
        match action {
            Action::NewColumnRight => {
                self.send(ClientMessage::CreatePane);
            }
            Action::NewRowBelow => {
                self.send(ClientMessage::SplitDown);
            }
            Action::ClosePane => {
                if let Some(pane_id) = self.workspaces.active_mut().active_pane_id() {
                    self.send(ClientMessage::ClosePane { pane_id });
                }
            }
            Action::FocusLeft => { self.send(ClientMessage::FocusLeft); }
            Action::FocusRight => { self.send(ClientMessage::FocusRight); }
            Action::FocusDown => { self.send(ClientMessage::FocusDown); }
            Action::FocusUp => { self.send(ClientMessage::FocusUp); }
            Action::MovePaneLeft => { self.send(ClientMessage::MovePaneLeft); }
            Action::MovePaneRight => { self.send(ClientMessage::MovePaneRight); }
            Action::ColumnWidthOneThird => {
                self.send(ClientMessage::SetColumnWidth { proportion: 1.0 / 3.0 });
            }
            Action::ColumnWidthHalf => {
                self.send(ClientMessage::SetColumnWidth { proportion: 0.5 });
            }
            Action::ColumnWidthTwoThirds => {
                self.send(ClientMessage::SetColumnWidth { proportion: 2.0 / 3.0 });
            }
            Action::ColumnWidthFull => {
                self.send(ClientMessage::SetColumnWidth { proportion: 1.0 });
            }
            Action::ColumnWidthIncrease => {
                // Handle locally for responsiveness, also send to server
                self.workspaces.active_mut().resize_active_column(0.02);
                self.snap_all_col_widths();
                self.animate_to_active();
            }
            Action::ColumnWidthDecrease => {
                self.workspaces.active_mut().resize_active_column(-0.02);
                self.snap_all_col_widths();
                self.animate_to_active();
            }
            Action::ExitOverview => {
                self.overview_active = false;
                self.overview_zoom.animate_to(1.0, self.config.animation.speed);
                self.animate_to_active();
            }
            Action::SwitchWorkspace(idx) => {
                self.switch_to_workspace(idx);
            }
            Action::ToggleOverview => {
                self.overview_active = !self.overview_active;
                let omega = self.config.animation.speed;
                if self.overview_active {
                    self.refresh_overview_zoom();
                    self.view_offset_x.animate_to(0.0, omega);
                    self.view_offset_y.animate_to(0.0, omega);
                } else {
                    self.overview_zoom.animate_to(1.0, omega);
                    self.animate_to_active();
                }
            }
            Action::SendLeaderKey => {
                if let Some(pid) = self.workspaces.active_mut().active_pane_id() {
                    self.send(ClientMessage::Input { pane_id: pid, data: vec![0x17] });
                }
            }
        }
    }

    fn animate_to_active(&mut self) {
        let omega = self.config.animation.speed;
        let enabled = self.config.animation.enabled;

        let target_x = self.workspaces.active_mut().target_offset_for_active();
        if enabled {
            self.view_offset_x.animate_to(target_x as f64, omega);
        } else {
            self.view_offset_x.jump_to(target_x as f64);
        }

        let target_y = self.workspaces.target_offset_y();
        if enabled {
            self.view_offset_y.animate_to(target_y as f64, omega);
        } else {
            self.view_offset_y.jump_to(target_y as f64);
            self.workspaces.view_offset_y = target_y;
        }

        self.sync_col_animations();
    }

    fn sync_col_animations(&mut self) {
        let ncols = self.workspaces.active().columns.len();
        while self.col_widths.len() < ncols {
            self.col_widths.push(ViewOffset::new());
        }
        self.col_widths.truncate(ncols);

        let vw = self.workspaces.active().view_size.width;
        let omega = self.config.animation.speed;
        for (i, col) in self.workspaces.active().columns.iter().enumerate() {
            let target = col.resolve_width(vw) as f64;
            let current = self.col_widths[i].value();
            let anim_target = self.col_widths[i].target();
            if self.config.animation.enabled {
                if current == 0.0 {
                    self.col_widths[i].jump_to(target);
                } else if (anim_target - target).abs() > 1.0 {
                    self.col_widths[i].animate_to(target, omega);
                }
            } else {
                self.col_widths[i].jump_to(target);
            }
        }

        let ws = self.workspaces.active_mut();
        for (i, col) in ws.columns.iter_mut().enumerate() {
            if i < self.col_widths.len() {
                col.set_rendered_width(self.col_widths[i].value() as f32);
            }
        }
    }

    fn advance_animations(&mut self, dt: f64) -> bool {
        let mut animating = self.view_offset_x.advance(dt);
        if self.view_offset_y.advance(dt) { animating = true; }
        if self.overview_zoom.advance(dt) { animating = true; }
        let vox = self.view_offset_x.value() as f32;
        for ws in &mut self.workspaces.rows {
            ws.view_offset_x = vox;
        }
        self.workspaces.view_offset_y = self.view_offset_y.value() as f32;

        if !self.col_widths.is_empty() {
            self.sync_col_animations();
            let ws = self.workspaces.active_mut();
            for (i, col) in ws.columns.iter_mut().enumerate() {
                if i < self.col_widths.len() {
                    if self.col_widths[i].advance(dt) { animating = true; }
                    col.set_rendered_width(self.col_widths[i].value() as f32);
                }
            }
        }
        animating
    }

    fn build_tiles(
        &mut self,
        tiles: &[(u64, GeoRect, bool)],
        zoom: f32,
        vw: f32,
        vh: f32,
        bg_rects: &mut Vec<Rect>,
        glyphs: &mut Vec<GlyphInstance>,
    ) {
        let zoom_threshold = self.config.animation.zoom_threshold;
        let padding = self.config.appearance.padding;
        let border_w = self.config.appearance.border_width;
        let active_border = ThemeConfig::parse_color(&self.config.theme.border_active);
        let inactive_border = ThemeConfig::parse_color(&self.config.theme.border_inactive);
        let bg_color = ThemeConfig::parse_color(&self.config.theme.background);

        for (pane_id, tile_rect, is_active) in tiles {
            let tr = if zoom < zoom_threshold {
                let cx = vw / 2.0;
                let cy = vh / 2.0;
                GeoRect::new(
                    cx + (tile_rect.x - cx) * zoom,
                    cy + (tile_rect.y - cy) * zoom,
                    tile_rect.w * zoom,
                    tile_rect.h * zoom,
                )
            } else {
                *tile_rect
            };

            bg_rects.push(Rect {
                x: tr.x, y: tr.y, w: tr.w, h: tr.h,
                color: if *is_active { active_border } else { inactive_border },
            });
            bg_rects.push(Rect {
                x: tr.x + border_w * zoom, y: tr.y + border_w * zoom,
                w: tr.w - border_w * zoom * 2.0, h: tr.h - border_w * zoom * 2.0,
                color: bg_color,
            });

            let Some(view) = self.cached_views.get(pane_id) else { continue };
            let inner_x = tr.x + (border_w + padding) * zoom;
            let inner_y = tr.y + (border_w + padding) * zoom;

            for r in &view.bg_rects {
                let src = GeoRect::new(
                    inner_x + r.x * zoom,
                    inner_y + r.y * zoom,
                    r.w * zoom,
                    r.h * zoom,
                );
                if let Some(c) = src.intersection(&tr) {
                    bg_rects.push(Rect { x: c.x, y: c.y, w: c.w, h: c.h, color: r.color });
                }
            }

            for cursor in &view.cursor_rects {
                let src = GeoRect::new(
                    inner_x + cursor.x * zoom,
                    inner_y + cursor.y * zoom,
                    cursor.w * zoom,
                    cursor.h * zoom,
                );
                if let Some(c) = src.intersection(&tr) {
                    bg_rects.push(Rect { x: c.x, y: c.y, w: c.w, h: c.h, color: cursor.color });
                }
            }

            glyphs.extend(view.glyph_instances.iter().filter_map(|g| {
                let sx = inner_x + g.px * zoom;
                let sy = inner_y + g.py * zoom;
                let gw = g.glyph_w * zoom;
                let gh = g.glyph_h * zoom;

                if gw <= 0.0 || gh <= 0.0 { return None; }

                if sx >= tr.x && sy >= tr.y && sx + gw <= tr.x + tr.w && sy + gh <= tr.y + tr.h {
                    return Some(GlyphInstance {
                        pos: [sx / vw * 2.0 - 1.0, 1.0 - sy / vh * 2.0],
                        size: [gw / vw * 2.0, -(gh / vh * 2.0)],
                        uv_pos: [g.u0, g.v0],
                        uv_size: [g.u1 - g.u0, g.v1 - g.v0],
                        color: g.color,
                    });
                }

                let src = GeoRect::new(sx, sy, gw, gh);
                let c = src.intersection(&tr)?;
                let u_full = g.u1 - g.u0;
                let v_full = g.v1 - g.v0;

                Some(GlyphInstance {
                    pos: [c.x / vw * 2.0 - 1.0, 1.0 - c.y / vh * 2.0],
                    size: [c.w / vw * 2.0, -(c.h / vh * 2.0)],
                    uv_pos: [
                        g.u0 + u_full * (c.x - sx) / gw,
                        g.v0 + v_full * (c.y - sy) / gh,
                    ],
                    uv_size: [u_full * c.w / gw, v_full * c.h / gh],
                    color: g.color,
                })
            }));
        }
    }

    fn build_status_bar(
        &mut self,
        vw: f32,
        vh: f32,
        bg_rects: &mut Vec<Rect>,
        glyphs: &mut Vec<GlyphInstance>,
    ) {
        let renderer = self.renderer.as_mut().unwrap();
        let atlas = self.glyph_atlas.as_mut().unwrap();

        let bar_height = atlas.cell_height + self.config.statusbar.height_padding;
        let bar_y = vh - bar_height;
        bg_rects.push(Rect {
            x: 0.0, y: bar_y, w: vw, h: bar_height,
            color: ThemeConfig::parse_color(&self.config.theme.statusbar_background),
        });

        let ws_idx = self.workspaces.active_workspace_idx();
        let leader_hint = if self.input.is_awaiting_action() { " LEADER " } else { "" };
        let overview_hint = if self.overview_active { " OVERVIEW " } else { "" };

        let mut status_left = String::new();
        for (i, ws) in self.workspaces.rows.iter().enumerate() {
            if !ws.is_empty() || i == ws_idx {
                if i == ws_idx {
                    status_left.push_str(&format!(" [{}*] ", i + 1));
                } else {
                    status_left.push_str(&format!(" [{}] ", i + 1));
                }
            }
        }

        let active_info = self.workspaces.active().active_pane_id()
            .map(|id| format!("pane:{id}"))
            .unwrap_or_default();
        let status_right = format!("{overview_hint}{leader_hint} {active_info} ");

        let text_y = bar_y + 2.0;
        let cw = atlas.cell_width;
        let baseline = atlas.cell_height * self.config.statusbar.text_baseline;

        let left_color = if self.input.is_awaiting_action() {
            ThemeConfig::parse_color(&self.config.theme.accent)
        } else {
            ThemeConfig::parse_color(&self.config.theme.foreground)
        };
        let right_color = if self.overview_active || self.input.is_awaiting_action() {
            ThemeConfig::parse_color(&self.config.theme.accent)
        } else {
            ThemeConfig::parse_color(&self.config.theme.bright_black)
        };

        emit_status_text(atlas, &mut renderer.text.font_system, &renderer.queue,
            &status_left, 0.0, text_y, cw, baseline, left_color, vw, vh, glyphs);

        let right_start_x = vw - status_right.len() as f32 * cw;
        emit_status_text(atlas, &mut renderer.text.font_system, &renderer.queue,
            &status_right, right_start_x, text_y, cw, baseline, right_color, vw, vh, glyphs);

        if self.input.is_awaiting_action() {
            let indicator_h = self.config.statusbar.leader_indicator_height;
            bg_rects.push(Rect {
                x: 0.0, y: bar_y - indicator_h, w: vw, h: indicator_h,
                color: ThemeConfig::parse_color(&self.config.theme.accent),
            });
        }
    }

    fn submit_frame(
        renderer: &mut Renderer,
        atlas: &mut GlyphAtlas,
        clear_color: [f32; 4],
        bg_rects: &[Rect],
        glyphs: &[GlyphInstance],
    ) {
        let (vw, vh) = renderer.surface_size();
        let vw_f = vw as f32;
        let vh_f = vh as f32;

        let output = match renderer.surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Lost) => { renderer.resize(vw, vh); return; }
            Err(e) => { log::error!("surface error: {e}"); return; }
        };

        let tex_view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = renderer.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor { label: Some("ciri") },
        );

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ciri_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &tex_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear_color[0] as f64,
                            g: clear_color[1] as f64,
                            b: clear_color[2] as f64,
                            a: clear_color[3] as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

            renderer.rects.render(&renderer.queue, &mut pass, bg_rects, vw_f, vh_f);
            atlas.render(&renderer.queue, &mut pass, glyphs);
        }

        renderer.queue.submit(std::iter::once(encoder.finish()));
        output.present();
    }

    fn render(&mut self) {
        if self.renderer.is_none() || self.glyph_atlas.is_none() {
            return;
        }

        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f64();
        self.last_frame = now;

        let animating = self.advance_animations(dt);

        let renderer = self.renderer.as_mut().unwrap();
        let atlas = self.glyph_atlas.as_mut().unwrap();
        let (vw, vh) = renderer.surface_size();
        let vw_f = vw as f32;
        let vh_f = vh as f32;
        let zoom = self.overview_zoom.value() as f32;
        let zoom_threshold = self.config.animation.zoom_threshold;

        let tiles = if self.overview_active || zoom < zoom_threshold {
            self.workspaces.all_tiles_2d()
        } else {
            self.workspaces.visible_tiles_2d()
        };

        // Update terminal views for dirty pane grids
        for (pane_id, _, _) in &tiles {
            let is_dirty = self.pane_grids.get(pane_id).is_some_and(|g| g.dirty);
            if (is_dirty || !self.cached_views.contains_key(pane_id))
                && let Some(grid) = self.pane_grids.get_mut(pane_id) {
                    let view = terminal::build_view_from_grid(
                        &grid.cells, grid.cols, grid.rows,
                        grid.cursor_line, grid.cursor_col, grid.cursor_shape,
                        atlas, &mut renderer.text.font_system, &renderer.queue,
                        &self.config,
                    );
                    grid.dirty = false;
                    self.cached_views.insert(*pane_id, view);
                }
        }

        // Update IME cursor area
        if let Some(window) = &self.window
            && let Some(active_pid) = self.workspaces.active().active_pane_id()
                && let Some((_, tile_rect, _)) = tiles.iter().find(|(id, _, _)| *id == active_pid)
                    && let Some(view) = self.cached_views.get(&active_pid)
                        && let Some(cursor) = view.cursor_rects.first() {
                            let padding = self.config.appearance.padding;
                            let border_w = self.config.appearance.border_width;
                            let cx = (tile_rect.x + border_w + padding + cursor.x) as i32;
                            let cy = (tile_rect.y + border_w + padding + cursor.y) as i32;
                            let pos = (cx, cy);
                            if self.last_ime_pos != Some(pos) {
                                self.last_ime_pos = Some(pos);
                                window.set_ime_cursor_area(
                                    winit::dpi::PhysicalPosition::new(cx as f64, cy as f64),
                                    winit::dpi::PhysicalSize::new(
                                        atlas.cell_width as f64,
                                        atlas.cell_height as f64,
                                    ),
                                );
                            }
                        }

        let mut bg_rects = std::mem::take(&mut self.bg_rects_buf);
        let mut glyphs = std::mem::take(&mut self.glyph_buf);
        bg_rects.clear();
        glyphs.clear();

        self.build_tiles(&tiles, zoom, vw_f, vh_f, &mut bg_rects, &mut glyphs);
        self.build_status_bar(vw_f, vh_f, &mut bg_rects, &mut glyphs);

        let clear_color = ThemeConfig::parse_color(&self.config.theme.ui_background);
        let renderer = self.renderer.as_mut().unwrap();
        let atlas = self.glyph_atlas.as_mut().unwrap();
        Self::submit_frame(renderer, atlas, clear_color, &bg_rects, &glyphs);

        self.bg_rects_buf = bg_rects;
        self.glyph_buf = glyphs;

        if animating
            && let Some(w) = &self.window { w.request_redraw(); }
    }
}

fn emit_status_text(
    atlas: &mut GlyphAtlas,
    font_system: &mut glyphon::FontSystem,
    queue: &wgpu::Queue,
    text: &str,
    x_start: f32,
    text_y: f32,
    cell_width: f32,
    baseline: f32,
    color: [f32; 4],
    vw: f32,
    vh: f32,
    glyphs: &mut Vec<GlyphInstance>,
) {
    for (i, ch) in text.chars().enumerate() {
        if let Some(entry) = atlas.ensure_char(ch, font_system, queue)
            && entry.width > 0 && entry.height > 0 {
                let sx = x_start + i as f32 * cell_width + entry.bearing_x as f32;
                let sy = text_y + baseline - entry.bearing_y as f32;
                glyphs.push(GlyphInstance {
                    pos: [sx / vw * 2.0 - 1.0, 1.0 - sy / vh * 2.0],
                    size: [entry.width as f32 / vw * 2.0, -(entry.height as f32 / vh * 2.0)],
                    uv_pos: [entry.u0, entry.v0],
                    uv_size: [entry.u1 - entry.u0, entry.v1 - entry.v0],
                    color,
                });
            }
    }
}

impl ApplicationHandler for App {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + self.frame_interval));

        if matches!(cause, StartCause::ResumeTimeReached { .. } | StartCause::Poll) {
            let mut needs_redraw = self.view_offset_x.is_animating()
                || self.view_offset_y.is_animating()
                || self.overview_zoom.is_animating()
                || self.col_widths.iter().any(|v| v.is_animating());

            // Process server events
            if self.process_server_events() { needs_redraw = true; }

            // Exit on disconnect
            if !self.connected && self.server_rx.is_none() {
                log::info!("server connection lost, exiting");
                self.cached_views.clear();
                self.glyph_atlas = None;
                self.renderer = None;
                self.window = None;
                event_loop.exit();
                return;
            }

            // Check if all panes are gone (server shutdown)
            if self.connected && self.pane_grids.is_empty() && self.workspaces.active().is_empty() {
                // Server has no panes left — exit
                self.cached_views.clear();
                self.glyph_atlas = None;
                self.renderer = None;
                self.window = None;
                event_loop.exit();
                return;
            }

            if needs_redraw
                && let Some(w) = &self.window { w.request_redraw(); }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() { return; }

        let attrs = WindowAttributes::default()
            .with_title(&self.config.window.title)
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.config.window.width,
                self.config.window.height,
            ));

        let window = Arc::new(event_loop.create_window(attrs).expect("failed to create window"));
        window.set_ime_allowed(true);
        let dpi_scale = window.scale_factor();
        let mut renderer = pollster::block_on(Renderer::new(window.clone(), &self.config.render)).expect("renderer init failed");

        let fmt = renderer.surface_format();
        let atlas = GlyphAtlas::new(
            &renderer.device, fmt,
            &mut renderer.text.font_system, self.config.font.size,
            dpi_scale, &self.config.font.family,
            &self.config.render,
        );

        let (w, h) = renderer.surface_size();
        let bar_h = atlas.cell_height + self.config.statusbar.height_padding;
        self.workspaces.resize_view(ViewSize { width: w as f32, height: h as f32 - bar_h });

        log::info!("cell: {:.1}x{:.1} (dpi_scale={:.2})", atlas.cell_width, atlas.cell_height, dpi_scale);

        // Connect to server (or spawn one)
        let (cw, ch) = self.cell_dimensions();
        let view = &self.workspaces.view_size;
        let viewport = ciri_protocol::codec::ClientViewport {
            width: view.width as u32,
            height: view.height as u32,
            cell_width: cw,
            cell_height: ch,
        };
        match connection::connect_or_spawn("default", viewport) {
            Ok((tx, rx)) => {
                self.server_tx = Some(tx);
                self.server_rx = Some(rx);
            }
            Err(e) => {
                log::error!("failed to connect to server: {e}");
            }
        }

        self.dpi_scale = dpi_scale;
        self.glyph_atlas = Some(atlas);
        self.snap_all_col_widths();
        self.animate_to_active();
        self.window = Some(window);
        self.renderer = Some(renderer);
        self.last_frame = Instant::now();

        self.window.as_ref().unwrap().request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.send(ClientMessage::Detach);
                self.pane_grids.clear();
                self.cached_views.clear();
                self.glyph_atlas = None;
                self.renderer = None;
                self.window = None;
                event_loop.exit();
            }

            WindowEvent::ModifiersChanged(mods) => { self.modifiers = mods.state(); }

            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
                let bar_h = self.status_bar_height();
                self.workspaces.resize_view(ViewSize {
                    width: size.width as f32, height: size.height as f32 - bar_h,
                });
                self.snap_all_col_widths();
                // Mark all grids dirty
                for grid in self.pane_grids.values_mut() { grid.dirty = true; }
                self.cached_views.clear();
                self.animate_to_active();
                // Notify server of resize
                let (cols, rows) = self.compute_grid_size();
                let (cw, ch) = self.cell_dimensions();
                let view = &self.workspaces.view_size;
                self.send(ClientMessage::Resize {
                    cols, rows,
                    width: view.width as u32, height: view.height as u32,
                    cell_width: cw, cell_height: ch,
                });
                if let Some(w) = &self.window { w.request_redraw(); }
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed { return; }
                if self.ime_preedit_active { return; }

                let ctrl = self.modifiers.control_key();
                let shift = self.modifiers.shift_key();

                let key_name = match &event.logical_key {
                    Key::Named(n) => match n {
                        NamedKey::Space => "space",
                        NamedKey::Tab => "tab",
                        NamedKey::Escape => "escape",
                        NamedKey::Enter => "enter",
                        NamedKey::ArrowLeft => "Left",
                        NamedKey::ArrowRight => "Right",
                        NamedKey::ArrowUp => "Up",
                        NamedKey::ArrowDown => "Down",
                        _ => "",
                    },
                    Key::Character(c) => c.as_str(),
                    _ => "",
                };

                if self.overview_active {
                    let combo = if shift {
                        KeyCombo::with_shift(&key_name.to_lowercase())
                    } else {
                        KeyCombo::new(&key_name.to_lowercase())
                    };
                    if let Some(action) = self.overview_keybinds.lookup(&combo) {
                        match action {
                            Action::ExitOverview => {
                                self.overview_active = false;
                                self.overview_zoom.animate_to(1.0, self.config.animation.speed);
                                self.animate_to_active();
                            }
                            other => self.handle_action(other),
                        }
                    }
                } else {
                    use ciri_input::leader::InputResult;
                    let result = if !key_name.is_empty() {
                        self.input.process_key(key_name, ctrl, shift)
                    } else {
                        InputResult::PassThrough
                    };

                    match result {
                        InputResult::Action(action) => self.handle_action(action),
                        InputResult::Consumed => {}
                        InputResult::PassThrough => {
                            let bytes = key_event_to_pty_bytes(&event, ctrl);
                            if !bytes.is_empty()
                                && let Some(pid) = self.workspaces.active_mut().active_pane_id() {
                                    self.send(ClientMessage::Input { pane_id: pid, data: bytes });
                                }
                        }
                    }
                }
                if let Some(w) = &self.window { w.request_redraw(); }
            }

            WindowEvent::CursorMoved { position, .. } => {
                let mx = position.x as f32;
                let my = position.y as f32;
                self.last_mouse_pos = Some((mx, my));

                if self.overview_active {
                    if let Some((row_idx, pane_id)) = self.hit_test_overview(mx, my) {
                        if row_idx < self.workspaces.rows.len() {
                            self.workspaces.active_row = row_idx;
                            let ws = self.workspaces.active_mut();
                            for (col_idx, col) in ws.columns.iter().enumerate() {
                                if col.pane_id == pane_id {
                                    ws.active_column_idx = col_idx;
                                    break;
                                }
                            }
                        }
                        if let Some(w) = &self.window { w.request_redraw(); }
                    }

                    if self.overview_dragging {
                        if let Some((lx, ly)) = self.drag_last_pos {
                            let zoom = self.overview_zoom.value() as f32;
                            let dx = (mx - lx) / zoom;
                            let dy = (my - ly) / zoom;
                            let cur_x = self.view_offset_x.value();
                            self.view_offset_x.jump_to(cur_x - dx as f64);
                            let vox = self.view_offset_x.value() as f32;
                            for ws in &mut self.workspaces.rows {
                                ws.view_offset_x = vox;
                            }
                            let cur_y = self.view_offset_y.value();
                            self.view_offset_y.jump_to(cur_y - dy as f64);
                            self.workspaces.view_offset_y = self.view_offset_y.value() as f32;
                            if let Some(w) = &self.window { w.request_redraw(); }
                        }
                        self.drag_last_pos = Some((mx, my));
                    }
                } else {
                    if let Some(drag_col) = self.resize_dragging {
                        let delta = mx - self.resize_drag_start_x;
                        let new_width = (self.resize_drag_start_width + delta).max(50.0);
                        let vw = self.workspaces.active().view_size.width;
                        let proportion = (new_width as f64 / vw as f64).clamp(0.1, 0.9);
                        self.workspaces.active_mut().set_column_width_by_index(
                            drag_col,
                            ColumnWidth::Proportion(proportion),
                        );
                        self.snap_all_col_widths();
                        if let Some(w) = &self.window { w.request_redraw(); }
                    } else {
                        let ws = self.workspaces.active();
                        let vox = ws.view_offset_x;
                        let mut near_border = false;
                        for i in 1..ws.columns.len() {
                            let col_x = ws.column_x(i) - vox;
                            if (mx - col_x).abs() < 4.0 {
                                near_border = true;
                                break;
                            }
                        }
                        if let Some(w) = &self.window {
                            if near_border {
                                w.set_cursor(winit::window::CursorIcon::ColResize);
                            } else {
                                w.set_cursor(winit::window::CursorIcon::Default);
                            }
                        }
                    }
                }
            }

            WindowEvent::MouseInput { state, button: winit::event::MouseButton::Left, .. } => {
                if let Some((mx, my)) = self.last_mouse_pos {
                    if state == ElementState::Pressed {
                        if self.overview_active {
                            if let Some((row_idx, pane_id)) = self.hit_test_overview(mx, my) {
                                if row_idx < self.workspaces.rows.len() {
                                    self.workspaces.active_row = row_idx;
                                    let ws = self.workspaces.active_mut();
                                    for (col_idx, col) in ws.columns.iter().enumerate() {
                                        if col.pane_id == pane_id {
                                            ws.active_column_idx = col_idx;
                                            break;
                                        }
                                    }
                                }
                                self.overview_active = false;
                                self.overview_zoom.animate_to(1.0, self.config.animation.speed);
                                self.animate_to_active();
                            } else {
                                self.overview_dragging = true;
                                self.drag_last_pos = Some((mx, my));
                            }
                        } else {
                            let ws = self.workspaces.active();
                            let vox = ws.view_offset_x;
                            let vw = ws.view_size.width;
                            let mut started_drag = false;
                            for i in 1..ws.columns.len() {
                                let col_x = ws.column_x(i) - vox;
                                if (mx - col_x).abs() < 4.0 {
                                    let left_col_idx = i - 1;
                                    let left_col_width = ws.columns[left_col_idx].effective_width(vw);
                                    self.resize_dragging = Some(left_col_idx);
                                    self.resize_drag_start_x = mx;
                                    self.resize_drag_start_width = left_col_width;
                                    started_drag = true;
                                    break;
                                }
                            }

                            if !started_drag {
                                let tiles = self.workspaces.active_mut().visible_tiles();
                                let clicked_pane = tiles.iter()
                                    .find(|(_, r, _)| r.contains(mx, my))
                                    .map(|(id, _, _)| *id);
                                if let Some(target_pane) = clicked_pane {
                                    let ws = self.workspaces.active_mut();
                                    for col_idx in 0..ws.columns.len() {
                                        if ws.columns[col_idx].pane_id == target_pane {
                                            ws.active_column_idx = col_idx;
                                            break;
                                        }
                                    }
                                    self.animate_to_active();
                                }
                            }
                        }
                    } else {
                        if self.resize_dragging.is_some() {
                            self.resize_dragging = None;
                            self.snap_all_col_widths();
                            if let Some(w) = &self.window {
                                w.set_cursor(winit::window::CursorIcon::Default);
                            }
                        }
                        self.overview_dragging = false;
                        self.drag_last_pos = None;
                    }
                    if let Some(w) = &self.window { w.request_redraw(); }
                }
            }

            WindowEvent::MouseWheel { delta, phase, .. } => {
                if self.overview_active {
                    let dy = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y as f64 * 0.05,
                        MouseScrollDelta::PixelDelta(pos) => pos.y * 0.001,
                    };
                    let cur_zoom = self.overview_zoom.value();
                    let new_zoom = (cur_zoom + dy).clamp(0.05, 1.0);
                    let omega = self.config.animation.speed;
                    if new_zoom >= self.config.animation.zoom_threshold as f64 {
                        self.overview_active = false;
                        self.overview_zoom.animate_to(1.0, omega);
                        self.animate_to_active();
                    } else {
                        self.overview_zoom.animate_to(new_zoom, omega);
                    }
                } else {
                    let scroll_mult = self.config.input.scroll_multiplier;
                    let dx = match delta {
                        MouseScrollDelta::LineDelta(x, _) => x as f64 * scroll_mult,
                        MouseScrollDelta::PixelDelta(pos) => pos.x,
                    };
                    match phase {
                        TouchPhase::Started => { self.view_offset_x.begin_gesture(); }
                        TouchPhase::Moved => {
                            self.view_offset_x.update_gesture(dx);
                            let val = self.view_offset_x.value() as f32;
                            for ws in &mut self.workspaces.rows {
                                ws.view_offset_x = val;
                            }
                        }
                        TouchPhase::Ended | TouchPhase::Cancelled => {
                            let t = self.workspaces.active_mut().target_offset_for_active();
                            let speed = self.config.animation.speed;
                            self.view_offset_x.end_gesture(t as f64, speed);
                        }
                    }
                }
                if let Some(w) = &self.window { w.request_redraw(); }
            }

            WindowEvent::Ime(ime) => {
                match ime {
                    Ime::Commit(text) => {
                        self.ime_preedit_active = false;
                        if let Some(pid) = self.workspaces.active_mut().active_pane_id() {
                            self.send(ClientMessage::Input { pane_id: pid, data: text.into_bytes() });
                        }
                        if let Some(w) = &self.window { w.request_redraw(); }
                    }
                    Ime::Preedit(text, _cursor) => {
                        self.ime_preedit_active = !text.is_empty();
                    }
                    Ime::Enabled | Ime::Disabled => {}
                }
            }

            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if (scale_factor - self.dpi_scale).abs() > 0.01 {
                    self.dpi_scale = scale_factor;
                    if let Some(renderer) = &mut self.renderer {
                        let fmt = renderer.surface_format();
                        let atlas = GlyphAtlas::new(
                            &renderer.device, fmt,
                            &mut renderer.text.font_system, self.config.font.size,
                            scale_factor, &self.config.font.family,
                            &self.config.render,
                        );
                        log::info!("DPI changed: scale={:.2} cell={:.1}x{:.1}", scale_factor, atlas.cell_width, atlas.cell_height);
                        let bar_h = atlas.cell_height + self.config.statusbar.height_padding;
                        let (w, h) = renderer.surface_size();
                        self.workspaces.resize_view(ViewSize {
                            width: w as f32, height: h as f32 - bar_h,
                        });
                        self.glyph_atlas = Some(atlas);
                        self.cached_views.clear();
                    }
                    if let Some(w) = &self.window { w.request_redraw(); }
                }
            }

            WindowEvent::RedrawRequested => self.render(),

            _ => {}
        }
    }
}

fn key_event_to_pty_bytes(event: &winit::event::KeyEvent, ctrl: bool) -> Vec<u8> {
    if ctrl
        && let Key::Character(c) = &event.logical_key {
            let ch = c.as_str();
            if ch.len() == 1 {
                let byte = ch.as_bytes()[0];
                if byte.is_ascii_lowercase() { return vec![byte - b'a' + 1]; }
                if byte.is_ascii_uppercase() { return vec![byte - b'A' + 1]; }
                return match byte {
                    b'[' => vec![0x1b], b'\\' => vec![0x1c], b']' => vec![0x1d],
                    b'^' => vec![0x1e], b'_' => vec![0x1f], b'@' => vec![0x00],
                    _ => vec![],
                };
            }
        }

    if let Key::Named(key) = &event.logical_key { match key {
        NamedKey::Enter => return vec![b'\r'],
        NamedKey::Backspace => return vec![0x7f],
        NamedKey::Tab => return vec![b'\t'],
        NamedKey::Escape => return vec![0x1b],
        NamedKey::Space => return vec![b' '],
        NamedKey::ArrowUp => return b"\x1b[A".to_vec(),
        NamedKey::ArrowDown => return b"\x1b[B".to_vec(),
        NamedKey::ArrowRight => return b"\x1b[C".to_vec(),
        NamedKey::ArrowLeft => return b"\x1b[D".to_vec(),
        NamedKey::Home => return b"\x1b[H".to_vec(),
        NamedKey::End => return b"\x1b[F".to_vec(),
        NamedKey::PageUp => return b"\x1b[5~".to_vec(),
        NamedKey::PageDown => return b"\x1b[6~".to_vec(),
        NamedKey::Delete => return b"\x1b[3~".to_vec(),
        NamedKey::Insert => return b"\x1b[2~".to_vec(),
        NamedKey::F1 => return b"\x1bOP".to_vec(),
        NamedKey::F2 => return b"\x1bOQ".to_vec(),
        NamedKey::F3 => return b"\x1bOR".to_vec(),
        NamedKey::F4 => return b"\x1bOS".to_vec(),
        NamedKey::F5 => return b"\x1b[15~".to_vec(),
        NamedKey::F6 => return b"\x1b[17~".to_vec(),
        NamedKey::F7 => return b"\x1b[18~".to_vec(),
        NamedKey::F8 => return b"\x1b[19~".to_vec(),
        NamedKey::F9 => return b"\x1b[20~".to_vec(),
        NamedKey::F10 => return b"\x1b[21~".to_vec(),
        NamedKey::F11 => return b"\x1b[23~".to_vec(),
        NamedKey::F12 => return b"\x1b[24~".to_vec(),
        _ => {}
    } }

    if let Some(text) = event.text_with_all_modifiers() {
        let s: &str = text;
        if !s.is_empty() {
            return s.as_bytes().to_vec();
        }
    }

    vec![]
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,wgpu_hal=warn,wgpu_core=warn,naga=warn"),
    ).init();

    let config = CiriConfig::load().unwrap_or_default();
    log::info!("config: font={} size={}", config.font.family, config.font.size);

    let event_loop = EventLoop::new()?;
    let mut app = App::new(config);
    event_loop.run_app(&mut app)?;
    Ok(())
}
