//! DirectWrite glyph rasterization for Windows.
//!
//! Replaces both crossfont and the thin FreeType path on Windows,
//! using native DirectWrite APIs for ClearType-quality text rendering.
//! Color emoji are rendered via `IDWriteFactory2::TranslateColorGlyphRun`,
//! decomposing COLR layers and compositing into RGBA bitmaps.

use std::mem::ManuallyDrop;

use windows::Win32::Foundation::BOOL;
use windows::Win32::Graphics::DirectWrite::*;
use windows::core::{Interface, PCWSTR};

use super::rasterize::RasterizedGlyph;
use super::types::FontStyle;

/// Glyph bounds from DWrite measurement (no pixel data extracted).
pub(crate) struct MeasuredGlyph {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) bearing_x: f32,
    pub(crate) bearing_y: f32,
    pub(crate) is_color: bool,
}

// ─── DWrite rasterizer ──────────────────────────────────────────────

/// DirectWrite-backed glyph rasterizer.
pub(crate) struct DWriteRasterizer {
    factory: IDWriteFactory,
    /// One face per style variant (Regular/Bold/Italic/BoldItalic) per font class.
    /// DWrite applies native bold/oblique simulations per face.
    primary_faces: [Option<IDWriteFontFace>; 4],
    emoji_faces: [Option<IDWriteFontFace>; 4],
    cjk_faces: [Option<IDWriteFontFace>; 4],
    /// True if the emoji font has color glyph layers (COLR/CPAL).
    emoji_is_color: bool,
}

impl DWriteRasterizer {
    pub(crate) fn new(
        family_name: &str,
        primary_path: Option<(&str, u32)>,
        emoji_path: Option<(&str, u32)>,
        cjk_path: Option<(&str, u32)>,
    ) -> anyhow::Result<Self> {
        let factory: IDWriteFactory =
            unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };

        // Try system font collection first for proper bold/italic variants.
        // Fall back to file-based loading with simulations if family not found.
        let primary_faces =
            load_styled_faces_from_collection(&factory, family_name)
                .unwrap_or_else(|| {
                    log::info!("DWrite: '{family_name}' not in system collection, loading from file");
                    primary_path
                        .map(|(p, i)| load_styled_faces_simulated(&factory, p, i))
                        .unwrap_or([None, None, None, None])
                });

        let emoji_faces = emoji_path
            .map(|(p, i)| load_styled_faces_simulated(&factory, p, i))
            .unwrap_or([None, None, None, None]);

        let cjk_faces = cjk_path
            .map(|(p, i)| load_styled_faces_simulated(&factory, p, i))
            .unwrap_or([None, None, None, None]);

        let emoji_is_color = emoji_faces[0]
            .as_ref()
            .is_some_and(is_color_font);

        if emoji_is_color {
            log::info!("DWrite: emoji font detected as color (COLR/CPAL)");
        }

        Ok(Self {
            factory,
            primary_faces,
            emoji_faces,
            cjk_faces,
            emoji_is_color,
        })
    }

    pub(crate) fn primary_face(&self, style: FontStyle) -> Option<&IDWriteFontFace> {
        self.primary_faces[style as usize].as_ref()
    }

    pub(crate) fn emoji_face(&self, style: FontStyle) -> Option<&IDWriteFontFace> {
        self.emoji_faces[style as usize].as_ref()
    }

    pub(crate) fn cjk_face(&self, style: FontStyle) -> Option<&IDWriteFontFace> {
        self.cjk_faces[style as usize].as_ref()
    }

    pub(crate) fn is_emoji_color(&self) -> bool {
        self.emoji_is_color
    }

    /// Rasterize a glyph by its ID.
    /// If `try_color` is true, attempts COLR layer decomposition first for RGBA output.
    /// Falls back to ClearType grayscale if color rendering is unavailable.
    pub(crate) fn rasterize_glyph(
        &self,
        face: &IDWriteFontFace,
        glyph_id: u32,
        pixel_size: f32,
        try_color: bool,
    ) -> Option<RasterizedGlyph> {
        if try_color {
            if let Some(g) = rasterize_color_glyph(&self.factory, face, glyph_id, pixel_size) {
                return Some(g);
            }
        }
        rasterize_grayscale_glyph(&self.factory, face, glyph_id, pixel_size)
    }

    /// Measure a glyph's bounds without extracting pixels.
    ///
    /// Used by the D2D atlas path: GlyphCache allocates atlas space from these
    /// bounds, then the DX backend renders via `DrawGlyphRun` at frame time.
    pub(crate) fn measure_glyph(
        &self,
        face: &IDWriteFontFace,
        glyph_id: u32,
        pixel_size: f32,
        try_color: bool,
    ) -> Option<MeasuredGlyph> {
        let glyph_index = glyph_id as u16;
        let mut glyph_run = build_glyph_run(face, &glyph_index, pixel_size);
        let analysis = create_analysis(&self.factory, &glyph_run, 0.0, 0.0);
        unsafe { ManuallyDrop::drop(&mut glyph_run.fontFace) };

        let (bounds, _) = read_alpha_texture(&analysis?)?;
        let w = (bounds.right - bounds.left) as u32;
        let h = (bounds.bottom - bounds.top) as u32;

        // Detect color glyph via TranslateColorGlyphRun
        let is_color = if try_color {
            is_color_glyph(&self.factory, face, glyph_id, pixel_size)
        } else {
            false
        };

        Some(MeasuredGlyph {
            width: w,
            height: h,
            bearing_x: bounds.left as f32,
            bearing_y: -bounds.top as f32,
            is_color,
        })
    }

    /// Convert a character to its glyph index in the given face.
    pub(crate) fn char_to_glyph(face: &IDWriteFontFace, ch: char) -> Option<u16> {
        let codepoint = ch as u32;
        let mut glyph_index = 0u16;
        unsafe {
            face.GetGlyphIndices(&codepoint, 1, &mut glyph_index)
                .ok()?;
        }
        if glyph_index == 0 {
            None
        } else {
            Some(glyph_index)
        }
    }
}

// ─── Font loading ───────────────────────────────────────────────────

/// Load 4 font face variants by matching family name in the system font collection.
/// Uses actual bold/italic font files when available (e.g. Font-Bold.ttf).
/// Returns `None` if the family name is not found in the collection.
fn load_styled_faces_from_collection(
    factory: &IDWriteFactory,
    family_name: &str,
) -> Option<[Option<IDWriteFontFace>; 4]> {
    let mut collection = None;
    unsafe { factory.GetSystemFontCollection(&mut collection, false) }.ok()?;
    let collection = collection?;

    let wide_name: Vec<u16> = family_name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut index = 0u32;
    let mut exists = BOOL::default();
    unsafe {
        collection
            .FindFamilyName(PCWSTR(wide_name.as_ptr()), &mut index, &mut exists)
            .ok()?;
    }
    if !exists.as_bool() {
        return None;
    }

    let family = unsafe { collection.GetFontFamily(index) }.ok()?;

    // (weight, style) for Regular, Bold, Italic, BoldItalic
    let variants = [
        (DWRITE_FONT_WEIGHT_REGULAR, DWRITE_FONT_STYLE_NORMAL),
        (DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_STYLE_NORMAL),
        (DWRITE_FONT_WEIGHT_REGULAR, DWRITE_FONT_STYLE_ITALIC),
        (DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_STYLE_ITALIC),
    ];

    // Load the regular face first — used as fallback for simulated variants.
    let regular_font = unsafe {
        family.GetFirstMatchingFont(
            DWRITE_FONT_WEIGHT_REGULAR,
            DWRITE_FONT_STRETCH_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
        )
    }
    .ok()?;
    let regular_face = unsafe { regular_font.CreateFontFace() }.ok()?;

    let faces = variants.map(|(weight, style)| {
        let font = unsafe {
            family.GetFirstMatchingFont(weight, DWRITE_FONT_STRETCH_NORMAL, style)
        }
        .ok()?;
        let face = unsafe { font.CreateFontFace() }.ok()?;
        let sim = unsafe { face.GetSimulations() };

        // If DWrite returned a simulated face (no native bold/italic variant),
        // fall back to the regular face to preserve consistent hinting.
        if sim != DWRITE_FONT_SIMULATIONS_NONE {
            log::info!(
                "DWrite collection: '{family_name}' weight={:?} style={:?} → simulated (sim={:?}), using regular",
                weight.0, style.0, sim.0,
            );
            return Some(regular_face.clone());
        }

        log::info!(
            "DWrite collection: '{family_name}' weight={:?} style={:?} → native",
            weight.0, style.0,
        );
        Some(face)
    });

    Some(faces)
}

/// Load 4 font face variants from a file path with DWrite simulations (fallback).
fn load_styled_faces_simulated(
    factory: &IDWriteFactory,
    path: &str,
    face_index: u32,
) -> [Option<IDWriteFontFace>; 4] {
    let simulations = [
        DWRITE_FONT_SIMULATIONS_NONE,    // Regular
        DWRITE_FONT_SIMULATIONS_BOLD,    // Bold
        DWRITE_FONT_SIMULATIONS_OBLIQUE, // Italic
        DWRITE_FONT_SIMULATIONS(         // BoldItalic
            DWRITE_FONT_SIMULATIONS_BOLD.0 | DWRITE_FONT_SIMULATIONS_OBLIQUE.0,
        ),
    ];
    simulations.map(|sim| load_face(factory, path, face_index, sim))
}

/// Load a single font face from a file path with the specified simulations.
fn load_face(
    factory: &IDWriteFactory,
    path: &str,
    face_index: u32,
    simulations: DWRITE_FONT_SIMULATIONS,
) -> Option<IDWriteFontFace> {
    let wide_path: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let font_file = factory
            .CreateFontFileReference(PCWSTR(wide_path.as_ptr()), None)
            .ok()?;

        let mut supported = BOOL::default();
        let mut file_type = DWRITE_FONT_FILE_TYPE_UNKNOWN;
        let mut face_type = DWRITE_FONT_FACE_TYPE_UNKNOWN;
        let mut num_faces = 0u32;
        font_file
            .Analyze(
                &mut supported,
                &mut file_type,
                Some(&mut face_type),
                &mut num_faces,
            )
            .ok()?;

        if !supported.as_bool() {
            log::warn!("DWrite: unsupported font file: {path}");
            return None;
        }

        let face = factory
            .CreateFontFace(
                face_type,
                &[Some(font_file)],
                face_index,
                simulations,
            )
            .ok()?;

        log::info!(
            "DWrite face loaded: {path} (index={face_index}, sim={:?})",
            simulations.0
        );
        Some(face)
    }
}

/// Check if a font face supports color glyphs (COLR/CPAL) via IDWriteFontFace2.
fn is_color_font(face: &IDWriteFontFace) -> bool {
    face.cast::<IDWriteFontFace2>()
        .map(|f2| unsafe { f2.IsColorFont().as_bool() })
        .unwrap_or(false)
}

/// Check if a specific glyph has color layers via `TranslateColorGlyphRun`.
fn is_color_glyph(
    factory: &IDWriteFactory,
    face: &IDWriteFontFace,
    glyph_id: u32,
    pixel_size: f32,
) -> bool {
    let Ok(factory2) = factory.cast::<IDWriteFactory2>() else {
        return false;
    };
    let glyph_index = glyph_id as u16;
    let mut glyph_run = build_glyph_run(face, &glyph_index, pixel_size);
    let result = unsafe {
        factory2
            .TranslateColorGlyphRun(0.0, 0.0, &glyph_run, None, DWRITE_MEASURING_MODE_NATURAL, None, 0)
            .is_ok()
    };
    unsafe { ManuallyDrop::drop(&mut glyph_run.fontFace) };
    result
}

// ─── Helpers ────────────────────────────────────────────────────────

/// Build a `DWRITE_GLYPH_RUN` for a single glyph.
///
/// The returned struct contains a `ManuallyDrop` clone of `face`.
/// Caller **must** call `ManuallyDrop::drop(&mut run.fontFace)` when done.
fn build_glyph_run(
    face: &IDWriteFontFace,
    glyph_index: *const u16,
    pixel_size: f32,
) -> DWRITE_GLYPH_RUN {
    DWRITE_GLYPH_RUN {
        fontFace: ManuallyDrop::new(Some(face.clone())),
        fontEmSize: pixel_size,
        glyphCount: 1,
        glyphIndices: glyph_index,
        glyphAdvances: std::ptr::null(),
        glyphOffsets: std::ptr::null(),
        isSideways: BOOL(0),
        bidiLevel: 0,
    }
}

// ─── Analysis helpers ───────────────────────────────────────────────

/// Create a glyph run analysis, preferring Factory3 grayscale AA over ClearType.
///
/// Factory3 (`DWRITE_TEXT_ANTIALIAS_MODE_GRAYSCALE`) produces true single-channel
/// alpha, avoiding the uneven-stroke artifacts that arise from averaging ClearType
/// RGB subpixel data. Falls back to Factory1 ClearType on older Windows.
fn create_analysis(
    factory: &IDWriteFactory,
    glyph_run: &DWRITE_GLYPH_RUN,
    baseline_x: f32,
    baseline_y: f32,
) -> Option<IDWriteGlyphRunAnalysis> {
    // Prefer Factory3 grayscale (Windows 10+)
    if let Ok(f3) = factory.cast::<IDWriteFactory3>() {
        if let Ok(a) = unsafe {
            f3.CreateGlyphRunAnalysis(
                glyph_run,
                None,
                DWRITE_RENDERING_MODE1_NATURAL_SYMMETRIC,
                DWRITE_MEASURING_MODE_NATURAL,
                DWRITE_GRID_FIT_MODE_DEFAULT,
                DWRITE_TEXT_ANTIALIAS_MODE_GRAYSCALE,
                baseline_x,
                baseline_y,
            )
        } {
            return Some(a);
        }
    }
    // Fallback: Factory1 ClearType
    unsafe {
        factory
            .CreateGlyphRunAnalysis(
                glyph_run,
                1.0,
                None,
                DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC,
                DWRITE_MEASURING_MODE_NATURAL,
                baseline_x,
                baseline_y,
            )
            .ok()
    }
}

use windows::Win32::Foundation::RECT;

/// Read single-channel alpha data from a glyph run analysis.
///
/// Tries `ALIASED_1x1` first (1 byte/pixel, produced by grayscale mode),
/// then falls back to `CLEARTYPE_3x1` (3 bytes/pixel, averaged to 1).
fn read_alpha_texture(
    analysis: &IDWriteGlyphRunAnalysis,
) -> Option<(RECT, Vec<u8>)> {
    // Grayscale path: direct single-channel
    if let Ok(bounds) = unsafe { analysis.GetAlphaTextureBounds(DWRITE_TEXTURE_ALIASED_1x1) } {
        let w = (bounds.right - bounds.left) as u32;
        let h = (bounds.bottom - bounds.top) as u32;
        if w > 0 && h > 0 {
            let mut data = vec![0u8; (w * h) as usize];
            if unsafe {
                analysis.CreateAlphaTexture(DWRITE_TEXTURE_ALIASED_1x1, &bounds, &mut data)
            }
            .is_ok()
            {
                return Some((bounds, data));
            }
        }
    }

    // ClearType fallback: average RGB → single channel
    let bounds = unsafe {
        analysis
            .GetAlphaTextureBounds(DWRITE_TEXTURE_CLEARTYPE_3x1)
            .ok()?
    };
    let w = (bounds.right - bounds.left) as u32;
    let h = (bounds.bottom - bounds.top) as u32;
    if w == 0 || h == 0 {
        return None;
    }
    let mut rgb = vec![0u8; (w * h * 3) as usize];
    unsafe {
        analysis
            .CreateAlphaTexture(DWRITE_TEXTURE_CLEARTYPE_3x1, &bounds, &mut rgb)
            .ok()?;
    }
    let alpha: Vec<u8> = rgb
        .chunks_exact(3)
        .map(|c| ((c[0] as u16 + c[1] as u16 + c[2] as u16) / 3) as u8)
        .collect();
    Some((bounds, alpha))
}

// ─── Grayscale glyph rasterization ─────────────────────────────────

/// Rasterize a single glyph as grayscale alpha.
fn rasterize_grayscale_glyph(
    factory: &IDWriteFactory,
    face: &IDWriteFontFace,
    glyph_id: u32,
    pixel_size: f32,
) -> Option<RasterizedGlyph> {
    let glyph_index = glyph_id as u16;
    let mut glyph_run = build_glyph_run(face, &glyph_index, pixel_size);
    let analysis = create_analysis(factory, &glyph_run, 0.0, 0.0);
    unsafe { ManuallyDrop::drop(&mut glyph_run.fontFace) };

    let (bounds, alpha_data) = read_alpha_texture(&analysis?)?;
    let w = (bounds.right - bounds.left) as u32;
    let h = (bounds.bottom - bounds.top) as u32;

    Some(RasterizedGlyph {
        width: w,
        height: h,
        bearing_x: bounds.left as f32,
        bearing_y: -bounds.top as f32,
        is_color: false,
        data: alpha_data,
    })
}

// ─── Color glyph rasterization (COLR/CPAL) ─────────────────────────

/// Rasterize a color glyph by decomposing COLR layers via `TranslateColorGlyphRun`.
///
/// Each layer is rendered as a grayscale alpha mask, then composited with its
/// palette color into an RGBA bitmap. Returns `None` if the glyph has no color
/// layers or if `IDWriteFactory2` is unavailable (pre-Windows 8.1).
fn rasterize_color_glyph(
    factory: &IDWriteFactory,
    face: &IDWriteFontFace,
    glyph_id: u32,
    pixel_size: f32,
) -> Option<RasterizedGlyph> {
    let factory2: IDWriteFactory2 = factory.cast().ok()?;

    let glyph_index = glyph_id as u16;
    let mut glyph_run = build_glyph_run(face, &glyph_index, pixel_size);

    // Get color layer enumerator. Fails with DWRITE_E_NOCOLOR if not a color glyph.
    let enumerator = unsafe {
        factory2.TranslateColorGlyphRun(
            0.0,
            0.0,
            &glyph_run,
            None,
            DWRITE_MEASURING_MODE_NATURAL,
            None,
            0,
        )
    };

    // Get total glyph bounds from the base outline.
    let base_analysis = create_analysis(factory, &glyph_run, 0.0, 0.0);

    // Release the glyph run face clone now — both API calls have captured what they need.
    unsafe { ManuallyDrop::drop(&mut glyph_run.fontFace) };

    let enumerator = enumerator.ok()?;
    let (bounds, _) = read_alpha_texture(&base_analysis?)?;

    let w = (bounds.right - bounds.left) as u32;
    let h = (bounds.bottom - bounds.top) as u32;
    if w == 0 || h == 0 {
        return None;
    }

    let mut rgba = vec![0u8; (w * h * 4) as usize];

    // Iterate COLR layers and composite.
    loop {
        let has_next = unsafe { enumerator.MoveNext() };
        if has_next.is_err() || !has_next.unwrap().as_bool() {
            break;
        }

        let color_run_ptr = match unsafe { enumerator.GetCurrentRun() } {
            Ok(p) => p,
            Err(_) => continue,
        };
        let color_run = unsafe { &*color_run_ptr };

        // Render this layer's alpha mask via the shared grayscale helper.
        let Some(layer_analysis) = create_analysis(
            factory,
            &color_run.glyphRun,
            color_run.baselineOriginX,
            color_run.baselineOriginY,
        ) else {
            continue;
        };

        let Some((layer_bounds, layer_alpha)) = read_alpha_texture(&layer_analysis) else {
            continue;
        };

        let lw = (layer_bounds.right - layer_bounds.left) as u32;
        let lh = (layer_bounds.bottom - layer_bounds.top) as u32;
        if lw == 0 || lh == 0 {
            continue;
        }

        // Determine layer color: paletteIndex 0xFFFF = use foreground (white).
        let (cr, cg, cb, ca) = if color_run.paletteIndex == 0xFFFF {
            (1.0f32, 1.0, 1.0, 1.0)
        } else {
            let c = &color_run.runColor;
            (c.r, c.g, c.b, c.a)
        };

        // Alpha-over composite into the RGBA buffer.
        composite_layer(
            &mut rgba,
            w,
            h,
            bounds.left,
            bounds.top,
            &layer_alpha,
            lw,
            lh,
            layer_bounds.left,
            layer_bounds.top,
            cr,
            cg,
            cb,
            ca,
        );
    }

    Some(RasterizedGlyph {
        width: w,
        height: h,
        bearing_x: bounds.left as f32,
        bearing_y: -bounds.top as f32,
        is_color: true,
        data: rgba,
    })
}

/// Alpha-over composite a single-channel alpha layer into an RGBA buffer.
#[allow(clippy::too_many_arguments)]
fn composite_layer(
    dst: &mut [u8],
    dst_w: u32,
    dst_h: u32,
    dst_origin_x: i32,
    dst_origin_y: i32,
    src_alpha: &[u8],
    src_w: u32,
    src_h: u32,
    src_origin_x: i32,
    src_origin_y: i32,
    cr: f32,
    cg: f32,
    cb: f32,
    ca: f32,
) {
    for sy in 0..src_h {
        for sx in 0..src_w {
            let coverage = src_alpha[(sy * src_w + sx) as usize];
            if coverage == 0 {
                continue;
            }

            let dx = (src_origin_x - dst_origin_x) + sx as i32;
            let dy = (src_origin_y - dst_origin_y) + sy as i32;
            if dx < 0 || dy < 0 || dx >= dst_w as i32 || dy >= dst_h as i32 {
                continue;
            }

            let di = (dy as u32 * dst_w + dx as u32) as usize * 4;
            let src_a = coverage as f32 / 255.0 * ca;
            let inv_a = 1.0 - src_a;

            dst[di] = (cr * 255.0 * src_a + dst[di] as f32 * inv_a).min(255.0) as u8;
            dst[di + 1] = (cg * 255.0 * src_a + dst[di + 1] as f32 * inv_a).min(255.0) as u8;
            dst[di + 2] = (cb * 255.0 * src_a + dst[di + 2] as f32 * inv_a).min(255.0) as u8;
            dst[di + 3] = (src_a * 255.0 + dst[di + 3] as f32 * inv_a).min(255.0) as u8;
        }
    }
}

// ─── Metrics ────────────────────────────────────────────────────────

/// Compute cell metrics from a DirectWrite font face.
/// Returns `(cell_width, cell_height, baseline_from_top, face_width_unrounded)`.
pub(crate) fn compute_dwrite_metrics(
    face: &IDWriteFontFace,
    pixel_size: f32,
) -> (f32, f32, f32, f32) {
    let mut metrics = DWRITE_FONT_METRICS::default();
    unsafe { face.GetMetrics(&mut metrics) };

    let units_per_em = metrics.designUnitsPerEm as f64;
    let px_per_unit = pixel_size as f64 / units_per_em;

    let ascent = metrics.ascent as f64 * px_per_unit;
    let descent = metrics.descent as f64 * px_per_unit;
    let line_gap = metrics.lineGap as f64 * px_per_unit;

    let face_height = ascent + descent + line_gap;
    let cell_height = face_height.round().max(1.0) as f32;

    let half_line_gap = line_gap / 2.0;
    let face_baseline_from_bottom = half_line_gap + descent;
    let cell_baseline_from_bottom =
        (face_baseline_from_bottom - (cell_height as f64 - face_height) / 2.0).round();
    let baseline_from_top = cell_height - cell_baseline_from_bottom as f32;

    let face_width = measure_max_ascii_advance(face, px_per_unit);
    let cell_width = face_width.round().max(1.0) as f32;

    log::info!(
        "font metrics (DWrite): ascent={ascent:.2} descent={descent:.2} line_gap={line_gap:.2} \
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

/// Measure max horizontal advance across printable ASCII using DWrite glyph metrics.
fn measure_max_ascii_advance(face: &IDWriteFontFace, px_per_unit: f64) -> f64 {
    let codepoints: Vec<u32> = (0x20u32..=0x7Eu32).collect();
    let mut glyph_indices = vec![0u16; codepoints.len()];

    unsafe {
        if face
            .GetGlyphIndices(
                codepoints.as_ptr(),
                codepoints.len() as u32,
                glyph_indices.as_mut_ptr(),
            )
            .is_err()
        {
            return 0.0;
        }
    }

    let mut glyph_metrics = vec![DWRITE_GLYPH_METRICS::default(); glyph_indices.len()];
    unsafe {
        if face
            .GetDesignGlyphMetrics(
                glyph_indices.as_ptr(),
                glyph_indices.len() as u32,
                glyph_metrics.as_mut_ptr(),
                false,
            )
            .is_err()
        {
            return 0.0;
        }
    }

    glyph_metrics
        .iter()
        .map(|m| m.advanceWidth as f64 * px_per_unit)
        .fold(0.0f64, f64::max)
}

// ─── CJK sizing ─────────────────────────────────────────────────────

/// Compute CJK pixel size using DWrite, matching the em-normalized ic_width approach.
pub(crate) fn compute_cjk_pixel_size_dwrite(
    pixel_size: f32,
    cell_width: f32,
    primary_face: Option<&IDWriteFontFace>,
    cjk_face: Option<&IDWriteFontFace>,
) -> f32 {
    let cjk = match cjk_face {
        Some(f) => f,
        None => return pixel_size,
    };

    let cjk_ic = match measure_char_advance(cjk, '水', pixel_size) {
        Some(w) if w > 0.0 => w,
        _ => return pixel_size,
    };

    let cjk_ppem = pixel_size as f64;

    let (primary_ic, primary_ppem) = if let Some(primary) = primary_face {
        let ic = measure_char_advance(primary, '水', pixel_size).unwrap_or_else(|| {
            let mut pm = DWRITE_FONT_METRICS::default();
            unsafe { primary.GetMetrics(&mut pm) };
            let px_per_unit = pixel_size as f64 / pm.designUnitsPerEm as f64;
            let cap_height = pm.capHeight as f64 * px_per_unit;
            if cap_height > 0.0 {
                (1.5 * cap_height).min(cell_width as f64 * 2.0)
            } else {
                cell_width as f64 * 2.0
            }
        });
        (ic, pixel_size as f64)
    } else {
        let ic = (cell_width as f64 * 2.0).min(pixel_size as f64 * 1.5);
        (ic, pixel_size as f64)
    };

    let primary_em = primary_ic / primary_ppem;
    let cjk_em = cjk_ic / cjk_ppem;

    if cjk_em <= 0.0 {
        return pixel_size;
    }

    let scale = primary_em / cjk_em;
    let adjusted = pixel_size as f64 * scale;

    log::info!(
        "CJK size adjustment (DWrite): primary_ic={primary_ic:.2} ({primary_em:.3}em) \
         cjk_ic={cjk_ic:.2} ({cjk_em:.3}em) scale={scale:.3} \
         px={pixel_size:.1}→{adjusted:.1}",
    );

    adjusted as f32
}

/// Measure the advance width of a character using DWrite design metrics.
fn measure_char_advance(face: &IDWriteFontFace, ch: char, pixel_size: f32) -> Option<f64> {
    let codepoint = ch as u32;
    let mut glyph_index = 0u16;
    unsafe {
        face.GetGlyphIndices(&codepoint, 1, &mut glyph_index)
            .ok()?;
    }
    if glyph_index == 0 {
        return None;
    }

    let mut glyph_metric = DWRITE_GLYPH_METRICS::default();
    unsafe {
        face.GetDesignGlyphMetrics(&glyph_index, 1, &mut glyph_metric, false)
            .ok()?;
    }

    let mut font_metrics = DWRITE_FONT_METRICS::default();
    unsafe { face.GetMetrics(&mut font_metrics) };

    let px_per_unit = pixel_size as f64 / font_metrics.designUnitsPerEm as f64;
    let advance = glyph_metric.advanceWidth as f64 * px_per_unit;

    if advance > 0.0 {
        Some(advance)
    } else {
        None
    }
}
