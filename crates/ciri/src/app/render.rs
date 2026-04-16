use ciri_config::config::{FocusRingStyle, PaneOpenStyle};
use ciri_config::theme::ThemeConfig;
use ciri_layout::geometry::Rect as GeoRect;
use ciri_protocol::message::*;
use ciri_render::FrameScene;
use ciri_render::glyph_cache::{GlyphInstance, ScissoredRange};
use ciri_render::rect::Rect;
use ciri_render::terminal;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Instant;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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

struct PaneGlyphFragment {
    glyphs: Vec<GlyphInstance>,
    color_glyphs: Vec<GlyphInstance>,
    scissor: (u32, u32, u32, u32),
    is_active: bool,
    snapshot: u64,
}

impl App {
    fn zero_glyph_instance() -> GlyphInstance {
        GlyphInstance {
            pos: [0.0, 0.0],
            size: [0.0, 0.0],
            uv_pos: [0.0, 0.0],
            uv_size: [0.0, 0.0],
            color: [0.0, 0.0, 0.0, 0.0],
        }
    }

    fn ordered_pane_tiles(tiles: &[(u64, GeoRect, bool)]) -> Vec<(u64, GeoRect, bool)> {
        let mut ordered = Vec::with_capacity(tiles.len());
        ordered.extend(
            tiles
                .iter()
                .copied()
                .filter(|(_, _, is_active)| !*is_active),
        );
        ordered.extend(tiles.iter().copied().filter(|(_, _, is_active)| *is_active));
        ordered
    }

    fn glyph_scene_capacity(len: usize) -> usize {
        if len == 0 { 0 } else { len + (len / 4).max(8) }
    }

    fn tile_paint_config(&self) -> TilePaintConfig {
        TilePaintConfig {
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
            // Per-row tile glyph/background caching disabled — rebuild from
            // atlas each frame (matches Ghostty / Windows Terminal approach).
            // render_snapshot_hash still skips entire idle frames.
            cache_tile_glyphs: false,
        }
    }

    fn pane_glyph_snapshot_hash(
        &self,
        pane_id: u64,
        tile_rect: GeoRect,
        is_active: bool,
        zoom: f32,
        vw: f32,
        vh: f32,
    ) -> Option<u64> {
        let visual = self.pane_visual_state(pane_id, tile_rect, zoom, vw, vh)?;
        let mut hasher = DefaultHasher::new();
        pane_id.hash(&mut hasher);
        is_active.hash(&mut hasher);
        visual.tr.x.to_bits().hash(&mut hasher);
        visual.tr.y.to_bits().hash(&mut hasher);
        visual.tr.w.to_bits().hash(&mut hasher);
        visual.tr.h.to_bits().hash(&mut hasher);
        visual.inner_x.to_bits().hash(&mut hasher);
        visual.inner_y.to_bits().hash(&mut hasher);
        visual.dim.to_bits().hash(&mut hasher);
        visual.scissor.hash(&mut hasher);

        if let Some(view) = self.cached_views.get(&pane_id) {
            view.generation.hash(&mut hasher);
        } else {
            0u64.hash(&mut hasher);
        }

        self.core
            .prediction
            .pane_visual_serial(pane_id)
            .hash(&mut hasher);
        self.hash_images_for_pane(pane_id, &mut hasher);
        Some(hasher.finish())
    }

    fn build_pane_glyph_fragment(
        &mut self,
        pane_id: u64,
        tile_rect: GeoRect,
        is_active: bool,
        zoom: f32,
        vw: f32,
        vh: f32,
        paint: TilePaintConfig,
    ) -> Option<PaneGlyphFragment> {
        let visual = self.pane_visual_state(pane_id, tile_rect, zoom, vw, vh)?;
        let view = self.cached_views.get(&pane_id)?;
        let snapshot =
            self.pane_glyph_snapshot_hash(pane_id, tile_rect, is_active, zoom, vw, vh)?;

        let mut glyphs = Vec::new();
        let mut color_glyphs = Vec::new();
        let mut dirty_glyph_ranges = Vec::new();
        let mut dirty_color_ranges = Vec::new();
        let tile_key = (
            visual.inner_x.to_bits(),
            visual.inner_y.to_bits(),
            zoom.to_bits(),
            visual.dim.to_bits(),
        );
        Self::emit_tile_glyphs(
            &mut self.cached_tile_glyphs,
            pane_id,
            view,
            visual.inner_x,
            visual.inner_y,
            zoom,
            visual.dim,
            tile_key,
            paint.cache_tile_glyphs,
            &mut dirty_glyph_ranges,
            &mut dirty_color_ranges,
            &mut glyphs,
            &mut color_glyphs,
        );

        self.build_pane_images(
            pane_id,
            visual.inner_x,
            visual.inner_y,
            zoom,
            visual.dim,
            &mut color_glyphs,
        );

        Some(PaneGlyphFragment {
            glyphs,
            color_glyphs,
            scissor: visual.scissor,
            is_active,
            snapshot,
        })
    }

    fn emit_tile_background_rows(
        background_cache: &mut std::collections::HashMap<u64, super::CachedTileBackgrounds>,
        pane_id: u64,
        view: &terminal::TerminalView,
        inner_x: f32,
        inner_y: f32,
        zoom: f32,
        tr: &GeoRect,
        tile_key: (u32, u32, u32, u32),
        cache_tile_backgrounds: bool,
        dirty_bg_ranges: &mut Vec<(usize, usize)>,
        bg_rects: &mut Vec<Rect>,
    ) {
        let make_rect = |r: &Rect| -> Option<Rect> {
            let src = GeoRect::new(
                inner_x + r.x * zoom,
                inner_y + r.y * zoom,
                r.w * zoom,
                r.h * zoom,
            );
            src.intersection(tr).map(|c| Rect {
                x: c.x,
                y: c.y,
                w: c.w,
                h: c.h,
                color: r.color,
            })
        };

        if !cache_tile_backgrounds {
            for row in 0..view.row_count() {
                let row_start = bg_rects.len();
                bg_rects.extend(view.row_bg_rects(row).iter().filter_map(make_rect));
                let row_end = bg_rects.len();
                if row_start < row_end {
                    dirty_bg_ranges.push((row_start, row_end));
                }
            }
            return;
        }

        let cached =
            background_cache
                .entry(pane_id)
                .or_insert_with(|| super::CachedTileBackgrounds {
                    key: tile_key,
                    rows: Vec::new(),
                });

        if cached.key != tile_key || cached.rows.len() != view.row_count() {
            cached.key = tile_key;
            cached.rows = vec![super::CachedTileBackgroundRow::default(); view.row_count()];
            for row in &mut cached.rows {
                row.epoch = 0;
                row.bg_rects.clear();
            }
        } else if view.last_scroll_shift != 0 {
            let shift = view.last_scroll_shift;
            let amount = shift.unsigned_abs() as usize;
            if amount > 0 && amount < cached.rows.len() {
                if shift > 0 {
                    cached.rows.rotate_right(amount);
                } else {
                    cached.rows.rotate_left(amount);
                }
                let delta_y = shift as f32 * view.cell_height * zoom;
                for row in &mut cached.rows {
                    for rect in &mut row.bg_rects {
                        rect.y += delta_y;
                    }
                }
            }
        }

        for row in 0..view.row_count() {
            let row_start = bg_rects.len();
            let rebuilt = cached.rows[row].epoch != view.row_epoch(row);
            if rebuilt {
                cached.rows[row].epoch = view.row_epoch(row);
                cached.rows[row].bg_rects.clear();
                cached.rows[row]
                    .bg_rects
                    .extend(view.row_bg_rects(row).iter().filter_map(make_rect));
            }
            bg_rects.extend_from_slice(&cached.rows[row].bg_rects);
            let row_end = bg_rects.len();
            if rebuilt && row_start < row_end {
                dirty_bg_ranges.push((row_start, row_end));
            }
        }
    }

    fn build_tile_backgrounds(
        &mut self,
        pane_id: u64,
        tile_rect: GeoRect,
        is_active: bool,
        zoom: f32,
        vw: f32,
        vh: f32,
        paint: TilePaintConfig,
        bg_rects: &mut Vec<Rect>,
    ) {
        let Some(visual) = self.pane_visual_state(pane_id, tile_rect, zoom, vw, vh) else {
            return;
        };
        let tr = visual.tr;
        let inner_x = visual.inner_x;
        let inner_y = visual.inner_y;
        let tile_key = (
            inner_x.to_bits(),
            inner_y.to_bits(),
            zoom.to_bits(),
            visual.dim.to_bits(),
        );

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

        bg_rects.push(Rect {
            x: tr.x + paint.border_w * zoom,
            y: tr.y + paint.border_w * zoom,
            w: tr.w - paint.border_w * zoom * 2.0,
            h: tr.h - paint.border_w * zoom * 2.0,
            color: paint.bg_color,
        });

        self.emit_overview_hover(pane_id, &tr, zoom, &paint, bg_rects);

        let Some(view) = self.cached_views.get(&pane_id) else {
            return;
        };

        Self::emit_tile_background_rows(
            &mut self.cached_tile_backgrounds,
            pane_id,
            view,
            inner_x,
            inner_y,
            zoom,
            &tr,
            tile_key,
            paint.cache_tile_glyphs,
            &mut self.render_bufs.dirty_bg_ranges,
            bg_rects,
        );

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

        self.emit_selection_highlight(pane_id, inner_x, inner_y, zoom, &tr, bg_rects);
        self.emit_link_underline(pane_id, inner_x, inner_y, zoom, &tr, &paint, bg_rects);
        self.emit_search_highlights(pane_id, inner_x, inner_y, zoom, &tr, bg_rects);

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

    fn hash_geo_rect(hasher: &mut DefaultHasher, rect: &GeoRect) {
        rect.x.to_bits().hash(hasher);
        rect.y.to_bits().hash(hasher);
        rect.w.to_bits().hash(hasher);
        rect.h.to_bits().hash(hasher);
    }

    fn hash_selection(&self, hasher: &mut DefaultHasher) {
        if let Some(sel) = &self.core.selection {
            true.hash(hasher);
            sel.pane_id.hash(hasher);
            sel.start.hash(hasher);
            sel.end.hash(hasher);
            sel.active.hash(hasher);
        } else {
            false.hash(hasher);
        }
    }

    fn hash_hovered_link(&self, hasher: &mut DefaultHasher) {
        if let Some(link) = &self.core.hovered_link {
            true.hash(hasher);
            link.pane_id.hash(hasher);
            link.url.hash(hasher);
            link.start.hash(hasher);
            link.end.hash(hasher);
        } else {
            false.hash(hasher);
        }
    }

    fn hash_search_state(&self, hasher: &mut DefaultHasher) {
        if let Some(search) = &self.core.search_state {
            true.hash(hasher);
            search.query.hash(hasher);
            search.current_match_idx.hash(hasher);
            search.pane_id.hash(hasher);
            search.original_scroll_offset.hash(hasher);
            search.matches.len().hash(hasher);
            if let Some(first) = search.matches.first() {
                first.buffer_row.hash(hasher);
                first.start_col.hash(hasher);
                first.end_col.hash(hasher);
            }
            if let Some(current) = search.matches.get(search.current_match_idx) {
                current.buffer_row.hash(hasher);
                current.start_col.hash(hasher);
                current.end_col.hash(hasher);
            }
            if let Some(last) = search.matches.last() {
                last.buffer_row.hash(hasher);
                last.start_col.hash(hasher);
                last.end_col.hash(hasher);
            }
        } else {
            false.hash(hasher);
        }
    }

    fn hash_command_palette(&self, hasher: &mut DefaultHasher) {
        if let Some(palette) = &self.core.command_palette {
            true.hash(hasher);
            palette.query.hash(hasher);
            palette.selected_idx.hash(hasher);
            palette.hovered_idx.hash(hasher);
            palette.sessions_only.hash(hasher);
            palette.remote_loading.hash(hasher);
            palette.remote_error.hash(hasher);
            palette.remote_input_mode.hash(hasher);
            palette.filtered.hash(hasher);
            palette.entries.len().hash(hasher);
            for entry in &palette.entries {
                entry.label.hash(hasher);
                let kind = match &entry.kind {
                    super::PaletteEntryKind::SectionHeader(_) => 0u8,
                    super::PaletteEntryKind::Action(_) => 1,
                    super::PaletteEntryKind::GoToSession { .. } => 2,
                    super::PaletteEntryKind::KillSession(_) => 3,
                    super::PaletteEntryKind::RemoteHost { .. } => 4,
                    super::PaletteEntryKind::RemoteSession { .. } => 5,
                    super::PaletteEntryKind::SshShell { .. } => 6,
                    super::PaletteEntryKind::DirectConnect { .. } => 7,
                    super::PaletteEntryKind::ConnectRemotePrompt => 8,
                };
                kind.hash(hasher);
            }
        } else {
            false.hash(hasher);
        }
    }

    fn hash_context_menu(&self, hasher: &mut DefaultHasher) {
        let menu = &self.core.context_menu;
        menu.visible.hash(hasher);
        if menu.visible {
            menu.x.to_bits().hash(hasher);
            menu.y.to_bits().hash(hasher);
            menu.target_pane_id.hash(hasher);
            menu.hovered_index.hash(hasher);
            menu.items.len().hash(hasher);
            for item in &menu.items {
                item.label.hash(hasher);
                item.enabled.hash(hasher);
            }
        }
    }

    fn hash_pending_paste(&self, hasher: &mut DefaultHasher) {
        if let Some(paste) = &self.core.pending_paste {
            true.hash(hasher);
            paste.preview.hash(hasher);
            paste.hovered_button.hash(hasher);
            paste.target.hash(hasher);
            paste.info.text.hash(hasher);
        } else {
            false.hash(hasher);
        }
    }

    pub(crate) fn ui_scene_hash(
        &self,
        vw: f32,
        vh: f32,
        cell_w: f32,
        cell_h: f32,
        ui_line_h: f32,
    ) -> u64 {
        let mut hasher = DefaultHasher::new();

        vw.to_bits().hash(&mut hasher);
        vh.to_bits().hash(&mut hasher);
        cell_w.to_bits().hash(&mut hasher);
        cell_h.to_bits().hash(&mut hasher);
        ui_line_h.to_bits().hash(&mut hasher);

        self.core.workspaces.active_workspace_idx.hash(&mut hasher);
        self.core.workspaces.workspaces.len().hash(&mut hasher);
        self.window_focused.hash(&mut hasher);
        self.core.broadcast_mode.hash(&mut hasher);
        self.core.input.is_locked().hash(&mut hasher);
        self.core.input.is_awaiting_action().hash(&mut hasher);
        self.core.input.current_mode_name().hash(&mut hasher);
        self.core.hovered_top_bar_region.hash(&mut hasher);
        self.core.hovered_pane_tab.hash(&mut hasher);
        self.core.pane_tab_scroll.to_bits().hash(&mut hasher);
        self.pane_tab_scroll_max().to_bits().hash(&mut hasher);
        self.core.overview.active.hash(&mut hasher);
        self.core.overview.hovered_pane.hash(&mut hasher);
        self.core.overview_action_hover.hash(&mut hasher);

        let zoom = self.core.anim_mgr.overview_zoom.value();
        let vox = self.core.anim_mgr.view_offset_x.value();
        let voy = self.core.anim_mgr.view_offset_y.value();
        zoom.to_bits().hash(&mut hasher);
        vox.to_bits().hash(&mut hasher);
        voy.to_bits().hash(&mut hasher);

        self.session_display_name().hash(&mut hasher);
        self.workspace_indicator_label().hash(&mut hasher);
        let (mode_label, mode_color) = self.current_mode_label();
        mode_label.hash(&mut hasher);
        for c in mode_color {
            c.to_bits().hash(&mut hasher);
        }

        let ws = self.core.workspaces.active();
        let pane_count = ws.columns.iter().map(|c| c.tiles.len()).sum::<usize>();
        pane_count.hash(&mut hasher);
        let active_pane_id = ws.active_pane_id();
        active_pane_id.hash(&mut hasher);
        if let Some(pid) = active_pane_id {
            self.core
                .pane_grids
                .get(&pid)
                .map(|grid| grid.title.trim())
                .unwrap_or_default()
                .hash(&mut hasher);
        }

        for (pane_id, title) in self.pane_tab_entries() {
            pane_id.hash(&mut hasher);
            title.hash(&mut hasher);
        }

        if self.core.overview.active {
            for (pane_id, rect, is_active) in
                self.core.workspaces.all_tiles_2d(vox as f32, voy as f32)
            {
                pane_id.hash(&mut hasher);
                Self::hash_geo_rect(&mut hasher, &rect);
                is_active.hash(&mut hasher);
            }
        }

        self.hash_command_palette(&mut hasher);
        self.hash_context_menu(&mut hasher);
        self.hash_pending_paste(&mut hasher);

        hasher.finish()
    }

    fn hash_images_for_pane(&self, pane_id: u64, hasher: &mut DefaultHasher) {
        if let Some(images) = self.core.image_placements.get(&pane_id) {
            images.len().hash(hasher);
            for img in images {
                img.image_id.hash(hasher);
                img.col.hash(hasher);
                img.row.hash(hasher);
                img.width_cells.hash(hasher);
                img.height_cells.hash(hasher);
                img.pixel_width.hash(hasher);
                img.pixel_height.hash(hasher);
                let mode = match img.display_mode {
                    ImageDisplayMode::Cells => 0u8,
                    ImageDisplayMode::Pixels => 1u8,
                };
                mode.hash(hasher);
                img.format.hash(hasher);
                img.data.len().hash(hasher);
            }
        } else {
            0usize.hash(hasher);
        }
    }

    fn cursor_blink_affects_scene(&self, tiles: &[(u64, GeoRect, bool)]) -> bool {
        tiles.iter().any(|(pane_id, _, is_active)| {
            *is_active
                && self
                    .cached_views
                    .get(pane_id)
                    .is_some_and(|view| !view.cursor_rects.is_empty())
        })
    }

    fn render_snapshot_hash(
        &self,
        tiles: &[(u64, GeoRect, bool)],
        vw: u32,
        vh: u32,
        zoom: f32,
    ) -> u64 {
        let mut hasher = DefaultHasher::new();

        vw.hash(&mut hasher);
        vh.hash(&mut hasher);
        zoom.to_bits().hash(&mut hasher);
        self.content_origin_y().to_bits().hash(&mut hasher);
        self.window_focused.hash(&mut hasher);
        self.core.overview.active.hash(&mut hasher);
        self.core.overview.dragging.hash(&mut hasher);
        self.core.overview.hovered_pane.hash(&mut hasher);
        self.core.overview_action_hover.hash(&mut hasher);
        self.core.broadcast_mode.hash(&mut hasher);
        self.core.input.is_awaiting_action().hash(&mut hasher);
        self.core.hovered_top_bar_region.hash(&mut hasher);
        self.core.hovered_pane_tab.hash(&mut hasher);
        self.core.pane_tab_scroll.to_bits().hash(&mut hasher);
        self.pane_tab_scroll_max().to_bits().hash(&mut hasher);
        self.core.ime.preedit_active.hash(&mut hasher);
        self.core.ime.preedit_text.hash(&mut hasher);
        self.core.ime.preedit_cursor.hash(&mut hasher);
        self.core.prediction.visual_serial().hash(&mut hasher);

        if self.cursor_blink_affects_scene(tiles) {
            self.core.cursor_blink_visible.hash(&mut hasher);
        }

        self.core.session_display_name().hash(&mut hasher);
        self.core.workspace_indicator_label().hash(&mut hasher);
        let (mode_label, mode_color) = self.core.current_mode_label();
        mode_label.hash(&mut hasher);
        for c in mode_color {
            c.to_bits().hash(&mut hasher);
        }
        for (pane_id, title) in self.core.pane_tab_entries() {
            pane_id.hash(&mut hasher);
            title.hash(&mut hasher);
        }

        self.hash_selection(&mut hasher);
        self.hash_hovered_link(&mut hasher);
        self.hash_search_state(&mut hasher);
        self.hash_command_palette(&mut hasher);
        self.hash_context_menu(&mut hasher);
        self.hash_pending_paste(&mut hasher);

        // Drag state affects border/scrollbar visuals
        self.core.drag.col_dragging.hash(&mut hasher);
        self.core.drag.tile_dragging.hash(&mut hasher);
        self.core
            .drag
            .scrollbar_dragging
            .is_some()
            .hash(&mut hasher);

        tiles.len().hash(&mut hasher);
        for (pane_id, rect, is_active) in tiles {
            pane_id.hash(&mut hasher);
            Self::hash_geo_rect(&mut hasher, rect);
            is_active.hash(&mut hasher);
            // Include animation-derived dim factor so that pane focus/open/drag
            // opacity changes are captured even when `animating` is already false.
            let focus_op = self.core.anim_mgr.pane_focus_opacity(*pane_id);
            let open_op = self.core.anim_mgr.pane_open_opacity(*pane_id);
            let drag_dim = self.core.anim_mgr.pane_drag_dim(*pane_id);
            focus_op.to_bits().hash(&mut hasher);
            open_op.to_bits().hash(&mut hasher);
            drag_dim.to_bits().hash(&mut hasher);
            let (move_dx, move_dy) = self.core.anim_mgr.pane_move_offset(*pane_id);
            move_dx.to_bits().hash(&mut hasher);
            move_dy.to_bits().hash(&mut hasher);
            if let Some(view) = self.cached_views.get(pane_id) {
                view.generation.hash(&mut hasher);
                view.scrollbar_key.hash(&mut hasher);
                // Hash full cursor rect geometry — len alone misses position changes
                for cr in &view.cursor_rects {
                    cr.x.to_bits().hash(&mut hasher);
                    cr.y.to_bits().hash(&mut hasher);
                    cr.w.to_bits().hash(&mut hasher);
                    cr.h.to_bits().hash(&mut hasher);
                }
            } else {
                0u64.hash(&mut hasher);
                Option::<(usize, usize, u16, u32, u32, u8)>::None.hash(&mut hasher);
                0usize.hash(&mut hasher);
            }
            if let Some(grid) = self.core.pane_grids.get(pane_id) {
                grid.title.hash(&mut hasher);
                grid.scroll_offset.hash(&mut hasher);
                grid.cursor_line.hash(&mut hasher);
                grid.cursor_col.hash(&mut hasher);
                grid.cursor_shape.hash(&mut hasher);
                grid.mode_flags.hash(&mut hasher);
            }
            self.hash_images_for_pane(*pane_id, &mut hasher);
        }

        hasher.finish()
    }

    fn preedit_cursor_display_cols(text: &str, cursor_byte: usize) -> usize {
        let cursor_byte = cursor_byte.min(text.len());
        text.char_indices()
            .take_while(|(idx, _)| *idx < cursor_byte)
            .map(|(_, ch)| UnicodeWidthChar::width(ch).unwrap_or(0))
            .sum()
    }

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

    fn mouse_over_scrollbar(
        mouse_content_pos: Option<(f32, f32)>,
        tile_rect: GeoRect,
        scrollbar_rect: &Rect,
        border_w: f32,
        padding: f32,
    ) -> bool {
        let Some((mx, my)) = mouse_content_pos else {
            return false;
        };
        if !tile_rect.contains(mx, my) {
            return false;
        }

        let inner_x = tile_rect.x + border_w + padding;
        let inner_y = tile_rect.y + border_w + padding;
        let sx = inner_x + scrollbar_rect.x;
        let sy = inner_y + scrollbar_rect.y;
        mx >= sx && mx <= sx + scrollbar_rect.w && my >= sy && my <= sy + scrollbar_rect.h
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
                    let alpha = paint.active_border[3] * (1.0 - layer as f32 / layers as f32) * 0.3;
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
        tile_key: (u32, u32, u32, u32),
        cache_tile_glyphs: bool,
        dirty_glyph_ranges: &mut Vec<(usize, usize)>,
        dirty_color_ranges: &mut Vec<(usize, usize)>,
        glyphs: &mut Vec<GlyphInstance>,
        color_glyphs: &mut Vec<GlyphInstance>,
    ) {
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

        if !cache_tile_glyphs {
            for row in 0..view.row_count() {
                let glyph_start = glyphs.len();
                glyphs.extend(view.row_glyphs(row).iter().filter_map(|g| {
                    let color = [
                        g.color[0] * dim,
                        g.color[1] * dim,
                        g.color[2] * dim,
                        g.color[3],
                    ];
                    make_instance(g, color)
                }));
                let glyph_end = glyphs.len();
                if glyph_start < glyph_end {
                    dirty_glyph_ranges.push((glyph_start, glyph_end));
                }
                let color_start = color_glyphs.len();
                let emoji_color = [dim, dim, dim, 1.0];
                color_glyphs.extend(
                    view.row_color_glyphs(row)
                        .iter()
                        .filter_map(|g| make_instance(g, emoji_color)),
                );
                let color_end = color_glyphs.len();
                if color_start < color_end {
                    dirty_color_ranges.push((color_start, color_end));
                }
            }
            return;
        }

        let cached = tile_cache
            .entry(pane_id)
            .or_insert_with(|| super::CachedTileGlyphs {
                key: tile_key,
                rows: Vec::new(),
            });

        if cached.key != tile_key || cached.rows.len() != view.row_count() {
            cached.key = tile_key;
            cached.rows = vec![super::CachedTileRow::default(); view.row_count()];
            for row in &mut cached.rows {
                row.epoch = 0;
                row.glyphs.clear();
                row.color_glyphs.clear();
            }
        } else if view.last_scroll_shift != 0 {
            let shift = view.last_scroll_shift;
            let amount = shift.unsigned_abs() as usize;
            if amount > 0 && amount < cached.rows.len() {
                if shift > 0 {
                    cached.rows.rotate_right(amount);
                } else {
                    cached.rows.rotate_left(amount);
                }
                let delta_y = shift as f32 * view.cell_height * zoom;
                for row in &mut cached.rows {
                    for glyph in &mut row.glyphs {
                        glyph.pos[1] += delta_y;
                    }
                    for glyph in &mut row.color_glyphs {
                        glyph.pos[1] += delta_y;
                    }
                }
            }
        }

        for row in 0..view.row_count() {
            let rebuilt = cached.rows[row].epoch != view.row_epoch(row);
            if rebuilt {
                cached.rows[row].epoch = view.row_epoch(row);
                cached.rows[row].glyphs.clear();
                cached.rows[row].color_glyphs.clear();
                cached.rows[row]
                    .glyphs
                    .extend(view.row_glyphs(row).iter().filter_map(|g| {
                        let color = [
                            g.color[0] * dim,
                            g.color[1] * dim,
                            g.color[2] * dim,
                            g.color[3],
                        ];
                        make_instance(g, color)
                    }));
                let emoji_color = [dim, dim, dim, 1.0];
                cached.rows[row].color_glyphs.extend(
                    view.row_color_glyphs(row)
                        .iter()
                        .filter_map(|g| make_instance(g, emoji_color)),
                );
            }

            let glyph_start = glyphs.len();
            glyphs.extend_from_slice(&cached.rows[row].glyphs);
            let glyph_end = glyphs.len();
            if rebuilt && glyph_start < glyph_end {
                dirty_glyph_ranges.push((glyph_start, glyph_end));
            }
            let color_start = color_glyphs.len();
            color_glyphs.extend_from_slice(&cached.rows[row].color_glyphs);
            let color_end = color_glyphs.len();
            if rebuilt && color_start < color_end {
                dirty_color_ranges.push((color_start, color_end));
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
        let tile_key = (
            inner_x.to_bits(),
            inner_y.to_bits(),
            zoom.to_bits(),
            visual.dim.to_bits(),
        );
        Self::emit_tile_background_rows(
            &mut self.cached_tile_backgrounds,
            pane_id,
            &view,
            inner_x,
            inner_y,
            zoom,
            &tr,
            tile_key,
            paint.cache_tile_glyphs,
            &mut self.render_bufs.dirty_bg_ranges,
            bg_rects,
        );

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
            tile_key,
            paint.cache_tile_glyphs,
            &mut self.render_bufs.dirty_glyph_ranges,
            &mut self.render_bufs.dirty_color_ranges,
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
            // Per-row tile glyph/background caching disabled — rebuild from
            // atlas each frame (matches Ghostty / Windows Terminal approach).
            // render_snapshot_hash still skips entire idle frames.
            cache_tile_glyphs: false,
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

    fn relayout_retained_pane_glyphs(
        &mut self,
        ordered_tiles: &[(u64, GeoRect, bool)],
        zoom: f32,
        vw: f32,
        vh: f32,
        paint: TilePaintConfig,
    ) {
        self.render_bufs.pane_order.clear();
        self.render_bufs.pane_regions.clear();
        self.render_bufs.glyphs.clear();
        self.render_bufs.color_glyphs.clear();

        let mut glyph_offset = 0usize;
        let mut color_offset = 0usize;

        for (pane_id, tile_rect, is_active) in ordered_tiles.iter().copied() {
            let Some(fragment) =
                self.build_pane_glyph_fragment(pane_id, tile_rect, is_active, zoom, vw, vh, paint)
            else {
                continue;
            };

            let glyph_cap = Self::glyph_scene_capacity(fragment.glyphs.len());
            let color_cap = Self::glyph_scene_capacity(fragment.color_glyphs.len());

            let glyph_end = glyph_offset + glyph_cap;
            let color_end = color_offset + color_cap;
            let zero = Self::zero_glyph_instance();

            if self.render_bufs.glyphs.len() < glyph_end {
                self.render_bufs.glyphs.resize(glyph_end, zero);
            }
            if self.render_bufs.color_glyphs.len() < color_end {
                self.render_bufs.color_glyphs.resize(color_end, zero);
            }

            if !fragment.glyphs.is_empty() {
                self.render_bufs.glyphs[glyph_offset..glyph_offset + fragment.glyphs.len()]
                    .copy_from_slice(&fragment.glyphs);
            }
            for slot in
                &mut self.render_bufs.glyphs[glyph_offset + fragment.glyphs.len()..glyph_end]
            {
                *slot = zero;
            }

            if !fragment.color_glyphs.is_empty() {
                self.render_bufs.color_glyphs
                    [color_offset..color_offset + fragment.color_glyphs.len()]
                    .copy_from_slice(&fragment.color_glyphs);
            }
            for slot in &mut self.render_bufs.color_glyphs
                [color_offset + fragment.color_glyphs.len()..color_end]
            {
                *slot = zero;
            }

            self.render_bufs.pane_order.push(pane_id);
            self.render_bufs.pane_regions.insert(
                pane_id,
                super::PaneSceneRegion {
                    glyph_offset,
                    glyph_len: fragment.glyphs.len(),
                    glyph_cap,
                    color_offset,
                    color_len: fragment.color_glyphs.len(),
                    color_cap,
                    scissor: fragment.scissor,
                    is_active: fragment.is_active,
                    snapshot: fragment.snapshot,
                    ..Default::default()
                },
            );

            glyph_offset = glyph_end;
            color_offset = color_end;
        }

        self.render_bufs.pane_glyph_end = glyph_offset;
        self.render_bufs.pane_color_glyph_end = color_offset;
        self.render_bufs.glyphs.truncate(glyph_offset);
        self.render_bufs.color_glyphs.truncate(color_offset);
    }

    fn sync_retained_pane_glyphs(
        &mut self,
        ordered_tiles: &[(u64, GeoRect, bool)],
        zoom: f32,
        vw: f32,
        vh: f32,
        paint: TilePaintConfig,
    ) {
        let current_order: Vec<u64> = ordered_tiles
            .iter()
            .filter_map(|(pane_id, tile_rect, is_active)| {
                self.pane_glyph_snapshot_hash(*pane_id, *tile_rect, *is_active, zoom, vw, vh)
                    .map(|_| *pane_id)
            })
            .collect();
        if self.render_bufs.pane_order != current_order {
            self.relayout_retained_pane_glyphs(ordered_tiles, zoom, vw, vh, paint);
            return;
        }

        let mut need_relayout = false;
        let zero = Self::zero_glyph_instance();

        for (pane_id, tile_rect, is_active) in ordered_tiles.iter().copied() {
            let Some(snapshot) =
                self.pane_glyph_snapshot_hash(pane_id, tile_rect, is_active, zoom, vw, vh)
            else {
                continue;
            };
            let Some(region) = self.render_bufs.pane_regions.get(&pane_id).copied() else {
                need_relayout = true;
                break;
            };
            if region.snapshot == snapshot {
                continue;
            }

            let Some(fragment) =
                self.build_pane_glyph_fragment(pane_id, tile_rect, is_active, zoom, vw, vh, paint)
            else {
                need_relayout = true;
                break;
            };

            if fragment.glyphs.len() > region.glyph_cap
                || fragment.color_glyphs.len() > region.color_cap
            {
                need_relayout = true;
                break;
            }

            if !fragment.glyphs.is_empty() {
                self.render_bufs.glyphs
                    [region.glyph_offset..region.glyph_offset + fragment.glyphs.len()]
                    .copy_from_slice(&fragment.glyphs);
            }
            for slot in &mut self.render_bufs.glyphs[region.glyph_offset + fragment.glyphs.len()
                ..region.glyph_offset + region.glyph_cap]
            {
                *slot = zero;
            }

            if !fragment.color_glyphs.is_empty() {
                self.render_bufs.color_glyphs
                    [region.color_offset..region.color_offset + fragment.color_glyphs.len()]
                    .copy_from_slice(&fragment.color_glyphs);
            }
            for slot in &mut self.render_bufs.color_glyphs[region.color_offset
                + fragment.color_glyphs.len()
                ..region.color_offset + region.color_cap]
            {
                *slot = zero;
            }

            if let Some(region_mut) = self.render_bufs.pane_regions.get_mut(&pane_id) {
                region_mut.glyph_len = fragment.glyphs.len();
                region_mut.color_len = fragment.color_glyphs.len();
                region_mut.scissor = fragment.scissor;
                region_mut.is_active = fragment.is_active;
                region_mut.snapshot = fragment.snapshot;
            }
        }

        if need_relayout {
            self.relayout_retained_pane_glyphs(ordered_tiles, zoom, vw, vh, paint);
        }
    }

    fn rebuild_retained_glyph_batches(&mut self) {
        self.render_bufs.glyph_batches.clear();
        self.render_bufs.color_glyph_batches.clear();
        self.render_bufs.active_glyph_batches.clear();
        self.render_bufs.active_color_glyph_batches.clear();

        for pane_id in self.render_bufs.pane_order.iter().copied() {
            let Some(region) = self.render_bufs.pane_regions.get(&pane_id).copied() else {
                continue;
            };
            let (x, y, w, h) = region.scissor;
            if region.glyph_len > 0 {
                let batch = ScissoredRange {
                    x,
                    y,
                    w,
                    h,
                    start: region.glyph_offset,
                    end: region.glyph_offset + region.glyph_len,
                };
                if region.is_active {
                    self.render_bufs.active_glyph_batches.push(batch);
                } else {
                    self.render_bufs.glyph_batches.push(batch);
                }
            }
            if region.color_len > 0 {
                let batch = ScissoredRange {
                    x,
                    y,
                    w,
                    h,
                    start: region.color_offset,
                    end: region.color_offset + region.color_len,
                };
                if region.is_active {
                    self.render_bufs.active_color_glyph_batches.push(batch);
                } else {
                    self.render_bufs.color_glyph_batches.push(batch);
                }
            }
        }
    }

    fn should_use_retained_pane_scene(
        &self,
        ordered_tiles: &[(u64, GeoRect, bool)],
        zoom: f32,
        vw: f32,
        vh: f32,
    ) -> bool {
        let _ = (ordered_tiles, zoom, vw, vh);
        false
    }

    pub fn build_search_bar(
        &mut self,
        tiles: &[(u64, GeoRect, bool)],
        _vw: f32,
        _vh: f32,
        bg_rects: &mut Vec<Rect>,
        glyphs: &mut Vec<GlyphInstance>,
        color_glyphs: &mut Vec<GlyphInstance>,
    ) {
        let Some(search) = &self.core.search_state else {
            return;
        };
        let atlas = self.glyph_cache.as_mut().unwrap();
        let mut ui_borrow = self.ui_shaper.as_ref().map(|c| c.borrow_mut());

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
            ui_borrow.as_deref_mut(),
            &bar_text,
            &TextEmitParams {
                x_start: bar_x + padding,
                y: text_y,
                cell_width: cw,
                baseline,
                color: text_color,
            },
            glyphs,
            color_glyphs,
        );
    }

    fn ime_input_anchor(
        &self,
        tiles: &[(u64, GeoRect, bool)],
        cell_w: f32,
        cell_h: f32,
    ) -> Option<(f32, f32)> {
        if let Some(palette) = &self.core.command_palette
            && let Some(layout) = self.command_palette_layout()
        {
            let prefix = if palette.remote_input_mode {
                "SSH> "
            } else {
                "> "
            };
            let cols =
                UnicodeWidthStr::width(prefix) + UnicodeWidthStr::width(palette.query.as_str());
            return Some((layout.text_x + cols as f32 * cell_w, layout.text_y));
        }

        if let Some(search) = &self.core.search_state
            && let Some((_, pane_rect, _)) = tiles.iter().find(|(pid, _, _)| *pid == search.pane_id)
        {
            let border_w = self.core.config.appearance.border_width;
            let padding = self.core.config.appearance.padding;
            let bar_height = cell_h + 4.0;
            let bar_y = pane_rect.y + pane_rect.h - border_w - bar_height;
            let bar_x = pane_rect.x + border_w;
            let prefix = " Search: ";
            let cols =
                UnicodeWidthStr::width(prefix) + UnicodeWidthStr::width(search.query.as_str());
            return Some((bar_x + padding + cols as f32 * cell_w, bar_y + 2.0));
        }

        let active_pid = self.core.workspaces.active().active_pane_id()?;
        let (_, tile_rect, _) = tiles.iter().find(|(id, _, _)| *id == active_pid)?;
        let view = self.cached_views.get(&active_pid)?;
        let cursor = view.cursor_rects.first()?;
        let padding = self.core.config.appearance.padding;
        let border_w = self.core.config.appearance.border_width;
        Some((
            tile_rect.x + border_w + padding + cursor.x,
            tile_rect.y + border_w + padding + cursor.y,
        ))
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
        color_glyphs: &mut Vec<GlyphInstance>,
    ) {
        if !self.core.ime.preedit_active || self.core.ime.preedit_text.is_empty() {
            return;
        }
        let (cw, ch) = {
            let atlas = self.glyph_cache.as_ref().unwrap();
            (atlas.cell_width, atlas.cell_height)
        };
        let Some((base_x, base_y)) = self.ime_input_anchor(tiles, cw, ch) else {
            return;
        };
        let atlas = self.glyph_cache.as_mut().unwrap();

        let text = &self.core.ime.preedit_text;
        let text_width = UnicodeWidthStr::width(text.as_str()) as f32 * cw;

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
        let mut ui_borrow = self.ui_shaper.as_ref().map(|c| c.borrow_mut());
        emit_status_text(
            atlas,
            ui_borrow.as_deref_mut(),
            text,
            &TextEmitParams {
                x_start: base_x + 2.0,
                y: base_y + 1.0,
                cell_width: cw,
                baseline,
                color: text_color,
            },
            glyphs,
            color_glyphs,
        );

        // Cursor within preedit text
        if let Some(cursor_pos) = self.core.ime.preedit_cursor {
            let cursor_cols = Self::preedit_cursor_display_cols(text, cursor_pos);
            let cx = base_x + 2.0 + cursor_cols as f32 * cw;
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
        let raw_dt = (now - self.core.last_frame).as_secs_f64();
        self.core.last_frame = now;
        // When the app wakes after a long idle stretch, a large frame delta can
        // collapse newly started focus/move animations into a single frame.
        let max_dt = (self.core.frame_interval.as_secs_f64() * 2.0).max(1.0 / 60.0);
        let dt = raw_dt.min(max_dt);

        // Focus change detection — must happen BEFORE advance so the new
        // animation is ticked in the same frame it starts.
        let current_focus = self.core.workspaces.active().active_pane_id();
        let config = self.anim_config();
        self.core.anim_mgr.on_focus_changed(current_focus, &config);

        let mut animating = self.advance_animations(dt);

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
        let mouse_content_pos = self.last_mouse_pos.and_then(|(mx, my)| {
            let my = self.content_y_from_screen(my)?;
            let mx = self.content_x_from_screen(mx, vw_f)?;
            Some((mx, my))
        });

        let (cache_cell_width, cache_cell_height) = {
            let cache = self.glyph_cache.as_ref().unwrap();
            (cache.cell_width, cache.cell_height)
        };
        let cache = self.glyph_cache.as_mut().unwrap();
        let shaper = self.text_shaper.as_ref().unwrap();

        // Update terminal views for dirty pane grids
        for (pane_id, tile_rect, _) in &tiles {
            // Snapshot dirty state before taking mutable borrows
            let (needs_full, has_dirty_rows, dirty_rows_copy, pending_scroll_delta) =
                if let Some(g) = self.core.pane_grids.get(pane_id) {
                    (
                        g.dirty,
                        g.dirty_rows.iter().any(|dirty| *dirty),
                        g.dirty_rows.clone(),
                        g.pending_scroll_delta,
                    )
                } else {
                    (false, false, Vec::new(), 0)
                };
            let needs_initial = !self.cached_views.contains_key(pane_id);
            let scroll_only = pending_scroll_delta != 0 && !has_dirty_rows && !needs_initial;

            if needs_initial || (needs_full && !scroll_only) {
                let Some(grid) = self.core.pane_grids.get_mut(pane_id) else {
                    continue;
                };
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
            } else if has_dirty_rows || pending_scroll_delta != 0 || scroll_only {
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
                        terminal::update_view_from_grid(
                            view,
                            &dirty_rows_copy,
                            pending_scroll_delta,
                            &inputs,
                            cache,
                        );
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
                } else if let Some(sb) = &view.scrollbar_rect {
                    if Self::mouse_over_scrollbar(
                        mouse_content_pos,
                        *tile_rect,
                        sb,
                        border_w,
                        padding,
                    ) {
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
            && let Some((x, y)) = self.ime_input_anchor(&tiles, cache_cell_width, cache_cell_height)
        {
            let cx = x as i32;
            let cy = y as i32;
            let pos = (cx, cy);
            if self.core.ime.last_pos != Some(pos) {
                self.core.ime.last_pos = Some(pos);
                window.set_ime_cursor_area(
                    winit::dpi::PhysicalPosition::new(cx as f64, cy as f64),
                    winit::dpi::PhysicalSize::new(
                        cache_cell_width as f64,
                        cache_cell_height as f64,
                    ),
                );
            }
        }

        let content_y = self.content_origin_y();
        let content_x = self.content_origin_x();
        let offset_tiles: Vec<(u64, GeoRect, bool)> = tiles
            .iter()
            .map(|(pane_id, rect, is_active)| {
                (
                    *pane_id,
                    GeoRect::new(rect.x + content_x, rect.y + content_y, rect.w, rect.h),
                    *is_active,
                )
            })
            .collect();

        let render_snapshot = self.render_snapshot_hash(&offset_tiles, vw, vh, zoom);
        if !animating && self.last_render_snapshot == Some(render_snapshot) {
            return;
        }

        self.render_bufs.dirty_bg_ranges.clear();
        self.render_bufs.dirty_glyph_ranges.clear();
        self.render_bufs.dirty_color_ranges.clear();

        let paint = self.tile_paint_config();
        let ordered_tiles = Self::ordered_pane_tiles(&offset_tiles);
        let use_retained_panes =
            self.should_use_retained_pane_scene(&ordered_tiles, zoom, vw_f, vh_f);

        let mut bg_rects = std::mem::take(&mut self.render_bufs.bg_rects);
        let glyphs = std::mem::take(&mut self.render_bufs.glyphs);
        let color_glyphs = std::mem::take(&mut self.render_bufs.color_glyphs);
        let glyph_batches = std::mem::take(&mut self.render_bufs.glyph_batches);
        let color_glyph_batches = std::mem::take(&mut self.render_bufs.color_glyph_batches);
        let active_glyph_batches = std::mem::take(&mut self.render_bufs.active_glyph_batches);
        let active_color_glyph_batches =
            std::mem::take(&mut self.render_bufs.active_color_glyph_batches);
        bg_rects.clear();
        let (active_bg_start, pane_glyph_end, pane_color_glyph_end, mut glyphs, mut color_glyphs) =
            if use_retained_panes {
                self.render_bufs.glyphs = glyphs;
                self.render_bufs.color_glyphs = color_glyphs;
                self.render_bufs.glyph_batches = glyph_batches;
                self.render_bufs.color_glyph_batches = color_glyph_batches;
                self.render_bufs.active_glyph_batches = active_glyph_batches;
                self.render_bufs.active_color_glyph_batches = active_color_glyph_batches;

                self.sync_retained_pane_glyphs(&ordered_tiles, zoom, vw_f, vh_f, paint);
                self.rebuild_retained_glyph_batches();

                let mut glyphs = std::mem::take(&mut self.render_bufs.glyphs);
                let mut color_glyphs = std::mem::take(&mut self.render_bufs.color_glyphs);
                glyphs.truncate(self.render_bufs.pane_glyph_end);
                color_glyphs.truncate(self.render_bufs.pane_color_glyph_end);

                for (pane_id, tile_rect, is_active) in ordered_tiles.iter().copied() {
                    if is_active {
                        continue;
                    }
                    self.build_tile_backgrounds(
                        pane_id,
                        tile_rect,
                        false,
                        zoom,
                        vw_f,
                        vh_f,
                        paint,
                        &mut bg_rects,
                    );
                }

                let zoom_threshold = self.core.config.animation.zoom_threshold;
                for (rect, opacity, _slide) in self.core.anim_mgr.closing_panes() {
                    let (rx, ry, rw, rh) = if zoom < zoom_threshold {
                        let cx = vw_f / 2.0;
                        let cy = vh_f / 2.0;
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
                for (pane_id, tile_rect, is_active) in ordered_tiles.iter().copied() {
                    if !is_active {
                        continue;
                    }
                    self.build_tile_backgrounds(
                        pane_id,
                        tile_rect,
                        true,
                        zoom,
                        vw_f,
                        vh_f,
                        paint,
                        &mut bg_rects,
                    );
                }

                (
                    active_bg_start,
                    self.render_bufs.pane_glyph_end,
                    self.render_bufs.pane_color_glyph_end,
                    glyphs,
                    color_glyphs,
                )
            } else {
                let mut glyphs = glyphs;
                let mut color_glyphs = color_glyphs;
                let mut glyph_batches = glyph_batches;
                let mut color_glyph_batches = color_glyph_batches;
                let mut active_glyph_batches = active_glyph_batches;
                let mut active_color_glyph_batches = active_color_glyph_batches;
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
                self.render_bufs.pane_order.clear();
                self.render_bufs.pane_regions.clear();
                self.render_bufs.pane_glyph_end = pane_glyph_end;
                self.render_bufs.pane_color_glyph_end = pane_color_glyph_end;
                self.render_bufs.glyph_batches = glyph_batches;
                self.render_bufs.color_glyph_batches = color_glyph_batches;
                self.render_bufs.active_glyph_batches = active_glyph_batches;
                self.render_bufs.active_color_glyph_batches = active_color_glyph_batches;

                (
                    active_bg_start,
                    pane_glyph_end,
                    pane_color_glyph_end,
                    glyphs,
                    color_glyphs,
                )
            };
        let glyph_batches = std::mem::take(&mut self.render_bufs.glyph_batches);
        let color_glyph_batches = std::mem::take(&mut self.render_bufs.color_glyph_batches);
        let active_glyph_batches = std::mem::take(&mut self.render_bufs.active_glyph_batches);
        let active_color_glyph_batches =
            std::mem::take(&mut self.render_bufs.active_color_glyph_batches);
        let overlay_bg_start = bg_rects.len();
        self.build_ui(vw_f, vh_f, &mut bg_rects, &mut glyphs, &mut color_glyphs);
        self.build_search_bar(
            &offset_tiles,
            vw_f,
            vh_f,
            &mut bg_rects,
            &mut glyphs,
            &mut color_glyphs,
        );
        self.build_bell_flash(&offset_tiles, zoom, vw_f, vh_f, &mut bg_rects);
        self.build_ime_preedit(
            &offset_tiles,
            vw_f,
            vh_f,
            &mut bg_rects,
            &mut glyphs,
            &mut color_glyphs,
        );

        let clear_color = if self.core.overview.active || zoom < zoom_threshold {
            ThemeConfig::parse_color(&self.core.config.theme.overview_background)
        } else {
            ThemeConfig::parse_color(&self.core.config.theme.ui_background)
        };
        let renderer = self.renderer.as_mut().unwrap();
        let cache = self.glyph_cache.as_mut().unwrap();
        let atlas_gpu = self.glyph_atlas_gpu.as_mut().unwrap();
        let draw_ok = if let Err(e) = renderer.draw_frame(
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
        ) {
            log::error!("draw_frame failed: {e}");
            // Don't schedule another redraw — a failed frame will fail again,
            // causing an infinite error loop at frame rate.
            animating = false;
            false
        } else {
            true
        };

        if draw_ok {
            self.last_render_snapshot = Some(render_snapshot);
        }

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

        if animating {
            self.schedule_redraw();
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
    use ciri_config::config::{CiriConfig, StatusBarPosition, TabBarPosition};
    use std::sync::Arc;
    use winit::dpi::PhysicalSize;

    fn make_app() -> App {
        App::new(CiriConfig::default(), "test-session")
    }

    fn make_content_app(tab_position: TabBarPosition) -> App {
        let mut config = CiriConfig::default();
        config.window.width = 900.0;
        config.window.height = 700.0;
        config.statusbar.position = StatusBarPosition::Top;
        config.tabbar.position = tab_position;
        config.tabbar.width = 96.0;

        let mut app = App::new(config, "test-session");
        app.preview_resize(PhysicalSize::new(900, 700));
        app
    }

    #[test]
    fn preedit_cursor_display_cols_handles_utf8_offsets_and_wide_chars() {
        let text = "你a好";
        assert_eq!(App::preedit_cursor_display_cols(text, 0), 0);
        assert_eq!(App::preedit_cursor_display_cols(text, "你".len()), 2);
        assert_eq!(App::preedit_cursor_display_cols(text, "你a".len()), 3);
        assert_eq!(App::preedit_cursor_display_cols(text, text.len()), 5);
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
    fn mouse_over_scrollbar_converts_left_tab_bar_and_top_bar_coords() {
        let mut app = make_content_app(TabBarPosition::Left);
        let tile_rect = GeoRect::new(0.0, 0.0, 300.0, 200.0);
        let scrollbar = Rect {
            x: 280.0,
            y: 20.0,
            w: 8.0,
            h: 32.0,
            color: [1.0, 1.0, 1.0, 1.0],
        };
        let screen_x = app.content_origin_x()
            + tile_rect.x
            + app.core.config.appearance.border_width
            + app.core.config.appearance.padding
            + scrollbar.x
            + 2.0;
        let screen_y = app.content_origin_y()
            + tile_rect.y
            + app.core.config.appearance.border_width
            + app.core.config.appearance.padding
            + scrollbar.y
            + 2.0;
        app.last_mouse_pos = Some((screen_x, screen_y));
        let mouse_content_pos = Some((
            app.content_x_from_screen(screen_x, 900.0).unwrap(),
            app.content_y_from_screen(screen_y).unwrap(),
        ));

        assert!(App::mouse_over_scrollbar(
            mouse_content_pos,
            tile_rect,
            &scrollbar,
            app.core.config.appearance.border_width,
            app.core.config.appearance.padding,
        ));
    }

    #[test]
    fn mouse_over_scrollbar_rejects_right_tab_bar_strip() {
        let mut app = make_content_app(TabBarPosition::Right);
        let tile_rect = GeoRect::new(0.0, 0.0, 300.0, 200.0);
        let scrollbar = Rect {
            x: 280.0,
            y: 20.0,
            w: 8.0,
            h: 32.0,
            color: [1.0, 1.0, 1.0, 1.0],
        };
        app.last_mouse_pos = Some((
            900.0 - app.core.config.tabbar.width / 2.0,
            app.content_origin_y() + 24.0,
        ));
        let mouse_content_pos = app.last_mouse_pos.and_then(|(mx, my)| {
            let my = app.content_y_from_screen(my)?;
            let mx = app.content_x_from_screen(mx, 900.0)?;
            Some((mx, my))
        });

        assert!(!App::mouse_over_scrollbar(
            mouse_content_pos,
            tile_rect,
            &scrollbar,
            app.core.config.appearance.border_width,
            app.core.config.appearance.padding,
        ));
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
