//! Font metrics computation from FreeType faces.

use freetype::face::LoadFlag;
use freetype::tt_os2::TrueTypeOS2Table;

/// Compute cell metrics directly from FreeType face, reading OS/2 and hhea
/// tables for accurate ascent/descent/line_gap. Returns (cell_width, cell_height,
/// baseline_from_top, face_width_unrounded).
///
/// Uses the ghostty approach: round() instead of ceil(), vertically center the
/// face in the cell by splitting the line gap evenly, and measure all printable
/// ASCII to get the true max advance for cell_width.
pub(crate) fn compute_ft_metrics(
    face: &mut freetype::Face,
    pixel_size: f32,
) -> (f32, f32, f32, f32) {
    face.set_char_size(0, (pixel_size * 64.0) as isize, 72, 72)
        .expect("FreeType set_char_size failed");

    let size_metrics = face.size_metrics().expect("FreeType size_metrics unavailable");
    let px_per_unit = (size_metrics.y_scale as f64 / 65536.0) / 64.0;

    // Read vertical metrics from OS/2 table, falling back to hhea then size_metrics.
    let (ascent, descent, line_gap) = read_vertical_metrics(face, px_per_unit, &size_metrics);

    // Face height and cell height (ghostty approach: round, not ceil)
    let face_height = ascent - descent + line_gap;
    let cell_height = face_height.round().max(1.0) as f32;

    // Baseline with vertical centering: split line_gap evenly top/bottom,
    // then center the face in the (possibly rounded) cell.
    let half_line_gap = line_gap / 2.0;
    let face_baseline_from_bottom = half_line_gap - descent; // descent is negative
    let cell_baseline_from_bottom =
        (face_baseline_from_bottom - (cell_height as f64 - face_height) / 2.0).round();
    let baseline_from_top = cell_height - cell_baseline_from_bottom as f32;

    // Measure cell_width from max advance across all printable ASCII.
    let face_width = measure_max_ascii_advance(face, &size_metrics);
    let cell_width = face_width.round().max(1.0) as f32;

    log::info!(
        "font metrics (FreeType): ascent={ascent:.2} descent={descent:.2} line_gap={line_gap:.2} \
         face_h={face_height:.2} face_w={face_width:.2} cw={cell_width:.1} ch={cell_height:.1} \
         baseline={baseline_from_top:.1}",
    );

    (cell_width, cell_height, baseline_from_top, face_width as f32)
}

/// Read ascent, descent (negative), and line_gap from font tables.
/// Priority: OS/2 typo metrics (if USE_TYPO_METRICS set) → hhea → FreeType size_metrics.
fn read_vertical_metrics(
    face: &mut freetype::Face,
    px_per_unit: f64,
    size_metrics: &freetype::ffi::FT_Size_Metrics,
) -> (f64, f64, f64) {
    // Try OS/2 table first
    if let Some(os2) = TrueTypeOS2Table::from_face(face) {
        let typo_ascent = os2.s_typo_ascender() as f64 * px_per_unit;
        let typo_descent = os2.s_typo_descender() as f64 * px_per_unit;
        let typo_line_gap = os2.s_typo_line_gap() as f64 * px_per_unit;

        // If USE_TYPO_METRICS is set (bit 7 of fsSelection), trust OS/2 typo metrics.
        if os2.fs_selection() & (1 << 7) != 0 {
            return (typo_ascent, typo_descent, typo_line_gap);
        }

        // Otherwise, prefer hhea if available (non-zero), then fall back to OS/2 typo.
        let hhea_ascent = face.ascender() as f64 * px_per_unit;
        let hhea_descent = face.descender() as f64 * px_per_unit;
        if hhea_ascent != 0.0 || hhea_descent != 0.0 {
            let hhea_height = face.height() as f64 * px_per_unit;
            let hhea_line_gap = hhea_height - (hhea_ascent - hhea_descent);
            return (hhea_ascent, hhea_descent, hhea_line_gap.max(0.0));
        }

        // hhea empty, use OS/2 typo
        if typo_ascent != 0.0 || typo_descent != 0.0 {
            return (typo_ascent, typo_descent, typo_line_gap);
        }
    } else {
        // No OS/2 table — try hhea (face.ascender/descender are from hhea in design units)
        let hhea_ascent = face.ascender() as f64 * px_per_unit;
        let hhea_descent = face.descender() as f64 * px_per_unit;
        if hhea_ascent != 0.0 || hhea_descent != 0.0 {
            let hhea_height = face.height() as f64 * px_per_unit;
            let hhea_line_gap = hhea_height - (hhea_ascent - hhea_descent);
            return (hhea_ascent, hhea_descent, hhea_line_gap.max(0.0));
        }
    }

    // Final fallback: FreeType size_metrics (already in 26.6 fixed point pixels)
    let ascent = size_metrics.ascender as f64 / 64.0;
    let descent = size_metrics.descender as f64 / 64.0;
    (ascent, descent, 0.0)
}

/// Measure the maximum horizontal advance across all printable ASCII characters.
/// Falls back to FreeType's max_advance if no glyphs can be loaded.
fn measure_max_ascii_advance(
    face: &freetype::Face,
    size_metrics: &freetype::ffi::FT_Size_Metrics,
) -> f64 {
    let mut max_advance: f64 = 0.0;
    for ch in 0x20u32..=0x7Eu32 {
        if let Some(glyph_index) = face.get_char_index(ch as usize) {
            if face.load_glyph(glyph_index, LoadFlag::DEFAULT).is_ok() {
                let adv = unsafe { (*face.raw().glyph).advance.x as f64 / 64.0 };
                if adv > max_advance {
                    max_advance = adv;
                }
            }
        }
    }
    if max_advance > 0.0 {
        max_advance
    } else {
        // Fallback to FreeType's max_advance
        size_metrics.max_advance as f64 / 64.0
    }
}
