//! Row-level text shaping: ligature detection, grapheme clustering, single-char shaping.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::glyph_cache::FontStyle;
use crate::shaper::{FaceSet, TextShaper};

use super::cell::CellProps;
use super::view::ViewBuildParams;

/// Pre-computed ligature info for a single row.
#[derive(Clone)]
pub(super) struct RowLigatureData {
    /// True for columns that are continuations of a ligature (should skip normal rendering).
    pub(super) skip_cols: Vec<bool>,
    /// Ligature glyphs to render: (col, glyph_id, font_id, style, fg_color).
    pub(super) ligature_glyphs: Vec<(usize, u32, fontdb::ID, FontStyle, [f32; 4])>,
    /// Pre-shaped grapheme clusters: (col, glyph_id, font_id, display_cols).
    pub(super) grapheme_glyphs: Vec<(usize, u32, fontdb::ID, usize)>,
    /// Per-char shaped glyph IDs for all-through-shaping path: (col, glyph_id, font_id, is_wide).
    pub(super) char_glyphs: Vec<(usize, u32, fontdb::ID, bool)>,
}

/// Pre-compute all ligature/grapheme shaping data for a single row.
/// Uses a pre-created [`FaceSet`] to avoid per-call Face::from_slice overhead.
pub(super) fn precompute_row_shaping(
    params: &ViewBuildParams<'_>,
    row: usize,
    faces: &FaceSet<'_>,
) -> RowLigatureData {
    struct LigatureRun<'a> {
        start: Option<usize>,
        text: &'a str,
        style: FontStyle,
        fg: [f32; 4],
    }

    fn flush_ligature_run(
        shaper: &TextShaper,
        faces: &FaceSet<'_>,
        cols_usize: usize,
        run: &LigatureRun<'_>,
        skip_cols: &mut [bool],
        ligature_glyphs: &mut Vec<(usize, u32, fontdb::ID, FontStyle, [f32; 4])>,
    ) {
        if let Some(start) = run.start
            && run.text.len() >= 2
        {
            for lig in shaper.detect_ligatures_with_face(run.text, faces.primary, faces.primary_id)
            {
                for k in 1..lig.char_count {
                    let c = start + lig.start_col + k;
                    if c < cols_usize {
                        skip_cols[c] = true;
                    }
                }
                ligature_glyphs.push((
                    start + lig.start_col,
                    lig.glyph_id,
                    lig.font_id,
                    run.style,
                    run.fg,
                ));
            }
        }
    }

    let cols_usize = params.grid.cols as usize;
    let mut skip_cols = vec![false; cols_usize];
    let mut ligature_glyphs = Vec::new();
    let mut grapheme_glyphs = Vec::new();

    // ── Detect ligatures via text shaping ──
    let mut run_start = None;
    let mut run_text = String::new();
    let mut run_style = FontStyle::Regular;
    let mut run_fg = [1.0f32; 4];

    for col in 0..=cols_usize {
        let cell_info = if col < cols_usize {
            let idx = row * cols_usize + col;
            if idx < params.grid.cells.len() {
                CellProps::from_packed_cell_fast(&params.grid.cells[idx], params.grid.colors)
                    .filter(|p| !p.is_hidden && p.ch != ' ' && p.ch != '\0' && !p.ch.is_control())
            } else {
                None
            }
        } else {
            None
        };

        if let Some(props) = &cell_info {
            if run_start.is_some() && props.style == run_style {
                run_text.push(props.ch);
                continue;
            }
            flush_ligature_run(
                params.shaper,
                faces,
                cols_usize,
                &LigatureRun {
                    start: run_start,
                    text: &run_text,
                    style: run_style,
                    fg: run_fg,
                },
                &mut skip_cols,
                &mut ligature_glyphs,
            );
            run_start = Some(col);
            run_text.clear();
            run_text.push(props.ch);
            run_style = props.style;
            run_fg = props.fg;
        } else {
            flush_ligature_run(
                params.shaper,
                faces,
                cols_usize,
                &LigatureRun {
                    start: run_start,
                    text: &run_text,
                    style: run_style,
                    fg: run_fg,
                },
                &mut skip_cols,
                &mut ligature_glyphs,
            );
            run_start = None;
            run_text.clear();
        }
    }

    // ── Detect grapheme clusters ──
    for col in 0..cols_usize {
        let idx = row * cols_usize + col;
        if idx >= params.grid.cells.len() {
            break;
        }
        let Some(props) =
            CellProps::from_packed_cell_fast(&params.grid.cells[idx], params.grid.colors)
        else {
            continue;
        };
        if props.is_hidden || props.ch == ' ' || props.ch == '\0' || props.ch.is_control() {
            continue;
        }
        if skip_cols[col] {
            continue;
        }

        // Check if the grapheme extras map has multi-codepoint data for this cell
        let (cluster_str, consumed_cols) =
            if let Some(full_grapheme) = params.grapheme_map.get(&(idx as u32)) {
                (full_grapheme.clone(), 0usize)
            } else if is_regional_indicator(props.ch) {
                // Regional Indicator: pair with the next cell if it's also an RI
                let mut s = String::from(props.ch);
                let next_col = col + 1;
                if next_col < cols_usize {
                    let li = row * cols_usize + next_col;
                    if li < params.grid.cells.len() {
                        let next_ch = params.grid.cells[li].ch();
                        if is_regional_indicator(next_ch) {
                            s.push(next_ch);
                        }
                    }
                }
                let consumed = s.chars().count() - 1;
                (s, consumed)
            } else {
                // Look ahead for combining/modifier characters in adjacent cells
                let mut s = String::from(props.ch);
                let mut look = col + if props.is_wide { 2 } else { 1 };
                let mut consumed = 0usize;
                while look < cols_usize {
                    let li = row * cols_usize + look;
                    if li >= params.grid.cells.len() {
                        break;
                    }
                    let next_ch = params.grid.cells[li].ch();
                    if is_combining_or_modifier(next_ch) {
                        s.push(next_ch);
                        consumed += 1;
                        look += 1;
                    } else {
                        break;
                    }
                }
                (s, consumed)
            };

        if cluster_str.graphemes(true).count() == 1
            && cluster_str.chars().count() > 1
            && let Some((gid, fid)) = params
                .shaper
                .shape_grapheme_with_fallback(&cluster_str, faces)
        {
            grapheme_glyphs.push((
                col,
                gid,
                fid,
                grapheme_display_cols(&cluster_str, props.is_wide),
            ));
            // Mark consumed cells so they aren't rendered independently
            let start = col + if props.is_wide { 2 } else { 1 };
            for k in 0..consumed_cols {
                let c = start + k;
                if c < cols_usize {
                    skip_cols[c] = true;
                }
            }
        }
    }

    // ── Single-char shaping for all remaining characters ──
    let mut char_glyphs = Vec::new();
    for (col, should_skip) in skip_cols.iter().enumerate().take(cols_usize) {
        if *should_skip {
            continue;
        }
        // Skip columns already handled by grapheme shaping
        if grapheme_glyphs
            .binary_search_by_key(&col, |(c, _, _, _)| *c)
            .is_ok()
        {
            continue;
        }
        // Skip columns handled by ligatures
        if ligature_glyphs.iter().any(|(c, _, _, _, _)| *c == col) {
            continue;
        }
        let idx = row * cols_usize + col;
        if idx >= params.grid.cells.len() {
            break;
        }
        let Some(props) =
            CellProps::from_packed_cell_fast(&params.grid.cells[idx], params.grid.colors)
        else {
            continue;
        };
        if props.is_hidden || props.ch == ' ' || props.ch == '\0' || props.ch.is_control() {
            continue;
        }
        if let Some((gid, fid)) = params.shaper.shape_char_with_fallback(props.ch, faces) {
            char_glyphs.push((col, gid, fid, props.is_wide));
        }
    }

    RowLigatureData {
        skip_cols,
        ligature_glyphs,
        grapheme_glyphs,
        char_glyphs,
    }
}

// ─── Unicode helpers ─────────────────────────────────────────────────

/// Check if a character is a Unicode combining character, ZWJ, or variation selector.
/// Regional Indicator Symbols are NOT included — they are base characters that pair
/// only with other regional indicators (handled separately in grapheme detection).
fn is_combining_or_modifier(c: char) -> bool {
    matches!(c,
        '\u{200D}'          // Zero Width Joiner
        | '\u{FE0E}'..='\u{FE0F}'  // Variation Selectors
        | '\u{0300}'..='\u{036F}'   // Combining Diacritical Marks
        | '\u{20D0}'..='\u{20FF}'   // Combining Marks for Symbols
        | '\u{1AB0}'..='\u{1AFF}'   // Combining Diacritical Marks Extended
        | '\u{1DC0}'..='\u{1DFF}'   // Combining Diacritical Marks Supplement
        | '\u{FE20}'..='\u{FE2F}'   // Combining Half Marks
        | '\u{E0100}'..='\u{E01EF}' // Variation Selectors Supplement
        | '\u{1F3FB}'..='\u{1F3FF}' // Emoji skin tone modifiers
    )
}

/// Regional Indicator Symbols: U+1F1E6 ('🇦') to U+1F1FF ('🇿').
/// Two adjacent RIs form a single flag emoji grapheme cluster.
fn is_regional_indicator(c: char) -> bool {
    ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)
}

fn grapheme_display_cols(cluster: &str, fallback_wide: bool) -> usize {
    UnicodeWidthStr::width(cluster)
        .max(if fallback_wide { 2 } else { 1 })
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::grapheme_display_cols;

    #[test]
    fn grapheme_display_cols_keeps_emoji_clusters_wide() {
        assert_eq!(grapheme_display_cols("🇺🇸", false), 2);
        assert_eq!(grapheme_display_cols("👨‍👩‍👧‍👦", false), 2);
        assert_eq!(grapheme_display_cols("e\u{301}", false), 1);
    }

    #[test]
    fn grapheme_display_cols_respects_fallback_wide_flag() {
        assert_eq!(grapheme_display_cols("A", true), 2);
    }
}
