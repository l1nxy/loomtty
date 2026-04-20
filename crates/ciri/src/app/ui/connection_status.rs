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

use ciri_ui::color::with_alpha;
use ciri_ui::{div, text, Div, Layer, ResolvedTheme, Styled};

use super::text_layout;
use super::tokens;
use super::types::UiContext;
use crate::app::App;

// `UiContext` is still consumed by `capture` for sizing; the paint path
// is the only thing that moved to `ciri-ui`.

/// Bouncing-dots animation cadence. 300ms/step × 4 frames = a full cycle in
/// ~1.2s; slow enough not to distract, fast enough to read as "working".
const DOT_PHASE_MS: u128 = 300;
const DOT_PHASES: u32 = 4;

/// Anchor for the banner's animation clock. Lazily set on first access so
/// the first visible phase is `0` regardless of how long the process has
/// been running before the banner appeared.
static ANIM_EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// Current dot-animation phase in `0..DOT_PHASES`. Same function is called
/// from `capture` (for the scene hash) and `paint` (for the rendered
/// string) so both agree in a single frame.
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

impl StatusKind {
    fn animates(&self) -> bool {
        matches!(self, StatusKind::Connecting | StatusKind::Reconnecting { .. })
    }
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
    /// Pre-measured width of the primary line *without* the animated
    /// ellipsis. Captured here so `build_tree` can reserve the suffix
    /// slot separately and the headline stays stationary while dots
    /// cycle through "", ".", "..", "...".
    primary_w: f32,
    /// Width of "..." in the current font — zero when the banner does
    /// not animate, so the reservation collapses for static states.
    suffix_w: f32,
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
        // Paste-dialog suppresses the banner for the same reason — its
        // 55% backdrop would let the banner ghost through.
        //
        // The context menu (now a ciri-ui widget on the `Modal` layer)
        // doesn't need suppression anymore: its panel is opaque and
        // z-order is handled by the scene's layer buckets, so a partial
        // overlap just leaves the banner visible outside the menu,
        // which is the desired behaviour.
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
        let has_secondary = !secondary.is_empty();
        // Measure with the animated line at its widest (3 dots) so the box
        // doesn't resize every 300ms as dots cycle.
        let primary_w = text_layout::measure(cx, &primary);
        let suffix_w = if kind.animates() {
            text_layout::measure(cx, "...")
        } else {
            0.0
        };
        let primary_block_w = primary_w + suffix_w;
        let secondary_w = text_layout::measure(cx, &secondary);
        let widest = primary_block_w.max(secondary_w);
        let side_pad = cx.cell_w * 2.0;
        let w = (widest + side_pad * 2.0).max(cx.cell_w * 28.0);
        // UI chrome uses the proportional UI shaper, not the terminal grid,
        // so row height must come from `ui_line_h` — `cell_h` leaves the
        // text unbalanced inside the row whenever the UI font differs from
        // the monospaced one.
        let row_h = cx.ui_line_h + tokens::SPACE_1 * 2.0;
        let v_pad = tokens::SPACE_2;
        let h = if has_secondary {
            v_pad * 2.0 + row_h * 2.0 + tokens::SPACE_1
        } else {
            v_pad * 2.0 + row_h
        };

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
            primary_w,
            suffix_w,
        })
    }
}

/// Primary + secondary lines *without* trailing ellipsis. Connecting /
/// Reconnecting render animated dots on top at paint time; keeping the
/// static "…" out of the measured text lets the box width stay stable.
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
            // `attempt` is 0 before the first retry (during initial backoff)
            // and incremented by `bump_reconnect_attempt` right before each
            // call. Clamping to [1, max] gives "attempt N in progress"
            // semantics: 1/N in initial backoff and during the first retry,
            // 2/N during the second, and so on.
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

impl ConnectionStatusComponent {
    /// Build the ciri-ui tree for this captured banner snapshot.
    ///
    /// The tree is a single flex-column panel positioned absolutely at
    /// the component's `(x, y)` via a paint-time `translate` on the
    /// root — Taffy lays it out at the origin, the paint walker adds
    /// the translate into the emitted SdfRect + glyph positions, and
    /// `in_layer(Overlay)` plants it above pane content but below
    /// modal UI (palette / context-menu / paste-dialog).
    pub(crate) fn build_tree(&self, theme: &ResolvedTheme) -> Div {
        let head_color = match &self.kind {
            StatusKind::Failed { .. } => theme.error,
            StatusKind::Reconnecting { .. } | StatusKind::Connecting => theme.accent,
        };
        let secondary_color = match &self.kind {
            StatusKind::Failed { .. } => theme.on_surface,
            _ => theme.on_surface_muted,
        };
        // Tint derived from the terminal `background`, not the UI
        // `surface`, so themes that set the two distinctly (e.g.
        // a darker pane bg with a lighter chrome bg) still get the
        // legacy "darkened pane colour with slight transparency"
        // recipe rather than a chrome-coloured overlay that no longer
        // matches the pane it covers.
        let panel_bg = with_alpha(ciri_ui::color::scale_rgb(theme.term_bg, 0.85), 0.97);

        let (primary, secondary) = banner_lines(&self.kind, &self.target);
        let animates = self.kind.animates();
        let dots = if animates { dot_suffix(dot_phase()) } else { "" };

        // Primary line: fix the combined block width to
        // `primary_w + suffix_w` so the column's `items_center`
        // centres the reserved block, not the currently-rendered
        // substring. Dots then grow inside the reserved slot without
        // shifting the headline every 300 ms. This restores the
        // stationary-centre behaviour the legacy imperative path had.
        let primary_block_w = self.primary_w + self.suffix_w;
        let primary_block = div()
            .flex_row()
            .w(primary_block_w)
            .child(text(primary).color(head_color))
            .child(text(dots).color(head_color));

        let mut panel = div()
            .in_layer(Layer::Overlay)
            .flex_col()
            .items_center()
            .w(self.w)
            .h(self.h)
            .translate(self.x, self.y)
            .bg(panel_bg)
            .border(tokens::BORDER_THIN, head_color)
            .rounded_md()
            .shadow_md()
            .px(tokens::SPACE_2)
            .py(tokens::SPACE_2)
            .gap(tokens::SPACE_1)
            .child(primary_block);

        if !secondary.is_empty() {
            panel = panel.child(text(secondary).color(secondary_color));
        }

        panel
    }
}

