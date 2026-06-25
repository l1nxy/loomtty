//! Data-driven field schema for the settings panel.
//!
//! Why a table instead of N hand-rolled rows: each row in the panel
//! needs five orthogonal facts — how to **read** the value out of
//! `LoomConfig`, how to **mutate** it (nudge / toggle / pick), how to
//! **write** it back through [`EditableConfig`], its **clamp/step**
//! bounds, and its **label/description/category** for paint. Hand-
//! rolling that quintet per field means ~25 lines of boilerplate per
//! row × ~25 rows = ~600 lines of repetitive match arms. The schema
//! table collapses it: every fact lives next to the field, the panel
//! and the dispatcher both walk the same table, and adding a new row
//! is one entry instead of three files.
//!
//! Adding a new field:
//! 1. Add a [`SettingsField`] variant.
//! 2. Add a [`FieldMeta`] entry to [`FIELDS`].
//! 3. Add a match arm in `read_*` / `apply_*` / `write_to` for the
//!    field. The compiler will tell you which functions need it.
//!
//! There are no other places to touch — hit-test, sidebar nav, and
//! dispatcher all derive from this table.
//!
//! ## Wire/disk identity
//!
//! [`SettingsField::id`] returns a stable u16 used by
//! `ContextMenuAction::SetSettingsEnum { field_id, .. }` so the
//! `loom-app` layer can carry "which dropdown" without depending on
//! this enum. Encoding == enum discriminant; if we ever rip out a
//! variant the gap is fine, only stability across a single session
//! matters.

use loom_app::app::SettingsCategory;
use loom_config::config::{
    AlphaBlending, AnimationPreset, CenterStrategy, DisableLigatures, FocusRingStyle, InputMode,
    LoomConfig, NewPaneSizing, NewPaneWidth, PaneOpenStyle, PredictionMode, PresentMode,
    RenderBackend, StatusBarPosition, TabBarPosition, UiFontConfig,
};
use loom_config::writer::EditableConfig;

/// Every row the settings panel can render. Schema-driven: see
/// [`FIELDS`] for the per-field metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum SettingsField {
    // ── Appearance ─────────────────────────────────────────
    ThemePreset,
    AppearancePaneOpacity,
    AppearancePaneCornerRadius,
    AppearancePadding,
    AppearanceColumnGap,
    AppearanceInactiveOpacity,
    AppearanceBackgroundDim,
    AppearanceBorderWidth,
    AppearanceFocusRingStyle,
    // ── Font ──────────────────────────────────────────────
    FontFamily,
    FontSize,
    FontWeight,
    FontLineHeight,
    UiFontFamily,
    UiFontSize,
    FontCellWidth,
    FontDisableLigatures,
    FontUnderlinePosition,
    FontUnderlineThickness,
    FontStrikethroughPosition,
    FontStrikethroughThickness,
    // ── Terminal ──────────────────────────────────────────
    TerminalCursorBlink,
    TerminalCopyOnSelect,
    TerminalScrollbackLines,
    TerminalCursorShape,
    TerminalCursorOpacity,
    TerminalCursorBlinkInterval,
    TerminalClearSelectionOnType,
    TerminalBellUrgency,
    TerminalPasteWarnThreshold,
    TerminalDefaultCols,
    TerminalDefaultRows,
    TerminalNotifyThreshold,
    // ── Layout ────────────────────────────────────────────
    LayoutCenterFocusedColumn,
    LayoutNewPaneSizing,
    LayoutNewPaneWidth,
    LayoutDynamicFullscreenWidth,
    // ── TabBar ────────────────────────────────────────────
    TabBarPosition,
    TabBarWidth,
    TabBarTabHeight,
    TabBarTabGap,
    TabBarPaneTabWidthChars,
    // ── StatusBar ─────────────────────────────────────────
    StatusBarPosition,
    StatusBarPaddingRatio,
    // ── Animation ─────────────────────────────────────────
    AnimationEnabled,
    AnimationPreset,
    AnimationPaneOpenStyle,
    AnimationDragOpacity,
    AnimationOverviewZoomFit,
    // ── Input ─────────────────────────────────────────────
    InputMode,
    InputFocusFollowsMouse,
    InputLeaderTimeout,
    InputDoubleTapWindow,
    InputScrollMultiplier,
    // ── Gesture ───────────────────────────────────────────
    GestureEnabled,
    GestureNaturalScroll,
    GestureSmoothScroll,
    GesturePinchSensitivity,
    GestureScrollPixelsPerLine,
    // ── Render ────────────────────────────────────────────
    RenderBackend,
    RenderPresentMode,
    RenderAlphaBlending,
    RenderSoftness,
    RenderFrameInterval,
    // ── Prediction ────────────────────────────────────────
    PredictionMode,
    PredictionThreshold,
    PredictionShowUnderline,
    // ── Session ───────────────────────────────────────────
    SessionRestoreAgents,
    SessionAgentSaveInterval,
    ServerIdleTimeout,
}

impl SettingsField {
    pub fn id(self) -> u16 {
        self as u16
    }

    pub fn from_id(id: u16) -> Option<Self> {
        FIELDS.iter().find(|m| m.field.id() == id).map(|m| m.field)
    }
}

/// Kind of widget the panel paints for a row, plus everything the
/// dispatcher needs to apply edits and bound them.
pub enum FieldKind {
    /// `[− value +]` stepper over a float field.
    Float {
        min: f32,
        max: f32,
        step: f32,
        /// Decimal places shown in the value cell.
        precision: u8,
    },
    /// `[− value +]` stepper over an unsigned-int field. Step is
    /// applied to the raw integer (so `step=1000` for scrollback feels
    /// natural even though the value is in the tens of thousands).
    Int { min: usize, max: usize, step: usize },
    /// On/off [`Switch`].
    Bool,
    /// Dropdown anchored over `context_menu`. `variants` is the
    /// kebab-case list (matches what `serde(rename_all = "kebab-case")`
    /// expects on the schema enum).
    Enum { variants: &'static [&'static str] },
}

/// A single panel row. `category` controls which sidebar tab it shows
/// up under; `label` / `description` are the visible text. Read /
/// nudge / toggle / set_enum / write are dispatched by `SettingsField`
/// (the metadata table doesn't carry function pointers — keeping this
/// `static` simple).
pub struct FieldMeta {
    pub field: SettingsField,
    pub category: SettingsCategory,
    pub label: &'static str,
    pub description: &'static str,
    pub kind: FieldKind,
}

/// Single source of truth for which rows exist, in what order, under
/// which sidebar category. The panel iterates this table filtered by
/// the active category; nothing else hard-codes the row list.
pub static FIELDS: &[FieldMeta] = &[
    // ── Appearance ──────────────────────────────────────────
    FieldMeta {
        field: SettingsField::ThemePreset,
        category: SettingsCategory::Appearance,
        label: "Theme",
        description: "Colour scheme for the chrome and terminal palette.",
        kind: FieldKind::Enum {
            // Populated dynamically from `ThemeConfig::preset_names()` —
            // the empty slice here just signals "enum-typed" to the
            // panel; the actual variants come from `enum_variants()`.
            variants: &[],
        },
    },
    FieldMeta {
        field: SettingsField::AppearancePaneOpacity,
        category: SettingsCategory::Appearance,
        label: "Pane Opacity",
        description: "Translucency of pane backgrounds over the wallpaper.",
        kind: FieldKind::Float {
            min: 0.05,
            max: 1.0,
            step: 0.05,
            precision: 2,
        },
    },
    FieldMeta {
        field: SettingsField::AppearancePaneCornerRadius,
        category: SettingsCategory::Appearance,
        label: "Pane Corners",
        description: "Logical-pixel corner radius for pane backgrounds and the focus ring.",
        kind: FieldKind::Float {
            min: 0.0,
            max: 32.0,
            step: 1.0,
            precision: 0,
        },
    },
    FieldMeta {
        field: SettingsField::AppearancePadding,
        category: SettingsCategory::Appearance,
        label: "Padding",
        description: "Inner padding around the terminal viewport.",
        kind: FieldKind::Float {
            min: 0.0,
            max: 32.0,
            step: 1.0,
            precision: 0,
        },
    },
    FieldMeta {
        field: SettingsField::AppearanceColumnGap,
        category: SettingsCategory::Appearance,
        label: "Column Gap",
        description: "Horizontal gap between adjacent columns.",
        kind: FieldKind::Float {
            min: 0.0,
            max: 32.0,
            step: 1.0,
            precision: 0,
        },
    },
    FieldMeta {
        field: SettingsField::AppearanceInactiveOpacity,
        category: SettingsCategory::Appearance,
        label: "Inactive Opacity",
        description: "Opacity applied to panes that don't hold focus.",
        kind: FieldKind::Float {
            min: 0.0,
            max: 1.0,
            step: 0.05,
            precision: 2,
        },
    },
    FieldMeta {
        field: SettingsField::AppearanceBackgroundDim,
        category: SettingsCategory::Appearance,
        label: "Background Dim",
        description: "Dim factor over the wallpaper image (0 = full image, 1 = solid colour).",
        kind: FieldKind::Float {
            min: 0.0,
            max: 1.0,
            step: 0.05,
            precision: 2,
        },
    },
    FieldMeta {
        field: SettingsField::AppearanceBorderWidth,
        category: SettingsCategory::Appearance,
        label: "Border Width",
        description: "Thickness of the pane border / focus ring in logical pixels.",
        kind: FieldKind::Float {
            min: 0.0,
            max: 16.0,
            step: 0.5,
            precision: 1,
        },
    },
    FieldMeta {
        field: SettingsField::AppearanceFocusRingStyle,
        category: SettingsCategory::Appearance,
        label: "Focus Ring Style",
        description: "How the active-pane border is drawn.",
        kind: FieldKind::Enum {
            variants: &["solid", "glow", "dashed"],
        },
    },
    // ── Font ────────────────────────────────────────────────
    FieldMeta {
        field: SettingsField::FontFamily,
        category: SettingsCategory::Font,
        label: "Font Family",
        description: "Font family for terminal text.",
        kind: FieldKind::Enum {
            // Populated dynamically from `loom_render::font_list::monospaced_families()` —
            // the empty slice here just signals "enum-typed" to the
            // panel; the actual variants come from `enum_variants()`.
            variants: &[],
        },
    },
    FieldMeta {
        field: SettingsField::FontSize,
        category: SettingsCategory::Font,
        label: "Font Size",
        description: "Terminal cell font size in points.",
        kind: FieldKind::Float {
            min: 6.0,
            max: 72.0,
            step: 0.5,
            precision: 1,
        },
    },
    FieldMeta {
        field: SettingsField::FontWeight,
        category: SettingsCategory::Font,
        label: "Font Weight",
        description: "OpenType weight for the regular face (100-900).",
        kind: FieldKind::Int {
            min: 100,
            max: 900,
            step: 100,
        },
    },
    FieldMeta {
        field: SettingsField::FontLineHeight,
        category: SettingsCategory::Font,
        label: "Line Height",
        description: "Vertical spacing preset for terminal lines.",
        kind: FieldKind::Enum {
            // The picker exposes `standard` (1.0) and `comfortable`
            // (1.2). `custom` is a read-only label shown when the
            // user has set an arbitrary `font.adjust_cell_height` in
            // TOML — `set_enum` ignores it.
            variants: &["standard", "comfortable"],
        },
    },
    FieldMeta {
        field: SettingsField::UiFontFamily,
        category: SettingsCategory::Font,
        label: "UI Font Family",
        description: "Font for chrome (status bar, menus, palette). 'system' keeps the OS default.",
        kind: FieldKind::Enum {
            // Picker list comes from `all_families()` with a leading
            // `system` sentinel; see `enum_variants`.
            variants: &[],
        },
    },
    FieldMeta {
        field: SettingsField::UiFontSize,
        category: SettingsCategory::Font,
        label: "UI Font Size",
        description: "Point size for chrome text. Falls back to Font Size when unset.",
        kind: FieldKind::Float {
            min: 6.0,
            max: 72.0,
            step: 0.5,
            precision: 1,
        },
    },
    FieldMeta {
        field: SettingsField::FontCellWidth,
        category: SettingsCategory::Font,
        label: "Cell Width",
        description: "Multiplier on the computed cell width (1.0 = unchanged).",
        kind: FieldKind::Float {
            min: 0.5,
            max: 3.0,
            step: 0.05,
            precision: 2,
        },
    },
    FieldMeta {
        field: SettingsField::FontDisableLigatures,
        category: SettingsCategory::Font,
        label: "Disable Ligatures",
        description: "When to suppress ligature shaping (never / on the cursor row / always).",
        kind: FieldKind::Enum {
            variants: &["never", "cursor", "always"],
        },
    },
    FieldMeta {
        field: SettingsField::FontUnderlinePosition,
        category: SettingsCategory::Font,
        label: "Underline Position",
        description: "Pixel offset added to the underline's vertical position (positive = lower).",
        kind: FieldKind::Float {
            min: -8.0,
            max: 8.0,
            step: 0.5,
            precision: 1,
        },
    },
    FieldMeta {
        field: SettingsField::FontUnderlineThickness,
        category: SettingsCategory::Font,
        label: "Underline Thickness",
        description: "Multiplier on the underline thickness (1.0 = default 1px).",
        kind: FieldKind::Float {
            min: 0.0,
            max: 16.0,
            step: 0.5,
            precision: 1,
        },
    },
    FieldMeta {
        field: SettingsField::FontStrikethroughPosition,
        category: SettingsCategory::Font,
        label: "Strikethrough Position",
        description: "Pixel offset added to the strikethrough's vertical position.",
        kind: FieldKind::Float {
            min: -8.0,
            max: 8.0,
            step: 0.5,
            precision: 1,
        },
    },
    FieldMeta {
        field: SettingsField::FontStrikethroughThickness,
        category: SettingsCategory::Font,
        label: "Strikethrough Thickness",
        description: "Multiplier on the strikethrough thickness (1.0 = default 1px).",
        kind: FieldKind::Float {
            min: 0.0,
            max: 16.0,
            step: 0.5,
            precision: 1,
        },
    },
    // ── Terminal ────────────────────────────────────────────
    FieldMeta {
        field: SettingsField::TerminalCursorBlink,
        category: SettingsCategory::Terminal,
        label: "Cursor Blink",
        description: "Blink the cursor while the pane has focus.",
        kind: FieldKind::Bool,
    },
    FieldMeta {
        field: SettingsField::TerminalCopyOnSelect,
        category: SettingsCategory::Terminal,
        label: "Copy on Select",
        description: "Copy to the clipboard automatically when text is selected.",
        kind: FieldKind::Bool,
    },
    FieldMeta {
        field: SettingsField::TerminalScrollbackLines,
        category: SettingsCategory::Terminal,
        label: "Scrollback Lines",
        description: "Maximum lines of history retained per pane.",
        kind: FieldKind::Int {
            min: 1_000,
            max: 200_000,
            step: 1_000,
        },
    },
    FieldMeta {
        field: SettingsField::TerminalCursorShape,
        category: SettingsCategory::Terminal,
        label: "Cursor Shape",
        description: "Force a cursor shape regardless of what the running app requests.",
        kind: FieldKind::Enum {
            variants: &["block", "beam", "underline", "hollow_block"],
        },
    },
    FieldMeta {
        field: SettingsField::TerminalCursorOpacity,
        category: SettingsCategory::Terminal,
        label: "Cursor Opacity",
        description: "Opacity of the text cursor.",
        kind: FieldKind::Float {
            min: 0.0,
            max: 1.0,
            step: 0.05,
            precision: 2,
        },
    },
    FieldMeta {
        field: SettingsField::TerminalCursorBlinkInterval,
        category: SettingsCategory::Terminal,
        label: "Cursor Blink Interval",
        description: "Milliseconds between cursor blink on/off phases.",
        kind: FieldKind::Int {
            min: 50,
            max: 2_000,
            step: 50,
        },
    },
    FieldMeta {
        field: SettingsField::TerminalClearSelectionOnType,
        category: SettingsCategory::Terminal,
        label: "Clear Selection on Type",
        description: "Drop the active selection as soon as you type.",
        kind: FieldKind::Bool,
    },
    FieldMeta {
        field: SettingsField::TerminalBellUrgency,
        category: SettingsCategory::Terminal,
        label: "Bell Urgency",
        description: "Mark the window urgent (taskbar flash) when the terminal bell rings.",
        kind: FieldKind::Bool,
    },
    FieldMeta {
        field: SettingsField::TerminalPasteWarnThreshold,
        category: SettingsCategory::Terminal,
        label: "Paste Warn Threshold",
        description: "Show the paste confirmation dialog above this many bytes (0 = never warn).",
        kind: FieldKind::Int {
            min: 0,
            max: 100_000,
            step: 1_000,
        },
    },
    FieldMeta {
        field: SettingsField::TerminalDefaultCols,
        category: SettingsCategory::Terminal,
        label: "Default Columns",
        description: "Initial column count for newly created panes.",
        kind: FieldKind::Int {
            min: 1,
            max: 500,
            step: 1,
        },
    },
    FieldMeta {
        field: SettingsField::TerminalDefaultRows,
        category: SettingsCategory::Terminal,
        label: "Default Rows",
        description: "Initial row count for newly created panes.",
        kind: FieldKind::Int {
            min: 1,
            max: 300,
            step: 1,
        },
    },
    FieldMeta {
        field: SettingsField::TerminalNotifyThreshold,
        category: SettingsCategory::Terminal,
        label: "Notify Command Threshold",
        description: "Notify when a foreground command runs longer than N seconds (0 = off).",
        kind: FieldKind::Int {
            min: 0,
            max: 3_600,
            step: 5,
        },
    },
    // ── Layout ──────────────────────────────────────────────
    FieldMeta {
        field: SettingsField::LayoutCenterFocusedColumn,
        category: SettingsCategory::Layout,
        label: "Center Focused Column",
        description: "How aggressively the viewport recentres on the active column.",
        kind: FieldKind::Enum {
            variants: &["never", "on-overflow", "always"],
        },
    },
    FieldMeta {
        field: SettingsField::LayoutNewPaneSizing,
        category: SettingsCategory::Layout,
        label: "New Pane Sizing",
        description: "Fixed: new panes are always half/full width. Dynamic: full on small windows, half on large.",
        kind: FieldKind::Enum {
            variants: &["fixed", "dynamic"],
        },
    },
    FieldMeta {
        field: SettingsField::LayoutNewPaneWidth,
        category: SettingsCategory::Layout,
        label: "Fixed Pane Width",
        description: "Width of a new pane in Fixed mode: half or full viewport. Ignored in Dynamic mode.",
        kind: FieldKind::Enum {
            variants: &["half", "full"],
        },
    },
    FieldMeta {
        field: SettingsField::LayoutDynamicFullscreenWidth,
        category: SettingsCategory::Layout,
        label: "Dynamic Fullscreen Below",
        description: "In Dynamic mode, open new panes full-width when the window is narrower than this (px).",
        kind: FieldKind::Int {
            min: 200,
            max: 4000,
            step: 50,
        },
    },
    // ── TabBar ──────────────────────────────────────────────
    FieldMeta {
        field: SettingsField::TabBarPosition,
        category: SettingsCategory::TabBar,
        label: "Position",
        description: "Where pane tabs are drawn: in the status bar, or a side bar on the left/right.",
        kind: FieldKind::Enum {
            variants: &["integrated", "left", "right"],
        },
    },
    FieldMeta {
        field: SettingsField::TabBarWidth,
        category: SettingsCategory::TabBar,
        label: "Side Bar Width",
        description: "Width of the side tab bar in pixels. Ignored when position is integrated.",
        kind: FieldKind::Float {
            min: 40.0,
            max: 800.0,
            step: 10.0,
            precision: 0,
        },
    },
    FieldMeta {
        field: SettingsField::TabBarTabHeight,
        category: SettingsCategory::TabBar,
        label: "Tab Height",
        description: "Height per tab in pixels. Ignored when position is integrated.",
        kind: FieldKind::Float {
            min: 12.0,
            max: 200.0,
            step: 2.0,
            precision: 0,
        },
    },
    FieldMeta {
        field: SettingsField::TabBarTabGap,
        category: SettingsCategory::TabBar,
        label: "Tab Gap",
        description: "Vertical gap between adjacent side tabs in pixels.",
        kind: FieldKind::Float {
            min: 0.0,
            max: 40.0,
            step: 1.0,
            precision: 0,
        },
    },
    FieldMeta {
        field: SettingsField::TabBarPaneTabWidthChars,
        category: SettingsCategory::TabBar,
        label: "Integrated Tab Width",
        description: "Per-tab width in characters when tabs are integrated into the status bar.",
        kind: FieldKind::Int {
            min: 4,
            max: 80,
            step: 1,
        },
    },
    // ── StatusBar ───────────────────────────────────────────
    FieldMeta {
        field: SettingsField::StatusBarPosition,
        category: SettingsCategory::StatusBar,
        label: "Position",
        description: "Whether the status bar sits at the top or bottom of the window.",
        kind: FieldKind::Enum {
            variants: &["top", "bottom"],
        },
    },
    FieldMeta {
        field: SettingsField::StatusBarPaddingRatio,
        category: SettingsCategory::StatusBar,
        label: "Padding Ratio",
        description: "Vertical padding of the status bar as a fraction of the line height.",
        kind: FieldKind::Float {
            min: 0.0,
            max: 2.0,
            step: 0.05,
            precision: 2,
        },
    },
    // ── Animation ───────────────────────────────────────────
    FieldMeta {
        field: SettingsField::AnimationEnabled,
        category: SettingsCategory::Animation,
        label: "Animations Enabled",
        description: "Master switch for layout / overview transitions.",
        kind: FieldKind::Bool,
    },
    FieldMeta {
        field: SettingsField::AnimationPreset,
        category: SettingsCategory::Animation,
        label: "Animation Preset",
        description: "Speed feel for chrome transitions.",
        kind: FieldKind::Enum {
            variants: &["snappy", "default", "smooth", "gentle"],
        },
    },
    FieldMeta {
        field: SettingsField::AnimationPaneOpenStyle,
        category: SettingsCategory::Animation,
        label: "Pane Open Style",
        description: "Transition used when a pane opens or closes.",
        kind: FieldKind::Enum {
            variants: &[
                "fade",
                "slide-up",
                "slide-down",
                "slide-left",
                "fade-slide-up",
            ],
        },
    },
    FieldMeta {
        field: SettingsField::AnimationDragOpacity,
        category: SettingsCategory::Animation,
        label: "Drag Opacity",
        description: "Opacity of a pane while it is being dragged.",
        kind: FieldKind::Float {
            min: 0.0,
            max: 1.0,
            step: 0.05,
            precision: 2,
        },
    },
    FieldMeta {
        field: SettingsField::AnimationOverviewZoomFit,
        category: SettingsCategory::Animation,
        label: "Overview Zoom Fit",
        description: "How much of the viewport the overview grid fills (0.9 = 90%).",
        kind: FieldKind::Float {
            min: 0.1,
            max: 1.0,
            step: 0.05,
            precision: 2,
        },
    },
    // ── Input ───────────────────────────────────────────────
    FieldMeta {
        field: SettingsField::InputMode,
        category: SettingsCategory::Input,
        label: "Input Mode",
        description: "Prefix (tmux-style, single-shot) or sticky (zellij-style, modal).",
        kind: FieldKind::Enum {
            variants: &["prefix", "sticky"],
        },
    },
    FieldMeta {
        field: SettingsField::InputFocusFollowsMouse,
        category: SettingsCategory::Input,
        label: "Focus Follows Mouse",
        description: "Move keyboard focus to whichever pane the cursor hovers.",
        kind: FieldKind::Bool,
    },
    FieldMeta {
        field: SettingsField::InputLeaderTimeout,
        category: SettingsCategory::Input,
        label: "Leader Timeout",
        description: "Milliseconds to wait for the next key after the leader before resetting.",
        kind: FieldKind::Int {
            min: 100,
            max: 5_000,
            step: 100,
        },
    },
    FieldMeta {
        field: SettingsField::InputDoubleTapWindow,
        category: SettingsCategory::Input,
        label: "Double-Tap Window",
        description: "Milliseconds within which two leader presses count as a double-tap.",
        kind: FieldKind::Int {
            min: 100,
            max: 1_000,
            step: 50,
        },
    },
    FieldMeta {
        field: SettingsField::InputScrollMultiplier,
        category: SettingsCategory::Input,
        label: "Scroll Multiplier",
        description: "Lines scrolled per wheel notch / scroll step.",
        kind: FieldKind::Float {
            min: 1.0,
            max: 200.0,
            step: 5.0,
            precision: 0,
        },
    },
    // ── Gesture ─────────────────────────────────────────────
    FieldMeta {
        field: SettingsField::GestureEnabled,
        category: SettingsCategory::Gesture,
        label: "Gestures Enabled",
        description: "Master switch for trackpad pinch / swipe gestures.",
        kind: FieldKind::Bool,
    },
    FieldMeta {
        field: SettingsField::GestureNaturalScroll,
        category: SettingsCategory::Gesture,
        label: "Natural Scroll",
        description: "Invert scroll direction (content tracks finger movement).",
        kind: FieldKind::Bool,
    },
    FieldMeta {
        field: SettingsField::GestureSmoothScroll,
        category: SettingsCategory::Gesture,
        label: "Smooth Scroll",
        description: "Interpolate scroll motion for finer-grained wheel/trackpad input.",
        kind: FieldKind::Bool,
    },
    FieldMeta {
        field: SettingsField::GesturePinchSensitivity,
        category: SettingsCategory::Gesture,
        label: "Pinch Sensitivity",
        description: "Multiplier on pinch-zoom gestures (higher = more zoom per pinch).",
        kind: FieldKind::Float {
            min: 0.5,
            max: 5.0,
            step: 0.1,
            precision: 1,
        },
    },
    FieldMeta {
        field: SettingsField::GestureScrollPixelsPerLine,
        category: SettingsCategory::Gesture,
        label: "Scroll Pixels per Line",
        description: "Pixels of trackpad scroll that map to one terminal line.",
        kind: FieldKind::Float {
            min: 1.0,
            max: 100.0,
            step: 1.0,
            precision: 0,
        },
    },
    // ── Render ──────────────────────────────────────────────
    FieldMeta {
        field: SettingsField::RenderBackend,
        category: SettingsCategory::Render,
        label: "Backend",
        description: "GPU backend (auto / blade / gl). Applies on restart.",
        kind: FieldKind::Enum {
            variants: &["auto", "blade", "gl"],
        },
    },
    FieldMeta {
        field: SettingsField::RenderPresentMode,
        category: SettingsCategory::Render,
        label: "Present Mode",
        description: "Swapchain present mode (fifo = vsync, mailbox / immediate = lower latency). Applies on restart.",
        kind: FieldKind::Enum {
            variants: &["fifo", "mailbox", "immediate"],
        },
    },
    FieldMeta {
        field: SettingsField::RenderAlphaBlending,
        category: SettingsCategory::Render,
        label: "Alpha Blending",
        description: "Text compositing colour space (native / linear / linear-corrected).",
        kind: FieldKind::Enum {
            variants: &["native", "linear", "linear-corrected"],
        },
    },
    FieldMeta {
        field: SettingsField::RenderSoftness,
        category: SettingsCategory::Render,
        label: "Softness",
        description: "Post-process softness on the final image (0 = off). GL backend + linear blending only.",
        kind: FieldKind::Float {
            min: 0.0,
            max: 1.0,
            step: 0.05,
            precision: 2,
        },
    },
    FieldMeta {
        field: SettingsField::RenderFrameInterval,
        category: SettingsCategory::Render,
        label: "Frame Interval",
        description: "Milliseconds between rendered frames (16 ≈ 60 FPS). Applies on restart.",
        kind: FieldKind::Int {
            min: 1,
            max: 100,
            step: 1,
        },
    },
    // ── Prediction ──────────────────────────────────────────
    FieldMeta {
        field: SettingsField::PredictionMode,
        category: SettingsCategory::Prediction,
        label: "Prediction Mode",
        description: "Mosh-style speculative input echo (never / always / latency-adaptive).",
        kind: FieldKind::Enum {
            variants: &["never", "always", "adaptive"],
        },
    },
    FieldMeta {
        field: SettingsField::PredictionThreshold,
        category: SettingsCategory::Prediction,
        label: "Adaptive Threshold",
        description: "Latency in ms above which adaptive mode starts predicting.",
        kind: FieldKind::Int {
            min: 0,
            max: 500,
            step: 5,
        },
    },
    FieldMeta {
        field: SettingsField::PredictionShowUnderline,
        category: SettingsCategory::Prediction,
        label: "Underline Predictions",
        description: "Underline speculatively-echoed characters until the server confirms them.",
        kind: FieldKind::Bool,
    },
    // ── Session ─────────────────────────────────────────────
    FieldMeta {
        field: SettingsField::SessionRestoreAgents,
        category: SettingsCategory::Session,
        label: "Restore Agents",
        description: "Detect and auto-resume AI agents on restart.",
        kind: FieldKind::Bool,
    },
    FieldMeta {
        field: SettingsField::SessionAgentSaveInterval,
        category: SettingsCategory::Session,
        label: "Agent Save Interval",
        description: "Seconds between agent-state autosaves.",
        kind: FieldKind::Int {
            min: 5,
            max: 600,
            step: 5,
        },
    },
    FieldMeta {
        field: SettingsField::ServerIdleTimeout,
        category: SettingsCategory::Session,
        label: "Server Idle Timeout",
        description: "Seconds the background server waits after the last client disconnects before exiting (0 = immediate).",
        kind: FieldKind::Int {
            min: 0,
            max: 3_600,
            step: 30,
        },
    },
];

/// Look up the metadata for a field. Panics on unknown variants —
/// every `SettingsField` MUST have an entry in [`FIELDS`].
pub fn meta(field: SettingsField) -> &'static FieldMeta {
    FIELDS
        .iter()
        .find(|m| m.field == field)
        .expect("every SettingsField has a FIELDS entry — see the schema invariant")
}

/// Variants for a dropdown row. Theme preset and font family pull
/// their lists from runtime sources (theme module / fontdb scan); all
/// other enum fields have a static list in `FIELDS`.
pub fn enum_variants(field: SettingsField) -> Vec<String> {
    if matches!(field, SettingsField::ThemePreset) {
        return loom_config::theme::ThemeConfig::preset_names()
            .iter()
            .map(|s| (*s).to_string())
            .collect();
    }
    if matches!(field, SettingsField::FontFamily) {
        // Show every family fontdb finds, not just monospaced — the
        // user is the boss; terminal-alignment is on them if they
        // pick a proportional face.
        return loom_render::font_list::all_families().to_vec();
    }
    if matches!(field, SettingsField::UiFontFamily) {
        // UI font is intended to be proportional. Lead with the
        // `system` sentinel (clears the override) so the picker has
        // an obvious "go back to the OS default" entry, then list
        // every family fontdb knows.
        let mut out = vec!["system".to_string()];
        out.extend(loom_render::font_list::all_families().iter().cloned());
        return out;
    }
    match meta(field).kind {
        FieldKind::Enum { variants } => variants.iter().map(|s| (*s).to_string()).collect(),
        _ => Vec::new(),
    }
}

// ── Read accessors ──────────────────────────────────────────────────
//
// One arm per field. Adding a `SettingsField` triggers a compile error
// on the relevant accessor; the compiler is the checklist.

pub fn read_float(field: SettingsField, c: &LoomConfig) -> f32 {
    match field {
        SettingsField::AppearancePaneOpacity => c.appearance.pane_opacity,
        SettingsField::AppearancePaneCornerRadius => c.appearance.pane_corner_radius,
        SettingsField::AppearancePadding => c.appearance.padding,
        SettingsField::AppearanceColumnGap => c.appearance.column_gap,
        SettingsField::AppearanceInactiveOpacity => c.appearance.inactive_opacity,
        SettingsField::AppearanceBackgroundDim => c.appearance.background_dim,
        SettingsField::AppearanceBorderWidth => c.appearance.border_width,
        SettingsField::FontSize => c.font.size,
        // Mirrors `resolve_ui_font_init`'s "no override → terminal size"
        // fallback so the stepper starts from a sensible value when
        // the user hasn't customised the UI font yet.
        SettingsField::UiFontSize => c.font.ui.as_ref().map(|u| u.size).unwrap_or(c.font.size),
        SettingsField::FontCellWidth => c.font.adjust_cell_width,
        SettingsField::FontUnderlinePosition => c.font.adjust_underline_position,
        SettingsField::FontUnderlineThickness => c.font.adjust_underline_thickness,
        SettingsField::FontStrikethroughPosition => c.font.adjust_strikethrough_position,
        SettingsField::FontStrikethroughThickness => c.font.adjust_strikethrough_thickness,
        SettingsField::TerminalCursorOpacity => c.terminal.cursor_opacity,
        SettingsField::TabBarWidth => c.tabbar.width,
        SettingsField::TabBarTabHeight => c.tabbar.tab_height,
        SettingsField::TabBarTabGap => c.tabbar.tab_gap,
        SettingsField::StatusBarPaddingRatio => c.statusbar.padding_ratio,
        SettingsField::AnimationDragOpacity => c.animation.drag_opacity,
        SettingsField::AnimationOverviewZoomFit => c.animation.overview_zoom_fit,
        // f64-backed leaves — narrow to f32 for the stepper; the
        // sub-bit precision lost here is irrelevant for a user-facing
        // multiplier and the writer re-widens on persist.
        SettingsField::InputScrollMultiplier => c.input.scroll_multiplier as f32,
        SettingsField::GesturePinchSensitivity => c.gesture.pinch_sensitivity as f32,
        SettingsField::GestureScrollPixelsPerLine => c.gesture.scroll_pixels_per_line as f32,
        SettingsField::RenderSoftness => c.render.softness,
        _ => 0.0,
    }
}

pub fn read_int(field: SettingsField, c: &LoomConfig) -> usize {
    match field {
        SettingsField::TerminalScrollbackLines => c.terminal.scrollback_lines,
        // `weight` is `Option<u16>`; `None` means "let the shaper pick
        // closest to 400". The stepper UI doesn't have a "none" state,
        // so default-on-read to 400 (the OpenType Regular weight).
        SettingsField::FontWeight => c.font.weight.unwrap_or(400) as usize,
        SettingsField::TerminalCursorBlinkInterval => c.terminal.cursor_blink_interval_ms as usize,
        SettingsField::TerminalPasteWarnThreshold => c.terminal.paste_warn_threshold,
        SettingsField::TerminalDefaultCols => c.terminal.default_cols as usize,
        SettingsField::TerminalDefaultRows => c.terminal.default_rows as usize,
        SettingsField::TerminalNotifyThreshold => c.terminal.notify_command_threshold_secs as usize,
        SettingsField::TabBarPaneTabWidthChars => c.tabbar.pane_tab_width_chars,
        SettingsField::InputLeaderTimeout => c.input.leader_timeout_ms as usize,
        SettingsField::InputDoubleTapWindow => c.input.double_tap_window_ms as usize,
        SettingsField::RenderFrameInterval => c.render.frame_interval_ms as usize,
        SettingsField::PredictionThreshold => c.prediction.threshold_ms as usize,
        SettingsField::SessionAgentSaveInterval => c.session.agent_save_interval_secs as usize,
        SettingsField::ServerIdleTimeout => c.server.idle_timeout_secs as usize,
        SettingsField::LayoutDynamicFullscreenWidth => {
            c.layout.dynamic_fullscreen_max_width as usize
        }
        _ => 0,
    }
}

pub fn read_bool(field: SettingsField, c: &LoomConfig) -> bool {
    match field {
        SettingsField::TerminalCursorBlink => c.terminal.cursor_blink,
        SettingsField::TerminalCopyOnSelect => c.terminal.copy_on_select,
        SettingsField::AnimationEnabled => c.animation.enabled,
        SettingsField::InputFocusFollowsMouse => c.input.focus_follows_mouse,
        SettingsField::TerminalClearSelectionOnType => c.terminal.clear_selection_on_type,
        SettingsField::TerminalBellUrgency => c.terminal.bell_urgency,
        SettingsField::GestureEnabled => c.gesture.enabled,
        SettingsField::GestureNaturalScroll => c.gesture.natural_scroll,
        SettingsField::GestureSmoothScroll => c.gesture.smooth_scroll,
        SettingsField::PredictionShowUnderline => c.prediction.show_underline,
        SettingsField::SessionRestoreAgents => c.session.restore_agents,
        _ => false,
    }
}

pub fn read_enum(field: SettingsField, c: &LoomConfig) -> String {
    match field {
        SettingsField::ThemePreset => {
            // Static names come from `preset_names()`, but `theme.preset`
            // is owned by the user (free-form string). We pick the
            // matching static slice if any so the dropdown shows a
            // checkmark on the right row; "loom_dark" is the documented
            // empty-string fallback.
            let raw = if c.theme.preset.is_empty() {
                "loom_dark"
            } else {
                c.theme.preset.as_str()
            };
            loom_config::theme::ThemeConfig::preset_names()
                .iter()
                .copied()
                .find(|n| *n == raw)
                .unwrap_or("loom_dark")
                .to_string()
        }
        // Free-form string from config — shown verbatim in the
        // dropdown trigger. The picker matches against the system
        // font list; an unmatched value (e.g. user-typed family that
        // isn't installed) shows no checkmark but still renders here.
        SettingsField::FontFamily => c.font.family.clone(),
        // Same shape as `FontFamily` but with a `system` sentinel for
        // "no override". Empty family inside `Some` is treated as the
        // system default too — `resolve_ui_font_init` already does
        // that, so the panel mirrors it.
        SettingsField::UiFontFamily => match c.font.ui.as_ref() {
            Some(u) if !u.family.is_empty() => u.family.clone(),
            _ => "system".into(),
        },
        SettingsField::FontLineHeight => {
            // `adjust_cell_height` is a multiplier. Map the two
            // preset values to their labels; everything else displays
            // as "custom" (a no-pick label — `set_enum("custom")` is
            // a no-op).
            let v = c.font.adjust_cell_height;
            if (v - 1.0).abs() < 1e-3 {
                "standard".into()
            } else if (v - 1.2).abs() < 1e-3 {
                "comfortable".into()
            } else {
                "custom".into()
            }
        }
        SettingsField::AnimationPreset => match c.animation.preset {
            AnimationPreset::Snappy => "snappy".into(),
            AnimationPreset::Default => "default".into(),
            AnimationPreset::Smooth => "smooth".into(),
            AnimationPreset::Gentle => "gentle".into(),
        },
        SettingsField::InputMode => match c.input.mode {
            InputMode::Prefix => "prefix".into(),
            InputMode::Sticky => "sticky".into(),
        },
        SettingsField::StatusBarPosition => match c.statusbar.position {
            StatusBarPosition::Top => "top".into(),
            StatusBarPosition::Bottom => "bottom".into(),
        },
        SettingsField::PredictionMode => match c.prediction.mode {
            PredictionMode::Never => "never".into(),
            PredictionMode::Always => "always".into(),
            PredictionMode::Adaptive => "adaptive".into(),
        },
        SettingsField::LayoutCenterFocusedColumn => match c.layout.center_focused_column {
            CenterStrategy::Always => "always".into(),
            CenterStrategy::OnOverflow => "on-overflow".into(),
            CenterStrategy::Never => "never".into(),
        },
        SettingsField::LayoutNewPaneSizing => match c.layout.new_pane_sizing {
            NewPaneSizing::Fixed => "fixed".into(),
            NewPaneSizing::Dynamic => "dynamic".into(),
        },
        SettingsField::LayoutNewPaneWidth => match c.layout.new_pane_width {
            NewPaneWidth::Half => "half".into(),
            NewPaneWidth::Full => "full".into(),
        },
        SettingsField::AppearanceFocusRingStyle => match c.appearance.focus_ring.style {
            FocusRingStyle::Solid => "solid".into(),
            FocusRingStyle::Glow => "glow".into(),
            FocusRingStyle::Dashed => "dashed".into(),
        },
        SettingsField::FontDisableLigatures => match c.font.disable_ligatures {
            DisableLigatures::Never => "never".into(),
            DisableLigatures::Cursor => "cursor".into(),
            DisableLigatures::Always => "always".into(),
        },
        // Free-form string from config (matches one of the static
        // variants when set through the panel; an app-driven/unknown
        // value renders verbatim with no checkmark).
        SettingsField::TerminalCursorShape => c.terminal.cursor_shape.clone(),
        SettingsField::TabBarPosition => match c.tabbar.position {
            TabBarPosition::Integrated => "integrated".into(),
            TabBarPosition::Left => "left".into(),
            TabBarPosition::Right => "right".into(),
        },
        SettingsField::AnimationPaneOpenStyle => match c.animation.pane_open_style {
            PaneOpenStyle::Fade => "fade".into(),
            PaneOpenStyle::SlideUp => "slide-up".into(),
            PaneOpenStyle::SlideDown => "slide-down".into(),
            PaneOpenStyle::SlideLeft => "slide-left".into(),
            PaneOpenStyle::FadeSlideUp => "fade-slide-up".into(),
        },
        SettingsField::RenderBackend => match c.render.backend {
            RenderBackend::Auto => "auto".into(),
            RenderBackend::Blade => "blade".into(),
            RenderBackend::Gl => "gl".into(),
        },
        SettingsField::RenderPresentMode => match c.render.present_mode {
            PresentMode::Fifo => "fifo".into(),
            PresentMode::Mailbox => "mailbox".into(),
            PresentMode::Immediate => "immediate".into(),
        },
        SettingsField::RenderAlphaBlending => match c.render.alpha_blending {
            AlphaBlending::Native => "native".into(),
            AlphaBlending::Linear => "linear".into(),
            AlphaBlending::LinearCorrected => "linear-corrected".into(),
        },
        _ => String::new(),
    }
}

/// Pretty-print a row's current value for the row's value cell.
pub fn display_value(field: SettingsField, c: &LoomConfig) -> String {
    match meta(field).kind {
        FieldKind::Float { precision, .. } => {
            format!("{:.*}", precision as usize, read_float(field, c))
        }
        FieldKind::Int { .. } => read_int(field, c).to_string(),
        FieldKind::Bool => if read_bool(field, c) { "On" } else { "Off" }.to_string(),
        FieldKind::Enum { .. } => read_enum(field, c),
    }
}

// ── Mutation ────────────────────────────────────────────────────────

/// Apply a stepper press. Clamps to the field's `[min, max]`.
/// Returns `true` if the value actually changed (callers use this to
/// gate redraws / writes).
pub fn nudge(field: SettingsField, c: &mut LoomConfig, delta_sign: i32) -> bool {
    match meta(field).kind {
        FieldKind::Float { min, max, step, .. } => {
            let current = read_float(field, c);
            let next = (current + step * delta_sign as f32).clamp(min, max);
            if (next - current).abs() < f32::EPSILON {
                return false;
            }
            write_float(field, c, next);
            true
        }
        FieldKind::Int { min, max, step } => {
            let current = read_int(field, c) as isize;
            let raw = current + step as isize * delta_sign as isize;
            let next = raw.clamp(min as isize, max as isize) as usize;
            if next == read_int(field, c) {
                return false;
            }
            write_int(field, c, next);
            true
        }
        FieldKind::Bool | FieldKind::Enum { .. } => false,
    }
}

/// Flip a boolean field. Returns the new value, or `None` if the
/// field isn't `Bool`-kinded.
pub fn toggle(field: SettingsField, c: &mut LoomConfig) -> Option<bool> {
    if !matches!(meta(field).kind, FieldKind::Bool) {
        return None;
    }
    let next = !read_bool(field, c);
    write_bool(field, c, next);
    Some(next)
}

/// Pick a value for an enum-typed field. Strings come from
/// [`enum_variants`] (kebab-case). Returns `true` if the value
/// actually changed.
pub fn set_enum(field: SettingsField, c: &mut LoomConfig, value: &str) -> bool {
    if read_enum(field, c) == value && !matches!(field, SettingsField::ThemePreset) {
        return false;
    }
    match field {
        SettingsField::ThemePreset => {
            // Theme preset write goes through the dedicated
            // `apply_theme_preset` path, NOT this generic setter, so
            // the chrome is re-resolved. The dispatcher routes
            // `SetSettingsEnum { ThemePreset, .. }` to `apply_theme_preset`
            // before reaching here.
            c.theme.preset = value.to_string();
        }
        SettingsField::FontFamily => {
            c.font.family = value.to_string();
        }
        SettingsField::UiFontFamily => {
            // `system` clears the override; anything else creates /
            // updates the `[font.ui]` struct in-place. We preserve
            // any existing size when only the family changes.
            if value == "system" {
                c.font.ui = None;
            } else {
                let size = c.font.ui.as_ref().map(|u| u.size).unwrap_or(c.font.size);
                c.font.ui = Some(UiFontConfig {
                    family: value.to_string(),
                    size,
                });
            }
        }
        SettingsField::FontLineHeight => {
            // The picker only offers `standard` / `comfortable`;
            // anything else (including the read-only `custom` label)
            // leaves the multiplier untouched.
            c.font.adjust_cell_height = match value {
                "standard" => 1.0,
                "comfortable" => 1.2,
                _ => return false,
            };
        }
        SettingsField::AnimationPreset => {
            c.animation.preset = match value {
                "snappy" => AnimationPreset::Snappy,
                "smooth" => AnimationPreset::Smooth,
                "gentle" => AnimationPreset::Gentle,
                _ => AnimationPreset::Default,
            };
        }
        SettingsField::InputMode => {
            c.input.mode = match value {
                "sticky" => InputMode::Sticky,
                _ => InputMode::Prefix,
            };
        }
        SettingsField::StatusBarPosition => {
            c.statusbar.position = match value {
                "bottom" => StatusBarPosition::Bottom,
                _ => StatusBarPosition::Top,
            };
        }
        SettingsField::PredictionMode => {
            c.prediction.mode = match value {
                "always" => PredictionMode::Always,
                "adaptive" => PredictionMode::Adaptive,
                _ => PredictionMode::Never,
            };
        }
        SettingsField::LayoutCenterFocusedColumn => {
            c.layout.center_focused_column = match value {
                "always" => CenterStrategy::Always,
                "on-overflow" => CenterStrategy::OnOverflow,
                _ => CenterStrategy::Never,
            };
        }
        SettingsField::LayoutNewPaneSizing => {
            c.layout.new_pane_sizing = match value {
                "dynamic" => NewPaneSizing::Dynamic,
                _ => NewPaneSizing::Fixed,
            };
        }
        SettingsField::LayoutNewPaneWidth => {
            c.layout.new_pane_width = match value {
                "full" => NewPaneWidth::Full,
                _ => NewPaneWidth::Half,
            };
        }
        SettingsField::AppearanceFocusRingStyle => {
            c.appearance.focus_ring.style = match value {
                "glow" => FocusRingStyle::Glow,
                "dashed" => FocusRingStyle::Dashed,
                _ => FocusRingStyle::Solid,
            };
        }
        SettingsField::FontDisableLigatures => {
            c.font.disable_ligatures = match value {
                "cursor" => DisableLigatures::Cursor,
                "always" => DisableLigatures::Always,
                _ => DisableLigatures::Never,
            };
        }
        SettingsField::TerminalCursorShape => {
            c.terminal.cursor_shape = value.to_string();
        }
        SettingsField::TabBarPosition => {
            c.tabbar.position = match value {
                "left" => TabBarPosition::Left,
                "right" => TabBarPosition::Right,
                _ => TabBarPosition::Integrated,
            };
        }
        SettingsField::AnimationPaneOpenStyle => {
            c.animation.pane_open_style = match value {
                "slide-up" => PaneOpenStyle::SlideUp,
                "slide-down" => PaneOpenStyle::SlideDown,
                "slide-left" => PaneOpenStyle::SlideLeft,
                "fade-slide-up" => PaneOpenStyle::FadeSlideUp,
                _ => PaneOpenStyle::Fade,
            };
        }
        SettingsField::RenderBackend => {
            c.render.backend = match value {
                "blade" => RenderBackend::Blade,
                "gl" => RenderBackend::Gl,
                _ => RenderBackend::Auto,
            };
        }
        SettingsField::RenderPresentMode => {
            c.render.present_mode = match value {
                "mailbox" => PresentMode::Mailbox,
                "immediate" => PresentMode::Immediate,
                _ => PresentMode::Fifo,
            };
        }
        SettingsField::RenderAlphaBlending => {
            c.render.alpha_blending = match value {
                "native" => AlphaBlending::Native,
                "linear" => AlphaBlending::Linear,
                _ => AlphaBlending::LinearCorrected,
            };
        }
        _ => {
            return false;
        }
    }
    true
}

fn write_float(field: SettingsField, c: &mut LoomConfig, v: f32) {
    match field {
        SettingsField::AppearancePaneOpacity => c.appearance.pane_opacity = v,
        SettingsField::AppearancePaneCornerRadius => c.appearance.pane_corner_radius = v,
        SettingsField::AppearancePadding => c.appearance.padding = v,
        SettingsField::AppearanceColumnGap => c.appearance.column_gap = v,
        SettingsField::AppearanceInactiveOpacity => c.appearance.inactive_opacity = v,
        SettingsField::AppearanceBackgroundDim => c.appearance.background_dim = v,
        SettingsField::AppearanceBorderWidth => c.appearance.border_width = v,
        SettingsField::FontCellWidth => c.font.adjust_cell_width = v,
        SettingsField::FontUnderlinePosition => c.font.adjust_underline_position = v,
        SettingsField::FontUnderlineThickness => c.font.adjust_underline_thickness = v,
        SettingsField::FontStrikethroughPosition => c.font.adjust_strikethrough_position = v,
        SettingsField::FontStrikethroughThickness => c.font.adjust_strikethrough_thickness = v,
        SettingsField::TerminalCursorOpacity => c.terminal.cursor_opacity = v,
        SettingsField::TabBarWidth => c.tabbar.width = v,
        SettingsField::TabBarTabHeight => c.tabbar.tab_height = v,
        SettingsField::TabBarTabGap => c.tabbar.tab_gap = v,
        SettingsField::StatusBarPaddingRatio => c.statusbar.padding_ratio = v,
        SettingsField::AnimationDragOpacity => c.animation.drag_opacity = v,
        SettingsField::AnimationOverviewZoomFit => c.animation.overview_zoom_fit = v,
        SettingsField::InputScrollMultiplier => c.input.scroll_multiplier = v as f64,
        SettingsField::GesturePinchSensitivity => c.gesture.pinch_sensitivity = v as f64,
        SettingsField::GestureScrollPixelsPerLine => c.gesture.scroll_pixels_per_line = v as f64,
        SettingsField::RenderSoftness => c.render.softness = v,
        SettingsField::FontSize => c.font.size = v,
        SettingsField::UiFontSize => {
            // Stepping the UI size implies the user wants an override —
            // create the `[font.ui]` struct if it doesn't exist yet.
            // Family preserves whatever was there (empty string =
            // "use system" inside the struct, matching the loader).
            let family = c
                .font
                .ui
                .as_ref()
                .map(|u| u.family.clone())
                .unwrap_or_default();
            c.font.ui = Some(UiFontConfig { family, size: v });
        }
        _ => {}
    }
}

fn write_int(field: SettingsField, c: &mut LoomConfig, v: usize) {
    match field {
        SettingsField::TerminalScrollbackLines => c.terminal.scrollback_lines = v,
        SettingsField::FontWeight => c.font.weight = Some(v.clamp(100, 1000) as u16),
        SettingsField::TerminalCursorBlinkInterval => {
            c.terminal.cursor_blink_interval_ms = v as u64
        }
        SettingsField::TerminalPasteWarnThreshold => c.terminal.paste_warn_threshold = v,
        SettingsField::TerminalDefaultCols => c.terminal.default_cols = v as u16,
        SettingsField::TerminalDefaultRows => c.terminal.default_rows = v as u16,
        SettingsField::TerminalNotifyThreshold => {
            c.terminal.notify_command_threshold_secs = v as u64
        }
        SettingsField::TabBarPaneTabWidthChars => c.tabbar.pane_tab_width_chars = v,
        SettingsField::InputLeaderTimeout => c.input.leader_timeout_ms = v as u64,
        SettingsField::InputDoubleTapWindow => c.input.double_tap_window_ms = v as u64,
        SettingsField::RenderFrameInterval => c.render.frame_interval_ms = v as u64,
        SettingsField::PredictionThreshold => c.prediction.threshold_ms = v as u64,
        SettingsField::SessionAgentSaveInterval => c.session.agent_save_interval_secs = v as u64,
        SettingsField::ServerIdleTimeout => c.server.idle_timeout_secs = v as u64,
        SettingsField::LayoutDynamicFullscreenWidth => {
            c.layout.dynamic_fullscreen_max_width = v as f64
        }
        _ => {}
    }
}

fn write_bool(field: SettingsField, c: &mut LoomConfig, v: bool) {
    match field {
        SettingsField::TerminalCursorBlink => c.terminal.cursor_blink = v,
        SettingsField::TerminalCopyOnSelect => c.terminal.copy_on_select = v,
        SettingsField::AnimationEnabled => c.animation.enabled = v,
        SettingsField::InputFocusFollowsMouse => c.input.focus_follows_mouse = v,
        SettingsField::TerminalClearSelectionOnType => c.terminal.clear_selection_on_type = v,
        SettingsField::TerminalBellUrgency => c.terminal.bell_urgency = v,
        SettingsField::GestureEnabled => c.gesture.enabled = v,
        SettingsField::GestureNaturalScroll => c.gesture.natural_scroll = v,
        SettingsField::GestureSmoothScroll => c.gesture.smooth_scroll = v,
        SettingsField::PredictionShowUnderline => c.prediction.show_underline = v,
        SettingsField::SessionRestoreAgents => c.session.restore_agents = v,
        _ => {}
    }
}

// ── Persistence ─────────────────────────────────────────────────────
//
// Writers are dispatched per-field on save so the `EditableConfig`
// API stays narrow (one setter per leaf, no reflection).

pub fn write_to_disk(field: SettingsField, c: &LoomConfig, w: &mut EditableConfig) {
    match field {
        SettingsField::ThemePreset => w.set_theme_preset(&c.theme.preset),
        SettingsField::AppearancePaneOpacity => {
            w.set_appearance_pane_opacity(c.appearance.pane_opacity)
        }
        SettingsField::AppearancePaneCornerRadius => {
            w.set_appearance_pane_corner_radius(c.appearance.pane_corner_radius)
        }
        SettingsField::AppearancePadding => w.set_appearance_padding(c.appearance.padding),
        SettingsField::AppearanceColumnGap => w.set_appearance_column_gap(c.appearance.column_gap),
        SettingsField::AppearanceInactiveOpacity => {
            w.set_appearance_inactive_opacity(c.appearance.inactive_opacity)
        }
        SettingsField::AppearanceBackgroundDim => {
            w.set_appearance_background_dim(c.appearance.background_dim)
        }
        SettingsField::FontFamily => w.set_font_family(&c.font.family),
        SettingsField::FontSize => w.set_font_size(c.font.size),
        SettingsField::FontWeight => {
            // Persist whatever the in-memory model now holds — `read_int`
            // would default `None` to 400, so the write path explicitly
            // mirrors what the stepper just produced.
            if let Some(w_val) = c.font.weight {
                w.set_font_weight(w_val);
            }
        }
        SettingsField::FontLineHeight => w.set_font_line_height(c.font.adjust_cell_height),
        SettingsField::UiFontFamily => {
            // `None` → write empty string (the writer removes the
            // whole `[font.ui]` table, matching the loader's "no
            // override" resting state).
            let family = c.font.ui.as_ref().map(|u| u.family.as_str()).unwrap_or("");
            w.set_font_ui_family(family);
        }
        SettingsField::UiFontSize => {
            // Only write the size when an override exists — if the
            // user reset to "system" we already wiped `[font.ui]`,
            // and re-asserting a size would re-create it with an
            // empty family.
            if let Some(u) = c.font.ui.as_ref() {
                w.set_font_ui_size(u.size);
            }
        }
        SettingsField::TerminalCursorBlink => w.set_terminal_cursor_blink(c.terminal.cursor_blink),
        SettingsField::TerminalCopyOnSelect => {
            w.set_terminal_copy_on_select(c.terminal.copy_on_select)
        }
        SettingsField::TerminalScrollbackLines => {
            w.set_terminal_scrollback_lines(c.terminal.scrollback_lines)
        }
        SettingsField::LayoutCenterFocusedColumn => {
            w.set_layout_center_focused_column(&read_enum(field, c))
        }
        SettingsField::LayoutNewPaneSizing => w.set_layout_new_pane_sizing(&read_enum(field, c)),
        SettingsField::LayoutNewPaneWidth => w.set_layout_new_pane_width(&read_enum(field, c)),
        SettingsField::LayoutDynamicFullscreenWidth => {
            w.set_layout_dynamic_fullscreen_max_width(c.layout.dynamic_fullscreen_max_width)
        }
        SettingsField::AnimationEnabled => w.set_animation_enabled(c.animation.enabled),
        SettingsField::AnimationPreset => w.set_animation_preset(&read_enum(field, c)),
        SettingsField::InputMode => w.set_input_mode(&read_enum(field, c)),
        SettingsField::InputFocusFollowsMouse => {
            w.set_input_focus_follows_mouse(c.input.focus_follows_mouse)
        }
        SettingsField::StatusBarPosition => w.set_statusbar_position(&read_enum(field, c)),
        SettingsField::PredictionMode => w.set_prediction_mode(&read_enum(field, c)),
        SettingsField::AppearanceBorderWidth => {
            w.set_appearance_border_width(c.appearance.border_width)
        }
        SettingsField::AppearanceFocusRingStyle => {
            w.set_appearance_focus_ring_style(&read_enum(field, c))
        }
        SettingsField::FontCellWidth => w.set_font_cell_width(c.font.adjust_cell_width),
        SettingsField::FontDisableLigatures => w.set_font_disable_ligatures(&read_enum(field, c)),
        SettingsField::FontUnderlinePosition => {
            w.set_font_underline_position(c.font.adjust_underline_position)
        }
        SettingsField::FontUnderlineThickness => {
            w.set_font_underline_thickness(c.font.adjust_underline_thickness)
        }
        SettingsField::FontStrikethroughPosition => {
            w.set_font_strikethrough_position(c.font.adjust_strikethrough_position)
        }
        SettingsField::FontStrikethroughThickness => {
            w.set_font_strikethrough_thickness(c.font.adjust_strikethrough_thickness)
        }
        SettingsField::TerminalCursorShape => w.set_terminal_cursor_shape(&c.terminal.cursor_shape),
        SettingsField::TerminalCursorOpacity => {
            w.set_terminal_cursor_opacity(c.terminal.cursor_opacity)
        }
        SettingsField::TerminalCursorBlinkInterval => {
            w.set_terminal_cursor_blink_interval(c.terminal.cursor_blink_interval_ms)
        }
        SettingsField::TerminalClearSelectionOnType => {
            w.set_terminal_clear_selection_on_type(c.terminal.clear_selection_on_type)
        }
        SettingsField::TerminalBellUrgency => w.set_terminal_bell_urgency(c.terminal.bell_urgency),
        SettingsField::TerminalPasteWarnThreshold => {
            w.set_terminal_paste_warn_threshold(c.terminal.paste_warn_threshold)
        }
        SettingsField::TerminalDefaultCols => w.set_terminal_default_cols(c.terminal.default_cols),
        SettingsField::TerminalDefaultRows => w.set_terminal_default_rows(c.terminal.default_rows),
        SettingsField::TerminalNotifyThreshold => {
            w.set_terminal_notify_command_threshold(c.terminal.notify_command_threshold_secs)
        }
        SettingsField::TabBarPosition => w.set_tabbar_position(&read_enum(field, c)),
        SettingsField::TabBarWidth => w.set_tabbar_width(c.tabbar.width),
        SettingsField::TabBarTabHeight => w.set_tabbar_tab_height(c.tabbar.tab_height),
        SettingsField::TabBarTabGap => w.set_tabbar_tab_gap(c.tabbar.tab_gap),
        SettingsField::TabBarPaneTabWidthChars => {
            w.set_tabbar_pane_tab_width_chars(c.tabbar.pane_tab_width_chars)
        }
        SettingsField::StatusBarPaddingRatio => {
            w.set_statusbar_padding_ratio(c.statusbar.padding_ratio)
        }
        SettingsField::AnimationPaneOpenStyle => {
            w.set_animation_pane_open_style(&read_enum(field, c))
        }
        SettingsField::AnimationDragOpacity => {
            w.set_animation_drag_opacity(c.animation.drag_opacity)
        }
        SettingsField::AnimationOverviewZoomFit => {
            w.set_animation_overview_zoom_fit(c.animation.overview_zoom_fit)
        }
        SettingsField::InputLeaderTimeout => w.set_input_leader_timeout(c.input.leader_timeout_ms),
        SettingsField::InputDoubleTapWindow => {
            w.set_input_double_tap_window(c.input.double_tap_window_ms)
        }
        SettingsField::InputScrollMultiplier => {
            w.set_input_scroll_multiplier(c.input.scroll_multiplier)
        }
        SettingsField::GestureEnabled => w.set_gesture_enabled(c.gesture.enabled),
        SettingsField::GestureNaturalScroll => {
            w.set_gesture_natural_scroll(c.gesture.natural_scroll)
        }
        SettingsField::GestureSmoothScroll => w.set_gesture_smooth_scroll(c.gesture.smooth_scroll),
        SettingsField::GesturePinchSensitivity => {
            w.set_gesture_pinch_sensitivity(c.gesture.pinch_sensitivity)
        }
        SettingsField::GestureScrollPixelsPerLine => {
            w.set_gesture_scroll_pixels_per_line(c.gesture.scroll_pixels_per_line)
        }
        SettingsField::RenderBackend => w.set_render_backend(&read_enum(field, c)),
        SettingsField::RenderPresentMode => w.set_render_present_mode(&read_enum(field, c)),
        SettingsField::RenderAlphaBlending => w.set_render_alpha_blending(&read_enum(field, c)),
        SettingsField::RenderSoftness => w.set_render_softness(c.render.softness),
        SettingsField::RenderFrameInterval => {
            w.set_render_frame_interval(c.render.frame_interval_ms)
        }
        SettingsField::PredictionThreshold => w.set_prediction_threshold(c.prediction.threshold_ms),
        SettingsField::PredictionShowUnderline => {
            w.set_prediction_show_underline(c.prediction.show_underline)
        }
        SettingsField::SessionRestoreAgents => {
            w.set_session_restore_agents(c.session.restore_agents)
        }
        SettingsField::SessionAgentSaveInterval => {
            w.set_session_agent_save_interval(c.session.agent_save_interval_secs)
        }
        SettingsField::ServerIdleTimeout => w.set_server_idle_timeout(c.server.idle_timeout_secs),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant of `SettingsField` MUST appear in `FIELDS`.
    /// Pin so adding a variant without a row entry trips immediately.
    #[test]
    fn every_field_has_a_meta_entry() {
        // We can't enumerate enum variants directly without a derive;
        // pin by walking FIELDS and confirming the count matches the
        // documented total. The next test pins the count itself.
        for m in FIELDS {
            // `meta()` panics if the lookup fails.
            let _ = meta(m.field);
        }
    }

    #[test]
    fn fields_table_has_the_documented_count() {
        // Bumping this is fine; the assertion exists so a stray edit
        // that drops a row gets caught instead of silently shrinking
        // the panel.
        assert_eq!(FIELDS.len(), 70);
    }

    #[test]
    fn nudge_clamps_at_bounds() {
        let mut cfg = LoomConfig::default();
        cfg.appearance.pane_opacity = 0.10;
        for _ in 0..100 {
            nudge(SettingsField::AppearancePaneOpacity, &mut cfg, -1);
        }
        assert!(
            (cfg.appearance.pane_opacity - 0.05).abs() < 1e-6,
            "got {}",
            cfg.appearance.pane_opacity
        );

        cfg.appearance.pane_opacity = 0.95;
        for _ in 0..100 {
            nudge(SettingsField::AppearancePaneOpacity, &mut cfg, 1);
        }
        assert!((cfg.appearance.pane_opacity - 1.0).abs() < 1e-6);
    }

    #[test]
    fn toggle_flips_bool_and_returns_new_state() {
        let mut cfg = LoomConfig::default();
        cfg.terminal.cursor_blink = true;
        let after = toggle(SettingsField::TerminalCursorBlink, &mut cfg);
        assert_eq!(after, Some(false));
        assert!(!cfg.terminal.cursor_blink);
    }

    #[test]
    fn toggle_rejects_non_bool_fields() {
        let mut cfg = LoomConfig::default();
        assert_eq!(toggle(SettingsField::AppearancePaneOpacity, &mut cfg), None);
    }

    #[test]
    fn set_enum_drives_input_mode_and_status_bar() {
        let mut cfg = LoomConfig::default();
        // Start from a known non-default state so the transition is a
        // real change regardless of what the default input mode is.
        cfg.input.mode = InputMode::Prefix;
        assert!(set_enum(SettingsField::InputMode, &mut cfg, "sticky"));
        assert_eq!(cfg.input.mode, InputMode::Sticky);
        assert!(set_enum(
            SettingsField::StatusBarPosition,
            &mut cfg,
            "bottom"
        ));
        assert_eq!(cfg.statusbar.position, StatusBarPosition::Bottom);
    }

    /// Walking every `FieldMeta` through `write_to_disk` exercises
    /// every per-field writer path. The output's TOML validity /
    /// loader round-trip is asserted by
    /// `loom_config::writer::tests::round_trip_through_loader_validates`,
    /// which has the `toml` crate dependency we don't carry here.
    #[test]
    fn write_to_disk_handles_every_field_without_panicking() {
        let cfg = LoomConfig::default();
        let path = std::env::temp_dir()
            .join("loom-settings-schema-tests")
            .join(format!(
                "write-each-{}.toml",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
            ));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut w = EditableConfig::load_from_path(&path).unwrap();
        for m in FIELDS {
            write_to_disk(m.field, &cfg, &mut w);
        }
        w.save().unwrap();
        // Just confirm the file isn't empty; the loader-side
        // validation test in loom-config covers semantic correctness.
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.is_empty());
    }

    #[test]
    fn from_id_round_trips() {
        for m in FIELDS {
            assert_eq!(SettingsField::from_id(m.field.id()), Some(m.field));
        }
    }
}
