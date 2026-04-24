//! Glue between `ciri-ui`'s paint output and the client's existing
//! renderer state.
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
use unicode_width::UnicodeWidthChar;

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
///
/// Currently consumed only from the crate's own tests — the next
/// widget migration PR that needs a ciri-ui paint tree in production
/// will wire this into `build_ui`. The per-item `allow(dead_code)`
/// keeps the compiler honest about any *other* unused code in this
/// module (was previously hidden by a file-level allow).
#[allow(dead_code)]
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
    /// Terminal cell height in logical px — used as the measured line
    /// height when no UI shaper has a loaded face. Baseline cannot be
    /// used here: it's the distance from the top of the line to the
    /// text baseline, not the full line height, and doubling it produces
    /// a 50%-too-tall fallback for the common `baseline ≈ 0.75 * cell_h`
    /// configuration.
    pub cell_height: f32,
    /// Logical-px font size the caller treats as "1x" — used to scale
    /// measured / emitted advances by `font_size_px / fallback`. The
    /// caller owns this so the bridge does not reach into ciri-ui's
    /// internal `elements::text::DEFAULT_FONT_SIZE_PX`. Set to the
    /// caller's `typography.md` (or whatever the element tree treats
    /// as "base font size"); mismatches here only drift the pre-font-load
    /// fallback by a small constant factor.
    pub fallback_font_size_px: f32,
}

impl<'a> CiriUiTextShaper for HostTextShaper<'a> {
    fn measure(&mut self, content: &str, font_size_px: f32) -> [f32; 2] {
        let scale = match self.shaper.as_deref() {
            Some(s) if s.has_face() => (font_size_px / s.pixel_size()).max(0.0),
            _ => (font_size_px / self.fallback_font_size_px.max(1e-3)).max(0.0),
        };
        let w = match self.shaper.as_deref_mut() {
            Some(s) if s.has_face() => s.measure(content),
            _ => {
                // Legacy-fallback advance — must mirror
                // `emit_text_legacy_chars`, which steps the pen by
                // `unicode_width` columns (wide chars like CJK count as
                // two cells). Earlier iterations used
                // `chars().count()`, which undercounted CJK runs by ×2
                // and let emitted glyphs spill past the measured box
                // during the brief pre-font-load window.
                let cols: usize = content
                    .chars()
                    .map(|c| UnicodeWidthChar::width(c).unwrap_or(0))
                    .sum();
                self.cell_width * cols.max(1) as f32
            }
        };
        let h = self
            .shaper
            .as_deref()
            .map(|s| s.line_height())
            .unwrap_or(self.cell_height);
        [w * scale, h * scale]
    }

    fn emit(
        &mut self,
        content: &str,
        pos: [f32; 2],
        color: [f32; 4],
        font_size_px: f32,
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
        let scale = match self.shaper.as_deref() {
            Some(s) if s.has_face() => (font_size_px / s.pixel_size()).max(0.0),
            _ => (font_size_px / self.fallback_font_size_px.max(1e-3)).max(0.0),
        };
        let params = TextEmitParams {
            x_start: pos[0],
            y: pos[1],
            cell_width: self.cell_width,
            baseline: self.baseline,
            color,
            scale,
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
#[allow(dead_code)]
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

    #[test]
    fn host_text_shaper_scales_fallback_measure_with_font_size() {
        let config = ciri_config::config::CiriConfig::default();
        let mut cache = ciri_render::glyph_cache::GlyphCache::new(
            &ciri_render::glyph_cache::FontInitParams {
                font_size_pt: 12.0,
                dpi_scale: 1.0,
                family_name: "",
                ui_family_name: None,
                primary_font_path: None,
                emoji_font_path: None,
                emoji_font_id: None,
                cjk_font_path: None,
                cjk_font_id: None,
                ui_font_path: None,
                ui_font_id: None,
                ui_pixel_size: None,
                render_config: &config.render,
                font_resolver: std::sync::Arc::new(
                    ciri_render::font_resolver::CmapResolver::new((&[], 0), None, None),
                ),
                #[cfg(windows)]
                dwrite_resolver: None,
            },
        );
        let mut shaper = HostTextShaper {
            atlas: &mut cache,
            shaper: None,
            cell_width: 8.0,
            baseline: 10.0,
            cell_height: 20.0,
            fallback_font_size_px: 13.0,
        };
        assert_eq!(shaper.measure("abcd", 13.0), [32.0, 20.0]);
        let scaled = shaper.measure("abcd", 26.0);
        assert_eq!(scaled, [64.0, 40.0]);
    }

    /// Regression: the no-face fallback counts display columns (via
    /// `UnicodeWidthChar`), matching what `emit_text_legacy_chars` steps
    /// the pen by. Previously the fallback used `chars().count()`, which
    /// reported half the real width for CJK and let glyphs spill past
    /// measured boxes until the font loaded.
    #[test]
    fn host_text_shaper_fallback_measure_matches_unicode_width_columns() {
        let config = ciri_config::config::CiriConfig::default();
        let mut cache = ciri_render::glyph_cache::GlyphCache::new(
            &ciri_render::glyph_cache::FontInitParams {
                font_size_pt: 12.0,
                dpi_scale: 1.0,
                family_name: "",
                ui_family_name: None,
                primary_font_path: None,
                emoji_font_path: None,
                emoji_font_id: None,
                cjk_font_path: None,
                cjk_font_id: None,
                ui_font_path: None,
                ui_font_id: None,
                ui_pixel_size: None,
                render_config: &config.render,
                font_resolver: std::sync::Arc::new(
                    ciri_render::font_resolver::CmapResolver::new((&[], 0), None, None),
                ),
                #[cfg(windows)]
                dwrite_resolver: None,
            },
        );
        let mut shaper = HostTextShaper {
            atlas: &mut cache,
            shaper: None,
            cell_width: 8.0,
            baseline: 10.0,
            cell_height: 20.0,
            fallback_font_size_px: 13.0,
        };
        // "你好" = 2 chars, 4 columns at wcwidth=2 each → 8 * 4 = 32.
        // A plain `chars().count()` would return 16 here.
        assert_eq!(shaper.measure("你好", 13.0)[0], 32.0);
    }
}
