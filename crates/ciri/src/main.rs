use anyhow::Result;
use ciri_anim::animation::ViewOffset;
use ciri_config::config::CiriConfig;
use ciri_config::theme::ThemeConfig;
use ciri_input::action::Action;
use ciri_input::leader::InputHandler;
use ciri_layout::column::ColumnWidth;
use ciri_layout::geometry::ViewSize;
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
use winit::event::{ElementState, MouseScrollDelta, StartCause, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId};

struct App {
    config: CiriConfig,
    frame_interval: Duration,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    glyph_atlas: Option<GlyphAtlas>,
    workspaces: WorkspaceSet,
    panes: HashMap<u64, Pane>,
    next_pane_id: u64,
    input: InputHandler,
    view_offset_x: ViewOffset,
    view_offset_y: ViewOffset,
    col_widths: Vec<ViewOffset>,
    last_frame: Instant,
    modifiers: ModifiersState,
    cached_views: HashMap<u64, TerminalView>,
    last_mouse_pos: Option<(f32, f32)>,
    overview_active: bool,
    overview_zoom: ViewOffset,
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
            overview_zoom: {
                let mut v = ViewOffset::new();
                v.jump_to(1.0);
                v
            },
        }
    }

    fn create_pane(&mut self) -> u64 {
        let id = self.next_pane_id;
        self.next_pane_id += 1;
        let (cols, rows) = self.default_grid_size();
        match Pane::new(id, cols, rows) {
            Ok(pane) => { self.panes.insert(id, pane); }
            Err(e) => { log::error!("failed to create pane: {e}"); }
        }
        id
    }

    fn default_grid_size(&self) -> (u16, u16) {
        if let Some(atlas) = &self.glyph_atlas {
            let col_w = self.workspaces.active().view_size.width * 0.5;
            let col_h = self.workspaces.active().view_size.height;
            let pad = self.config.appearance.padding * 2.0 + self.config.appearance.border_width * 2.0;
            atlas.grid_size(col_w - pad, col_h - pad)
        } else {
            (self.config.terminal.default_cols, self.config.terminal.default_rows)
        }
    }

    fn resize_panes_to_layout(&mut self) {
        let Some(atlas) = &self.glyph_atlas else { return };
        let pad = self.config.appearance.padding * 2.0 + self.config.appearance.border_width * 2.0;
        let tiles = self.workspaces.active_mut().visible_tiles();
        for (pane_id, rect, _) in &tiles {
            if let Some(pane) = self.panes.get_mut(pane_id) {
                let (cols, rows) = atlas.grid_size(rect.w - pad, rect.h - pad);
                pane.resize(cols, rows);
            }
        }
    }

    fn switch_to_workspace(&mut self, idx: usize) {
        let old = self.workspaces.active_workspace_idx();
        self.workspaces.switch_to(idx);
        if old != idx {
            self.col_widths.clear();
            self.cached_views.clear();
            self.resize_panes_to_layout();
            self.animate_to_active();
        }
    }

    fn handle_action(&mut self, action: Action) {
        match action {
            Action::NewColumnRight => {
                if self.workspaces.active_mut().columns.len() == 1 {
                    self.workspaces.active_mut().set_active_column_width(ColumnWidth::Proportion(0.5));
                }
                let id = self.create_pane();
                self.workspaces.active_mut().add_column_right(id);
                self.resize_panes_to_layout();
                self.animate_to_active();
            }
            Action::NewRowBelow => {
                let id = self.create_pane();
                self.workspaces.add_row_below(id);
                self.view_offset_x.jump_to(0.0);
                self.col_widths.clear();
                self.resize_panes_to_layout();
                self.animate_to_active();
            }
            Action::ClosePane => {
                if let Some(pane_id) = self.workspaces.active_mut().close_active_pane() {
                    self.panes.remove(&pane_id);
                    self.cached_views.remove(&pane_id);
                    self.resize_panes_to_layout();
                    self.animate_to_active();
                    if self.workspaces.active().is_empty() {
                        self.workspaces.cleanup_empty();
                        let target = self.workspaces.active_mut().target_offset_for_active();
                        self.view_offset_x.jump_to(target as f64);
                        self.col_widths.clear();
                        self.resize_panes_to_layout();
                    }
                }
            }
            Action::FocusLeft => { self.workspaces.active_mut().focus_left(); self.animate_to_active(); }
            Action::FocusRight => { self.workspaces.active_mut().focus_right(); self.animate_to_active(); }
            Action::FocusDown => {
                self.workspaces.focus_down();
                self.col_widths.clear();
                self.resize_panes_to_layout();
                self.animate_to_active();
            }
            Action::FocusUp => {
                self.workspaces.focus_up();
                self.col_widths.clear();
                self.resize_panes_to_layout();
                self.animate_to_active();
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
                    let ws = self.workspaces.active_mut();
                    let total_w = ws.total_width();
                    let vw = ws.view_size.width;
                    if total_w > 0.0 {
                        let fit = self.config.animation.overview_zoom_fit;
                        let zoom = (vw / total_w).min(1.0) * fit;
                        self.overview_zoom.animate_to(zoom as f64, omega);
                    }
                    self.view_offset_x.animate_to(0.0, omega);
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
            self.workspaces.active_mut().view_offset_x = target_x;
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
    }

    fn render(&mut self) {
        if self.renderer.is_none() || self.glyph_atlas.is_none() {
            return;
        }

        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f64();
        self.last_frame = now;

        let mut animating = self.view_offset_x.advance(dt);
        if self.view_offset_y.advance(dt) { animating = true; }
        if self.overview_zoom.advance(dt) { animating = true; }
        self.workspaces.active_mut().view_offset_x = self.view_offset_x.value() as f32;
        self.workspaces.view_offset_y = self.view_offset_y.value() as f32;

        self.sync_col_animations();
        {
            let ws = self.workspaces.active_mut();
            for (i, col) in ws.columns.iter_mut().enumerate() {
                if i < self.col_widths.len() {
                    if self.col_widths[i].advance(dt) { animating = true; }
                    col.animated_width = Some(self.col_widths[i].value() as f32);
                }
            }
        }

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
        let padding = self.config.appearance.padding;
        let border_w = self.config.appearance.border_width;

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

        let mut all_bg_rects: Vec<Rect> = Vec::new();
        let mut all_glyph_instances: Vec<GlyphInstance> = Vec::new();

        for (pane_id, tile_rect, is_active) in &tiles {
            let tr = if zoom < zoom_threshold {
                let cx = vw_f / 2.0;
                let cy = vh_f / 2.0;
                ciri_layout::geometry::Rect::new(
                    cx + (tile_rect.x - cx) * zoom,
                    cy + (tile_rect.y - cy) * zoom,
                    tile_rect.w * zoom,
                    tile_rect.h * zoom,
                )
            } else {
                *tile_rect
            };

            let border_color = if *is_active {
                ThemeConfig::parse_color(&self.config.appearance.active_border_color)
            } else {
                ThemeConfig::parse_color(&self.config.appearance.inactive_border_color)
            };
            all_bg_rects.push(Rect {
                x: tr.x, y: tr.y, w: tr.w, h: tr.h, color: border_color,
            });
            all_bg_rects.push(Rect {
                x: tr.x + border_w * zoom, y: tr.y + border_w * zoom,
                w: tr.w - border_w * zoom * 2.0, h: tr.h - border_w * zoom * 2.0,
                color: ThemeConfig::parse_color(&self.config.theme.background),
            });

            if let Some(view) = self.cached_views.get(pane_id) {
                let inner_x = tr.x + (border_w + padding) * zoom;
                let inner_y = tr.y + (border_w + padding) * zoom;

                for r in &view.bg_rects {
                    let rx = inner_x + r.x * zoom;
                    let ry = inner_y + r.y * zoom;
                    let rw = r.w * zoom;
                    let rh = r.h * zoom;
                    if rx + rw < tr.x || rx > tr.x + tr.w || ry + rh < tr.y || ry > tr.y + tr.h {
                        continue;
                    }
                    let cx = rx.max(tr.x);
                    let cy = ry.max(tr.y);
                    let cw = (rx + rw).min(tr.x + tr.w) - cx;
                    let ch = (ry + rh).min(tr.y + tr.h) - cy;
                    if cw > 0.0 && ch > 0.0 {
                        all_bg_rects.push(Rect { x: cx, y: cy, w: cw, h: ch, color: r.color });
                    }
                }

                if let Some(cursor) = &view.cursor_rect {
                    all_bg_rects.push(Rect {
                        x: inner_x + cursor.x * zoom, y: inner_y + cursor.y * zoom,
                        w: cursor.w * zoom, h: cursor.h * zoom, color: cursor.color,
                    });
                }

                all_glyph_instances.extend(view.glyph_instances.iter().filter_map(|g| {
                    let sx = inner_x + g.px * zoom;
                    let sy = inner_y + g.py * zoom;
                    let gw = g.glyph_w * zoom;
                    let gh = g.glyph_h * zoom;

                    if sx + gw < tr.x || sx > tr.x + tr.w
                        || sy + gh < tr.y || sy > tr.y + tr.h
                    {
                        return None;
                    }

                    Some(GlyphInstance {
                        pos: [sx / vw_f * 2.0 - 1.0, 1.0 - sy / vh_f * 2.0],
                        size: [gw / vw_f * 2.0, -(gh / vh_f * 2.0)],
                        uv_pos: [g.u0, g.v0],
                        uv_size: [g.u1 - g.u0, g.v1 - g.v0],
                        color: g.color,
                    })
                }));
            }
        }

        // --- Status bar ---
        let bar_height = atlas.cell_height + self.config.statusbar.height_padding;
        let bar_y = vh_f - bar_height;
        all_bg_rects.push(Rect {
            x: 0.0, y: bar_y, w: vw_f, h: bar_height,
            color: ThemeConfig::parse_color(&self.config.statusbar.background_color),
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

        let leader_text_color = ThemeConfig::parse_color(&self.config.statusbar.leader_text_color);
        let normal_text_color = ThemeConfig::parse_color(&self.config.statusbar.text_color);
        let active_mode_color = ThemeConfig::parse_color(&self.config.statusbar.active_mode_color);
        let inactive_text_color = ThemeConfig::parse_color(&self.config.statusbar.inactive_text_color);

        for (i, ch) in status_left.chars().enumerate() {
            if let Some(entry) = atlas.ensure_char(ch, &mut renderer.text.font_system, &renderer.queue) {
                if entry.width > 0 && entry.height > 0 {
                    let sx = i as f32 * cw + entry.bearing_x as f32;
                    let sy = text_y + baseline - entry.bearing_y as f32;
                    all_glyph_instances.push(GlyphInstance {
                        pos: [sx / vw_f * 2.0 - 1.0, 1.0 - sy / vh_f * 2.0],
                        size: [entry.width as f32 / vw_f * 2.0, -(entry.height as f32 / vh_f * 2.0)],
                        uv_pos: [entry.u0, entry.v0],
                        uv_size: [entry.u1 - entry.u0, entry.v1 - entry.v0],
                        color: if self.input.is_awaiting_action() { leader_text_color } else { normal_text_color },
                    });
                }
            }
        }

        let right_start_x = vw_f - status_right.len() as f32 * cw;
        for (i, ch) in status_right.chars().enumerate() {
            if let Some(entry) = atlas.ensure_char(ch, &mut renderer.text.font_system, &renderer.queue) {
                if entry.width > 0 && entry.height > 0 {
                    let sx = right_start_x + i as f32 * cw + entry.bearing_x as f32;
                    let sy = text_y + baseline - entry.bearing_y as f32;
                    let color = if self.overview_active || self.input.is_awaiting_action() {
                        active_mode_color
                    } else {
                        inactive_text_color
                    };
                    all_glyph_instances.push(GlyphInstance {
                        pos: [sx / vw_f * 2.0 - 1.0, 1.0 - sy / vh_f * 2.0],
                        size: [entry.width as f32 / vw_f * 2.0, -(entry.height as f32 / vh_f * 2.0)],
                        uv_pos: [entry.u0, entry.v0],
                        uv_size: [entry.u1 - entry.u0, entry.v1 - entry.v0],
                        color,
                    });
                }
            }
        }

        if self.input.is_awaiting_action() {
            let indicator_h = self.config.statusbar.leader_indicator_height;
            all_bg_rects.push(Rect {
                x: 0.0, y: bar_y - indicator_h, w: vw_f, h: indicator_h,
                color: ThemeConfig::parse_color(&self.config.statusbar.leader_indicator_color),
            });
        }

        // --- GPU rendering ---
        let clear_color = ThemeConfig::parse_color(&self.config.render.clear_color);
        let output = match renderer.surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Lost) => {
                renderer.resize(vw, vh);
                return;
            }
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

            renderer.rects.render(&renderer.queue, &mut pass, &all_bg_rects, vw_f, vh_f);
            atlas.render(&renderer.queue, &mut pass, &all_glyph_instances);
        }

        renderer.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        if animating {
            if let Some(w) = &self.window { w.request_redraw(); }
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
                for id in &dead_ids {
                    self.panes.remove(id);
                    self.cached_views.remove(id);
                    self.workspaces.active_mut().close_pane(*id);
                }
                if !self.workspaces.active_mut().is_empty() {
                    self.resize_panes_to_layout();
                    self.animate_to_active();
                }
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
        let mut renderer = pollster::block_on(Renderer::new(window.clone(), &self.config.render)).expect("renderer init failed");

        let fmt = renderer.surface_format();
        let atlas = GlyphAtlas::new(
            &renderer.device, &renderer.queue, fmt,
            &mut renderer.text.font_system, self.config.font.size,
            &self.config.render,
        );

        let (w, h) = renderer.surface_size();
        self.workspaces.resize_view(ViewSize { width: w as f32, height: h as f32 });

        log::info!("cell: {:.1}x{:.1}", atlas.cell_width, atlas.cell_height);

        let id = self.create_pane();
        self.workspaces.active_mut().add_column_right(id);
        self.workspaces.active_mut().set_active_column_width(ColumnWidth::Proportion(1.0));

        self.glyph_atlas = Some(atlas);
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
                self.workspaces.resize_view(ViewSize {
                    width: size.width as f32, height: size.height as f32,
                });
                for pane in self.panes.values_mut() { pane.dirty = true; }
                self.cached_views.clear();
                self.resize_panes_to_layout();
                self.animate_to_active();
                if let Some(w) = &self.window { w.request_redraw(); }
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed { return; }

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
                self.last_mouse_pos = Some((position.x as f32, position.y as f32));
            }

            WindowEvent::MouseInput { state: ElementState::Pressed, button: winit::event::MouseButton::Left, .. } => {
                if let Some((mx, my)) = self.last_mouse_pos {
                    let tiles = self.workspaces.active_mut().visible_tiles();
                    let mut clicked_pane = None;
                    for (pane_id, tile_rect, _) in &tiles {
                        if mx >= tile_rect.x && mx < tile_rect.x + tile_rect.w
                            && my >= tile_rect.y && my < tile_rect.y + tile_rect.h
                        {
                            clicked_pane = Some(*pane_id);
                            break;
                        }
                    }
                    if let Some(target_pane) = clicked_pane {
                        let ws = self.workspaces.active_mut();
                        for col_idx in 0..ws.columns.len() {
                            if ws.columns[col_idx].pane_id == target_pane {
                                ws.active_column_idx = col_idx;
                                break;
                            }
                        }
                        if self.overview_active {
                            self.overview_active = false;
                            self.overview_zoom.animate_to(1.0, self.config.animation.speed);
                        }
                        self.animate_to_active();
                    }
                    if let Some(w) = &self.window { w.request_redraw(); }
                }
            }

            WindowEvent::MouseWheel { delta, phase, .. } => {
                let scroll_mult = self.config.input.scroll_multiplier;
                let dx = match delta {
                    MouseScrollDelta::LineDelta(x, _) => x as f64 * scroll_mult,
                    MouseScrollDelta::PixelDelta(pos) => pos.x,
                };
                match phase {
                    TouchPhase::Started => self.view_offset_x.begin_gesture(),
                    TouchPhase::Moved => {
                        self.view_offset_x.update_gesture(dx);
                        self.workspaces.active_mut().view_offset_x = self.view_offset_x.value() as f32;
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        let t = self.workspaces.active_mut().target_offset_for_active();
                        self.view_offset_x.end_gesture(t as f64, self.config.animation.speed);
                    }
                }
                if let Some(w) = &self.window { w.request_redraw(); }
            }

            WindowEvent::RedrawRequested => self.render(),

            _ => {}
        }
    }
}

fn key_event_to_pty_bytes(event: &winit::event::KeyEvent, ctrl: bool) -> Vec<u8> {
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

    if let Some(ref text) = event.text {
        let s = text.as_str();
        if !s.is_empty() { return s.as_bytes().to_vec(); }
    }

    match &event.logical_key {
        Key::Named(key) => match key {
            NamedKey::Enter => vec![b'\r'],
            NamedKey::Backspace => vec![0x7f],
            NamedKey::Tab => vec![b'\t'],
            NamedKey::Escape => vec![0x1b],
            NamedKey::Space => vec![b' '],
            NamedKey::ArrowUp => b"\x1b[A".to_vec(),
            NamedKey::ArrowDown => b"\x1b[B".to_vec(),
            NamedKey::ArrowRight => b"\x1b[C".to_vec(),
            NamedKey::ArrowLeft => b"\x1b[D".to_vec(),
            NamedKey::Home => b"\x1b[H".to_vec(),
            NamedKey::End => b"\x1b[F".to_vec(),
            NamedKey::PageUp => b"\x1b[5~".to_vec(),
            NamedKey::PageDown => b"\x1b[6~".to_vec(),
            NamedKey::Delete => b"\x1b[3~".to_vec(),
            NamedKey::Insert => b"\x1b[2~".to_vec(),
            NamedKey::F1 => b"\x1bOP".to_vec(),
            NamedKey::F2 => b"\x1bOQ".to_vec(),
            NamedKey::F3 => b"\x1bOR".to_vec(),
            NamedKey::F4 => b"\x1bOS".to_vec(),
            NamedKey::F5 => b"\x1b[15~".to_vec(),
            NamedKey::F6 => b"\x1b[17~".to_vec(),
            NamedKey::F7 => b"\x1b[18~".to_vec(),
            NamedKey::F8 => b"\x1b[19~".to_vec(),
            NamedKey::F9 => b"\x1b[20~".to_vec(),
            NamedKey::F10 => b"\x1b[21~".to_vec(),
            NamedKey::F11 => b"\x1b[23~".to_vec(),
            NamedKey::F12 => b"\x1b[24~".to_vec(),
            _ => vec![],
        },
        _ => vec![],
    }
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
