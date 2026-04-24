use super::super::layout::{Axis, SizeHint, UiElement, UiRect};
use super::super::types::{UiAction, UiContext, UiScene};
use crate::app::ciri_ui_bridge::paint_ui_tree;
use ciri_ui::{Layer, Styled, div, text};

pub(super) struct WorkspaceIndicator<'a> {
    pub(super) label: &'a str,
    pub(super) hovered: bool,
    pub(super) width: f32,
}

impl<'a> UiElement for WorkspaceIndicator<'a> {
    fn size_hint(&self, axis: Axis, _cx: &UiContext<'_>) -> SizeHint {
        match axis {
            Axis::Horizontal => SizeHint::Fixed(self.width),
            Axis::Vertical => SizeHint::Fill,
        }
    }

    fn paint(&self, rect: UiRect, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        if self.label.is_empty() || rect.is_empty() {
            return;
        }
        let fg = cx.theme.on_surface;
        let accent = cx.theme.accent;
        let color = if self.hovered { fg } else { accent };
        let padding = cx
            .config
            .statusbar
            .height_padding
            .unwrap_or(cx.cell_h * cx.config.statusbar.padding_ratio);
        let text_y = rect.y + padding * 0.5;

        let root = div().w(cx.viewport_w).h(cx.viewport_h).child(
            div()
                .in_layer(Layer::Chrome)
                .w(rect.w)
                .h(cx.cell_h)
                .translate(rect.x, text_y)
                .child(text(self.label).color(color)),
        );
        paint_ui_tree(&root, cx, scene);
    }

    fn hit(&self, rect: UiRect, mx: f32, my: f32, _cx: &UiContext<'_>) -> Option<UiAction> {
        // Match the pre-split semantics: empty workspace label → not
        // clickable, even though Linear still hands us a zero-width rect.
        if self.width <= 0.0 || self.label.is_empty() {
            return None;
        }
        rect.contains(mx, my).then_some(UiAction::CycleWorkspace)
    }
}
