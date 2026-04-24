//! Connection-status banner.
//!
//! A centred, non-modal overlay that narrates the connection lifecycle so the
//! user is not left staring at a blank window when DNS hangs or ssh refuses.

use super::text_layout;
use super::tokens;
use super::types::{UiContext, UiScene};
use crate::app::App;
use crate::app::ciri_ui_adapter::paint_ui_tree;
use ciri_ui::{Div, Layer, Styled, div, text};

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
        matches!(
            self,
            StatusKind::Connecting | StatusKind::Reconnecting { .. }
        )
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
                last_reason: app
                    .core
                    .last_disconnect_reason
                    .as_ref()
                    .map(|r| r.to_string()),
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

impl ConnectionStatusComponent {
    pub(crate) fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let root = self.build_tree(cx);
        paint_ui_tree(&root, cx, scene);
    }

    fn build_tree(&self, cx: &UiContext<'_>) -> Div {
        let bg = cx.theme.surface;
        let accent = cx.theme.accent;
        let dim = cx.theme.on_surface_muted;
        let red = cx.theme.error;

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
        // Sink below `bg` by a flat sRGB delta — matches the chrome
        // treatment in `context_menu.rs` and avoids the hue skew that a
        // raw `[c * k]` multiply introduces on non-neutral backgrounds.
        let sunk = tokens::surface_sink([bg[0], bg[1], bg[2], 1.0], tokens::SURFACE_SINK);
        let bg_color = [sunk[0], sunk[1], sunk[2], 0.97];

        let (primary, secondary) = banner_lines(&self.kind, &self.target);
        let animates = self.kind.animates();
        let dots = if animates {
            dot_suffix(dot_phase())
        } else {
            ""
        };

        let suffix_w = if animates {
            text_layout::measure(cx, "...")
        } else {
            0.0
        };
        let head = if animates {
            fit_without_ellipsis(cx, &primary, (content_w - suffix_w).max(0.0))
        } else {
            text_layout::truncate_with_ellipsis(cx, &primary, content_w)
        };
        let mut primary_row = div()
            .w_full()
            .h(row_h)
            .flex_row()
            .items_center()
            .justify_center()
            .child(text(head).color(head_color));
        if animates && !dots.is_empty() {
            primary_row = primary_row.child(text(dots).color(head_color));
        }

        let mut panel = div()
            .in_layer(Layer::Overlay)
            .absolute()
            .left(self.x)
            .top(self.y)
            .w(self.w)
            .h(self.h)
            .flex_col()
            .items_center()
            .bg(bg_color)
            .rounded(tokens::SPACE_1)
            .border(bw, head_color)
            .shadow_md()
            .child(div().w(content_w).h(v_pad))
            .child(primary_row);
        if !secondary.is_empty() {
            let line = text_layout::truncate_with_ellipsis(cx, &secondary, content_w);
            let color = match &self.kind {
                StatusKind::Failed { .. } => cx.theme.on_surface,
                _ => dim,
            };
            panel = panel.child(div().w(content_w).h(tokens::SPACE_1)).child(
                div()
                    .w_full()
                    .h(row_h)
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .child(text(line).color(color)),
            );
        }
        panel = panel.child(div().w(content_w).h(v_pad));

        div().w(cx.viewport_w).h(cx.viewport_h).child(panel)
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
        let theme = ciri_ui::ResolvedTheme::default();
        let cx = UiContext {
            config: &app.core.config,
            theme: &theme,
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
