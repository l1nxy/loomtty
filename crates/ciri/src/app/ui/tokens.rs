//! Design tokens — centralized spacing / alpha / control-sizing constants.
//!
//! Components should consume these instead of hard-coding pixel offsets,
//! alpha values, or control heights. Colors themselves still come from
//! `ciri_config::theme::ThemeConfig` so users can re-skin; tokens only
//! describe *how* colors are composed (e.g. selection tint alpha), not
//! which hue.
//!
//! The spacing scale is an 8-point grid with 4px subdivision, matching
//! common UI kits. Stick to these values — a new literal `7.0` in a
//! component is a smell.

#![allow(dead_code)] // reserved tokens for upcoming components

// ── Spacing (8pt grid, 4pt subdivision) ───────────────────────────────

pub const SPACE_1: f32 = 4.0;
pub const SPACE_2: f32 = 8.0;
pub const SPACE_3: f32 = 12.0;
pub const SPACE_4: f32 = 16.0;
pub const SPACE_6: f32 = 24.0;

// ── Alpha layers ──────────────────────────────────────────────────────
//
// Every overlay / tint in the UI should pick from this small set so
// selection / hover / disabled states feel the same across components.

/// Modal backdrop dim — darkens the viewport behind palette / dialogs.
pub const ALPHA_BACKDROP: f32 = 0.55;
/// Background tint for a selected row (using accent color).
pub const ALPHA_SELECTED_BG: f32 = 0.25;
/// Background tint for a hovered row (using accent color).
pub const ALPHA_HOVER_BG: f32 = 0.14;
/// Background tint for an always-active tab (using accent color). Weaker
/// than selection so the active-tab accent strip stays the dominant cue.
pub const ALPHA_TAB_ACTIVE_BG: f32 = 0.10;
/// Background tint for a pressed / primary-button resting state.
pub const ALPHA_PRIMARY_REST: f32 = 0.55;
/// Background tint for a pressed / primary-button hover state.
pub const ALPHA_PRIMARY_HOVER: f32 = 0.80;
/// Background tint for a secondary / ghost button resting state.
pub const ALPHA_SECONDARY_REST: f32 = 0.18;
/// Background tint for a secondary / ghost button hover state.
pub const ALPHA_SECONDARY_HOVER: f32 = 0.32;
/// Subtle header strip (e.g. info-box title row on accent).
pub const ALPHA_TINT_HEADER: f32 = 0.20;
/// Separator / hairline over a bar background.
pub const ALPHA_SEPARATOR: f32 = 0.25;
/// Scrollbar track (over background).
pub const ALPHA_SCROLL_TRACK: f32 = 0.20;
/// Scrollbar thumb (accent).
pub const ALPHA_SCROLL_THUMB: f32 = 0.65;
/// Text-cursor rectangle over foreground color.
pub const ALPHA_CURSOR: f32 = 0.80;

// ── Border widths ─────────────────────────────────────────────────────

/// Default hairline border (panels, modals).
pub const BORDER_THIN: f32 = 1.0;
/// Emphasized border — reserve for focus rings / active elements.
pub const BORDER_THICK: f32 = 2.0;

// ── Control sizing ────────────────────────────────────────────────────
//
// Express row / button heights as `cell_h + vertical_padding * 2` so they
// scale with the glyph size. Vertical padding uses the spacing scale.

/// Compact control — one row of text + 4px padding each side.
pub fn control_height_sm(cell_h: f32) -> f32 {
    cell_h + SPACE_1 * 2.0
}
/// Standard control — one row of text + 8px padding each side.
pub fn control_height_md(cell_h: f32) -> f32 {
    cell_h + SPACE_2 * 2.0
}
/// Prominent control — one row of text + 12px padding each side.
pub fn control_height_lg(cell_h: f32) -> f32 {
    cell_h + SPACE_3 * 2.0
}

// ── Helpers ───────────────────────────────────────────────────────────

/// Overlay a theme RGB color with a given alpha, preserving RGB channels.
#[inline]
pub fn tint(rgb: [f32; 4], alpha: f32) -> [f32; 4] {
    [rgb[0], rgb[1], rgb[2], alpha]
}
