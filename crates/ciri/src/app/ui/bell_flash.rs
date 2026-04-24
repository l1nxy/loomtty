use ciri_layout::geometry::Rect as GeoRect;
use ciri_ui::{Layer, Styled, div};

use super::types::{UiContext, UiScene};
use crate::app::ciri_ui_bridge::paint_ui_tree;

#[derive(Debug, Clone, Copy)]
pub(crate) struct BellFlashRect {
    pub(crate) rect: GeoRect,
    pub(crate) intensity: f32,
}

pub(crate) struct BellFlashComponent {
    pub(crate) flashes: Vec<BellFlashRect>,
}

impl BellFlashComponent {
    pub(crate) fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        if self.flashes.is_empty() {
            return;
        }

        let mut root = div().w(cx.viewport_w).h(cx.viewport_h);
        for flash in &self.flashes {
            let alpha = 0.15 * flash.intensity;
            let rect = flash.rect;
            root = root.child(
                div()
                    .in_layer(Layer::Overlay)
                    .absolute()
                    .left(rect.x)
                    .top(rect.y)
                    .w(rect.w)
                    .h(rect.h)
                    .bg([1.0, 0.9, 0.5, alpha]),
            );
        }

        paint_ui_tree(&root, cx, scene);
    }
}
