use super::types::{UiContext, UiScene};
use super::{BellFlashComponent, ImePreeditComponent, SearchBarComponent};

pub(crate) struct TransientOverlayFrame {
    pub(crate) search_bar: Option<SearchBarComponent>,
    pub(crate) bell_flash: Option<BellFlashComponent>,
    pub(crate) ime_preedit: Option<ImePreeditComponent>,
}

impl TransientOverlayFrame {
    pub(crate) fn is_empty(&self) -> bool {
        self.search_bar.is_none() && self.bell_flash.is_none() && self.ime_preedit.is_none()
    }

    pub(crate) fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        if let Some(component) = &self.search_bar {
            component.paint(cx, scene);
        }
        if let Some(component) = &self.bell_flash {
            component.paint(cx, scene);
        }
        if let Some(component) = &self.ime_preedit {
            component.paint(cx, scene);
        }
    }
}
