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
    pub layout: LayoutConfig,
    pub gesture: GestureConfig,
    pub remote: RemoteConfig,
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

/// Focus ring style for the active pane border.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FocusRingStyle {
    Solid,
    Glow,
    Dashed,
}

impl Default for FocusRingStyle {
    fn default() -> Self {
        Self::Solid
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FocusRingConfig {
    pub style: FocusRingStyle,
    pub glow_radius: f32,
    pub glow_layers: u8,
    pub dash_length: f32,
    pub gap_length: f32,
}

impl Default for FocusRingConfig {
    fn default() -> Self {
        FocusRingConfig {
            style: FocusRingStyle::Solid,
            glow_radius: 4.0,
            glow_layers: 3,
            dash_length: 8.0,
            gap_length: 4.0,
        }
    }
}

/// Pane open/close animation style.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PaneOpenStyle {
    Fade,
    SlideUp,
    SlideDown,
    SlideLeft,
    FadeSlideUp,
}

impl Default for PaneOpenStyle {
    fn default() -> Self {
        Self::Fade
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
    pub inactive_opacity: f32,
    pub focus_ring: FocusRingConfig,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        AppearanceConfig {
            padding: 4.0,
            column_gap: 8.0,
            border_width: 2.0,
            active_border_color: String::new(),
            inactive_border_color: String::new(),
            inactive_opacity: 0.7,
            focus_ring: FocusRingConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnimationConfig {
    pub speed: f64,
    pub enabled: bool,
    pub epsilon: f64,
    pub overview_zoom_fit: f32,
    pub zoom_threshold: f32,
    pub pane_open_style: PaneOpenStyle,
    pub pane_open_duration_ms: u64,
    pub pane_close_duration_ms: u64,
    pub focus_transition_speed: f64,
}

impl Default for AnimationConfig {
    fn default() -> Self {
        AnimationConfig {
            speed: 12.0,
            enabled: true,
            epsilon: 0.1,
            overview_zoom_fit: 0.9,
            zoom_threshold: 0.99,
            pane_open_style: PaneOpenStyle::Fade,
            pane_open_duration_ms: 200,
            pane_close_duration_ms: 150,
            focus_transition_speed: 15.0,
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
    /// Automatically copy selected text to clipboard on mouse release.
    pub copy_on_select: bool,
    /// Clear text selection when typing.
    pub clear_selection_on_type: bool,
    /// Send desktop notification when a command takes longer than this many seconds.
    /// Requires shell integration (OSC 133). 0 = disabled.
    pub notify_command_threshold_secs: u64,
    /// Audio file path for bell notification. Empty = no audio.
    pub bell_audio: String,
    /// Request window attention on bell (urgency hint).
    pub bell_urgency: bool,
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
            copy_on_select: false,
            clear_selection_on_type: true,
            notify_command_threshold_secs: 0,
            bell_audio: String::new(),
            bell_urgency: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StatusBarConfig {
    /// Vertical padding as a proportion of cell height (applied above and below text).
    pub padding_ratio: f32,
    /// Text baseline factor (0.0-1.0) relative to cell height.
    pub text_baseline: f32,
    /// Leader indicator line height as a proportion of cell height.
    pub leader_indicator_ratio: f32,
    /// Legacy: extra height in pixels (overrides padding_ratio if set by user config).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_padding: Option<f32>,
}

impl Default for StatusBarConfig {
    fn default() -> Self {
        StatusBarConfig {
            padding_ratio: 0.25,
            text_baseline: 0.8,
            leader_indicator_ratio: 0.1,
            height_padding: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InputConfig {
    /// Leader mode timeout in milliseconds (prefix mode only).
    pub leader_timeout_ms: u64,
    /// Double-tap window in milliseconds for SendLeaderKey.
    pub double_tap_window_ms: u64,
    /// Scroll multiplier for line-based scroll delta.
    pub scroll_multiplier: f64,
    /// Input mode: "prefix" (tmux-style, one action per leader press)
    /// or "sticky" (zellij-style, stay in leader until Esc).
    pub mode: String,
    /// Enable focus-follows-mouse: hovering over a pane focuses it.
    pub focus_follows_mouse: bool,
}

impl Default for InputConfig {
    fn default() -> Self {
        InputConfig {
            leader_timeout_ms: 1000,
            double_tap_window_ms: 300,
            scroll_multiplier: 50.0,
            mode: "prefix".to_string(),
            focus_follows_mouse: false,
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
    /// Present mode: "fifo" (vsync), "mailbox" (low-latency), "immediate" (no vsync).
    pub present_mode: String,
    /// GPU backend: "auto" (default), "blade" (Vulkan), "gl" (OpenGL/EGL).
    pub backend: String,
}

impl Default for RenderConfig {
    fn default() -> Self {
        RenderConfig {
            frame_interval_ms: 16,
            atlas_size: 2048,
            max_glyph_instances: 32768,
            max_rectangles: 8192,
            frame_latency: 2,
            present_mode: "fifo".to_string(),
            backend: "auto".to_string(),
        }
    }
}

/// A single width preset: either a proportion of viewport or fixed pixels.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PresetWidth {
    Proportion { proportion: f64 },
    Fixed { fixed: f64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutConfig {
    /// Default width for newly created columns.
    /// If not set, uses the first entry in preset_widths.
    pub default_column_width: Option<PresetWidth>,
    /// Width presets to cycle through with the preset key.
    /// Supports both proportional and fixed-pixel widths.
    pub preset_widths: Vec<PresetWidth>,
    /// How the viewport centers on the focused column.
    /// "always" = always center, "on-overflow" = center only when column wider than viewport,
    /// "never" = left-aligned scrolling.
    pub center_focused_column: CenterStrategy,
}

/// Strategy for centering the focused column in the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CenterStrategy {
    Always,
    OnOverflow,
    Never,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        LayoutConfig {
            default_column_width: None,
            center_focused_column: CenterStrategy::Always,
            preset_widths: vec![
                PresetWidth::Proportion {
                    proportion: 1.0 / 3.0,
                },
                PresetWidth::Proportion { proportion: 0.5 },
                PresetWidth::Proportion {
                    proportion: 2.0 / 3.0,
                },
                PresetWidth::Proportion { proportion: 1.0 },
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GestureConfig {
    /// Enable trackpad gesture support.
    pub enabled: bool,
    /// Pinch-to-zoom sensitivity multiplier.
    pub pinch_sensitivity: f64,
    /// Natural (inverted) scrolling direction for trackpad.
    pub natural_scroll: bool,
    /// Pixel threshold to trigger a workspace row switch via vertical swipe.
    pub vertical_swipe_threshold: f64,
    /// Pixel threshold to trigger a column switch via horizontal swipe.
    pub horizontal_swipe_threshold: f64,
    /// Smooth scrollback: track gesture phases for momentum scrolling.
    pub smooth_scroll: bool,
    /// Pixels per scrollback line for smooth scroll conversion.
    pub scroll_pixels_per_line: f64,
}

impl Default for GestureConfig {
    fn default() -> Self {
        GestureConfig {
            enabled: true,
            pinch_sensitivity: 2.0,
            natural_scroll: true,
            vertical_swipe_threshold: 50.0,
            horizontal_swipe_threshold: 50.0,
            smooth_scroll: true,
            scroll_pixels_per_line: 20.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RemoteConfig {
    /// Enable TCP listener for remote connections (default: false).
    pub enabled: bool,
    /// TCP port to listen on, bound to 127.0.0.1 only (default: 7890).
    pub port: u16,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        RemoteConfig {
            enabled: false,
            port: 7890,
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
        config.validate();
        Ok(config)
    }

    pub fn validate(&mut self) {
        if self.animation.speed <= 0.0 {
            log::warn!("animation.speed <= 0.0, resetting to 12.0");
            self.animation.speed = 12.0;
        }
        if self.animation.epsilon <= 0.0 {
            log::warn!("animation.epsilon <= 0.0, resetting to 0.1");
            self.animation.epsilon = 0.1;
        }
        if self.animation.focus_transition_speed <= 0.0 {
            log::warn!("animation.focus_transition_speed <= 0.0, resetting to 15.0");
            self.animation.focus_transition_speed = 15.0;
        }
        if self.animation.pane_open_duration_ms == 0 {
            log::warn!("animation.pane_open_duration_ms == 0, resetting to 200");
            self.animation.pane_open_duration_ms = 200;
        }
        if self.animation.pane_close_duration_ms == 0 {
            log::warn!("animation.pane_close_duration_ms == 0, resetting to 150");
            self.animation.pane_close_duration_ms = 150;
        }
        if self.appearance.border_width < 0.0 {
            log::warn!("appearance.border_width < 0.0, resetting to 0.0");
            self.appearance.border_width = 0.0;
        }
        if self.appearance.inactive_opacity < 0.0 || self.appearance.inactive_opacity > 1.0 {
            log::warn!(
                "appearance.inactive_opacity out of range, clamping to [0.0, 1.0]"
            );
            self.appearance.inactive_opacity = self.appearance.inactive_opacity.clamp(0.0, 1.0);
        }
        if self.font.size <= 0.0 {
            log::warn!("font.size <= 0.0, resetting to 14.0");
            self.font.size = 14.0;
        }
        if self.terminal.cursor_opacity < 0.0 || self.terminal.cursor_opacity > 1.0 {
            log::warn!(
                "terminal.cursor_opacity out of range, clamping to [0.0, 1.0]"
            );
            self.terminal.cursor_opacity = self.terminal.cursor_opacity.clamp(0.0, 1.0);
        }
        if self.render.frame_interval_ms == 0 {
            log::warn!("render.frame_interval_ms == 0, resetting to 16");
            self.render.frame_interval_ms = 16;
        }
    }
}

#[cfg(unix)]
fn home_dir() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("HOME")
        && !home.is_empty()
    {
        return Some(PathBuf::from(home));
    }
    let uid = unsafe { libc::getuid() };
    let mut buf = vec![0u8; 4096];
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result = std::ptr::null_mut();
    let ret = unsafe {
        libc::getpwuid_r(
            uid,
            &mut pwd,
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
            &mut result,
        )
    };
    if ret == 0 && !result.is_null() {
        let dir = unsafe { std::ffi::CStr::from_ptr(pwd.pw_dir) };
        if let Ok(s) = dir.to_str() {
            return Some(PathBuf::from(s));
        }
    }
    None
}

pub fn config_path() -> PathBuf {
    #[cfg(unix)]
    {
        if let Ok(config_home) = std::env::var("XDG_CONFIG_HOME")
            && !config_home.is_empty()
        {
            return PathBuf::from(config_home).join("ciri").join("config.toml");
        }
        // $HOME/.config is the XDG default when XDG_CONFIG_HOME is unset
        match home_dir() {
            Some(home) => home.join(".config").join("ciri").join("config.toml"),
            None => {
                log::warn!("cannot determine home directory, using /tmp/ciri as config base");
                PathBuf::from("/tmp").join(".config").join("ciri").join("config.toml")
            }
        }
    }
    #[cfg(windows)]
    {
        let appdata = std::env::var("APPDATA")
            .unwrap_or_else(|_| r"C:\Users\Default\AppData\Roaming".to_string());
        PathBuf::from(appdata).join("ciri").join("config.toml")
    }
}
