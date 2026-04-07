//! Generic cmap-based font resolver.
//!
//! Determines the preferred font by checking:
//! 1. Unicode Emoji_Presentation property (UTR #51)
//! 2. Whether the primary font actually has a cmap entry for the codepoint
//! 3. CJK range heuristics
//!
//! This works on all platforms and requires no OS font APIs.

use super::{FontResolver, ResolvedFont};
use std::collections::HashSet;
use std::sync::Arc;

/// Font resolver based on cmap coverage and Unicode properties.
///
/// Pre-computes a set of codepoints covered by each font (primary, CJK,
/// emoji) and uses Unicode Emoji_Presentation to decide fallback order.
pub struct CmapResolver {
    primary_cmap: HashSet<u32>,
    cjk_cmap: Option<HashSet<u32>>,
    emoji_cmap: Option<HashSet<u32>>,
}

impl CmapResolver {
    /// Build a resolver from raw font data.
    ///
    /// Each font is specified as `Option<(data, face_index)>`. The cmap
    /// table is parsed once at construction time.
    pub fn new(
        primary: (&[u8], u32),
        cjk: Option<(&[u8], u32)>,
        emoji: Option<(&[u8], u32)>,
    ) -> Self {
        CmapResolver {
            primary_cmap: extract_cmap(primary.0, primary.1),
            cjk_cmap: cjk.map(|(d, i)| extract_cmap(d, i)),
            emoji_cmap: emoji.map(|(d, i)| extract_cmap(d, i)),
        }
    }

    /// Build from `Arc<Vec<u8>>` font data (matches `TextShaper`'s storage).
    pub fn from_arcs(
        primary: (&Arc<Vec<u8>>, u32),
        cjk: Option<(&Arc<Vec<u8>>, u32)>,
        emoji: Option<(&Arc<Vec<u8>>, u32)>,
    ) -> Self {
        Self::new(
            (primary.0, primary.1),
            cjk.map(|(d, i)| (d.as_slice(), i)),
            emoji.map(|(d, i)| (d.as_slice(), i)),
        )
    }
}

impl FontResolver for CmapResolver {
    fn resolve_char(&self, ch: char) -> ResolvedFont {
        let cp = ch as u32;

        // Emoji_Presentation characters should prefer the emoji font.
        if is_default_emoji_presentation(ch) {
            if self.emoji_cmap.as_ref().is_some_and(|c| c.contains(&cp)) {
                return ResolvedFont::Emoji;
            }
            // Emoji font doesn't have it — fall through to primary/CJK.
        }

        // Primary font has it → use primary (most common path).
        if self.primary_cmap.contains(&cp) {
            return ResolvedFont::Primary;
        }

        // CJK fallback.
        if self.cjk_cmap.as_ref().is_some_and(|c| c.contains(&cp)) {
            return ResolvedFont::Cjk;
        }

        // Emoji fallback (for non-Emoji_Presentation characters that the
        // primary and CJK fonts lack).
        if self.emoji_cmap.as_ref().is_some_and(|c| c.contains(&cp)) {
            return ResolvedFont::Emoji;
        }

        // No font has it — return Primary and let the rasterizer produce .notdef.
        ResolvedFont::Primary
    }
}

/// Extract the set of codepoints covered by a font's cmap table.
fn extract_cmap(data: &[u8], face_index: u32) -> HashSet<u32> {
    let mut set = HashSet::new();
    let Ok(face) = ttf_parser::Face::parse(data, face_index) else {
        return set;
    };
    // ttf_parser doesn't expose a cmap iterator directly, but we can
    // enumerate the subtables and collect mapped codepoints.
    if let Some(cmap) = face.tables().cmap {
        for subtable in cmap.subtables.into_iter() {
            if !subtable.is_unicode() {
                continue;
            }
            subtable.codepoints(|cp| {
                set.insert(cp);
            });
        }
    }
    set
}

/// Check if a character has default Emoji_Presentation per Unicode UTR #51.
///
/// Characters with this property should default to emoji-style rendering
/// (color glyphs) rather than text-style (monochrome).
fn is_default_emoji_presentation(ch: char) -> bool {
    use unicode_properties::EmojiStatus::*;
    use unicode_properties::UnicodeEmoji;
    matches!(
        ch.emoji_status(),
        EmojiPresentation
            | EmojiPresentationAndModifierBase
            | EmojiPresentationAndEmojiComponent
            | EmojiPresentationAndModifierAndEmojiComponent
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emoji_presentation_basic() {
        // 😀 U+1F600 — has Emoji_Presentation
        assert!(is_default_emoji_presentation('😀'));
        // 'A' — does not
        assert!(!is_default_emoji_presentation('A'));
        // ❤ U+2764 — text presentation by default (needs VS16 for emoji)
        assert!(!is_default_emoji_presentation('❤'));
        // ❤️ components: U+2764 is text, but 🔥 U+1F525 has Emoji_Presentation
        assert!(is_default_emoji_presentation('🔥'));
        // # U+0023 — does not (even though it can be emoji with VS16)
        assert!(!is_default_emoji_presentation('#'));
    }
}
