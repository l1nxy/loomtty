use ciri_render::glyph_cache::{GlyphCache, GlyphInstance};

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_status_text(
    atlas: &mut GlyphCache,
    text: &str,
    x_start: f32,
    text_y: f32,
    cell_width: f32,
    baseline: f32,
    color: [f32; 4],
    glyphs: &mut Vec<GlyphInstance>,
) {
    for (i, ch) in text.chars().enumerate() {
        if let Some(entry) = atlas.ensure_char(ch)
            && entry.width > 0
            && entry.height > 0
        {
            let sx = x_start + i as f32 * cell_width + entry.bearing_x;
            let sy = text_y + baseline - entry.bearing_y;
            glyphs.push(GlyphInstance {
                pos: [sx, sy],
                size: [entry.width as f32, entry.height as f32],
                uv_pos: [entry.u0, entry.v0],
                uv_size: [entry.u1 - entry.u0, entry.v1 - entry.v0],
                color,
            });
        }
    }
}
