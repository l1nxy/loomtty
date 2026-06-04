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
    #[garde(dive)]
    pub tabbar: TabBarConfig,
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
    #[garde(dive)]
    pub web: WebConfig,
}

/// Controls when ligatures are applied during shaping.
///
/// Mirrors Kitty's `disable_ligatures` setting. Ghostty and WT achieve the
/// same effect via OpenType `features = ["-calt", "-liga"]`, but the explicit
/// enum reads better and lets us cheaply skip ligature shaping on the cursor
/// row only.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DisableLigatures {
    /// Always shape ligatures — the default.
    #[default]
    Never,
    /// Disable ligatures only on the row containing the cursor.
    Cursor,
    /// Never shape ligatures.
    Always,
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
#[serde(default)]
pub struct FontConfig {
    #[garde(skip)]
    pub family: String,
    #[garde(range(min = 1.0, max = 200.0))]
    pub size: f32,
    /// Optional UI font overrides. If `None`, UI text reuses the terminal
    /// font. If `Some`, the UI (status bar, palette, tab bar, etc.) uses
    /// the given font — which may be proportional (Inter, Segoe UI, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[garde(skip)]
    pub ui: Option<UiFontConfig>,

    /// OpenType feature toggles. Strings follow the HarfBuzz/CSS syntax,
    /// e.g. `"liga"`, `"+ss01"`, `"-calt"`, `"zero=1"`. Applied to terminal
    /// shaping (rustybuzz) on Linux/Windows.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[garde(skip)]
    pub features: Vec<String>,

    /// When to apply ligatures during shaping (`never` / `cursor` / `always`).
    #[serde(default, skip_serializing_if = "is_default_disable_ligatures")]
    #[garde(skip)]
    pub disable_ligatures: DisableLigatures,

    /// Preferred OpenType weight for the regular face (100..=1000). When
    /// `None`, the closest-to-400 face is selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[garde(skip)]
    pub weight: Option<u16>,

    /// Multiplier on the computed cell width (1.0 = unchanged).
    #[garde(range(min = 0.5, max = 3.0))]
    pub adjust_cell_width: f32,
    /// Multiplier on the computed cell height (1.0 = unchanged).
    #[garde(range(min = 0.5, max = 3.0))]
    pub adjust_cell_height: f32,

    /// Pixel offset added to the underline's vertical position
    /// (positive = lower).
    #[garde(skip)]
    pub adjust_underline_position: f32,
    /// Multiplier on the underline thickness (default 1.0px).
    #[garde(range(min = 0.0, max = 16.0))]
    pub adjust_underline_thickness: f32,

    /// Pixel offset added to the strikethrough vertical position.
    #[garde(skip)]
    pub adjust_strikethrough_position: f32,
    /// Multiplier on the strikethrough thickness (default 1.0px).
    #[garde(range(min = 0.0, max = 16.0))]
    pub adjust_strikethrough_thickness: f32,
}

fn is_default_disable_ligatures(v: &DisableLigatures) -> bool {
    *v == DisableLigatures::Never
}

impl FontConfig {
    /// Parse `features` strings into `(tag, value, range)` triples ready for
    /// HarfBuzz-style consumers. Tags shorter than four bytes are zero-padded
    /// (HarfBuzz convention); over-long tags are truncated.
    ///
    /// Accepted forms:
    /// - `"liga"` / `"+liga"`        → `("liga", 1)`
    /// - `"-liga"` / `"liga=0"`      → `("liga", 0)`
    /// - `"ss05=2"`                  → `("ss05", 2)`
    pub fn parsed_features(&self) -> Vec<([u8; 4], u32)> {
        self.features
            .iter()
            .filter_map(|raw| parse_feature_string(raw))
            .collect()
    }
}

fn parse_feature_string(raw: &str) -> Option<([u8; 4], u32)> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    let (sign, rest) = match s.as_bytes()[0] {
        b'+' => (1u32, &s[1..]),
        b'-' => (0u32, &s[1..]),
        _ => (1u32, s),
    };
    let (name, value) = match rest.split_once('=') {
        Some((name, val)) => {
            let v: u32 = val.trim().parse().ok()?;
            (name.trim(), v)
        }
        None => (rest.trim(), sign),
    };
    if name.is_empty() {
        return None;
    }
    let bytes = name.as_bytes();
    let mut tag = [b' '; 4];
    for (i, slot) in tag.iter_mut().enumerate() {
        if let Some(b) = bytes.get(i) {
            *slot = *b;
        }
    }
    Some((tag, value))
}

/// UI-only font override. Does not have to be monospace.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
#[serde(default)]
pub struct UiFontConfig {
    #[garde(skip)]
    pub family: String,
    #[garde(range(min = 1.0, max = 200.0))]
    pub size: f32,
}

impl Default for UiFontConfig {
    fn default() -> Self {
        UiFontConfig {
            family: String::new(),
            size: 10.0,
        }
    }
}

impl Default for FontConfig {
    fn default() -> Self {
        FontConfig {
            family: default_font_family().to_string(),
            size: 10.0,
            ui: None,
            features: Vec::new(),
            disable_ligatures: DisableLigatures::Never,
            weight: None,
            adjust_cell_width: 1.0,
            adjust_cell_height: 1.0,
            adjust_underline_position: 0.0,
            adjust_underline_thickness: 1.0,
            adjust_strikethrough_position: 0.0,
            adjust_strikethrough_thickness: 1.0,
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
    /// Logical-pixel corner radius for pane backgrounds, cell content, and
    /// the focus ring. `0.0` keeps the historical sharp-corner look; values
    /// above zero round all four corners of every pane and clip cell
    /// background / glyph alpha at the arc (niri-style mask, no extra pass).
    #[garde(range(min = 0.0, max = 64.0))]
    pub pane_corner_radius: f32,
    /// Optional path to a background image rendered behind everything
    /// (panes in normal mode, thumbnails in overview mode). Empty =
    /// solid `theme.overview_background` fill. Supports `~`, absolute
    /// paths, and paths relative to the config directory. PNG and JPEG
    /// only. Fitted with a `cover` policy (filled to viewport, overflow
    /// cropped, aspect preserved).
    #[garde(skip)]
    pub background_image: String,
    /// Dim factor applied over the background image. `0.0` shows the
    /// image at full strength; `1.0` blends it entirely toward
    /// `theme.overview_background` (hides the image). Use a mid value
    /// (~0.4-0.6) to keep pane content legible over a busy wallpaper.
    /// Ignored when `background_image` is empty.
    #[garde(range(min = 0.0, max = 1.0))]
    pub background_dim: f32,
    /// Pane background opacity (`0.0`-`1.0`). `1.0` keeps panes fully
    /// opaque; lower values let the global `background_image` (or
    /// `theme.overview_background` if no image is set) show through.
    /// `~0.8` is a good middle ground over a wallpaper. Applies only to
    /// pane interior bg rects (terminal pane background + per-cell ANSI
    /// bg colours); chrome (palette, status bar, etc.) stays opaque so
    /// it remains legible.
    #[garde(range(min = 0.0, max = 1.0))]
    pub pane_opacity: f32,
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
            // Default 0.0 preserves the historical sharp-corner look on
            // upgrade; users opt in by raising the value in their config.
            pane_corner_radius: 0.0,
            background_image: String::new(),
            // Default 0.0 = no dim layer when no image is set; users
            // typically pick ~0.5 once they configure an image.
            background_dim: 0.0,
            // Default 1.0 = fully opaque panes (existing visual behaviour
            // on upgrade). Users opt in to translucency by lowering it.
            pane_opacity: 1.0,
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
            title: "ciritty".to_string(),
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
    /// Override cursor shape regardless of what the running app reports.
    /// Accepted values: "block", "beam", "underline", "hollow_block".
    /// Empty string means "follow the app's reported shape".
    #[garde(skip)]
    pub cursor_shape: String,
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
            cursor_shape: "beam".to_string(),
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

/// Where to draw the pane-tab list.
///
/// - `Integrated`: tabs sit inside the status bar (default, matches the
///   pre-tree-layout behaviour).
/// - `Left` / `Right`: tabs are extracted into a dedicated vertical bar
///   on the chosen side of the terminal viewport; the status bar loses
///   the tab strip and only shows session / workspace / mode badges.
///
/// Top/Bottom (as dedicated bars separate from the status bar) are not
/// supported: the status bar already exists in those positions and
/// embedding the tabs there is what `Integrated` means.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TabBarPosition {
    #[default]
    Integrated,
    Left,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
#[serde(default)]
pub struct TabBarConfig {
    #[garde(skip)]
    pub position: TabBarPosition,
    /// Horizontal width of the side tab bar in pixels. Ignored when
    /// `position == Integrated`.
    #[garde(range(min = 40.0, max = 800.0))]
    pub width: f32,
    /// Vertical height per tab in pixels. Ignored when
    /// `position == Integrated`.
    #[garde(range(min = 12.0, max = 200.0))]
    pub tab_height: f32,
    /// Vertical gap between adjacent tabs in pixels. Ignored when
    /// `position == Integrated`.
    #[garde(range(min = 0.0, max = 40.0))]
    pub tab_gap: f32,
    /// Fixed per-tab width for `position = Integrated`, expressed in
    /// characters. The actual pixel width is measured through the UI
    /// shaper (≈ `N * ui_font_char_advance`) so the slot matches what
    /// an `N`-char label renders to in the UI font — otherwise the old
    /// `N * terminal_cell_w` formula over-allocates when the UI font
    /// is narrower than the terminal monospace cell. Labels longer than
    /// the slot are ellipsized; shorter ones leave background on the
    /// trailing edge (same as an at-max label that just reached the cap).
    #[garde(range(min = 4, max = 80))]
    pub pane_tab_width_chars: usize,
}

impl Default for TabBarConfig {
    fn default() -> Self {
        TabBarConfig {
            position: TabBarPosition::Integrated,
            width: 200.0,
            tab_height: 36.0,
            tab_gap: 4.0,
            pane_tab_width_chars: 25,
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

/// Alpha blending color space.
///
/// Controls how anti-aliased text is composited onto the background:
/// - `native`: blend in sRGB (gamma-encoded) space — fastest, but text appears
///   slightly brighter/thicker than perceptually correct.
/// - `linear`: blend in linear light — correct compositing, but dark-on-light
///   text can appear thinner than expected.
/// - `linear-corrected`: linear blend with a per-pixel weight correction that
///   preserves the stroke weight of gamma-space rendering while eliminating
///   brightness artifacts. Recommended default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AlphaBlending {
    Native,
    Linear,
    LinearCorrected,
}

impl AlphaBlending {
    pub fn is_linear(self) -> bool {
        matches!(self, Self::Linear | Self::LinearCorrected)
    }

    pub fn use_correction(self) -> bool {
        matches!(self, Self::LinearCorrected)
    }
}

fn default_alpha_blending() -> AlphaBlending {
    if cfg!(target_os = "macos") {
        AlphaBlending::Native
    } else {
        AlphaBlending::LinearCorrected
    }
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
    #[garde(skip)]
    #[serde(default = "default_alpha_blending")]
    pub alpha_blending: AlphaBlending,
    /// Post-process softness strength applied to the final image, in `[0, 1]`.
    /// `0` disables the pass entirely (no overhead). `1` fully replaces the
    /// image with a 5-tap separable Gaussian. Only honoured by the GL backend
    /// when linear blending is enabled.
    #[garde(range(min = 0.0, max = 1.0))]
    #[serde(default)]
    pub softness: f32,
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
            alpha_blending: default_alpha_blending(),
            softness: 0.0,
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
    /// How the viewport centers on the focused column.
    /// "never" (default) = minimal scroll, keep neighbors visible when possible
    /// "on-overflow" = center only when the focused column and the one we came
    ///   from don't fit on-screen together (PaperWM-style)
    /// "always" = always center the focused column
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
            center_focused_column: CenterStrategy::Never,
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

// ── Web (browser) gateway ──────────────────────────────────────────

/// Browser-facing WebSocket gateway. Disabled by default; when enabled,
/// the daemon spins up an extra listener that wraps each WS connection
/// in an `AsyncRead + AsyncWrite` shim and hands it to the same client
/// handler used by Unix/TCP transports.
///
/// `bind` defaults to loopback. If you point this at a non-loopback
/// address you are responsible for terminating TLS upstream — the
/// gateway itself speaks plain ws:// and relies on a single shared
/// token for authentication.
/// Minimum byte-length for `web.token` when the gateway is enabled.
/// Exposed at config-crate scope so the schema validator and the daemon
/// startup check agree on a single floor. `openssl rand -hex 16`
/// produces 32 bytes, well above this.
pub const MIN_WEB_TOKEN_BYTES: usize = 16;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WebConfig {
    pub enabled: bool,
    /// Empty string is read as "127.0.0.1" by the daemon. Otherwise the
    /// value must parse as an IP address.
    pub bind: String,
    /// Reject `port = 0` (OS-assigned ephemeral); see Validate impl.
    pub port: u16,
    /// Shared token required on the upgrade. The Validate impl enforces
    /// a `MIN_WEB_TOKEN_BYTES` floor when `enabled = true`. `enabled =
    /// false, token = ""` is a valid resting state.
    pub token: String,
    /// CSRF defense for browser clients. Each entry is exact-matched
    /// against the `Origin` header on the upgrade request (e.g.,
    /// `"http://localhost:5173"`, `"https://terminal.example.com"`, or
    /// the literal `"null"` for `file://` / sandboxed contexts).
    ///
    /// Behavior:
    /// - empty + loopback `bind` → permissive (no Origin check).
    /// - empty + non-loopback `bind` → daemon refuses to start; an
    ///   exposed gateway with no Origin policy is a CSRF magnet.
    /// - non-empty → strict exact-match on every handshake.
    pub allowed_origins: Vec<String>,
    /// Directory the daemon serves the browser SPA from over plain HTTP
    /// on the same `[web]` port. Empty → `<server-exe-dir>/web` (the
    /// install directory next to `ciritty-server`). These files are
    /// served WITHOUT the token (only the `/ws` data channel is
    /// authenticated), so the directory must contain only the public web
    /// bundle — never secrets, `.env`, or config dumps.
    pub static_dir: String,
}

/// `Validate` is implemented manually rather than via `#[derive]` so
/// it can enforce the cross-field `enabled = true → token >=
/// MIN_WEB_TOKEN_BYTES` rule that no per-field attribute can express.
impl garde::Validate for WebConfig {
    type Context = ();

    fn validate_into(
        &self,
        _ctx: &Self::Context,
        parent: &mut dyn FnMut() -> garde::Path,
        report: &mut garde::Report,
    ) {
        if self.port == 0 {
            report.append(
                parent().join("port"),
                garde::Error::new("port 0 is OS-assigned ephemeral; pick an explicit port"),
            );
        }

        let parsed_bind = if self.bind.is_empty() {
            // Empty means "default to loopback" at daemon bind time.
            Some(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST))
        } else {
            match self.bind.parse::<std::net::IpAddr>() {
                Ok(ip) => Some(ip),
                Err(_) => {
                    report.append(
                        parent().join("bind"),
                        garde::Error::new(format!(
                            "`bind` must be an IP address, got {:?}",
                            self.bind,
                        )),
                    );
                    None
                }
            }
        };

        if self.enabled {
            let trimmed = self.token.trim();
            if trimmed.is_empty() {
                report.append(
                    parent().join("token"),
                    garde::Error::new("token must be non-empty when web.enabled = true"),
                );
            } else if trimmed.len() < MIN_WEB_TOKEN_BYTES {
                report.append(
                    parent().join("token"),
                    garde::Error::new(format!(
                        "token is {} bytes (after trim); minimum is {MIN_WEB_TOKEN_BYTES} \
                         bytes when web.enabled = true",
                        trimmed.len(),
                    )),
                );
            } else if is_placeholder_token(trimmed) {
                // The sample in `config/default.toml` is longer than
                // MIN_WEB_TOKEN_BYTES, so it passes the length floor
                // and would otherwise start the gateway with a token
                // every clone of this repo knows. On the default
                // loopback bind + empty origins policy, any local
                // browser tab could then authenticate to the daemon.
                report.append(
                    parent().join("token"),
                    garde::Error::new(
                        "token looks like the documented placeholder; \
                         replace it with a real secret (e.g. \
                         `openssl rand -hex 16`)",
                    ),
                );
            }

            // Non-loopback bind with no Origin allowlist invites CSRF
            // from any browser tab. Enforce at schema level so
            // config-only validation (e.g., a future --check-config
            // CLI) catches it without booting the daemon.
            if let Some(ip) = parsed_bind
                && !ip.is_loopback()
                && self.allowed_origins.is_empty()
            {
                report.append(
                    parent().join("allowed_origins"),
                    garde::Error::new(format!(
                        "bind {ip} is not loopback — set allowed_origins to the \
                         browser origin(s) you intend to serve"
                    )),
                );
            }
        }

        for (i, origin) in self.allowed_origins.iter().enumerate() {
            if !looks_like_origin(origin) {
                report.append(
                    parent().join("allowed_origins").join(i),
                    garde::Error::new(format!(
                        "{origin:?} is not a valid origin \
                         (expected scheme://host[:port] or the literal \"null\")"
                    )),
                );
            } else if origin != "null" && origin_contains_uppercase(origin) {
                // Browsers normalise scheme/host to lowercase per RFC
                // 6454 §6.2; an uppercase entry here would never match
                // the inbound Origin header on a real handshake.
                report.append(
                    parent().join("allowed_origins").join(i),
                    garde::Error::new(format!(
                        "{origin:?} contains uppercase characters; \
                         browsers always send lowercase scheme/host — \
                         normalise this entry to lowercase"
                    )),
                );
            }
        }
    }
}

fn origin_contains_uppercase(value: &str) -> bool {
    value.chars().any(|c| c.is_ascii_uppercase())
}

/// Reject the documented sample token (and a handful of obvious
/// near-variants) so a user who uncomments the example in
/// `config/default.toml` without replacing the value can't start a
/// gateway with a secret every clone of this repo knows.
/// Returns true if `token` is fit to enable the web gateway: non-empty,
/// meets the [`MIN_WEB_TOKEN_BYTES`] floor, and isn't a known
/// placeholder. `ciritty web` uses this to decide whether to reuse the
/// configured token or mint a fresh one.
pub fn web_token_is_usable(token: &str) -> bool {
    let t = token.trim();
    t.len() >= MIN_WEB_TOKEN_BYTES && !is_placeholder_token(t)
}

fn is_placeholder_token(trimmed: &str) -> bool {
    const PLACEHOLDERS: &[&str] = &[
        "REPLACE_WITH_OUTPUT_OF_openssl_rand_hex_16",
        "replace_with_output_of_openssl_rand_hex_16",
        "CHANGE_ME",
        "changeme",
        "TODO",
        "placeholder",
    ];
    PLACEHOLDERS.iter().any(|p| trimmed.eq_ignore_ascii_case(p))
}

fn looks_like_origin(value: &str) -> bool {
    // Cheap structural check; defers full RFC 6454 parsing to the
    // browser. `null` is the sentinel sandboxed contexts send (RFC
    // 6454 §6).
    //
    // Only http/https are accepted. Per RFC 6454 the Origin header
    // always carries an HTTP-origin even for WebSocket connections —
    // browsers re-derive the HTTP scheme from the page's URL. A
    // configured `ws://` / `wss://` entry would never match a real
    // browser handshake and would silently break authentication.
    if value == "null" {
        return true;
    }
    let Some(rest) = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"))
    else {
        return false;
    };
    // Origin is exactly `scheme://host[:port]` — reject any path,
    // query, fragment, or userinfo that would never appear in a real
    // browser `Origin` header (and that the exact-match runtime check
    // would silently fail against).
    !rest.is_empty()
        && !rest.contains('/')
        && !rest.contains('?')
        && !rest.contains('#')
        && !rest.contains('@')
}

#[cfg(test)]
mod web_config_tests {
    use super::*;
    use garde::Validate;

    #[test]
    fn defaults_validate() {
        WebConfig::default().validate().expect("default must pass");
    }

    #[test]
    fn rejects_port_zero() {
        let cfg = WebConfig {
            port: 0,
            ..WebConfig::default()
        };
        let err = cfg.validate().expect_err("port=0 must fail");
        assert!(err.to_string().contains("port"), "got: {err}");
    }

    #[test]
    fn rejects_hostname_bind() {
        for bind in ["example.com", "localhost", "host.local", "10.0.0..1"] {
            let cfg = WebConfig {
                bind: bind.to_string(),
                ..WebConfig::default()
            };
            cfg.validate()
                .expect_err(&format!("bind={bind:?} must fail"));
        }
    }

    #[test]
    fn accepts_valid_ipv4_and_ipv6_binds() {
        for bind in ["127.0.0.1", "0.0.0.0", "10.0.0.1", "::1", "::"] {
            let cfg = WebConfig {
                bind: bind.to_string(),
                ..WebConfig::default()
            };
            cfg.validate()
                .unwrap_or_else(|e| panic!("bind={bind:?} should validate, got {e}"));
        }
    }

    #[test]
    fn accepts_empty_bind_as_loopback_placeholder() {
        let cfg = WebConfig {
            bind: String::new(),
            ..WebConfig::default()
        };
        cfg.validate().expect("empty bind defers to daemon default");
    }

    #[test]
    fn rejects_enabled_without_token() {
        let cfg = WebConfig {
            enabled: true,
            token: String::new(),
            ..WebConfig::default()
        };
        let err = cfg.validate().expect_err("enabled+empty token must fail");
        assert!(err.to_string().contains("token"), "got: {err}");
    }

    #[test]
    fn rejects_enabled_with_short_token() {
        let cfg = WebConfig {
            enabled: true,
            token: "a".repeat(MIN_WEB_TOKEN_BYTES - 1),
            ..WebConfig::default()
        };
        let err = cfg.validate().expect_err("short token must fail");
        let s = err.to_string();
        assert!(s.contains("15 bytes"), "got: {s}");
        assert!(s.contains("16"), "got: {s}");
    }

    #[test]
    fn rejects_enabled_with_only_whitespace_token() {
        let cfg = WebConfig {
            enabled: true,
            token: "   \n\t".to_string(),
            ..WebConfig::default()
        };
        cfg.validate()
            .expect_err("whitespace-only token must fail when enabled");
    }

    #[test]
    fn disabled_with_empty_token_validates() {
        // Resting default state must pass — the floor only applies when
        // the gateway is actually enabled.
        let cfg = WebConfig {
            enabled: false,
            token: String::new(),
            ..WebConfig::default()
        };
        cfg.validate().expect("disabled with empty token is fine");
    }

    #[test]
    fn allowed_origins_accepts_valid_entries() {
        let cfg = WebConfig {
            allowed_origins: vec![
                "http://localhost:5173".to_string(),
                "https://terminal.example.com".to_string(),
                "null".to_string(),
            ],
            ..WebConfig::default()
        };
        cfg.validate().expect("valid origins must pass");
    }

    #[test]
    fn allowed_origins_rejects_uppercase_host() {
        // Browsers send lowercase scheme/host; an uppercase entry would
        // never match.
        let cfg = WebConfig {
            allowed_origins: vec!["https://Terminal.Example.com".to_string()],
            ..WebConfig::default()
        };
        cfg.validate()
            .expect_err("uppercase origin host must be rejected");
    }

    #[test]
    fn allowed_origins_rejects_malformed_entries() {
        for bad in [
            "example.com",              // no scheme
            "http://example.com/path",  // path attached
            "http://",                  // no host
            "",                         // empty
            "ws://127.0.0.1:7891",      // browsers never send ws:// Origin
            "wss://example.com",        // ditto for wss://
            "http://user@example.com",  // userinfo never in real Origin
            "http://example.com?foo=1", // query never in real Origin
            "http://example.com#frag",  // fragment never in real Origin
        ] {
            let cfg = WebConfig {
                allowed_origins: vec![bad.to_string()],
                ..WebConfig::default()
            };
            cfg.validate()
                .expect_err(&format!("origin={bad:?} must fail"));
        }
    }

    #[test]
    fn rejects_non_loopback_bind_without_allowed_origins() {
        let cfg = WebConfig {
            enabled: true,
            token: "0123456789abcdef0123456789abcdef".to_string(),
            bind: "0.0.0.0".to_string(),
            allowed_origins: Vec::new(),
            ..WebConfig::default()
        };
        let err = cfg
            .validate()
            .expect_err("non-loopback bind + empty origins must fail");
        assert!(
            err.to_string().contains("allowed_origins"),
            "got: {err}"
        );
    }

    #[test]
    fn accepts_non_loopback_bind_with_explicit_origins() {
        let cfg = WebConfig {
            enabled: true,
            token: "0123456789abcdef0123456789abcdef".to_string(),
            bind: "0.0.0.0".to_string(),
            allowed_origins: vec!["https://terminal.example.com".to_string()],
            ..WebConfig::default()
        };
        cfg.validate()
            .expect("non-loopback bind with explicit origins is fine");
    }
}

impl Default for WebConfig {
    fn default() -> Self {
        WebConfig {
            enabled: false,
            // Empty string is the loopback placeholder — the daemon
            // substitutes 127.0.0.1 at bind time.
            bind: String::new(),
            port: 7891,
            token: String::new(),
            allowed_origins: Vec::new(),
            // Empty → daemon resolves `<server-exe-dir>/web` at startup.
            static_dir: String::new(),
        }
    }
}

/// Custom `Debug` that redacts the bearer token. The `Debug` derive
/// would otherwise let any `log::debug!("{config:?}")` (or a panic
/// payload, or a `dbg!` left in by a careless reviewer) print the
/// secret in plaintext into operator logs.
impl std::fmt::Debug for WebConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebConfig")
            .field("enabled", &self.enabled)
            .field("bind", &self.bind)
            .field("port", &self.port)
            .field(
                "token",
                &if self.token.is_empty() {
                    "<empty>"
                } else {
                    "<redacted>"
                },
            )
            .field("allowed_origins", &self.allowed_origins)
            .field("static_dir", &self.static_dir)
            .finish()
    }
}

#[cfg(test)]
mod font_feature_tests {
    use super::*;

    fn tag(s: &str) -> [u8; 4] {
        let mut t = [b' '; 4];
        for (i, b) in s.as_bytes().iter().enumerate().take(4) {
            t[i] = *b;
        }
        t
    }

    #[test]
    fn parses_plain_tag_as_enabled() {
        assert_eq!(parse_feature_string("liga"), Some((tag("liga"), 1)));
        assert_eq!(parse_feature_string(" liga "), Some((tag("liga"), 1)));
    }

    #[test]
    fn plus_and_minus_prefixes() {
        assert_eq!(parse_feature_string("+ss01"), Some((tag("ss01"), 1)));
        assert_eq!(parse_feature_string("-calt"), Some((tag("calt"), 0)));
    }

    #[test]
    fn explicit_value_wins_over_sign() {
        assert_eq!(parse_feature_string("ss05=2"), Some((tag("ss05"), 2)));
        assert_eq!(parse_feature_string("-liga=1"), Some((tag("liga"), 1)));
    }

    #[test]
    fn rejects_empty_or_malformed() {
        assert_eq!(parse_feature_string(""), None);
        assert_eq!(parse_feature_string("   "), None);
        assert_eq!(parse_feature_string("=1"), None);
        assert_eq!(parse_feature_string("liga=abc"), None);
    }

    #[test]
    fn pads_short_tags_to_four_bytes() {
        assert_eq!(
            parse_feature_string("aa"),
            Some(([b'a', b'a', b' ', b' '], 1))
        );
    }

    #[test]
    fn parsed_features_filters_invalid() {
        let cfg = FontConfig {
            features: vec!["liga".into(), "".into(), "-calt".into(), "=junk".into()],
            ..FontConfig::default()
        };
        let parsed = cfg.parsed_features();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0], (tag("liga"), 1));
        assert_eq!(parsed[1], (tag("calt"), 0));
    }

    #[test]
    fn default_is_backward_compatible() {
        let cfg = FontConfig::default();
        assert!(cfg.features.is_empty());
        assert_eq!(cfg.disable_ligatures, DisableLigatures::Never);
        assert_eq!(cfg.adjust_cell_width, 1.0);
        assert_eq!(cfg.adjust_cell_height, 1.0);
        assert_eq!(cfg.adjust_underline_thickness, 1.0);
        assert_eq!(cfg.adjust_strikethrough_thickness, 1.0);
        assert_eq!(cfg.adjust_underline_position, 0.0);
        assert_eq!(cfg.adjust_strikethrough_position, 0.0);
        assert_eq!(cfg.weight, None);
    }
}
