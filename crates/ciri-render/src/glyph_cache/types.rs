//! Data types for the glyph cache subsystem.

#[cfg(target_os = "linux")]
use crossfont::FontKey;

// ─── Font style ──────────────────────────────────────────────────────

/// Font style for glyph cache lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontStyle {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

impl FontStyle {
    /// Select font style from bold/italic flags.
    pub fn from_bold_italic(bold: bool, italic: bool) -> Self {
        match (bold, italic) {
            (true, true) => Self::BoldItalic,
            (true, false) => Self::Bold,
            (false, true) => Self::Italic,
            (false, false) => Self::Regular,
        }
    }
}

// ─── Font key set ────────────────────────────────────────────────────

/// The 4 crossfont FontKeys for regular/bold/italic/bold_italic.
#[cfg(target_os = "linux")]
pub(crate) struct FontKeySet {
    pub(crate) regular: FontKey,
    pub(crate) bold: FontKey,
    pub(crate) italic: FontKey,
    pub(crate) bold_italic: FontKey,
}

#[cfg(target_os = "linux")]
impl FontKeySet {
    pub(crate) fn get(&self, style: FontStyle) -> FontKey {
        match style {
            FontStyle::Regular => self.regular,
            FontStyle::Bold => self.bold,
            FontStyle::Italic => self.italic,
            FontStyle::BoldItalic => self.bold_italic,
        }
    }
}

// ─── Glyph entry ─────────────────────────────────────────────────────

/// UV coordinates and metrics of a cached glyph in the atlas.
#[derive(Debug, Clone, Copy)]
pub struct GlyphEntry {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
    pub width: u16,
    pub height: u16,
    pub bearing_x: f32,
    pub bearing_y: f32,
    /// True if this glyph was rasterized as RGBA color (emoji).
    pub is_color: bool,
}

impl GlyphEntry {
    pub const EMPTY: Self = GlyphEntry {
        u0: 0.0,
        v0: 0.0,
        u1: 0.0,
        v1: 0.0,
        width: 0,
        height: 0,
        bearing_x: 0.0,
        bearing_y: 0.0,
        is_color: false,
    };
}

/// Font class for glyph-ID cache key discrimination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FontClass {
    Primary,
    Emoji,
    Cjk,
}

// ─── Per-instance data ───────────────────────────────────────────────

/// Per-instance data for instanced glyph rendering.
/// `pos`/`size` are in pixel coordinates; the vertex shader converts to NDC.
#[repr(C)]
#[derive(Copy, Clone, bytemuck_derive::Pod, bytemuck_derive::Zeroable)]
pub struct GlyphInstance {
    pub pos: [f32; 2],     // pixel position (top-left of glyph quad)
    pub size: [f32; 2],    // pixel size
    pub uv_pos: [f32; 2],  // atlas UV top-left
    pub uv_size: [f32; 2], // atlas UV size
    pub color: [f32; 4],   // RGBA color
}

/// Draw range clipped to a scissor rect.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScissoredRange {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub start: usize,
    pub end: usize,
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn font_style_hash_distinct() {
        let mut map = HashMap::new();
        map.insert(('A', FontStyle::Regular), 1);
        map.insert(('A', FontStyle::Bold), 2);
        map.insert(('A', FontStyle::Italic), 3);
        assert_eq!(map.len(), 3);
        assert_eq!(map[&('A', FontStyle::Bold)], 2);
    }

    #[test]
    fn font_style_from_bold_italic() {
        assert_eq!(
            FontStyle::from_bold_italic(false, false),
            FontStyle::Regular
        );
        assert_eq!(FontStyle::from_bold_italic(true, false), FontStyle::Bold);
        assert_eq!(FontStyle::from_bold_italic(false, true), FontStyle::Italic);
        assert_eq!(
            FontStyle::from_bold_italic(true, true),
            FontStyle::BoldItalic
        );
    }
}
