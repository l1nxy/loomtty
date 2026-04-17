//! `Text` — leaf element for a single string of chrome text.
//!
//! Carries content + an optional colour override. Sizing uses a
//! glyph-width heuristic for now (7px per char, 16px tall) so Taffy has
//! something concrete to lay out around; real shaping through
//! `UiTextShaper` lands with the renderer wiring PR. Paint emits nothing
//! in this PR — glyph atlas plumbing arrives with that same follow-up.

use crate::color::Color;
use crate::element::{Element, PaintCtx};

/// Logical-px glyph-width heuristic until the real shaper is bridged.
const HEURISTIC_ADVANCE: f32 = 7.0;
/// Logical-px UI line height default.
const HEURISTIC_LINE_H: f32 = 16.0;

/// Free constructor: `text("hi")` reads better than `Text::new("hi")`.
pub fn text(s: impl Into<String>) -> Text {
    Text::new(s)
}

/// A single run of text.
pub struct Text {
    content: String,
    color: Option<Color>,
}

impl Text {
    pub fn new(s: impl Into<String>) -> Self {
        Self {
            content: s.into(),
            color: None,
        }
    }

    /// Override the text colour (else inherits `text_color` from parent style).
    pub fn color(mut self, c: Color) -> Self {
        self.color = Some(c);
        self
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn color_ref(&self) -> Option<Color> {
        self.color
    }

    /// Heuristic advance width for layout. Char count ≠ glyph count in
    /// multi-codepoint scripts, but for chrome text this is close enough
    /// until the shaper bridge lands.
    fn heuristic_width(&self) -> f32 {
        HEURISTIC_ADVANCE * self.content.chars().count() as f32
    }
}

impl Element for Text {
    fn taffy_style(&self) -> taffy::Style {
        // Fixed-size leaf. Real font metrics flow in once the shaper is
        // wired; at that point this becomes `measure_function`-backed.
        taffy::Style {
            size: taffy::Size {
                width: taffy::Dimension::Length(self.heuristic_width()),
                height: taffy::Dimension::Length(HEURISTIC_LINE_H),
            },
            ..Default::default()
        }
    }

    fn type_id(&self) -> &'static str {
        "ciri.text"
    }

    fn paint(&self, _cx: &mut PaintCtx<'_>) {
        // Glyph emission lands with the renderer wiring PR.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn heuristic_width_scales_with_content() {
        let short = text("ab").heuristic_width();
        let long = text("abcdef").heuristic_width();
        assert!(long > short);
    }
}
