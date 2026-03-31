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

pub(crate) struct RenderOutput<'a> {
    pub bg_rects: &'a mut Vec<Rect>,
    pub glyphs: &'a mut Vec<GlyphInstance>,
    pub color_glyphs: &'a mut Vec<GlyphInstance>,
    pub glyph_batches: &'a mut Vec<ScissoredRange>,
    pub color_glyph_batches: &'a mut Vec<ScissoredRange>,
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

    pub fn build_tiles(
        &mut self,
        tiles: &[(u64, GeoRect, bool)],
        zoom: f32,
        vw: f32,
        vh: f32,
        out: &mut RenderOutput<'_>,
    ) {
        let zoom_threshold = self.core.config.animation.zoom_threshold;
        let padding = self.core.config.appearance.padding;
        let border_w = self.core.config.appearance.border_width;
        let active_border = if self.core.config.appearance.active_border_color.is_empty() {
            ThemeConfig::parse_color(&self.core.config.theme.border_active)
        } else {
            ThemeConfig::parse_color(&self.core.config.appearance.active_border_color)
        };
        let inactive_border = if self.core.config.appearance.inactive_border_color.is_empty() {
            ThemeConfig::parse_color(&self.core.config.theme.border_inactive)
        } else {
            ThemeConfig::parse_color(&self.core.config.appearance.inactive_border_color)
        };
        // inactive_opacity is now handled by AnimConfig in the animation manager
        let bg_color = ThemeConfig::parse_color(&self.core.config.theme.background);
        let link_color = ThemeConfig::parse_color(&self.core.config.theme.accent);
        let accent = ThemeConfig::parse_color(&self.core.config.theme.accent);
        let cache_tile_glyphs = self.pending_resize.is_none();

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
            let scissor = scissor_rect(&tr, vw, vh);
            if scissor.is_none() {
                continue;
            }

            // Focus ring: configurable style for active pane
            if *is_active {
                match &self.core.config.appearance.focus_ring.style {
                    FocusRingStyle::Glow => {
                        let fr = &self.core.config.appearance.focus_ring;
                        let layers = fr.glow_layers.max(1) as usize;
                        for layer in (0..layers).rev() {
                            let offset = fr.glow_radius * (layer + 1) as f32 / layers as f32;
                            let alpha =
                                active_border[3] * (1.0 - layer as f32 / layers as f32) * 0.3;
                            out.bg_rects.push(Rect {
                                x: tr.x - offset,
                                y: tr.y - offset,
                                w: tr.w + offset * 2.0,
                                h: tr.h + offset * 2.0,
                                color: [
                                    active_border[0],
                                    active_border[1],
                                    active_border[2],
                                    alpha,
                                ],
                            });
                        }
                        out.bg_rects.push(Rect {
                            x: tr.x,
                            y: tr.y,
                            w: tr.w,
                            h: tr.h,
                            color: active_border,
                        });
                    }
                    FocusRingStyle::Dashed => {
                        let fr = &self.core.config.appearance.focus_ring;
                        let bw = border_w * zoom;
                        emit_dashed_border(
                            out.bg_rects,
                            &tr,
                            bw,
                            fr.dash_length,
                            fr.gap_length,
                            active_border,
                        );
                    }
                    FocusRingStyle::Solid => {
                        out.bg_rects.push(Rect {
                            x: tr.x,
                            y: tr.y,
                            w: tr.w,
                            h: tr.h,
                            color: active_border,
                        });
                    }
                }
            } else {
                out.bg_rects.push(Rect {
                    x: tr.x,
                    y: tr.y,
                    w: tr.w,
                    h: tr.h,
                    color: inactive_border,
                });
            }
            out.bg_rects.push(Rect {
                x: tr.x + border_w * zoom,
                y: tr.y + border_w * zoom,
                w: tr.w - border_w * zoom * 2.0,
                h: tr.h - border_w * zoom * 2.0,
                color: bg_color,
            });
            if self.core.overview.active
                && self
                    .core
                    .overview
                    .hovered_pane
                    .is_some_and(|(_, hovered_pane_id)| hovered_pane_id == *pane_id)
            {
                let hover_border_w = (border_w * zoom).max(1.0);
                for layer in (1..=2).rev() {
                    let spread = layer as f32 * 2.0 * zoom.max(1.0);
                    out.bg_rects.push(Rect {
                        x: tr.x - spread,
                        y: tr.y - spread,
                        w: tr.w + spread * 2.0,
                        h: tr.h + spread * 2.0,
                        color: [accent[0], accent[1], accent[2], 0.08 / layer as f32],
                    });
                }
                out.bg_rects.push(Rect {
                    x: tr.x,
                    y: tr.y,
                    w: tr.w,
                    h: hover_border_w,
                    color: [accent[0], accent[1], accent[2], 0.65],
                });
                out.bg_rects.push(Rect {
                    x: tr.x,
                    y: tr.y + tr.h - hover_border_w,
                    w: tr.w,
                    h: hover_border_w,
                    color: [accent[0], accent[1], accent[2], 0.65],
                });
                out.bg_rects.push(Rect {
                    x: tr.x,
                    y: tr.y,
                    w: hover_border_w,
                    h: tr.h,
                    color: [accent[0], accent[1], accent[2], 0.65],
                });
                out.bg_rects.push(Rect {
                    x: tr.x + tr.w - hover_border_w,
                    y: tr.y,
                    w: hover_border_w,
                    h: tr.h,
                    color: [accent[0], accent[1], accent[2], 0.65],
                });

                // Action bar is rendered in the overlay layer (build_ui)
                // to avoid being clipped by pane scissor rects.
            }

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
                    out.bg_rects.push(Rect {
                        x: c.x,
                        y: c.y,
                        w: c.w,
                        h: c.h,
                        color: r.color,
                    });
                }
            }

            if self.core.cursor_blink_visible && *is_active {
                for cursor in &view.cursor_rects {
                    let src = GeoRect::new(
                        inner_x + cursor.x * zoom,
                        inner_y + cursor.y * zoom,
                        cursor.w * zoom,
                        cursor.h * zoom,
                    );
                    if let Some(c) = src.intersection(&tr) {
                        out.bg_rects.push(Rect {
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
                    out.bg_rects.push(Rect {
                        x: c.x,
                        y: c.y,
                        w: c.w,
                        h: c.h,
                        color: sb.color,
                    });
                }
            }

            // Selection overlay — uses the terminal foreground color as a
            // semi-opaque selection background (matching ghostty's default).
            // One rect per row keeps performance at O(rows).
            if let Some(sel) = &self.core.selection
                && sel.pane_id == *pane_id
                && sel.start != sel.end // Don't render zero-width selection (single click anchor)
                && let Some(grid) = self.core.pane_grids.get(pane_id)
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
                    if let Some(c) = src.intersection(&tr) {
                        out.bg_rects.push(Rect {
                            x: c.x,
                            y: c.y,
                            w: c.w,
                            h: c.h,
                            color: sel_color,
                        });
                    }
                }
            }

            if let Some(link) = &self.core.hovered_link
                && link.pane_id == *pane_id
                && let Some(grid) = self.core.pane_grids.get(pane_id)
                && let Some(viewport_row) = grid.buffer_to_viewport_row(link.start.1)
            {
                let (cw, ch) = self.cell_dimensions();
                let underline_h = (zoom.max(1.0)).clamp(1.0, 2.0);
                let sx = inner_x + link.start.0 as f32 * cw * zoom;
                let sy = inner_y + (viewport_row as f32 + 1.0) * ch * zoom - underline_h - zoom;
                let sw = (link.end.0 - link.start.0 + 1) as f32 * cw * zoom;
                let src = GeoRect::new(sx, sy, sw, underline_h);
                if let Some(c) = src.intersection(&tr) {
                    out.bg_rects.push(Rect {
                        x: c.x,
                        y: c.y,
                        w: c.w,
                        h: c.h,
                        color: link_color,
                    });
                }
            }

            // Search match highlights
            if let Some(search) = &self.core.search_state
                && search.pane_id == *pane_id
                && let Some(grid) = self.core.pane_grids.get(pane_id)
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
                        [1.0, 0.6, 0.0, 0.5] // orange for current match
                    } else {
                        [1.0, 1.0, 0.0, 0.3] // yellow for other matches
                    };

                    let src = GeoRect::new(sx, sy, sw, sh);
                    if let Some(c) = src.intersection(&tr) {
                        out.bg_rects.push(Rect {
                            x: c.x,
                            y: c.y,
                            w: c.w,
                            h: c.h,
                            color,
                        });
                    }
                }
            }

            // Animated focus opacity (smooth transition on focus change)
            let focus_dim = self.core.anim_mgr.pane_focus_opacity(*pane_id);
            let open_opacity = self.core.anim_mgr.pane_open_opacity(*pane_id);
            let drag_dim = self.core.anim_mgr.pane_drag_dim(*pane_id);
            let dim = focus_dim * open_opacity * drag_dim;

            // Combined pane offset: open slide + move animation
            let slide_progress = self.core.anim_mgr.pane_open_slide(*pane_id);
            let (open_dx, open_dy) = match self.core.config.animation.pane_open_style {
                PaneOpenStyle::SlideUp | PaneOpenStyle::FadeSlideUp => {
                    (0.0, -tr.h * slide_progress)
                }
                PaneOpenStyle::SlideDown => (0.0, tr.h * slide_progress),
                PaneOpenStyle::SlideLeft => (-tr.w * slide_progress, 0.0),
                PaneOpenStyle::Fade => (0.0, 0.0),
            };
            let (move_dx, move_dy) = self.core.anim_mgr.pane_move_offset(*pane_id);
            let offset_dx = open_dx + move_dx * zoom;
            let offset_dy = open_dy + move_dy * zoom;

            // Check if cached tile out.glyphs are still valid
            // Include slide offsets in cache key so animations invalidate the cache
            let tile_key = (
                (inner_x + offset_dx).to_bits(),
                (inner_y + offset_dy).to_bits(),
                zoom.to_bits(),
                dim.to_bits(),
            );
            let glyph_start = out.glyphs.len();
            let color_start = out.color_glyphs.len();
            let cache_hit = cache_tile_glyphs
                && self
                    .cached_tile_glyphs
                    .get(pane_id)
                    .is_some_and(|c| c.generation == view.generation && c.key == tile_key);

            if cache_hit {
                let cached = self.cached_tile_glyphs.get(pane_id).unwrap();
                out.glyphs.extend_from_slice(&cached.glyphs);
                out.color_glyphs.extend_from_slice(&cached.color_glyphs);
            } else {
                // Convert relative glyphs to pixel-coord GlyphInstances.
                // Tile clipping is handled by the render-pass scissor rect.
                let make_instance =
                    |g: &terminal::RelativeGlyph, color: [f32; 4]| -> Option<GlyphInstance> {
                        let sx = (inner_x + g.px * zoom + offset_dx).round();
                        let sy = (inner_y + g.py * zoom + offset_dy).round();
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

                // Regular text glyphs (alpha atlas): dim the foreground color
                out.glyphs
                    .extend(view.glyph_instances.iter().filter_map(|g| {
                        let color = [
                            g.color[0] * dim,
                            g.color[1] * dim,
                            g.color[2] * dim,
                            g.color[3],
                        ];
                        make_instance(g, color)
                    }));

                // Color emoji glyphs (RGBA atlas)
                let emoji_color = [dim, dim, dim, 1.0];
                out.color_glyphs.extend(
                    view.color_glyph_instances
                        .iter()
                        .filter_map(|g| make_instance(g, emoji_color)),
                );

                // Cache the result
                if cache_tile_glyphs {
                    self.cached_tile_glyphs.insert(
                        *pane_id,
                        super::CachedTileGlyphs {
                            generation: view.generation,
                            key: tile_key,
                            glyphs: out.glyphs[glyph_start..].to_vec(),
                            color_glyphs: out.color_glyphs[color_start..].to_vec(),
                        },
                    );
                }
            }

            if let Some((sx, sy, sw, sh)) = scissor {
                if glyph_start < out.glyphs.len() {
                    out.glyph_batches.push(ScissoredRange {
                        x: sx,
                        y: sy,
                        w: sw,
                        h: sh,
                        start: glyph_start,
                        end: out.glyphs.len(),
                    });
                }
                if color_start < out.color_glyphs.len() {
                    out.color_glyph_batches.push(ScissoredRange {
                        x: sx,
                        y: sy,
                        w: sw,
                        h: sh,
                        start: color_start,
                        end: out.color_glyphs.len(),
                    });
                }
            }

            // Fade-in overlay for newly opened panes
            if open_opacity < 1.0 {
                let overlay_alpha = 1.0 - open_opacity;
                out.bg_rects.push(Rect {
                    x: tr.x,
                    y: tr.y,
                    w: tr.w,
                    h: tr.h,
                    color: [0.0, 0.0, 0.0, overlay_alpha],
                });
            }
        }

        // Render closing panes as fading-out rects
        for (rect, opacity, _slide) in self.core.anim_mgr.closing_panes() {
            out.bg_rects.push(Rect {
                x: rect.x,
                y: rect.y,
                w: rect.w,
                h: rect.h,
                color: [0.1, 0.1, 0.1, opacity * 0.5],
            });
        }
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
        let zoom_threshold = self.core.config.animation.zoom_threshold;
        for (pane_id, tile_rect, _) in tiles {
            let intensity = self.core.anim_mgr.bell_flash(*pane_id);
            if intensity <= 0.0 {
                continue;
            }
            let alpha = 0.15 * intensity;
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

    pub fn build_image_placements(
        &mut self,
        tiles: &[(u64, GeoRect, bool)],
        zoom: f32,
        vw: f32,
        vh: f32,
        bg_rects: &mut Vec<Rect>,
        glyphs: &mut Vec<GlyphInstance>,
    ) {
        if self.core.image_placements.is_empty() {
            return;
        }
        let atlas = self.glyph_cache.as_mut().unwrap();
        let border_w = self.core.config.appearance.border_width;
        let padding = self.core.config.appearance.padding;
        let (cw, ch) = (atlas.cell_width, atlas.cell_height);
        let zoom_threshold = self.core.config.animation.zoom_threshold;

        for (pane_id, tile_rect, _) in tiles {
            let Some(placements) = self.core.image_placements.get(pane_id) else {
                continue;
            };
            if placements.is_empty() {
                continue;
            }

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

            let inner_x = tr.x + (border_w + padding) * zoom;
            let inner_y = tr.y + (border_w + padding) * zoom;

            for img in placements {
                let ix = inner_x + img.col as f32 * cw * zoom;
                let iy = inner_y + img.row as f32 * ch * zoom;
                let iw = img.width_cells as f32 * cw * zoom;
                let ih = img.height_cells as f32 * ch * zoom;

                // Image placeholder: dark semi-transparent background
                let src = GeoRect::new(ix, iy, iw, ih);
                if let Some(c) = src.intersection(&tr) {
                    bg_rects.push(Rect {
                        x: c.x,
                        y: c.y,
                        w: c.w,
                        h: c.h,
                        color: [0.1, 0.1, 0.15, 0.8],
                    });
                    // Border
                    let bw = (1.0 * zoom).max(1.0);
                    bg_rects.push(Rect {
                        x: c.x,
                        y: c.y,
                        w: c.w,
                        h: bw,
                        color: [0.4, 0.6, 0.8, 0.6],
                    });
                    bg_rects.push(Rect {
                        x: c.x,
                        y: c.y + c.h - bw,
                        w: c.w,
                        h: bw,
                        color: [0.4, 0.6, 0.8, 0.6],
                    });
                    bg_rects.push(Rect {
                        x: c.x,
                        y: c.y,
                        w: bw,
                        h: c.h,
                        color: [0.4, 0.6, 0.8, 0.6],
                    });
                    bg_rects.push(Rect {
                        x: c.x + c.w - bw,
                        y: c.y,
                        w: bw,
                        h: c.h,
                        color: [0.4, 0.6, 0.8, 0.6],
                    });
                }

                // "IMG" label
                let label = format!("IMG {}x{}", img.pixel_width, img.pixel_height);
                let baseline = ch * self.core.config.statusbar.text_baseline;
                emit_status_text(
                    atlas,
                    &label,
                    &TextEmitParams {
                        x_start: ix + 4.0 * zoom,
                        y: iy + 2.0 * zoom,
                        cell_width: cw * zoom,
                        baseline: baseline * zoom,
                        color: [0.6, 0.8, 1.0, 0.7],
                    },
                    glyphs,
                );
            }
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
        let cache = self.glyph_cache.as_mut().unwrap();
        let shaper = self.text_shaper.as_ref().unwrap();
        let (vw, vh) = renderer.surface_size();
        let vw_f = vw as f32;
        let vh_f = vh as f32;
        let zoom = self.core.anim_mgr.overview_zoom.value() as f32;
        let zoom_threshold = self.core.config.animation.zoom_threshold;

        let vox = self.core.anim_mgr.view_offset_x.value() as f32;
        let voy = self.core.anim_mgr.view_offset_y.value() as f32;
        let tiles = if self.core.overview.active || zoom < zoom_threshold {
            self.core.workspaces.all_tiles_2d(vox, voy)
        } else {
            self.core.workspaces.visible_tiles_2d(vox, voy)
        };

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
                let (cur_col, cur_line, cur_shape) =
                    if let Some((col, line)) = grid.cursor_in_viewport() {
                        (col, line, grid.cursor_shape)
                    } else {
                        (0, 0, CURSOR_HIDDEN)
                    };
                let inputs = terminal::PackedViewInputs {
                    cells: &visible,
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
                    let (cur_col, cur_line, cur_shape) =
                        if let Some((col, line)) = grid.cursor_in_viewport() {
                            (col, line, grid.cursor_shape)
                        } else {
                            (0, 0, CURSOR_HIDDEN)
                        };
                    // Clear dirty before visible_cells() to avoid borrow conflict
                    // (clear_dirty only resets flags, not cell data)
                    grid.clear_dirty();
                    let visible = grid.visible_cells();
                    if let Some(view) = self.cached_views.get_mut(pane_id) {
                        let inputs = terminal::PackedViewInputs {
                            cells: &visible,
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
        bg_rects.clear();
        glyphs.clear();
        color_glyphs.clear();
        glyph_batches.clear();
        color_glyph_batches.clear();

        self.build_tiles(
            &offset_tiles,
            zoom,
            vw_f,
            vh_f,
            &mut RenderOutput {
                bg_rects: &mut bg_rects,
                glyphs: &mut glyphs,
                color_glyphs: &mut color_glyphs,
                glyph_batches: &mut glyph_batches,
                color_glyph_batches: &mut color_glyph_batches,
            },
        );
        let pane_glyph_end = glyphs.len();
        let pane_color_glyph_end = color_glyphs.len();
        let overlay_bg_start = bg_rects.len();
        self.build_ui(vw_f, vh_f, &mut bg_rects, &mut glyphs);
        self.build_search_bar(&offset_tiles, vw_f, vh_f, &mut bg_rects, &mut glyphs);
        self.build_bell_flash(&offset_tiles, zoom, vw_f, vh_f, &mut bg_rects);
        self.build_ime_preedit(&offset_tiles, vw_f, vh_f, &mut bg_rects, &mut glyphs);
        self.build_image_placements(&offset_tiles, zoom, vw_f, vh_f, &mut bg_rects, &mut glyphs);

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

        // Atlas overflow recovery: clear (CPU-only) and force rebuild on next frame.
        // The actual texture clear is deferred to next draw_frame's flush_uploads.
        if cache.atlas_needs_clear {
            cache.clear_cache();
            cache.atlas_needs_clear = false;
            self.cached_views.clear();
            self.cached_tile_glyphs.clear();
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
