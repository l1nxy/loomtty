use serde::{Deserialize, Serialize};

/// Theme configuration with preset support.
///
/// Usage in config.toml:
/// ```toml
/// [theme]
/// preset = "one_dark"        # use a built-in preset
/// # Any field below overrides the preset:
/// # background = "#000000"
/// ```
///
/// Available presets: "one_dark", "one_half_dark", "catppuccin_mocha", "tokyo_night", "dracula", "nord", "gruvbox_dark", "ghostty", "loom_dark"
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct ThemeConfig {
    /// Preset name. Applied first, then individual fields override.
    #[serde(default)]
    pub preset: String,
    // Terminal colors
    pub foreground: ThemeValue,
    pub background: ThemeValue,
    pub black: ThemeValue,
    pub red: ThemeValue,
    pub green: ThemeValue,
    pub yellow: ThemeValue,
    pub blue: ThemeValue,
    pub magenta: ThemeValue,
    pub cyan: ThemeValue,
    pub white: ThemeValue,
    pub bright_black: ThemeValue,
    pub bright_red: ThemeValue,
    pub bright_green: ThemeValue,
    pub bright_yellow: ThemeValue,
    pub bright_blue: ThemeValue,
    pub bright_magenta: ThemeValue,
    pub bright_cyan: ThemeValue,
    pub bright_white: ThemeValue,
    // UI colors (overview, status bar, borders, etc.)
    /// Background color shown behind panes and in the overview backdrop.
    pub overview_background: ThemeValue,
    /// Status bar background.
    pub statusbar_background: ThemeValue,
    /// Active border color.
    pub border_active: ThemeValue,
    /// Inactive border color.
    pub border_inactive: ThemeValue,
    /// Accent color (leader indicator, active mode text).
    pub accent: ThemeValue,
    /// Status bar dim text color.
    pub statusbar_dim: ThemeValue,
    /// Broadcast mode indicator color.
    pub mode_broadcast: ThemeValue,

    // ── Chrome (UI) palette ────────────────────────────────────────────
    //
    // These describe the colours of floating chrome surfaces (palette,
    // context menu, paste / info dialogs, top-bar) and the chrome status
    // hues. They are intentionally separated from the terminal palette
    // (`background`/`foreground`/ANSI) so chrome can have its own visual
    // identity that does not change when the user swaps terminal themes.
    //
    // Every field falls back to a preset-independent loom default
    // (resolved in the `ui_*_color()` methods below) when left empty —
    // see the inline doc on each resolver for the exact hex value and
    // its design rationale. Status hues (error / warning / success /
    // info) joined the preset-independent set in stage 0.4 so Banner
    // and connection_status read the same across themes.
    /// Background colour for floating chrome surfaces (palette, dialogs,
    /// context menu, info box).
    pub ui_surface: ThemeValue,
    /// Default text colour painted on chrome surfaces.
    pub ui_on_surface: ThemeValue,
    /// Muted text colour for chrome (descriptions, mode labels in their
    /// resting state, dim hints). Preset-independent so chrome typography
    /// stays consistent across terminal themes; the preset's
    /// `statusbar_dim` keeps driving status-bar-internal text only.
    pub ui_on_surface_muted: ThemeValue,
    /// Chrome panel edge colour (palette / dialog / context-menu outer
    /// border). Neutral by design — terminal-pane focus indication still
    /// rides on `border_active` (preset-driven), but chrome panels stop
    /// inheriting the per-theme accent on their own border.
    pub ui_border: ThemeValue,
    /// Hover background for chrome interactive rows (palette row,
    /// context-menu item). Neutral on purpose — the per-theme accent is
    /// reserved for selection / explicit "you picked this" cues, while
    /// hover stays a subtle brightness lift over the panel body.
    pub ui_element_hover: ThemeValue,
    /// Active / pressed-state background for chrome interactive rows
    /// and tabs. Slightly stronger lift than `ui_element_hover` so the
    /// press read clearly without falling back onto accent-tint (which
    /// shifted hue per preset before stage 0.4).
    pub ui_element_active: ThemeValue,
    /// Chrome status colour for errors (e.g. failed-connection banner).
    pub ui_error: ThemeValue,
    /// Chrome status colour for warnings.
    pub ui_warning: ThemeValue,
    /// Chrome status colour for success indicators.
    pub ui_success: ThemeValue,
    /// Chrome status colour for informational accents.
    pub ui_info: ThemeValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(from = "String")]
pub struct ThemeValue {
    value: String,
    #[serde(skip)]
    explicitly_set: bool,
}

impl From<String> for ThemeValue {
    fn from(value: String) -> Self {
        Self {
            value,
            explicitly_set: true,
        }
    }
}

impl ThemeValue {
    fn apply_fallback(&mut self, fallback: ThemeValue) {
        if !self.explicitly_set {
            *self = fallback;
        }
    }
}

impl AsRef<str> for ThemeValue {
    fn as_ref(&self) -> &str {
        &self.value
    }
}

impl PartialEq<&str> for ThemeValue {
    fn eq(&self, other: &&str) -> bool {
        self.value == *other
    }
}

impl std::ops::Deref for ThemeValue {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl ThemeConfig {
    pub fn parse_color(hex: &str) -> [f32; 4] {
        let hex = hex.trim_start_matches('#');
        // Guard on ASCII hex digits before byte-slicing: theme colors are
        // unvalidated user TOML, and a 6-byte-but-not-6-char string (e.g.
        // "aé234") would otherwise panic slicing through a char boundary.
        if hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            let r = u8::from_str_radix(&hex[0..2], 16);
            let g = u8::from_str_radix(&hex[2..4], 16);
            let b = u8::from_str_radix(&hex[4..6], 16);
            match (r, g, b) {
                (Ok(r), Ok(g), Ok(b)) => {
                    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
                }
                _ => {
                    log::warn!("invalid hex digits in color #{hex}, falling back to light gray");
                    [0.9, 0.9, 0.9, 1.0]
                }
            }
        } else {
            log::warn!("invalid hex color: {:?}, falling back to light gray", hex);
            [0.9, 0.9, 0.9, 1.0]
        }
    }

    pub fn srgb_to_linear(color: [f32; 4]) -> [f32; 4] {
        fn convert(channel: f32) -> f32 {
            if channel <= 0.04045 {
                channel / 12.92
            } else {
                ((channel + 0.055) / 1.055).powf(2.4)
            }
        }

        [
            convert(color[0]),
            convert(color[1]),
            convert(color[2]),
            color[3],
        ]
    }

    pub fn parse_color_linear(hex: &str) -> [f32; 4] {
        Self::srgb_to_linear(Self::parse_color(hex))
    }

    pub fn linear_to_srgb(color: [f32; 4]) -> [f32; 4] {
        fn convert(channel: f32) -> f32 {
            if channel <= 0.0031308 {
                channel * 12.92
            } else {
                1.055 * channel.powf(1.0 / 2.4) - 0.055
            }
        }

        [
            convert(color[0]),
            convert(color[1]),
            convert(color[2]),
            color[3],
        ]
    }

    pub fn premultiply(color: [f32; 4]) -> [f32; 4] {
        [
            color[0] * color[3],
            color[1] * color[3],
            color[2] * color[3],
            color[3],
        ]
    }

    /// Resolve preset: fills empty fields from the preset, preserving user overrides.
    /// Fields default to "" (empty), so any non-empty value was explicitly set by the user.
    pub fn resolve_preset(&mut self) {
        let base = self.preset_theme();
        self.apply_missing_fields(base);
    }

    /// Names of the built-in presets, in the order surfaced by the
    /// settings panel theme dropdown. Single source of truth — keep in
    /// sync with `preset_theme`'s match arms (the
    /// `all_builtin_presets_fill_every_color_field` test exercises
    /// every name returned here, so divergence is caught at test time).
    pub fn preset_names() -> &'static [&'static str] {
        &[
            "loom_dark",
            "one_dark",
            "one_half_dark",
            "catppuccin_mocha",
            "tokyo_night",
            "dracula",
            "nord",
            "gruvbox_dark",
            "ghostty",
        ]
    }

    fn preset_theme(&self) -> ThemeConfig {
        let toml_str = match self.preset.as_str() {
            "catppuccin_mocha" => include_str!("../themes/catppuccin_mocha.toml"),
            "tokyo_night" => include_str!("../themes/tokyo_night.toml"),
            "dracula" => include_str!("../themes/dracula.toml"),
            "nord" => include_str!("../themes/nord.toml"),
            "gruvbox_dark" => include_str!("../themes/gruvbox_dark.toml"),
            "" | "loom_dark" => include_str!("../themes/loom_dark.toml"),
            "ghostty" => include_str!("../themes/ghostty.toml"),
            "one_dark" => include_str!("../themes/one_dark.toml"),
            "one_half_dark" => include_str!("../themes/one_half_dark.toml"),
            _ => {
                log::warn!("unknown theme preset '{}', using loom_dark", self.preset);
                include_str!("../themes/loom_dark.toml")
            }
        };
        toml::from_str(toml_str).expect("built-in theme TOML is invalid")
    }

    fn apply_missing_fields(&mut self, base: ThemeConfig) {
        apply_if_missing(&mut self.foreground, base.foreground);
        apply_if_missing(&mut self.background, base.background);
        apply_if_missing(&mut self.black, base.black);
        apply_if_missing(&mut self.red, base.red);
        apply_if_missing(&mut self.green, base.green);
        apply_if_missing(&mut self.yellow, base.yellow);
        apply_if_missing(&mut self.blue, base.blue);
        apply_if_missing(&mut self.magenta, base.magenta);
        apply_if_missing(&mut self.cyan, base.cyan);
        apply_if_missing(&mut self.white, base.white);
        apply_if_missing(&mut self.bright_black, base.bright_black);
        apply_if_missing(&mut self.bright_red, base.bright_red);
        apply_if_missing(&mut self.bright_green, base.bright_green);
        apply_if_missing(&mut self.bright_yellow, base.bright_yellow);
        apply_if_missing(&mut self.bright_blue, base.bright_blue);
        apply_if_missing(&mut self.bright_magenta, base.bright_magenta);
        apply_if_missing(&mut self.bright_cyan, base.bright_cyan);
        apply_if_missing(&mut self.bright_white, base.bright_white);
        apply_if_missing(&mut self.overview_background, base.overview_background);
        apply_if_missing(&mut self.statusbar_background, base.statusbar_background);
        apply_if_missing(&mut self.border_active, base.border_active);
        apply_if_missing(&mut self.border_inactive, base.border_inactive);
        apply_if_missing(&mut self.accent, base.accent);
        apply_if_missing(&mut self.statusbar_dim, base.statusbar_dim);
        apply_if_missing(&mut self.mode_broadcast, base.mode_broadcast);
        // Chrome palette — preset can supply explicit values; if both
        // user and preset leave them empty, the resolver methods below
        // apply loom's built-in warm-neutral defaults (status hues fall
        // back to the preset's ANSI red/yellow/green/blue so semantic
        // colours stay tunable per terminal theme).
        apply_if_missing(&mut self.ui_surface, base.ui_surface);
        apply_if_missing(&mut self.ui_on_surface, base.ui_on_surface);
        apply_if_missing(&mut self.ui_on_surface_muted, base.ui_on_surface_muted);
        apply_if_missing(&mut self.ui_border, base.ui_border);
        apply_if_missing(&mut self.ui_element_hover, base.ui_element_hover);
        apply_if_missing(&mut self.ui_element_active, base.ui_element_active);
        apply_if_missing(&mut self.ui_error, base.ui_error);
        apply_if_missing(&mut self.ui_warning, base.ui_warning);
        apply_if_missing(&mut self.ui_success, base.ui_success);
        apply_if_missing(&mut self.ui_info, base.ui_info);
    }

    // ── Chrome palette fallback chain ─────────────────────────────────
    //
    // Every `ui_*` resolver falls back to a preset-independent loom
    // default when the user / preset hasn't supplied an explicit value.
    // Chrome (including its status hues) gets its own visual identity
    // that doesn't ride along when users swap terminal themes — even
    // error / warning / success / info pick from a warm-friendly
    // loom-tuned palette so the Banner / connection_status banner read
    // the same way across `dracula` / `gruvbox` / `tokyo_night` / etc.

    fn ui_color_or_default(primary: &ThemeValue, default_hex: &str) -> [f32; 4] {
        if primary.as_ref().is_empty() {
            Self::parse_color(default_hex)
        } else {
            Self::parse_color(primary.as_ref())
        }
    }

    /// Chrome surface colour (floating panel / dialog / popup background).
    /// Default `#1A1816` — warm near-black with a yellow tint, picked to
    /// sit clearly above terminal cell backgrounds without competing
    /// with them. Decoupled from terminal palette.
    pub fn ui_surface_color(&self) -> [f32; 4] {
        Self::ui_color_or_default(&self.ui_surface, "#1A1816")
    }
    /// Default chrome text colour. `#E2DCD6` — warm near-white. Pairs
    /// with `ui_surface` for ~13:1 contrast (well above WCAG AAA).
    pub fn ui_on_surface_color(&self) -> [f32; 4] {
        Self::ui_color_or_default(&self.ui_on_surface, "#E2DCD6")
    }
    /// Muted chrome text colour. `#8E8780` — warm mid-grey. Sized for
    /// description text, resting mode labels, hint rows; readable but
    /// recedes from `on_surface`.
    pub fn ui_on_surface_muted_color(&self) -> [f32; 4] {
        Self::ui_color_or_default(&self.ui_on_surface_muted, "#8E8780")
    }
    /// Chrome panel edge colour. `#2A2622` — warm hairline. Used by
    /// floating panels (palette / dialog / context menu) for their outer
    /// border. Pane-focus indication still rides on `border_active`.
    pub fn ui_border_color(&self) -> [f32; 4] {
        Self::ui_color_or_default(&self.ui_border, "#2A2622")
    }
    /// Hover background for interactive chrome rows. `#22201E` — about
    /// `+0x12` over a sunk panel body (`#100E0C`), so the hover reads as
    /// a subtle brightness lift without competing with the accent-tinted
    /// "selected" state.
    pub fn ui_element_hover_color(&self) -> [f32; 4] {
        Self::ui_color_or_default(&self.ui_element_hover, "#22201E")
    }
    /// Active / press-state background. `#2A2724` — `+0x08` over
    /// `ui_element_hover`, deeper enough to feel like a press without
    /// coming close to the accent-tinted "selected" state.
    pub fn ui_element_active_color(&self) -> [f32; 4] {
        Self::ui_color_or_default(&self.ui_element_active, "#2A2724")
    }
    /// Chrome error colour. `#E27870` — warm coral red, picked to read
    /// against `#1A1816` chrome surface without the cool "alert red"
    /// shout that VS Code / Linear go for. Preset-independent.
    pub fn ui_error_color(&self) -> [f32; 4] {
        Self::ui_color_or_default(&self.ui_error, "#E27870")
    }
    /// Chrome warning colour. `#E6B26B` — warm amber, designed to fit
    /// the warm-neutral chrome palette.
    pub fn ui_warning_color(&self) -> [f32; 4] {
        Self::ui_color_or_default(&self.ui_warning, "#E6B26B")
    }
    /// Chrome success colour. `#A0BC75` — sage green; lower saturation
    /// than typical "success green" so it doesn't clash with warm chrome.
    pub fn ui_success_color(&self) -> [f32; 4] {
        Self::ui_color_or_default(&self.ui_success, "#A0BC75")
    }
    /// Chrome info colour. `#7DAEC8` — soft sky blue. Close to
    /// `loom_dark`'s preset accent (`#7DB4CB`) but distinct on every
    /// other preset where the accent diverges.
    pub fn ui_info_color(&self) -> [f32; 4] {
        Self::ui_color_or_default(&self.ui_info, "#7DAEC8")
    }
}

fn apply_if_missing(slot: &mut ThemeValue, fallback: ThemeValue) {
    slot.apply_fallback(fallback);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_preset_keeps_overrides_and_fills_missing_values() {
        let mut theme = ThemeConfig {
            preset: "dracula".to_string(),
            accent: "#123456".to_string().into(),
            ..ThemeConfig::default()
        };

        theme.resolve_preset();

        assert_eq!(theme.accent, "#123456");
        assert_eq!(theme.background, "#282A36");
        assert_eq!(theme.border_active, "#BD93F9");
    }

    #[test]
    fn resolve_preset_unknown_name_falls_back_to_loom_dark() {
        let mut theme = ThemeConfig {
            preset: "unknown".to_string(),
            ..ThemeConfig::default()
        };

        theme.resolve_preset();

        assert_eq!(theme.background, "#1C1B1A");
        assert_eq!(theme.accent, "#7DB4CB");
    }

    #[test]
    fn parse_color_returns_light_gray_for_invalid_input() {
        assert_eq!(ThemeConfig::parse_color("oops"), [0.9, 0.9, 0.9, 1.0]);
        assert_eq!(ThemeConfig::parse_color("#GG0000"), [0.9, 0.9, 0.9, 1.0]);
    }

    #[test]
    fn parse_color_accepts_hashless_hex() {
        assert_eq!(
            ThemeConfig::parse_color("112233"),
            [17.0 / 255.0, 34.0 / 255.0, 51.0 / 255.0, 1.0]
        );
    }

    #[test]
    fn all_builtin_presets_fill_every_color_field() {
        // Source from `preset_names()` so adding a preset there is
        // automatically exercised. Catches divergence between the
        // settings-panel dropdown list and the actual loadable presets.
        for preset in ThemeConfig::preset_names() {
            let preset = *preset;
            let mut theme = ThemeConfig {
                preset: preset.to_string(),
                ..ThemeConfig::default()
            };
            theme.resolve_preset();

            // 18 terminal palette fields + 7 legacy UI fields = 25.
            // The `ui_*` chrome fields are intentionally OPTIONAL by
            // design (preset-independent defaults live in
            // `ui_*_color()`); they're exercised by the
            // `chrome_palette_is_preset_independent` test in
            // `crates/loom-ui/src/theme.rs`.
            let fields: &[(&str, &ThemeValue)] = &[
                ("foreground", &theme.foreground),
                ("background", &theme.background),
                ("black", &theme.black),
                ("red", &theme.red),
                ("green", &theme.green),
                ("yellow", &theme.yellow),
                ("blue", &theme.blue),
                ("magenta", &theme.magenta),
                ("cyan", &theme.cyan),
                ("white", &theme.white),
                ("bright_black", &theme.bright_black),
                ("bright_red", &theme.bright_red),
                ("bright_green", &theme.bright_green),
                ("bright_yellow", &theme.bright_yellow),
                ("bright_blue", &theme.bright_blue),
                ("bright_magenta", &theme.bright_magenta),
                ("bright_cyan", &theme.bright_cyan),
                ("bright_white", &theme.bright_white),
                ("overview_background", &theme.overview_background),
                ("statusbar_background", &theme.statusbar_background),
                ("border_active", &theme.border_active),
                ("border_inactive", &theme.border_inactive),
                ("accent", &theme.accent),
                ("statusbar_dim", &theme.statusbar_dim),
                ("mode_broadcast", &theme.mode_broadcast),
            ];
            for (name, value) in fields {
                assert!(
                    !value.is_empty(),
                    "preset {preset}: {name} is empty after resolve"
                );
                // Also verify each value is a parseable hex color
                let color = ThemeConfig::parse_color(value);
                assert_ne!(
                    color,
                    [0.9, 0.9, 0.9, 1.0],
                    "preset {preset}: {name} = {:?} failed to parse as hex",
                    value.as_ref()
                );
            }
        }
    }

    #[test]
    fn srgb_linear_round_trip() {
        let original = [0.5, 0.2, 0.8, 1.0];
        let linear = ThemeConfig::srgb_to_linear(original);
        let back = ThemeConfig::linear_to_srgb(linear);
        for i in 0..3 {
            assert!(
                (original[i] - back[i]).abs() < 1e-5,
                "channel {i}: {:.6} != {:.6}",
                original[i],
                back[i]
            );
        }
    }

    #[test]
    fn srgb_to_linear_known_values() {
        // Pure black stays black
        let black = ThemeConfig::srgb_to_linear([0.0, 0.0, 0.0, 1.0]);
        assert_eq!(black, [0.0, 0.0, 0.0, 1.0]);

        // Pure white stays white
        let white = ThemeConfig::srgb_to_linear([1.0, 1.0, 1.0, 1.0]);
        for ch in &white[..3] {
            assert!((*ch - 1.0).abs() < 1e-5);
        }

        // Mid-gray: sRGB 0.5 → linear ~0.214
        let mid = ThemeConfig::srgb_to_linear([0.5, 0.5, 0.5, 1.0]);
        assert!((mid[0] - 0.214).abs() < 0.01);
    }

    #[test]
    fn premultiply_halves_rgb_at_half_alpha() {
        let result = ThemeConfig::premultiply([0.8, 0.6, 0.4, 0.5]);
        assert!((result[0] - 0.4).abs() < 1e-6);
        assert!((result[1] - 0.3).abs() < 1e-6);
        assert!((result[2] - 0.2).abs() < 1e-6);
        assert!((result[3] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn premultiply_identity_at_full_alpha() {
        let color = [0.8, 0.6, 0.4, 1.0];
        let result = ThemeConfig::premultiply(color);
        assert_eq!(result, color);
    }

    #[test]
    fn parse_color_pure_black_and_white() {
        assert_eq!(ThemeConfig::parse_color("#000000"), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(ThemeConfig::parse_color("#FFFFFF"), [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(ThemeConfig::parse_color("#ffffff"), [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn parse_color_rejects_short_and_long_hex() {
        // 3-digit shorthand not supported
        assert_eq!(ThemeConfig::parse_color("#FFF"), [0.9, 0.9, 0.9, 1.0]);
        // 8-digit (with alpha) not supported
        assert_eq!(ThemeConfig::parse_color("#FF000080"), [0.9, 0.9, 0.9, 1.0]);
    }

    #[test]
    fn theme_value_explicit_override_survives_fallback() {
        let mut tv: ThemeValue = "#FF0000".to_string().into();
        assert!(tv.explicitly_set);
        tv.apply_fallback("#0000FF".to_string().into());
        assert_eq!(tv, "#FF0000"); // override preserved

        let mut tv = ThemeValue::default();
        assert!(!tv.explicitly_set);
        tv.apply_fallback("#0000FF".to_string().into());
        assert_eq!(tv, "#0000FF"); // fallback applied
    }

    #[test]
    fn resolve_preset_empty_string_uses_loom_dark() {
        let mut theme = ThemeConfig {
            preset: "".to_string(),
            ..ThemeConfig::default()
        };
        theme.resolve_preset();
        // Should match loom_dark preset
        assert_eq!(theme.background, "#1C1B1A");
    }
}
