use ciri_anim::animation::ViewOffset;
use ciri_config::theme::ThemeConfig;
use ciri_layout::geometry::Rect as GeoRect;
use ciri_protocol::message::*;
use ciri_render::glyph_cache::{GlyphAtlas, GlyphInstance};
use ciri_render::rect::Rect;
use ciri_render::renderer::Renderer;
use ciri_render::terminal;
use std::time::Instant;

use super::App;

impl App {
    pub fn snap_all_col_widths(&mut self) {
        let vw = self.workspaces.view_size.width;
        self.col_widths.clear();
        for ws in &mut self.workspaces.workspaces {
            for col in &mut ws.columns {
                col.snap_width(vw);
            }
        }
    }

    pub fn refresh_overview_zoom(&mut self) {
        if !self.overview_active {
            return;
        }
        let omega = self.config.animation.speed;
        let vw = self.workspaces.view_size.width;
        let vh = self.workspaces.view_size.height;
        let max_w = self
            .workspaces
            .workspaces
            .iter()
            .map(|ws| ws.total_width())
            .fold(0.0f32, f32::max)
            .max(vw);
        let nrows = self
            .workspaces
            .workspaces
            .iter()
            .filter(|ws| !ws.is_empty())
            .count()
            .max(1);
        let total_h =
            nrows as f32 * vh + (nrows.saturating_sub(1)) as f32 * self.workspaces.workspace_gap;
        let fit = self.config.animation.overview_zoom_fit;
        let zoom_x = vw / max_w;
        let zoom_y = vh / total_h;
        let zoom = (zoom_x.min(zoom_y).min(1.0) * fit).max(0.15);
        self.overview_zoom.animate_to(zoom as f64, omega);
    }

    pub fn animate_to_active(&mut self) {
        let omega = self.config.animation.speed;
        let enabled = self.config.animation.enabled;

        let center_strategy = match self.config.layout.center_focused_column {
            ciri_config::config::CenterStrategy::Always => 2,
            ciri_config::config::CenterStrategy::OnOverflow => 1,
            ciri_config::config::CenterStrategy::Never => 0,
        };
        let target_x = self.workspaces.active_mut().target_offset_for_active_with_strategy(center_strategy);
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

    pub fn sync_col_animations(&mut self) {
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

    pub fn advance_animations(&mut self, dt: f64) -> bool {
        let mut animating = self.view_offset_x.advance(dt);
        if self.view_offset_y.advance(dt) {
            animating = true;
        }
        if self.overview_zoom.advance(dt) {
            animating = true;
        }
        let vox = self.view_offset_x.value() as f32;
        for ws in &mut self.workspaces.workspaces {
            ws.view_offset_x = vox;
        }
        self.workspaces.view_offset_y = self.view_offset_y.value() as f32;

        if !self.col_widths.is_empty() {
            self.sync_col_animations();
            let ws = self.workspaces.active_mut();
            for (i, col) in ws.columns.iter_mut().enumerate() {
                if i < self.col_widths.len() {
                    if self.col_widths[i].advance(dt) {
                        animating = true;
                    }
                    col.set_rendered_width(self.col_widths[i].value() as f32);
                }
            }
        }
        animating
    }

    pub fn build_tiles(
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
        let active_border = if self.config.appearance.active_border_color.is_empty() {
            ThemeConfig::parse_color(&self.config.theme.border_active)
        } else {
            ThemeConfig::parse_color(&self.config.appearance.active_border_color)
        };
        let inactive_border = if self.config.appearance.inactive_border_color.is_empty() {
            ThemeConfig::parse_color(&self.config.theme.border_inactive)
        } else {
            ThemeConfig::parse_color(&self.config.appearance.inactive_border_color)
        };
        let inactive_opacity = self.config.appearance.inactive_opacity;
        let bg_color = ThemeConfig::parse_color(&self.config.theme.background);
        let link_color = ThemeConfig::parse_color(&self.config.theme.accent);

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
                x: tr.x,
                y: tr.y,
                w: tr.w,
                h: tr.h,
                color: if *is_active {
                    active_border
                } else {
                    inactive_border
                },
            });
            bg_rects.push(Rect {
                x: tr.x + border_w * zoom,
                y: tr.y + border_w * zoom,
                w: tr.w - border_w * zoom * 2.0,
                h: tr.h - border_w * zoom * 2.0,
                color: bg_color,
            });

            let Some(view) = self.cached_views.get(pane_id) else {
                continue;
            };
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
                    bg_rects.push(Rect {
                        x: c.x,
                        y: c.y,
                        w: c.w,
                        h: c.h,
                        color: r.color,
                    });
                }
            }

            if self.cursor_blink_visible && *is_active {
                for cursor in &view.cursor_rects {
                    let src = GeoRect::new(
                        inner_x + cursor.x * zoom,
                        inner_y + cursor.y * zoom,
                        cursor.w * zoom,
                        cursor.h * zoom,
                    );
                    if let Some(c) = src.intersection(&tr) {
                        bg_rects.push(Rect {
                            x: c.x,
                            y: c.y,
                            w: c.w,
                            h: c.h,
                            color: cursor.color,
                        });
                    }
                }
            }

            // Selection overlay
            if let Some(sel) = &self.selection {
                if sel.pane_id == *pane_id {
                    if let Some(grid) = self.pane_grids.get(pane_id) {
                        let (cw, ch) = self.cell_dimensions();
                        let (start, end) = if sel.start.1 < sel.end.1
                            || (sel.start.1 == sel.end.1 && sel.start.0 <= sel.end.0)
                        {
                            (sel.start, sel.end)
                        } else {
                            (sel.end, sel.start)
                        };
                        let sel_color = [0.3, 0.5, 0.8, 0.3];
                        for buf_row in start.1..=end.1 {
                            let viewport_row = match grid.buffer_to_viewport_row(buf_row) {
                                Some(r) => r,
                                None => continue,
                            };
                            let left = if buf_row == start.1 { start.0 } else { 0 };
                            let right = if buf_row == end.1 {
                                end.0
                            } else {
                                grid.cols.saturating_sub(1)
                            };
                            let sx = inner_x + left as f32 * cw * zoom;
                            let sy = inner_y + viewport_row as f32 * ch * zoom;
                            let sw = (right - left + 1) as f32 * cw * zoom;
                            let sh = ch * zoom;
                            let src = GeoRect::new(sx, sy, sw, sh);
                            if let Some(c) = src.intersection(&tr) {
                                bg_rects.push(Rect {
                                    x: c.x,
                                    y: c.y,
                                    w: c.w,
                                    h: c.h,
                                    color: sel_color,
                                });
                            }
                        }
                    }
                }
            }

            if let Some(link) = &self.hovered_link {
                if link.pane_id == *pane_id {
                    if let Some(grid) = self.pane_grids.get(pane_id) {
                        if let Some(viewport_row) = grid.buffer_to_viewport_row(link.start.1) {
                            let (cw, ch) = self.cell_dimensions();
                            let underline_h = (zoom.max(1.0)).clamp(1.0, 2.0);
                            let sx = inner_x + link.start.0 as f32 * cw * zoom;
                            let sy = inner_y + (viewport_row as f32 + 1.0) * ch * zoom
                                - underline_h
                                - zoom;
                            let sw = (link.end.0 - link.start.0 + 1) as f32 * cw * zoom;
                            let src = GeoRect::new(sx, sy, sw, underline_h);
                            if let Some(c) = src.intersection(&tr) {
                                bg_rects.push(Rect {
                                    x: c.x,
                                    y: c.y,
                                    w: c.w,
                                    h: c.h,
                                    color: link_color,
                                });
                            }
                        }
                    }
                }
            }

            // Search match highlights
            if let Some(search) = &self.search_state {
                if search.pane_id == *pane_id {
                    if let Some(grid) = self.pane_grids.get(pane_id) {
                        let (cw, ch) = self.cell_dimensions();
                        let vp_top = grid.viewport_top();
                        let vp_bottom = vp_top + grid.rows as usize;

                        for (match_idx, m) in search.matches.iter().enumerate() {
                            if m.buffer_row < vp_top || m.buffer_row >= vp_bottom {
                                continue;
                            }
                            let vp_row = m.buffer_row - vp_top;

                            let sx = inner_x + m.start_col as f32 * cw * zoom;
                            let sy = inner_y + vp_row as f32 * ch * zoom;
                            let sw = (m.end_col - m.start_col + 1) as f32 * cw * zoom;
                            let sh = ch * zoom;

                            let color = if match_idx == search.current_match_idx {
                                [1.0, 0.6, 0.0, 0.5] // orange for current match
                            } else {
                                [1.0, 1.0, 0.0, 0.3] // yellow for other matches
                            };

                            let src = GeoRect::new(sx, sy, sw, sh);
                            if let Some(c) = src.intersection(&tr) {
                                bg_rects.push(Rect {
                                    x: c.x,
                                    y: c.y,
                                    w: c.w,
                                    h: c.h,
                                    color,
                                });
                            }
                        }
                    }
                }
            }

            // Dim factor for inactive panes: multiply glyph colors to reduce brightness
            let dim = if *is_active { 1.0 } else { inactive_opacity };
            // Apply open animation opacity on top of dim
            let open_opacity = self.pane_open_opacity.get(pane_id).copied().unwrap_or(1.0);
            let dim = dim * open_opacity;

            glyphs.extend(view.glyph_instances.iter().filter_map(|g| {
                let sx = (inner_x + g.px * zoom).round();
                let sy = (inner_y + g.py * zoom).round();
                let gw = (g.glyph_w * zoom).round();
                let gh = (g.glyph_h * zoom).round();

                if gw <= 0.0 || gh <= 0.0 {
                    return None;
                }

                let color = [
                    g.color[0] * dim,
                    g.color[1] * dim,
                    g.color[2] * dim,
                    g.color[3],
                ];

                if sx >= tr.x && sy >= tr.y && sx + gw <= tr.x + tr.w && sy + gh <= tr.y + tr.h {
                    return Some(GlyphInstance {
                        pos: [sx / vw * 2.0 - 1.0, 1.0 - sy / vh * 2.0],
                        size: [gw / vw * 2.0, -(gh / vh * 2.0)],
                        uv_pos: [g.u0, g.v0],
                        uv_size: [g.u1 - g.u0, g.v1 - g.v0],
                        color,
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
                    color,
                })
            }));

            // Dimming overlay for inactive panes: semi-transparent black rect over pane content
            if !*is_active && inactive_opacity < 1.0 {
                let overlay_alpha = 1.0 - inactive_opacity;
                bg_rects.push(Rect {
                    x: tr.x + border_w * zoom,
                    y: tr.y + border_w * zoom,
                    w: tr.w - border_w * zoom * 2.0,
                    h: tr.h - border_w * zoom * 2.0,
                    color: [0.0, 0.0, 0.0, overlay_alpha],
                });
            }

            // Fade-in overlay for newly opened panes
            if open_opacity < 1.0 {
                let overlay_alpha = 1.0 - open_opacity;
                bg_rects.push(Rect {
                    x: tr.x,
                    y: tr.y,
                    w: tr.w,
                    h: tr.h,
                    color: [0.0, 0.0, 0.0, overlay_alpha],
                });
            }
        }

        // Render closing panes as fading-out rects
        for cp in &self.closing_panes {
            bg_rects.push(Rect {
                x: cp.rect.x,
                y: cp.rect.y,
                w: cp.rect.w,
                h: cp.rect.h,
                color: [0.1, 0.1, 0.1, cp.opacity * 0.5],
            });
        }
    }

    pub fn build_status_bar(
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
            x: 0.0,
            y: bar_y,
            w: vw,
            h: bar_height,
            color: ThemeConfig::parse_color(&self.config.theme.statusbar_background),
        });

        let ws_idx = self.workspaces.active_workspace_idx;
        let leader_hint = if self.input.is_awaiting_action() {
            " LEADER "
        } else {
            ""
        };
        let overview_hint = if self.overview_active {
            " OVERVIEW "
        } else {
            ""
        };
        let broadcast_hint = if self.broadcast_mode {
            " BROADCAST "
        } else {
            ""
        };

        let mut status_left = String::new();
        for (i, ws) in self.workspaces.workspaces.iter().enumerate() {
            if !ws.is_empty() || i == ws_idx {
                if i == ws_idx {
                    status_left.push_str(&format!(" [{}*] ", i + 1));
                } else {
                    status_left.push_str(&format!(" [{}] ", i + 1));
                }
            }
        }

        let active_info = self
            .workspaces
            .active()
            .active_pane_id()
            .map(|id| format!("pane:{id}"))
            .unwrap_or_default();
        let session_info = format!("session:{}", self.session_name);
        let status_right = format!("{broadcast_hint}{overview_hint}{leader_hint} {session_info} {active_info} ");

        let text_y = bar_y + 2.0;
        let cw = atlas.cell_width;
        let baseline = atlas.cell_height * self.config.statusbar.text_baseline;

        let left_color = if self.input.is_awaiting_action() {
            ThemeConfig::parse_color(&self.config.theme.accent)
        } else {
            ThemeConfig::parse_color(&self.config.theme.foreground)
        };
        let right_color = if self.broadcast_mode || self.overview_active || self.input.is_awaiting_action() {
            ThemeConfig::parse_color(&self.config.theme.accent)
        } else {
            ThemeConfig::parse_color(&self.config.theme.bright_black)
        };

        emit_status_text(
            atlas,
            &mut renderer.text.font_system,
            &renderer.queue,
            &status_left,
            0.0,
            text_y,
            cw,
            baseline,
            left_color,
            vw,
            vh,
            glyphs,
        );

        let right_start_x = vw - status_right.len() as f32 * cw;
        emit_status_text(
            atlas,
            &mut renderer.text.font_system,
            &renderer.queue,
            &status_right,
            right_start_x,
            text_y,
            cw,
            baseline,
            right_color,
            vw,
            vh,
            glyphs,
        );

        if self.input.is_awaiting_action() {
            let indicator_h = self.config.statusbar.leader_indicator_height;
            bg_rects.push(Rect {
                x: 0.0,
                y: bar_y - indicator_h,
                w: vw,
                h: indicator_h,
                color: ThemeConfig::parse_color(&self.config.theme.accent),
            });
        }
    }

    pub fn build_search_bar(
        &mut self,
        tiles: &[(u64, GeoRect, bool)],
        vw: f32,
        vh: f32,
        bg_rects: &mut Vec<Rect>,
        glyphs: &mut Vec<GlyphInstance>,
    ) {
        let Some(search) = &self.search_state else {
            return;
        };
        let renderer = self.renderer.as_mut().unwrap();
        let atlas = self.glyph_atlas.as_mut().unwrap();

        // Find the tile rect for the search pane
        let Some((_, pane_rect, _)) = tiles.iter().find(|(pid, _, _)| *pid == search.pane_id)
        else {
            return;
        };

        let border_w = self.config.appearance.border_width;
        let padding = self.config.appearance.padding;
        let bar_height = atlas.cell_height + 4.0;
        let bar_y = pane_rect.y + pane_rect.h - border_w - bar_height;
        let bar_x = pane_rect.x + border_w;
        let bar_w = pane_rect.w - border_w * 2.0;

        // Search bar background
        bg_rects.push(Rect {
            x: bar_x,
            y: bar_y,
            w: bar_w,
            h: bar_height,
            color: [0.15, 0.15, 0.2, 0.95],
        });

        let match_info = if search.matches.is_empty() {
            if search.query.is_empty() {
                String::new()
            } else {
                " [no matches]".to_string()
            }
        } else {
            format!(" [{}/{}]", search.current_match_idx + 1, search.matches.len())
        };
        let bar_text = format!(" Search: {}{}", search.query, match_info);

        let cw = atlas.cell_width;
        let baseline = atlas.cell_height * self.config.statusbar.text_baseline;
        let text_y = bar_y + 2.0;
        let text_color = [1.0, 1.0, 1.0, 1.0];

        emit_status_text(
            atlas,
            &mut renderer.text.font_system,
            &renderer.queue,
            &bar_text,
            bar_x + padding,
            text_y,
            cw,
            baseline,
            text_color,
            vw,
            vh,
            glyphs,
        );
    }

    pub fn submit_frame(
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
            Err(wgpu::SurfaceError::Lost) => {
                renderer.resize(vw, vh);
                return;
            }
            Err(e) => {
                log::error!("surface error: {e}");
                return;
            }
        };

        let tex_view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = renderer
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ciri"),
            });

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

            renderer
                .rects
                .render(&renderer.queue, &mut pass, bg_rects, vw_f, vh_f);
            atlas.render(&renderer.queue, &mut pass, glyphs);
        }

        renderer.queue.submit(std::iter::once(encoder.finish()));
        output.present();
    }

    pub fn render(&mut self) {
        if self.renderer.is_none() || self.glyph_atlas.is_none() {
            return;
        }
        let (sw, sh) = self.renderer.as_ref().unwrap().surface_size();
        if sw == 0 || sh == 0 {
            return;
        }

        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f64();
        self.last_frame = now;

        let mut animating = self.advance_animations(dt);

        // Update pane open fade-in animations
        let fade_speed = 5.0; // opacity units per second (~200ms to reach 1.0)
        let mut open_done = Vec::new();
        for (pane_id, opacity) in &mut self.pane_open_opacity {
            *opacity = (*opacity + dt as f32 * fade_speed).min(1.0);
            if *opacity >= 1.0 {
                open_done.push(*pane_id);
            }
        }
        for pid in open_done {
            self.pane_open_opacity.remove(&pid);
        }

        // Update closing pane fade-out animations
        self.closing_panes.retain_mut(|cp| {
            let elapsed = cp.started.elapsed().as_millis() as u64;
            cp.opacity = 1.0 - (elapsed as f32 / cp.duration_ms as f32).min(1.0);
            cp.opacity > 0.0
        });

        if !self.pane_open_opacity.is_empty() || !self.closing_panes.is_empty() {
            animating = true;
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

        // Update terminal views for dirty pane grids
        for (pane_id, _, _) in &tiles {
            let is_dirty = self.pane_grids.get(pane_id).is_some_and(|g| g.dirty);
            if (is_dirty || !self.cached_views.contains_key(pane_id))
                && let Some(grid) = self.pane_grids.get_mut(pane_id)
            {
                let visible = grid.visible_cells();
                let (cur_col, cur_line, cur_shape) =
                    if let Some((col, line)) = grid.cursor_in_viewport() {
                        (col, line, grid.cursor_shape)
                    } else {
                        (0, 0, CURSOR_HIDDEN)
                    };
                let view = terminal::build_view_from_grid(
                    &visible,
                    grid.cols,
                    grid.rows,
                    cur_line,
                    cur_col,
                    cur_shape,
                    atlas,
                    &mut renderer.text.font_system,
                    &renderer.queue,
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
            && let Some(cursor) = view.cursor_rects.first()
        {
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
        self.build_search_bar(&tiles, vw_f, vh_f, &mut bg_rects, &mut glyphs);

        let clear_color = ThemeConfig::parse_color(&self.config.theme.ui_background);
        let renderer = self.renderer.as_mut().unwrap();
        let atlas = self.glyph_atlas.as_mut().unwrap();
        Self::submit_frame(renderer, atlas, clear_color, &bg_rects, &glyphs);

        self.bg_rects_buf = bg_rects;
        self.glyph_buf = glyphs;

        if animating && let Some(w) = &self.window {
            w.request_redraw();
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
        if let Some(entry) = atlas.ensure_char(ch, font_system, queue)
            && entry.width > 0
            && entry.height > 0
        {
            let sx = x_start + i as f32 * cell_width + entry.bearing_x as f32;
            let sy = text_y + baseline - entry.bearing_y as f32;
            glyphs.push(GlyphInstance {
                pos: [sx / vw * 2.0 - 1.0, 1.0 - sy / vh * 2.0],
                size: [
                    entry.width as f32 / vw * 2.0,
                    -(entry.height as f32 / vh * 2.0),
                ],
                uv_pos: [entry.u0, entry.v0],
                uv_size: [entry.u1 - entry.u0, entry.v1 - entry.v0],
                color,
            });
        }
    }
}
