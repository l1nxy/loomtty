//! Screen-space row instances, reused while both content and placement agree.

use super::glyph::RelativeGlyph;
use crate::glyph_cache::GlyphInstance;

#[derive(Clone, Copy, PartialEq)]
pub struct GlyphTransform {
    pub origin: [f32; 2],
    pub zoom: f32,
    pub dim: f32,
    pub background: [f32; 4],
}

impl GlyphTransform {
    fn instance(self, g: &RelativeGlyph, color: [f32; 4]) -> Option<GlyphInstance> {
        let width = (g.glyph_w * self.zoom).round();
        let height = (g.glyph_h * self.zoom).round();
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        Some(GlyphInstance {
            pos: [
                (self.origin[0] + g.px * self.zoom).round(),
                (self.origin[1] + g.py * self.zoom).round(),
            ],
            size: [width, height],
            uv_pos: [g.u0, g.v0],
            uv_size: [g.u1 - g.u0, g.v1 - g.v0],
            color,
            bg_color: self.background,
        })
    }
}

#[derive(Clone, Default)]
pub(super) struct RowInstances {
    transform: Option<GlyphTransform>,
    glyphs: Vec<GlyphInstance>,
    color_glyphs: Vec<GlyphInstance>,
}

impl RowInstances {
    pub(super) fn invalidate(&mut self) {
        self.transform = None;
    }

    pub(super) fn get(
        &mut self,
        glyphs: &[RelativeGlyph],
        color_glyphs: &[RelativeGlyph],
        transform: GlyphTransform,
    ) -> (&[GlyphInstance], &[GlyphInstance]) {
        if self.transform != Some(transform) {
            self.glyphs.clear();
            self.color_glyphs.clear();
            self.glyphs.extend(glyphs.iter().filter_map(|g| {
                transform.instance(
                    g,
                    [
                        g.color[0] * transform.dim,
                        g.color[1] * transform.dim,
                        g.color[2] * transform.dim,
                        g.color[3],
                    ],
                )
            }));
            self.color_glyphs
                .extend(color_glyphs.iter().filter_map(|g| {
                    transform.instance(g, [transform.dim, transform.dim, transform.dim, 1.0])
                }));
            self.transform = Some(transform);
        }
        (&self.glyphs, &self.color_glyphs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_focus_theme_and_content_changes_refresh_instances() {
        let mut glyph = RelativeGlyph {
            px: 2.0,
            py: 3.0,
            glyph_w: 4.0,
            glyph_h: 5.0,
            u0: 0.25,
            v0: 0.25,
            u1: 0.5,
            v1: 0.5,
            color: [0.8, 0.6, 0.4, 0.7],
        };
        let mut transform = GlyphTransform {
            origin: [10.0, 20.0],
            zoom: 1.0,
            dim: 1.0,
            background: [0.1, 0.2, 0.3, 1.0],
        };
        let mut cache = RowInstances::default();
        for step in 0..7 {
            match step {
                1 => transform.origin = [30.0, 40.0],
                2 => transform.zoom = 1.5,
                3 => transform.dim = 0.5,
                4 => transform.background = [0.9, 0.8, 0.7, 1.0],
                5 => {
                    glyph.u0 = 0.75;
                    glyph.py += 10.0;
                    cache.invalidate();
                }
                6 => transform.zoom = 0.01,
                _ => {}
            }
            for _ in 0..2 {
                let (text, emoji) = cache.get(&[glyph], &[glyph], transform);
                if step == 6 {
                    assert!(text.is_empty() && emoji.is_empty());
                    continue;
                }
                assert_eq!(text.len(), 1);
                assert_eq!(emoji.len(), 1);
                assert_eq!(
                    text[0].pos,
                    [
                        (transform.origin[0] + glyph.px * transform.zoom).round(),
                        (transform.origin[1] + glyph.py * transform.zoom).round()
                    ]
                );
                assert_eq!(
                    text[0].size,
                    [
                        (glyph.glyph_w * transform.zoom).round(),
                        (glyph.glyph_h * transform.zoom).round()
                    ]
                );
                assert_eq!(
                    text[0].color,
                    [
                        glyph.color[0] * transform.dim,
                        glyph.color[1] * transform.dim,
                        glyph.color[2] * transform.dim,
                        glyph.color[3]
                    ]
                );
                assert_eq!(
                    emoji[0].color,
                    [transform.dim, transform.dim, transform.dim, 1.0]
                );
                assert_eq!(text[0].bg_color, transform.background);
                assert_eq!(text[0].uv_pos, [glyph.u0, glyph.v0]);
                assert_eq!(text[0].uv_size, [glyph.u1 - glyph.u0, glyph.v1 - glyph.v0]);
            }
        }
    }
}
