//! Paint-time accumulators for a single frame.
//!
//! [`Scene`] is the output of painting an `Element` tree: it owns the
//! emitted GPU primitives and is handed to `ciri-render`'s `FrameScene`
//! construction at the callsite that draws a frame.
//!
//! Foundation scope: only `sdf_rects` are populated today. Glyph
//! accumulators and a clip stack arrive with the text-shaper integration
//! in the follow-up PR.

pub use ciri_render::sdf_rect::SdfRect;

/// A frame's worth of chrome primitives, emitted by `paint_tree`.
#[derive(Default, Clone, Debug)]
pub struct Scene {
    /// SDF chrome rects (rounded corners / border / shadow). Appended in
    /// paint order (back-to-front), which matches the GPU blend order.
    pub sdf_rects: Vec<SdfRect>,
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.sdf_rects.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.sdf_rects.is_empty()
    }

    pub fn len(&self) -> usize {
        self.sdf_rects.len()
    }
}
