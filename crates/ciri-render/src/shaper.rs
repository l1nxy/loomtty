//! Text shaping via rustybuzz for ligature and complex text layout support.

use cosmic_text::FontSystem;
use cosmic_text::fontdb;
use std::collections::HashMap;

/// Font data cached for text shaping.
struct FontData {
    data: Vec<u8>,
    face_index: u32,
}

/// Detected ligature: multiple input characters shaped into a single glyph.
#[derive(Debug, Clone)]
pub struct Ligature {
    /// Starting column (character index) in the input text.
    pub start_col: usize,
    /// Number of input characters consumed by this ligature.
    pub char_count: usize,
    /// The glyph ID produced by the shaper.
    pub glyph_id: u32,
    /// The font ID that produced this glyph.
    pub font_id: fontdb::ID,
}

/// Text shaper using rustybuzz (Rust port of HarfBuzz).
pub struct TextShaper {
    fonts: HashMap<fontdb::ID, FontData>,
    primary_font_id: Option<fontdb::ID>,
}

impl TextShaper {
    pub fn new(primary_font_id: Option<fontdb::ID>) -> Self {
        TextShaper {
            fonts: HashMap::new(),
            primary_font_id,
        }
    }

    pub fn primary_font_id(&self) -> Option<fontdb::ID> {
        self.primary_font_id
    }

    /// Load font data for a given font ID. No-op if already loaded.
    pub fn load_font(&mut self, font_id: fontdb::ID, font_system: &FontSystem) {
        if self.fonts.contains_key(&font_id) {
            return;
        }
        let db = font_system.db();
        let Some(face_info) = db.face(font_id) else {
            return;
        };
        let face_index = face_info.index;
        let data = match &face_info.source {
            fontdb::Source::File(path) => std::fs::read(path).ok(),
            fontdb::Source::Binary(arc) => {
                let slice: &[u8] = (*arc).as_ref().as_ref();
                Some(slice.to_vec())
            }
            fontdb::Source::SharedFile(_, arc) => {
                let slice: &[u8] = (*arc).as_ref().as_ref();
                Some(slice.to_vec())
            }
        };

        if let Some(data) = data {
            log::info!(
                "loaded font data for shaping: {} bytes, face_index={}",
                data.len(),
                face_index
            );
            self.fonts.insert(font_id, FontData { data, face_index });
        }
    }

    /// Check if a font is loaded for shaping.
    pub fn has_font(&self, font_id: fontdb::ID) -> bool {
        self.fonts.contains_key(&font_id)
    }

    /// Create a reusable Face from cached font data.
    /// The returned Face borrows from this TextShaper and can be passed to
    /// `detect_ligatures_with_face` / `shape_grapheme_with_face` to avoid
    /// re-parsing the font on every call.
    pub fn create_face(&self, font_id: fontdb::ID) -> Option<rustybuzz::Face<'_>> {
        let font_data = self.fonts.get(&font_id)?;
        rustybuzz::Face::from_slice(&font_data.data, font_data.face_index)
    }

    /// Detect ligatures using a pre-created face (avoids Face re-creation per call).
    pub fn detect_ligatures_with_face(
        &self,
        text: &str,
        face: &rustybuzz::Face,
        font_id: fontdb::ID,
    ) -> Vec<Ligature> {
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(text);

        let output = rustybuzz::shape(face, &[], buffer);
        let infos = output.glyph_infos();

        if infos.is_empty() {
            return Vec::new();
        }

        let char_byte_starts: Vec<usize> = text.char_indices().map(|(bi, _)| bi).collect();
        let char_count = char_byte_starts.len();
        let byte_to_char = |byte_off: usize| -> usize {
            char_byte_starts
                .binary_search(&byte_off)
                .unwrap_or_else(|x| x)
        };
        let mut ligatures = Vec::new();

        for i in 0..infos.len() {
            let cluster_start = byte_to_char(infos[i].cluster as usize);
            let cluster_end = if i + 1 < infos.len() {
                byte_to_char(infos[i + 1].cluster as usize)
            } else {
                char_count
            };

            let span = cluster_end.saturating_sub(cluster_start);
            if span > 1 && infos[i].glyph_id != 0 {
                ligatures.push(Ligature {
                    start_col: cluster_start,
                    char_count: span,
                    glyph_id: infos[i].glyph_id,
                    font_id,
                });
            }
        }

        ligatures
    }

    /// Shape a grapheme cluster using a pre-created face.
    pub fn shape_grapheme_with_face(&self, cluster: &str, face: &rustybuzz::Face) -> Option<u32> {
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(cluster);

        let output = rustybuzz::shape(face, &[], buffer);
        output
            .glyph_infos()
            .iter()
            .find(|gi| gi.glyph_id != 0)
            .map(|gi| gi.glyph_id)
    }

    /// Detect ligatures in a line of text for the given font.
    /// Returns ligatures where multiple input characters map to a single glyph.
    pub fn detect_ligatures(&self, text: &str, font_id: fontdb::ID) -> Vec<Ligature> {
        let Some(face) = self.create_face(font_id) else {
            return Vec::new();
        };
        self.detect_ligatures_with_face(text, &face, font_id)
    }

    /// Shape a grapheme cluster (multi-codepoint string) and return the primary glyph ID.
    /// Returns None if the font doesn't have a glyph for this cluster.
    pub fn shape_grapheme(&self, cluster: &str, font_id: fontdb::ID) -> Option<u32> {
        let face = self.create_face(font_id)?;
        self.shape_grapheme_with_face(cluster, &face)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shaper_new_empty() {
        let shaper = TextShaper::new(None);
        assert!(!shaper.has_font(fontdb::ID::dummy()));
    }

    #[test]
    fn detect_ligatures_no_font() {
        let shaper = TextShaper::new(None);
        let ligs = shaper.detect_ligatures("hello", fontdb::ID::dummy());
        assert!(ligs.is_empty());
    }

    #[test]
    fn shape_grapheme_no_font() {
        let shaper = TextShaper::new(None);
        assert!(shaper.shape_grapheme("a", fontdb::ID::dummy()).is_none());
    }
}
