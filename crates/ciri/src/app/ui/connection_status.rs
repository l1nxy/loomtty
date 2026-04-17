//! Connection-status banner.
//!
//! A centred, non-modal overlay that narrates the connection lifecycle so
//! the user isn't left staring at a blank window when DNS hangs or ssh
//! refuses. Three states drive the banner:
//!
//!   - **Connecting** — no connection yet, not halted, not retrying. Shown
//!     immediately after a connect attempt starts and stays up until the
//!     first `StateSync` frame arrives or the attempt fails.
//!   - **Reconnecting** — `reconnect_state` is set, transient failure in
//!     flight. Shows "Reconnecting… (N/M)" plus the last reason.
//!   - **Failed** — `is_halted()` is true (permanent reason, no retry
//!     scheduled). Shows the reason and a dismiss hint.

use ciri_config::theme::ThemeConfig;

use super::builder::UiBuilder;
use super::text_layout;
use super::tokens;
use super::types::{UiComponent, UiContext, UiScene};
use crate::app::App;

/// What the banner is saying right now. One snapshot per frame.
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

pub(crate) struct ConnectionStatusComponent {
    kind: StatusKind,
    /// Human label for the target of the connection ("user@host" for remote,
    /// `"session"` otherwise). Empty when we can't figure it out.
    target: String,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl ConnectionStatusComponent {
    pub fn capture(app: &App, cx: &UiContext<'_>) -> Option<Self> {
        // Visible whenever we're not currently connected AND we haven't just
        // shut down for an unrelated reason. If `connected == true` there's
        // nothing to say.
        if app.core.connected {
            return None;
        }
        // Don't compete with modal UI. The palette has its own footer line
        // for remote errors, so hiding the banner underneath it keeps a
        // single authoritative error surface while the palette is open.
        if app.core.command_palette.is_some() || app.core.pending_paste.is_some() {
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
            // IO thread is alive but we haven't seen the first StateSync yet.
            StatusKind::Connecting
        } else {
            // No channels, no reconnect state, not halted — nothing to show.
            return None;
        };

        let target = app
            .core
            .remote_config
            .as_ref()
            .map(|rc| rc.host.clone())
            .unwrap_or_else(|| app.core.session_name.clone());

        let (primary, secondary) = banner_lines(&kind, &target);
        let padding = cx.cell_w;
        let widest = text_layout::measure(cx, &primary)
            .max(text_layout::measure(cx, &secondary));
        let w = (widest + padding * 4.0).max(cx.cell_w * 28.0);
        let row_h = cx.cell_h + tokens::SPACE_1 * 2.0;
        let h = row_h * 2.0 + padding * 2.0;

        // Centre horizontally; sit a comfortable distance above the vertical
        // centre so the eye lands on it immediately without blocking panes.
        let x = (cx.viewport_w - w) * 0.5;
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
                "Connecting…".to_string()
            } else {
                format!("Connecting to {target}…")
            },
            "".to_string(),
        ),
        StatusKind::Reconnecting {
            attempt,
            max_attempts,
            last_reason,
        } => {
            // `attempt` is 0 before the first retry (during initial backoff)
            // and incremented by `bump_reconnect_attempt` right before each
            // call. Clamping to [1, max] gives "attempt N in progress"
            // semantics: 1/N in initial backoff and during the first retry,
            // 2/N during the second, and so on.
            let shown = (*attempt).max(1).min(*max_attempts);
            let head = format!("Reconnecting to {target}… ({shown}/{max_attempts})");
            let tail = match last_reason {
                Some(r) => format!("last error: {r}"),
                None => "".to_string(),
            };
            (head, tail)
        }
        StatusKind::Failed { reason } => (
            format!("Connection failed: {reason}"),
            "Press Esc to dismiss".to_string(),
        ),
    }
}

impl UiComponent for ConnectionStatusComponent {
    fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let bg = ThemeConfig::parse_color(&cx.config.theme.background);
        let fg = ThemeConfig::parse_color(&cx.config.theme.foreground);
        let accent = ThemeConfig::parse_color(&cx.config.theme.accent);
        let dim = ThemeConfig::parse_color(&cx.config.theme.statusbar_dim);
        let red = ThemeConfig::parse_color(&cx.config.theme.red);

        // The colour of the primary text and border hints at severity.
        let head_color = match &self.kind {
            StatusKind::Failed { .. } => red,
            StatusKind::Reconnecting { .. } => accent,
            StatusKind::Connecting => accent,
        };

        let bw = tokens::BORDER_THIN;
        let row_h = cx.cell_h + tokens::SPACE_1 * 2.0;

        let mut ui = UiBuilder::new_vertical(
            self.x + bw,
            self.y + bw,
            self.w - bw * 2.0,
            self.h - bw * 2.0,
            0.0,
            0.0,
            0.0,
            false,
            cx,
            scene,
        );

        let bg_color = [bg[0] * 0.85, bg[1] * 0.85, bg[2] * 0.85, 0.97];
        ui.bordered_panel_inset(self.x, self.y, self.w, self.h, bg_color, head_color, bw, true);

        let (primary, secondary) = banner_lines(&self.kind, &self.target);

        // Primary line: centred, in the severity colour.
        ui.horizontal(Some(self.w - bw * 2.0), row_h, 0.0, |ui| {
            let (rx, ry) = ui.cursor_pos();
            let text_y = ry + (row_h - cx.cell_h) * 0.5;
            let tw = ui.text_width(&primary);
            let tx = rx + ((self.w - bw * 2.0) - tw) * 0.5;
            ui.abs_text(&primary, tx, text_y, head_color);
        });

        ui.bg_rect(self.w - bw * 2.0, tokens::SPACE_1, [0.0; 4]);

        // Secondary line: dim/fg, same centring, may be blank.
        if !secondary.is_empty() {
            ui.horizontal(Some(self.w - bw * 2.0), row_h, 0.0, |ui| {
                let (rx, ry) = ui.cursor_pos();
                let text_y = ry + (row_h - cx.cell_h) * 0.5;
                let color = match &self.kind {
                    StatusKind::Failed { .. } => fg,
                    _ => dim,
                };
                let tw = ui.text_width(&secondary);
                let tx = rx + ((self.w - bw * 2.0) - tw) * 0.5;
                ui.abs_text(&secondary, tx, text_y, color);
            });
        } else {
            // Keep the height predictable whether or not we have a sub-line.
            ui.bg_rect(self.w - bw * 2.0, row_h, [0.0; 4]);
        }
    }
}
