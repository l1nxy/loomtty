use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::keys::KeybindConfig;
use crate::theme::ThemeConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CiriConfig {
    pub font: FontConfig,
    pub appearance: AppearanceConfig,
    pub animation: AnimationConfig,
    pub keys: KeybindConfig,
    pub theme: ThemeConfig,
    pub window: WindowConfig,
    pub terminal: TerminalConfig,
    pub statusbar: StatusBarConfig,
    pub input: InputConfig,
    pub render: RenderConfig,
}

impl Default for CiriConfig {
    fn default() -> Self {
        CiriConfig {
            font: FontConfig::default(),
            appearance: AppearanceConfig::default(),
            animation: AnimationConfig::default(),
            keys: KeybindConfig::default(),
            theme: ThemeConfig::default(),
            window: WindowConfig::default(),
            terminal: TerminalConfig::default(),
            statusbar: StatusBarConfig::default(),
            input: InputConfig::default(),
            render: RenderConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FontConfig {
    pub family: String,
    pub size: f32,
}

impl Default for FontConfig {
    fn default() -> Self {
        FontConfig {
            family: "monospace".to_string(),
            size: 16.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceConfig {
    pub padding: f32,
    pub column_gap: f32,
    pub border_width: f32,
    pub active_border_color: String,
    pub inactive_border_color: String,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        AppearanceConfig {
            padding: 4.0,
            column_gap: 8.0,
            border_width: 2.0,
            active_border_color: "#88C0D0".to_string(),
            inactive_border_color: "#4C566A".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnimationConfig {
    /// Animation speed (omega). Higher = faster. 8-15 recommended.
    pub speed: f64,
    pub enabled: bool,
    /// Spring rest threshold. Lower = more precise settling.
    pub epsilon: f64,
    /// Overview zoom fit factor (0.0-1.0). 0.9 = 90% of viewport.
    pub overview_zoom_fit: f32,
    /// Zoom threshold below which overview tiles are rendered.
    pub zoom_threshold: f32,
}

impl Default for AnimationConfig {
    fn default() -> Self {
        AnimationConfig {
            speed: 12.0,
            enabled: true,
            epsilon: 0.1,
            overview_zoom_fit: 0.9,
            zoom_threshold: 0.99,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowConfig {
    pub width: f64,
    pub height: f64,
    pub title: String,
}

impl Default for WindowConfig {
    fn default() -> Self {
        WindowConfig {
            width: 1024.0,
            height: 768.0,
            title: "ciri".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalConfig {
    pub default_cols: u16,
    pub default_rows: u16,
    /// Cursor color as hex string.
    pub cursor_color: String,
    /// Cursor opacity (0.0-1.0).
    pub cursor_opacity: f32,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        TerminalConfig {
            default_cols: 80,
            default_rows: 24,
            cursor_color: "#E6E6E6".to_string(),
            cursor_opacity: 0.7,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StatusBarConfig {
    /// Extra height added to cell height for status bar.
    pub height_padding: f32,
    /// Status bar background color.
    pub background_color: String,
    /// Text baseline factor (0.0-1.0) relative to cell height.
    pub text_baseline: f32,
    /// Text color when leader mode is active.
    pub leader_text_color: String,
    /// Normal text color.
    pub text_color: String,
    /// Active mode text color (overview, leader).
    pub active_mode_color: String,
    /// Inactive/dimmed text color.
    pub inactive_text_color: String,
    /// Leader indicator line color.
    pub leader_indicator_color: String,
    /// Leader indicator line height.
    pub leader_indicator_height: f32,
}

impl Default for StatusBarConfig {
    fn default() -> Self {
        StatusBarConfig {
            height_padding: 4.0,
            background_color: "#1F1F26".to_string(),
            text_baseline: 0.8,
            leader_text_color: "#4CE64C".to_string(),
            text_color: "#B3B3B3".to_string(),
            active_mode_color: "#4CE64C".to_string(),
            inactive_text_color: "#808080".to_string(),
            leader_indicator_color: "#4CE64C".to_string(),
            leader_indicator_height: 2.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InputConfig {
    /// Leader mode timeout in milliseconds.
    pub leader_timeout_ms: u64,
    /// Double-tap window in milliseconds for SendLeaderKey.
    pub double_tap_window_ms: u64,
    /// Scroll multiplier for line-based scroll delta.
    pub scroll_multiplier: f64,
}

impl Default for InputConfig {
    fn default() -> Self {
        InputConfig {
            leader_timeout_ms: 1000,
            double_tap_window_ms: 300,
            scroll_multiplier: 50.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RenderConfig {
    /// Frame interval in milliseconds (16 = ~60fps).
    pub frame_interval_ms: u64,
    /// Background clear color.
    pub clear_color: String,
    /// Glyph atlas texture size (width and height).
    pub atlas_size: u32,
    /// Maximum glyph instances per frame.
    pub max_glyph_instances: usize,
    /// Maximum rectangles per frame.
    pub max_rectangles: usize,
    /// Desired maximum frame latency.
    pub frame_latency: u32,
}

impl Default for RenderConfig {
    fn default() -> Self {
        RenderConfig {
            frame_interval_ms: 16,
            clear_color: "#010114".to_string(),
            atlas_size: 2048,
            max_glyph_instances: 32768,
            max_rectangles: 8192,
            frame_latency: 2,
        }
    }
}

impl CiriConfig {
    pub fn load() -> Result<Self> {
        let path = config_path();
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            let config: CiriConfig = toml::from_str(&content)?;
            Ok(config)
        } else {
            Ok(CiriConfig::default())
        }
    }
}

pub fn config_path() -> PathBuf {
    let config_home = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{home}/.config")
    });
    PathBuf::from(config_home).join("ciri").join("config.toml")
}
