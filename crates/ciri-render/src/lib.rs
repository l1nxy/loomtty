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

use glyph_cache::{GlyphInstance, ScissoredRange};
use rect::Rect;
use sdf_rect::SdfRect;

/// All data needed to render a single frame.
/// Shared across all backends.
pub struct FrameScene<'a> {
    pub clear_color: [f32; 4],
    pub bg_rects: &'a [Rect],
    pub glyphs: &'a [GlyphInstance],
    pub color_glyphs: &'a [GlyphInstance],
    pub glyph_batches: &'a [ScissoredRange],
    pub color_glyph_batches: &'a [ScissoredRange],
    /// Index into `bg_rects` where the focused pane begins.
    /// This segment is rendered after non-focused panes so the focused pane stays on top.
    pub active_bg_start: usize,
    pub active_glyph_batches: &'a [ScissoredRange],
    pub active_color_glyph_batches: &'a [ScissoredRange],
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
}
