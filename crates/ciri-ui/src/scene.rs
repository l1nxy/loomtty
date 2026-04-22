//! Paint-time accumulators for a single frame.
//!
//! [`Scene`] is the output of painting an `Element` tree: per-layer
//! buckets of emitted GPU primitives that the caller hands to
//! `ciri-render`'s `FrameScene`. Layers are paint-ordered — `Chrome`
//! first, `Tooltip` last — so z-order is driven by the semantic
//! classification of each element, not its position in the tree.
//!
//! A scene carries three primitive streams: [`SdfRect`] for chrome
//! boxes (rounded / bordered / shadowed), and two glyph streams
//! mirroring `ciri-render`'s alpha-mask vs. color-texture split. All
//! three are per-layer so a `Modal` text label sits on top of `Chrome`
//! boxes regardless of tree order.

use crate::element::Layer;
pub use ciri_render::glyph_cache::GlyphInstance;
pub use ciri_render::sdf_rect::SdfRect;

/// Total number of `Layer` variants. Keep in sync with the enum.
pub(crate) const LAYER_COUNT: usize = 5;

const _: () = {
    // Compile-time guarantee that the Layer enum fits in the per-layer
    // array storage used by Scene. If a new variant is added and this
    // assertion trips, bump LAYER_COUNT and extend the arrays.
    assert!(Layer::Tooltip as usize + 1 == LAYER_COUNT);
};

/// Per-layer accumulators for one primitive stream.
type LayerBuckets<T> = [Vec<T>; LAYER_COUNT];

/// A frame's worth of chrome primitives, bucketed by [`Layer`].
///
/// `Debug` is intentionally not derived: `GlyphInstance` is a GPU-facing
/// POD and doesn't implement `Debug` — dumping thousands of them per
/// frame would not be useful anyway.
#[derive(Clone, Default)]
pub struct Scene {
    sdf_rects: LayerBuckets<SdfRect>,
    /// Alpha-mask glyphs (regular text). Atlas layer chosen by the
    /// shaper at emit time.
    glyphs: LayerBuckets<GlyphInstance>,
    /// Color-texture glyphs (emoji). Separate stream matching
    /// ciri-render's two-atlas split.
    color_glyphs: LayerBuckets<GlyphInstance>,
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }

    // ── SDF rects ────────────────────────────────────────────────────

    pub fn push_sdf(&mut self, layer: Layer, rect: SdfRect) {
        self.sdf_rects[layer as usize].push(rect);
    }

    pub fn sdf_in_layer(&self, layer: Layer) -> &[SdfRect] {
        &self.sdf_rects[layer as usize]
    }

    #[deprecated(
        note = "allocates a Vec every call — prefer `sdf_rects_iter()` on hot paths"
    )]
    pub fn sdf_rects(&self) -> Vec<SdfRect> {
        let mut out = Vec::with_capacity(self.sdf_len());
        for bucket in &self.sdf_rects {
            out.extend_from_slice(bucket);
        }
        out
    }

    pub fn sdf_rects_iter(&self) -> impl Iterator<Item = &SdfRect> {
        self.sdf_rects.iter().flat_map(|v| v.iter())
    }

    pub fn sdf_len(&self) -> usize {
        self.sdf_rects.iter().map(Vec::len).sum()
    }

    // ── Alpha glyphs ─────────────────────────────────────────────────

    pub fn push_glyph(&mut self, layer: Layer, g: GlyphInstance) {
        self.glyphs[layer as usize].push(g);
    }

    pub fn glyphs_in_layer(&self, layer: Layer) -> &[GlyphInstance] {
        &self.glyphs[layer as usize]
    }

    #[deprecated(
        note = "allocates a Vec every call — prefer `glyphs_iter()` on hot paths"
    )]
    pub fn glyphs(&self) -> Vec<GlyphInstance> {
        let mut out = Vec::with_capacity(self.glyph_len());
        for bucket in &self.glyphs {
            out.extend_from_slice(bucket);
        }
        out
    }

    pub fn glyphs_iter(&self) -> impl Iterator<Item = &GlyphInstance> {
        self.glyphs.iter().flat_map(|v| v.iter())
    }

    pub fn glyph_len(&self) -> usize {
        self.glyphs.iter().map(Vec::len).sum()
    }

    // ── Color glyphs (emoji) ─────────────────────────────────────────

    pub fn push_color_glyph(&mut self, layer: Layer, g: GlyphInstance) {
        self.color_glyphs[layer as usize].push(g);
    }

    pub fn color_glyphs_in_layer(&self, layer: Layer) -> &[GlyphInstance] {
        &self.color_glyphs[layer as usize]
    }

    #[deprecated(
        note = "allocates a Vec every call — prefer `color_glyphs_iter()` on hot paths"
    )]
    pub fn color_glyphs(&self) -> Vec<GlyphInstance> {
        let mut out = Vec::with_capacity(self.color_glyph_len());
        for bucket in &self.color_glyphs {
            out.extend_from_slice(bucket);
        }
        out
    }

    pub fn color_glyphs_iter(&self) -> impl Iterator<Item = &GlyphInstance> {
        self.color_glyphs.iter().flat_map(|v| v.iter())
    }

    pub fn color_glyph_len(&self) -> usize {
        self.color_glyphs.iter().map(Vec::len).sum()
    }

    // ── Aggregate ────────────────────────────────────────────────────

    /// Total number of emitted primitives across all streams.
    pub fn len(&self) -> usize {
        self.sdf_len() + self.glyph_len() + self.color_glyph_len()
    }

    pub fn is_empty(&self) -> bool {
        self.sdf_len() == 0 && self.glyph_len() == 0 && self.color_glyph_len() == 0
    }

    pub fn clear(&mut self) {
        for bucket in &mut self.sdf_rects {
            bucket.clear();
        }
        for bucket in &mut self.glyphs {
            bucket.clear();
        }
        for bucket in &mut self.color_glyphs {
            bucket.clear();
        }
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
        }
    }

    #[test]
    fn sdf_round_trip_by_layer() {
        let mut s = Scene::new();
        s.push_sdf(Layer::Chrome, sdf(1.0));
        s.push_sdf(Layer::Modal, sdf(2.0));
        assert_eq!(s.sdf_in_layer(Layer::Chrome).len(), 1);
        assert_eq!(s.sdf_in_layer(Layer::Modal).len(), 1);
        assert_eq!(s.sdf_len(), 2);
    }

    #[test]
    #[allow(deprecated)] // intentionally covers the deprecated flatten path
    fn glyphs_paint_order_flatten() {
        let mut s = Scene::new();
        s.push_glyph(Layer::Modal, glyph(2.0));
        s.push_glyph(Layer::Chrome, glyph(1.0));
        let flat = s.glyphs();
        assert_eq!(flat[0].pos[0], 1.0, "chrome first");
        assert_eq!(flat[1].pos[0], 2.0, "modal second");
    }

    #[test]
    fn color_glyphs_are_separate_from_alpha_glyphs() {
        let mut s = Scene::new();
        s.push_color_glyph(Layer::Chrome, glyph(0.0));
        assert_eq!(s.color_glyph_len(), 1);
        assert_eq!(s.glyph_len(), 0);
    }

    #[test]
    fn len_sums_all_streams() {
        let mut s = Scene::new();
        s.push_sdf(Layer::Chrome, sdf(0.0));
        s.push_glyph(Layer::Chrome, glyph(0.0));
        s.push_color_glyph(Layer::Chrome, glyph(0.0));
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn is_empty_and_clear_cover_all_streams() {
        let mut s = Scene::new();
        s.push_glyph(Layer::Chrome, glyph(0.0));
        assert!(!s.is_empty());
        s.clear();
        assert!(s.is_empty());
    }
}
