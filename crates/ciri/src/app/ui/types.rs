use ciri_render::glyph_cache::{GlyphCache, GlyphInstance};
use ciri_render::sdf_rect::SdfRect;
use ciri_render::ui_shaper::UiTextShaper;
use std::cell::RefCell;
use winit::window::CursorIcon;

/// A pixel-aligned axis-aligned rectangle used to describe UI slots.
///
/// Kept separate from `ciri_render::rect::Rect` (which is a coloured GPU quad)
/// because this type has no colour and no side effects.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct UiRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl UiRect {
    #[allow(dead_code)]
    pub const ZERO: UiRect = UiRect {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    };

    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    #[inline]
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    #[inline]
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    #[inline]
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    /// Slice a strip off the left edge; returns (taken, remaining).
    /// If `width >= self.w`, `remaining` is empty.
    #[allow(dead_code)]
    pub fn split_left(self, width: f32) -> (UiRect, UiRect) {
        let w = width.clamp(0.0, self.w);
        (
            UiRect::new(self.x, self.y, w, self.h),
            UiRect::new(self.x + w, self.y, self.w - w, self.h),
        )
    }

    /// Slice a strip off the right edge; returns (remaining, taken).
    #[allow(dead_code)]
    pub fn split_right(self, width: f32) -> (UiRect, UiRect) {
        let w = width.clamp(0.0, self.w);
        (
            UiRect::new(self.x, self.y, self.w - w, self.h),
            UiRect::new(self.x + self.w - w, self.y, w, self.h),
        )
    }

    /// Slice a strip off the top edge; returns (taken, remaining).
    #[allow(dead_code)]
    pub fn split_top(self, height: f32) -> (UiRect, UiRect) {
        let h = height.clamp(0.0, self.h);
        (
            UiRect::new(self.x, self.y, self.w, h),
            UiRect::new(self.x, self.y + h, self.w, self.h - h),
        )
    }

    /// Slice a strip off the bottom edge; returns (remaining, taken).
    #[allow(dead_code)]
    pub fn split_bottom(self, height: f32) -> (UiRect, UiRect) {
        let h = height.clamp(0.0, self.h);
        (
            UiRect::new(self.x, self.y, self.w, self.h - h),
            UiRect::new(self.x, self.y + self.h - h, self.w, h),
        )
    }
}

pub(crate) struct UiContext<'a> {
    pub config: &'a ciri_config::config::CiriConfig,
    /// Pre-resolved theme tokens — paint paths should read colors via
    /// `cx.theme.<token>` rather than hex-parsing `cx.config.theme.*`
    /// every frame. Kept alongside `config` during the migration so
    /// legacy call sites can still reach the raw hex strings.
    pub theme: &'a ciri_ui::ResolvedTheme,
    pub viewport_w: f32,
    pub viewport_h: f32,
    pub cell_w: f32,
    pub cell_h: f32,
    pub baseline: f32,
    /// Line height of the UI font in pixels. Derived from the UI shaper's
    /// font metrics (ascent − descent + line gap). Falls back to `cell_h`
    /// when no UI shaper is available.
    pub ui_line_h: f32,
    /// UI text shaper (advance-based) used by all UI chrome text. `None` only
    /// in tests or before the renderer has been set up — callers must then
    /// use `cell_w`-based measurement as a fallback.
    pub ui_shaper: Option<&'a RefCell<UiTextShaper>>,
}

pub(crate) struct UiScene<'a> {
    pub atlas: &'a mut GlyphCache,
    pub glyphs: &'a mut Vec<GlyphInstance>,
    pub color_glyphs: &'a mut Vec<GlyphInstance>,
    /// SDF rounded-chrome primitives. Emitted by components that want
    /// rounded corners, shadows, or AA'd borders without building a full
    /// ciri-ui Element tree; paired 1:1 with the blade SDF pipeline.
    pub sdf_rects: &'a mut Vec<SdfRect>,
}

pub(crate) struct UiHoverOutcome {
    pub handled: bool,
    pub cursor: CursorIcon,
    pub needs_redraw: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UiAction {
    OpenSessionPalette,
    ToggleOverview,
    CycleWorkspace,
    FocusPaneTab(u64),
    /// Close the pane backing a tab (middle-click). Sent as
    /// `ClientMessage::ClosePane` to the server, same as the overview's
    /// close-icon path and the context menu's "Close pane" entry.
    ClosePaneTab(u64),
    ExecutePaletteEntry(usize),
    ClosePalette,
    ExecuteContextMenuEntry(usize),
    CloseContextMenu,
    ConfirmPaste,
    CancelPaste,
    FocusOverviewPane(usize, u64),
    CloseOverviewPane(u64),
    StartOverviewDrag,
}

// Per-component hit enums — kept internal, used only within each component's
// click()/hover() implementation to convert to UiAction.

#[derive(Debug, PartialEq, Eq)]
pub(super) enum UiTopBarHit {
    Session,
    Workspace,
    Mode,
    PaneTab(u64),
    Background,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum UiPaletteHit {
    Entry(usize),
    Panel,
    None,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum UiContextMenuHit {
    Entry(usize),
    Menu,
    None,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum UiPasteDialogHit {
    Paste,
    Cancel,
    Dialog,
    None,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum UiOverviewHit {
    Pane(usize, u64),
    ClosePane(u64),
    FocusPane(usize, u64),
    Background,
    None,
}

#[cfg(test)]
mod rect_tests {
    use super::*;

    #[test]
    fn rect_split_left_basic() {
        let r = UiRect::new(0.0, 0.0, 100.0, 20.0);
        let (a, b) = r.split_left(30.0);
        assert_eq!(a, UiRect::new(0.0, 0.0, 30.0, 20.0));
        assert_eq!(b, UiRect::new(30.0, 0.0, 70.0, 20.0));
    }

    #[test]
    fn rect_split_right_basic() {
        let r = UiRect::new(10.0, 0.0, 100.0, 20.0);
        let (remain, taken) = r.split_right(30.0);
        assert_eq!(remain, UiRect::new(10.0, 0.0, 70.0, 20.0));
        assert_eq!(taken, UiRect::new(80.0, 0.0, 30.0, 20.0));
    }

    #[test]
    fn rect_split_overflow_clamps() {
        let r = UiRect::new(0.0, 0.0, 50.0, 20.0);
        let (a, b) = r.split_left(999.0);
        assert_eq!(a, UiRect::new(0.0, 0.0, 50.0, 20.0));
        assert!(b.is_empty());
    }

    #[test]
    fn rect_split_top_bottom_symmetry() {
        let r = UiRect::new(0.0, 0.0, 100.0, 80.0);
        let (t, rest1) = r.split_top(20.0);
        let (rest2, b) = r.split_bottom(20.0);
        assert_eq!(t.h, 20.0);
        assert_eq!(b.h, 20.0);
        assert_eq!(rest1.h, 60.0);
        assert_eq!(rest2.h, 60.0);
        assert_eq!(rest1.y, 20.0);
        assert_eq!(rest2.y, 0.0);
    }

    #[test]
    fn rect_contains_respects_right_and_bottom_exclusive() {
        let r = UiRect::new(10.0, 10.0, 20.0, 20.0);
        assert!(r.contains(10.0, 10.0));
        assert!(r.contains(29.9, 29.9));
        assert!(!r.contains(30.0, 20.0));
        assert!(!r.contains(20.0, 30.0));
        assert!(!r.contains(9.99, 20.0));
    }
}
