//! CoreText-based text shaping for macOS.
//!
//! Provides glyph shaping via CTLine/CTRun, replacing rustybuzz on macOS.
//! Used by `TextShaper` when compiled for `target_os = "macos"`.

use core_foundation::attributed_string::CFMutableAttributedString;
use core_foundation::base::{CFRange, TCFType};
use core_foundation::string::CFString;
use core_text::font::CTFont;
use core_text::line::CTLine;
use core_text::string_attributes::kCTFontAttributeName;

use crate::shaper::Ligature;

/// Create a CTLine from text shaped with the given font.
///
/// This is the core shaping primitive: creates an attributed string with the
/// font attribute set, then lays it out via CTLine (which internally uses
/// CTTypesetter).
///
/// Note: No explicit LTR paragraph style is set because terminal text is
/// always in logical LTR order. The ligature detection sorts string_indices
/// to handle any unexpected RTL runs correctly.
fn create_ct_line(font: &CTFont, text: &str) -> CTLine {
    let cf_string = CFString::new(text);
    let mut attr_string = CFMutableAttributedString::new();
    attr_string.replace_str(&cf_string, CFRange::init(0, 0));
    let len = attr_string.char_len();
    unsafe {
        attr_string.set_attribute(CFRange::init(0, len), kCTFontAttributeName, font);
    }
    CTLine::new_with_attributed_string(attr_string.as_concrete_TypeRef())
}

/// Shape a single character and return its glyph ID.
///
/// Returns `None` if the font has no glyph for this character (glyph ID = 0).
pub(crate) fn ct_shape_char(font: &CTFont, ch: char) -> Option<u32> {
    let s = String::from(ch);
    let line = create_ct_line(font, &s);
    let runs = line.glyph_runs();

    // Search all runs for the first non-zero glyph
    for run in runs.iter() {
        let glyphs = run.glyphs();
        if let Some(&g) = glyphs.iter().find(|&&g| g != 0) {
            log::debug!(
                "CoreText shaping: ct_shape_char U+{:04X} '{}' -> glyph_id={} font={}",
                ch as u32,
                ch.escape_unicode(),
                g,
                font.display_name(),
            );
            return Some(g as u32);
        }
    }
    log::debug!(
        "CoreText shaping: ct_shape_char U+{:04X} '{}' -> no glyph (font={})",
        ch as u32,
        ch.escape_unicode(),
        font.display_name(),
    );
    None
}

/// Shape a grapheme cluster (multi-codepoint string) and return its glyph ID.
///
/// Returns `None` if the font doesn't actually combine the cluster into fewer
/// glyphs than input characters (i.e., no GSUB substitution happened), or if
/// all glyphs are zero (.notdef).
pub(crate) fn ct_shape_grapheme(font: &CTFont, cluster: &str) -> Option<u32> {
    let line = create_ct_line(font, cluster);
    let runs = line.glyph_runs();
    if runs.is_empty() {
        return None;
    }

    // Count total glyphs across all runs
    let mut total_glyphs: usize = 0;
    let mut first_nonzero_glyph: Option<u32> = None;
    for run in runs.iter() {
        let glyphs = run.glyphs();
        total_glyphs += glyphs.len();
        if first_nonzero_glyph.is_none()
            && let Some(&g) = glyphs.iter().find(|&&g| g != 0)
        {
            first_nonzero_glyph = Some(g as u32);
        }
    }

    // If shaping produced as many glyphs as input chars, the font didn't
    // actually combine them.
    let input_chars = cluster.chars().count();
    if input_chars > 1 && total_glyphs >= input_chars {
        log::debug!(
            "CoreText shaping: ct_shape_grapheme '{}' -> not combined ({} glyphs for {} chars, font={})",
            cluster.escape_default(),
            total_glyphs,
            input_chars,
            font.display_name(),
        );
        return None;
    }

    if let Some(gid) = first_nonzero_glyph {
        log::debug!(
            "CoreText shaping: ct_shape_grapheme '{}' -> glyph_id={} ({} glyphs from {} chars, font={})",
            cluster.escape_default(),
            gid,
            total_glyphs,
            input_chars,
            font.display_name(),
        );
    }

    first_nonzero_glyph
}

/// Detect ligatures in a text run using CoreText shaping.
///
/// Compares glyph string_indices to find cases where a single glyph covers
/// multiple input characters (ligatures like ff, fi, ffi, or programming
/// ligatures like ==, !=, =>).
pub(crate) fn ct_detect_ligatures(font: &CTFont, text: &str, font_id: fontdb::ID) -> Vec<Ligature> {
    if text.chars().count() < 2 {
        return Vec::new();
    }

    let line = create_ct_line(font, text);
    let runs = line.glyph_runs();

    // Build a mapping from UTF-16 offset to char index.
    // CoreText string_indices are in UTF-16 units.
    let char_count = text.chars().count();
    let utf16_len = text.encode_utf16().count();
    let utf16_to_char = build_utf16_to_char_map(text);

    let mut ligatures = Vec::new();

    log::debug!(
        "CoreText shaping: ct_detect_ligatures '{}' ({} chars, {} UTF-16 units)",
        text.escape_default(),
        char_count,
        utf16_len,
    );

    for run in runs.iter() {
        let glyph_count = run.glyph_count() as usize;
        if glyph_count == 0 {
            continue;
        }
        let glyphs = run.glyphs();
        let raw_indices = run.string_indices();

        // Sort (string_index, glyph_index) pairs by string_index for correct
        // span computation regardless of run direction (LTR or RTL).
        let mut pairs: Vec<(usize, usize)> = (0..glyph_count)
            .map(|i| (raw_indices[i] as usize, i))
            .collect();
        pairs.sort_unstable_by_key(|&(si, _)| si);

        for p in 0..pairs.len() {
            let (utf16_start, gi) = pairs[p];
            let glyph_id = glyphs[gi];
            if glyph_id == 0 {
                continue;
            }

            let utf16_end = if p + 1 < pairs.len() {
                pairs[p + 1].0
            } else {
                utf16_len
            };

            // Convert UTF-16 offsets to char indices
            let char_start = utf16_offset_to_char(&utf16_to_char, utf16_start);
            let char_end = if utf16_end <= utf16_to_char.len() {
                utf16_offset_to_char(&utf16_to_char, utf16_end)
            } else {
                char_count
            };

            let span = char_end.saturating_sub(char_start);
            if span > 1 {
                ligatures.push(Ligature {
                    start_col: char_start,
                    char_count: span,
                    glyph_id: glyph_id as u32,
                    font_id,
                });
            }
        }
    }

    if !ligatures.is_empty() {
        log::debug!(
            "CoreText shaping: detected {} ligature(s) in '{}'",
            ligatures.len(),
            text.escape_default(),
        );
        for lig in &ligatures {
            log::debug!(
                "CoreText shaping:   ligature col={}..{} glyph_id={}",
                lig.start_col,
                lig.start_col + lig.char_count,
                lig.glyph_id,
            );
        }
    }

    ligatures
}

/// CoreText analogue of `TextShaper::shape_run_with_face`. Returns one
/// `ShapedGlyph` per CTRun glyph, with `char_count` reflecting the cluster
/// span. `char_count == 0` for additional glyphs sharing a cluster with an
/// earlier glyph (multi-glyph ligatures).
pub(crate) fn ct_shape_run(
    font: &CTFont,
    text: &str,
    font_id: fontdb::ID,
) -> Vec<crate::shaper::ShapedGlyph> {
    use crate::shaper::ShapedGlyph;
    if text.is_empty() {
        return Vec::new();
    }
    let line = create_ct_line(font, text);
    let runs = line.glyph_runs();
    let char_count = text.chars().count();
    let utf16_to_char = build_utf16_to_char_map(text);
    let utf16_len = text.encode_utf16().count();

    let mut out = Vec::new();
    for run in runs.iter() {
        let glyph_count = run.glyph_count() as usize;
        if glyph_count == 0 {
            continue;
        }
        let glyphs = run.glyphs();
        let raw_indices = run.string_indices();

        let mut pairs: Vec<(usize, usize)> = (0..glyph_count)
            .map(|i| (raw_indices[i] as usize, i))
            .collect();
        pairs.sort_unstable_by_key(|&(si, _)| si);

        // Collapse multi-glyph clusters to their first glyph (mirrors the
        // rustybuzz path); skip .notdef.
        let mut last_utf16: Option<usize> = None;
        for p in 0..pairs.len() {
            let (utf16_start, gi) = pairs[p];
            let glyph_id = glyphs[gi];
            if glyph_id == 0 {
                continue;
            }
            if last_utf16 == Some(utf16_start) {
                continue;
            }
            last_utf16 = Some(utf16_start);
            let mut next = p + 1;
            while next < pairs.len() && pairs[next].0 == utf16_start {
                next += 1;
            }
            let next_utf16 = if next < pairs.len() {
                pairs[next].0
            } else {
                utf16_len
            };
            let start = utf16_offset_to_char(&utf16_to_char, utf16_start);
            let end = if next_utf16 <= utf16_to_char.len() {
                utf16_offset_to_char(&utf16_to_char, next_utf16)
            } else {
                char_count
            };
            out.push(ShapedGlyph {
                start_col: start,
                char_count: end.saturating_sub(start).max(1),
                glyph_id: glyph_id as u32,
                font_id,
            });
        }
    }
    out
}

/// Build a lookup table: for each UTF-16 code unit offset, what char index is it?
/// Returns a Vec where `result[utf16_offset]` = char_index.
fn build_utf16_to_char_map(text: &str) -> Vec<usize> {
    let utf16_len = text.encode_utf16().count();
    let mut map = Vec::with_capacity(utf16_len + 1);
    let mut char_idx = 0;
    for ch in text.chars() {
        let units = ch.len_utf16();
        for _ in 0..units {
            map.push(char_idx);
        }
        char_idx += 1;
    }
    // Sentinel for end-of-string
    map.push(char_idx);
    map
}

/// Convert a UTF-16 offset to a char index using the precomputed map.
fn utf16_offset_to_char(map: &[usize], utf16_offset: usize) -> usize {
    if utf16_offset < map.len() {
        map[utf16_offset]
    } else {
        // Past end — return total char count (last entry in map)
        *map.last().unwrap_or(&0)
    }
}
