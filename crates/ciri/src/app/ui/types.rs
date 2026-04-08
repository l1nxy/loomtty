use ciri_render::glyph_cache::{GlyphCache, GlyphInstance};
use ciri_render::rect::Rect;
use winit::window::CursorIcon;

pub(crate) struct UiContext<'a> {
    pub config: &'a ciri_config::config::CiriConfig,
    pub viewport_w: f32,
    pub viewport_h: f32,
    pub cell_w: f32,
    pub cell_h: f32,
    pub baseline: f32,
}

pub(crate) struct UiScene<'a> {
    pub atlas: &'a mut GlyphCache,
    pub bg_rects: &'a mut Vec<Rect>,
    pub glyphs: &'a mut Vec<GlyphInstance>,
    pub color_glyphs: &'a mut Vec<GlyphInstance>,
}

/// Unified component trait for all UI chrome elements.
///
/// Each component captures a snapshot from `App` state, then can:
/// - `paint()` into the render scene
/// - `click()` to map a mouse click to a `UiAction`
pub(crate) trait UiComponent {
    /// Draw this component.
    fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>);

    /// Map a click at (mx, my) to a UiAction.
    /// Returns `None` if the click is outside this component.
    fn click(&self, _mx: f32, _my: f32, _cx: &UiContext<'_>) -> Option<UiAction> {
        None
    }

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

pub(super) enum UiTopBarHit {
    Session,
    Workspace,
    Mode,
    PaneTab(u64),
    Background,
}

pub(super) enum UiPaletteHit {
    Entry(usize),
    Panel,
    None,
}

pub(super) enum UiContextMenuHit {
    Entry(usize),
    Menu,
    None,
}

pub(super) enum UiPasteDialogHit {
    Paste,
    Cancel,
    Dialog,
    None,
}

pub(super) enum UiOverviewHit {
    Pane(usize, u64),
    ClosePane(u64),
    FocusPane(usize, u64),
    Background,
    None,
}
