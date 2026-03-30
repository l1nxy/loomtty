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
/// Available presets: "one_dark", "catppuccin_mocha", "tokyo_night", "dracula", "nord", "gruvbox_dark", "ghostty", "ciri_dark"
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct ThemeConfig {
    /// Preset name. Applied first, then individual fields override.
    #[serde(default)]
    pub preset: String,
    // Terminal colors
    pub foreground: String,
    pub background: String,
    pub black: String,
    pub red: String,
    pub green: String,
    pub yellow: String,
    pub blue: String,
    pub magenta: String,
    pub cyan: String,
    pub white: String,
    pub bright_black: String,
    pub bright_red: String,
    pub bright_green: String,
    pub bright_yellow: String,
    pub bright_blue: String,
    pub bright_magenta: String,
    pub bright_cyan: String,
    pub bright_white: String,
    // UI colors (overview, status bar, borders, etc.)
    /// Window clear / normal background color.
    pub ui_background: String,
    /// Overview mode background color (lighter to distinguish from pane content).
    pub overview_background: String,
    /// Status bar background.
    pub statusbar_background: String,
    /// Active border color.
    pub border_active: String,
    /// Inactive border color.
    pub border_inactive: String,
    /// Accent color (leader indicator, active mode text).
    pub accent: String,
    /// Status bar dim text color.
    pub statusbar_dim: String,
    /// Broadcast mode indicator color.
    pub mode_broadcast: String,
}

impl ThemeConfig {
    pub fn parse_color(hex: &str) -> [f32; 4] {
        let hex = hex.trim_start_matches('#');
        if hex.len() == 6 {
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

    fn preset_theme(&self) -> ThemeConfig {
        let toml_str = match self.preset.as_str() {
            "catppuccin_mocha" => include_str!("../themes/catppuccin_mocha.toml"),
            "tokyo_night" => include_str!("../themes/tokyo_night.toml"),
            "dracula" => include_str!("../themes/dracula.toml"),
            "nord" => include_str!("../themes/nord.toml"),
            "gruvbox_dark" => include_str!("../themes/gruvbox_dark.toml"),
            "" | "ciri_dark" => include_str!("../themes/ciri_dark.toml"),
            "ghostty" => include_str!("../themes/ghostty.toml"),
            "one_dark" => include_str!("../themes/one_dark.toml"),
            _ => {
                log::warn!("unknown theme preset '{}', using ciri_dark", self.preset);
                include_str!("../themes/ciri_dark.toml")
            }
        };
        toml::from_str(toml_str).expect("built-in theme TOML is invalid")
    }

    fn apply_missing_fields(&mut self, base: ThemeConfig) {
        apply_if_empty(&mut self.foreground, base.foreground);
        apply_if_empty(&mut self.background, base.background);
        apply_if_empty(&mut self.black, base.black);
        apply_if_empty(&mut self.red, base.red);
        apply_if_empty(&mut self.green, base.green);
        apply_if_empty(&mut self.yellow, base.yellow);
        apply_if_empty(&mut self.blue, base.blue);
        apply_if_empty(&mut self.magenta, base.magenta);
        apply_if_empty(&mut self.cyan, base.cyan);
        apply_if_empty(&mut self.white, base.white);
        apply_if_empty(&mut self.bright_black, base.bright_black);
        apply_if_empty(&mut self.bright_red, base.bright_red);
        apply_if_empty(&mut self.bright_green, base.bright_green);
        apply_if_empty(&mut self.bright_yellow, base.bright_yellow);
        apply_if_empty(&mut self.bright_blue, base.bright_blue);
        apply_if_empty(&mut self.bright_magenta, base.bright_magenta);
        apply_if_empty(&mut self.bright_cyan, base.bright_cyan);
        apply_if_empty(&mut self.bright_white, base.bright_white);
        apply_if_empty(&mut self.ui_background, base.ui_background);
        apply_if_empty(&mut self.overview_background, base.overview_background);
        apply_if_empty(&mut self.statusbar_background, base.statusbar_background);
        apply_if_empty(&mut self.border_active, base.border_active);
        apply_if_empty(&mut self.border_inactive, base.border_inactive);
        apply_if_empty(&mut self.accent, base.accent);
        apply_if_empty(&mut self.statusbar_dim, base.statusbar_dim);
        apply_if_empty(&mut self.mode_broadcast, base.mode_broadcast);
    }
}

fn apply_if_empty(slot: &mut String, fallback: String) {
    if slot.is_empty() {
        *slot = fallback;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_preset_keeps_overrides_and_fills_missing_values() {
        let mut theme = ThemeConfig {
            preset: "dracula".to_string(),
            accent: "#123456".to_string(),
            ..ThemeConfig::default()
        };

        theme.resolve_preset();

        assert_eq!(theme.accent, "#123456");
        assert_eq!(theme.background, "#282A36");
        assert_eq!(theme.border_active, "#BD93F9");
    }

    #[test]
    fn resolve_preset_unknown_name_falls_back_to_ciri_dark() {
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
}
