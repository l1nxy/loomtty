use ciri_layout::geometry::Rect as GeoRect;
use ciri_render::glyph_cache::GlyphInstance;
use ciri_render::sdf_rect::SdfRect;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::bell_flash::{BellFlashComponent, BellFlashRect};
use super::ime_preedit::ImePreeditComponent;
use super::search_bar::SearchBarComponent;
use super::types::{UiContext, UiScene, ui_context_from_metrics};
use crate::app::App;

pub(crate) struct TransientOverlayFrame {
    search_bar: Option<SearchBarComponent>,
    bell_flash: Option<BellFlashComponent>,
    ime_preedit: Option<ImePreeditComponent>,
}

impl TransientOverlayFrame {
    pub(crate) fn new(
        search_bar: Option<SearchBarComponent>,
        bell_flash: Option<BellFlashComponent>,
        ime_preedit: Option<ImePreeditComponent>,
    ) -> Self {
        Self {
            search_bar,
            bell_flash,
            ime_preedit,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.search_bar.is_none() && self.bell_flash.is_none() && self.ime_preedit.is_none()
    }

    pub(crate) fn paint(&mut self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        if let Some(component) = &mut self.search_bar {
            component.paint(cx, scene);
        }
        if let Some(component) = &mut self.bell_flash {
            component.paint(cx, scene);
        }
        if let Some(component) = &mut self.ime_preedit {
            component.paint(cx, scene);
        }
    }
}

impl App {
    #[cfg(test)]
    pub(in crate::app) fn build_search_bar(
        &mut self,
        tiles: &[(u64, GeoRect, bool)],
        vw: f32,
        vh: f32,
        sdf_rects: &mut Vec<SdfRect>,
        glyphs: &mut Vec<GlyphInstance>,
        color_glyphs: &mut Vec<GlyphInstance>,
    ) {
        let (cw, ch) = self.ui_cell_metrics();
        let Some(mut component) = self.search_bar_component(tiles) else {
            return;
        };
        self.paint_transient_ui_with_metrics(
            vw,
            vh,
            cw,
            ch,
            sdf_rects,
            glyphs,
            color_glyphs,
            |cx, scene| {
                component.paint(cx, scene);
            },
        );
    }

    fn search_bar_component(&self, tiles: &[(u64, GeoRect, bool)]) -> Option<SearchBarComponent> {
        let Some(search) = &self.core.search_state else {
            return None;
        };

        let Some((_, pane_rect, _)) = tiles.iter().find(|(pid, _, _)| *pid == search.pane_id)
        else {
            return None;
        };

        let (_, ch) = self.ui_cell_metrics();
        Some(SearchBarComponent {
            query: search.query.clone(),
            matches_len: search.matches.len(),
            current_match_idx: search.current_match_idx,
            pane_rect: *pane_rect,
            border_w: self.core.config.appearance.border_width,
            padding: self.core.config.appearance.padding,
            cell_h: ch,
        })
    }

    #[cfg(test)]
    pub(in crate::app) fn build_bell_flash(
        &mut self,
        tiles: &[(u64, GeoRect, bool)],
        zoom: f32,
        vw: f32,
        vh: f32,
        sdf_rects: &mut Vec<SdfRect>,
        glyphs: &mut Vec<GlyphInstance>,
        color_glyphs: &mut Vec<GlyphInstance>,
    ) {
        let (cw, ch) = self.ui_cell_metrics();
        let Some(mut component) = self.bell_flash_component(tiles, zoom, vw, vh) else {
            return;
        };
        self.paint_transient_ui_with_metrics(
            vw,
            vh,
            cw,
            ch,
            sdf_rects,
            glyphs,
            color_glyphs,
            |cx, scene| {
                component.paint(cx, scene);
            },
        );
    }

    fn bell_flash_component(
        &self,
        tiles: &[(u64, GeoRect, bool)],
        zoom: f32,
        vw: f32,
        vh: f32,
    ) -> Option<BellFlashComponent> {
        let flashes = self
            .bell_flash_rects(tiles, zoom, vw, vh)
            .into_iter()
            .map(|(rect, intensity)| BellFlashRect { rect, intensity })
            .collect::<Vec<_>>();
        (!flashes.is_empty()).then_some(BellFlashComponent { flashes })
    }

    pub(in crate::app) fn ime_input_anchor(
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

    #[cfg(test)]
    pub(in crate::app) fn build_ime_preedit(
        &mut self,
        tiles: &[(u64, GeoRect, bool)],
        vw: f32,
        vh: f32,
        sdf_rects: &mut Vec<SdfRect>,
        glyphs: &mut Vec<GlyphInstance>,
        color_glyphs: &mut Vec<GlyphInstance>,
    ) {
        let (cw, ch) = self.ui_cell_metrics();
        let Some(mut component) = self.ime_preedit_component(tiles, cw, ch) else {
            return;
        };
        self.paint_transient_ui_with_metrics(
            vw,
            vh,
            cw,
            ch,
            sdf_rects,
            glyphs,
            color_glyphs,
            |cx, scene| {
                component.paint(cx, scene);
            },
        );
    }

    fn ime_preedit_component(
        &self,
        tiles: &[(u64, GeoRect, bool)],
        cw: f32,
        ch: f32,
    ) -> Option<ImePreeditComponent> {
        if !self.core.ime.preedit_active || self.core.ime.preedit_text.is_empty() {
            return None;
        }
        let Some((base_x, base_y)) = self.ime_input_anchor(tiles, cw, ch) else {
            return None;
        };
        let preedit_text = self.core.ime.preedit_text.clone();
        let cursor_cols = self
            .core
            .ime
            .preedit_cursor
            .map(|cursor_pos| preedit_cursor_display_cols(&preedit_text, cursor_pos));

        Some(ImePreeditComponent {
            text: preedit_text,
            base_x,
            base_y,
            cursor_cols,
            cell_w: cw,
            cell_h: ch,
        })
    }

    pub(in crate::app) fn build_transient_ui(
        &mut self,
        tiles: &[(u64, GeoRect, bool)],
        zoom: f32,
        vw: f32,
        vh: f32,
        sdf_rects: &mut Vec<SdfRect>,
        glyphs: &mut Vec<GlyphInstance>,
        color_glyphs: &mut Vec<GlyphInstance>,
    ) {
        let (cw, ch) = self.ui_cell_metrics();
        let mut frame = TransientOverlayFrame::new(
            self.search_bar_component(tiles),
            self.bell_flash_component(tiles, zoom, vw, vh),
            self.ime_preedit_component(tiles, cw, ch),
        );
        if frame.is_empty() {
            return;
        }

        self.paint_transient_ui_with_metrics(
            vw,
            vh,
            cw,
            ch,
            sdf_rects,
            glyphs,
            color_glyphs,
            |cx, scene| {
                frame.paint(cx, scene);
            },
        );
    }

    pub(in crate::app) fn paint_transient_ui_with_metrics(
        &mut self,
        vw: f32,
        vh: f32,
        cell_w: f32,
        cell_h: f32,
        sdf_rects: &mut Vec<SdfRect>,
        glyphs: &mut Vec<GlyphInstance>,
        color_glyphs: &mut Vec<GlyphInstance>,
        paint: impl FnOnce(&UiContext<'_>, &mut UiScene<'_>),
    ) {
        let baseline = cell_h * self.core.config.statusbar.text_baseline;
        // Clear the element arena and enter the scope so any
        // `Div::child` inside the closure (palette / context-menu /
        // paste-dialog tree builders) bump-allocates into this arena
        // rather than the thread-local fallback.
        self.ui_arena.borrow_mut().clear();
        let _arena_scope = ciri_ui::ElementArenaScope::enter(&self.ui_arena);
        let atlas = self.glyph_cache.as_mut().unwrap();
        let mut scene = UiScene {
            atlas,
            glyphs,
            color_glyphs,
            sdf_rects,
        };
        let cx = ui_context_from_metrics(
            &self.core.config,
            &self.cached_resolved_theme,
            self.ui_shaper.as_ref(),
            Some(&self.ui_taffy_tree),
            self.last_mouse_pos.map(|(x, y)| [x, y]),
            self.active_hit_id,
            Some(&self.ui_states),
            vw,
            vh,
            cell_w,
            cell_h,
            baseline,
        );
        paint(&cx, &mut scene);
    }
}

pub(in crate::app) fn preedit_cursor_display_cols(text: &str, cursor_byte: usize) -> usize {
    let cursor_byte = cursor_byte.min(text.len());
    text.char_indices()
        .take_while(|(idx, _)| *idx < cursor_byte)
        .map(|(_, ch)| UnicodeWidthChar::width(ch).unwrap_or(0))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::preedit_cursor_display_cols;

    #[test]
    fn preedit_cursor_display_cols_handles_utf8_offsets_and_wide_chars() {
        let text = "你a好";
        assert_eq!(preedit_cursor_display_cols(text, 0), 0);
        assert_eq!(preedit_cursor_display_cols(text, "你".len()), 2);
        assert_eq!(preedit_cursor_display_cols(text, "你a".len()), 3);
        assert_eq!(preedit_cursor_display_cols(text, text.len()), 5);
    }
}
