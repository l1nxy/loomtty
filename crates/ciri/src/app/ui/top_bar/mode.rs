use super::super::builder::UiBuilder;
use super::super::layout::{Axis, SizeHint, UiElement, UiRect};
use super::super::types::{UiAction, UiContext, UiScene};

pub(super) struct ModeIndicator<'a> {
    pub(super) label: &'a str,
    pub(super) color: [f32; 4],
    pub(super) width: f32,
}

impl<'a> UiElement for ModeIndicator<'a> {
    fn size_hint(&self, axis: Axis, _cx: &UiContext<'_>) -> SizeHint {
        match axis {
            Axis::Horizontal => SizeHint::Fixed(self.width),
            Axis::Vertical => SizeHint::Fill,
        }
    }

    fn paint(&self, rect: UiRect, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        if rect.is_empty() {
            return;
        }
        let padding = cx
            .config
            .statusbar
            .height_padding
            .unwrap_or(cx.cell_h * cx.config.statusbar.padding_ratio);
        let text_y = rect.y + padding * 0.5;

        let mut ui = UiBuilder::new_horizontal(
            rect.x, text_y, rect.w, cx.cell_h, 0.0, 0.0, 0.0, false, cx, scene,
        );
        ui.label(self.label, self.color);
    }

    fn hit(
        &self,
        rect: UiRect,
        mx: f32,
        my: f32,
        _cx: &UiContext<'_>,
    ) -> Option<UiAction> {
        rect.contains(mx, my).then_some(UiAction::ToggleOverview)
    }
}
