use super::bell_flash::BellFlashComponent;
use super::ime_preedit::ImePreeditComponent;
use super::search_bar::SearchBarComponent;
use super::types::{UiContext, UiScene};

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
