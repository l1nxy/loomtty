pub mod font_list;
pub mod font_resolver;
pub mod glyph_cache;
pub mod rect;
pub mod sdf_rect;
pub mod shaper;
#[cfg(target_os = "macos")]
mod shaper_coretext;
pub mod terminal;
pub mod ui_shaper;

pub use fontdb;

use glyph_cache::{GlyphInstance, PaneGlyphRange};
use rect::{PaneRectRange, Rect};
use sdf_rect::SdfRect;

/// All data needed to render a single frame.
/// Shared across all backends.
pub struct FrameScene<'a> {
    pub clear_color: [f32; 4],
    /// Opacity (0..1) of the background image drawn after the clear and
    /// before anything else. `0.0` skips the draw entirely (so backends pay
    /// no cost when no image is configured or the user dialed dim to 1.0).
    /// The renderer also skips the draw if no image has been uploaded via
    /// `Renderer::set_background_image`. `1.0` shows the image at full
    /// strength; values in between blend toward `clear_color` via standard
    /// alpha (premultiplied output `(rgb*a, a)` over the cleared
    /// framebuffer).
    pub background_image_opacity: f32,
    pub bg_rects: &'a [Rect],
    pub bg_rect_ranges: &'a [PaneRectRange],
    pub glyphs: &'a [GlyphInstance],
    pub color_glyphs: &'a [GlyphInstance],
    pub glyph_batches: &'a [PaneGlyphRange],
    pub color_glyph_batches: &'a [PaneGlyphRange],
    /// Index into `bg_rects` where the focused pane begins.
    /// This segment is rendered after non-focused panes so the focused pane stays on top.
    pub active_bg_start: usize,
    pub active_glyph_batches: &'a [PaneGlyphRange],
    pub active_color_glyph_batches: &'a [PaneGlyphRange],
    pub pane_glyph_end: usize,
    pub pane_color_glyph_end: usize,
    /// Index into `bg_rects` where overlay rects begin.
    /// Overlay rects are rendered after pane glyphs so they occlude terminal text.
    pub overlay_bg_start: usize,
    /// SDF-rendered chrome rectangles (rounded corners / border / shadow).
    /// Drawn after flat overlay bgs and before overlay glyphs, so chrome
    /// text stays crisply on top of its rounded panel.
    ///
    /// Backends that haven't implemented the SDF pass silently skip this
    /// slice — flat chrome still renders via `bg_rects`, so no widget
    /// disappears while support rolls out.
    pub sdf_rects: &'a [SdfRect],
    /// Index in `sdf_rects` where the cached chrome's Base layer ends and
    /// the Overlay layer (palette / context_menu, etc.) begins. Backends
    /// that issue a single combined draw can ignore this; the layered
    /// renderer splits the SDF + glyph passes here so popup rects can
    /// occlude base-layer glyphs (settings_panel labels, top_bar text)
    /// instead of sitting beneath them.
    pub chrome_base_sdf_end: usize,
    /// Indices in `glyphs` / `color_glyphs` where the Base chrome layer
    /// ends. The slice between `pane_*_glyph_end` and these is the Base
    /// chrome's text; the slice from these to the end is Overlay +
    /// transient text (drawn after the Overlay SDF rects).
    pub chrome_base_alpha_glyph_end: usize,
    pub chrome_base_color_glyph_end: usize,
}
