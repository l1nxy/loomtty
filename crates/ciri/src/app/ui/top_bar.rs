use ciri_config::config::StatusBarPosition;
use ciri_config::theme::ThemeConfig;
use unicode_width::UnicodeWidthStr;

use super::builder::UiBuilder;
use super::types::{UiAction, UiComponent, UiContext, UiScene, UiTopBarHit};
use crate::app::top_bar::{PaneTabLayout, TopBarLayout};
use crate::app::{App, TopBarHoverRegion};

pub(crate) struct TopBarComponent {
    pub layout: TopBarLayout,
    session_text: String,
    workspace_label: String,
    mode_label: String,
    mode_color: [f32; 4],
    pane_tabs: Vec<PaneTabLayout>,
    hovered_region: Option<TopBarHoverRegion>,
    hovered_pane_tab: Option<u64>,
    is_leader: bool,
    is_broadcast: bool,
    is_overview: bool,
    tab_scroll: f32,
    tab_scroll_max: f32,
}

impl TopBarComponent {
    pub fn capture(app: &App, layout: TopBarLayout, cx: &UiContext<'_>) -> Self {
        let (mode_label, mode_color) = app.current_mode_label();
        let workspace_label = app.workspace_indicator_label();
        let pane_tabs = app.pane_tab_layouts(cx.cell_w, layout.tabs_area_px);
        Self {
            layout,
            session_text: format!(" {}  ", app.session_display_name()),
            workspace_label,
            mode_label,
            mode_color,
            pane_tabs,
            hovered_region: app.core.hovered_top_bar_region,
            hovered_pane_tab: app.core.hovered_pane_tab,
            is_leader: app.core.input.is_awaiting_action(),
            is_broadcast: app.core.broadcast_mode,
            is_overview: app.core.overview.active,
            tab_scroll: app.core.pane_tab_scroll,
            tab_scroll_max: app.pane_tab_scroll_max(),
        }
    }

    pub(super) fn hit_test(&self, mx: f32, my: f32, cx: &UiContext<'_>) -> Option<UiTopBarHit> {
        if my < self.layout.bar_y || my > self.layout.bar_y + self.layout.bar_height {
            return None;
        }
        if mx >= self.layout.session_x && mx <= self.layout.session_x + self.layout.session_w {
            return Some(UiTopBarHit::Session);
        }
        if self.layout.workspace_w > 0.0
            && mx >= self.layout.workspace_x
            && mx <= self.layout.workspace_x + self.layout.workspace_w
        {
            return Some(UiTopBarHit::Workspace);
        }
        if mx >= self.layout.mode_x && mx <= self.layout.mode_x + self.layout.mode_w {
            return Some(UiTopBarHit::Mode);
        }
        let tabs_start_x = self.layout.session_x + self.layout.session_w;
        let tabs_end_x = tabs_start_x + self.layout.tabs_area_px;
        for tab in &self.pane_tabs {
            let visible_left = tab.x.max(tabs_start_x);
            let visible_right = (tab.x + tab.w).min(tabs_end_x);
            if mx >= visible_left && mx <= visible_right {
                return Some(UiTopBarHit::PaneTab(tab.pane_id));
            }
        }
        let _ = cx;
        Some(UiTopBarHit::Background)
    }
}

impl UiComponent for TopBarComponent {
    fn click(&self, mx: f32, my: f32, cx: &UiContext<'_>) -> Option<UiAction> {
        match self.hit_test(mx, my, cx) {
            Some(UiTopBarHit::Session) => Some(UiAction::OpenSessionPalette),
            Some(UiTopBarHit::Workspace) => Some(UiAction::CycleWorkspace),
            Some(UiTopBarHit::Mode) => Some(UiAction::ToggleOverview),
            Some(UiTopBarHit::PaneTab(pane_id)) => Some(UiAction::FocusPaneTab(pane_id)),
            Some(UiTopBarHit::Background) | None => None,
        }
    }

    fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let padding = cx
            .config
            .statusbar
            .height_padding
            .unwrap_or(cx.cell_h * cx.config.statusbar.padding_ratio);
        let bar_height = cx.cell_h + padding;
        let text_y = self.layout.bar_y + padding * 0.5;
        let bar_bg = ThemeConfig::parse_color(&cx.config.theme.statusbar_background);
        let fg = ThemeConfig::parse_color(&cx.config.theme.foreground);
        let dim = ThemeConfig::parse_color(&cx.config.theme.statusbar_dim);
        let accent = ThemeConfig::parse_color(&cx.config.theme.accent);
        let broadcast_color = ThemeConfig::parse_color(&cx.config.theme.mode_broadcast);
        let sep_color = [dim[0], dim[1], dim[2], 0.25];
        let separator_color = [dim[0], dim[1], dim[2], 0.3];

        // Pre-compute right side width for three-zone split
        let mode_w = self.mode_label.chars().count() as f32 * cx.cell_w;
        let ws_w = if self.workspace_label.is_empty() {
            0.0
        } else {
            UnicodeWidthStr::width(self.workspace_label.as_str()) as f32 * cx.cell_w
        };
        let right_w = mode_w + ws_w;
        let session_w = self.layout.session_w;
        let tabs_area_w = (cx.viewport_w - session_w - right_w).max(0.0);

        let mut ui = UiBuilder::new_horizontal(
            0.0,
            text_y,
            cx.viewport_w,
            cx.cell_h,
            0.0,
            0.0,
            0.0,
            false,
            cx,
            scene,
        );

        // Bar background + separator (absolute decorations)
        ui.abs_rect(0.0, self.layout.bar_y, cx.viewport_w, bar_height, bar_bg);
        let sep_y = match cx.config.statusbar.position {
            StatusBarPosition::Top => self.layout.bar_y + bar_height - 1.0,
            StatusBarPosition::Bottom => self.layout.bar_y,
        };
        ui.abs_rect(0.0, sep_y, cx.viewport_w, 1.0, sep_color);

        // === Left zone: session name ===
        let session_color = if self.hovered_region == Some(TopBarHoverRegion::Session) {
            fg
        } else {
            dim
        };
        ui.label(&self.session_text, session_color);

        // === Middle zone: pane tabs (scrollable, clipped) ===
        let tabs_start_x = ui.cursor_pos().0;
        let tabs_end_x = tabs_start_x + tabs_area_w;
        let indicator_thickness = 1.5_f32;
        let indicator_y = match cx.config.statusbar.position {
            StatusBarPosition::Top => self.layout.bar_y + bar_height - indicator_thickness,
            StatusBarPosition::Bottom => self.layout.bar_y,
        };
        let separator_inset = bar_height * 0.2;

        // Reserve tab area in the layout (cursor advances past it)
        ui.bg_rect(tabs_area_w, cx.cell_h, [0.0; 4]);

        // Draw tabs with absolute positioning (they use scroll offset + clipping)
        for tab in &self.pane_tabs {
            let hovered = self.hovered_pane_tab == Some(tab.pane_id);
            let visible_left = tab.x.max(tabs_start_x);
            let visible_right = (tab.x + tab.w).min(tabs_end_x);
            let visible_w = (visible_right - visible_left).max(0.0);
            if visible_w <= 0.0 {
                continue;
            }

            // Separator
            if tab.x > tabs_start_x - 1.0 && tab.x < tabs_end_x {
                ui.abs_rect(
                    tab.x - 0.5,
                    self.layout.bar_y + separator_inset,
                    1.0,
                    bar_height - separator_inset * 2.0,
                    separator_color,
                );
            }
            // Active indicator
            if tab.active {
                ui.abs_rect(
                    visible_left,
                    indicator_y,
                    visible_w,
                    indicator_thickness,
                    accent,
                );
            }
            // Clipped label
            let color = if tab.active || hovered { fg } else { dim };
            if let Some((label, label_x)) = clip_tab_label(
                &tab.label,
                tab.x,
                tab.w,
                cx.cell_w,
                tabs_start_x,
                tabs_end_x,
            ) {
                ui.abs_text(&label, label_x, text_y, color);
            }
        }

        // Fade gradients
        let fade_w = (cx.cell_w * 3.0).min(tabs_area_w * 0.25);
        if fade_w > 0.0 {
            if self.tab_scroll > 0.5 {
                for i in 0..4 {
                    let alpha = 0.22 * (1.0 - i as f32 / 4.0);
                    ui.abs_rect(
                        tabs_start_x + i as f32 * (fade_w / 4.0),
                        self.layout.bar_y,
                        fade_w / 4.0 + 1.0,
                        bar_height,
                        [bar_bg[0], bar_bg[1], bar_bg[2], alpha],
                    );
                }
            }
            if self.tab_scroll < self.tab_scroll_max - 0.5 {
                for i in 0..4 {
                    let alpha = 0.22 * (i as f32 + 1.0) / 4.0;
                    ui.abs_rect(
                        tabs_end_x - fade_w + i as f32 * (fade_w / 4.0),
                        self.layout.bar_y,
                        fade_w / 4.0 + 1.0,
                        bar_height,
                        [bar_bg[0], bar_bg[1], bar_bg[2], alpha],
                    );
                }
            }
        }

        // === Right zone: workspace + mode ===
        if !self.workspace_label.is_empty() {
            let ws_color = if self.hovered_region == Some(TopBarHoverRegion::Workspace) {
                fg
            } else {
                accent
            };
            ui.label(&self.workspace_label, ws_color);
        }
        ui.label(&self.mode_label, self.mode_color);

        // Leader/broadcast/overview indicator strip (absolute, below/above bar)
        if self.is_leader || self.is_broadcast || self.is_overview {
            let indicator_h = cx.cell_h * cx.config.statusbar.leader_indicator_ratio;
            let indicator_color = if self.is_broadcast {
                broadcast_color
            } else {
                accent
            };
            let band_y = match cx.config.statusbar.position {
                StatusBarPosition::Top => self.layout.bar_y + bar_height,
                StatusBarPosition::Bottom => self.layout.bar_y - indicator_h,
            };
            ui.abs_rect(0.0, band_y, cx.viewport_w, indicator_h, indicator_color);
        }
    }
}

fn clip_tab_label(
    label: &str,
    tab_x: f32,
    tab_w: f32,
    cw: f32,
    tabs_start_x: f32,
    tabs_end_x: f32,
) -> Option<(String, f32)> {
    use unicode_width::UnicodeWidthChar;

    let visible_left = tab_x.max(tabs_start_x);
    let visible_right = (tab_x + tab_w).min(tabs_end_x);
    if visible_right <= visible_left {
        return None;
    }
    let skip_cols = ((visible_left - tab_x) / cw).floor().max(0.0) as usize;
    let visible_cols = ((visible_right - visible_left) / cw).floor().max(0.0) as usize;

    let mut col = 0usize;
    let mut clipped = String::new();
    let mut clip_start_col = skip_cols;
    for ch in label.chars() {
        let char_w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if col + char_w > skip_cols + visible_cols {
            break;
        }
        if col >= skip_cols {
            if clipped.is_empty() {
                // Record the actual column where we start clipping.
                // For wide chars straddling the boundary, this may be > skip_cols.
                clip_start_col = col;
            }
            clipped.push(ch);
        }
        col += char_w;
    }
    if clipped.is_empty() {
        None
    } else {
        // Shift draw position right if a wide char was partially skipped.
        let overhang = (clip_start_col - skip_cols) as f32 * cw;
        Some((clipped, visible_left + overhang))
    }
}
