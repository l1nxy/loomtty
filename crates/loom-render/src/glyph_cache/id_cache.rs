//! A bounded front cache for shaped glyphs. Terminal rows usually repeat a
//! small alphabet; those hits need a key comparison, not a general hash lookup.

use std::collections::HashMap;

use super::types::{GlyphEntry, GlyphIdKey};

pub(super) struct GlyphIdCache {
    entries: HashMap<GlyphIdKey, GlyphEntry>,
    recent: Box<[Option<(GlyphIdKey, GlyphEntry)>; 256]>,
}

impl GlyphIdCache {
    pub(super) fn new() -> Self {
        Self {
            entries: HashMap::new(),
            recent: Box::new([None; 256]),
        }
    }

    #[inline]
    pub(super) fn get(&mut self, key: &GlyphIdKey) -> Option<&GlyphEntry> {
        let slot = &mut self.recent[key.recent_index()];
        if slot.as_ref().is_none_or(|(cached, _)| cached != key) {
            *slot = Some((*key, *self.entries.get(key)?));
        }
        slot.as_ref().map(|(_, entry)| entry)
    }

    pub(super) fn insert(&mut self, key: GlyphIdKey, entry: GlyphEntry) {
        self.entries.insert(key, entry);
        self.recent[key.recent_index()] = Some((key, entry));
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.recent.fill(None);
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.recent.iter().all(Option::is_none)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glyph_cache::types::{FontClass, FontStyle};

    #[test]
    fn collisions_preserve_every_glyph_face_style_and_width() {
        let mut cache = GlyphIdCache::new();
        let mut keys = Vec::new();
        for glyph in [0, 1, 257, 65535, u32::MAX] {
            for face in [
                FontClass::Primary,
                FontClass::Emoji,
                FontClass::Cjk,
                FontClass::Ui,
            ] {
                for style in [
                    FontStyle::Regular,
                    FontStyle::Bold,
                    FontStyle::Italic,
                    FontStyle::BoldItalic,
                ] {
                    for wide in [false, true] {
                        let key = GlyphIdKey::new(glyph, face, style, wide);
                        let entry = GlyphEntry {
                            width: keys.len() as u16,
                            ..GlyphEntry::EMPTY
                        };
                        keys.push(key);
                        cache.insert(key, entry);
                    }
                }
            }
        }
        for _ in 0..2 {
            for (i, key) in keys.iter().enumerate() {
                assert_eq!(cache.get(key).unwrap().width, i as u16);
                assert_eq!(cache.get(key).unwrap().width, i as u16);
            }
        }
        // Atlas eviction must invalidate both tiers, including transparent glyphs.
        cache.clear();
        assert!(cache.is_empty());
        for key in &keys {
            assert!(cache.get(key).is_none());
        }
        let key = keys[0];
        cache.insert(
            key,
            GlyphEntry {
                width: 42,
                ..GlyphEntry::EMPTY
            },
        );
        assert_eq!(cache.get(&key).unwrap().width, 42);
        cache.insert(key, GlyphEntry::EMPTY);
        assert_eq!(cache.get(&key).unwrap().width, 0);
    }
}
