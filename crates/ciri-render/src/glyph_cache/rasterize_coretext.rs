//! CoreText/Core Graphics glyph rasterization for macOS.
//!
//! Replaces both crossfont and the thin FreeType path on macOS,
//! using native CoreText for text shaping and Core Graphics for rendering.
//! Color emoji are rendered via sbix bitmap tables through CG's color glyph support.

use core_graphics::color_space::CGColorSpace;
use core_graphics::context::CGContext;
use core_graphics::geometry::{CGAffineTransform, CGPoint, CGRect, CGSize};
use core_text::font::CTFont;
use core_text::font_descriptor::{
    SymbolicTraitAccessors, TraitAccessors, kCTFontBoldTrait, kCTFontItalicTrait,
    kCTFontOrientationDefault,
};

use super::rasterize::RasterizedGlyph;
use super::types::FontStyle;

/// CoreText-backed glyph rasterizer for macOS.
pub(crate) struct CoreTextRasterizer {
    /// Primary font in 4 style variants (Regular/Bold/Italic/BoldItalic).
    /// If a native variant is unavailable, falls back to the regular face
    /// with synthetic bold/italic applied at render time.
    primary_fonts: [CTFont; 4],
    /// Track which styles need synthetic bold/italic.
    primary_synth: [SyntheticStyle; 4],
    /// Emoji font (usually Apple Color Emoji).
    emoji_font: Option<CTFont>,
    /// CJK font.
    cjk_font: Option<CTFont>,
    /// Whether the emoji font has sbix (color bitmap) table.
    emoji_is_color: bool,
}

/// Tracks what synthetic transformations to apply.
#[derive(Clone, Copy, Default)]
struct SyntheticStyle {
    bold: bool,
    italic: bool,
}

impl CoreTextRasterizer {
    /// Create a new CoreText rasterizer.
    ///
    /// `family_name`: font family to search in the system.
    /// `primary_path`: optional file path + face index for loading from file.
    /// `emoji_path`: optional emoji font file path + face index.
    /// `cjk_path`: optional CJK font file path + face index.
    /// `pixel_size`: the rendering size in pixels.
    pub(crate) fn new(
        family_name: &str,
        primary_path: Option<(&str, u32)>,
        emoji_path: Option<(&str, u32)>,
        cjk_path: Option<(&str, u32)>,
        pixel_size: f32,
    ) -> anyhow::Result<Self> {
        let pt_size = pixel_size as f64;

        // Load the regular face — prefer file path, fall back to system font
        let regular = if let Some((path, index)) = primary_path {
            load_font_from_file(path, index, pt_size)
                .unwrap_or_else(|| load_system_font(family_name, pt_size))
        } else {
            load_system_font(family_name, pt_size)
        };

        // Create style variants using CTFont::clone_with_symbolic_traits
        let (bold, bold_synth) = create_variant(&regular, kCTFontBoldTrait, false, true);
        let (italic, italic_synth) = create_variant(&regular, kCTFontItalicTrait, true, false);
        let (bold_italic, bold_italic_synth) = {
            // Try to get a native bold-italic from the bold face
            let (bi, synth) = create_variant(&bold, kCTFontItalicTrait, true, bold_synth.bold);
            (bi, synth)
        };

        let primary_fonts = [regular, bold, italic, bold_italic];
        let primary_synth = [
            SyntheticStyle::default(),
            bold_synth,
            italic_synth,
            bold_italic_synth,
        ];

        // Log loaded primary font variants
        let style_names = ["Regular", "Bold", "Italic", "BoldItalic"];
        for (i, font) in primary_fonts.iter().enumerate() {
            let ps_name = font.postscript_name();
            let family = font.family_name();
            log::info!(
                "CoreText raster: primary[{}] ps_name='{}' family='{}' synth_bold={} synth_italic={}",
                style_names[i],
                ps_name,
                family,
                primary_synth[i].bold,
                primary_synth[i].italic,
            );
        }
        // Log primary font metrics
        {
            let f = &primary_fonts[0];
            log::info!(
                "CoreText raster: primary metrics ascent={:.2} descent={:.2} leading={:.2} units_per_em={}",
                f.ascent(),
                f.descent(),
                f.leading(),
                f.units_per_em(),
            );
        }

        // Load emoji font
        let emoji_font = if let Some((path, index)) = emoji_path {
            load_font_from_file(path, index, pt_size)
        } else {
            // Try Apple Color Emoji
            core_text::font::new_from_name("Apple Color Emoji", pt_size).ok()
        };

        let emoji_is_color = emoji_font
            .as_ref()
            .map(|f| has_sbix_table(f))
            .unwrap_or(false);

        if let Some(ref ef) = emoji_font {
            log::info!(
                "CoreText raster: emoji font ps_name='{}' family='{}' is_color(sbix)={}",
                ef.postscript_name(),
                ef.family_name(),
                emoji_is_color,
            );
        } else {
            log::info!("CoreText raster: no emoji font loaded");
        }

        // Load CJK font
        let cjk_font = if let Some((path, index)) = cjk_path {
            load_font_from_file(path, index, pt_size)
        } else {
            None
        };

        if let Some(ref cf) = cjk_font {
            log::info!(
                "CoreText raster: CJK font ps_name='{}' family='{}' pt_size={:.1}",
                cf.postscript_name(),
                cf.family_name(),
                cf.pt_size(),
            );
        } else {
            log::info!("CoreText raster: no CJK font loaded");
        }

        Ok(Self {
            primary_fonts,
            primary_synth,
            emoji_font,
            cjk_font,
            emoji_is_color,
        })
    }

    /// Get the primary font for the given style.
    pub(crate) fn primary_font(&self, style: FontStyle) -> &CTFont {
        &self.primary_fonts[style as usize]
    }

    /// Get the emoji font (Regular only; emoji don't have bold/italic).
    pub(crate) fn emoji_font(&self) -> Option<&CTFont> {
        self.emoji_font.as_ref()
    }

    /// Get the CJK font.
    pub(crate) fn cjk_font(&self) -> Option<&CTFont> {
        self.cjk_font.as_ref()
    }

    /// Whether the emoji font supports color rendering.
    pub(crate) fn is_emoji_color(&self) -> bool {
        self.emoji_is_color
    }

    /// Recreate the CJK font at the adjusted pixel size.
    /// Called after `compute_cjk_pixel_size_ct()` determines the correct size.
    pub(crate) fn set_cjk_pixel_size(&mut self, cjk_pixel_size: f32) {
        if let Some(ref cjk) = self.cjk_font {
            self.cjk_font = Some(cjk.clone_with_font_size(cjk_pixel_size as f64));
            log::info!("CoreText: CJK font resized to {:.1}px", cjk_pixel_size);
        }
    }

    /// Rasterize a character to a glyph bitmap.
    pub(crate) fn rasterize_char(
        &self,
        ch: char,
        style: FontStyle,
        font: &CTFont,
        try_color: bool,
    ) -> Option<RasterizedGlyph> {
        let glyph_id = char_to_glyph(font, ch)?;
        let synth = if std::ptr::eq(font, self.primary_font(style)) {
            self.primary_synth[style as usize]
        } else {
            SyntheticStyle::default()
        };
        self.rasterize_glyph_inner(font, glyph_id, synth, try_color)
    }

    /// Rasterize a glyph by its ID.
    pub(crate) fn rasterize_glyph_id(
        &self,
        font: &CTFont,
        glyph_id: u32,
        style: FontStyle,
        try_color: bool,
    ) -> Option<RasterizedGlyph> {
        let synth = if std::ptr::eq(font, self.primary_font(style)) {
            self.primary_synth[style as usize]
        } else {
            // For emoji/CJK, apply synthetic bold/italic based on style
            SyntheticStyle {
                bold: matches!(style, FontStyle::Bold | FontStyle::BoldItalic),
                italic: matches!(style, FontStyle::Italic | FontStyle::BoldItalic),
            }
        };
        self.rasterize_glyph_inner(font, glyph_id as u16, synth, try_color)
    }

    /// Core rasterization: renders a single glyph to a bitmap.
    fn rasterize_glyph_inner(
        &self,
        font: &CTFont,
        glyph: u16,
        synth: SyntheticStyle,
        try_color: bool,
    ) -> Option<RasterizedGlyph> {
        // Try color rendering for emoji
        if try_color && self.emoji_is_color {
            if let Some(g) = render_color_glyph(font, glyph) {
                log::debug!(
                    "CoreText raster: glyph_id={} color render {}x{} bearing=({:.1},{:.1}) font={}",
                    glyph,
                    g.width,
                    g.height,
                    g.bearing_x,
                    g.bearing_y,
                    font.family_name(),
                );
                return Some(g);
            }
        }

        let result = render_grayscale_glyph(font, glyph, synth);
        match &result {
            Some(g) => log::debug!(
                "CoreText raster: glyph_id={} grayscale {}x{} bearing=({:.1},{:.1}) font={}",
                glyph,
                g.width,
                g.height,
                g.bearing_x,
                g.bearing_y,
                font.family_name(),
            ),
            None => log::debug!(
                "CoreText raster: glyph_id={} -> no bitmap (font={})",
                glyph,
                font.family_name(),
            ),
        }
        result
    }
}

// ─── Font loading helpers ──────────────────────────────────────────

/// Load a font from a file path + face index via CoreText.
/// Delegates to the shared `load_ct_font_from_path` which correctly handles TTC face indices.
fn load_font_from_file(path: &str, index: u32, size: f64) -> Option<CTFont> {
    crate::shaper::load_ct_font_from_path(path, index, size)
}

/// Load a font by family name from the system.
fn load_system_font(family_name: &str, size: f64) -> CTFont {
    core_text::font::new_from_name(family_name, size).unwrap_or_else(|_| {
        log::warn!("font '{}' not found, falling back to Menlo", family_name);
        core_text::font::new_from_name("Menlo", size).expect("fallback font Menlo not found")
    })
}

/// Create a style variant of a font using `clone_with_symbolic_traits`.
/// Returns (font, synthetic_style).
fn create_variant(
    base: &CTFont,
    trait_mask: u32,
    need_italic: bool,
    need_bold: bool,
) -> (CTFont, SyntheticStyle) {
    // Try to get a native variant via CoreText
    if let Some(variant) = base.clone_with_symbolic_traits(trait_mask, trait_mask) {
        // Check if the variant actually has the requested traits
        let desc = variant.copy_descriptor();
        let traits = desc.traits();
        let symbolic = traits.symbolic_traits();
        let has_bold = symbolic.is_bold();
        let has_italic = symbolic.is_italic();
        let synth = SyntheticStyle {
            bold: need_bold && !has_bold,
            italic: need_italic && !has_italic,
        };
        return (variant, synth);
    }

    // No native variant — use base font with full synthetic style
    (
        base.clone_with_font_size(base.pt_size()),
        SyntheticStyle {
            bold: need_bold,
            italic: need_italic,
        },
    )
}

/// Check if a font has an sbix (Standard Bitmap Graphics) table, indicating color emoji.
fn has_sbix_table(font: &CTFont) -> bool {
    let tag = u32::from_be_bytes(*b"sbix");
    font.get_font_table(tag).is_some()
}

/// Convert a character to a glyph index using CoreText.
fn char_to_glyph(font: &CTFont, ch: char) -> Option<u16> {
    let mut buf = [0u16; 2];
    let utf16: &[u16] = ch.encode_utf16(&mut buf);
    let mut glyphs = vec![0u16; utf16.len()];
    let success = unsafe {
        font.get_glyphs_for_characters(utf16.as_ptr(), glyphs.as_mut_ptr(), utf16.len() as isize)
    };
    if success && glyphs[0] != 0 {
        Some(glyphs[0])
    } else {
        None
    }
}

// ─── Grayscale rendering ───────────────────────────────────────────

/// Render a glyph as a grayscale (alpha-only) bitmap.
fn render_grayscale_glyph(
    font: &CTFont,
    glyph: u16,
    synth: SyntheticStyle,
) -> Option<RasterizedGlyph> {
    // Get glyph bounding rect
    let glyphs = [glyph];
    let bounds = font.get_bounding_rects_for_glyphs(kCTFontOrientationDefault, &glyphs);

    // Bounds in CG coordinates (origin at bottom-left, y up)
    if bounds.size.width <= 0.0 || bounds.size.height <= 0.0 {
        return None;
    }

    // Add 1px padding on each side for anti-aliasing
    let pad: u32 = 1;
    let w = (bounds.size.width.ceil() as u32) + pad * 2;
    let h = (bounds.size.height.ceil() as u32) + pad * 2;

    if w == 0 || h == 0 {
        return None;
    }

    // Create grayscale bitmap context (alpha-only, 1 byte per pixel)
    let color_space = CGColorSpace::create_device_gray();
    let mut ctx = CGContext::create_bitmap_context(
        None, // auto-allocate buffer
        w as usize,
        h as usize,
        8,          // bits per component
        w as usize, // bytes per row
        &color_space,
        core_graphics::image::CGImageAlphaInfo::CGImageAlphaOnly as u32,
    );

    // Configure rendering
    ctx.set_allows_font_smoothing(true);
    ctx.set_should_smooth_fonts(true);
    ctx.set_allows_antialiasing(true);
    ctx.set_should_antialias(true);
    ctx.set_allows_font_subpixel_positioning(true);
    ctx.set_allows_font_subpixel_quantization(false);

    // Apply synthetic transformations
    if synth.bold {
        // Synthetic bold: use fill+stroke mode
        let stroke_width = font.pt_size() * 0.03;
        ctx.set_text_drawing_mode(core_graphics::context::CGTextDrawingMode::CGTextFillStroke);
        ctx.set_line_width(stroke_width);
        ctx.set_gray_fill_color(1.0, 1.0);
        // core-graphics 0.24 doesn't expose set_gray_stroke_color;
        // RGB white converts cleanly to gray white in an alpha-only context.
        ctx.set_rgb_stroke_color(1.0, 1.0, 1.0, 1.0);
    } else {
        ctx.set_text_drawing_mode(core_graphics::context::CGTextDrawingMode::CGTextFill);
        ctx.set_gray_fill_color(1.0, 1.0);
    }

    // Position: offset so glyph origin aligns with padding
    let origin_x = -bounds.origin.x + pad as f64;
    let origin_y = -bounds.origin.y + pad as f64;

    let positions = if synth.italic {
        // Synthetic italic: apply skew via concat_ctm (~14 degrees, tan(14°) ≈ 0.25).
        // CTFontDrawGlyphs ignores the text matrix, so concat_ctm is required.
        // Compensate position: skew maps (x,y) → (x + 0.25*y, y), so pre-subtract.
        let skew = 0.25;
        let transform = CGAffineTransform::new(1.0, 0.0, skew, 1.0, 0.0, 0.0);
        ctx.concat_ctm(transform);
        [CGPoint::new(origin_x - skew * origin_y, origin_y)]
    } else {
        [CGPoint::new(origin_x, origin_y)]
    };
    font.draw_glyphs(&glyphs, &positions, ctx.clone());

    // Extract pixel data
    let data_len = w as usize * h as usize;
    let data = ctx.data()[..data_len].to_vec();

    // Bearing: CG y-axis is up, our bearing_y = distance from baseline to top of glyph
    let bearing_x = bounds.origin.x as f32 - pad as f32;
    let bearing_y = (bounds.origin.y + bounds.size.height) as f32 + pad as f32;

    Some(RasterizedGlyph {
        width: w,
        height: h,
        bearing_x,
        bearing_y,
        is_color: false,
        data,
    })
}

// ─── Color emoji rendering ─────────────────────────────────────────

/// Render a color emoji glyph to an RGBA bitmap.
fn render_color_glyph(font: &CTFont, glyph: u16) -> Option<RasterizedGlyph> {
    let glyphs = [glyph];
    let bounds = font.get_bounding_rects_for_glyphs(kCTFontOrientationDefault, &glyphs);

    if bounds.size.width <= 0.0 || bounds.size.height <= 0.0 {
        return None;
    }

    let pad: u32 = 1;
    let w = (bounds.size.width.ceil() as u32) + pad * 2;
    let h = (bounds.size.height.ceil() as u32) + pad * 2;

    if w == 0 || h == 0 {
        return None;
    }

    // Create RGBA bitmap context for color rendering
    let color_space = CGColorSpace::create_device_rgb();
    let bytes_per_row = w as usize * 4;
    let mut ctx = CGContext::create_bitmap_context(
        None,
        w as usize,
        h as usize,
        8,
        bytes_per_row,
        &color_space,
        core_graphics::base::kCGImageAlphaPremultipliedFirst
            | core_graphics::base::kCGBitmapByteOrder32Host,
    );

    ctx.set_allows_font_smoothing(true);
    ctx.set_should_smooth_fonts(false); // No smoothing for color glyphs
    ctx.set_allows_antialiasing(true);
    ctx.set_should_antialias(true);
    ctx.set_allows_font_subpixel_positioning(true);

    // Clear context to transparent
    let clear_rect = CGRect::new(&CGPoint::new(0.0, 0.0), &CGSize::new(w as f64, h as f64));
    ctx.clear_rect(clear_rect);

    let origin_x = -bounds.origin.x + pad as f64;
    let origin_y = -bounds.origin.y + pad as f64;

    let positions = [CGPoint::new(origin_x, origin_y)];
    font.draw_glyphs(&glyphs, &positions, ctx.clone());

    // Extract pixel data and convert BGRA → RGBA
    let data_len = bytes_per_row * h as usize;
    let raw = &ctx.data()[..data_len];

    let mut rgba_data = Vec::with_capacity(data_len);
    for pixel in raw.chunks_exact(4) {
        // CG premultiplied-first = BGRA on little-endian
        rgba_data.push(pixel[2]); // R
        rgba_data.push(pixel[1]); // G
        rgba_data.push(pixel[0]); // B
        rgba_data.push(pixel[3]); // A
    }

    // Check if any pixel has non-zero alpha — if not, this is not a color glyph
    let has_color = rgba_data.iter().skip(3).step_by(4).any(|&a| a > 0);
    if !has_color {
        return None;
    }

    let bearing_x = bounds.origin.x as f32 - pad as f32;
    let bearing_y = (bounds.origin.y + bounds.size.height) as f32 + pad as f32;

    Some(RasterizedGlyph {
        width: w,
        height: h,
        bearing_x,
        bearing_y,
        is_color: true,
        data: rgba_data,
    })
}
