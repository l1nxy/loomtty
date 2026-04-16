//! CJK font size scaling for CoreText (macOS).

use core_text::font::CTFont;
use core_text::font_descriptor::kCTFontOrientationDefault;

/// Compute the CJK font pixel size using em-normalized ic_width matching (ghostty approach).
///
/// Measures "水" (U+6C34) advance in both fonts, normalizes to em units, and scales
/// the CJK pixel size so its ic_width proportion matches the primary font's.
/// If the primary font lacks "水", falls back to 2 * cell_width as estimate.
pub(crate) fn compute_cjk_pixel_size_ct(
    pixel_size: f32,
    cell_width: f32,
    primary_font: &CTFont,
    cjk_font: Option<&CTFont>,
) -> f32 {
    let Some(cjk) = cjk_font else {
        return pixel_size;
    };

    // Measure "水" advance in CJK font
    let cjk_ic = match measure_char_advance(cjk, '水') {
        Some(w) if w > 0.0 => {
            log::info!("CJK sizing: '水' advance in CJK font = {w:.4}");
            w
        }
        other => {
            log::info!(
                "CJK sizing: '水' not measurable in CJK font (got {:?}), skipping adjustment",
                other
            );
            return pixel_size;
        }
    };

    // Get points-per-em for both fonts (CTFont size in points = ppem)
    let cjk_ppem = cjk.pt_size();

    // Compute primary font's ic_width (or estimate)
    let primary_ic = measure_char_advance(primary_font, '水').unwrap_or_else(|| {
        // Primary font lacks "水" — estimate from cell_width
        let est = (cell_width as f64 * 2.0).min(pixel_size as f64 * 1.5);
        log::info!("CJK sizing: primary font lacks '水', estimating advance = {est:.4}");
        est
    });
    if measure_char_advance(primary_font, '水').is_some() {
        log::info!("CJK sizing: '水' advance in primary font = {primary_ic:.4}");
    }
    let primary_ppem = primary_font.pt_size();

    // Normalize to em units and compute scale factor
    let primary_em = primary_ic / primary_ppem;
    let cjk_em = cjk_ic / cjk_ppem;

    if cjk_em <= 0.0 {
        return pixel_size;
    }

    let scale = primary_em / cjk_em;
    let adjusted = pixel_size as f64 * scale;

    log::info!(
        "CJK size adjustment (CoreText): primary_ic={primary_ic:.2} ({primary_em:.3}em) \
         cjk_ic={cjk_ic:.2} ({cjk_em:.3}em) scale={scale:.3} \
         px={pixel_size:.1}→{adjusted:.1}",
    );

    adjusted as f32
}

/// Measure the horizontal advance of a single character in the given CoreText font.
fn measure_char_advance(font: &CTFont, ch: char) -> Option<f64> {
    let mut buf = [0u16; 2];
    let utf16 = ch.encode_utf16(&mut buf);
    let mut glyphs = vec![0u16; utf16.len()];
    let success = unsafe {
        font.get_glyphs_for_characters(utf16.as_ptr(), glyphs.as_mut_ptr(), utf16.len() as isize)
    };
    if !success || glyphs[0] == 0 {
        return None;
    }

    let mut advances = [core_graphics::geometry::CGSize::new(0.0, 0.0)];
    let total = unsafe {
        font.get_advances_for_glyphs(
            kCTFontOrientationDefault,
            glyphs.as_ptr(),
            advances.as_mut_ptr(),
            1,
        )
    };
    if total > 0.0 { Some(total) } else { None }
}
