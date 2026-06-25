use loom_layout::geometry::Rect as GeoRect;

use super::tokens;
use super::types::{UiContext, UiOverviewHit, UiScene, ui_hit_id};
use crate::app::App;
use crate::app::loom_ui_adapter::paint_element_tree;
use loom_ui::{Div, IntoElement, Render, RenderCtx, Styled, deferred, div, text};

const HIT_ACTION_BAR: u64 = 1;
const HIT_CLOSE: u64 = 2;
const HIT_FOCUS: u64 = 3;

pub(crate) struct OverviewComponent {
    hovered_pane: Option<(usize, u64)>,
}

#[derive(Clone)]
pub(crate) struct OverviewActionBarData {
    pub pane_x: f32,
    pub pane_w: f32,
    pub bar_y: f32,
    pub bar_h: f32,
    pub close_w: f32,
    pub focus_w: f32,
}

/// Render-trait component wrapping the overview action bar so it goes
/// through the same paint pipeline as the rest of the chrome.
/// Hover styling is declarative (`.hover()` per button); cache
/// invalidation on cursor-cross-button-boundary stays correct because
/// the chrome cache hash hashes whichever button is currently
/// hovered (derived at hash time from cursor + bar geometry).
pub(crate) struct OverviewActionBarComponent {
    pub data: OverviewActionBarData,
}

impl OverviewComponent {
    pub fn capture(app: &App, _cx: &UiContext<'_>) -> Self {
        Self {
            hovered_pane: app.overview_hovered_pane,
        }
    }

    pub(super) fn hit_test(&self, app: &App, mx: f32, my: f32) -> UiOverviewHit {
        if !app.core.overview.active {
            return UiOverviewHit::None;
        }
        // Check action bar on hovered pane first. Pane tiles themselves
        // remain renderer/workspace-domain geometry; this overlay is a
        // regular UI tree and should use the same layout snapshot as paint.
        if let Some((ws_idx, hovered_id)) = self.hovered_pane
            && let Some(bar) = overview_action_bar_data(app, self.hovered_pane)
            && let Some(hit) =
                overview_action_bar_hit(&bar, ws_idx, hovered_id, mx, my, &app.ui_context())
        {
            return hit;
        }
        if let Some((ws_idx, pane_id)) = app.hit_test_overview(mx, my) {
            UiOverviewHit::Pane(ws_idx, pane_id)
        } else {
            UiOverviewHit::Background
        }
    }
}

fn overview_action_bar_hit(
    d: &OverviewActionBarData,
    ws_idx: usize,
    pane_id: u64,
    mx: f32,
    my: f32,
    cx: &UiContext<'_>,
) -> Option<UiOverviewHit> {
    // Reuse the same Render-trait tree the painter would build —
    // hit_ids are always present now (no more `with_hits` split),
    // so the hit-test walker sees the same layout the painter does.
    let render_cx = OverviewActionBarComponent::render_cx(cx);
    let root = OverviewActionBarComponent { data: d.clone() }.build_tree(&render_cx);
    match ui_hit_id(&root, cx, mx, my) {
        Some(HIT_CLOSE) => Some(UiOverviewHit::ClosePane(pane_id)),
        Some(HIT_FOCUS) => Some(UiOverviewHit::FocusPane(ws_idx, pane_id)),
        Some(HIT_ACTION_BAR) => Some(UiOverviewHit::Background),
        _ => None,
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
            close_w: half_w,
            focus_w: half_w,
        });
    }
    None
}

impl OverviewActionBarComponent {
    pub(crate) fn paint(&mut self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let render_cx = Self::render_cx(cx);
        // 11th production usage of `loom_ui::Render`. Hover is now
        // declarative on each button — refinement-aware text_color
        // inheritance (Step 16) propagates the hover-state colour
        // change to the descendant Text without an `if hovered { ... }`
        // branch.
        let root = <Self as Render>::render(self, &render_cx).into_element();
        paint_element_tree(&root, cx, scene);
    }

    fn render_cx<'a>(cx: &'a UiContext<'_>) -> RenderCtx<'a> {
        RenderCtx {
            theme: cx.theme,
            viewport: [cx.viewport_w, cx.viewport_h],
            scale: 1.0,
        }
    }

    fn build_tree(&self, cx: &RenderCtx<'_>) -> Div {
        let d = &self.data;
        let accent = cx.theme.accent;
        let red = cx.theme.error;
        let fg = cx.theme.on_surface;

        // Both buttons use refinement-aware text_color: rest text
        // colour on the Div (red for Close, fg for Focus), hover
        // refinement switches text_color (and adds a tinted bg).
        // Descendant Text inherits — no `.color()` on the Text node.
        let close_button = div()
            .w(d.close_w)
            .h(d.bar_h)
            .flex_row()
            .items_center()
            .justify_center()
            .text_color(red)
            .hit_id(HIT_CLOSE)
            .cursor_pointer()
            .hover(|s| {
                s.bg(tokens::tint(red, tokens::ALPHA_PRIMARY_REST))
                    .text_color(fg)
            })
            .child(text("\u{2715} Close"));

        let focus_button = div()
            .w((d.focus_w - 1.0).max(0.0))
            .h(d.bar_h)
            .flex_row()
            .items_center()
            .justify_center()
            .text_color(fg)
            .hit_id(HIT_FOCUS)
            .cursor_pointer()
            .hover(|s| s.bg(tokens::tint(accent, tokens::ALPHA_SELECTED_BG + 0.10)))
            .child(text("Focus"));

        let bar = div()
            .absolute()
            .left(d.pane_x)
            .top(d.bar_y)
            .w(d.pane_w)
            .h(d.bar_h)
            .flex_row()
            .items_center()
            .bg([0.0, 0.0, 0.0, tokens::ALPHA_PRIMARY_HOVER])
            .hit_id(HIT_ACTION_BAR);

        // The action bar is wrapped in `deferred()` so it sits z-on-top
        // of the overview tile thumbnails (painted earlier in the
        // OverviewComponent), matching the old `Layer::Overlay`
        // behaviour without the enum.
        let bar = bar
            .child(close_button)
            .child(
                div()
                    .w(1.0)
                    .h(d.bar_h - tokens::SPACE_1 * 2.0)
                    .bg(tokens::tint(fg, tokens::ALPHA_SEPARATOR * 0.6)),
            )
            .child(focus_button);
        div()
            .w(cx.viewport[0])
            .h(cx.viewport[1])
            .child(deferred(bar))
    }
}

impl Render for OverviewActionBarComponent {
    fn render(&mut self, cx: &RenderCtx<'_>) -> impl IntoElement {
        self.build_tree(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui::types::test_ui_context;
    use loom_config::config::LoomConfig;

    #[test]
    fn action_bar_hit_uses_loom_ui_layout_snapshot() {
        let config = LoomConfig::default();
        let theme = loom_ui::ResolvedTheme::default();
        let cx = test_ui_context(&config, &theme, 400.0, 240.0);
        let data = OverviewActionBarData {
            pane_x: 40.0,
            pane_w: 200.0,
            bar_y: 100.0,
            bar_h: 32.0,
            close_w: 100.0,
            focus_w: 100.0,
        };

        assert_eq!(
            overview_action_bar_hit(&data, 2, 99, 50.0, 110.0, &cx),
            Some(UiOverviewHit::ClosePane(99))
        );
        assert_eq!(
            overview_action_bar_hit(&data, 2, 99, 150.0, 110.0, &cx),
            Some(UiOverviewHit::FocusPane(2, 99))
        );
        assert_eq!(
            overview_action_bar_hit(&data, 2, 99, 140.5, 110.0, &cx),
            Some(UiOverviewHit::Background)
        );
        assert_eq!(overview_action_bar_hit(&data, 2, 99, 10.0, 10.0, &cx), None);
    }
}
