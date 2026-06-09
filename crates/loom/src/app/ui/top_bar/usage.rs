//! Usage segment renderer — Claude Code + Codex OAuth probe results.

use super::super::tokens::SEGMENT_PAD_X;
use super::super::types::UiRect;
use crate::app::usage::UsageSnapshot;
use loom_ui::{Color, Div, Styled, div, text};

pub(super) struct UsageSegment<'a> {
    pub(super) snapshot: &'a UsageSnapshot,
}

impl<'a> UsageSegment<'a> {
    /// Same flat-rectangle geometry as the workspace indicator: a
    /// `surface_elevated` block, accent text. Display-only — no hit_id.
    pub(super) fn into_div(
        self,
        slot: UiRect,
        accent: Color,
        surface_elevated: Color,
    ) -> Div {
        if slot.is_empty() {
            return div();
        }
        div()
            .absolute()
            .left(slot.x)
            .top(slot.y)
            .w(slot.w)
            .h(slot.h)
            .bg(surface_elevated)
            .text_color(accent)
            .flex_row()
            .items_center()
            .pl(SEGMENT_PAD_X)
            .pr(SEGMENT_PAD_X)
            .child(text(self.label()))
    }

    /// `claude 12% / 41%  •  codex 7% / 23%` style. Missing data shows a
    /// dim placeholder so the layout doesn't jump width when a probe is
    /// in flight. Snapshot fields are 0..1 fractions; multiply by 100
    /// for the percentage display. Only used as a fallback when the
    /// Lua plugin engine failed to start.
    pub(super) fn label(&self) -> String {
        let pct = |frac: Option<f64>| frac.map(|v| format!("{:.0}%", v * 100.0));
        let mut parts: Vec<String> = Vec::with_capacity(2);
        if let Some(c) = &self.snapshot.claude.data {
            let s = pct(c.session.utilization);
            let w = pct(c.weekly.utilization);
            match (s, w) {
                (Some(s), Some(w)) => parts.push(format!("claude {s}/{w}")),
                (Some(s), None) => parts.push(format!("claude {s}")),
                (None, Some(w)) => parts.push(format!("claude wk {w}")),
                (None, None) => parts.push("claude --".into()),
            }
        } else if self.snapshot.claude.last_error.is_some() {
            parts.push("claude !".into());
        }
        if let Some(c) = &self.snapshot.codex.data {
            let p = pct(c.primary.utilization);
            let s = pct(c.secondary.utilization);
            match (p, s) {
                (Some(p), Some(s)) => parts.push(format!("codex {p}/{s}")),
                (Some(p), None) => parts.push(format!("codex {p}")),
                (None, Some(s)) => parts.push(format!("codex 2 {s}")),
                (None, None) => parts.push("codex --".into()),
            }
        } else if self.snapshot.codex.last_error.is_some() {
            parts.push("codex !".into());
        }
        if parts.is_empty() {
            "usage --".into()
        } else {
            parts.join("  •  ")
        }
    }
}

/// Just-the-string renderer for tests + plugin formatting fallback. Kept
/// public to the module so `format-status-bar` can call it without
/// instantiating the full segment.
pub(crate) fn format_label(snapshot: &UsageSnapshot) -> String {
    UsageSegment { snapshot }.label()
}
