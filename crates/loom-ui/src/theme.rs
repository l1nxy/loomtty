//! Pre-resolved theme tokens.
//!
//! Turns `loom_config::theme::ThemeConfig` (hex strings) into sRGB-encoded
//! RGBA ready for the GPU, computed once per theme change. sRGB — not
//! linear — to match the existing loom-gpu pipeline, which does not
//! gamma-decode its inputs before blending (see `ResolvedTheme`'s doc
//! comment for the rationale and the regression this fixes). Fixes the
//! parse-hex-per-frame smell in the current chrome path.
//!
//! The token schema is semantic (surface / on-surface / accent / …) rather
//! than terminal-centric so plugin UIs and future theme-aware widgets can
//! read one stable API.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::color::{Color, contrast_on, mul_alpha, scale_rgb};
use loom_config::theme::ThemeConfig;

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
    // Interactive states — neutral by design. Selection / active-tab
    // states continue to compose with `accent` in their consumers, so
    // the per-theme accent stays reserved for the "explicit pick" cue.
    pub element_hover: Color,
    pub element_active: Color,
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
    /// existing loom-gpu blend / shader path, which renders hex-coded
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
        // Chrome (UI) palette — `ui_*` fields with empty-fallback to
        // `overview_background` / `foreground` / ANSI hues. See
        // `ThemeConfig`'s field comments for the fallback chain. The
        // intent is for chrome panels (palette, dialogs, context menus)
        // to have their own visual identity that doesn't dissolve into
        // the terminal pane underneath them.
        let surface = cfg.ui_surface_color();
        let on_surface = cfg.ui_on_surface_color();
        let err = cfg.ui_error_color();
        let warn = cfg.ui_warning_color();
        let ok = cfg.ui_success_color();
        let info = cfg.ui_info_color();

        let muted = cfg.ui_on_surface_muted_color();
        let border_chrome = cfg.ui_border_color();
        let element_hover = cfg.ui_element_hover_color();
        let element_active = cfg.ui_element_active_color();

        // Terminal palette — kept distinct so terminal cells don't follow
        // chrome re-skinning. `term_fg` reads `cfg.foreground` directly
        // (not `on_surface`) so a user override of `ui_on_surface` does
        // not change the terminal text colour.
        let term_bg = ThemeConfig::parse_color(cfg.background.as_ref());
        let term_fg = ThemeConfig::parse_color(cfg.foreground.as_ref());

        let accent = ThemeConfig::parse_color(cfg.accent.as_ref());
        // `border_active` keeps driving terminal pane focus indication —
        // that's where preset hue belongs (theme identity on the focused
        // pane edge). Chrome panel borders use `border_chrome` instead.
        let border_a = ThemeConfig::parse_color(cfg.border_active.as_ref());
        let statusbar_bg = ThemeConfig::parse_color(cfg.statusbar_background.as_ref());
        let broadcast = ThemeConfig::parse_color(cfg.mode_broadcast.as_ref());

        Self {
            surface,
            surface_elevated: scale_rgb(surface, 1.12),
            // Additive sink (-0.04 per channel) so `surface_sunken` lands
            // on the same `#100E0C`-ish tone that floating chrome panels
            // (palette / context-menu / dialog body) compute locally via
            // `tokens::surface_sink(surface, SURFACE_SINK)`. Multiplicative
            // sink (`scale_rgb(0.88)`) shifts hue on non-neutral surfaces,
            // and the codebase trends toward the additive variant.
            surface_sunken: [
                (surface[0] - 0.04).max(0.0),
                (surface[1] - 0.04).max(0.0),
                (surface[2] - 0.04).max(0.0),
                surface[3],
            ],
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
            border: border_chrome,
            border_focus: border_a,
            element_hover,
            element_active,
            statusbar_bg,
            broadcast,
            term_fg,
            term_bg,
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
            element_hover: [0.16, 0.16, 0.18, 1.0],
            element_active: [0.20, 0.20, 0.22, 1.0],
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

/// Semantic elevation tier for chrome surfaces.
///
/// Each tier resolves to a `(bg, shadow)` pair via [`ElevationIndex::bg`]
/// and the `shadow_*()` styled helpers — consumers say "I'm a Panel" /
/// "I'm a Modal" instead of hand-picking sink amounts and shadow sizes.
///
/// Mirrors the role Zed's `ElevationIndex` plays, scaled down to loom's
/// 4 chrome layers:
///
/// - [`Background`] — pane backdrop / overview area. No chrome on top.
/// - [`Surface`] — chrome resting tier (top bar, status bar, settings
///   page background once it lands).
/// - [`Panel`] — floating chrome panels: command palette body, context
///   menu, dialog body. Recessed below `Surface`.
/// - [`Modal`] — emphasised modals stacked over panels. Same colour as
///   `Panel` for now; differentiated by a stronger shadow at consumer.
///
/// [`Background`]: ElevationIndex::Background
/// [`Surface`]: ElevationIndex::Surface
/// [`Panel`]: ElevationIndex::Panel
/// [`Modal`]: ElevationIndex::Modal
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElevationIndex {
    Background,
    Surface,
    Panel,
    Modal,
}

impl ElevationIndex {
    /// Resolve the background colour for this tier from a theme.
    ///
    /// `Background` and `Surface` return the same `theme.surface` here —
    /// `Background` callers are expected to overlay `term_bg` themselves
    /// for terminal panes, and there's no chrome `Background` paint
    /// today. Reserves the variant for future consumers (e.g. a
    /// settings-page wrapper that wants to fill a region with the
    /// pane-area backdrop).
    pub fn bg(self, theme: &ResolvedTheme) -> Color {
        match self {
            Self::Background | Self::Surface => theme.surface,
            Self::Panel | Self::Modal => theme.surface_sunken,
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
        let mut cfg = loom_config::theme::ThemeConfig::default();
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
        let mut cfg = loom_config::theme::ThemeConfig::default();
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
        let mut cfg = loom_config::theme::ThemeConfig {
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

    /// Chrome surface, text, muted text and border colours are
    /// preset-independent (loom-built-in warm-neutral defaults).
    /// Iterate every preset to catch anyone reintroducing a per-preset
    /// chrome colour. Status hues (error/warning/success/info) and
    /// `border_focus` (terminal-pane focus indicator) are intentionally
    /// excluded — those still ride along with the preset.
    #[test]
    fn chrome_palette_is_preset_independent() {
        const EXPECTED_SURFACE: &str = "#1A1816";
        const EXPECTED_ON_SURFACE: &str = "#E2DCD6";
        const EXPECTED_ON_SURFACE_MUTED: &str = "#8E8780";
        const EXPECTED_BORDER: &str = "#2A2622";
        const EXPECTED_ELEMENT_HOVER: &str = "#22201E";
        const EXPECTED_ELEMENT_ACTIVE: &str = "#2A2724";
        const EXPECTED_ERROR: &str = "#E27870";
        const EXPECTED_WARNING: &str = "#E6B26B";
        const EXPECTED_SUCCESS: &str = "#A0BC75";
        const EXPECTED_INFO: &str = "#7DAEC8";
        for preset in [
            "loom_dark",
            "one_dark",
            "catppuccin_mocha",
            "tokyo_night",
            "dracula",
            "nord",
            "gruvbox_dark",
            "ghostty",
        ] {
            let mut cfg = loom_config::theme::ThemeConfig {
                preset: preset.into(),
                ..Default::default()
            };
            cfg.resolve_preset();
            let theme = ResolvedTheme::from_config(&cfg);
            assert_eq!(
                theme.surface,
                ThemeConfig::parse_color(EXPECTED_SURFACE),
                "{preset}: chrome surface must not follow preset",
            );
            assert_eq!(
                theme.on_surface,
                ThemeConfig::parse_color(EXPECTED_ON_SURFACE),
                "{preset}: chrome on_surface must not follow preset",
            );
            assert_eq!(
                theme.on_surface_muted,
                ThemeConfig::parse_color(EXPECTED_ON_SURFACE_MUTED),
                "{preset}: chrome on_surface_muted must not follow preset",
            );
            assert_eq!(
                theme.border,
                ThemeConfig::parse_color(EXPECTED_BORDER),
                "{preset}: chrome border must not follow preset",
            );
            assert_eq!(
                theme.element_hover,
                ThemeConfig::parse_color(EXPECTED_ELEMENT_HOVER),
                "{preset}: chrome element_hover must not follow preset",
            );
            assert_eq!(
                theme.element_active,
                ThemeConfig::parse_color(EXPECTED_ELEMENT_ACTIVE),
                "{preset}: chrome element_active must not follow preset",
            );
            assert_eq!(
                theme.error,
                ThemeConfig::parse_color(EXPECTED_ERROR),
                "{preset}: chrome error must not follow preset ANSI red",
            );
            assert_eq!(
                theme.warning,
                ThemeConfig::parse_color(EXPECTED_WARNING),
                "{preset}: chrome warning must not follow preset ANSI yellow",
            );
            assert_eq!(
                theme.success,
                ThemeConfig::parse_color(EXPECTED_SUCCESS),
                "{preset}: chrome success must not follow preset ANSI green",
            );
            assert_eq!(
                theme.info,
                ThemeConfig::parse_color(EXPECTED_INFO),
                "{preset}: chrome info must not follow preset ANSI blue",
            );
        }
    }

    /// `border_focus` (terminal-pane focus colour) intentionally still
    /// follows the preset — it represents theme identity on the
    /// focused-pane edge. Catches anyone broadening the
    /// preset-independent rule onto pane focus.
    #[test]
    fn pane_focus_border_still_follows_preset() {
        let mut cfg = loom_config::theme::ThemeConfig {
            preset: "dracula".into(),
            ..Default::default()
        };
        cfg.resolve_preset();
        let theme = ResolvedTheme::from_config(&cfg);
        assert_eq!(
            theme.border_focus,
            ThemeConfig::parse_color(cfg.border_active.as_ref()),
            "pane-focus border should ride along with preset border_active",
        );
    }

    /// Stage 0.1 regression: chrome surface must not be the terminal
    /// cell background. Catches anyone re-introducing the old
    /// `ResolvedTheme::from_config` line that used `cfg.background` for
    /// `surface`. Uses `loom_dark` because its terminal bg (`#1C1B1A`)
    /// and chrome surface (`#1A1816`) are very close in luminance —
    /// asserting inequality on the array still works because the warm
    /// tint differs from the cool terminal-grey by a perceptible
    /// channel-by-channel offset.
    #[test]
    fn surface_decoupled_from_terminal_bg() {
        let mut cfg = loom_config::theme::ThemeConfig {
            preset: "loom_dark".into(),
            ..Default::default()
        };
        cfg.resolve_preset();
        let theme = ResolvedTheme::from_config(&cfg);
        assert_ne!(
            theme.surface, theme.term_bg,
            "chrome surface must not collapse onto terminal bg: surface={:?} term_bg={:?}",
            theme.surface, theme.term_bg,
        );
    }

    /// Explicit `ui_surface` overrides loom's built-in default. Pin the
    /// fallback-chain order so future refactors can't silently flip it.
    #[test]
    fn ui_surface_override_wins_over_default() {
        let mut cfg = loom_config::theme::ThemeConfig {
            preset: "loom_dark".into(),
            ui_surface: "#112233".to_string().into(),
            ..Default::default()
        };
        cfg.resolve_preset();
        let theme = ResolvedTheme::from_config(&cfg);
        assert_eq!(theme.surface, ThemeConfig::parse_color("#112233"));
    }

    /// `surface_sunken` is the `#100E0C`-ish tier that floating chrome
    /// panels (palette / context_menu / paste_dialog) paint into via
    /// `ElevationIndex::Panel`. Pin the additive `-0.04` offset so a
    /// future refactor can't silently revert to the multiplicative
    /// formula that produced a different (hue-shifted) sink colour.
    #[test]
    fn surface_sunken_is_additive_sink_below_surface() {
        let mut cfg = loom_config::theme::ThemeConfig {
            preset: "loom_dark".into(),
            ..Default::default()
        };
        cfg.resolve_preset();
        let theme = ResolvedTheme::from_config(&cfg);
        // Each channel should be exactly `surface - 0.04` (clamped at 0).
        for i in 0..3 {
            let expected = (theme.surface[i] - 0.04).max(0.0);
            assert!(
                (theme.surface_sunken[i] - expected).abs() < 1e-6,
                "channel {i}: expected {expected}, got {}",
                theme.surface_sunken[i]
            );
        }
    }

    /// `ElevationIndex::Panel` and `Modal` resolve to `surface_sunken`
    /// today so consumers can pick by semantic meaning instead of
    /// computing sink amounts locally. Pin the mapping.
    #[test]
    fn elevation_panel_and_modal_resolve_to_sunken_surface() {
        let mut cfg = loom_config::theme::ThemeConfig {
            preset: "loom_dark".into(),
            ..Default::default()
        };
        cfg.resolve_preset();
        let theme = ResolvedTheme::from_config(&cfg);
        assert_eq!(ElevationIndex::Panel.bg(&theme), theme.surface_sunken);
        assert_eq!(ElevationIndex::Modal.bg(&theme), theme.surface_sunken);
        assert_eq!(ElevationIndex::Surface.bg(&theme), theme.surface);
        assert_eq!(ElevationIndex::Background.bg(&theme), theme.surface);
    }

    /// Stage 0.4 flipped status hues from "follow preset ANSI" to
    /// "preset-independent loom default". Pin that the chrome `error`
    /// no longer collapses onto the preset's ANSI red so a future
    /// resolver tweak can't silently restore the old behaviour and
    /// re-introduce per-theme Banner colour drift.
    #[test]
    fn chrome_status_diverges_from_preset_ansi() {
        let mut cfg = loom_config::theme::ThemeConfig {
            preset: "dracula".into(),
            ..Default::default()
        };
        cfg.resolve_preset();
        let theme = ResolvedTheme::from_config(&cfg);
        assert_ne!(
            theme.error,
            ThemeConfig::parse_color(cfg.red.as_ref()),
            "chrome error should not collapse onto preset ANSI red"
        );
    }

    /// Explicit `ui_error` overrides the loom default. Pins that the
    /// fallback chain stays open for users / preset authors to tune
    /// the chrome status palette per theme if they want.
    #[test]
    fn ui_error_override_wins_over_default() {
        let mut cfg = loom_config::theme::ThemeConfig {
            preset: "loom_dark".into(),
            ui_error: "#FF0000".to_string().into(),
            ..Default::default()
        };
        cfg.resolve_preset();
        let theme = ResolvedTheme::from_config(&cfg);
        assert_eq!(theme.error, ThemeConfig::parse_color("#FF0000"));
    }
}
