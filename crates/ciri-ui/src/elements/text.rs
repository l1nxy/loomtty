//! `Text` — leaf element for a single string of chrome text.
//!
//! Carries content, an optional colour override, and an optional
//! font-size override. Layout size is computed at `taffy_style()` time
//! via a heuristic (real shaper measurement happens during paint
//! through the injected [`crate::shaper::TextShaper`]). Paint emits
//! glyph instances into the host-provided shaper, which turns them
//! into GPU draws.

use crate::color::Color;
use crate::element::{Element, PaintCtx};

/// Heuristic logical-px advance ratio used when the element is built
/// without access to a shaper. Matches the NullShaper ratio so tests
/// get consistent numbers regardless of which shaper's measure is
/// consulted.
const HEURISTIC_ADVANCE_RATIO: f32 = 0.5;

/// Free constructor: `text("hi")` reads better than `Text::new("hi")`.
pub fn text(s: impl Into<String>) -> Text {
    Text::new(s)
}

/// A single run of text.
pub struct Text {
    content: String,
    color: Option<Color>,
    /// Logical-px font size. `None` means "use theme default
    /// (`typography.md`)" — resolved at paint time so runtime theme
    /// edits don't require rebuilding text elements.
    font_size_px: Option<f32>,
}

impl Text {
    pub fn new(s: impl Into<String>) -> Self {
        Self {
            content: s.into(),
            color: None,
            font_size_px: None,
        }
    }

    /// Override the text colour (else inherits `text_color` from parent
    /// style → theme `on_surface`).
    pub fn color(mut self, c: Color) -> Self {
        self.color = Some(c);
        self
    }

    /// Override the font size. Default: theme `typography.md`.
    pub fn size(mut self, px: f32) -> Self {
        self.font_size_px = Some(px);
        self
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn color_ref(&self) -> Option<Color> {
        self.color
    }

    pub fn font_size(&self) -> Option<f32> {
        self.font_size_px
    }

    /// Effective font size used by `taffy_style`. Falls back to a
    /// "looks like medium UI text" constant when no override is set,
    /// because `taffy_style` has no access to the theme.
    fn heuristic_font_size(&self) -> f32 {
        self.font_size_px.unwrap_or(13.0)
    }
}

impl Element for Text {
    fn taffy_style(&self) -> taffy::Style {
        let fs = self.heuristic_font_size();
        let w = fs * HEURISTIC_ADVANCE_RATIO * self.content.chars().count() as f32;
        // Fixed-size leaf. Real font metrics flow in once Taffy's
        // measure_function is wired to the shaper; at that point the
        // width-estimator becomes irrelevant.
        taffy::Style {
            size: taffy::Size {
                width: taffy::Dimension::Length(w),
                height: taffy::Dimension::Length(fs),
            },
            ..Default::default()
        }
    }

    fn type_id(&self) -> &'static str {
        "ciri.text"
    }

    fn paint(&self, cx: &mut PaintCtx<'_>) {
        if self.content.is_empty() {
            return;
        }
        let color = self.color.unwrap_or(cx.theme.on_surface);
        let font_size = self.font_size_px.unwrap_or(cx.theme.typography.md);
        let pos = [cx.bounds[0], cx.bounds[1]];
        cx.emit_text(&self.content, pos, color, font_size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::{Layer, PaintCtx};
    use crate::scene::Scene;
    use crate::shaper::{NullShaper, RecordingShaper};
    use crate::theme::ResolvedTheme;

    fn make_ctx<'a, S: crate::shaper::TextShaper>(
        theme: &'a ResolvedTheme,
        scene: &'a mut Scene,
        shaper: &'a mut S,
    ) -> PaintCtx<'a> {
        PaintCtx {
            theme,
            bounds: [10.0, 20.0, 80.0, 16.0],
            scene,
            text_shaper: shaper,
            scale: 1.0,
            element_id: Default::default(),
            inherited_opacity: 1.0,
            layer: Layer::Chrome,
        }
    }

    #[test]
    fn content_is_preserved() {
        let t = text("Connecting");
        assert_eq!(t.content(), "Connecting");
    }

    #[test]
    fn color_override_sticks() {
        let t = text("x").color([1.0, 0.0, 0.0, 1.0]);
        assert_eq!(t.color_ref(), Some([1.0, 0.0, 0.0, 1.0]));
    }

    #[test]
    fn size_override_sticks() {
        let t = text("x").size(20.0);
        assert_eq!(t.font_size(), Some(20.0));
    }

    #[test]
    fn empty_text_is_a_paint_noop() {
        let theme = ResolvedTheme::default();
        let mut scene = Scene::new();
        let mut shaper = RecordingShaper::default();
        let mut cx = make_ctx(&theme, &mut scene, &mut shaper);
        text("").paint(&mut cx);
        assert!(shaper.calls.is_empty());
        assert!(scene.is_empty());
    }

    #[test]
    fn paint_calls_shaper_with_resolved_color_and_size() {
        let theme = ResolvedTheme::default();
        let mut scene = Scene::new();
        let mut shaper = RecordingShaper::default();
        let mut cx = make_ctx(&theme, &mut scene, &mut shaper);
        text("hi").paint(&mut cx);
        assert_eq!(shaper.calls.len(), 1);
        let call = &shaper.calls[0];
        assert_eq!(call.content, "hi");
        assert_eq!(call.pos, [10.0, 20.0]);
        assert_eq!(call.color, theme.on_surface);
        assert!((call.font_size_px - theme.typography.md).abs() < 1e-6);
    }

    #[test]
    fn paint_uses_overrides_when_set() {
        let theme = ResolvedTheme::default();
        let mut scene = Scene::new();
        let mut shaper = RecordingShaper::default();
        let mut cx = make_ctx(&theme, &mut scene, &mut shaper);
        text("hi").color([1.0, 0.0, 0.0, 1.0]).size(20.0).paint(&mut cx);
        let call = &shaper.calls[0];
        assert_eq!(call.color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(call.font_size_px, 20.0);
    }

    #[test]
    fn inherited_opacity_fades_emitted_alpha() {
        let theme = ResolvedTheme::default();
        let mut scene = Scene::new();
        let mut shaper = RecordingShaper::default();
        let mut cx = make_ctx(&theme, &mut scene, &mut shaper);
        cx.inherited_opacity = 0.5;
        text("hi").color([1.0, 0.0, 0.0, 0.8]).paint(&mut cx);
        // α * inherited → 0.8 * 0.5 = 0.4
        assert!((shaper.calls[0].color[3] - 0.4).abs() < 1e-6);
    }

    #[test]
    fn null_shaper_does_not_emit_but_paint_still_runs() {
        // Ensures the NullShaper path is ergonomic for headless tests
        // that only care about layout / SdfRect output.
        let theme = ResolvedTheme::default();
        let mut scene = Scene::new();
        let mut shaper = NullShaper;
        let mut cx = make_ctx(&theme, &mut scene, &mut shaper);
        text("hello").paint(&mut cx);
        assert_eq!(scene.glyph_len(), 0);
    }
}
