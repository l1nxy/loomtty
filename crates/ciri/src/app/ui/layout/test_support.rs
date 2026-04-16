//! Shared test fixtures used by `linear_tests.rs` and `border_tests.rs`.
//!
//! Defines a few small probe types (so individual test files don't have
//! to redefine `Probe` / `OffsetProbe` / `dummy_cx`) and exposes them
//! via `pub(super)` so per-test-file modules can `use` them.

use super::super::types::{UiAction, UiContext, UiScene};
use super::element::UiElement;
use super::hint::{Axis, SizeHint};
use super::offset::Offset;
use super::rect::UiRect;

/// A leaf with configurable per-axis size hints, ignoring paint+hit.
pub(super) struct Probe {
    hint_main: SizeHint,
    hint_cross: SizeHint,
    main_axis: Axis,
}

impl Probe {
    pub(super) fn fixed_horizontal(w: f32) -> Self {
        Self {
            hint_main: SizeHint::Fixed(w),
            hint_cross: SizeHint::Fill,
            main_axis: Axis::Horizontal,
        }
    }
    pub(super) fn fill_horizontal() -> Self {
        Self {
            hint_main: SizeHint::Fill,
            hint_cross: SizeHint::Fill,
            main_axis: Axis::Horizontal,
        }
    }
}

impl UiElement for Probe {
    fn size_hint(&self, axis: Axis, _cx: &UiContext<'_>) -> SizeHint {
        if axis == self.main_axis {
            self.hint_main
        } else {
            self.hint_cross
        }
    }
    fn paint(&self, _rect: UiRect, _cx: &UiContext<'_>, _scene: &mut UiScene<'_>) {}
}

/// Probe that returns a specified `render_offset` and a marker hit
/// action (`UiAction::CycleWorkspace`) so callers can assert the hit
/// reached this probe.
pub(super) struct OffsetProbe {
    offset: Offset,
}

impl OffsetProbe {
    pub(super) fn new(offset: Offset) -> Self {
        Self { offset }
    }
}

impl UiElement for OffsetProbe {
    fn size_hint(&self, _axis: Axis, _cx: &UiContext<'_>) -> SizeHint {
        SizeHint::Fixed(50.0)
    }
    fn paint(&self, _rect: UiRect, _cx: &UiContext<'_>, _scene: &mut UiScene<'_>) {}
    fn hit(
        &self,
        _rect: UiRect,
        _mx: f32,
        _my: f32,
        _cx: &UiContext<'_>,
    ) -> Option<UiAction> {
        Some(UiAction::CycleWorkspace)
    }
    fn render_offset(&self) -> Offset {
        self.offset
    }
}

/// Synthetic `UiContext` for tests that don't actually read the config.
/// Leaks a default `CiriConfig` for `'static` borrow.
pub(super) fn dummy_cx() -> UiContext<'static> {
    let cfg = Box::leak(Box::new(ciri_config::config::CiriConfig::default()));
    UiContext {
        config: cfg,
        viewport_w: 0.0,
        viewport_h: 0.0,
        cell_w: 8.0,
        cell_h: 16.0,
        baseline: 12.0,
        ui_line_h: 16.0,
        ui_shaper: None,
    }
}
