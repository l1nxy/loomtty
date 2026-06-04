//! Connection-status banner.
//!
//! A centred, non-modal overlay that narrates the connection lifecycle so the
//! user is not left staring at a blank window when DNS hangs or ssh refuses.

use super::text_layout;
use super::tokens;
use super::types::{UiContext, UiScene};
use crate::app::App;
use crate::app::loom_ui_adapter::paint_element_tree;
use loom_ui::{Div, ElevationIndex, IntoElement, Render, RenderCtx, Styled, deferred, div, text};

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
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    /// Pre-fitted primary banner line (head + dot suffix already
    /// concatenated into the displayed string for non-animated kinds;
    /// animated kinds keep the head string and emit the dot suffix as
    /// a separate Text node so it can be coloured the same way).
    head: String,
    /// Suffix (dot string) for animated banners, frozen at the
    /// current `dot_phase` so `build_tree` doesn't recompute it.
    /// Empty for non-animated kinds.
    dots: &'static str,
    /// Pre-truncated secondary line — empty when there's no second
    /// row of text.
    tail: String,
    /// Cached value of `head_color` choice from the kind discriminant.
    /// Could be re-derived in `build_tree`, but keeping it here keeps
    /// the trait's render path entirely self-contained.
    has_secondary: bool,
}

impl ConnectionStatusComponent {
    pub fn capture(app: &App, cx: &UiContext<'_>) -> Option<Self> {
        if app.core.connected {
            return None;
        }
        if app.core.command_palette.is_some()
            || app.core.pending_paste.is_some()
            || app.core.context_menu.visible
            || app.core.settings_panel_visible
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

        // Pre-fit the displayed strings so `build_tree` doesn't need
        // the host shaper. Animation phase is frozen at the current
        // `dot_phase()` — the chrome cache hash already invalidates on
        // phase change (`render.rs:650`), so the captured value is
        // always fresh-per-frame.
        //
        // Pre-existing race window: this `dot_phase()` and the one in
        // `ui_scene_hash` are independent wall-clock reads. A 300 ms
        // phase boundary between them can produce a one-frame visual
        // lag (cached scene served with stale dots). Same behaviour as
        // the pre-migration code which read `dot_phase()` in
        // `build_tree` instead of `capture` — fix would require
        // snapshotting the phase at frame start and threading it to
        // both call sites.
        let bw = tokens::BORDER_THIN;
        let content_w = w - bw * 2.0;
        let animates = kind.animates();
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
        let tail = if has_secondary {
            text_layout::truncate_with_ellipsis(cx, &secondary, content_w)
        } else {
            String::new()
        };

        Some(Self {
            kind,
            x,
            y,
            w,
            h,
            head,
            dots,
            tail,
            has_secondary,
        })
    }

    fn render_cx<'a>(cx: &'a UiContext<'_>) -> RenderCtx<'a> {
        RenderCtx {
            theme: cx.theme,
            viewport: [cx.viewport_w, cx.viewport_h],
            scale: 1.0,
        }
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
    pub(crate) fn paint(&mut self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let render_cx = Self::render_cx(cx);
        // 7th production usage of `loom_ui::Render`. All text strings
        // (head, dots, tail) and dimensions are pre-baked in capture so
        // `build_tree` can run from a minimal `RenderCtx` without re-
        // borrowing the host shaper.
        let root = <Self as Render>::render(self, &render_cx).into_element();
        paint_element_tree(&root, cx, scene);
    }

    fn build_tree(&self, cx: &RenderCtx<'_>) -> Div {
        let accent = cx.theme.accent;
        let dim = cx.theme.on_surface_muted;
        let red = cx.theme.error;

        let head_color = match &self.kind {
            StatusKind::Failed { .. } => red,
            StatusKind::Reconnecting { .. } | StatusKind::Connecting => accent,
        };

        let bw = tokens::BORDER_THIN;
        // Row height was `cx.ui_line_h + SPACE_1 * 2`. We don't have
        // ui_line_h on `RenderCtx`, but the captured `self.h` already
        // encodes layout: `h = v_pad*2 + row_h * (1 or 2) + (SPACE_1
        // for has_secondary)`. Derive row_h back out so divs size the
        // same way the original layout did.
        let v_pad = tokens::SPACE_2;
        let row_h = if self.has_secondary {
            (self.h - v_pad * 2.0 - tokens::SPACE_1) * 0.5
        } else {
            self.h - v_pad * 2.0
        };
        let content_w = self.w - bw * 2.0;

        // Outer banner sits at the `Panel` elevation tier (sunk surface).
        // The `0.97` alpha preserves a tiny amount of pane bleed-through —
        // the banner is a transient overlay during reconnection so the
        // user can still read motion in the pane underneath.
        let panel = ElevationIndex::Panel.bg(cx.theme);
        let bg_color = [panel[0], panel[1], panel[2], 0.97];

        let mut primary_row = div()
            .w_full()
            .h(row_h)
            .flex_row()
            .items_center()
            .justify_center()
            .child(text(self.head.clone()).color(head_color));
        if !self.dots.is_empty() {
            primary_row = primary_row.child(text(self.dots).color(head_color));
        }

        let mut panel = div()
            .absolute()
            .left(self.x)
            .top(self.y)
            .w(self.w)
            .h(self.h)
            .flex_col()
            .items_center()
            .bg(bg_color)
            .rounded(cx.theme.radius.md)
            .border(bw, head_color)
            .shadow_md()
            .child(div().w(content_w).h(v_pad))
            .child(primary_row);
        if self.has_secondary {
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
                    .child(text(self.tail.clone()).color(color)),
            );
        }
        panel = panel.child(div().w(content_w).h(v_pad));

        // Drained via `deferred()` so the banner sits z-on-top of any
        // chrome the host appends after this widget'\''s scene.
        div()
            .w(cx.viewport[0])
            .h(cx.viewport[1])
            .child(deferred(panel))
    }
}

impl Render for ConnectionStatusComponent {
    fn render(&mut self, cx: &RenderCtx<'_>) -> impl IntoElement {
        self.build_tree(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui::types::test_ui_context;
    use loom_app::app::DisconnectReason;
    use loom_config::config::LoomConfig;

    fn make_app() -> App {
        App::new(LoomConfig::default(), "test-session")
    }

    #[test]
    fn failed_banner_is_clamped_inside_viewport() {
        let mut app = make_app();
        app.core.last_disconnect_reason = Some(DisconnectReason::SshSpawnFailed(
            "very long error message that should not push the banner off screen".into(),
        ));
        app.core.reconnect_state = None;
        let theme = loom_ui::ResolvedTheme::default();
        let cx = test_ui_context(&app.core.config, &theme, 220.0, 160.0);
        let banner = ConnectionStatusComponent::capture(&app, &cx).expect("banner visible");
        assert!(banner.x >= 0.0);
        assert!(banner.x + banner.w <= cx.viewport_w + 0.001);
    }
}
