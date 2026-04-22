//! Connection-status banner.
//!
//! A centred, non-modal overlay that narrates the connection lifecycle so the
//! user is not left staring at a blank window when DNS hangs or ssh refuses.

use ciri_config::theme::ThemeConfig;

use super::builder::UiBuilder;
use super::text_layout;
use super::tokens;
use super::types::{UiComponent, UiContext, UiScene};
use crate::app::App;

const DOT_PHASE_MS: u128 = 300;
const DOT_PHASES: u32 = 4;

static ANIM_EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

pub(crate) fn dot_phase() -> u32 {
    let epoch = ANIM_EPOCH.get_or_init(std::time::Instant::now);
    ((epoch.elapsed().as_millis() / DOT_PHASE_MS) as u32) % DOT_PHASES
}

fn dot_suffix(phase: u32) -> &'static str {
    match phase % DOT_PHASES {
        0 => "",
        1 => ".",
        2 => "..",
        _ => "...",
    }
}

pub(crate) enum StatusKind {
    Connecting,
    Reconnecting {
        attempt: u32,
        max_attempts: u32,
        last_reason: Option<String>,
    },
    Failed {
        reason: String,
    },
}

impl StatusKind {
    fn animates(&self) -> bool {
        matches!(self, StatusKind::Connecting | StatusKind::Reconnecting { .. })
    }
}

pub(crate) struct ConnectionStatusComponent {
    kind: StatusKind,
    target: String,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl ConnectionStatusComponent {
    pub fn capture(app: &App, cx: &UiContext<'_>) -> Option<Self> {
        if app.core.connected {
            return None;
        }
        if app.core.command_palette.is_some()
            || app.core.pending_paste.is_some()
            || app.core.context_menu.visible
        {
            return None;
        }

        let kind = if let Some(state) = &app.core.reconnect_state {
            StatusKind::Reconnecting {
                attempt: state.attempt,
                max_attempts: state.max_attempts,
                last_reason: app.core.last_disconnect_reason.as_ref().map(|r| r.to_string()),
            }
        } else if app.core.is_halted() {
            let reason = app
                .core
                .last_disconnect_reason
                .as_ref()
                .map(|r| r.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            StatusKind::Failed { reason }
        } else if app.core.server_rx.is_some() || app.core.server_tx.is_some() {
            StatusKind::Connecting
        } else {
            return None;
        };

        let target = app
            .core
            .remote_config
            .as_ref()
            .map(|rc| rc.host.clone())
            .unwrap_or_else(|| app.core.session_name.clone());

        let (primary, secondary) = banner_lines(&kind, &target);
        let has_secondary = !secondary.is_empty();
        let primary_w = text_layout::measure(cx, &primary)
            + if kind.animates() {
                text_layout::measure(cx, "...")
            } else {
                0.0
            };
        let secondary_w = text_layout::measure(cx, &secondary);
        let widest = primary_w.max(secondary_w);
        let side_pad = cx.cell_w * 2.0;
        let row_h = cx.ui_line_h + tokens::SPACE_1 * 2.0;
        let v_pad = tokens::SPACE_2;
        let h = if has_secondary {
            v_pad * 2.0 + row_h * 2.0 + tokens::SPACE_1
        } else {
            v_pad * 2.0 + row_h
        };
        let max_w = (cx.viewport_w - tokens::SPACE_4 * 2.0).max(cx.cell_w * 12.0);
        let w = ((widest + side_pad * 2.0).max(cx.cell_w * 28.0)).min(max_w);
        let x = ((cx.viewport_w - w) * 0.5).max(tokens::SPACE_2);
        let y = (cx.viewport_h * 0.35 - h * 0.5).max(tokens::SPACE_2);

        Some(Self {
            kind,
            target,
            x,
            y,
            w,
            h,
        })
    }
}

fn banner_lines(kind: &StatusKind, target: &str) -> (String, String) {
    match kind {
        StatusKind::Connecting => (
            if target.is_empty() {
                "Connecting".to_string()
            } else {
                format!("Connecting to {target}")
            },
            String::new(),
        ),
        StatusKind::Reconnecting {
            attempt,
            max_attempts,
            last_reason,
        } => {
            let shown = (*attempt).max(1).min(*max_attempts);
            let head = format!("Reconnecting to {target} ({shown}/{max_attempts})");
            let tail = match last_reason {
                Some(r) => format!("last error: {r}"),
                None => String::new(),
            };
            (head, tail)
        }
        StatusKind::Failed { reason } => (
            format!("Connection failed: {reason}"),
            "Press Esc to dismiss".to_string(),
        ),
    }
}

fn fit_without_ellipsis(cx: &UiContext<'_>, text: &str, max_w: f32) -> String {
    if text_layout::measure(cx, text) <= max_w {
        return text.to_string();
    }
    let (cut, _) = text_layout::prefix_fit(cx, text, max_w.max(0.0));
    text[..text.floor_char_boundary(cut.min(text.len()))].to_string()
}

impl UiComponent for ConnectionStatusComponent {
    fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let bg = ThemeConfig::parse_color(&cx.config.theme.background);
        let fg = ThemeConfig::parse_color(&cx.config.theme.foreground);
        let accent = ThemeConfig::parse_color(&cx.config.theme.accent);
        let dim = ThemeConfig::parse_color(&cx.config.theme.statusbar_dim);
        let red = ThemeConfig::parse_color(&cx.config.theme.red);

        let head_color = match &self.kind {
            StatusKind::Failed { .. } => red,
            StatusKind::Reconnecting { .. } | StatusKind::Connecting => accent,
        };

        let bw = tokens::BORDER_THIN;
        let row_h = cx.ui_line_h + tokens::SPACE_1 * 2.0;
        let v_pad = tokens::SPACE_2;
        let content_w = self.w - bw * 2.0;

        // Outer banner via SDF: rounded + colored border (red on Failed,
        // accent while reconnecting/connecting) + drop shadow.
        let bg_color = [bg[0] * 0.85, bg[1] * 0.85, bg[2] * 0.85, 0.97];
        scene.sdf_rects.push(ciri_render::sdf_rect::SdfRect {
            pos: [self.x, self.y],
            size: [self.w, self.h],
            color: bg_color,
            radii: [tokens::SPACE_1; 4],
            border_color: head_color,
            border_width: bw,
            shadow_blur: tokens::SPACE_2,
            shadow_offset: [0.0, tokens::SPACE_1],
            shadow_color: [0.0, 0.0, 0.0, 0.30],
        });

        let mut ui = UiBuilder::new_vertical(
            self.x + bw,
            self.y + bw,
            content_w,
            self.h - bw * 2.0,
            0.0,
            0.0,
            0.0,
            false,
            cx,
            scene,
        );

        let (primary, secondary) = banner_lines(&self.kind, &self.target);
        let animates = self.kind.animates();
        let dots = if animates { dot_suffix(dot_phase()) } else { "" };

        ui.bg_rect(content_w, v_pad, [0.0; 4]);
        ui.horizontal(Some(content_w), row_h, 0.0, |ui| {
            let (rx, ry) = ui.cursor_pos();
            let text_y = ry + (row_h - cx.ui_line_h) * 0.5;
            let suffix_w = if animates { ui.text_width("...") } else { 0.0 };
            let head = if animates {
                fit_without_ellipsis(cx, &primary, (content_w - suffix_w).max(0.0))
            } else {
                text_layout::truncate_with_ellipsis(cx, &primary, content_w)
            };
            let head_w = ui.text_width(&head);
            let tx = rx + (content_w - head_w - suffix_w) * 0.5;
            ui.abs_text(&head, tx, text_y, head_color);
            if animates && !dots.is_empty() {
                ui.abs_text(dots, tx + head_w, text_y, head_color);
            }
        });

        if !secondary.is_empty() {
            ui.bg_rect(content_w, tokens::SPACE_1, [0.0; 4]);
            ui.horizontal(Some(content_w), row_h, 0.0, |ui| {
                let (rx, ry) = ui.cursor_pos();
                let text_y = ry + (row_h - cx.ui_line_h) * 0.5;
                let line = text_layout::truncate_with_ellipsis(cx, &secondary, content_w);
                let tw = ui.text_width(&line);
                let tx = rx + (content_w - tw) * 0.5;
                let color = match &self.kind {
                    StatusKind::Failed { .. } => fg,
                    _ => dim,
                };
                ui.abs_text(&line, tx, text_y, color);
            });
        }

        ui.bg_rect(content_w, v_pad, [0.0; 4]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ciri_app::app::DisconnectReason;
    use ciri_config::config::CiriConfig;

    fn make_app() -> App {
        App::new(CiriConfig::default(), "test-session")
    }

    #[test]
    fn failed_banner_is_clamped_inside_viewport() {
        let mut app = make_app();
        app.core.last_disconnect_reason = Some(DisconnectReason::SshSpawnFailed(
            "very long error message that should not push the banner off screen".into(),
        ));
        app.core.reconnect_state = None;
        let cx = UiContext {
            config: &app.core.config,
            viewport_w: 220.0,
            viewport_h: 160.0,
            cell_w: 8.0,
            cell_h: 16.0,
            baseline: 12.0,
            ui_line_h: 16.0,
            ui_shaper: None,
        };
        let banner = ConnectionStatusComponent::capture(&app, &cx).expect("banner visible");
        assert!(banner.x >= 0.0);
        assert!(banner.x + banner.w <= cx.viewport_w + 0.001);
    }
}
