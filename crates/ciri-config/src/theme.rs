use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
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
}

impl Default for ThemeConfig {
    fn default() -> Self {
        ThemeConfig {
            foreground: "#E6E6E6".to_string(),
            background: "#1E1E2E".to_string(),
            black: "#000000".to_string(),
            red: "#CC0000".to_string(),
            green: "#00CC00".to_string(),
            yellow: "#CCCC00".to_string(),
            blue: "#4D4DE6".to_string(),
            magenta: "#CC00CC".to_string(),
            cyan: "#00CCCC".to_string(),
            white: "#BFBFBF".to_string(),
            bright_black: "#808080".to_string(),
            bright_red: "#FF0000".to_string(),
            bright_green: "#00FF00".to_string(),
            bright_yellow: "#FFFF00".to_string(),
            bright_blue: "#6666FF".to_string(),
            bright_magenta: "#FF00FF".to_string(),
            bright_cyan: "#00FFFF".to_string(),
            bright_white: "#FFFFFF".to_string(),
        }
    }
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
}
