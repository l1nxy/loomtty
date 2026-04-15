use ciri_layout::geometry::Rect as GeoRect;
use ciri_render::rect::Rect;

use super::tokens;
use super::types::{UiContext, UiOverviewHit, UiScene};
use crate::app::App;
use crate::app::status_bar::{TextEmitParams, emit_status_text};

pub(crate) struct OverviewComponent {
    hovered_pane: Option<(usize, u64)>,
}

pub(crate) struct OverviewActionBarData {
    pub pane_x: f32,
    pub pane_w: f32,
    pub bar_y: f32,
    pub bar_h: f32,
    pub close_x: f32,
    pub close_w: f32,
    pub focus_x: f32,
    pub focus_w: f32,
}

impl OverviewComponent {
    pub fn capture(app: &App, _cx: &UiContext<'_>) -> Self {
        Self {
            hovered_pane: app.core.overview.hovered_pane,
        }
    }

    pub(super) fn hit_test(&self, app: &App, mx: f32, my: f32) -> UiOverviewHit {
        if !app.core.overview.active {
            return UiOverviewHit::None;
        }
        // Check action bar on hovered pane first
        if let Some((ws_idx, hovered_id)) = self.hovered_pane
            && let Some(bar) = overview_action_bar_data(app, self.hovered_pane)
            && mx >= bar.pane_x
            && mx < bar.pane_x + bar.pane_w
            && my >= bar.bar_y
            && my < bar.bar_y + bar.bar_h
        {
            if mx >= bar.close_x && mx < bar.close_x + bar.close_w {
                return UiOverviewHit::ClosePane(hovered_id);
            }
            if mx >= bar.focus_x && mx < bar.focus_x + bar.focus_w {
                return UiOverviewHit::FocusPane(ws_idx, hovered_id);
            }
            return UiOverviewHit::Background;
        }
        if let Some((ws_idx, pane_id)) = app.hit_test_overview(mx, my) {
            UiOverviewHit::Pane(ws_idx, pane_id)
        } else {
            UiOverviewHit::Background
        }
    }
}

/// Compute overview action bar geometry (shared by hit_test and paint).
pub(crate) fn overview_action_bar_data(
    app: &App,
    hovered_pane: Option<(usize, u64)>,
) -> Option<OverviewActionBarData> {
    let (_, hovered_id) = hovered_pane?;
    let zoom = app.core.anim_mgr.overview_zoom.value() as f32;
    let vox = app.core.anim_mgr.view_offset_x.value() as f32;
    let voy = app.core.anim_mgr.view_offset_y.value() as f32;
    let tiles = app.overview_visible_tiles(zoom, vox, voy);
    let (vw, vh) = app.command_palette_viewport_size();
    let cell_w = app
        .glyph_cache
        .as_ref()
        .map(|c| c.cell_width)
        .unwrap_or(8.0);
    let cell_h = app
        .glyph_cache
        .as_ref()
        .map(|c| c.cell_height)
        .unwrap_or(16.0);

    for (pane_id, tile_rect, _) in &tiles {
        if *pane_id != hovered_id {
            continue;
        }
        // Match the painter: apply content origin to the tile rect and
        // then transform. Transforming first (around the window center)
        // and offsetting afterwards shifts the result by
        // `content_origin * (1 - zoom)` relative to where the tile is
        // actually drawn, which misplaces the overlay.
        let content_x = app.content_origin_x();
        let content_y = app.content_origin_y();
        let offset_rect = GeoRect::new(
            tile_rect.x + content_x,
            tile_rect.y + content_y,
            tile_rect.w,
            tile_rect.h,
        );
        let tr = app.transformed_tile_rect(offset_rect, zoom, vw, vh);
        let min_w = cell_w * 14.0;
        if tr.w < min_w {
            return None;
        }
        let bar_h = (cell_h * 2.0).max(28.0);
        let bar_y = tr.y + tr.h - bar_h;
        let pane_x = tr.x;
        let half_w = tr.w / 2.0;
        return Some(OverviewActionBarData {
            pane_x,
            pane_w: tr.w,
            bar_y,
            bar_h,
            close_x: pane_x,
            close_w: half_w,
            focus_x: pane_x + half_w,
            focus_w: half_w,
        });
    }
    None
}

/// Paint the overview action bar (Close / Focus buttons).
pub(crate) fn paint_overview_action_bar(
    d: &OverviewActionBarData,
    hover: Option<super::super::OverviewActionHover>,
    cx: &UiContext<'_>,
    scene: &mut UiScene<'_>,
) {
    let accent = ciri_config::theme::ThemeConfig::parse_color(&cx.config.theme.accent);
    let red = ciri_config::theme::ThemeConfig::parse_color(&cx.config.theme.red);
    let fg = ciri_config::theme::ThemeConfig::parse_color(&cx.config.theme.foreground);
    let text_y = d.bar_y + (d.bar_h - cx.cell_h) * 0.5;

    let close_hovered = hover == Some(super::super::OverviewActionHover::Close);
    let focus_hovered = hover == Some(super::super::OverviewActionHover::Focus);

    // Semi-opaque backdrop over the tile so the action bar reads as an
    // overlay rather than part of the tile content.
    scene.bg_rects.push(Rect {
        x: d.pane_x,
        y: d.bar_y,
        w: d.pane_w,
        h: d.bar_h,
        color: [0.0, 0.0, 0.0, tokens::ALPHA_PRIMARY_HOVER],
    });
    // Vertical divider between Close / Focus.
    scene.bg_rects.push(Rect {
        x: d.focus_x,
        y: d.bar_y + tokens::SPACE_1,
        w: 1.0,
        h: d.bar_h - tokens::SPACE_1 * 2.0,
        color: tokens::tint(fg, tokens::ALPHA_SEPARATOR * 0.6),
    });

    let close_label = "\u{2715} Close";
    let focus_label = "Focus";
    let (close_text_w, focus_text_w) = if let Some(cell) = cx.ui_shaper {
        let mut s = cell.borrow_mut();
        if s.has_face() {
            (s.measure(close_label), s.measure(focus_label))
        } else {
            (
                close_label.chars().count() as f32 * cx.cell_w,
                focus_label.chars().count() as f32 * cx.cell_w,
            )
        }
    } else {
        (
            close_label.chars().count() as f32 * cx.cell_w,
            focus_label.chars().count() as f32 * cx.cell_w,
        )
    };

    if close_hovered {
        scene.bg_rects.push(Rect {
            x: d.close_x,
            y: d.bar_y,
            w: d.close_w,
            h: d.bar_h,
            color: tokens::tint(red, tokens::ALPHA_PRIMARY_REST),
        });
    }
    let close_text_x = d.close_x + (d.close_w - close_text_w) * 0.5;
    // Invert to full foreground on hover so contrast stays AA on the red
    // wash; resting state uses red to signal a destructive action.
    let close_text_color = if close_hovered {
        fg
    } else {
        tokens::tint(red, 1.0)
    };
    emit_status_text(
        scene.atlas,
        close_label,
        &TextEmitParams {
            x_start: close_text_x,
            y: text_y,
            cell_width: cx.cell_w,
            baseline: cx.baseline,
            color: close_text_color,
        },
        scene.glyphs,
        scene.color_glyphs,
    );

    if focus_hovered {
        scene.bg_rects.push(Rect {
            x: d.focus_x,
            y: d.bar_y,
            w: d.focus_w,
            h: d.bar_h,
            color: tokens::tint(accent, tokens::ALPHA_SELECTED_BG + 0.10),
        });
    }
    let focus_text_x = d.focus_x + (d.focus_w - focus_text_w) * 0.5;
    emit_status_text(
        scene.atlas,
        focus_label,
        &TextEmitParams {
            x_start: focus_text_x,
            y: text_y,
            cell_width: cx.cell_w,
            baseline: cx.baseline,
            color: fg,
        },
        scene.glyphs,
        scene.color_glyphs,
    );
}
