use ciri_render::glyph_cache::{GlyphCache, GlyphEntry, GlyphInstance};
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

fn make_text_glyph_instance(
    entry: &GlyphEntry,
    params: &TextEmitParams,
    col: usize,
    cell_height: f32,
    display_cols: usize,
) -> GlyphInstance {
    let base_x = params.x_start + col as f32 * params.cell_width;
    if entry.is_color && display_cols > 1 {
        let gw = entry.width as f32;
        let gh = entry.height as f32;
        let target_w = params.cell_width * display_cols as f32;
        let target_h = cell_height;
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
        }
    } else {
        let sx = base_x + entry.bearing_x;
        let sy = params.y + params.baseline - entry.bearing_y;
        GlyphInstance {
            pos: [sx, sy],
            size: [entry.width as f32, entry.height as f32],
            uv_pos: [entry.u0, entry.v0],
            uv_size: [entry.u1 - entry.u0, entry.v1 - entry.v0],
            color: params.color,
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
            },
            3,
            20.0,
            1,
        );
        assert_eq!(inst.pos[0], 36.0);
        assert_eq!(inst.pos[1], 9.0);
        assert_eq!(inst.size, [8.0, 16.0]);
    }
}
