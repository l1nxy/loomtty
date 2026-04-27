//! Text-shaping seam between `ciri-ui` and whatever the host renders with.
//!
//! `ciri-ui` doesn't want to depend on the concrete `UiTextShaper` /
//! `GlyphCache` types (they pull in freetype, rasterization, etc.) —
//! only on the contract: "given a string and a font size, tell me how
//! wide/tall it will be, and then produce GlyphInstances at a position".
//!
//! The host (currently `ciri-app`) implements this trait by wrapping its
//! real shaper + atlas; unit tests inside `ciri-ui` use [`NullShaper`]
//! which measures with a fixed advance heuristic and emits nothing.

use crate::color::Color;
use crate::scene::Scene;

/// Bridge trait for measuring + shaping UI chrome text.
///
/// Implementations are expected to be fast (called per-frame) and
/// internally caching — the existing `UiTextShaper` already shapes runs
/// on first use and keeps an in-memory cache.
pub trait TextShaper {
    /// Return the `[width, height]` in logical pixels that `content`
    /// would occupy at `font_size_px`. Used by Taffy's layout pass; must
    /// not produce glyphs, mutate an atlas, or allocate in the hot path.
    fn measure(&mut self, content: &str, font_size_px: f32) -> [f32; 2];

    /// Emit glyph instances for `content` anchored at `pos` (top-left),
    /// in `color`, at `font_size_px`. Glyphs are appended to the
    /// provided `scene` in emit order, partitioned by alpha vs color
    /// atlas. Z-order = emit order; the walker arranges that for the
    /// caller via tree-order paint plus deferred draining.
    ///
    /// `pos` is in logical pixels, in the same space as `SdfRect.pos`.
    fn emit(
        &mut self,
        content: &str,
        pos: [f32; 2],
        color: Color,
        font_size_px: f32,
        scene: &mut Scene,
    );
}

/// No-op shaper for tests and headless / stub contexts.
///
/// `measure` returns a heuristic width (6 logical px per character) and
/// the font size as height. `emit` is a silent no-op — useful when a
/// test exercises layout behaviour but doesn't care about pixel output.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullShaper;

/// Heuristic advance used by [`NullShaper`]. Matches the historical
/// "roughly 6px/char at 12px" UI font estimate.
const NULL_ADVANCE_RATIO: f32 = 0.5;

impl TextShaper for NullShaper {
    fn measure(&mut self, content: &str, font_size_px: f32) -> [f32; 2] {
        let advance = font_size_px * NULL_ADVANCE_RATIO;
        let w = advance * content.chars().count() as f32;
        [w, font_size_px]
    }

    fn emit(
        &mut self,
        _content: &str,
        _pos: [f32; 2],
        _color: Color,
        _font_size_px: f32,
        _scene: &mut Scene,
    ) {
        // No glyphs — tests using NullShaper assert on SdfRects /
        // layout only. Real emission comes from the host-provided shaper.
    }
}

/// Count-everything shaper for tests that need to verify `emit` was
/// called with the right arguments without linking to a real shaper.
/// Records content + position for every emit; tests inspect `calls`.
#[cfg(test)]
#[derive(Default)]
pub struct RecordingShaper {
    pub calls: Vec<RecordedShape>,
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub struct RecordedShape {
    pub content: String,
    pub pos: [f32; 2],
    pub color: Color,
    pub font_size_px: f32,
}

#[cfg(test)]
impl TextShaper for RecordingShaper {
    fn measure(&mut self, content: &str, font_size_px: f32) -> [f32; 2] {
        NullShaper.measure(content, font_size_px)
    }

    fn emit(
        &mut self,
        content: &str,
        pos: [f32; 2],
        color: Color,
        font_size_px: f32,
        _scene: &mut Scene,
    ) {
        self.calls.push(RecordedShape {
            content: content.to_string(),
            pos,
            color,
            font_size_px,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_measure_scales_with_content() {
        let mut s = NullShaper;
        let short = s.measure("ab", 12.0);
        let long = s.measure("abcdef", 12.0);
        assert!(long[0] > short[0]);
        assert_eq!(short[1], 12.0);
    }

    #[test]
    fn null_emit_is_silent() {
        let mut s = NullShaper;
        let mut scene = Scene::new();
        s.emit("hi", [0.0, 0.0], [1.0; 4], 12.0, &mut scene);
        assert!(scene.is_empty());
    }

    #[test]
    fn recording_captures_emit_call() {
        let mut r = RecordingShaper::default();
        let mut scene = Scene::new();
        r.emit(
            "hello",
            [10.0, 20.0],
            [1.0, 0.0, 0.0, 1.0],
            13.0,
            &mut scene,
        );
        assert_eq!(r.calls.len(), 1);
        assert_eq!(r.calls[0].content, "hello");
        assert_eq!(r.calls[0].pos, [10.0, 20.0]);
        assert_eq!(r.calls[0].font_size_px, 13.0);
    }
}
