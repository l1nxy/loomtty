//! Text shaping via rustybuzz for ligature and complex text layout support.

use fontdb;
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
///
/// Owns a standalone `fontdb::Database` for font discovery and raw font byte
/// access. This replaces the previous `cosmic_text::FontSystem` dependency.
pub struct TextShaper {
    db: fontdb::Database,
    fonts: HashMap<fontdb::ID, FontData>,
    primary_font_id: Option<fontdb::ID>,
    emoji_font_id: Option<fontdb::ID>,
    cjk_font_id: Option<fontdb::ID>,
}

impl TextShaper {
    /// Create a new TextShaper that discovers system fonts via fontdb.
    ///
    /// Finds the primary font matching `family_name` and preloads its data
    /// for shaping. Falls back to the first monospaced font if no match.
    pub fn new(family_name: &str) -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();

        let primary_font_id = find_primary_font(&db, family_name);
        let emoji_font_id = find_emoji_font(&db);
        let cjk_font_id = find_cjk_font(&db, primary_font_id);

        let mut shaper = TextShaper {
            db,
            fonts: HashMap::new(),
            primary_font_id,
            emoji_font_id,
            cjk_font_id,
        };

        // Preload primary font data for shaping
        if let Some(fid) = primary_font_id {
            shaper.load_font(fid);
        }
        // Preload emoji font data for fallback shaping
        if let Some(eid) = emoji_font_id
            && Some(eid) != primary_font_id
        {
            shaper.load_font(eid);
        }
        // Preload CJK font data for fallback shaping
        if let Some(cid) = cjk_font_id
            && Some(cid) != primary_font_id
            && Some(cid) != emoji_font_id
        {
            shaper.load_font(cid);
        }

        shaper
    }

    pub fn primary_font_id(&self) -> Option<fontdb::ID> {
        self.primary_font_id
    }

    pub fn emoji_font_id(&self) -> Option<fontdb::ID> {
        self.emoji_font_id
    }

    pub fn cjk_font_id(&self) -> Option<fontdb::ID> {
        self.cjk_font_id
    }

    /// Return the file path and face index of the primary font, for FreeType loading.
    pub fn primary_font_path(&self) -> Option<(String, u32)> {
        self.font_path(self.primary_font_id?)
    }

    /// Return the file path and face index of the emoji font, for FreeType loading.
    pub fn emoji_font_path(&self) -> Option<(String, u32)> {
        self.font_path(self.emoji_font_id?)
    }

    /// Return the file path and face index of the CJK font, for FreeType loading.
    pub fn cjk_font_path(&self) -> Option<(String, u32)> {
        self.font_path(self.cjk_font_id?)
    }

    fn font_path(&self, fid: fontdb::ID) -> Option<(String, u32)> {
        let face = self.db.face(fid)?;
        match &face.source {
            fontdb::Source::File(path) => Some((path.to_string_lossy().to_string(), face.index)),
            _ => None,
        }
    }

    /// Load font data for a given font ID. No-op if already loaded.
    pub fn load_font(&mut self, font_id: fontdb::ID) {
        if self.fonts.contains_key(&font_id) {
            return;
        }
        let Some(face_info) = self.db.face(font_id) else {
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

    /// Shape a grapheme cluster, trying the primary font first, then CJK, then emoji.
    /// Returns `(glyph_id, font_id)` on success.
    pub fn shape_grapheme_with_fallback(
        &self,
        cluster: &str,
        primary_face: &rustybuzz::Face,
    ) -> Option<(u32, fontdb::ID)> {
        self.try_shape_with_fallback_faces(cluster, primary_face, Self::shape_grapheme_with_face)
    }

    /// Shape a single character with fallback through primary → CJK → emoji fonts.
    /// Returns `(glyph_id, font_id)` on success.
    pub fn shape_char_with_fallback(
        &self,
        ch: char,
        primary_face: &rustybuzz::Face,
    ) -> Option<(u32, fontdb::ID)> {
        let s = String::from(ch);
        self.try_shape_with_fallback_faces(&s, primary_face, Self::shape_single_char)
    }

    /// Shape a single character string and return its glyph ID.
    /// Unlike `shape_grapheme_with_face`, does NOT require GSUB reduction.
    fn shape_single_char(&self, s: &str, face: &rustybuzz::Face) -> Option<u32> {
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(s);
        let output = rustybuzz::shape(face, &[], buffer);
        output
            .glyph_infos()
            .iter()
            .find(|gi| gi.glyph_id != 0)
            .map(|gi| gi.glyph_id)
    }

    fn try_shape_with_fallback_faces(
        &self,
        text: &str,
        primary_face: &rustybuzz::Face,
        shape: fn(&Self, &str, &rustybuzz::Face) -> Option<u32>,
    ) -> Option<(u32, fontdb::ID)> {
        if let Some(font_id) = self.primary_font_id
            && let Some(glyph_id) = shape(self, text, primary_face)
        {
            return Some((glyph_id, font_id));
        }

        self.try_shape_with_font_id(text, self.cjk_font_id, shape)
            .or_else(|| self.try_shape_with_font_id(text, self.emoji_font_id, shape))
    }

    fn try_shape_with_font_id(
        &self,
        text: &str,
        font_id: Option<fontdb::ID>,
        shape: fn(&Self, &str, &rustybuzz::Face) -> Option<u32>,
    ) -> Option<(u32, fontdb::ID)> {
        let font_id = font_id?;
        let face = self.create_face(font_id)?;
        let glyph_id = shape(self, text, &face)?;
        Some((glyph_id, font_id))
    }

    /// Shape a grapheme cluster using a pre-created face.
    ///
    /// Returns `None` if the font doesn't actually combine the cluster into
    /// fewer glyphs than input characters (i.e., no GSUB substitution happened).
    pub fn shape_grapheme_with_face(&self, cluster: &str, face: &rustybuzz::Face) -> Option<u32> {
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(cluster);

        let output = rustybuzz::shape(face, &[], buffer);
        let infos = output.glyph_infos();

        // If shaping produced as many glyphs as input chars, the font didn't
        // actually combine them — it just mapped each char individually.
        let input_chars = cluster.chars().count();
        if input_chars > 1 && infos.len() >= input_chars {
            return None;
        }

        infos
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

/// Find the primary font ID for the given family name.
fn find_primary_font(db: &fontdb::Database, family_name: &str) -> Option<fontdb::ID> {
    let family_lower = family_name.to_ascii_lowercase();

    // Exact match first
    for face in db.faces() {
        if face.style != fontdb::Style::Normal {
            continue;
        }
        for family in &face.families {
            if family.0.eq_ignore_ascii_case(family_name) {
                log::info!(
                    "primary font: {} (monospaced={})",
                    family.0,
                    face.monospaced
                );
                return Some(face.id);
            }
        }
    }

    // Substring match
    for face in db.faces() {
        if face.style != fontdb::Style::Normal {
            continue;
        }
        for family in &face.families {
            if family.0.to_ascii_lowercase().contains(&family_lower) {
                log::info!(
                    "primary font (substring): {} (monospaced={})",
                    family.0,
                    face.monospaced
                );
                return Some(face.id);
            }
        }
    }

    // First monospaced font
    for face in db.faces() {
        if face.monospaced && face.style == fontdb::Style::Normal {
            let name = face.families.first().map(|f| f.0.as_str()).unwrap_or("?");
            log::info!("primary font (monospace fallback): {}", name);
            return Some(face.id);
        }
    }

    log::warn!("no suitable font found for '{}'", family_name);
    None
}

/// Find a CJK font from system fonts for fallback rendering.
/// Skips the primary font since it's already tried first.
/// Prefers Normal style and Regular (400) weight.
fn find_cjk_font(db: &fontdb::Database, primary: Option<fontdb::ID>) -> Option<fontdb::ID> {
    let known = [
        "Noto Sans CJK SC",
        "Noto Sans CJK TC",
        "Noto Sans CJK JP",
        "Noto Sans CJK KR",
        "Noto Sans CJK",
        "Source Han Sans SC",
        "Source Han Sans",
        "WenQuanYi Micro Hei Mono",
        "WenQuanYi Micro Hei",
        "WenQuanYi Zen Hei Mono",
        "WenQuanYi Zen Hei",
        "Microsoft YaHei",
        "SimHei",
        "Hiragino Sans",
        "PingFang SC",
        "Apple SD Gothic Neo",
    ];
    // First pass: exact family name, prefer regular weight
    for name in &known {
        let mut best: Option<fontdb::ID> = None;
        let mut best_weight_dist = u16::MAX;
        for face in db.faces() {
            if Some(face.id) == primary || face.style != fontdb::Style::Normal {
                continue;
            }
            for fam in &face.families {
                if fam.0.eq_ignore_ascii_case(name) {
                    let dist = (face.weight.0 as i32 - 400).unsigned_abs() as u16;
                    if dist < best_weight_dist {
                        best = Some(face.id);
                        best_weight_dist = dist;
                    }
                }
            }
        }
        if let Some(id) = best {
            let w = db.face(id).map(|f| f.weight.0).unwrap_or(0);
            log::info!("CJK font: {} (weight={})", name, w);
            return Some(id);
        }
    }
    // Fallback: any font with CJK-related keywords, prefer regular weight
    let keywords = [
        "cjk",
        "han",
        "wenquanyi",
        "yahei",
        "simhei",
        "hiragino",
        "pingfang",
    ];
    let mut best: Option<(fontdb::ID, u16)> = None;
    for face in db.faces() {
        if Some(face.id) == primary || face.style != fontdb::Style::Normal {
            continue;
        }
        for fam in &face.families {
            let lower = fam.0.to_ascii_lowercase();
            if keywords.iter().any(|kw| lower.contains(kw)) {
                let dist = (face.weight.0 as i32 - 400).unsigned_abs() as u16;
                if best.is_none_or(|(_, d)| dist < d) {
                    log::info!("CJK font candidate: {} (weight={})", fam.0, face.weight.0);
                    best = Some((face.id, dist));
                }
            }
        }
    }
    if let Some((id, _)) = best {
        return Some(id);
    }
    log::info!("no CJK font found on system");
    None
}

/// Find a color emoji font from system fonts.
fn find_emoji_font(db: &fontdb::Database) -> Option<fontdb::ID> {
    // Prioritized list of known emoji font families
    let known = [
        "Noto Color Emoji",
        "Apple Color Emoji",
        "Segoe UI Emoji",
        "Twemoji",
        "Twitter Color Emoji",
        "EmojiOne Color",
        "JoyPixels",
    ];
    for name in &known {
        for face in db.faces() {
            for fam in &face.families {
                if fam.0.eq_ignore_ascii_case(name) {
                    log::info!("emoji font: {}", fam.0);
                    return Some(face.id);
                }
            }
        }
    }
    // Fallback: any font with "emoji" in the family name
    for face in db.faces() {
        for fam in &face.families {
            if fam.0.to_ascii_lowercase().contains("emoji") {
                log::info!("emoji font (fallback match): {}", fam.0);
                return Some(face.id);
            }
        }
    }
    log::info!("no emoji font found on system");
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shaper_new_empty() {
        let shaper = TextShaper::new("NonexistentFontFamily12345");
        // May or may not find a font depending on system, but shouldn't panic
        let _ = shaper.primary_font_id();
    }

    #[test]
    fn detect_ligatures_no_font() {
        let shaper = TextShaper::new("NonexistentFontFamily12345");
        let ligs = shaper.detect_ligatures("hello", fontdb::ID::dummy());
        assert!(ligs.is_empty());
    }

    #[test]
    fn shape_grapheme_no_font() {
        let shaper = TextShaper::new("NonexistentFontFamily12345");
        assert!(shaper.shape_grapheme("a", fontdb::ID::dummy()).is_none());
    }
}
