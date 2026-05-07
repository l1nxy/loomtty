use ciri_render::glyph_cache::{FontStyle, GlyphCache, GlyphEntry, GlyphInstance};
use ciri_render::ui_shaper::{UiShapedGlyph, UiTextShaper};
use unicode_width::UnicodeWidthChar;

pub(crate) struct TextEmitParams {
    pub x_start: f32,
    pub y: f32,
    pub cell_width: f32,
    pub baseline: f32,
    pub color: [f32; 4],
    /// Backdrop the glyphs sit on, used by the linear-correction shader
    /// to compute perceptually-correct alpha. `[0; 4]` = no opaque
    /// ancestor (text on default chrome bg).
    pub bg_color: [f32; 4],
    pub scale: f32,
}

/// Emit glyph instances for UI text.
///
/// Two paths:
/// - Shaper path (preferred): hand `text` to `UiTextShaper::shape` to get
///   `glyph_id` + `x_advance` in pixels, then rasterize via
///   `GlyphCache::ensure_glyph_id`. Pen advances by the actual shaped
///   advance, so proportional fonts, ligatures, and kerning all work.
/// - Legacy char path (fallback): used when the shaper has no loaded face
///   (e.g. pre-init in tests). Keeps the old `cell_width × unicode_width`
///   layout so status bar still renders something reasonable.
pub(crate) fn emit_status_text(
    atlas: &mut GlyphCache,
    shaper: Option<&mut UiTextShaper>,
    text: &str,
    params: &TextEmitParams,
    glyphs: &mut Vec<GlyphInstance>,
    color_glyphs: &mut Vec<GlyphInstance>,
) {
    if let Some(s) = shaper
        && s.has_face()
    {
        emit_text_via_shaper(atlas, s, text, params, glyphs, color_glyphs);
        return;
    }
    emit_text_legacy_chars(atlas, text, params, glyphs, color_glyphs);
}

fn emit_text_via_shaper(
    atlas: &mut GlyphCache,
    shaper: &mut UiTextShaper,
    text: &str,
    params: &TextEmitParams,
    glyphs: &mut Vec<GlyphInstance>,
    color_glyphs: &mut Vec<GlyphInstance>,
) {
    if !shaper.has_face() {
        emit_text_legacy_chars(atlas, text, params, glyphs, color_glyphs);
        return;
    }
    let shaped = shaper.shape(text);
    let scale = params.scale.max(0.0);
    let mut pen_x = params.x_start;
    for g in &shaped {
        // .notdef from shaper — fall back to ensure_char which uses the
        // full terminal font resolution (including DWrite system fallback
        // for Braille, symbols, etc. that the UI font lacks).
        if g.glyph_id == 0 {
            let byte = g.cluster as usize;
            if byte < text.len() {
                if let Some(ch) = text[byte..].chars().next() {
                    if let Some(entry) = atlas.ensure_char(ch) {
                        if entry.width > 0 && entry.height > 0 {
                            let cw = UnicodeWidthChar::width(ch).unwrap_or(1).max(1);
                            let inst =
                                make_text_glyph_instance(&entry, params, 0, atlas.cell_height, cw);
                            // Reposition to pen_x instead of col-based x, applying
                            // `scale` consistently with the scaled `inst.size` that
                            // `make_text_glyph_instance` already produced.
                            let mut inst = inst;
                            let sx = (pen_x + entry.bearing_x * scale).round();
                            let sy = (params.y + params.baseline * scale - entry.bearing_y * scale)
                                .round();
                            inst.pos = [sx, sy];
                            if entry.is_color {
                                color_glyphs.push(inst);
                            } else {
                                glyphs.push(inst);
                            }
                        }
                    }
                }
            }
            pen_x += g.x_advance * scale;
            continue;
        }
        if let Some(entry) =
            atlas.ensure_ui_glyph_id(g.glyph_id, g.font_id, FontStyle::Regular, false)
            && entry.width > 0
            && entry.height > 0
        {
            let inst = make_shaped_glyph_instance(&entry, params, pen_x, g);
            if entry.is_color {
                color_glyphs.push(inst);
            } else {
                glyphs.push(inst);
            }
        }
        pen_x += g.x_advance * scale;
    }
}

fn emit_text_legacy_chars(
    atlas: &mut GlyphCache,
    text: &str,
    params: &TextEmitParams,
    glyphs: &mut Vec<GlyphInstance>,
    color_glyphs: &mut Vec<GlyphInstance>,
) {
    let mut col = 0usize;
    for ch in text.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if let Some(entry) = atlas.ensure_char(ch)
            && entry.width > 0
            && entry.height > 0
        {
            let inst = make_text_glyph_instance(&entry, params, col, atlas.cell_height, cw.max(1));
            if entry.is_color {
                color_glyphs.push(inst);
            } else {
                glyphs.push(inst);
            }
        }
        col += cw;
    }
}

fn make_shaped_glyph_instance(
    entry: &GlyphEntry,
    params: &TextEmitParams,
    pen_x: f32,
    g: &UiShapedGlyph,
) -> GlyphInstance {
    // Snap to pixel grid — matches the terminal render path in
    // `app::render::make_instance`. DirectWrite bearings and rustybuzz
    // offsets are fractional, and the atlas sampler is LINEAR, so
    // fractional positions blur across texel boundaries.
    let scale = params.scale.max(0.0);
    let sx = (pen_x + (g.x_offset + entry.bearing_x) * scale).round();
    let sy =
        (params.y + params.baseline * scale - entry.bearing_y * scale + g.y_offset * scale).round();
    GlyphInstance {
        pos: [sx, sy],
        size: [entry.width as f32 * scale, entry.height as f32 * scale],
        uv_pos: [entry.u0, entry.v0],
        uv_size: [entry.u1 - entry.u0, entry.v1 - entry.v0],
        color: params.color,
        bg_color: params.bg_color,
    }
}

fn make_text_glyph_instance(
    entry: &GlyphEntry,
    params: &TextEmitParams,
    col: usize,
    cell_height: f32,
    display_cols: usize,
) -> GlyphInstance {
    let scale = params.scale.max(0.0);
    let base_x = params.x_start + col as f32 * params.cell_width * scale;
    if entry.is_color && display_cols > 1 {
        let gw = entry.width as f32;
        let gh = entry.height as f32;
        let target_w = params.cell_width * display_cols as f32 * scale;
        let target_h = cell_height * scale;
        let scale = (target_w / gw).min(target_h / gh);
        let final_w = gw * scale;
        let final_h = gh * scale;
        let offset_x = (target_w - final_w) * 0.5;
        let offset_y = (target_h - final_h) * 0.5;
        GlyphInstance {
            pos: [(base_x + offset_x).round(), (params.y + offset_y).round()],
            size: [final_w, final_h],
            uv_pos: [entry.u0, entry.v0],
            uv_size: [entry.u1 - entry.u0, entry.v1 - entry.v0],
            color: params.color,
            bg_color: params.bg_color,
        }
    } else {
        let sx = (base_x + entry.bearing_x * scale).round();
        let sy = (params.y + params.baseline * scale - entry.bearing_y * scale).round();
        GlyphInstance {
            pos: [sx, sy],
            size: [entry.width as f32 * scale, entry.height as f32 * scale],
            uv_pos: [entry.u0, entry.v0],
            uv_size: [entry.u1 - entry.u0, entry.v1 - entry.v0],
            color: params.color,
            bg_color: params.bg_color,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_entry(is_color: bool, width: u16, height: u16) -> GlyphEntry {
        GlyphEntry {
            u0: 0.0,
            v0: 0.0,
            u1: 0.25,
            v1: 0.25,
            width,
            height,
            bearing_x: 1.0,
            bearing_y: 14.0,
            is_color,
        }
    }

    #[test]
    fn wide_color_glyph_fills_display_cells() {
        let inst = make_text_glyph_instance(
            &test_entry(true, 16, 16),
            &TextEmitParams {
                x_start: 10.0,
                y: 20.0,
                cell_width: 10.0,
                baseline: 16.0,
                color: [1.0; 4],
                bg_color: [0.0; 4],
                scale: 1.0,
            },
            0,
            20.0,
            2,
        );
        assert!((inst.size[0] - 20.0).abs() < 0.01);
        assert!((inst.size[1] - 20.0).abs() < 0.01);
        assert_eq!(inst.pos[0], 10.0);
        assert_eq!(inst.pos[1], 20.0);
    }

    #[test]
    fn alpha_glyph_keeps_bearing_positioning() {
        let inst = make_text_glyph_instance(
            &test_entry(false, 8, 16),
            &TextEmitParams {
                x_start: 5.0,
                y: 7.0,
                cell_width: 10.0,
                baseline: 16.0,
                color: [1.0; 4],
                bg_color: [0.0; 4],
                scale: 1.0,
            },
            3,
            20.0,
            1,
        );
        assert_eq!(inst.pos[0], 36.0);
        assert_eq!(inst.pos[1], 9.0);
        assert_eq!(inst.size, [8.0, 16.0]);
    }

    #[test]
    fn shaped_glyph_places_at_pen_with_offsets() {
        let entry = test_entry(false, 8, 16);
        let inst = make_shaped_glyph_instance(
            &entry,
            &TextEmitParams {
                x_start: 0.0,
                y: 0.0,
                cell_width: 10.0,
                baseline: 14.0,
                color: [1.0; 4],
                bg_color: [0.0; 4],
                scale: 1.0,
            },
            100.0,
            &UiShapedGlyph {
                glyph_id: 1,
                font_id: ciri_render::fontdb::ID::dummy(),
                x_advance: 8.0,
                x_offset: 0.5,
                y_offset: -1.0,
                cluster: 0,
            },
        );
        // pen_x 100 + x_offset 0.5 + bearing_x 1.0 = 101.5 → snapped to 102
        assert_eq!(inst.pos[0], 102.0);
        // y 0 + baseline 14 - bearing_y 14 + y_offset -1 = -1
        assert_eq!(inst.pos[1], -1.0);
    }

    #[test]
    fn shaped_glyph_scales_geometry() {
        let entry = test_entry(false, 8, 16);
        let inst = make_shaped_glyph_instance(
            &entry,
            &TextEmitParams {
                x_start: 0.0,
                y: 10.0,
                cell_width: 10.0,
                baseline: 14.0,
                color: [1.0; 4],
                bg_color: [0.0; 4],
                scale: 1.5,
            },
            20.0,
            &UiShapedGlyph {
                glyph_id: 1,
                font_id: ciri_render::fontdb::ID::dummy(),
                x_advance: 8.0,
                x_offset: 0.0,
                y_offset: 0.0,
                cluster: 0,
            },
        );
        assert_eq!(inst.size, [12.0, 24.0]);
        // pen_x 20 + (x_offset 0 + bearing_x 1.0) * scale 1.5 = 21.5 → snapped to 22
        assert_eq!(inst.pos[0], 22.0);
        assert_eq!(inst.pos[1], 10.0);
    }
}
