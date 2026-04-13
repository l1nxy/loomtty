//! Font metrics computation from CoreText fonts (macOS).

use core_text::font::CTFont;
use core_text::font_descriptor::kCTFontOrientationDefault;

/// Compute cell metrics from a CoreText font.
/// Returns (cell_width, cell_height, baseline_from_top, face_width_unrounded).
///
/// Uses the ghostty approach: round() instead of ceil(), vertically center the
/// face in the cell by splitting the line gap evenly, and measure all printable
/// ASCII to get the true max advance for cell_width.
pub(crate) fn compute_ct_metrics(font: &CTFont) -> (f32, f32, f32, f32) {
    // CoreText provides ascent/descent/leading directly in points (at the current size)
    let ascent = font.ascent();
    let descent = font.descent(); // CoreText returns positive descent
    let leading = font.leading();

    log::info!(
        "CT metrics: raw CTFont ascent={ascent:.4} descent={descent:.4} leading={leading:.4} \
         units_per_em={} pt_size={:.1}",
        font.units_per_em(),
        font.pt_size(),
    );

    // Face height with line gap
    let face_height = ascent + descent + leading;
    let cell_height = face_height.round().max(1.0) as f32;

    // Baseline calculation with vertical centering (ghostty approach):
    // Split the line gap evenly between top and bottom.
    let half_leading = leading / 2.0;
    let face_baseline_from_bottom = half_leading + descent; // descent is positive in CT
    let cell_baseline_from_bottom =
        (face_baseline_from_bottom - (cell_height as f64 - face_height) / 2.0).round();
    let baseline_from_top = cell_height - cell_baseline_from_bottom as f32;

    // Measure cell_width from max advance across all printable ASCII
    let face_width = measure_max_ascii_advance(font);
    let cell_width = face_width.round().max(1.0) as f32;

    log::info!(
        "font metrics (CoreText): ascent={ascent:.2} descent={descent:.2} leading={leading:.2} \
         face_h={face_height:.2} face_w={face_width:.2} cw={cell_width:.1} ch={cell_height:.1} \
         baseline={baseline_from_top:.1}",
    );

    (
        cell_width,
        cell_height,
        baseline_from_top,
        face_width as f32,
    )
}

/// Measure the maximum horizontal advance across all printable ASCII characters.
/// Falls back to a reasonable default if no glyphs can be measured.
fn measure_max_ascii_advance(font: &CTFont) -> f64 {
    let mut max_advance: f64 = 0.0;

    for ch_code in 0x20u16..=0x7Eu16 {
        let characters = [ch_code];
        let mut glyphs = [0u16; 1];
        let found = unsafe {
            font.get_glyphs_for_characters(characters.as_ptr(), glyphs.as_mut_ptr(), 1)
        };
        if found && glyphs[0] != 0 {
            let mut advances = [core_graphics::geometry::CGSize::new(0.0, 0.0)];
            let total = unsafe {
                font.get_advances_for_glyphs(
                    kCTFontOrientationDefault,
                    glyphs.as_ptr(),
                    advances.as_mut_ptr(),
                    1,
                )
            };
            // total is the sum of advances; for a single glyph it equals the advance
            if total > max_advance {
                max_advance = total;
            }
        }
    }

    if max_advance > 0.0 {
        log::info!(
            "CT metrics: max ASCII advance = {max_advance:.4} (measured from printable ASCII)"
        );
        max_advance
    } else {
        let fallback = font.pt_size() * 0.6;
        log::info!(
            "CT metrics: no ASCII glyphs measured, fallback advance = {fallback:.4}"
        );
        fallback
    }
}
