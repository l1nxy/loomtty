//! CJK font size scaling and character measurement.

use freetype::face::LoadFlag;

/// Compute the CJK font pixel size using em-normalized ic_width matching (ghostty approach).
///
/// Measures "水" (U+6C34) advance in both fonts, normalizes to em units, and scales
/// the CJK pixel size so its ic_width proportion matches the primary font's.
/// If the primary font lacks "水", falls back to min(asciiHeight, 2*cell_width) as estimate.
pub(crate) fn compute_cjk_pixel_size(
    ft_pixel_size: f32,
    cell_width: f32,
    primary_face: Option<&freetype::Face>,
    cjk_face: Option<&freetype::Face>,
) -> f32 {
    let Some(cjk) = cjk_face else {
        return ft_pixel_size;
    };

    // Initialize both faces at the base pixel size for measurement.
    let px = ft_pixel_size;
    let init = |face: &freetype::Face| -> bool {
        face.set_char_size(0, (px * 64.0) as isize, 72, 72).is_ok()
    };

    if !init(cjk) {
        return ft_pixel_size;
    }

    // Measure "水" advance in CJK font — if we can't, no adjustment possible.
    let cjk_ic = match measure_char_advance(cjk, '水') {
        Some(w) if w > 0.0 => w,
        _ => return ft_pixel_size,
    };

    // Get px_per_em for both faces (= y_ppem from size_metrics).
    let cjk_ppem = cjk
        .size_metrics()
        .map(|m| m.y_ppem as f64)
        .unwrap_or(px as f64);

    // Compute primary font's ic_width (or estimate).
    let (primary_ic, primary_ppem) = if let Some(primary) = primary_face {
        if !init(primary) {
            return ft_pixel_size;
        }
        let ppem = primary
            .size_metrics()
            .map(|m| m.y_ppem as f64)
            .unwrap_or(px as f64);
        let ic = measure_char_advance(primary, '水').unwrap_or_else(|| {
            // Primary font lacks "水" — estimate ic_width from font metrics.
            // Prefer capHeight-based estimate (1.5 * capHeight), which is more stable
            // than asciiHeight (inflated by tall symbols like $, @, {).
            // Fall back to asciiHeight if capHeight unavailable.
            let ic_estimate = if let Some(ch) = estimate_cap_height(primary) {
                1.5 * ch
            } else if let Some(ah) = estimate_ascii_height(primary) {
                ah
            } else {
                cell_width as f64 * 2.0
            };
            ic_estimate.min(cell_width as f64 * 2.0)
        });
        (ic, ppem)
    } else {
        let ic = (cell_width as f64 * 2.0).min(px as f64 * 1.5);
        (ic, px as f64)
    };

    // Normalize to em units and compute scale factor.
    let primary_em = primary_ic / primary_ppem;
    let cjk_em = cjk_ic / cjk_ppem;

    if cjk_em <= 0.0 {
        return ft_pixel_size;
    }

    let scale = primary_em / cjk_em;
    let adjusted = ft_pixel_size as f64 * scale;

    log::info!(
        "CJK size adjustment: primary_ic={primary_ic:.2} ({primary_em:.3}em) \
         cjk_ic={cjk_ic:.2} ({cjk_em:.3}em) scale={scale:.3} \
         px={ft_pixel_size:.1}→{adjusted:.1}",
    );

    adjusted as f32
}

/// Measure the horizontal advance of a single character in the given face.
/// The face must already have set_char_size called.
fn measure_char_advance(face: &freetype::Face, ch: char) -> Option<f64> {
    let glyph_index = face.get_char_index(ch as usize)?;
    face.load_glyph(glyph_index, LoadFlag::DEFAULT).ok()?;
    let adv = unsafe { (*face.raw().glyph).advance.x as f64 / 64.0 };
    if adv > 0.0 { Some(adv) } else { None }
}

/// Estimate cap height by measuring the 'H' glyph outline bbox height.
fn estimate_cap_height(face: &freetype::Face) -> Option<f64> {
    let gi = face.get_char_index('H' as usize)?;
    face.load_glyph(gi, LoadFlag::DEFAULT | LoadFlag::NO_BITMAP)
        .ok()?;
    let slot = unsafe { &*face.raw().glyph };
    if slot.format == freetype::ffi::FT_GLYPH_FORMAT_OUTLINE {
        let mut bbox = freetype::ffi::FT_BBox {
            xMin: 0,
            yMin: 0,
            xMax: 0,
            yMax: 0,
        };
        unsafe {
            freetype::ffi::FT_Outline_Get_BBox(
                &slot.outline as *const _ as *mut _,
                &mut bbox,
            );
        }
        let height = (bbox.yMax - bbox.yMin) as f64 / 64.0;
        if height > 0.0 {
            return Some(height);
        }
    }
    None
}

/// Estimate the bounding box height of printable ASCII characters using outline bbox.
/// Uses FT_Outline_Get_BBox for precision (matching ghostty's getGlyphSize approach),
/// rather than hinted glyph metrics which can overestimate.
fn estimate_ascii_height(face: &freetype::Face) -> Option<f64> {
    let mut top: f64 = 0.0;
    let mut bottom: f64 = 0.0;
    let mut any = false;
    for ch in 0x20u32..=0x7Eu32 {
        if let Some(gi) = face.get_char_index(ch as usize) {
            if face.load_glyph(gi, LoadFlag::DEFAULT | LoadFlag::NO_BITMAP)
                .is_ok()
            {
                let slot = unsafe { &*face.raw().glyph };
                if slot.format == freetype::ffi::FT_GLYPH_FORMAT_OUTLINE {
                    let mut bbox = freetype::ffi::FT_BBox {
                        xMin: 0,
                        yMin: 0,
                        xMax: 0,
                        yMax: 0,
                    };
                    unsafe {
                        freetype::ffi::FT_Outline_Get_BBox(
                            &slot.outline as *const _ as *mut _,
                            &mut bbox,
                        );
                    }
                    let y_min = bbox.yMin as f64 / 64.0;
                    let y_max = bbox.yMax as f64 / 64.0;
                    top = top.max(y_max);
                    bottom = bottom.min(y_min);
                    any = true;
                } else {
                    // Bitmap glyph: use metrics as fallback
                    let metrics = slot.metrics;
                    let g_top = metrics.horiBearingY as f64 / 64.0;
                    let g_bottom = g_top - metrics.height as f64 / 64.0;
                    top = top.max(g_top);
                    bottom = bottom.min(g_bottom);
                    any = true;
                }
            }
        }
    }
    if any && top > bottom {
        Some(top - bottom)
    } else {
        None
    }
}
