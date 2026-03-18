use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::keys::KeybindConfig;
use crate::theme::ThemeConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
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
            size: 14.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceConfig {
    pub padding: f32,
    pub column_gap: f32,
    pub border_width: f32,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        AppearanceConfig {
            padding: 4.0,
            column_gap: 8.0,
            border_width: 2.0,
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
    /// Enable cursor blinking.
    pub cursor_blink: bool,
    /// Cursor blink interval in milliseconds.
    pub cursor_blink_interval_ms: u64,
    /// Shell program to spawn. If empty, uses platform default (cmd.exe on Windows, $SHELL on Unix).
    pub shell: String,
    /// Maximum scrollback lines per pane. 0 = no scrollback.
    pub scrollback_lines: usize,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        TerminalConfig {
            default_cols: 80,
            default_rows: 24,
            cursor_color: "#E6E6E6".to_string(),
            cursor_opacity: 0.7,
            cursor_blink: true,
            cursor_blink_interval_ms: 500,
            shell: String::new(),
            scrollback_lines: 10000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StatusBarConfig {
    /// Extra height added to cell height for status bar.
    pub height_padding: f32,
    /// Text baseline factor (0.0-1.0) relative to cell height.
    pub text_baseline: f32,
    /// Leader indicator line height.
    pub leader_indicator_height: f32,
}

impl Default for StatusBarConfig {
    fn default() -> Self {
        StatusBarConfig {
            height_padding: 4.0,
            text_baseline: 0.8,
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
        let mut config = if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            toml::from_str(&content)?
        } else {
            CiriConfig::default()
        };
        config.theme.resolve_preset();
        Ok(config)
    }
}

pub fn config_path() -> PathBuf {
    #[cfg(unix)]
    {
        if let Ok(config_home) = std::env::var("XDG_CONFIG_HOME")
            && !config_home.is_empty() {
                return PathBuf::from(config_home).join("ciri").join("config.toml");
            }
        // $HOME/.config is the XDG default when XDG_CONFIG_HOME is unset
        let home = std::env::var("HOME").expect("neither XDG_CONFIG_HOME nor HOME is set");
        PathBuf::from(home).join(".config").join("ciri").join("config.toml")
    }
    #[cfg(windows)]
    {
        let appdata = std::env::var("APPDATA")
            .unwrap_or_else(|_| r"C:\Users\Default\AppData\Roaming".to_string());
        PathBuf::from(appdata).join("ciri").join("config.toml")
    }
}
