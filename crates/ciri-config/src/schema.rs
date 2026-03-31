//! Configuration schema: all struct/enum definitions and their defaults.

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::keys::KeybindConfig;
use crate::theme::ThemeConfig;

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
#[serde(default)]
#[derive(Default)]
pub struct CiriConfig {
    #[garde(dive)]
    pub font: FontConfig,
    #[garde(dive)]
    pub appearance: AppearanceConfig,
    #[garde(dive)]
    pub animation: AnimationConfig,
    #[garde(skip)]
    pub keys: KeybindConfig,
    #[garde(skip)]
    pub theme: ThemeConfig,
    #[garde(skip)]
    pub window: WindowConfig,
    #[garde(dive)]
    pub terminal: TerminalConfig,
    #[garde(dive)]
    pub statusbar: StatusBarConfig,
    #[garde(skip)]
    pub input: InputConfig,
    #[garde(dive)]
    pub render: RenderConfig,
    #[garde(skip)]
    pub layout: LayoutConfig,
    #[garde(skip)]
    pub gesture: GestureConfig,
    #[garde(skip)]
    pub remote: RemoteConfig,
    #[garde(skip)]
    pub session: SessionConfig,
    #[garde(skip)]
    pub server: ServerConfig,
    #[garde(skip)]
    pub prediction: PredictionConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
#[serde(default)]
pub struct FontConfig {
    #[garde(skip)]
    pub family: String,
    #[garde(range(min = 1.0, max = 200.0))]
    pub size: f32,
}

impl Default for FontConfig {
    fn default() -> Self {
        FontConfig {
            family: default_font_family().to_string(),
            size: 10.0,
        }
    }
}

fn default_font_family() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Menlo"
    }
    #[cfg(target_os = "windows")]
    {
        "Consolas"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "monospace"
    }
}

/// Focus ring style for the active pane border.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum FocusRingStyle {
    #[default]
    Solid,
    Glow,
    Dashed,
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

/// Global animation speed preset.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AnimationPreset {
    /// ~0.3s — quick and responsive.
    Snappy,
    /// ~0.5s — comfortable balance (Apple-like).
    #[default]
    Default,
    /// ~0.7s — relaxed, smooth transitions.
    Smooth,
    /// ~1.0s — slow and elegant.
    Gentle,
}

/// Pane open/close animation style.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PaneOpenStyle {
    #[default]
    Fade,
    SlideUp,
    SlideDown,
    SlideLeft,
    FadeSlideUp,
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
#[serde(default)]
pub struct AppearanceConfig {
    #[garde(skip)]
    pub padding: f32,
    #[garde(skip)]
    pub column_gap: f32,
    #[garde(range(min = 0.0))]
    pub border_width: f32,
    #[garde(skip)]
    pub active_border_color: String,
    #[garde(skip)]
    pub inactive_border_color: String,
    #[garde(range(min = 0.0, max = 1.0))]
    pub inactive_opacity: f32,
    #[garde(skip)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
#[serde(default)]
pub struct AnimationConfig {
    #[garde(skip)]
    pub enabled: bool,
    #[garde(skip)]
    pub preset: AnimationPreset,
    #[garde(skip)]
    pub pane_open_style: PaneOpenStyle,
    #[garde(range(min = 0.000_001, max = 1.0))]
    pub overview_zoom_fit: f32,
    #[garde(range(min = 0.000_001, max = 1.0))]
    pub zoom_threshold: f32,
    #[garde(range(min = 0.0, max = 1.0))]
    pub drag_opacity: f32,
}

impl Default for AnimationConfig {
    fn default() -> Self {
        AnimationConfig {
            enabled: true,
            preset: AnimationPreset::Default,
            pane_open_style: PaneOpenStyle::Fade,
            overview_zoom_fit: 0.9,
            zoom_threshold: 0.99,
            drag_opacity: 0.6,
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

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
#[serde(default)]
pub struct TerminalConfig {
    #[garde(range(min = 1))]
    pub default_cols: u16,
    #[garde(range(min = 1))]
    pub default_rows: u16,
    #[garde(skip)]
    pub cursor_color: String,
    #[garde(range(min = 0.0, max = 1.0))]
    pub cursor_opacity: f32,
    #[garde(skip)]
    pub cursor_blink: bool,
    #[garde(range(min = 1))]
    pub cursor_blink_interval_ms: u64,
    #[garde(skip)]
    pub shell: String,
    #[garde(range(min = 1))]
    pub scrollback_lines: usize,
    #[garde(skip)]
    pub copy_on_select: bool,
    #[garde(skip)]
    pub clear_selection_on_type: bool,
    #[garde(skip)]
    pub notify_command_threshold_secs: u64,
    #[garde(skip)]
    pub bell_audio: String,
    #[garde(skip)]
    pub bell_urgency: bool,
    #[garde(skip)]
    pub paste_warn_threshold: usize,
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
            paste_warn_threshold: 5000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
#[serde(default)]
pub struct StatusBarConfig {
    #[garde(skip)]
    pub position: StatusBarPosition,
    #[garde(range(min = 0.0))]
    pub padding_ratio: f32,
    #[garde(skip)]
    pub text_baseline: f32,
    #[garde(skip)]
    pub leader_indicator_ratio: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[garde(skip)]
    pub height_padding: Option<f32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum StatusBarPosition {
    #[default]
    Top,
    Bottom,
}

impl Default for StatusBarConfig {
    fn default() -> Self {
        StatusBarConfig {
            position: StatusBarPosition::Top,
            padding_ratio: 0.25,
            text_baseline: 0.8,
            leader_indicator_ratio: 0.1,
            height_padding: None,
        }
    }
}

/// Input mode determines how keybindings are activated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum InputMode {
    #[default]
    Prefix,
    Sticky,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InputConfig {
    pub leader_timeout_ms: u64,
    pub double_tap_window_ms: u64,
    pub scroll_multiplier: f64,
    pub mode: InputMode,
    pub focus_follows_mouse: bool,
}

impl Default for InputConfig {
    fn default() -> Self {
        InputConfig {
            leader_timeout_ms: 1000,
            double_tap_window_ms: 300,
            scroll_multiplier: 50.0,
            mode: InputMode::Prefix,
            focus_follows_mouse: false,
        }
    }
}

/// GPU present mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PresentMode {
    #[default]
    Fifo,
    Mailbox,
    Immediate,
}

/// GPU backend selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RenderBackend {
    #[default]
    Auto,
    Blade,
    Gl,
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
#[serde(default)]
pub struct RenderConfig {
    #[garde(range(min = 1))]
    pub frame_interval_ms: u64,
    #[garde(skip)]
    pub atlas_size: u32,
    #[garde(skip)]
    pub max_glyph_instances: usize,
    #[garde(skip)]
    pub max_rectangles: usize,
    #[garde(skip)]
    pub frame_latency: u32,
    #[garde(skip)]
    pub present_mode: PresentMode,
    #[garde(skip)]
    pub backend: RenderBackend,
}

impl Default for RenderConfig {
    fn default() -> Self {
        RenderConfig {
            frame_interval_ms: 16,
            atlas_size: 2048,
            max_glyph_instances: 32768,
            max_rectangles: 8192,
            frame_latency: 2,
            present_mode: PresentMode::Fifo,
            backend: RenderBackend::Auto,
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
    pub default_column_width: Option<PresetWidth>,
    pub preset_widths: Vec<PresetWidth>,
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
    pub enabled: bool,
    pub pinch_sensitivity: f64,
    pub natural_scroll: bool,
    pub vertical_swipe_threshold: f64,
    pub horizontal_swipe_threshold: f64,
    pub smooth_scroll: bool,
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
    pub enabled: bool,
    pub port: u16,
    pub hosts: Vec<RemoteHostConfig>,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        RemoteConfig {
            enabled: false,
            port: 7890,
            hosts: Vec::new(),
        }
    }
}

fn default_remote_port() -> u16 {
    7890
}
fn default_ssh_port() -> u16 {
    22
}

/// A configured remote host for the command palette.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteHostConfig {
    pub name: String,
    pub host: String,
    #[serde(default = "default_remote_port")]
    pub port: u16,
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
}

// ── Session restore ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionConfig {
    /// Whether to detect and auto-resume AI agents on restart.
    pub restore_agents: bool,
    /// Interval in seconds for agent-state autosave (separate from layout autosave).
    pub agent_save_interval_secs: u64,
}

impl Default for SessionConfig {
    fn default() -> Self {
        SessionConfig {
            restore_agents: true,
            agent_save_interval_secs: 30,
        }
    }
}

// ── Server daemon ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// Seconds to wait before shutting down when all sessions/clients are gone.
    /// 0 means shut down immediately (old behavior). Default: 300 (5 minutes).
    pub idle_timeout_secs: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            idle_timeout_secs: 300,
        }
    }
}

// ── Input prediction (Mosh-style speculative echo) ────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PredictionMode {
    #[default]
    Never,
    Always,
    Adaptive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PredictionConfig {
    pub mode: PredictionMode,
    pub threshold_ms: u64,
    pub show_underline: bool,
}

impl Default for PredictionConfig {
    fn default() -> Self {
        PredictionConfig {
            mode: PredictionMode::Never,
            threshold_ms: 30,
            show_underline: true,
        }
    }
}
