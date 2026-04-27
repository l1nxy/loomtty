//! Row-level text shaping: ligature detection, grapheme clustering, single-char shaping.

use ciri_config::schema::DisableLigatures;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::glyph_cache::FontStyle;
use crate::shaper::{FaceSet, TextShaper};

use super::cell::CellProps;
use super::view::ViewBuildParams;

/// Pre-computed ligature info for a single row.
#[derive(Clone)]
pub(crate) struct RowLigatureData {
    /// True for columns that are continuations of a ligature (should skip normal rendering).
    pub(crate) skip_cols: Vec<bool>,
    /// Ligature glyphs to render: (col, glyph_id, font_id, style, fg_color).
    pub(crate) ligature_glyphs: Vec<(usize, u32, fontdb::ID, FontStyle, [f32; 4])>,
    /// Pre-shaped grapheme clusters: (col, glyph_id, font_id, display_cols).
    pub(crate) grapheme_glyphs: Vec<(usize, u32, fontdb::ID, usize)>,
    /// Per-char shaped glyph IDs for all-through-shaping path: (col, glyph_id, font_id, is_wide).
    pub(crate) char_glyphs: Vec<(usize, u32, fontdb::ID, bool)>,
}

impl RowLigatureData {
    fn with_row_capacity(cols: usize) -> Self {
        Self {
            skip_cols: vec![false; cols],
            ligature_glyphs: Vec::with_capacity((cols / 8).max(4)),
            grapheme_glyphs: Vec::with_capacity((cols / 8).max(4)),
            char_glyphs: Vec::with_capacity(cols),
        }
    }

    fn reset(&mut self, cols: usize) {
        if self.skip_cols.len() != cols {
            self.skip_cols = vec![false; cols];
        } else {
            self.skip_cols.fill(false);
        }
        self.ligature_glyphs.clear();
        self.grapheme_glyphs.clear();
        self.char_glyphs.clear();
    }
}

/// Pre-compute all ligature/grapheme shaping data for a single row.
/// Uses a pre-created [`FaceSet`] to avoid per-call Face::from_slice overhead.
pub(super) fn precompute_row_shaping(
    params: &ViewBuildParams<'_>,
    row: usize,
    faces: &FaceSet<'_>,
) -> RowLigatureData {
    let cols = params.grid.cols as usize;
    let mut data = RowLigatureData::with_row_capacity(cols);
    precompute_row_shaping_into(params, row, faces, &mut data);
    data
}

pub(super) fn precompute_row_shaping_into(
    params: &ViewBuildParams<'_>,
    row: usize,
    faces: &FaceSet<'_>,
    data: &mut RowLigatureData,
) {
    let ligatures_enabled = match params.config.font.disable_ligatures {
        DisableLigatures::Never => true,
        DisableLigatures::Always => false,
        DisableLigatures::Cursor => params.cursor_line < 0 || (row as i16) != params.cursor_line,
    };

    struct LigatureRun<'a> {
        start: Option<usize>,
        text: &'a str,
        char_count: usize,
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
        char_glyphs: &mut Vec<(usize, u32, fontdb::ID, bool)>,
    ) {
        let Some(start) = run.start else { return };
        if run.char_count == 0 {
            return;
        }
        // Always shape the run, even single-char runs — `calt`/`clig` may
        // substitute a single codepoint based on context (e.g. Cascadia
        // Code's `=>` flips earlier substitution decisions when the run
        // grows). For a 1-char run with no substitution this is cheap and
        // produces the same glyph the per-char fallback would.
        let shaped = shaper.shape_run_with_face(run.text, faces.primary, faces.primary_id);
        for sg in shaped {
            let abs_col = start + sg.start_col;
            if abs_col >= cols_usize {
                continue;
            }
            if sg.char_count >= 2 {
                // Real cluster-merging ligature: render the merged glyph at
                // the start column and skip continuation columns.
                for k in 1..sg.char_count {
                    let c = abs_col + k;
                    if c < cols_usize {
                        skip_cols[c] = true;
                    }
                }
                ligature_glyphs.push((abs_col, sg.glyph_id, sg.font_id, run.style, run.fg));
            } else {
                // Single-cluster glyph (possibly a contextual substitution
                // like Cascadia Code's `equal_equal.liga`/`LIG` pair). Route
                // through `char_glyphs` so the renderer uses the shaped
                // glyph_id rather than re-mapping the bare codepoint.
                // `char_count == 0` means it's a continuation of a multi-
                // glyph cluster — we still want to render it, attached to
                // the same column as the previous shaped glyph.
                char_glyphs.push((abs_col, sg.glyph_id, sg.font_id, false));
            }
        }
    }

    let cols_usize = params.grid.cols as usize;
    data.reset(cols_usize);

    // ── Detect ligatures via text shaping ──
    // When `disable_ligatures` is `always`, or `cursor` and we're on the
    // cursor row, skip the entire detection pass. The grapheme/single-char
    // pass below already copes with `ligature_glyphs` being empty.
    if ligatures_enabled {
    let mut run_start = None;
    let mut run_text = String::with_capacity(cols_usize);
    let mut run_char_count = 0usize;
    let mut run_style = FontStyle::Regular;
    let mut run_fg = [1.0f32; 4];

    for col in 0..=cols_usize {
        let cell_info = if col < cols_usize {
            let idx = row * cols_usize + col;
            if idx < params.grid.cells.len() {
                CellProps::from_packed_cell_fast(&params.grid.cells[idx], params.grid.colors)
                    .filter(|p| {
                        !p.is_hidden
                            && p.ch != ' '
                            && p.ch != '\0'
                            && !p.ch.is_control()
                            // Exclude wide cells (CJK / emoji): they span two
                            // columns, but `run_text` is built from chars
                            // alone. Mixing them in would break the
                            // `start + sg.start_col` column math used to
                            // place shaped glyphs back onto the grid.
                            // Wide cells are handled by the grapheme /
                            // single-char fallback below.
                            && !p.is_wide
                            // Exclude box-drawing / block-element cells:
                            // they don't go through the font at all (the
                            // render path emits geometric rects instead),
                            // so keeping them out of the run avoids both
                            // wasted shaping work and the renderer seeing
                            // a shaped glyph it must then suppress.
                            && !super::box_drawing::is_in_range(p.ch)
                    })
            } else {
                None
            }
        } else {
            None
        };

        if let Some(props) = &cell_info {
            if run_start.is_some() && props.style == run_style {
                run_text.push(props.ch);
                run_char_count += 1;
                continue;
            }
            flush_ligature_run(
                params.shaper,
                faces,
                cols_usize,
                &LigatureRun {
                    start: run_start,
                    text: &run_text,
                    char_count: run_char_count,
                    style: run_style,
                    fg: run_fg,
                },
                &mut data.skip_cols,
                &mut data.ligature_glyphs,
                &mut data.char_glyphs,
            );
            run_start = Some(col);
            run_text.clear();
            run_text.push(props.ch);
            run_char_count = 1;
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
                    char_count: run_char_count,
                    style: run_style,
                    fg: run_fg,
                },
                &mut data.skip_cols,
                &mut data.ligature_glyphs,
                &mut data.char_glyphs,
            );
            run_start = None;
            run_text.clear();
            run_char_count = 0;
        }
    }
    } // end `if ligatures_enabled`

    // Cells that the run-shape pass already produced a glyph for. Skip them
    // in the grapheme/single-char fallback below so we don't push a second
    // glyph for the same cell. (The run-shape glyph_id reflects contextual
    // substitution like Cascadia Code's `==`; per-char shaping would lose
    // it.) Sort first so binary_search is valid.
    data.char_glyphs.sort_by_key(|(col, ..)| *col);
    let mut covered_by_run: Vec<bool> = vec![false; cols_usize];
    for &(col, ..) in data.char_glyphs.iter() {
        if col < cols_usize {
            covered_by_run[col] = true;
        }
    }

    // ── Detect grapheme clusters + single-char shaping ──
    let mut ligature_idx = 0usize;
    for col in 0..cols_usize {
        while ligature_idx < data.ligature_glyphs.len()
            && data.ligature_glyphs[ligature_idx].0 < col
        {
            ligature_idx += 1;
        }
        if ligature_idx < data.ligature_glyphs.len() && data.ligature_glyphs[ligature_idx].0 == col
        {
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
        if data.skip_cols[col] {
            continue;
        }

        if let Some(full_grapheme) = params.grapheme_map.get(&(idx as u32))
            && push_shaped_grapheme(
                params.shaper,
                faces,
                full_grapheme,
                col,
                &props,
                0,
                cols_usize,
                &mut data.skip_cols,
                &mut data.grapheme_glyphs,
            )
        {
            continue;
        }

        if is_regional_indicator(props.ch) {
            let next_col = col + 1;
            if next_col < cols_usize {
                let li = row * cols_usize + next_col;
                if li < params.grid.cells.len() {
                    let next_ch = params.grid.cells[li].ch();
                    if is_regional_indicator(next_ch) {
                        let mut cluster =
                            String::with_capacity(props.ch.len_utf8() + next_ch.len_utf8());
                        cluster.push(props.ch);
                        cluster.push(next_ch);
                        if push_shaped_grapheme(
                            params.shaper,
                            faces,
                            &cluster,
                            col,
                            &props,
                            1,
                            cols_usize,
                            &mut data.skip_cols,
                            &mut data.grapheme_glyphs,
                        ) {
                            continue;
                        }
                    }
                }
            }
        }

        let mut look = col + if props.is_wide { 2 } else { 1 };
        if look < cols_usize {
            let li = row * cols_usize + look;
            if li < params.grid.cells.len() {
                let next_ch = params.grid.cells[li].ch();
                if is_combining_or_modifier(next_ch) {
                    let mut cluster = String::new();
                    cluster.push(props.ch);
                    cluster.push(next_ch);
                    look += 1;
                    let mut consumed = 1usize;
                    while look < cols_usize {
                        let li = row * cols_usize + look;
                        if li >= params.grid.cells.len() {
                            break;
                        }
                        let next_ch = params.grid.cells[li].ch();
                        if is_combining_or_modifier(next_ch) {
                            cluster.push(next_ch);
                            consumed += 1;
                            look += 1;
                        } else {
                            break;
                        }
                    }
                    if push_shaped_grapheme(
                        params.shaper,
                        faces,
                        &cluster,
                        col,
                        &props,
                        consumed,
                        cols_usize,
                        &mut data.skip_cols,
                        &mut data.grapheme_glyphs,
                    ) {
                        continue;
                    }
                }
            }
        }

        if covered_by_run[col] {
            // Run shaping already produced the right glyph for this cell;
            // don't add a duplicate per-char entry. The grapheme path above
            // is allowed to override it, since combined clusters
            // (e + ◌́, regional indicators, ZWJ) carry information the
            // run shape can't see.
            continue;
        }
        if let Some((gid, fid)) = params.shaper.shape_char_with_fallback(props.ch, faces) {
            data.char_glyphs.push((col, gid, fid, props.is_wide));
        }
    }
}

fn push_shaped_grapheme(
    shaper: &TextShaper,
    faces: &FaceSet<'_>,
    cluster: &str,
    col: usize,
    props: &CellProps,
    consumed_cols: usize,
    cols_usize: usize,
    skip_cols: &mut [bool],
    grapheme_glyphs: &mut Vec<(usize, u32, fontdb::ID, usize)>,
) -> bool {
    if cluster.chars().count() <= 1 || cluster.graphemes(true).count() != 1 {
        return false;
    }
    let Some((gid, fid)) = shaper.shape_grapheme_with_fallback(cluster, faces) else {
        return false;
    };
    grapheme_glyphs.push((col, gid, fid, grapheme_display_cols(cluster, props.is_wide)));
    let start = col + if props.is_wide { 2 } else { 1 };
    for k in 0..consumed_cols {
        let c = start + k;
        if c < cols_usize {
            skip_cols[c] = true;
        }
    }
    true
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
    let unicode_w = UnicodeWidthStr::width(cluster);
    let cols = unicode_w.max(if fallback_wide { 2 } else { 1 }).max(1);
    // Log non-ASCII multi-cell graphemes for width verification
    if cols >= 2 && !cluster.is_ascii() {
        log::debug!(
            "width calc: grapheme '{}' unicode_width={} fallback_wide={} -> cols={}",
            cluster.escape_default(),
            unicode_w,
            fallback_wide,
            cols,
        );
    }
    cols
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
