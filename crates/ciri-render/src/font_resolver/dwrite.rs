//! DirectWrite-based font resolver for Windows.
//!
//! Delegates to `IDWriteFontFallback::MapCharacters()` for system-native
//! font matching, identical to how Windows Terminal resolves fonts.
//!
//! TODO: implement in Phase 2. Currently falls through to CmapResolver.

use super::{FontResolver, ResolvedFont};

/// Font resolver using DirectWrite's `IDWriteFontFallback`.
///
/// Not yet implemented — currently delegates to `CmapResolver`.
pub struct DWriteResolver {
    inner: super::CmapResolver,
}

impl DWriteResolver {
    /// Create a DWrite resolver.  Currently wraps `CmapResolver`.
    pub fn new(inner: super::CmapResolver) -> Self {
        DWriteResolver { inner }
    }
}

impl FontResolver for DWriteResolver {
    fn resolve_char(&self, ch: char) -> ResolvedFont {
        // TODO: use IDWriteFontFallback::MapCharacters
        self.inner.resolve_char(ch)
    }
}
