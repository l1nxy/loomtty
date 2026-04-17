//! Glue between `ciri-ui`'s paint output and the client's existing
//! renderer state.
#![allow(dead_code)] // wired in by the first widget migration PR
//!
//! `ciri-ui` runs Taffy layout, walks its Element tree, and produces a
//! `ciri_ui::Scene` of SdfRects + alpha/color glyph instances — all
//! keyed by semantic `Layer`. This module is what lets the client's
//! `App::render` actually consume that output:
//!
//! - [`HostTextShaper`] implements `ciri_ui::TextShaper` by delegating
//!   to the existing `emit_status_text` pipeline (UiTextShaper +
//!   GlyphCache + atlas fallback). That means migrated widgets produce
//!   glyphs through exactly the same path as legacy `UiComponent`s,
//!   sharing the shape cache and atlas.
//! - [`merge_ui_scene`] appends the scene's flat paint-order output
//!   into the accumulators that `FrameScene` is built from — one call
//!   per painted ciri-ui tree.
//!
//! No App state is touched in this module: consumers instantiate a
//! `HostTextShaper` per-frame (cheap — just borrows) and call
//! `merge_ui_scene` afterwards. Wiring into `App::build_ui` lands with
//! the first widget migration.

use ciri_render::glyph_cache::{GlyphCache, GlyphInstance};
use ciri_render::sdf_rect::SdfRect;
use ciri_render::ui_shaper::UiTextShaper;
use ciri_ui::{Layer, Scene, TextShaper as CiriUiTextShaper};

use crate::app::status_bar::{emit_status_text, TextEmitParams};

/// Bridge implementing [`ciri_ui::TextShaper`] on top of the client's
/// existing `UiTextShaper` + `GlyphCache` pair.
///
/// Borrowed for the duration of a single `paint_tree` call — the caller
/// assembles one inline from their mutable atlas + optional shaper
/// mutable borrow, rather than storing a long-lived struct. The
/// `RefCell<UiTextShaper>` held by `App` already requires callers to
/// arrange the borrow order (see `UiBuilder::abs_text` for the
/// canonical borrow-then-release pattern); this adapter matches that.
pub(crate) struct HostTextShaper<'a> {
    pub atlas: &'a mut GlyphCache,
    pub shaper: Option<&'a mut UiTextShaper>,
    /// Terminal cell width in logical px — used as the legacy-fallback
    /// advance when no UI shaper has a loaded face.
    pub cell_width: f32,
    /// UI baseline in logical px from the frame's `UiContext`. The
    /// existing emit pipeline expects this (it offsets each glyph from
    /// the run's top-left to the baseline).
    pub baseline: f32,
}

impl<'a> CiriUiTextShaper for HostTextShaper<'a> {
    /// Measure returns the shaped width + line height of `content`.
    ///
    /// `font_size_px` is currently ignored: the real `UiTextShaper`
    /// carries its own pixel size from when it was constructed (one
    /// global UI font size for chrome). If that ever becomes multi-size
    /// we'll need to thread a size-parameterised shape cache through.
    fn measure(&mut self, content: &str, _font_size_px: f32) -> [f32; 2] {
        let w = match self.shaper.as_deref_mut() {
            Some(s) if s.has_face() => s.measure(content),
            _ => self.cell_width * content.chars().count() as f32,
        };
        let h = self
            .shaper
            .as_deref()
            .map(|s| s.line_height())
            .unwrap_or(self.baseline * 2.0);
        [w, h]
    }

    fn emit(
        &mut self,
        content: &str,
        pos: [f32; 2],
        color: [f32; 4],
        _font_size_px: f32,
        layer: Layer,
        scene: &mut Scene,
    ) {
        if content.is_empty() {
            return;
        }
        // Buffer into temporary vecs so we can route each glyph into
        // the scene's per-layer buckets afterwards. Chrome text runs
        // are short (a few words per label), so the per-frame cost is
        // well under a microsecond for realistic UIs.
        let mut alpha_buf = Vec::new();
        let mut color_buf = Vec::new();
        let params = TextEmitParams {
            x_start: pos[0],
            y: pos[1],
            cell_width: self.cell_width,
            baseline: self.baseline,
            color,
        };
        emit_status_text(
            self.atlas,
            self.shaper.as_deref_mut(),
            content,
            &params,
            &mut alpha_buf,
            &mut color_buf,
        );
        for g in alpha_buf {
            scene.push_glyph(layer, g);
        }
        for g in color_buf {
            scene.push_color_glyph(layer, g);
        }
    }
}

/// Append a freshly-painted ciri-ui scene into the client's accumulator
/// Vecs for the current frame.
///
/// The scene's flat paint-order iterators already honour the `Layer`
/// z-order (Chrome → Sidebar → Overlay → Modal → Tooltip), so this is
/// a single `extend` per stream. The caller is expected to have
/// already emitted all legacy-widget chrome into these same vecs;
/// ciri-ui output lands on top of that, which matches its "chrome is
/// above pane content" semantics.
pub(crate) fn merge_ui_scene(
    ui: &Scene,
    sdf_out: &mut Vec<SdfRect>,
    glyph_out: &mut Vec<GlyphInstance>,
    color_glyph_out: &mut Vec<GlyphInstance>,
) {
    sdf_out.extend(ui.sdf_rects_iter().copied());
    glyph_out.extend(ui.glyphs_iter().copied());
    color_glyph_out.extend(ui.color_glyphs_iter().copied());
}

#[cfg(test)]
mod tests {
    use super::*;
    use ciri_ui::{div, Styled};

    #[test]
    fn merge_ui_scene_preserves_paint_order() {
        // Chrome sibling first (green), Modal sibling second (red) in the
        // element tree — scene.sdf_rects() must flatten Chrome before
        // Modal, and merge_ui_scene must preserve that order verbatim.
        use ciri_ui::paint_tree;
        use ciri_ui::shaper::NullShaper;
        use ciri_ui::ResolvedTheme;

        let theme = ResolvedTheme::default();
        let root = div()
            .w(200.0)
            .h(100.0)
            .child(
                div()
                    .in_layer(Layer::Modal)
                    .w(50.0)
                    .h(50.0)
                    .bg([1.0, 0.0, 0.0, 1.0]),
            )
            .child(div().w(50.0).h(50.0).bg([0.0, 1.0, 0.0, 1.0]));
        let scene = paint_tree(&root, &theme, [200.0, 100.0], 1.0, &mut NullShaper);

        let mut sdf = Vec::new();
        let mut g = Vec::new();
        let mut cg = Vec::new();
        merge_ui_scene(&scene, &mut sdf, &mut g, &mut cg);

        assert_eq!(sdf.len(), 2);
        assert_eq!(sdf[0].color, [0.0, 1.0, 0.0, 1.0], "chrome first");
        assert_eq!(sdf[1].color, [1.0, 0.0, 0.0, 1.0], "modal second");
        assert!(g.is_empty());
        assert!(cg.is_empty());
    }

    #[test]
    fn merge_appends_rather_than_replaces() {
        // Existing legacy chrome must survive the merge: ciri-ui output
        // lands on top, not in place of. Simulate a pre-existing SdfRect
        // and verify it stays at index 0 after the merge.
        use ciri_ui::paint_tree;
        use ciri_ui::shaper::NullShaper;
        use ciri_ui::ResolvedTheme;

        let theme = ResolvedTheme::default();
        let scene = paint_tree(
            &div().w(10.0).h(10.0).bg([1.0, 0.0, 0.0, 1.0]),
            &theme,
            [100.0, 100.0],
            1.0,
            &mut NullShaper,
        );
        let mut sdf = vec![SdfRect {
            pos: [50.0, 50.0],
            size: [5.0, 5.0],
            color: [0.0, 0.0, 1.0, 1.0],
            ..Default::default()
        }];
        let mut g = Vec::new();
        let mut cg = Vec::new();
        merge_ui_scene(&scene, &mut sdf, &mut g, &mut cg);
        assert_eq!(sdf.len(), 2);
        assert_eq!(sdf[0].color, [0.0, 0.0, 1.0, 1.0], "legacy rect survives");
    }
}
