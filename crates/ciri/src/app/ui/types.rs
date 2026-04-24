use ciri_render::glyph_cache::{GlyphCache, GlyphInstance};
use ciri_render::sdf_rect::SdfRect;
use ciri_render::ui_shaper::UiTextShaper;
use std::cell::RefCell;
use winit::window::CursorIcon;

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
