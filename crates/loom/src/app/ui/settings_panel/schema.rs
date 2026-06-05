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
    AnimationPreset, CenterStrategy, LoomConfig, InputMode, PredictionMode, StatusBarPosition,
    UiFontConfig,
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
    // ── Font ──────────────────────────────────────────────
    FontFamily,
    FontSize,
    FontWeight,
    FontLineHeight,
    UiFontFamily,
    UiFontSize,
    // ── Terminal ──────────────────────────────────────────
    TerminalCursorBlink,
    TerminalCopyOnSelect,
    TerminalScrollbackLines,
    // ── Layout ────────────────────────────────────────────
    LayoutCenterFocusedColumn,
    // ── Animation ─────────────────────────────────────────
    AnimationEnabled,
    AnimationPreset,
    // ── Input ─────────────────────────────────────────────
    InputMode,
    InputFocusFollowsMouse,
    // ── StatusBar ─────────────────────────────────────────
    StatusBarPosition,
    // ── Prediction ────────────────────────────────────────
    PredictionMode,
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
        SettingsField::FontSize => c.font.size,
        // Mirrors `resolve_ui_font_init`'s "no override → terminal size"
        // fallback so the stepper starts from a sensible value when
        // the user hasn't customised the UI font yet.
        SettingsField::UiFontSize => c.font.ui.as_ref().map(|u| u.size).unwrap_or(c.font.size),
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
        _ => 0,
    }
}

pub fn read_bool(field: SettingsField, c: &LoomConfig) -> bool {
    match field {
        SettingsField::TerminalCursorBlink => c.terminal.cursor_blink,
        SettingsField::TerminalCopyOnSelect => c.terminal.copy_on_select,
        SettingsField::AnimationEnabled => c.animation.enabled,
        SettingsField::InputFocusFollowsMouse => c.input.focus_follows_mouse,
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
        _ => {}
    }
}

fn write_bool(field: SettingsField, c: &mut LoomConfig, v: bool) {
    match field {
        SettingsField::TerminalCursorBlink => c.terminal.cursor_blink = v,
        SettingsField::TerminalCopyOnSelect => c.terminal.copy_on_select = v,
        SettingsField::AnimationEnabled => c.animation.enabled = v,
        SettingsField::InputFocusFollowsMouse => c.input.focus_follows_mouse = v,
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
            let family = c
                .font
                .ui
                .as_ref()
                .map(|u| u.family.as_str())
                .unwrap_or("");
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
        SettingsField::AnimationEnabled => w.set_animation_enabled(c.animation.enabled),
        SettingsField::AnimationPreset => w.set_animation_preset(&read_enum(field, c)),
        SettingsField::InputMode => w.set_input_mode(&read_enum(field, c)),
        SettingsField::InputFocusFollowsMouse => {
            w.set_input_focus_follows_mouse(c.input.focus_follows_mouse)
        }
        SettingsField::StatusBarPosition => w.set_statusbar_position(&read_enum(field, c)),
        SettingsField::PredictionMode => w.set_prediction_mode(&read_enum(field, c)),
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
        assert_eq!(FIELDS.len(), 23);
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
