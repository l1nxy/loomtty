use ciri_config::config::{FocusRingStyle, PaneOpenStyle};
use ciri_config::theme::ThemeConfig;
use ciri_layout::geometry::Rect as GeoRect;
use ciri_protocol::message::*;
use ciri_render::FrameScene;
use ciri_render::glyph_cache::{GlyphInstance, ScissoredRange};
use ciri_render::rect::Rect;
use ciri_render::terminal;
use std::time::Instant;

use super::App;
use super::status_bar::{TextEmitParams, emit_status_text};

#[derive(Clone, Copy)]
struct TilePaintConfig {
    border_w: f32,
    active_border: [f32; 4],
    inactive_border: [f32; 4],
    bg_color: [f32; 4],
    link_color: [f32; 4],
    accent: [f32; 4],
    cache_tile_glyphs: bool,
}

#[derive(Clone, Copy)]
struct PaneVisualState {
    tr: GeoRect,
    scissor: (u32, u32, u32, u32),
    inner_x: f32,
    inner_y: f32,
    dim: f32,
    open_opacity: f32,
}

impl App {
    /// Delegate: snap all column widths.
    pub fn snap_all_col_widths(&mut self) {
        self.core.snap_all_col_widths();
    }

    /// Delegate: refresh overview zoom level.
    pub fn refresh_overview_zoom(&mut self) {
        self.core.refresh_overview_zoom();
    }

    /// Delegate: animate view to active column/workspace.
    pub fn animate_to_active(&mut self) {
        self.core.animate_to_active();
    }

    /// Delegate: advance all animations by dt seconds.
    pub fn advance_animations(&mut self, dt: f64) -> bool {
        self.core.advance_animations(dt)
    }

    fn pane_visual_state(
        &self,
        pane_id: u64,
        tile_rect: GeoRect,
        zoom: f32,
        vw: f32,
        vh: f32,
    ) -> Option<PaneVisualState> {
        let zoom_threshold = self.core.config.animation.zoom_threshold;
        let border_w = self.core.config.appearance.border_width;
        let padding = self.core.config.appearance.padding;
        let base_tr = Self::transformed_tile_rect_for_zoom(tile_rect, zoom, zoom_threshold, vw, vh);

        let focus_opacity = self.core.anim_mgr.pane_focus_opacity(pane_id);
        let open_opacity = self.core.anim_mgr.pane_open_opacity(pane_id);
        let drag_dim = self.core.anim_mgr.pane_drag_dim(pane_id);
        let dim = focus_opacity * open_opacity * drag_dim;

        let slide_progress = self.core.anim_mgr.pane_open_slide(pane_id);
        let (open_dx, open_dy) = match self.core.config.animation.pane_open_style {
            PaneOpenStyle::SlideUp | PaneOpenStyle::FadeSlideUp => {
                (0.0, -base_tr.h * slide_progress)
            }
            PaneOpenStyle::SlideDown => (0.0, base_tr.h * slide_progress),
            PaneOpenStyle::SlideLeft => (-base_tr.w * slide_progress, 0.0),
            PaneOpenStyle::Fade => (0.0, 0.0),
        };
        let (move_dx, move_dy) = self.core.anim_mgr.pane_move_offset(pane_id);
        let tr = GeoRect::new(
            base_tr.x + open_dx + move_dx * zoom,
            base_tr.y + open_dy + move_dy * zoom,
            base_tr.w,
            base_tr.h,
        );
        let scissor = scissor_rect(&tr, vw, vh)?;
        Some(PaneVisualState {
            tr,
            scissor,
            inner_x: tr.x + (border_w + padding) * zoom,
            inner_y: tr.y + (border_w + padding) * zoom,
            dim,
            open_opacity,
        })
    }

    fn emit_focus_ring(
        &self,
        tr: &GeoRect,
        zoom: f32,
        paint: &TilePaintConfig,
        bg_rects: &mut Vec<Rect>,
    ) {
        match &self.core.config.appearance.focus_ring.style {
            FocusRingStyle::Glow => {
                let fr = &self.core.config.appearance.focus_ring;
                let layers = fr.glow_layers.max(1) as usize;
                for layer in (0..layers).rev() {
                    let offset = fr.glow_radius * (layer + 1) as f32 / layers as f32;
                    let alpha =
                        paint.active_border[3] * (1.0 - layer as f32 / layers as f32) * 0.3;
                    bg_rects.push(Rect {
                        x: tr.x - offset,
                        y: tr.y - offset,
                        w: tr.w + offset * 2.0,
                        h: tr.h + offset * 2.0,
                        color: [
                            paint.active_border[0],
                            paint.active_border[1],
                            paint.active_border[2],
                            alpha,
                        ],
                    });
                }
                bg_rects.push(Rect {
                    x: tr.x,
                    y: tr.y,
                    w: tr.w,
                    h: tr.h,
                    color: paint.active_border,
                });
            }
            FocusRingStyle::Dashed => {
                let fr = &self.core.config.appearance.focus_ring;
                emit_dashed_border(
                    bg_rects,
                    tr,
                    paint.border_w * zoom,
                    fr.dash_length,
                    fr.gap_length,
                    paint.active_border,
                );
            }
            FocusRingStyle::Solid => {
                bg_rects.push(Rect {
                    x: tr.x,
                    y: tr.y,
                    w: tr.w,
                    h: tr.h,
                    color: paint.active_border,
                });
            }
        }
    }

    fn emit_overview_hover(
        &self,
        pane_id: u64,
        tr: &GeoRect,
        zoom: f32,
        paint: &TilePaintConfig,
        bg_rects: &mut Vec<Rect>,
    ) {
        if !self.core.overview.active
            || !self
                .core
                .overview
                .hovered_pane
                .is_some_and(|(_, hovered_pane_id)| hovered_pane_id == pane_id)
        {
            return;
        }
        let hover_border_w = (paint.border_w * zoom).max(1.0);
        for layer in (1..=2).rev() {
            let spread = layer as f32 * 2.0 * zoom.max(1.0);
            bg_rects.push(Rect {
                x: tr.x - spread,
                y: tr.y - spread,
                w: tr.w + spread * 2.0,
                h: tr.h + spread * 2.0,
                color: [
                    paint.accent[0],
                    paint.accent[1],
                    paint.accent[2],
                    0.08 / layer as f32,
                ],
            });
        }
        bg_rects.push(Rect {
            x: tr.x,
            y: tr.y,
            w: tr.w,
            h: hover_border_w,
            color: [paint.accent[0], paint.accent[1], paint.accent[2], 0.65],
        });
        bg_rects.push(Rect {
            x: tr.x,
            y: tr.y + tr.h - hover_border_w,
            w: tr.w,
            h: hover_border_w,
            color: [paint.accent[0], paint.accent[1], paint.accent[2], 0.65],
        });
        bg_rects.push(Rect {
            x: tr.x,
            y: tr.y,
            w: hover_border_w,
            h: tr.h,
            color: [paint.accent[0], paint.accent[1], paint.accent[2], 0.65],
        });
        bg_rects.push(Rect {
            x: tr.x + tr.w - hover_border_w,
            y: tr.y,
            w: hover_border_w,
            h: tr.h,
            color: [paint.accent[0], paint.accent[1], paint.accent[2], 0.65],
        });
    }

    fn emit_selection_highlight(
        &self,
        pane_id: u64,
        inner_x: f32,
        inner_y: f32,
        zoom: f32,
        tr: &GeoRect,
        bg_rects: &mut Vec<Rect>,
    ) {
        if let Some(sel) = &self.core.selection
            && sel.pane_id == pane_id
            && sel.start != sel.end
            && let Some(grid) = self.core.pane_grids.get(&pane_id)
        {
            let (cw, ch) = self.cell_dimensions();
            let (start, end) = if sel.start.1 < sel.end.1
                || (sel.start.1 == sel.end.1 && sel.start.0 <= sel.end.0)
            {
                (sel.start, sel.end)
            } else {
                (sel.end, sel.start)
            };
            let fg = self
                .cached_color_table
                .resolve_packed(ciri_protocol::message::DEFAULT_FOREGROUND);
            let sel_color = [fg[0], fg[1], fg[2], 0.35];
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
                if let Some(c) = src.intersection(tr) {
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

    fn emit_link_underline(
        &self,
        pane_id: u64,
        inner_x: f32,
        inner_y: f32,
        zoom: f32,
        tr: &GeoRect,
        paint: &TilePaintConfig,
        bg_rects: &mut Vec<Rect>,
    ) {
        if let Some(link) = &self.core.hovered_link
            && link.pane_id == pane_id
            && let Some(grid) = self.core.pane_grids.get(&pane_id)
            && let Some(viewport_row) = grid.buffer_to_viewport_row(link.start.1)
        {
            let (cw, ch) = self.cell_dimensions();
            let underline_h = (zoom.max(1.0)).clamp(1.0, 2.0);
            let sx = inner_x + link.start.0 as f32 * cw * zoom;
            let sy = inner_y + (viewport_row as f32 + 1.0) * ch * zoom - underline_h - zoom;
            let sw = (link.end.0 - link.start.0 + 1) as f32 * cw * zoom;
            let src = GeoRect::new(sx, sy, sw, underline_h);
            if let Some(c) = src.intersection(tr) {
                bg_rects.push(Rect {
                    x: c.x,
                    y: c.y,
                    w: c.w,
                    h: c.h,
                    color: paint.link_color,
                });
            }
        }
    }

    fn emit_search_highlights(
        &self,
        pane_id: u64,
        inner_x: f32,
        inner_y: f32,
        zoom: f32,
        tr: &GeoRect,
        bg_rects: &mut Vec<Rect>,
    ) {
        if let Some(search) = &self.core.search_state
            && search.pane_id == pane_id
            && let Some(grid) = self.core.pane_grids.get(&pane_id)
        {
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
                    [1.0, 0.6, 0.0, 0.5]
                } else {
                    [1.0, 1.0, 0.0, 0.3]
                };
                let src = GeoRect::new(sx, sy, sw, sh);
                if let Some(c) = src.intersection(tr) {
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

    #[allow(clippy::too_many_arguments)]
    fn emit_tile_glyphs(
        tile_cache: &mut std::collections::HashMap<u64, super::CachedTileGlyphs>,
        pane_id: u64,
        view: &terminal::TerminalView,
        inner_x: f32,
        inner_y: f32,
        zoom: f32,
        dim: f32,
        generation: u64,
        tile_key: (u32, u32, u32, u32),
        cache_tile_glyphs: bool,
        glyphs: &mut Vec<GlyphInstance>,
        color_glyphs: &mut Vec<GlyphInstance>,
    ) {
        let glyph_start = glyphs.len();
        let color_start = color_glyphs.len();
        let cache_hit = cache_tile_glyphs
            && tile_cache
                .get(&pane_id)
                .is_some_and(|c| c.generation == generation && c.key == tile_key);

        if cache_hit {
            let cached = tile_cache.get(&pane_id).unwrap();
            glyphs.extend_from_slice(&cached.glyphs);
            color_glyphs.extend_from_slice(&cached.color_glyphs);
        } else {
            let make_instance =
                |g: &terminal::RelativeGlyph, color: [f32; 4]| -> Option<GlyphInstance> {
                    let sx = (inner_x + g.px * zoom).round();
                    let sy = (inner_y + g.py * zoom).round();
                    let gw = (g.glyph_w * zoom).round();
                    let gh = (g.glyph_h * zoom).round();

                    if gw <= 0.0 || gh <= 0.0 {
                        return None;
                    }

                    Some(GlyphInstance {
                        pos: [sx, sy],
                        size: [gw, gh],
                        uv_pos: [g.u0, g.v0],
                        uv_size: [g.u1 - g.u0, g.v1 - g.v0],
                        color,
                    })
                };

            glyphs.extend(view.glyph_instances.iter().filter_map(|g| {
                let color = [
                    g.color[0] * dim,
                    g.color[1] * dim,
                    g.color[2] * dim,
                    g.color[3],
                ];
                make_instance(g, color)
            }));

            let emoji_color = [dim, dim, dim, 1.0];
            color_glyphs.extend(
                view.color_glyph_instances
                    .iter()
                    .filter_map(|g| make_instance(g, emoji_color)),
            );

            if cache_tile_glyphs {
                tile_cache.insert(
                    pane_id,
                    super::CachedTileGlyphs {
                        generation,
                        key: tile_key,
                        glyphs: glyphs[glyph_start..].to_vec(),
                        color_glyphs: color_glyphs[color_start..].to_vec(),
                    },
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_tile(
        &mut self,
        pane_id: u64,
        tile_rect: GeoRect,
        is_active: bool,
        zoom: f32,
        vw: f32,
        vh: f32,
        paint: TilePaintConfig,
        bg_rects: &mut Vec<Rect>,
        glyphs: &mut Vec<GlyphInstance>,
        color_glyphs: &mut Vec<GlyphInstance>,
        glyph_batches: &mut Vec<ScissoredRange>,
        color_glyph_batches: &mut Vec<ScissoredRange>,
    ) {
        let Some(visual) = self.pane_visual_state(pane_id, tile_rect, zoom, vw, vh) else {
            return;
        };
        let tr = visual.tr;
        let inner_x = visual.inner_x;
        let inner_y = visual.inner_y;

        // Focus ring / inactive border
        if is_active {
            self.emit_focus_ring(&tr, zoom, &paint, bg_rects);
        } else {
            bg_rects.push(Rect {
                x: tr.x,
                y: tr.y,
                w: tr.w,
                h: tr.h,
                color: paint.inactive_border,
            });
        }

        // Background fill
        bg_rects.push(Rect {
            x: tr.x + paint.border_w * zoom,
            y: tr.y + paint.border_w * zoom,
            w: tr.w - paint.border_w * zoom * 2.0,
            h: tr.h - paint.border_w * zoom * 2.0,
            color: paint.bg_color,
        });

        // Overview hover highlight
        self.emit_overview_hover(pane_id, &tr, zoom, &paint, bg_rects);

        // Cell backgrounds from view
        let Some(view) = self.cached_views.remove(&pane_id) else {
            return;
        };

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

        // Cursor rects
        if self.core.cursor_blink_visible && is_active {
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

        // Scrollbar
        if let Some(sb) = &view.scrollbar_rect {
            let src = GeoRect::new(
                inner_x + sb.x * zoom,
                inner_y + sb.y * zoom,
                sb.w * zoom,
                sb.h * zoom,
            );
            if let Some(c) = src.intersection(&tr) {
                bg_rects.push(Rect {
                    x: c.x,
                    y: c.y,
                    w: c.w,
                    h: c.h,
                    color: sb.color,
                });
            }
        }

        // Selection highlight
        self.emit_selection_highlight(pane_id, inner_x, inner_y, zoom, &tr, bg_rects);

        // Hovered link underline
        self.emit_link_underline(pane_id, inner_x, inner_y, zoom, &tr, &paint, bg_rects);

        // Search match highlights
        self.emit_search_highlights(pane_id, inner_x, inner_y, zoom, &tr, bg_rects);

        // Glyph rendering + caching
        let tile_key = (
            inner_x.to_bits(),
            inner_y.to_bits(),
            zoom.to_bits(),
            visual.dim.to_bits(),
        );
        let generation = view.generation;
        let glyph_start = glyphs.len();
        let color_start = color_glyphs.len();
        Self::emit_tile_glyphs(
            &mut self.cached_tile_glyphs,
            pane_id,
            &view,
            inner_x,
            inner_y,
            zoom,
            visual.dim,
            generation,
            tile_key,
            paint.cache_tile_glyphs,
            glyphs,
            color_glyphs,
        );

        // Reinsert the view
        self.cached_views.insert(pane_id, view);

        // Pane images
        self.build_pane_images(pane_id, inner_x, inner_y, zoom, visual.dim, color_glyphs);

        // Scissor batches
        let (sx, sy, sw, sh) = visual.scissor;
        if glyph_start < glyphs.len() {
            glyph_batches.push(ScissoredRange {
                x: sx,
                y: sy,
                w: sw,
                h: sh,
                start: glyph_start,
                end: glyphs.len(),
            });
        }
        if color_start < color_glyphs.len() {
            color_glyph_batches.push(ScissoredRange {
                x: sx,
                y: sy,
                w: sw,
                h: sh,
                start: color_start,
                end: color_glyphs.len(),
            });
        }

        // Open animation overlay
        if visual.open_opacity < 1.0 {
            let overlay_alpha = 1.0 - visual.open_opacity;
            bg_rects.push(Rect {
                x: tr.x,
                y: tr.y,
                w: tr.w,
                h: tr.h,
                color: [0.0, 0.0, 0.0, overlay_alpha],
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn build_tiles(
        &mut self,
        tiles: &[(u64, GeoRect, bool)],
        zoom: f32,
        vw: f32,
        vh: f32,
        bg_rects: &mut Vec<Rect>,
        glyphs: &mut Vec<GlyphInstance>,
        color_glyphs: &mut Vec<GlyphInstance>,
        glyph_batches: &mut Vec<ScissoredRange>,
        color_glyph_batches: &mut Vec<ScissoredRange>,
        active_glyph_batches: &mut Vec<ScissoredRange>,
        active_color_glyph_batches: &mut Vec<ScissoredRange>,
    ) -> usize {
        let zoom_threshold = self.core.config.animation.zoom_threshold;
        let paint = TilePaintConfig {
            border_w: self.core.config.appearance.border_width,
            active_border: if self.core.config.appearance.active_border_color.is_empty() {
                ThemeConfig::parse_color(&self.core.config.theme.border_active)
            } else {
                ThemeConfig::parse_color(&self.core.config.appearance.active_border_color)
            },
            inactive_border: if self.core.config.appearance.inactive_border_color.is_empty() {
                ThemeConfig::parse_color(&self.core.config.theme.border_inactive)
            } else {
                ThemeConfig::parse_color(&self.core.config.appearance.inactive_border_color)
            },
            bg_color: ThemeConfig::parse_color(&self.core.config.theme.background),
            link_color: ThemeConfig::parse_color(&self.core.config.theme.accent),
            accent: ThemeConfig::parse_color(&self.core.config.theme.accent),
            cache_tile_glyphs: self.pending_resize.is_none(),
        };

        for (pane_id, tile_rect, is_active) in tiles.iter().copied() {
            if is_active {
                continue;
            }
            self.build_tile(
                pane_id,
                tile_rect,
                false,
                zoom,
                vw,
                vh,
                paint,
                bg_rects,
                glyphs,
                color_glyphs,
                glyph_batches,
                color_glyph_batches,
            );
        }

        for (rect, opacity, _slide) in self.core.anim_mgr.closing_panes() {
            let (rx, ry, rw, rh) = if zoom < zoom_threshold {
                let cx = vw / 2.0;
                let cy = vh / 2.0;
                (
                    cx + (rect.x - cx) * zoom,
                    cy + (rect.y - cy) * zoom,
                    rect.w * zoom,
                    rect.h * zoom,
                )
            } else {
                (rect.x, rect.y, rect.w, rect.h)
            };
            bg_rects.push(Rect {
                x: rx,
                y: ry,
                w: rw,
                h: rh,
                color: [0.1, 0.1, 0.1, opacity * 0.5],
            });
        }

        let active_bg_start = bg_rects.len();

        for (pane_id, tile_rect, is_active) in tiles.iter().copied() {
            if !is_active {
                continue;
            }
            self.build_tile(
                pane_id,
                tile_rect,
                true,
                zoom,
                vw,
                vh,
                paint,
                bg_rects,
                glyphs,
                color_glyphs,
                active_glyph_batches,
                active_color_glyph_batches,
            );
        }

        active_bg_start
    }

    pub fn build_search_bar(
        &mut self,
        tiles: &[(u64, GeoRect, bool)],
        _vw: f32,
        _vh: f32,
        bg_rects: &mut Vec<Rect>,
        glyphs: &mut Vec<GlyphInstance>,
    ) {
        let Some(search) = &self.core.search_state else {
            return;
        };
        let atlas = self.glyph_cache.as_mut().unwrap();

        // Find the tile rect for the search pane
        let Some((_, pane_rect, _)) = tiles.iter().find(|(pid, _, _)| *pid == search.pane_id)
        else {
            return;
        };

        let border_w = self.core.config.appearance.border_width;
        let padding = self.core.config.appearance.padding;
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
            format!(
                " [{}/{}]",
                search.current_match_idx + 1,
                search.matches.len()
            )
        };
        let bar_text = format!(" Search: {}{}", search.query, match_info);

        let cw = atlas.cell_width;
        let baseline = atlas.cell_height * self.core.config.statusbar.text_baseline;
        let text_y = bar_y + 2.0;
        let text_color = [1.0, 1.0, 1.0, 1.0];

        emit_status_text(
            atlas,
            &bar_text,
            &TextEmitParams {
                x_start: bar_x + padding,
                y: text_y,
                cell_width: cw,
                baseline,
                color: text_color,
            },
            glyphs,
        );
    }

    pub fn build_bell_flash(
        &mut self,
        tiles: &[(u64, GeoRect, bool)],
        zoom: f32,
        vw: f32,
        vh: f32,
        bg_rects: &mut Vec<Rect>,
    ) {
        for (pane_id, tile_rect, _) in tiles {
            let intensity = self.core.anim_mgr.bell_flash(*pane_id);
            if intensity <= 0.0 {
                continue;
            }
            let alpha = 0.15 * intensity;
            let Some(visual) = self.pane_visual_state(*pane_id, *tile_rect, zoom, vw, vh) else {
                continue;
            };
            let tr = visual.tr;
            bg_rects.push(Rect {
                x: tr.x,
                y: tr.y,
                w: tr.w,
                h: tr.h,
                color: [1.0, 0.9, 0.5, alpha],
            });
        }
    }

    pub fn build_ime_preedit(
        &mut self,
        tiles: &[(u64, GeoRect, bool)],
        _vw: f32,
        _vh: f32,
        bg_rects: &mut Vec<Rect>,
        glyphs: &mut Vec<GlyphInstance>,
    ) {
        if !self.core.ime.preedit_active || self.core.ime.preedit_text.is_empty() {
            return;
        }
        let atlas = self.glyph_cache.as_mut().unwrap();

        // Find active pane tile rect and cursor position
        let active_pid = match self.core.workspaces.active().active_pane_id() {
            Some(pid) => pid,
            None => return,
        };
        let tile_rect = match tiles.iter().find(|(pid, _, _)| *pid == active_pid) {
            Some((_, rect, _)) => *rect,
            None => return,
        };
        let view = match self.cached_views.get(&active_pid) {
            Some(v) => v,
            None => return,
        };
        let cursor_rect = match view.cursor_rects.first() {
            Some(c) => c,
            None => return,
        };

        let border_w = self.core.config.appearance.border_width;
        let padding = self.core.config.appearance.padding;
        let cw = atlas.cell_width;
        let ch = atlas.cell_height;

        // Position at cursor
        let base_x = tile_rect.x + border_w + padding + cursor_rect.x;
        let base_y = tile_rect.y + border_w + padding + cursor_rect.y;

        let text = &self.core.ime.preedit_text;
        let text_width = text.chars().count() as f32 * cw;

        // Background box
        bg_rects.push(Rect {
            x: base_x,
            y: base_y,
            w: text_width + 4.0,
            h: ch + 2.0,
            color: [0.15, 0.15, 0.25, 0.95],
        });

        // Underline the preedit region
        bg_rects.push(Rect {
            x: base_x,
            y: base_y + ch,
            w: text_width + 4.0,
            h: 2.0,
            color: [0.5, 0.7, 1.0, 0.9],
        });

        // Render text
        let baseline = ch * self.core.config.statusbar.text_baseline;
        let text_color = [1.0, 1.0, 1.0, 1.0];
        emit_status_text(
            atlas,
            text,
            &TextEmitParams {
                x_start: base_x + 2.0,
                y: base_y + 1.0,
                cell_width: cw,
                baseline,
                color: text_color,
            },
            glyphs,
        );

        // Cursor within preedit text
        if let Some(cursor_pos) = self.core.ime.preedit_cursor {
            let cx = base_x + 2.0 + cursor_pos as f32 * cw;
            bg_rects.push(Rect {
                x: cx,
                y: base_y + 1.0,
                w: 2.0,
                h: ch,
                color: [1.0, 1.0, 1.0, 0.8],
            });
        }
    }

    fn rgb_to_rgba(width: u32, height: u32, data: &[u8]) -> Option<Vec<u8>> {
        let rgb_len = width.checked_mul(height)?.checked_mul(3)? as usize;
        if data.len() != rgb_len {
            log::warn!(
                "rejecting RGB image upload: got {} bytes, expected {rgb_len} for {width}x{height}",
                data.len()
            );
            return None;
        }

        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for rgb in data.chunks_exact(3) {
            rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        Some(rgba)
    }

    fn ensure_image_atlas_entry(
        &mut self,
        pane_id: u64,
        img: &super::ClientImagePlacement,
    ) -> Option<ciri_render::glyph_cache::GlyphEntry> {
        let cache_key = (pane_id, img.image_id);
        if let Some(entry) = self.image_atlas_entries.get(&cache_key).copied() {
            return Some(entry);
        }

        let rgba = match img.format.as_str() {
            "rgba" => img.data.as_slice().to_vec(),
            "rgb" => Self::rgb_to_rgba(img.pixel_width, img.pixel_height, img.data.as_slice())?,
            other => {
                log::warn!(
                    "unsupported inline image format {other:?} for image {}",
                    img.image_id
                );
                return None;
            }
        };

        let entry = self
            .glyph_cache
            .as_mut()
            .and_then(|cache| cache.cache_rgba_image(img.pixel_width, img.pixel_height, &rgba))?;
        self.image_atlas_entries.insert(cache_key, entry);
        Some(entry)
    }

    fn build_pane_images(
        &mut self,
        pane_id: u64,
        inner_x: f32,
        inner_y: f32,
        zoom: f32,
        dim: f32,
        color_glyphs: &mut Vec<GlyphInstance>,
    ) {
        let Some(placements) = self.core.image_placements.get(&pane_id).cloned() else {
            return;
        };
        if placements.is_empty() {
            return;
        }

        let (cw, ch) = {
            let Some(atlas) = self.glyph_cache.as_ref() else {
                return;
            };
            (atlas.cell_width, atlas.cell_height)
        };

        for img in &placements {
            let Some(entry) = self.ensure_image_atlas_entry(pane_id, img) else {
                continue;
            };

            let sx = (inner_x + img.col as f32 * cw * zoom).round();
            let sy = (inner_y + img.row as f32 * ch * zoom).round();
            let (iw, ih) = Self::image_display_size(img, cw, ch, zoom);

            color_glyphs.push(GlyphInstance {
                pos: [sx, sy],
                size: [iw, ih],
                uv_pos: [entry.u0, entry.v0],
                uv_size: [entry.u1 - entry.u0, entry.v1 - entry.v0],
                color: [dim, dim, dim, 1.0],
            });
        }
    }

    fn image_display_size(
        img: &super::ClientImagePlacement,
        cell_width: f32,
        cell_height: f32,
        zoom: f32,
    ) -> (f32, f32) {
        match img.display_mode {
            ImageDisplayMode::Cells => (
                (img.width_cells.max(1) as f32 * cell_width * zoom)
                    .round()
                    .max(1.0),
                (img.height_cells.max(1) as f32 * cell_height * zoom)
                    .round()
                    .max(1.0),
            ),
            ImageDisplayMode::Pixels => (
                (img.pixel_width.max(1) as f32 * zoom).round().max(1.0),
                (img.pixel_height.max(1) as f32 * zoom).round().max(1.0),
            ),
        }
    }

    pub fn render(&mut self) {
        if self.renderer.is_none() || self.glyph_cache.is_none() || self.glyph_atlas_gpu.is_none() {
            return;
        }

        let (sw, sh) = self.renderer.as_ref().unwrap().surface_size();
        if sw == 0 || sh == 0 {
            return;
        }

        let now = Instant::now();
        let dt = (now - self.core.last_frame).as_secs_f64();
        self.core.last_frame = now;

        let mut animating = self.advance_animations(dt);

        // Focus change detection → delegate to AnimationManager
        let current_focus = self.core.workspaces.active().active_pane_id();
        let config = self.anim_config();
        self.core.anim_mgr.on_focus_changed(current_focus, &config);

        // Advance all pane animations (open, close, focus, bell) in one call
        animating |= self.core.anim_mgr.advance_all(dt);

        let renderer = self.renderer.as_mut().unwrap();
        let (vw, vh) = renderer.surface_size();
        let vw_f = vw as f32;
        let vh_f = vh as f32;
        let zoom = self.core.anim_mgr.overview_zoom.value() as f32;
        let zoom_threshold = self.core.config.animation.zoom_threshold;
        let vox = self.core.anim_mgr.view_offset_x.value() as f32;
        let voy = self.core.anim_mgr.view_offset_y.value() as f32;
        let tiles = if self.core.overview.active || zoom < zoom_threshold {
            self.overview_visible_tiles(zoom, vox, voy)
        } else {
            self.core.workspaces.visible_tiles_2d(vox, voy)
        };

        let cache = self.glyph_cache.as_mut().unwrap();
        let shaper = self.text_shaper.as_ref().unwrap();

        // Update terminal views for dirty pane grids
        for (pane_id, tile_rect, _) in &tiles {
            // Snapshot dirty state before taking mutable borrows
            let (needs_full, has_dirty_rows, dirty_rows_copy) =
                if let Some(g) = self.core.pane_grids.get(pane_id) {
                    (g.dirty, g.is_dirty(), g.dirty_rows.clone())
                } else {
                    (false, false, Vec::new())
                };
            let needs_initial = !self.cached_views.contains_key(pane_id);

            if (needs_full || needs_initial)
                && let Some(grid) = self.core.pane_grids.get_mut(pane_id)
            {
                // Full rebuild path
                let visible = grid.visible_cells();
                // Apply prediction overlay
                let visible_cow = if self.core.prediction.has_overlay(*pane_id) {
                    let mut cells = visible.into_owned();
                    for i in 0..cells.len() {
                        let row = (i / grid.cols as usize) as u16;
                        let col = (i % grid.cols as usize) as u16;
                        if let Some(replacement) =
                            self.core.prediction.get_overlay_cell(*pane_id, row, col)
                        {
                            cells[i] = replacement;
                        }
                    }
                    std::borrow::Cow::Owned(cells)
                } else {
                    visible
                };
                let (mut cur_col, mut cur_line, cur_shape) =
                    if let Some((col, line)) = grid.cursor_in_viewport() {
                        (col, line, grid.cursor_shape)
                    } else {
                        (0, 0, CURSOR_HIDDEN)
                    };
                // Apply prediction cursor overlay
                if let Some((pred_line, pred_col)) =
                    self.core.prediction.get_overlay_cursor(*pane_id)
                {
                    cur_line = pred_line;
                    cur_col = pred_col;
                }
                let inputs = terminal::PackedViewInputs {
                    cells: &visible_cow,
                    cols: grid.cols,
                    rows: grid.rows,
                    cursor_line: cur_line,
                    cursor_col: cur_col,
                    cursor_shape: cur_shape,
                    config: &self.core.config,
                    shaper,
                    colors: &self.cached_color_table,
                    grapheme_map: &grid.grapheme_map,
                };
                let view = terminal::build_view_from_grid(cache, &inputs);
                grid.clear_dirty();
                self.cached_views.insert(*pane_id, view);
                // Invalidate tile glyph cache — generation counter alone is
                // unreliable for full rebuilds (resets to 1 on insert).
                self.cached_tile_glyphs.remove(pane_id);
            } else if has_dirty_rows && !needs_full {
                // Incremental update path — only re-render dirty rows
                if let Some(grid) = self.core.pane_grids.get_mut(pane_id) {
                    let (mut cur_col, mut cur_line, cur_shape) =
                        if let Some((col, line)) = grid.cursor_in_viewport() {
                            (col, line, grid.cursor_shape)
                        } else {
                            (0, 0, CURSOR_HIDDEN)
                        };
                    // Apply prediction cursor overlay
                    if let Some((pred_line, pred_col)) =
                        self.core.prediction.get_overlay_cursor(*pane_id)
                    {
                        cur_line = pred_line;
                        cur_col = pred_col;
                    }
                    // Clear dirty before visible_cells() to avoid borrow conflict
                    // (clear_dirty only resets flags, not cell data)
                    grid.clear_dirty();
                    let visible = grid.visible_cells();
                    // Apply prediction overlay
                    let visible_final = if self.core.prediction.has_overlay(*pane_id) {
                        let mut cells = visible.into_owned();
                        for i in 0..cells.len() {
                            let row = (i / grid.cols as usize) as u16;
                            let col = (i % grid.cols as usize) as u16;
                            if let Some(replacement) =
                                self.core.prediction.get_overlay_cell(*pane_id, row, col)
                            {
                                cells[i] = replacement;
                            }
                        }
                        std::borrow::Cow::Owned(cells)
                    } else {
                        visible
                    };
                    if let Some(view) = self.cached_views.get_mut(pane_id) {
                        let inputs = terminal::PackedViewInputs {
                            cells: &visible_final,
                            cols: grid.cols,
                            rows: grid.rows,
                            cursor_line: cur_line,
                            cursor_col: cur_col,
                            cursor_shape: cur_shape,
                            config: &self.core.config,
                            shaper,
                            colors: &self.cached_color_table,
                            grapheme_map: &grid.grapheme_map,
                        };
                        terminal::update_view_from_grid(view, &dirty_rows_copy, &inputs, cache);
                    }
                }
            }

            // Recompute scrollbar only when parameters change (avoids redundant float math).
            if let Some(grid) = self.core.pane_grids.get(pane_id)
                && let Some(view) = self.cached_views.get_mut(pane_id)
            {
                let border_w = self.core.config.appearance.border_width;
                let padding = self.core.config.appearance.padding;
                let inset = (border_w + padding) * 2.0;
                let pw = tile_rect.w - inset;
                let ph = tile_rect.h - inset;
                // Determine scrollbar visual state (Pressed > Hovered > Idle)
                let sb_state = if self
                    .core
                    .drag
                    .scrollbar_dragging
                    .as_ref()
                    .is_some_and(|info| info.pane_id == *pane_id)
                {
                    terminal::ScrollbarState::Pressed
                } else if let Some((mx, my)) = self.last_mouse_pos
                    && tile_rect.contains(mx, my)
                    && let Some(sb) = &view.scrollbar_rect
                {
                    let ix = tile_rect.x + border_w + padding;
                    let iy = tile_rect.y + border_w + padding;
                    let sx = ix + sb.x;
                    let sy = iy + sb.y;
                    if mx >= sx && mx <= sx + sb.w && my >= sy && my <= sy + sb.h {
                        terminal::ScrollbarState::Hovered
                    } else {
                        terminal::ScrollbarState::Idle
                    }
                } else {
                    terminal::ScrollbarState::Idle
                };
                let sb_key = (
                    grid.scroll_offset,
                    grid.total_lines(),
                    grid.rows,
                    pw.to_bits(),
                    ph.to_bits(),
                    sb_state as u8,
                );
                if view.scrollbar_key != Some(sb_key) {
                    view.scrollbar_rect = terminal::build_scrollbar(
                        grid.scroll_offset,
                        grid.total_lines(),
                        grid.rows,
                        pw,
                        ph,
                        sb_state,
                        &self.core.config,
                    );
                    view.scrollbar_key = Some(sb_key);
                }
            }
        }

        // Update IME cursor area
        if self.pending_resize.is_none()
            && let Some(window) = &self.window
            && let Some(active_pid) = self.core.workspaces.active().active_pane_id()
            && let Some((_, tile_rect, _)) = tiles.iter().find(|(id, _, _)| *id == active_pid)
            && let Some(view) = self.cached_views.get(&active_pid)
            && let Some(cursor) = view.cursor_rects.first()
        {
            let padding = self.core.config.appearance.padding;
            let border_w = self.core.config.appearance.border_width;
            let cx = (tile_rect.x + border_w + padding + cursor.x) as i32;
            let cy = (tile_rect.y + border_w + padding + cursor.y) as i32;
            let pos = (cx, cy);
            if self.core.ime.last_pos != Some(pos) {
                self.core.ime.last_pos = Some(pos);
                window.set_ime_cursor_area(
                    winit::dpi::PhysicalPosition::new(cx as f64, cy as f64),
                    winit::dpi::PhysicalSize::new(
                        cache.cell_width as f64,
                        cache.cell_height as f64,
                    ),
                );
            }
        }

        let content_y = self.content_origin_y();
        let offset_tiles: Vec<(u64, GeoRect, bool)> = tiles
            .iter()
            .map(|(pane_id, rect, is_active)| {
                (
                    *pane_id,
                    GeoRect::new(rect.x, rect.y + content_y, rect.w, rect.h),
                    *is_active,
                )
            })
            .collect();

        let mut bg_rects = std::mem::take(&mut self.render_bufs.bg_rects);
        let mut glyphs = std::mem::take(&mut self.render_bufs.glyphs);
        let mut color_glyphs = std::mem::take(&mut self.render_bufs.color_glyphs);
        let mut glyph_batches = std::mem::take(&mut self.render_bufs.glyph_batches);
        let mut color_glyph_batches = std::mem::take(&mut self.render_bufs.color_glyph_batches);
        let mut active_glyph_batches = std::mem::take(&mut self.render_bufs.active_glyph_batches);
        let mut active_color_glyph_batches =
            std::mem::take(&mut self.render_bufs.active_color_glyph_batches);
        bg_rects.clear();
        glyphs.clear();
        color_glyphs.clear();
        glyph_batches.clear();
        color_glyph_batches.clear();
        active_glyph_batches.clear();
        active_color_glyph_batches.clear();

        let active_bg_start = self.build_tiles(
            &offset_tiles,
            zoom,
            vw_f,
            vh_f,
            &mut bg_rects,
            &mut glyphs,
            &mut color_glyphs,
            &mut glyph_batches,
            &mut color_glyph_batches,
            &mut active_glyph_batches,
            &mut active_color_glyph_batches,
        );
        let pane_glyph_end = glyphs.len();
        let pane_color_glyph_end = color_glyphs.len();
        let overlay_bg_start = bg_rects.len();
        self.build_ui(vw_f, vh_f, &mut bg_rects, &mut glyphs);
        self.build_search_bar(&offset_tiles, vw_f, vh_f, &mut bg_rects, &mut glyphs);
        self.build_bell_flash(&offset_tiles, zoom, vw_f, vh_f, &mut bg_rects);
        self.build_ime_preedit(&offset_tiles, vw_f, vh_f, &mut bg_rects, &mut glyphs);

        let clear_color = if self.core.overview.active || zoom < zoom_threshold {
            ThemeConfig::parse_color(&self.core.config.theme.overview_background)
        } else {
            ThemeConfig::parse_color(&self.core.config.theme.ui_background)
        };
        let renderer = self.renderer.as_mut().unwrap();
        let cache = self.glyph_cache.as_mut().unwrap();
        let atlas_gpu = self.glyph_atlas_gpu.as_mut().unwrap();
        renderer.draw_frame(
            atlas_gpu,
            cache,
            FrameScene {
                clear_color,
                bg_rects: &bg_rects,
                glyphs: &glyphs,
                color_glyphs: &color_glyphs,
                glyph_batches: &glyph_batches,
                color_glyph_batches: &color_glyph_batches,
                active_bg_start,
                active_glyph_batches: &active_glyph_batches,
                active_color_glyph_batches: &active_color_glyph_batches,
                pane_glyph_end,
                pane_color_glyph_end,
                overlay_bg_start,
            },
        );

        self.render_bufs.bg_rects = bg_rects;
        self.render_bufs.glyphs = glyphs;
        self.render_bufs.color_glyphs = color_glyphs;
        self.render_bufs.glyph_batches = glyph_batches;
        self.render_bufs.color_glyph_batches = color_glyph_batches;
        self.render_bufs.active_glyph_batches = active_glyph_batches;
        self.render_bufs.active_color_glyph_batches = active_color_glyph_batches;

        // Atlas overflow recovery: clear (CPU-only) and force rebuild on next frame.
        // The actual texture clear is deferred to next draw_frame's flush_uploads.
        if cache.atlas_needs_clear {
            cache.clear_cache();
            cache.atlas_needs_clear = false;
            self.clear_render_caches();
            for grid in self.core.pane_grids.values_mut() {
                grid.dirty = true;
            }
            animating = true; // ensure redraw to rebuild glyphs
            log::info!("atlas overflow: cleared cache, will rebuild next frame");
        }

        if animating && let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

/// Emit a dashed border (4 edges) as rect segments.
fn emit_dashed_border(
    rects: &mut Vec<Rect>,
    tr: &GeoRect,
    bw: f32,
    dash: f32,
    gap: f32,
    color: [f32; 4],
) {
    let mut emit = |x, y, total, horizontal: bool| {
        let mut off = 0.0;
        while off < total {
            let seg = dash.min(total - off);
            if horizontal {
                rects.push(Rect {
                    x: x + off,
                    y,
                    w: seg,
                    h: bw,
                    color,
                });
            } else {
                rects.push(Rect {
                    x,
                    y: y + off,
                    w: bw,
                    h: seg,
                    color,
                });
            }
            off += dash + gap;
        }
    };
    emit(tr.x, tr.y, tr.w, true); // top
    emit(tr.x, tr.y + tr.h - bw, tr.w, true); // bottom
    emit(tr.x, tr.y + bw, tr.h - bw * 2.0, false); // left
    emit(tr.x + tr.w - bw, tr.y + bw, tr.h - bw * 2.0, false); // right
}

fn scissor_rect(tr: &GeoRect, viewport_w: f32, viewport_h: f32) -> Option<(u32, u32, u32, u32)> {
    let left = tr.x.max(0.0).floor();
    let top = tr.y.max(0.0).floor();
    let right = (tr.x + tr.w).min(viewport_w).ceil();
    let bottom = (tr.y + tr.h).min(viewport_h).ceil();
    if right <= left || bottom <= top {
        None
    } else {
        Some((
            left as u32,
            top as u32,
            (right - left).max(1.0) as u32,
            (bottom - top).max(1.0) as u32,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ciri_config::config::CiriConfig;
    use std::sync::Arc;

    fn make_app() -> App {
        App::new(CiriConfig::default(), "test-session")
    }

    #[test]
    fn pane_visual_state_applies_move_offset_to_tile_rect() {
        let mut app = make_app();
        let config = app.anim_config();
        app.core.anim_mgr.ensure_pane_registered(7);
        app.core
            .anim_mgr
            .start_move_animation(7, 48.0, -12.0, &config);

        let visual = app
            .pane_visual_state(
                7,
                GeoRect::new(100.0, 80.0, 320.0, 240.0),
                1.0,
                1600.0,
                900.0,
            )
            .unwrap();

        assert!((visual.tr.x - 148.0).abs() < 0.01);
        assert!((visual.tr.y - 68.0).abs() < 0.01);
    }

    #[test]
    fn build_tiles_places_focused_pane_in_late_layer() {
        let mut app = make_app();
        app.core.config.appearance.focus_ring.style = FocusRingStyle::Solid;

        let tiles = vec![
            (1, GeoRect::new(0.0, 0.0, 300.0, 200.0), false),
            (2, GeoRect::new(320.0, 0.0, 300.0, 200.0), true),
        ];
        let mut bg_rects = Vec::new();
        let mut glyphs = Vec::new();
        let mut color_glyphs = Vec::new();
        let mut glyph_batches = Vec::new();
        let mut color_glyph_batches = Vec::new();
        let mut active_glyph_batches = Vec::new();
        let mut active_color_glyph_batches = Vec::new();

        let active_bg_start = app.build_tiles(
            &tiles,
            1.0,
            1600.0,
            900.0,
            &mut bg_rects,
            &mut glyphs,
            &mut color_glyphs,
            &mut glyph_batches,
            &mut color_glyph_batches,
            &mut active_glyph_batches,
            &mut active_color_glyph_batches,
        );

        assert_eq!(active_bg_start, 2);
        assert!(active_bg_start < bg_rects.len());
        assert!(bg_rects[0].x < bg_rects[active_bg_start].x);
        assert!(glyph_batches.is_empty());
        assert!(active_glyph_batches.is_empty());
    }

    #[test]
    fn image_display_size_uses_pixels_for_sixel_images() {
        let img = crate::app::ClientImagePlacement {
            image_id: 1,
            col: 0,
            row: 0,
            width_cells: 33,
            height_cells: 17,
            pixel_width: 260,
            pixel_height: 260,
            display_mode: ImageDisplayMode::Pixels,
            format: "rgba".to_string(),
            data: Arc::new(vec![0; 260 * 260 * 4]),
        };

        let (w, h) = App::image_display_size(&img, 9.0, 19.0, 1.0);
        assert_eq!((w, h), (260.0, 260.0));
    }

    #[test]
    fn image_display_size_uses_cells_for_kitty_images() {
        let img = crate::app::ClientImagePlacement {
            image_id: 1,
            col: 0,
            row: 0,
            width_cells: 4,
            height_cells: 3,
            pixel_width: 200,
            pixel_height: 150,
            display_mode: ImageDisplayMode::Cells,
            format: "rgba".to_string(),
            data: Arc::new(vec![0; 200 * 150 * 4]),
        };

        let (w, h) = App::image_display_size(&img, 9.0, 19.0, 1.0);
        assert_eq!((w, h), (36.0, 57.0));
    }
}
