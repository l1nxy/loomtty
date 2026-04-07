//! Font resolution: determine which font face to use for each character.
//!
//! The resolver runs **before** text shaping to select the correct font
//! (primary, CJK, or emoji) for each codepoint.  Platform-specific
//! implementations can delegate to the OS font stack (e.g. DirectWrite
//! on Windows, fontconfig on Linux) while a generic `CmapResolver`
//! provides a portable baseline.

mod cmap;
#[cfg(windows)]
mod dwrite;

pub use cmap::CmapResolver;
#[cfg(windows)]
pub use dwrite::DWriteResolver;

/// Which font class a character should be rendered with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResolvedFont {
    Primary,
    Cjk,
    Emoji,
}

/// A contiguous run of text that shares the same resolved font.
#[derive(Debug, Clone)]
pub struct FontRun {
    /// Column start (inclusive).
    pub col_start: usize,
    /// Column end (exclusive).
    pub col_end: usize,
    /// Which font to use.
    pub font: ResolvedFont,
}

/// Resolve which font face to use for individual characters.
///
/// Implementations must be cheap for single-char lookups — the shaping
/// pipeline calls `resolve_char` per cell.
pub trait FontResolver {
    /// Determine the preferred font for a single character.
    fn resolve_char(&self, ch: char) -> ResolvedFont;
}
