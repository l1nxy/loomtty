//! Glyph rasterization and atlas caching.
//!
//! On non-Windows: FreeType glyph-ID rendering + crossfont character rendering.
//! On Windows: rasterization lives in `rasterize_dwrite.rs`.

#[cfg(target_os = "linux")]
use crossfont::BitmapBuffer;
#[cfg(target_os = "linux")]
use freetype::face::LoadFlag;

use super::atlas::{make_glyph_entry, AtlasRegion, PendingUpload};
#[cfg(target_os = "linux")]
use super::types::FontStyle;
use super::types::GlyphEntry;

/// Backend-agnostic rasterized glyph data.
pub(crate) struct RasterizedGlyph {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) bearing_x: f32,
    pub(crate) bearing_y: f32,
    pub(crate) is_color: bool,
    pub(crate) data: Vec<u8>,
}

#[cfg(target_os = "linux")]
/// Rasterize a glyph by ID using the thin FreeType path.
/// Tries color bitmap first (for emoji), then falls back to grayscale outline.
/// `pixel_size` controls the rendering size.
/// `constrained` disables hinting for glyphs that will be scaled/repositioned.
pub(crate) fn rasterize_glyph_id_ft(
    ft_face: Option<&freetype::Face>,
    glyph_id: u32,
    style: FontStyle,
    pixel_size: f32,
    constrained: bool,
    cell_height: f32,
) -> Option<RasterizedGlyph> {
    let ft_face = ft_face?;

    ft_face
        .set_char_size(0, (pixel_size * 64.0) as isize, 72, 72)
        .ok()?;

    // Try color bitmap first (CBDT/sbix emoji)
    let color_flags = LoadFlag::COLOR | LoadFlag::RENDER;
    if ft_face.load_glyph(glyph_id, color_flags).is_ok() {
        let glyph = ft_face.glyph();
        let bitmap = glyph.bitmap();
        let is_bgra = matches!(bitmap.pixel_mode(), Ok(freetype::bitmap::PixelMode::Bgra));
        if is_bgra && bitmap.width() > 0 && bitmap.rows() > 0 {
            let w = bitmap.width() as u32;
            let h = bitmap.rows() as u32;
            let pitch = bitmap.pitch().unsigned_abs();
            let raw = bitmap.buffer();
            // Convert BGRA → RGBA
            let mut data =
                Vec::with_capacity((w as usize).saturating_mul(h as usize).saturating_mul(4));
            for row in 0..h {
                let start = (row as usize).saturating_mul(pitch as usize);
                for x in 0..w as usize {
                    let offset = start.saturating_add(x.saturating_mul(4));
                    if offset + 3 < raw.len() {
                        data.push(raw[offset + 2]); // R
                        data.push(raw[offset + 1]); // G
                        data.push(raw[offset]); // B
                        data.push(raw[offset + 3]); // A
                    }
                }
            }
            // Scale color bitmap to cell size if needed
            let target_h = cell_height as u32;
            if h != target_h && target_h > 0 {
                let scale = target_h as f32 / h as f32;
                let new_w = (w as f32 * scale).round() as u32;
                let scaled = downsample_rgba(&data, w, h, new_w, target_h);
                return Some(RasterizedGlyph {
                    width: new_w,
                    height: target_h,
                    bearing_x: glyph.bitmap_left() as f32 * scale,
                    bearing_y: glyph.bitmap_top() as f32 * scale,
                    is_color: true,
                    data: scaled,
                });
            }
            return Some(RasterizedGlyph {
                width: w,
                height: h,
                bearing_x: glyph.bitmap_left() as f32,
                bearing_y: glyph.bitmap_top() as f32,
                is_color: true,
                data,
            });
        }
    }

    // Grayscale outline path — disable hinting for constrained (scaled) glyphs
    let load_flags = if constrained {
        LoadFlag::NO_HINTING
    } else {
        LoadFlag::TARGET_LIGHT
    };
    ft_face.load_glyph(glyph_id, load_flags).ok()?;
    let glyph = ft_face.glyph();

    // Synthetic bold/italic transformations
    let need_synth_bold = matches!(style, FontStyle::Bold | FontStyle::BoldItalic);
    let need_synth_italic = matches!(style, FontStyle::Italic | FontStyle::BoldItalic);
    unsafe {
        let slot = ft_face.raw().glyph;
        if (*slot).format == freetype::ffi::FT_GLYPH_FORMAT_OUTLINE {
            let outline = &mut (*slot).outline;
            if need_synth_bold {
                let font_height = (*ft_face.raw().size).metrics.height as f64;
                let amount = (font_height * 64.0 / 2048.0).ceil() as freetype::ffi::FT_Pos;
                freetype::ffi::FT_Outline_Embolden(outline, amount);
            }
            if need_synth_italic {
                let matrix = freetype::ffi::FT_Matrix {
                    xx: 0x10000 as freetype::ffi::FT_Fixed,
                    xy: (0.2125 * 65536.0) as freetype::ffi::FT_Fixed,
                    yx: 0 as freetype::ffi::FT_Fixed,
                    yy: 0x10000 as freetype::ffi::FT_Fixed,
                };
                freetype::ffi::FT_Outline_Transform(outline, &matrix);
            }
        }
    }

    glyph.render_glyph(freetype::RenderMode::Normal).ok()?;

    let bitmap = glyph.bitmap();
    let w = bitmap.width() as u32;
    let h = bitmap.rows() as u32;
    if w == 0 || h == 0 {
        return None;
    }

    let pitch = bitmap.pitch().unsigned_abs();
    let raw = bitmap.buffer();
    let mut data = Vec::with_capacity((w as usize).saturating_mul(h as usize));
    for row in 0..h {
        let start = (row as usize).saturating_mul(pitch as usize);
        let end = start + w as usize;
        if end <= raw.len() {
            data.extend_from_slice(&raw[start..end]);
        }
    }

    Some(RasterizedGlyph {
        width: w,
        height: h,
        bearing_x: glyph.bitmap_left() as f32,
        bearing_y: glyph.bitmap_top() as f32,
        is_color: false,
        data,
    })
}

#[cfg(target_os = "linux")]
/// Convert a crossfont `RasterizedGlyph` to our internal format.
///
/// - `BitmapBuffer::Rgb` → single-channel alpha: `(R + G + B) / 3`
/// - `BitmapBuffer::Rgba` → color emoji: keep as-is
pub(crate) fn convert_crossfont_glyph(glyph: crossfont::RasterizedGlyph) -> RasterizedGlyph {
    let w = glyph.width as u32;
    let h = glyph.height as u32;

    match glyph.buffer {
        BitmapBuffer::Rgb(rgb_data) => {
            // crossfont's RGB bitmap contains LCD coverage, not sRGB color.
            // Collapse the subpixel coverage back to a single-channel mask
            // without applying color-space transforms.
            let alpha_data: Vec<u8> = rgb_data
                .chunks(3)
                .map(|rgb| ((rgb[0] as u16 + rgb[1] as u16 + rgb[2] as u16) / 3) as u8)
                .collect();
            RasterizedGlyph {
                width: w,
                height: h,
                bearing_x: glyph.left as f32,
                bearing_y: glyph.top as f32,
                is_color: false,
                data: alpha_data,
            }
        }
        BitmapBuffer::Rgba(rgba_data) => RasterizedGlyph {
            width: w,
            height: h,
            bearing_x: glyph.left as f32,
            bearing_y: glyph.top as f32,
            is_color: true,
            data: rgba_data,
        },
    }
}

/// Allocate atlas space, queue pixel data for upload, return the entry.
pub(crate) fn cache_rasterized_glyph(
    glyph: RasterizedGlyph,
    alpha_packer: &mut super::atlas::ShelfPacker,
    color_packer: &mut super::atlas::ShelfPacker,
    alpha_pending: &mut Vec<PendingUpload>,
    color_pending: &mut Vec<PendingUpload>,
    atlas_size: u32,
    atlas_needs_clear: &mut bool,
) -> Option<GlyphEntry> {
    let is_color = glyph.is_color;
    let pos = if is_color {
        color_packer.allocate(glyph.width, glyph.height)
    } else {
        alpha_packer.allocate(glyph.width, glyph.height)
    };
    let (x, y) = match pos {
        Some(pos) => pos,
        None => {
            let atlas_name = if is_color { "color" } else { "alpha" };
            log::warn!("{atlas_name} atlas full, flagging for clear");
            *atlas_needs_clear = true;
            return None;
        }
    };
    let region = AtlasRegion {
        x,
        y,
        w: glyph.width,
        h: glyph.height,
    };
    let entry = make_glyph_entry(
        region,
        glyph.bearing_x,
        glyph.bearing_y,
        atlas_size,
        is_color,
    );
    let pending = PendingUpload {
        x: region.x,
        y: region.y,
        w: region.w,
        h: region.h,
        data: glyph.data,
    };
    if is_color {
        color_pending.push(pending);
    } else {
        alpha_pending.push(pending);
    }
    Some(entry)
}

/// Allocate atlas space for a DWrite-measured glyph and queue a D2D render command.
/// No pixel data — the DX backend renders via D2D DrawGlyphRun at flush time.
#[cfg(windows)]
pub(crate) fn cache_measured_dwrite_glyph(
    measured: &super::rasterize_dwrite::MeasuredGlyph,
    face: &windows::Win32::Graphics::DirectWrite::IDWriteFontFace,
    glyph_index: u16,
    pixel_size: f32,
    alpha_packer: &mut super::atlas::ShelfPacker,
    color_packer: &mut super::atlas::ShelfPacker,
    dwrite_alpha_pending: &mut Vec<super::atlas::PendingDwriteGlyph>,
    dwrite_color_pending: &mut Vec<super::atlas::PendingDwriteGlyph>,
    atlas_size: u32,
    atlas_needs_clear: &mut bool,
) -> Option<GlyphEntry> {
    let is_color = measured.is_color;
    let pos = if is_color {
        color_packer.allocate(measured.width, measured.height)
    } else {
        alpha_packer.allocate(measured.width, measured.height)
    };
    let (x, y) = match pos {
        Some(pos) => pos,
        None => {
            let atlas_name = if is_color { "color" } else { "alpha" };
            log::warn!("{atlas_name} atlas full, flagging for clear");
            *atlas_needs_clear = true;
            return None;
        }
    };
    let region = AtlasRegion {
        x,
        y,
        w: measured.width,
        h: measured.height,
    };
    let entry = make_glyph_entry(
        region,
        measured.bearing_x,
        measured.bearing_y,
        atlas_size,
        is_color,
    );
    // baseline origin: offset so the glyph renders at the correct atlas position.
    // bearing_x = bounds.left, bearing_y = -bounds.top
    let pending = super::atlas::PendingDwriteGlyph {
        x,
        y,
        w: measured.width,
        h: measured.height,
        glyph_index,
        pixel_size,
        baseline_x: x as f32 - measured.bearing_x,
        baseline_y: y as f32 + measured.bearing_y,
        is_color,
        face: face.clone(),
    };
    if is_color {
        dwrite_color_pending.push(pending);
    } else {
        dwrite_alpha_pending.push(pending);
    }
    Some(entry)
}

#[cfg(target_os = "linux")]
/// Nearest-neighbor downscale of RGBA bitmap data.
fn downsample_rgba(src: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
    let out_len = (dst_w as usize)
        .saturating_mul(dst_h as usize)
        .saturating_mul(4);
    let mut out = vec![0u8; out_len];
    for dy in 0..dst_h {
        let sy = (dy as f32 * src_h as f32 / dst_h as f32) as usize;
        for dx in 0..dst_w {
            let sx = (dx as f32 * src_w as f32 / dst_w as f32) as usize;
            let si = sy
                .saturating_mul(src_w as usize)
                .saturating_add(sx)
                .saturating_mul(4);
            let di = (dy as usize)
                .saturating_mul(dst_w as usize)
                .saturating_add(dx as usize)
                .saturating_mul(4);
            if si + 4 <= src.len() && di + 4 <= out.len() {
                out[di..di + 4].copy_from_slice(&src[si..si + 4]);
            }
        }
    }
    out
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use crossfont::BitmapBuffer;

    #[test]
    fn rgb_to_alpha_conversion() {
        let glyph = crossfont::RasterizedGlyph {
            character: 'A',
            width: 1,
            height: 1,
            top: 0,
            left: 0,
            advance: (0, 0),
            buffer: BitmapBuffer::Rgb(vec![255, 0, 0]),
        };
        let converted = convert_crossfont_glyph(glyph);
        assert!(!converted.is_color);
        assert_eq!(converted.data, vec![85]);
    }

    #[test]
    fn rgba_passthrough() {
        let glyph = crossfont::RasterizedGlyph {
            character: '😀',
            width: 1,
            height: 1,
            top: 0,
            left: 0,
            advance: (0, 0),
            buffer: BitmapBuffer::Rgba(vec![255, 0, 0, 128]),
        };
        let converted = convert_crossfont_glyph(glyph);
        assert!(converted.is_color);
        assert_eq!(converted.data, vec![255, 0, 0, 128]);
    }
}
