//! `Text` — leaf element for a single string of chrome text.
//!
//! Intentionally minimal in the foundation PR: carries the content + an
//! optional colour override. Font sizing + shaping integration with
//! `ciri-render`'s `UiTextShaper` lands with the renderer wiring.

use crate::color::Color;
use crate::element::{Element, PaintCtx};

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
}

impl Element for Text {
    fn type_id(&self) -> &'static str {
        "ciri.text"
    }

    fn paint(&self, _cx: &mut PaintCtx<'_>) {
        // Foundation PR: glyph emission lands with the renderer wiring.
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
}
