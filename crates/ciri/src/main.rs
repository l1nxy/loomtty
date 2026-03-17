use anyhow::Result;
use ciri_anim::animation::ViewOffset;
use ciri_config::config::CiriConfig;
use ciri_config::theme::ThemeConfig;
use ciri_input::action::Action;
use ciri_input::leader::InputHandler;
use ciri_layout::column::ColumnWidth;
use ciri_layout::geometry::{Rect as GeoRect, ViewSize};
use ciri_layout::workspace_set::WorkspaceSet;
use ciri_render::glyph_cache::{GlyphAtlas, GlyphInstance};
use ciri_render::rect::Rect;
use ciri_render::renderer::Renderer;
use ciri_render::terminal::{self, TerminalView};
use ciri_term::pane::Pane;
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
    panes: HashMap<u64, Pane>,
    next_pane_id: u64,
    input: InputHandler,
    /// Global horizontal scroll offset (shared across all workspaces).
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
    // Reusable render buffers to avoid per-frame allocation
    bg_rects_buf: Vec<Rect>,
    glyph_buf: Vec<GlyphInstance>,
    /// True when IME has active preedit text — KeyboardInput is suppressed.
    ime_preedit_active: bool,
    /// Last IME cursor position, for change detection.
    last_ime_pos: Option<(i32, i32)>,
}

impl App {
    fn new(config: CiriConfig) -> Self {
        let frame_interval = Duration::from_millis(config.render.frame_interval_ms);
        let initial_view = ViewSize {
            width: config.window.width as f32,
            height: config.window.height as f32,
        };
        let input = InputHandler::new(
            Duration::from_millis(config.input.leader_timeout_ms),
            Duration::from_millis(config.input.double_tap_window_ms),
        );
        App {
            config,
            frame_interval,
            window: None,
            renderer: None,
            glyph_atlas: None,
            dpi_scale: 1.0,
            workspaces: WorkspaceSet::new(initial_view),
            panes: HashMap::new(),
            next_pane_id: 1,
            input,
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
        }
    }

    /// Find which (workspace_row, pane_id) is under the screen position (mx, my),
    /// accounting for the current zoom level.
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
                // Find which workspace row this pane belongs to
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

    /// Status bar height in pixels. Must be subtracted from viewport for usable area.
    fn status_bar_height(&self) -> f32 {
        let cell_h = self.glyph_atlas.as_ref()
            .map(|a| a.cell_height)
            // Fallback for pre-atlas window resize events.
            .unwrap_or(self.config.font.size * 1.2);
        cell_h + self.config.statusbar.height_padding
    }

    fn create_pane(&mut self) -> Option<u64> {
        let id = self.next_pane_id;
        self.next_pane_id += 1;
        let (cols, rows) = self.default_grid_size();
        match Pane::new(id, cols, rows, &self.config.terminal.shell) {
            Ok(pane) => { self.panes.insert(id, pane); Some(id) }
            Err(e) => { log::error!("failed to create pane: {e}"); None }
        }
    }

    fn default_grid_size(&self) -> (u16, u16) {
        if let Some(atlas) = &self.glyph_atlas {
            let col_w = self.workspaces.active().view_size.width * 0.5;
            let col_h = self.workspaces.active().view_size.height;
            let pad = self.total_inset();
            atlas.grid_size(col_w - pad, col_h - pad)
        } else {
            (self.config.terminal.default_cols, self.config.terminal.default_rows)
        }
    }

    fn resize_panes_to_layout(&mut self) {
        let Some(atlas) = &self.glyph_atlas else { return };
        let pad = self.total_inset();
        let tiles = self.workspaces.active_mut().visible_tiles();
        for (pane_id, rect, _) in &tiles {
            if let Some(pane) = self.panes.get_mut(pane_id) {
                let (cols, rows) = atlas.grid_size(rect.w - pad, rect.h - pad);
                pane.resize(cols, rows);
            }
        }
    }

    /// Snap all columns in ALL workspaces to their target width (no animation).
    /// Ensures every workspace has correct rendered_width regardless of which is active.
    fn snap_all_col_widths(&mut self) {
        let vw = self.workspaces.view_size.width;
        self.col_widths.clear();
        for ws in &mut self.workspaces.rows {
            for col in &mut ws.columns {
                col.snap_width(vw);
            }
        }
    }

    fn switch_to_workspace(&mut self, idx: usize) {
        let old = self.workspaces.active_workspace_idx();
        self.workspaces.switch_to(idx);
        if old != idx {
            self.snap_all_col_widths();
            self.cached_views.clear();
            self.animate_to_active();
            self.resize_panes_to_layout();
        }
    }

    fn handle_action(&mut self, action: Action) {
        match action {
            Action::NewColumnRight => {
                if let Some(id) = self.create_pane() {
                    self.workspaces.active_mut().add_column_right(id);
                    self.snap_all_col_widths();
                    self.resize_panes_to_layout();
                    self.animate_to_active();
                }
            }
            Action::NewRowBelow => {
                if let Some(id) = self.create_pane() {
                    self.workspaces.add_row_below(id);
                    self.view_offset_x.jump_to(0.0);
                    self.snap_all_col_widths();
                    self.animate_to_active();
                    self.resize_panes_to_layout();
                }
            }
            Action::ClosePane => {
                // Remember where the remaining columns were before closing
                let ws = self.workspaces.active_mut();
                let closing_idx = ws.active_column_idx;
                let closing_width = ws.columns.get(closing_idx)
                    .map(|c| c.effective_width(ws.view_size.width) + ws.column_gap)
                    .unwrap_or(0.0);

                if let Some(pane_id) = self.workspaces.active_mut().close_active_pane() {
                    self.panes.remove(&pane_id);
                    self.cached_views.remove(&pane_id);
                    self.snap_all_col_widths();

                    if self.workspaces.active().is_empty() {
                        self.workspaces.cleanup_empty();
                        self.snap_all_col_widths();
                        self.animate_to_active();
                        self.resize_panes_to_layout();
                    } else {
                        let cur = self.view_offset_x.value();
                        self.view_offset_x.jump_to(cur - closing_width as f64);
                        self.animate_to_active();
                        self.resize_panes_to_layout();
                    }
                }
            }
            Action::FocusLeft => { self.workspaces.active_mut().focus_left(); self.animate_to_active(); }
            Action::FocusRight => { self.workspaces.active_mut().focus_right(); self.animate_to_active(); }
            Action::FocusDown => {
                self.workspaces.focus_down();
                self.snap_all_col_widths();
                self.animate_to_active();
                self.resize_panes_to_layout();
            }
            Action::FocusUp => {
                self.workspaces.focus_up();
                self.snap_all_col_widths();
                self.animate_to_active();
                self.resize_panes_to_layout();
            }
            Action::MovePaneLeft => {
                self.workspaces.active_mut().move_pane_left();
                self.resize_panes_to_layout();
                self.animate_to_active();
            }
            Action::MovePaneRight => {
                self.workspaces.active_mut().move_pane_right();
                self.resize_panes_to_layout();
                self.animate_to_active();
            }
            Action::ColumnWidthOneThird => {
                self.workspaces.active_mut().set_active_column_width(ColumnWidth::Proportion(1.0 / 3.0));
                self.resize_panes_to_layout(); self.animate_to_active();
            }
            Action::ColumnWidthHalf => {
                self.workspaces.active_mut().set_active_column_width(ColumnWidth::Proportion(0.5));
                self.resize_panes_to_layout(); self.animate_to_active();
            }
            Action::ColumnWidthTwoThirds => {
                self.workspaces.active_mut().set_active_column_width(ColumnWidth::Proportion(2.0 / 3.0));
                self.resize_panes_to_layout(); self.animate_to_active();
            }
            Action::ColumnWidthFull => {
                self.workspaces.active_mut().set_active_column_width(ColumnWidth::Proportion(1.0));
                self.resize_panes_to_layout(); self.animate_to_active();
            }
            Action::SwitchWorkspace(idx) => {
                self.switch_to_workspace(idx);
            }
            Action::ToggleOverview => {
                self.overview_active = !self.overview_active;
                let omega = self.config.animation.speed;
                if self.overview_active {
                    // Compute zoom to fit ALL workspaces
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
                    let zoom = zoom_x.min(zoom_y).min(1.0) * fit;
                    self.overview_zoom.animate_to(zoom as f64, omega);
                    self.view_offset_x.animate_to(0.0, omega);
                    self.view_offset_y.animate_to(0.0, omega);
                } else {
                    self.overview_zoom.animate_to(1.0, omega);
                    self.animate_to_active();
                }
            }
            Action::SendLeaderKey => {
                if let Some(pid) = self.workspaces.active_mut().active_pane_id() {
                    if let Some(pane) = self.panes.get(&pid) {
                        pane.write_to_pty(&[0x17]);
                    }
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

    /// Sync col_widths animations for within-workspace width changes (e.g. adding columns).
    /// Only animates when col_widths already has non-zero values from the same workspace.
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
            if self.config.animation.enabled {
                if current == 0.0 {
                    self.col_widths[i].jump_to(target);
                } else if (current - target).abs() > 1.0 && !self.col_widths[i].is_animating() {
                    self.col_widths[i].animate_to(target, omega);
                }
            } else {
                self.col_widths[i].jump_to(target);
            }
        }

        // Update rendered_width from col_widths
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
        // Write the SAME view_offset_x to ALL workspaces (unified coordinate system)
        let vox = self.view_offset_x.value() as f32;
        for ws in &mut self.workspaces.rows {
            ws.view_offset_x = vox;
        }
        self.workspaces.view_offset_y = self.view_offset_y.value() as f32;

        // Only advance col_widths if there are active animations
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

        // Pre-parse colors used per tile
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

            // Border rect
            bg_rects.push(Rect {
                x: tr.x, y: tr.y, w: tr.w, h: tr.h,
                color: if *is_active { active_border } else { inactive_border },
            });
            // Inner background rect
            bg_rects.push(Rect {
                x: tr.x + border_w * zoom, y: tr.y + border_w * zoom,
                w: tr.w - border_w * zoom * 2.0, h: tr.h - border_w * zoom * 2.0,
                color: bg_color,
            });

            let Some(view) = self.cached_views.get(pane_id) else { continue };
            let inner_x = tr.x + (border_w + padding) * zoom;
            let inner_y = tr.y + (border_w + padding) * zoom;

            // Clip bg rects to tile bounds
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

            // Clip cursor to tile bounds
            if let Some(cursor) = &view.cursor_rect {
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

            // Clip glyphs to tile bounds, adjusting UVs proportionally
            glyphs.extend(view.glyph_instances.iter().filter_map(|g| {
                let sx = inner_x + g.px * zoom;
                let sy = inner_y + g.py * zoom;
                let gw = g.glyph_w * zoom;
                let gh = g.glyph_h * zoom;

                if gw <= 0.0 || gh <= 0.0 { return None; }

                // Fast path: glyph fully inside tile
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

        // Update terminal views for dirty panes
        for (pane_id, _, _) in &tiles {
            let is_dirty = self.panes.get(pane_id).is_some_and(|p| p.dirty);
            if is_dirty || !self.cached_views.contains_key(pane_id) {
                if let Some(pane) = self.panes.get_mut(pane_id) {
                    let term = pane.term.lock().unwrap();
                    let view = terminal::build_terminal_view(
                        &*term, atlas, &mut renderer.text.font_system, &renderer.queue,
                        &self.config,
                    );
                    drop(term);
                    pane.dirty = false;
                    self.cached_views.insert(*pane_id, view);
                }
            }
        }

        // Update IME cursor area only when position changes
        if let Some(window) = &self.window {
            if let Some(active_pid) = self.workspaces.active().active_pane_id() {
                if let Some((_, tile_rect, _)) = tiles.iter().find(|(id, _, _)| *id == active_pid) {
                    if let Some(view) = self.cached_views.get(&active_pid) {
                        if let Some(cursor) = &view.cursor_rect {
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
                    }
                }
            }
        }

        // Build scene
        let mut bg_rects = std::mem::take(&mut self.bg_rects_buf);
        let mut glyphs = std::mem::take(&mut self.glyph_buf);
        bg_rects.clear();
        glyphs.clear();

        self.build_tiles(&tiles, zoom, vw_f, vh_f, &mut bg_rects, &mut glyphs);
        self.build_status_bar(vw_f, vh_f, &mut bg_rects, &mut glyphs);

        // Submit to GPU
        let clear_color = ThemeConfig::parse_color(&self.config.theme.ui_background);
        let renderer = self.renderer.as_mut().unwrap();
        let atlas = self.glyph_atlas.as_mut().unwrap();
        Self::submit_frame(renderer, atlas, clear_color, &bg_rects, &glyphs);

        // Return buffers for reuse
        self.bg_rects_buf = bg_rects;
        self.glyph_buf = glyphs;

        if animating {
            if let Some(w) = &self.window { w.request_redraw(); }
        }
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
        if let Some(entry) = atlas.ensure_char(ch, font_system, queue) {
            if entry.width > 0 && entry.height > 0 {
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
}

impl ApplicationHandler for App {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + self.frame_interval));

        if matches!(cause, StartCause::ResumeTimeReached { .. } | StartCause::Poll) {
            let mut needs_redraw = self.view_offset_x.is_animating()
                || self.view_offset_y.is_animating()
                || self.overview_zoom.is_animating()
                || self.col_widths.iter().any(|v| v.is_animating());

            for pane in self.panes.values_mut() {
                if pane.process_pty_output() { needs_redraw = true; }
            }

            let dead_ids: Vec<u64> = self.panes.iter()
                .filter(|(_, p)| p.exited)
                .map(|(id, _)| *id)
                .collect();
            if !dead_ids.is_empty() {
                // Calculate width of columns being removed that are LEFT of the
                // active column in the active workspace (for camera compensation).
                let ws = self.workspaces.active_mut();
                let gap = ws.column_gap;
                let vw = ws.view_size.width;
                let active_idx = ws.active_column_idx;
                let mut left_removed_width: f32 = 0.0;
                for (i, col) in ws.columns.iter().enumerate() {
                    if i < active_idx && dead_ids.contains(&col.pane_id) {
                        left_removed_width += col.effective_width(vw) + gap;
                    }
                }
                // Also check if the active column itself is dying
                let active_dying = ws.columns.get(active_idx)
                    .is_some_and(|c| dead_ids.contains(&c.pane_id));
                if active_dying {
                    // Active column is dying; include its width for camera offset
                    if let Some(c) = ws.columns.get(active_idx) {
                        left_removed_width += c.effective_width(vw) + gap;
                    }
                }

                for id in &dead_ids {
                    self.panes.remove(id);
                    self.cached_views.remove(id);
                    for ws in &mut self.workspaces.rows {
                        ws.close_pane(*id);
                    }
                }
                self.workspaces.cleanup_empty();
                if self.workspaces.active().is_empty() {
                    if let Some(idx) = self.workspaces.rows.iter()
                        .position(|ws| !ws.is_empty())
                    {
                        self.workspaces.active_row = idx;
                    }
                }
                self.snap_all_col_widths();
                // Camera compensation: offset so remaining columns stay on screen
                if left_removed_width > 0.0 {
                    let cur = self.view_offset_x.value();
                    self.view_offset_x.jump_to(cur - left_removed_width as f64);
                }
                self.animate_to_active();
                self.resize_panes_to_layout();
                needs_redraw = true;
            }

            if self.panes.is_empty() && self.window.is_some() {
                self.cached_views.clear();
                self.glyph_atlas = None;
                self.renderer = None;
                self.window = None;
                event_loop.exit();
                return;
            }

            if needs_redraw {
                if let Some(w) = &self.window { w.request_redraw(); }
            }
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
        // Reserve space for status bar at bottom
        let bar_h = atlas.cell_height + self.config.statusbar.height_padding;
        self.workspaces.resize_view(ViewSize { width: w as f32, height: h as f32 - bar_h });

        log::info!("cell: {:.1}x{:.1} (dpi_scale={:.2})", atlas.cell_width, atlas.cell_height, dpi_scale);

        if let Some(id) = self.create_pane() {
            self.workspaces.active_mut().add_column_right(id);
            self.workspaces.active_mut().set_active_column_width(ColumnWidth::Proportion(1.0));
        }

        self.dpi_scale = dpi_scale;
        self.glyph_atlas = Some(atlas);
        self.snap_all_col_widths();
        self.animate_to_active();
        self.resize_panes_to_layout();
        self.window = Some(window);
        self.renderer = Some(renderer);
        self.last_frame = Instant::now();

        self.window.as_ref().unwrap().request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.panes.clear();
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
                for pane in self.panes.values_mut() { pane.dirty = true; }
                self.cached_views.clear();
                self.resize_panes_to_layout();
                self.animate_to_active();
                if let Some(w) = &self.window { w.request_redraw(); }
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed { return; }

                // When IME is composing (preedit active), suppress all keyboard
                // processing. Text will arrive via Ime::Commit instead.
                if self.ime_preedit_active {
                    return;
                }

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
                    let key_lower = key_name.to_lowercase();
                    match key_lower.as_str() {
                        "h" | "left" => { self.workspaces.active_mut().focus_left(); self.animate_to_active(); }
                        "l" | "right" => { self.workspaces.active_mut().focus_right(); self.animate_to_active(); }
                        "k" | "up" => { self.workspaces.focus_up(); self.animate_to_active(); }
                        "j" | "down" => { self.workspaces.focus_down(); self.animate_to_active(); }
                        "x" => { self.handle_action(Action::ClosePane); }
                        "escape" | "enter" | "o" | "tab" => {
                            self.overview_active = false;
                            self.overview_zoom.animate_to(1.0, self.config.animation.speed);
                            self.animate_to_active();
                        }
                        _ => {}
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
                            if !bytes.is_empty() {
                                if let Some(pid) = self.workspaces.active_mut().active_pane_id() {
                                    if let Some(pane) = self.panes.get(&pid) {
                                        pane.write_to_pty(&bytes);
                                    }
                                }
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
                    // Hover: select the pane under cursor
                    if let Some((row_idx, pane_id)) = self.hit_test_overview(mx, my) {
                        self.workspaces.active_row = row_idx;
                        let ws = self.workspaces.active_mut();
                        for (col_idx, col) in ws.columns.iter().enumerate() {
                            if col.pane_id == pane_id {
                                ws.active_column_idx = col_idx;
                                break;
                            }
                        }
                        if let Some(w) = &self.window { w.request_redraw(); }
                    }

                    // Drag panning
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
                }
            }

            WindowEvent::MouseInput { state, button: winit::event::MouseButton::Left, .. } => {
                if let Some((mx, my)) = self.last_mouse_pos {
                    if state == ElementState::Pressed {
                        if self.overview_active {
                            // Click on a panel: select it and exit overview
                            if let Some((row_idx, pane_id)) = self.hit_test_overview(mx, my) {
                                self.workspaces.active_row = row_idx;
                                let ws = self.workspaces.active_mut();
                                for (col_idx, col) in ws.columns.iter().enumerate() {
                                    if col.pane_id == pane_id {
                                        ws.active_column_idx = col_idx;
                                        break;
                                    }
                                }
                                self.overview_active = false;
                                self.overview_zoom.animate_to(1.0, self.config.animation.speed);
                                self.animate_to_active();
                            } else {
                                // Click on empty space: start drag panning
                                self.overview_dragging = true;
                                self.drag_last_pos = Some((mx, my));
                            }
                        } else {
                            // Normal mode: click to focus pane
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
                    } else {
                        // Mouse released
                        self.overview_dragging = false;
                        self.drag_last_pos = None;
                    }
                    if let Some(w) = &self.window { w.request_redraw(); }
                }
            }

            WindowEvent::MouseWheel { delta, phase, .. } => {
                if self.overview_active {
                    // In overview: scroll wheel zooms
                    let dy = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y as f64 * 0.05,
                        MouseScrollDelta::PixelDelta(pos) => pos.y * 0.001,
                    };
                    let cur_zoom = self.overview_zoom.value();
                    let new_zoom = (cur_zoom + dy).clamp(0.05, 1.0);
                    let omega = self.config.animation.speed;
                    if new_zoom >= self.config.animation.zoom_threshold as f64 {
                        // Zoomed back to ~1.0: exit overview
                        self.overview_active = false;
                        self.overview_zoom.animate_to(1.0, omega);
                        self.animate_to_active();
                    } else {
                        self.overview_zoom.animate_to(new_zoom, omega);
                    }
                } else {
                    // Normal mode: horizontal scroll gesture
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
                        // Write committed IME text to active pane's PTY
                        if let Some(pid) = self.workspaces.active_mut().active_pane_id() {
                            if let Some(pane) = self.panes.get(&pid) {
                                pane.write_to_pty(text.as_bytes());
                            }
                        }
                        if let Some(w) = &self.window { w.request_redraw(); }
                    }
                    Ime::Preedit(text, _cursor) => {
                        // Track whether IME is composing — if so, KeyboardInput is suppressed.
                        self.ime_preedit_active = !text.is_empty();
                    }
                    Ime::Enabled | Ime::Disabled => {}
                }
            }

            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if (scale_factor - self.dpi_scale).abs() > 0.01 {
                    self.dpi_scale = scale_factor;
                    // Rebuild glyph atlas with new DPI
                    if let Some(renderer) = &mut self.renderer {
                        let fmt = renderer.surface_format();
                        let atlas = GlyphAtlas::new(
                            &renderer.device, fmt,
                            &mut renderer.text.font_system, self.config.font.size,
                            scale_factor, &self.config.font.family,
                            &self.config.render,
                        );
                        log::info!("DPI changed: scale={:.2} cell={:.1}x{:.1}", scale_factor, atlas.cell_width, atlas.cell_height);
                        // Update view size with new status bar height
                        let bar_h = atlas.cell_height + self.config.statusbar.height_padding;
                        let (w, h) = renderer.surface_size();
                        self.workspaces.resize_view(ViewSize {
                            width: w as f32, height: h as f32 - bar_h,
                        });
                        self.glyph_atlas = Some(atlas);
                        self.cached_views.clear();
                        self.resize_panes_to_layout();
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
    // Ctrl+key combinations → control codes
    if ctrl {
        if let Key::Character(c) = &event.logical_key {
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
    }

    // Named keys → escape sequences (checked before text to avoid consuming
    // control chars like \x08 from text_with_all_modifiers).
    match &event.logical_key {
        Key::Named(key) => match key {
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
        },
        _ => {}
    }

    // Text input: use text_with_all_modifiers (like Alacritty) for accurate
    // character data. This is the path for regular typing (a-z, symbols, etc.).
    if let Some(text) = event.text_with_all_modifiers() {
        let s: &str = &text;
        if !s.is_empty() {
            return s.as_bytes().to_vec();
        }
    }

    // Fallback: nothing to send
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
