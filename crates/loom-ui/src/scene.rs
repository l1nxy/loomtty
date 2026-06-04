//! Paint-time accumulators for a single frame.
//!
//! [`Scene`] is the output of painting an `Element` tree: three flat
//! ordered streams of GPU primitives that the caller hands to
//! `loom-render`'s `FrameScene`. Z-order comes purely from emit
//! sequence — non-deferred elements paint in tree order, then the
//! walker drains queued [`crate::elements::Deferred`] subtrees in
//! ascending priority. The host can rely on "later index = on top"
//! without consulting any layer enum.
//!
//! A scene carries three primitive streams: [`SdfRect`] for chrome
//! boxes (rounded / bordered / shadowed), and two glyph streams
//! mirroring `loom-render`'s alpha-mask vs. color-texture split.

pub use loom_render::glyph_cache::GlyphInstance;
pub use loom_render::sdf_rect::SdfRect;

/// A frame's worth of chrome primitives in paint order.
///
/// `Debug` is intentionally not derived: `GlyphInstance` is a GPU-facing
/// POD and doesn't implement `Debug` — dumping thousands of them per
/// frame would not be useful anyway.
#[derive(Clone, Default)]
pub struct Scene {
    sdf_rects: Vec<SdfRect>,
    /// Alpha-mask glyphs (regular text). Atlas layer chosen by the
    /// shaper at emit time.
    glyphs: Vec<GlyphInstance>,
    /// Color-texture glyphs (emoji). Separate stream matching
    /// loom-render's two-atlas split.
    color_glyphs: Vec<GlyphInstance>,
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }

    // ── SDF rects ────────────────────────────────────────────────────

    pub fn push_sdf(&mut self, rect: SdfRect) {
        self.sdf_rects.push(rect);
    }

    pub fn sdf_rects_iter(&self) -> impl Iterator<Item = &SdfRect> {
        self.sdf_rects.iter()
    }

    pub fn sdf_len(&self) -> usize {
        self.sdf_rects.len()
    }

    // ── Alpha glyphs ─────────────────────────────────────────────────

    pub fn push_glyph(&mut self, g: GlyphInstance) {
        self.glyphs.push(g);
    }

    pub fn glyphs_iter(&self) -> impl Iterator<Item = &GlyphInstance> {
        self.glyphs.iter()
    }

    pub fn glyph_len(&self) -> usize {
        self.glyphs.len()
    }

    // ── Color glyphs (emoji) ─────────────────────────────────────────

    pub fn push_color_glyph(&mut self, g: GlyphInstance) {
        self.color_glyphs.push(g);
    }

    pub fn color_glyphs_iter(&self) -> impl Iterator<Item = &GlyphInstance> {
        self.color_glyphs.iter()
    }

    pub fn color_glyph_len(&self) -> usize {
        self.color_glyphs.len()
    }

    // ── Aggregate ────────────────────────────────────────────────────

    /// Total number of emitted primitives across all streams.
    pub fn len(&self) -> usize {
        self.sdf_len() + self.glyph_len() + self.color_glyph_len()
    }

    pub fn is_empty(&self) -> bool {
        self.sdf_rects.is_empty() && self.glyphs.is_empty() && self.color_glyphs.is_empty()
    }

    pub fn clear(&mut self) {
        self.sdf_rects.clear();
        self.glyphs.clear();
        self.color_glyphs.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sdf(r: f32) -> SdfRect {
        SdfRect {
            pos: [r, 0.0],
            size: [1.0, 1.0],
            color: [1.0; 4],
            ..Default::default()
        }
    }

    fn glyph(x: f32) -> GlyphInstance {
        GlyphInstance {
            pos: [x, 0.0],
            size: [8.0, 16.0],
            uv_pos: [0.0; 2],
            uv_size: [0.0; 2],
            color: [1.0; 4],
            bg_color: [0.0; 4],
        }
    }

    #[test]
    fn sdf_round_trip() {
        let mut s = Scene::new();
        s.push_sdf(sdf(1.0));
        s.push_sdf(sdf(2.0));
        let collected: Vec<_> = s.sdf_rects_iter().collect();
        assert_eq!(collected.len(), 2);
        assert_eq!(s.sdf_len(), 2);
    }

    #[test]
    fn glyphs_round_trip_in_emit_order() {
        let mut s = Scene::new();
        s.push_glyph(glyph(2.0));
        s.push_glyph(glyph(1.0));
        let flat: Vec<_> = s.glyphs_iter().collect();
        assert_eq!(flat[0].pos[0], 2.0, "first emitted first");
        assert_eq!(flat[1].pos[0], 1.0, "second emitted second");
    }

    #[test]
    fn color_glyphs_are_separate_from_alpha_glyphs() {
        let mut s = Scene::new();
        s.push_color_glyph(glyph(0.0));
        assert_eq!(s.color_glyph_len(), 1);
        assert_eq!(s.glyph_len(), 0);
    }

    #[test]
    fn len_sums_all_streams() {
        let mut s = Scene::new();
        s.push_sdf(sdf(0.0));
        s.push_glyph(glyph(0.0));
        s.push_color_glyph(glyph(0.0));
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn is_empty_and_clear_cover_all_streams() {
        let mut s = Scene::new();
        s.push_glyph(glyph(0.0));
        assert!(!s.is_empty());
        s.clear();
        assert!(s.is_empty());
    }
}
