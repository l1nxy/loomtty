use ciri_render::glyph_cache::{GlyphCache, GlyphInstance};
use unicode_width::UnicodeWidthChar;

pub(crate) struct TextEmitParams {
    pub x_start: f32,
    pub y: f32,
    pub cell_width: f32,
    pub baseline: f32,
    pub color: [f32; 4],
}

pub(crate) fn emit_status_text(
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
            let sx = params.x_start + col as f32 * params.cell_width + entry.bearing_x;
            let sy = params.y + params.baseline - entry.bearing_y;
            let inst = GlyphInstance {
                pos: [sx, sy],
                size: [entry.width as f32, entry.height as f32],
                uv_pos: [entry.u0, entry.v0],
                uv_size: [entry.u1 - entry.u0, entry.v1 - entry.v0],
                color: params.color,
            };
            if entry.is_color {
                color_glyphs.push(inst);
            } else {
                glyphs.push(inst);
            }
        }
        col += cw;
    }
}
