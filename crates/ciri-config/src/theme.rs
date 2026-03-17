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
/// Available presets: "one_dark", "catppuccin_mocha", "tokyo_night", "dracula", "nord", "gruvbox_dark"
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
    /// Window clear / overview background color.
    pub ui_background: String,
    /// Status bar background.
    pub statusbar_background: String,
    /// Active border color.
    pub border_active: String,
    /// Inactive border color.
    pub border_inactive: String,
    /// Accent color (leader indicator, active mode text).
    pub accent: String,
}


impl ThemeConfig {
    pub fn parse_color(hex: &str) -> [f32; 4] {
        let hex = hex.trim_start_matches('#');
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
            let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
            let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
            [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
        } else {
            [0.9, 0.9, 0.9, 1.0]
        }
    }

    /// Resolve preset: fills empty fields from the preset, preserving user overrides.
    /// Fields default to "" (empty), so any non-empty value was explicitly set by the user.
    pub fn resolve_preset(&mut self) {
        let base = match self.preset.as_str() {
            "catppuccin_mocha" => catppuccin_mocha(),
            "tokyo_night" => tokyo_night(),
            "dracula" => dracula(),
            "nord" => nord(),
            "gruvbox_dark" => gruvbox_dark(),
            "" | "one_dark" => one_dark(),
            _ => {
                log::warn!("unknown theme preset '{}', using one_dark", self.preset);
                one_dark()
            }
        };
        // Fill empty fields from preset; user-set fields are preserved.
        macro_rules! fill {
            ($($field:ident),*) => {
                $(if self.$field.is_empty() { self.$field = base.$field; })*
            };
        }
        fill!(
            foreground, background,
            black, red, green, yellow, blue, magenta, cyan, white,
            bright_black, bright_red, bright_green, bright_yellow,
            bright_blue, bright_magenta, bright_cyan, bright_white,
            ui_background, statusbar_background, border_active, border_inactive, accent
        );
    }
}

fn one_dark() -> ThemeConfig {
    ThemeConfig {
        preset: "one_dark".to_string(),
        foreground: "#ABB2BF".to_string(),
        background: "#282C34".to_string(),
        black: "#282C34".to_string(),
        red: "#E06C75".to_string(),
        green: "#98C379".to_string(),
        yellow: "#E5C07B".to_string(),
        blue: "#61AFEF".to_string(),
        magenta: "#C678DD".to_string(),
        cyan: "#56B6C2".to_string(),
        white: "#ABB2BF".to_string(),
        bright_black: "#5C6370".to_string(),
        bright_red: "#E06C75".to_string(),
        bright_green: "#98C379".to_string(),
        bright_yellow: "#E5C07B".to_string(),
        bright_blue: "#61AFEF".to_string(),
        bright_magenta: "#C678DD".to_string(),
        bright_cyan: "#56B6C2".to_string(),
        bright_white: "#FFFFFF".to_string(),
        ui_background: "#282C34".to_string(),
        statusbar_background: "#21252B".to_string(),
        border_active: "#528BFF".to_string(),
        border_inactive: "#3E4452".to_string(),
        accent: "#98C379".to_string(),
    }
}

fn catppuccin_mocha() -> ThemeConfig {
    ThemeConfig {
        preset: "catppuccin_mocha".to_string(),
        foreground: "#CDD6F4".to_string(),
        background: "#1E1E2E".to_string(),
        black: "#45475A".to_string(),
        red: "#F38BA8".to_string(),
        green: "#A6E3A1".to_string(),
        yellow: "#F9E2AF".to_string(),
        blue: "#89B4FA".to_string(),
        magenta: "#F5C2E7".to_string(),
        cyan: "#94E2D5".to_string(),
        white: "#BAC2DE".to_string(),
        bright_black: "#585B70".to_string(),
        bright_red: "#F38BA8".to_string(),
        bright_green: "#A6E3A1".to_string(),
        bright_yellow: "#F9E2AF".to_string(),
        bright_blue: "#89B4FA".to_string(),
        bright_magenta: "#F5C2E7".to_string(),
        bright_cyan: "#94E2D5".to_string(),
        bright_white: "#A6ADC8".to_string(),
        ui_background: "#1E1E2E".to_string(),
        statusbar_background: "#181825".to_string(),
        border_active: "#89B4FA".to_string(),
        border_inactive: "#313244".to_string(),
        accent: "#A6E3A1".to_string(),
    }
}

fn tokyo_night() -> ThemeConfig {
    ThemeConfig {
        preset: "tokyo_night".to_string(),
        foreground: "#C0CAF5".to_string(),
        background: "#1A1B26".to_string(),
        black: "#15161E".to_string(),
        red: "#F7768E".to_string(),
        green: "#9ECE6A".to_string(),
        yellow: "#E0AF68".to_string(),
        blue: "#7AA2F7".to_string(),
        magenta: "#BB9AF7".to_string(),
        cyan: "#7DCFFF".to_string(),
        white: "#A9B1D6".to_string(),
        bright_black: "#414868".to_string(),
        bright_red: "#F7768E".to_string(),
        bright_green: "#9ECE6A".to_string(),
        bright_yellow: "#E0AF68".to_string(),
        bright_blue: "#7AA2F7".to_string(),
        bright_magenta: "#BB9AF7".to_string(),
        bright_cyan: "#7DCFFF".to_string(),
        bright_white: "#C0CAF5".to_string(),
        ui_background: "#1A1B26".to_string(),
        statusbar_background: "#16161E".to_string(),
        border_active: "#7AA2F7".to_string(),
        border_inactive: "#292E42".to_string(),
        accent: "#9ECE6A".to_string(),
    }
}

fn dracula() -> ThemeConfig {
    ThemeConfig {
        preset: "dracula".to_string(),
        foreground: "#F8F8F2".to_string(),
        background: "#282A36".to_string(),
        black: "#21222C".to_string(),
        red: "#FF5555".to_string(),
        green: "#50FA7B".to_string(),
        yellow: "#F1FA8C".to_string(),
        blue: "#BD93F9".to_string(),
        magenta: "#FF79C6".to_string(),
        cyan: "#8BE9FD".to_string(),
        white: "#F8F8F2".to_string(),
        bright_black: "#6272A4".to_string(),
        bright_red: "#FF6E6E".to_string(),
        bright_green: "#69FF94".to_string(),
        bright_yellow: "#FFFFA5".to_string(),
        bright_blue: "#D6ACFF".to_string(),
        bright_magenta: "#FF92DF".to_string(),
        bright_cyan: "#A4FFFF".to_string(),
        bright_white: "#FFFFFF".to_string(),
        ui_background: "#282A36".to_string(),
        statusbar_background: "#21222C".to_string(),
        border_active: "#BD93F9".to_string(),
        border_inactive: "#44475A".to_string(),
        accent: "#50FA7B".to_string(),
    }
}

fn nord() -> ThemeConfig {
    ThemeConfig {
        preset: "nord".to_string(),
        foreground: "#D8DEE9".to_string(),
        background: "#2E3440".to_string(),
        black: "#3B4252".to_string(),
        red: "#BF616A".to_string(),
        green: "#A3BE8C".to_string(),
        yellow: "#EBCB8B".to_string(),
        blue: "#81A1C1".to_string(),
        magenta: "#B48EAD".to_string(),
        cyan: "#88C0D0".to_string(),
        white: "#E5E9F0".to_string(),
        bright_black: "#4C566A".to_string(),
        bright_red: "#BF616A".to_string(),
        bright_green: "#A3BE8C".to_string(),
        bright_yellow: "#EBCB8B".to_string(),
        bright_blue: "#81A1C1".to_string(),
        bright_magenta: "#B48EAD".to_string(),
        bright_cyan: "#8FBCBB".to_string(),
        bright_white: "#ECEFF4".to_string(),
        ui_background: "#2E3440".to_string(),
        statusbar_background: "#272C36".to_string(),
        border_active: "#88C0D0".to_string(),
        border_inactive: "#4C566A".to_string(),
        accent: "#A3BE8C".to_string(),
    }
}

fn gruvbox_dark() -> ThemeConfig {
    ThemeConfig {
        preset: "gruvbox_dark".to_string(),
        foreground: "#EBDBB2".to_string(),
        background: "#282828".to_string(),
        black: "#282828".to_string(),
        red: "#CC241D".to_string(),
        green: "#98971A".to_string(),
        yellow: "#D79921".to_string(),
        blue: "#458588".to_string(),
        magenta: "#B16286".to_string(),
        cyan: "#689D6A".to_string(),
        white: "#A89984".to_string(),
        bright_black: "#928374".to_string(),
        bright_red: "#FB4934".to_string(),
        bright_green: "#B8BB26".to_string(),
        bright_yellow: "#FABD2F".to_string(),
        bright_blue: "#83A598".to_string(),
        bright_magenta: "#D3869B".to_string(),
        bright_cyan: "#8EC07C".to_string(),
        bright_white: "#EBDBB2".to_string(),
        ui_background: "#282828".to_string(),
        statusbar_background: "#1D2021".to_string(),
        border_active: "#FABD2F".to_string(),
        border_inactive: "#504945".to_string(),
        accent: "#B8BB26".to_string(),
    }
}
