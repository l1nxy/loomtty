use loom_layout::geometry::Rect as GeoRect;
use loom_ui::{Div, IntoElement, Render, RenderCtx, Styled, deferred, div};

use super::types::{UiContext, UiScene};
use crate::app::loom_ui_adapter::paint_element_tree;

#[derive(Debug, Clone, Copy)]
pub(crate) struct BellFlashRect {
    pub(crate) rect: GeoRect,
    pub(crate) intensity: f32,
}

pub(crate) struct BellFlashComponent {
    pub(crate) flashes: Vec<BellFlashRect>,
}

impl BellFlashComponent {
    pub(crate) fn paint(&mut self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        if self.flashes.is_empty() {
            return;
        }
        let render_cx = RenderCtx {
            theme: cx.theme,
            viewport: [cx.viewport_w, cx.viewport_h],
            scale: 1.0,
        };
        // 8th production usage of `loom_ui::Render`. Bell flash is
        // tiny — a viewport-sized wrapper with one tinted rect per
        // flashing pane. No host-derived metrics in `build_tree`, so
        // the trait migration is mechanical.
        let root = <Self as Render>::render(self, &render_cx).into_element();
        paint_element_tree(&root, cx, scene);
    }

    fn build_tree(&self, cx: &RenderCtx<'_>) -> Div {
        let mut root = div().w(cx.viewport[0]).h(cx.viewport[1]);
        for flash in &self.flashes {
            let alpha = 0.15 * flash.intensity;
            let rect = flash.rect;
            // Each flash gets its own `deferred()` so it sits above
            // pane content and other chrome merged earlier; matches
            // the old `Layer::Overlay` placement without the enum.
            root = root.child(deferred(
                div()
                    .absolute()
                    .left(rect.x)
                    .top(rect.y)
                    .w(rect.w)
                    .h(rect.h)
                    .bg([1.0, 0.9, 0.5, alpha]),
            ));
        }
        root
    }
}

impl Render for BellFlashComponent {
    fn render(&mut self, cx: &RenderCtx<'_>) -> impl IntoElement {
        self.build_tree(cx)
    }
}
