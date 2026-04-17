//! Paint-time accumulators for a single frame.
//!
//! [`Scene`] is the output of painting an `Element` tree: per-layer
//! buckets of emitted GPU primitives that the caller hands to
//! `ciri-render`'s `FrameScene`. Layers are paint-ordered — `Chrome`
//! first, `Tooltip` last — so z-order is driven by the semantic
//! classification of each element, not its position in the tree.
//!
//! Foundation scope: only `SdfRect` buckets are populated today. Glyph
//! accumulators and a clip stack arrive with the text-shaper integration.

use crate::element::Layer;
pub use ciri_render::sdf_rect::SdfRect;

/// Total number of `Layer` variants. Keep in sync with the enum.
pub(crate) const LAYER_COUNT: usize = 5;

const _: () = {
    // Compile-time guarantee that the Layer enum fits in the per-layer
    // array storage used by Scene. If a new variant is added and this
    // assertion trips, bump LAYER_COUNT and extend the array.
    assert!(Layer::Tooltip as usize + 1 == LAYER_COUNT);
};

/// A frame's worth of chrome primitives, bucketed by [`Layer`].
///
/// Callers that need an unlayered sequence use [`Scene::sdf_rects`];
/// callers that need a specific layer (e.g. to drive a per-layer GPU
/// pass) use [`Scene::layer`].
#[derive(Clone, Debug, Default)]
pub struct Scene {
    layers: [Vec<SdfRect>; LAYER_COUNT],
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push an SDF rect into its owning layer's bucket. The walker
    /// resolves the effective layer for every element before calling
    /// this, honouring `in_layer()` overrides and inheritance.
    pub fn push_sdf(&mut self, layer: Layer, rect: SdfRect) {
        self.layers[layer as usize].push(rect);
    }

    /// Read-only slice of one layer. Order inside a layer is paint order.
    pub fn layer(&self, layer: Layer) -> &[SdfRect] {
        &self.layers[layer as usize]
    }

    /// All SDF rects flattened in paint order: `Chrome` → `Sidebar` →
    /// `Overlay` → `Modal` → `Tooltip`. Allocates a fresh `Vec` — use
    /// [`Scene::sdf_rects_iter`] if zero-alloc iteration is fine.
    pub fn sdf_rects(&self) -> Vec<SdfRect> {
        let mut out = Vec::with_capacity(self.len());
        for bucket in &self.layers {
            out.extend_from_slice(bucket);
        }
        out
    }

    /// Paint-order iterator over every SDF rect. Skips empty layers
    /// implicitly (by yielding nothing from them).
    pub fn sdf_rects_iter(&self) -> impl Iterator<Item = &SdfRect> {
        self.layers.iter().flat_map(|v| v.iter())
    }

    pub fn len(&self) -> usize {
        self.layers.iter().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.layers.iter().all(Vec::is_empty)
    }

    pub fn clear(&mut self) {
        for bucket in &mut self.layers {
            bucket.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy(r: f32) -> SdfRect {
        SdfRect {
            pos: [r, 0.0],
            size: [1.0, 1.0],
            color: [1.0; 4],
            ..Default::default()
        }
    }

    #[test]
    fn push_and_read_by_layer() {
        let mut s = Scene::new();
        s.push_sdf(Layer::Chrome, dummy(1.0));
        s.push_sdf(Layer::Modal, dummy(2.0));
        assert_eq!(s.layer(Layer::Chrome).len(), 1);
        assert_eq!(s.layer(Layer::Modal).len(), 1);
        assert_eq!(s.layer(Layer::Overlay).len(), 0);
    }

    #[test]
    fn sdf_rects_flatten_in_paint_order() {
        let mut s = Scene::new();
        // Push Modal first then Chrome so any naive tree-order bug would
        // surface here as Modal appearing before Chrome.
        s.push_sdf(Layer::Modal, dummy(2.0));
        s.push_sdf(Layer::Chrome, dummy(1.0));
        let flat = s.sdf_rects();
        assert_eq!(flat[0].pos[0], 1.0, "chrome must come first");
        assert_eq!(flat[1].pos[0], 2.0, "modal must come after chrome");
    }

    #[test]
    fn is_empty_and_clear() {
        let mut s = Scene::new();
        assert!(s.is_empty());
        s.push_sdf(Layer::Chrome, dummy(0.0));
        assert!(!s.is_empty());
        s.clear();
        assert!(s.is_empty());
    }
}
