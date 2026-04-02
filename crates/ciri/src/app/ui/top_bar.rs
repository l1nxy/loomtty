use ciri_config::config::StatusBarPosition;
use ciri_config::theme::ThemeConfig;
use ciri_render::rect::Rect;

use super::types::{UiAction, UiComponent, UiContext, UiScene, UiTopBarHit};
use crate::app::status_bar::{TextEmitParams, emit_status_text};
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
        let padding = if let Some(px) = cx.config.statusbar.height_padding {
            px
        } else {
            cx.cell_h * cx.config.statusbar.padding_ratio
        };
        let bar_height = cx.cell_h + padding;
        let text_y = self.layout.bar_y + padding * 0.5;
        let bar_bg = ThemeConfig::parse_color(&cx.config.theme.statusbar_background);
        let fg = ThemeConfig::parse_color(&cx.config.theme.foreground);
        let dim = ThemeConfig::parse_color(&cx.config.theme.statusbar_dim);
        let accent = ThemeConfig::parse_color(&cx.config.theme.accent);
        let broadcast_color = ThemeConfig::parse_color(&cx.config.theme.mode_broadcast);

        scene.bg_rects.push(Rect {
            x: 0.0,
            y: self.layout.bar_y,
            w: cx.viewport_w,
            h: bar_height,
            color: bar_bg,
        });

        let sep_y = match cx.config.statusbar.position {
            StatusBarPosition::Top => self.layout.bar_y + bar_height - 1.0,
            StatusBarPosition::Bottom => self.layout.bar_y,
        };
        scene.bg_rects.push(Rect {
            x: 0.0,
            y: sep_y,
            w: cx.viewport_w,
            h: 1.0,
            color: [dim[0], dim[1], dim[2], 0.25],
        });

        let session_color = if self.hovered_region == Some(TopBarHoverRegion::Session) {
            fg
        } else {
            dim
        };
        emit_status_text(
            scene.atlas,
            &self.session_text,
            &TextEmitParams {
                x_start: 0.0,
                y: text_y,
                cell_width: cx.cell_w,
                baseline: cx.baseline,
                color: session_color,
            },
            scene.glyphs,
        );

        if !self.workspace_label.is_empty() {
            emit_status_text(
                scene.atlas,
                &self.workspace_label,
                &TextEmitParams {
                    x_start: self.layout.workspace_x,
                    y: text_y,
                    cell_width: cx.cell_w,
                    baseline: cx.baseline,
                    color: accent,
                },
                scene.glyphs,
            );
        }

        let indicator_thickness = 1.5_f32;
        let tabs_start_x = self.layout.session_x + self.layout.session_w;
        let tabs_end_x = tabs_start_x + self.layout.tabs_area_px;
        let indicator_y = match cx.config.statusbar.position {
            StatusBarPosition::Top => self.layout.bar_y + bar_height - indicator_thickness,
            StatusBarPosition::Bottom => self.layout.bar_y,
        };

        let separator_w = 1.0_f32;
        let separator_color = [dim[0], dim[1], dim[2], 0.3];
        let separator_inset = bar_height * 0.2;

        for tab in &self.pane_tabs {
            let hovered = self.hovered_pane_tab == Some(tab.pane_id);
            let visible_left = tab.x.max(tabs_start_x);
            let visible_right = (tab.x + tab.w).min(tabs_end_x);
            let visible_w = (visible_right - visible_left).max(0.0);
            if visible_w <= 0.0 {
                continue;
            }

            let sep_x = tab.x;
            if sep_x > tabs_start_x - 1.0 && sep_x < tabs_end_x {
                scene.bg_rects.push(Rect {
                    x: sep_x - separator_w * 0.5,
                    y: self.layout.bar_y + separator_inset,
                    w: separator_w,
                    h: bar_height - separator_inset * 2.0,
                    color: separator_color,
                });
            }

            if tab.active {
                scene.bg_rects.push(Rect {
                    x: visible_left,
                    y: indicator_y,
                    w: visible_w,
                    h: indicator_thickness,
                    color: accent,
                });
            }

            let tab_text_color = if tab.active || hovered { fg } else { dim };
            if let Some((label, label_x)) = clip_tab_label(
                &tab.label,
                tab.x,
                tab.w,
                cx.cell_w,
                tabs_start_x,
                tabs_end_x,
            ) {
                emit_status_text(
                    scene.atlas,
                    &label,
                    &TextEmitParams {
                        x_start: label_x,
                        y: text_y,
                        cell_width: cx.cell_w,
                        baseline: cx.baseline,
                        color: tab_text_color,
                    },
                    scene.glyphs,
                );
            }
        }

        let fade_w = (cx.cell_w * 3.0).min(self.layout.tabs_area_px * 0.25);
        if fade_w > 0.0 {
            if self.tab_scroll > 0.5 {
                for i in 0..4 {
                    let alpha = 0.22 * (1.0 - i as f32 / 4.0);
                    let strip_w = fade_w / 4.0 + 1.0;
                    scene.bg_rects.push(Rect {
                        x: tabs_start_x + i as f32 * (fade_w / 4.0),
                        y: self.layout.bar_y,
                        w: strip_w,
                        h: bar_height,
                        color: [bar_bg[0], bar_bg[1], bar_bg[2], alpha],
                    });
                }
            }
            if self.tab_scroll < self.tab_scroll_max - 0.5 {
                for i in 0..4 {
                    let alpha = 0.22 * (i as f32 + 1.0) / 4.0;
                    let strip_w = fade_w / 4.0 + 1.0;
                    scene.bg_rects.push(Rect {
                        x: tabs_end_x - fade_w + i as f32 * (fade_w / 4.0),
                        y: self.layout.bar_y,
                        w: strip_w,
                        h: bar_height,
                        color: [bar_bg[0], bar_bg[1], bar_bg[2], alpha],
                    });
                }
            }
        }

        let mode_str = self.mode_label.as_str();
        let mode_chars = mode_str.chars().count();
        let rx = cx.viewport_w - mode_chars as f32 * cx.cell_w;
        emit_status_text(
            scene.atlas,
            mode_str,
            &TextEmitParams {
                x_start: rx,
                y: text_y,
                cell_width: cx.cell_w,
                baseline: cx.baseline,
                color: self.mode_color,
            },
            scene.glyphs,
        );

        if self.is_leader || self.is_broadcast || self.is_overview {
            let indicator_h = cx.cell_h * cx.config.statusbar.leader_indicator_ratio;
            let indicator_color = if self.is_broadcast {
                broadcast_color
            } else {
                accent
            };
            scene.bg_rects.push(Rect {
                x: 0.0,
                y: match cx.config.statusbar.position {
                    StatusBarPosition::Top => self.layout.bar_y + bar_height,
                    StatusBarPosition::Bottom => self.layout.bar_y - indicator_h,
                },
                w: cx.viewport_w,
                h: indicator_h,
                color: indicator_color,
            });
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
    for ch in label.chars() {
        let char_w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if col + char_w > skip_cols + visible_cols {
            break;
        }
        if col >= skip_cols {
            clipped.push(ch);
        }
        col += char_w;
    }
    if clipped.is_empty() {
        None
    } else {
        Some((clipped, visible_left))
    }
}
