use loom_render::glyph_cache::{GlyphCache, GlyphInstance};
use loom_render::sdf_rect::SdfRect;
use loom_render::ui_shaper::UiTextShaper;
use loom_ui::NodeContext;
use std::cell::RefCell;
use winit::window::CursorIcon;

pub(crate) type UiTaffyTree = taffy::TaffyTree<NodeContext>;

/// A pixel-aligned axis-aligned rectangle used to describe UI slots.
///
/// Kept separate from `loom_render::rect::Rect` (which is a coloured GPU quad)
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
    pub config: &'a loom_config::config::LoomConfig,
    /// Pre-resolved theme tokens — paint paths should read colors via
    /// `cx.theme.<token>` rather than hex-parsing `cx.config.theme.*`
    /// every frame. Kept alongside `config` for call sites that still need raw
    /// non-theme settings.
    pub theme: &'a loom_ui::ResolvedTheme,
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
    /// Persistent Taffy layout tree. The render path borrows this, calls
    /// `clear()` on it, and rebuilds the tree each paint — preserving the
    /// SlotMap allocator instead of dropping and re-allocating dozens of
    /// nodes per widget per frame. `None` only in tests, where each call
    /// falls back to allocating a fresh tree.
    pub taffy_tree: Option<&'a RefCell<UiTaffyTree>>,
    /// Last mouse position in window-local pixels, when the cursor is
    /// inside the window. Used by the paint walker to determine which
    /// element is currently hovered (`Element::paint` reads
    /// `cx.is_hovered(hit_id)` to apply hover-state styles). `None`
    /// when the cursor has left the window or the host hasn't reported
    /// a position yet.
    pub mouse_pos: Option<[f32; 2]>,
    /// Hit-id of the chrome element the user pressed on at the most
    /// recent mouse-down, retained until mouse-up. Sourced from
    /// `App::active_hit_id`. Threaded into `PaintCtx::active_hit_id`
    /// so widgets can apply `.active(|s| ...)` refinements during a
    /// press. `None` outside of a press.
    pub active_hit_id: Option<u64>,
    /// Cross-frame element-state map (scroll offsets, virtual list
    /// anchors, etc.). Borrowed from `App.ui_states`; the adapter
    /// reborrows during paint so elements can call
    /// `cx.states.use_state::<S>(id)` without a 1-frame lag.
    pub element_states: Option<&'a RefCell<loom_ui::ElementStates>>,
}

pub(crate) fn ui_context_from_metrics<'a>(
    config: &'a loom_config::config::LoomConfig,
    theme: &'a loom_ui::ResolvedTheme,
    ui_shaper: Option<&'a RefCell<UiTextShaper>>,
    taffy_tree: Option<&'a RefCell<UiTaffyTree>>,
    mouse_pos: Option<[f32; 2]>,
    active_hit_id: Option<u64>,
    element_states: Option<&'a RefCell<loom_ui::ElementStates>>,
    viewport_w: f32,
    viewport_h: f32,
    cell_w: f32,
    cell_h: f32,
    baseline: f32,
) -> UiContext<'a> {
    UiContext {
        config,
        theme,
        viewport_w,
        viewport_h,
        cell_w,
        cell_h,
        baseline,
        ui_line_h: ui_shaper
            .map(|s| s.borrow().line_height())
            .unwrap_or(cell_h),
        ui_shaper,
        taffy_tree,
        mouse_pos,
        active_hit_id,
        element_states,
    }
}

#[cfg(test)]
pub(crate) fn test_ui_context<'a>(
    config: &'a loom_config::config::LoomConfig,
    theme: &'a loom_ui::ResolvedTheme,
    viewport_w: f32,
    viewport_h: f32,
) -> UiContext<'a> {
    UiContext {
        config,
        theme,
        viewport_w,
        viewport_h,
        cell_w: 8.0,
        cell_h: 16.0,
        baseline: 12.0,
        ui_line_h: 16.0,
        ui_shaper: None,
        taffy_tree: None,
        mouse_pos: None,
        active_hit_id: None,
        element_states: None,
    }
}

pub(super) fn ui_hit_id(
    root: &impl loom_ui::Element,
    cx: &UiContext<'_>,
    mx: f32,
    my: f32,
) -> Option<u64> {
    with_hit_layout(root, cx, |layout| {
        layout.hit_test(mx, my).and_then(|node| node.hit_id)
    })
}

/// Look up the screen-space bounds `[x, y, w, h]` of the first
/// element in `root` that carries `hit_id`. Used by interaction
/// paths that need geometry (scrollbar track Y, etc.) — the same
/// layout walk drives `ui_hit_id` so the values stay consistent.
pub(super) fn ui_hit_bounds(
    root: &impl loom_ui::Element,
    cx: &UiContext<'_>,
    hit_id: u64,
) -> Option<[f32; 4]> {
    with_hit_layout(root, cx, |layout| {
        layout
            .nodes()
            .iter()
            .find(|node| node.hit_id == Some(hit_id))
            .map(|node| node.bounds)
    })
}

/// Run a layout-only walk of `root`, populate a `LayoutSnapshot`, and
/// hand it to `f`. The walker is the lightweight hit-test variant —
/// it skips `Element::paint`, so callers that only need bounds/ids
/// don't pay for SDF rect emission, glyph shaping, or paint-time
/// transform composition.
fn with_hit_layout<R>(
    root: &impl loom_ui::Element,
    cx: &UiContext<'_>,
    f: impl FnOnce(&loom_ui::LayoutSnapshot) -> R,
) -> R {
    // MUST use the real host shaper, not `NullShaper`, so the
    // hit-test layout matches the paint layout. Settings rows lay
    // out label + description side-by-side at proportional widths;
    // measuring those at zero width (NullShaper) shifts the dropdown
    // bounds and the same cursor coord then resolves to a different
    // `hit_id` than the paint walk produces — `current_settings_hover`
    // and the paint prepass disagree, so `.hover()` styles never apply.
    if cx.taffy_tree.is_some() {
        let mut layout = loom_ui::LayoutSnapshot::new();
        crate::app::loom_ui_adapter::layout_only_into(root, cx, &mut layout);
        f(&layout)
    } else {
        // Test fallback: no retained tree / shaper. Use NullShaper here
        // because the test harness never sets `cx.ui_shaper` either, so
        // matching paint-time `NullShaper` measurement is correct.
        let mut shaper = loom_ui::NullShaper;
        let viewport = [cx.viewport_w, cx.viewport_h];
        let mut tree = taffy::TaffyTree::<loom_ui::NodeContext>::new();
        let mut layout = loom_ui::LayoutSnapshot::new();
        loom_ui::layout_tree_into_retained(root, viewport, &mut shaper, &mut layout, &mut tree);
        f(&layout)
    }
}

pub(crate) struct UiScene<'a> {
    pub atlas: &'a mut GlyphCache,
    pub glyphs: &'a mut Vec<GlyphInstance>,
    pub color_glyphs: &'a mut Vec<GlyphInstance>,
    /// SDF rounded-chrome primitives. Emitted by components that want
    /// rounded corners, shadows, or AA'd borders without building a full
    /// loom-ui Element tree; paired 1:1 with the blade SDF pipeline.
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
    /// Close the settings panel (Esc / outside click / × button).
    CloseSettings,
    /// Dismiss the keybindings help overlay (any click anywhere).
    CloseHelp,
    /// Click landed on a settings panel chrome region that should
    /// absorb without dismissing (e.g. empty body, row gutter).
    /// Distinguished from `CloseSettings` so the dispatcher can no-op.
    SettingsNoOp,
    /// Reveal `settings.toml` in the OS file manager / open with editor.
    OpenSettingsToml,
    /// Sidebar nav: switch the panel to a different category page.
    SelectSettingsCategory(loom_app::app::SettingsCategory),
    /// Settings row interaction — nudge a stepper or flip a toggle.
    /// Enum dropdowns route through [`Self::OpenSettingsDropdown`]
    /// instead so the popup machinery (`context_menu`) is reused.
    SettingsControl(crate::app::ui::settings_panel::SettingsActionPayload),
    /// Open the picker popup for an enum-typed settings row. The
    /// `SettingsField` identifies which dropdown — the dispatcher
    /// populates `context_menu` with the schema's `enum_variants`.
    /// Theme preset reuses this path; clicked items dispatch
    /// `ContextMenuAction::SetSettingsEnum { field_id, value }` so
    /// the field round-trip stays opaque to `loom-app`.
    OpenSettingsDropdown(crate::app::ui::settings_panel::schema::SettingsField),
    /// Copy the derived browser UI URL to the clipboard.
    CopyWebUrl,
    /// Regenerate the browser UI token and restart the gateway when enabled.
    RegenWebToken,
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
