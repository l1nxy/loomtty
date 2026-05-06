//! Pre-resolved theme tokens.
//!
//! Turns `ciri_config::theme::ThemeConfig` (hex strings) into sRGB-encoded
//! RGBA ready for the GPU, computed once per theme change. sRGB — not
//! linear — to match the existing ciri-gpu pipeline, which does not
//! gamma-decode its inputs before blending (see `ResolvedTheme`'s doc
//! comment for the rationale and the regression this fixes). Fixes the
//! parse-hex-per-frame smell in the current chrome path.
//!
//! The token schema is semantic (surface / on-surface / accent / …) rather
//! than terminal-centric so plugin UIs and future theme-aware widgets can
//! read one stable API.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::color::{Color, contrast_on, mul_alpha, scale_rgb};
use ciri_config::theme::ThemeConfig;

/// Monotonic counter backing [`ResolvedTheme::version`]. Bumped on every
/// `from_config` call so cache keys stay sound regardless of whether the
/// caller uses `reload()` or constructs a fresh theme.
static THEME_VERSION: AtomicU64 = AtomicU64::new(1);

fn next_theme_version() -> u64 {
    THEME_VERSION.fetch_add(1, Ordering::Relaxed)
}

/// Spacing scale — roughly a 4pt grid. Units are logical px at DPR = 1.0.
#[derive(Clone, Copy, Debug)]
pub struct SpaceScale {
    pub s1: f32,
    pub s2: f32,
    pub s3: f32,
    pub s4: f32,
    pub s6: f32,
    pub s8: f32,
}

impl Default for SpaceScale {
    fn default() -> Self {
        Self {
            s1: 4.0,
            s2: 8.0,
            s3: 12.0,
            s4: 16.0,
            s6: 24.0,
            s8: 32.0,
        }
    }
}

/// Corner-radius scale.
#[derive(Clone, Copy, Debug)]
pub struct RadiusScale {
    pub none: f32,
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
    pub full: f32,
}

impl Default for RadiusScale {
    fn default() -> Self {
        Self {
            none: 0.0,
            sm: 6.0,
            md: 10.0,
            lg: 14.0,
            full: 9999.0,
        }
    }
}

/// Typography scale (logical px).
#[derive(Clone, Copy, Debug)]
pub struct TypeScale {
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
}

impl Default for TypeScale {
    fn default() -> Self {
        Self {
            sm: 11.0,
            md: 13.0,
            lg: 16.0,
        }
    }
}

/// Fully-resolved theme tokens. All colors are sRGB-encoded RGBA (straight
/// alpha) — the same space `ThemeConfig` hex strings parse into. This
/// matches the existing `Rect` / glyph pipeline, which does not gamma-decode
/// its inputs. Consumers that want linear blending must decode themselves.
///
/// Construct via [`ResolvedTheme::from_config`] whenever the `ThemeConfig`
/// changes (startup + config reload). Never parse hex inside `paint`.
#[derive(Clone, Debug)]
pub struct ResolvedTheme {
    // Surface tokens
    pub surface: Color,
    pub surface_elevated: Color,
    pub surface_sunken: Color,
    pub surface_overlay: Color,
    // Content tokens
    pub on_surface: Color,
    pub on_surface_muted: Color,
    pub on_surface_disabled: Color,
    // Accent tokens
    pub accent: Color,
    pub on_accent: Color,
    /// Muted foreground that stays legible on `tint(accent, …)` backgrounds.
    /// Use this for secondary text placed over accent-tinted chrome rows —
    /// `on_surface_muted` is tuned for the `surface` backdrop and can drop
    /// below readable contrast when the accent hue shifts close to it.
    pub on_accent_muted: Color,
    pub accent_muted: Color,
    // Semantic status
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub info: Color,
    // Border
    pub border: Color,
    pub border_focus: Color,
    // Chrome-specific tokens that have no derived counterpart.
    pub statusbar_bg: Color,
    pub broadcast: Color,
    // Terminal palette — needed by overlays that sit on top of cells
    pub term_fg: Color,
    pub term_bg: Color,
    // Scales
    pub space: SpaceScale,
    pub radius: RadiusScale,
    pub typography: TypeScale,
    /// Unique, monotonically increasing id bumped by every `from_config`
    /// call (via a global counter). Stable to use as a cache-key component:
    /// two resolved themes with the same `version` are the same instance,
    /// and every constructor call produces a fresh `version` — including
    /// when the caller rebuilds the struct themselves rather than going
    /// through `reload()`.
    pub version: u64,
}

impl ResolvedTheme {
    /// Convert a `ThemeConfig` (hex strings) into resolved tokens.
    ///
    /// Values are kept in sRGB space (no gamma-decode) to match the
    /// existing ciri-gpu blend / shader path, which renders hex-coded
    /// theme colors directly to the swapchain without sRGB encoding on
    /// write. Going through `parse_color_linear` here produced values
    /// ~20× darker than what the legacy rect pipeline puts on screen,
    /// so SDF-backed widgets (palette panel, etc.) ended up darker
    /// than the backdrop-dimmed pane and looked transparent. When the
    /// GPU pipeline grows an sRGB-aware surface format this is the one
    /// spot that should flip back to linear.
    ///
    /// Derived tokens (`surface_elevated`, `accent_muted`, etc.) are
    /// computed mechanically from base fields so theme authors only need to
    /// set the handful of colors that exist in `ThemeConfig` today.
    pub fn from_config(cfg: &ThemeConfig) -> Self {
        let surface = ThemeConfig::parse_color(cfg.background.as_ref());
        let bg_term = ThemeConfig::parse_color(cfg.background.as_ref());
        let on_surface = ThemeConfig::parse_color(cfg.foreground.as_ref());
        let muted = ThemeConfig::parse_color(cfg.statusbar_dim.as_ref());
        let accent = ThemeConfig::parse_color(cfg.accent.as_ref());
        let err = ThemeConfig::parse_color(cfg.red.as_ref());
        let warn = ThemeConfig::parse_color(cfg.yellow.as_ref());
        let ok = ThemeConfig::parse_color(cfg.green.as_ref());
        let info = ThemeConfig::parse_color(cfg.blue.as_ref());
        let border_a = ThemeConfig::parse_color(cfg.border_active.as_ref());
        let border_i = ThemeConfig::parse_color(cfg.border_inactive.as_ref());
        let statusbar_bg = ThemeConfig::parse_color(cfg.statusbar_background.as_ref());
        let broadcast = ThemeConfig::parse_color(cfg.mode_broadcast.as_ref());

        Self {
            surface,
            surface_elevated: scale_rgb(surface, 1.12),
            surface_sunken: scale_rgb(surface, 0.88),
            surface_overlay: mul_alpha(surface, 0.55),
            on_surface,
            on_surface_muted: muted,
            on_surface_disabled: scale_rgb(muted, 0.6),
            accent,
            on_accent: contrast_on(accent),
            // Muted foreground for text painted over `tint(accent, …)`.
            // We can't just dim `on_accent` — a near-black `contrast_on`
            // result would collapse toward the backdrop and disappear.
            // Instead, render `on_accent` at reduced alpha so the straight
            // color is preserved and the mute effect comes from blending
            // with the accent underneath.
            on_accent_muted: mul_alpha(contrast_on(accent), 0.7),
            accent_muted: scale_rgb(accent, 0.55),
            success: ok,
            warning: warn,
            error: err,
            info,
            border: border_i,
            border_focus: border_a,
            statusbar_bg,
            broadcast,
            term_fg: on_surface,
            term_bg: bg_term,
            space: SpaceScale::default(),
            radius: RadiusScale::default(),
            typography: TypeScale::default(),
            version: next_theme_version(),
        }
    }

    /// Rebuild from a new config. `version` is fresh — callers can compare
    /// `theme.version` before and after to detect that the theme changed.
    pub fn reload(&mut self, cfg: &ThemeConfig) {
        *self = Self::from_config(cfg);
    }
}

impl Default for ResolvedTheme {
    /// An opinionated dark theme for tests / headless builds where no
    /// `ThemeConfig` is available. Not reachable from production code.
    fn default() -> Self {
        Self {
            surface: [0.10, 0.10, 0.12, 1.0],
            surface_elevated: [0.14, 0.14, 0.16, 1.0],
            surface_sunken: [0.07, 0.07, 0.09, 1.0],
            surface_overlay: [0.10, 0.10, 0.12, 0.55],
            on_surface: [0.90, 0.90, 0.92, 1.0],
            on_surface_muted: [0.55, 0.55, 0.60, 1.0],
            on_surface_disabled: [0.35, 0.35, 0.40, 1.0],
            accent: [0.30, 0.68, 0.80, 1.0],
            on_accent: [0.05, 0.05, 0.06, 1.0],
            // Alpha 0.7 mirrors `from_config`'s `mul_alpha(contrast_on(accent), 0.7)`
            // contract — a muted foreground blends toward the backdrop via
            // alpha rather than toward black/white via channel scaling.
            on_accent_muted: [0.05, 0.05, 0.06, 0.7],
            accent_muted: [0.18, 0.40, 0.48, 1.0],
            success: [0.40, 0.72, 0.38, 1.0],
            warning: [0.90, 0.68, 0.24, 1.0],
            error: [0.85, 0.32, 0.30, 1.0],
            info: [0.38, 0.60, 0.90, 1.0],
            border: [0.22, 0.22, 0.25, 1.0],
            border_focus: [0.30, 0.68, 0.80, 1.0],
            statusbar_bg: [0.08, 0.08, 0.10, 1.0],
            broadcast: [0.85, 0.32, 0.30, 1.0],
            term_fg: [0.90, 0.90, 0.92, 1.0],
            term_bg: [0.07, 0.07, 0.09, 1.0],
            space: SpaceScale::default(),
            radius: RadiusScale::default(),
            typography: TypeScale::default(),
            // 0 is reserved for "synthetic / never constructed from a
            // real config" so production cache keys can't collide with
            // the test-only Default themes.
            version: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_defaults_are_nonzero() {
        let t = ResolvedTheme::default();
        assert!(t.space.s2 > 0.0);
        assert!(t.radius.md > 0.0);
        assert!(t.typography.md > 0.0);
    }

    #[test]
    fn reload_bumps_version() {
        let mut t = ResolvedTheme::default();
        let v0 = t.version;
        let mut cfg = ciri_config::theme::ThemeConfig::default();
        cfg.resolve_preset();
        t.reload(&cfg);
        assert_ne!(t.version, v0);
    }

    /// Regression for Codex P3: two successive `from_config` calls must
    /// produce distinct versions so callers that key caches off
    /// `theme.version` (without going through `reload()`) don't miss
    /// the first theme change.
    #[test]
    fn from_config_bumps_version_on_each_call() {
        let mut cfg = ciri_config::theme::ThemeConfig::default();
        cfg.resolve_preset();
        let a = ResolvedTheme::from_config(&cfg);
        let b = ResolvedTheme::from_config(&cfg);
        assert_ne!(a.version, b.version);
        assert!(b.version > a.version, "version must be monotonic");
    }

    #[test]
    fn default_theme_uses_sentinel_version_zero() {
        let t = ResolvedTheme::default();
        assert_eq!(t.version, 0);
    }

    #[test]
    fn from_config_keeps_widget_theme_colors_in_srgb_space() {
        let mut cfg = ciri_config::theme::ThemeConfig {
            preset: "dracula".into(),
            ..Default::default()
        };
        cfg.resolve_preset();

        let theme = ResolvedTheme::from_config(&cfg);

        assert_eq!(
            theme.term_bg,
            ThemeConfig::parse_color(cfg.background.as_ref()),
            "SDF-backed widgets share the same color space as the legacy rect pipeline",
        );
        assert_eq!(
            theme.border_focus,
            ThemeConfig::parse_color(cfg.border_active.as_ref()),
            "focused borders must not be gamma-decoded before the current GPU pipeline",
        );
        assert_ne!(
            theme.term_bg,
            ThemeConfig::parse_color_linear(cfg.background.as_ref()),
            "linear-decoding the theme would darken palette panels enough to read as transparent",
        );
    }

    #[test]
    fn from_config_reads_preset_colors() {
        let mut cfg = ciri_config::theme::ThemeConfig {
            preset: "dracula".into(),
            ..Default::default()
        };
        cfg.resolve_preset();
        let theme = ResolvedTheme::from_config(&cfg);
        // Dracula ui_background "#282A36" → sRGB-normalised dark
        // colour. Summing channels stays well under 1.0 for any dark
        // theme, which is enough to assert we actually loaded the
        // preset rather than hitting the magenta parse fallback.
        let lum = theme.surface[0] + theme.surface[1] + theme.surface[2];
        assert!(
            lum < 1.0,
            "surface should be dark in dracula: {:?}",
            theme.surface
        );
    }
}
