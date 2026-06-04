//! Design tokens — centralized spacing / alpha / control-sizing constants.
//!
//! Components should consume these instead of hard-coding pixel offsets,
//! alpha values, or control heights. Colors themselves still come from
//! `loom_config::theme::ThemeConfig` so users can re-skin; tokens only
//! describe *how* colors are composed (e.g. selection tint alpha), not
//! which hue.
//!
//! The spacing scale is an 8-point grid with 4px subdivision, matching
//! common UI kits. Stick to these values — a new literal `7.0` in a
//! component is a smell.
//!
//! Duplication note: `loom_ui::theme::SpaceScale` / `RadiusScale` carry
//! the same numbers under `s1 / s2 / …` field names. The two tables are
//! intentionally kept in lock-step while components move toward direct
//! `theme.space.s*` reads; the `spacing_scales_agree_*` static asserts
//! below will fail a build if either side drifts.

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
/// Background tint for an actively pressed row (using accent color).
/// Slightly deeper than hover so the user feels the press; lighter
/// than `ALPHA_SELECTED_BG` to keep selection the dominant cue.
pub const ALPHA_PRESS_BG: f32 = 0.22;
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

// ── Status-line sections (lualine-style) ─────────────────────────────
//
// The status bar is a row of solid-colour sections that tile the bar
// edge-to-edge — no gaps, no rounded corners, full bar height. Only
// the text inside each section is padded.

/// Horizontal padding from the section edge to the text inside.
pub const SEGMENT_PAD_X: f32 = 10.0;

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

// ── Surface elevation ─────────────────────────────────────────────────
//
// Flat additive deltas on sRGB background channels to distinguish stacked
// surfaces (input row above panel, recessed preview below panel, etc.).
// Small because values are sRGB-encoded; the visual lift is non-linear.

/// Raise a surface slightly above its backdrop (e.g. palette input row,
/// palette outer panel). Intended as the first elevation step over
/// `term_bg` — two stacked raises (panel + input row) read as a clear
/// two-level hierarchy.
pub const SURFACE_LIFT: f32 = 0.05;
/// Raise a surface more prominently — use when a stacked layer needs to
/// sit clearly above a layer that is itself already raised (e.g. the
/// palette input row over the palette panel body).
pub const SURFACE_LIFT_HIGH: f32 = 0.10;
/// Raise a surface mildly (e.g. paste dialog container over terminal bg).
pub const SURFACE_LIFT_SUBTLE: f32 = 0.03;
/// Sink a surface below its backdrop (e.g. paste preview recess).
pub const SURFACE_SINK: f32 = 0.04;

/// Raise an sRGB color by `delta`, preserving alpha and clamping to `[0, 1]`.
#[inline]
pub fn surface_raise(rgb: [f32; 4], delta: f32) -> [f32; 4] {
    [
        (rgb[0] + delta).min(1.0),
        (rgb[1] + delta).min(1.0),
        (rgb[2] + delta).min(1.0),
        rgb[3],
    ]
}

/// Lower an sRGB color by `delta`, preserving alpha and clamping to `[0, 1]`.
#[inline]
pub fn surface_sink(rgb: [f32; 4], delta: f32) -> [f32; 4] {
    [
        (rgb[0] - delta).max(0.0),
        (rgb[1] - delta).max(0.0),
        (rgb[2] - delta).max(0.0),
        rgb[3],
    ]
}

// ── Helpers ───────────────────────────────────────────────────────────

/// Overlay a theme RGB color with a given alpha, preserving RGB channels.
#[inline]
pub fn tint(rgb: [f32; 4], alpha: f32) -> [f32; 4] {
    [rgb[0], rgb[1], rgb[2], alpha]
}

// Guardrail: the two spacing tables must stay numerically identical while
// tokens.rs remains a compatibility layer over loom-ui's theme scale.
// Runtime assertion rather than a `const _: () = assert!(...)` because
// `SpaceScale::default()` isn't const.
#[cfg(test)]
mod scale_sync_tests {
    use super::*;
    use loom_ui::theme::SpaceScale;

    #[test]
    fn spacing_scales_agree_with_loom_ui_theme() {
        let s = SpaceScale::default();
        assert_eq!(s.s1, SPACE_1);
        assert_eq!(s.s2, SPACE_2);
        assert_eq!(s.s3, SPACE_3);
        assert_eq!(s.s4, SPACE_4);
        assert_eq!(s.s6, SPACE_6);
    }

    /// `SpaceScale` has an `s8` field that tokens.rs does not expose —
    /// pin its expected value so drift on the loom-ui side is caught
    /// before a migrated component picks it up and diverges.
    #[test]
    fn loom_ui_s8_pinned_at_32() {
        assert_eq!(SpaceScale::default().s8, 32.0);
    }
}
