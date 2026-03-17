use glyphon::FontSystem;

/// Holds the font system used by the glyph atlas for font discovery.
/// The glyphon text rendering pipeline is not used — we render directly
/// via the custom glyph atlas + instanced quads.
pub struct TextRenderer {
    pub font_system: FontSystem,
}

impl Default for TextRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl TextRenderer {
    pub fn new() -> Self {
        TextRenderer {
            font_system: FontSystem::new(),
        }
    }
}
