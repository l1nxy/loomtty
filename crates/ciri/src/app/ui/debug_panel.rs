//! "Gotta Go Fast"-style debug overlay — per-phase frame timings, input
//! event timings, repaint ratio, and last-frame upload size.
//!
//! Built on the same `ciri-ui` element-tree pattern as `info_box.rs`:
//! `capture()` snapshots the App's debug metrics into a small POD struct,
//! and `Render::render` builds a Div tree that the host paints via
//! `paint_element_tree`. Visibility is gated on `App.debug_metrics.enabled`,
//! so when off this entire path is skipped at `capture()` time.

use ciri_config::config::StatusBarPosition;

use super::text_layout;
use super::tokens;
use super::types::{UiContext, UiScene};
use crate::app::App;
use crate::app::ciri_ui_adapter::paint_element_tree;
use crate::app::debug_metrics::Stats;
use ciri_ui::{Div, IntoElement, Render, RenderCtx, Styled, deferred, div, text};

const TITLE: &str = "Gotta Go Fast";
const COL_HEADERS: [&str; 3] = ["Last", "P95", "P99"];

#[derive(Clone, Copy, Debug)]
enum Severity {
    Ok,
    Warn,
    Bad,
}

#[derive(Clone)]
struct PanelRow {
    /// Left-column label ("Key", "Build", …).
    label: &'static str,
    /// Already-formatted column strings (Last / P95 / P99) — formatting
    /// happens at capture time so the render pass is purely layout.
    values: [String; 3],
    severity: [Severity; 3],
}

pub(crate) struct DebugPanelComponent {
    rows: Vec<PanelRow>,
    /// Section breaks split rows visually (input / pipeline / frame).
    /// Stored as the row index *before* which to insert a thin gap row.
    section_breaks: Vec<usize>,
    /// Geometry — frozen at capture so `Render::render` builds its tree
    /// without touching the host shaper.
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    cell_w: f32,
    cell_h: f32,
    /// Pre-measured column widths so right-aligned text in each
    /// column lines up across rows under a proportional UI font.
    label_col_w: f32,
    value_col_w: f32,
}

/// How a timing metric should be graded against the frame budget.
///
/// Vsync-bounded rows include the swapchain present / GPU fence wait
/// — at 60Hz that's ≈16.7ms of unavoidable wall time, so the threshold
/// is forgiving (warn near the budget, bad above it).
///
/// CPU-only rows are pure work the renderer does itself; they should
/// stay well under a fraction of the budget so the rest of the pipeline
/// has headroom — hence the stricter 25% / 50% split.
#[derive(Clone, Copy)]
enum Grading {
    CpuOnly,
    VsyncBounded,
}

impl DebugPanelComponent {
    pub fn capture(app: &App, cx: &UiContext<'_>) -> Option<Self> {
        if !app.debug_metrics.enabled {
            return None;
        }
        let snap = app.debug_metrics.snapshot();
        let budget_ms = (app.core.frame_interval.as_secs_f32() * 1000.0).max(1.0);
        let mut rows = Vec::with_capacity(11);
        rows.push(timing_row("Key", snap.key, budget_ms, Grading::CpuOnly));
        rows.push(timing_row("Mouse", snap.mouse, budget_ms, Grading::CpuOnly));
        rows.push(timing_row("Build", snap.build, budget_ms, Grading::CpuOnly));
        rows.push(timing_row(
            "Layout",
            snap.layout,
            budget_ms,
            Grading::CpuOnly,
        ));
        rows.push(timing_row("Paint", snap.paint, budget_ms, Grading::CpuOnly));
        rows.push(timing_row(
            "Render",
            snap.render,
            budget_ms,
            Grading::VsyncBounded,
        ));
        rows.push(timing_row(
            "Frame",
            snap.frame,
            budget_ms,
            Grading::VsyncBounded,
        ));
        rows.push(repaint_row(snap.repaint_ratio));
        rows.push(bytes_row(snap.bytes));

        // Sections: input rows {0,1}, pipeline rows {2..6}, frame rows {7..}.
        let section_breaks = vec![2, 7];

        let label_col_w = rows
            .iter()
            .map(|r| text_layout::measure(cx, r.label))
            .fold(0.0_f32, f32::max);
        let value_col_w = {
            let header_max = COL_HEADERS
                .iter()
                .map(|h| text_layout::measure(cx, h))
                .fold(0.0_f32, f32::max);
            let cell_max = rows
                .iter()
                .flat_map(|r| r.values.iter())
                .map(|s| text_layout::measure(cx, s))
                .fold(0.0_f32, f32::max);
            header_max.max(cell_max)
        };
        // Title isn't in any column — measure separately for box-width.
        let title_w = text_layout::measure(cx, TITLE);

        let pad = cx.cell_w;
        let gap = cx.cell_w; // gap between label col and first value col, also between value cols
        let row_h = cx.cell_h + tokens::SPACE_1 * 2.0;
        let title_h = cx.cell_h + tokens::SPACE_1 * 2.0;
        let break_h = tokens::SPACE_2;
        let content_w = label_col_w + gap + value_col_w * 3.0 + gap * 2.0;
        // Title row needs at least its own measured width plus 2-cell side padding.
        let title_min_w = title_w + cx.cell_w * 4.0;
        let inner_w = content_w.max(title_min_w);
        let w = inner_w + pad * 2.0;

        // +1 row for the column-header line ("Last  P95  P99").
        let row_count = rows.len() as f32 + 1.0;
        let breaks_h = break_h * section_breaks.len() as f32;
        let h = title_h + pad + row_count * row_h + breaks_h + pad;

        let margin = tokens::SPACE_2;
        let x = (cx.viewport_w - w - margin).max(margin);
        // Anchor below the top bar when the bar is at the top so the
        // overlay never collides with the session/workspace chrome.
        // When the bar is at the bottom, tuck against the top edge.
        let top_bar_h = app
            .top_bar_layout(
                cx.viewport_w,
                cx.viewport_h,
                cx.cell_w,
                cx.cell_h,
                cx.ui_shaper,
            )
            .bar_height;
        let y = match cx.config.statusbar.position {
            StatusBarPosition::Top => top_bar_h + margin,
            StatusBarPosition::Bottom => margin,
        };

        Some(Self {
            rows,
            section_breaks,
            x,
            y,
            w,
            h,
            cell_w: cx.cell_w,
            cell_h: cx.cell_h,
            label_col_w,
            value_col_w,
        })
    }

    fn render_cx<'a>(cx: &'a UiContext<'_>) -> RenderCtx<'a> {
        RenderCtx {
            theme: cx.theme,
            viewport: [cx.viewport_w, cx.viewport_h],
            scale: 1.0,
        }
    }

    pub(crate) fn paint(&mut self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let render_cx = Self::render_cx(cx);
        let root = <Self as Render>::render(self, &render_cx).into_element();
        paint_element_tree(&root, cx, scene);
    }

    fn build_tree(&self, cx: &RenderCtx<'_>) -> Div {
        let bg_base = cx.theme.surface;
        let accent = cx.theme.accent;
        let fg = cx.theme.on_surface;
        let dim = cx.theme.on_surface_muted;

        let sunk = tokens::surface_sink(
            [bg_base[0], bg_base[1], bg_base[2], 1.0],
            tokens::SURFACE_SINK,
        );
        let bg = [sunk[0], sunk[1], sunk[2], 0.92];

        let bw = tokens::BORDER_THIN;
        let pad = self.cell_w;
        let gap = self.cell_w;
        let row_h = self.cell_h + tokens::SPACE_1 * 2.0;
        let title_h = self.cell_h + tokens::SPACE_1 * 2.0;
        let break_h = tokens::SPACE_2;
        let content_w = self.w - bw * 2.0;
        let inner_w = content_w - pad * 2.0;

        let mut panel = div()
            .absolute()
            .left(self.x)
            .top(self.y)
            .w(self.w)
            .h(self.h)
            .flex_col()
            .items_center()
            .bg(bg)
            .rounded(cx.theme.radius.md)
            .border(bw, accent)
            .shadow_md();

        // Title row — accent-tinted strip with centered title.
        panel = panel.child(
            div()
                .w(content_w)
                .h(title_h)
                .flex_row()
                .items_center()
                .justify_center()
                .bg(tokens::tint(accent, tokens::ALPHA_TINT_HEADER))
                .child(text(TITLE.to_string()).color(accent)),
        );
        panel = panel.child(div().w(content_w).h(pad));

        // Column header row.
        panel = panel.child(self.row_frame(content_w, pad, inner_w, row_h, gap, |row| {
            row.child(div().w(self.label_col_w).h(row_h).flex_row().items_center())
                .child(div().w(gap).h(row_h))
                .child(self.value_cell(row_h, COL_HEADERS[0], dim))
                .child(div().w(gap).h(row_h))
                .child(self.value_cell(row_h, COL_HEADERS[1], dim))
                .child(div().w(gap).h(row_h))
                .child(self.value_cell(row_h, COL_HEADERS[2], dim))
        }));

        for (i, r) in self.rows.iter().enumerate() {
            if self.section_breaks.contains(&i) {
                panel = panel.child(div().w(content_w).h(break_h));
            }
            let label = r.label;
            let values = r.values.clone();
            let severities = r.severity;
            panel = panel.child(self.row_frame(content_w, pad, inner_w, row_h, gap, |row| {
                row.child(
                    div()
                        .w(self.label_col_w)
                        .h(row_h)
                        .flex_row()
                        .items_center()
                        .justify_end()
                        .child(text(label.to_string()).color(dim)),
                )
                .child(div().w(gap).h(row_h))
                .child(self.value_cell(row_h, &values[0], color_for(cx, fg, severities[0])))
                .child(div().w(gap).h(row_h))
                .child(self.value_cell(row_h, &values[1], color_for(cx, fg, severities[1])))
                .child(div().w(gap).h(row_h))
                .child(self.value_cell(
                    row_h,
                    &values[2],
                    color_for(cx, fg, severities[2]),
                ))
            }));
        }

        div()
            .w(cx.viewport[0])
            .h(cx.viewport[1])
            .child(deferred(panel))
    }

    fn row_frame(
        &self,
        content_w: f32,
        pad: f32,
        _inner_w: f32,
        row_h: f32,
        _gap: f32,
        body: impl FnOnce(Div) -> Div,
    ) -> Div {
        let row = div().w(content_w).h(row_h).flex_row().items_center();
        let row = row.child(div().w(pad).h(row_h));
        body(row)
    }

    fn value_cell(&self, row_h: f32, s: &str, color: [f32; 4]) -> Div {
        div()
            .w(self.value_col_w)
            .h(row_h)
            .flex_row()
            .items_center()
            .justify_end()
            .child(text(s.to_string()).color(color))
    }
}

impl Render for DebugPanelComponent {
    fn render(&mut self, cx: &RenderCtx<'_>) -> impl IntoElement {
        self.build_tree(cx)
    }
}

fn color_for(cx: &RenderCtx<'_>, fg: [f32; 4], sev: Severity) -> [f32; 4] {
    match sev {
        Severity::Ok => fg,
        Severity::Warn => cx.theme.warning,
        Severity::Bad => cx.theme.error,
    }
}

/// Severity for a timing relative to the configured frame budget.
///
/// CpuOnly rows are real CPU work — they should leave headroom, so the
/// thresholds are a fraction of the budget (25% / 50%).
///
/// VsyncBounded rows (Render, Frame) include swapchain present / fence
/// wait that's pinned to the refresh rate; at 60Hz that's ≈16.7ms of
/// unavoidable wall time. Grading those at a CpuOnly cutoff would paint
/// the panel red on a perfectly healthy app — so the threshold is the
/// budget itself, with red reserved for "actually missing the budget".
fn severity_ms(ms: f32, budget_ms: f32, grading: Grading) -> Severity {
    let (warn, bad) = match grading {
        Grading::CpuOnly => (budget_ms * 0.25, budget_ms * 0.50),
        Grading::VsyncBounded => (budget_ms * 0.95, budget_ms * 1.10),
    };
    if ms < warn {
        Severity::Ok
    } else if ms < bad {
        Severity::Warn
    } else {
        Severity::Bad
    }
}

fn fmt_ms(ms: f32) -> String {
    if ms >= 100.0 {
        format!("{:.0}", ms)
    } else if ms >= 10.0 {
        format!("{:.1}", ms)
    } else {
        format!("{:.2}", ms)
    }
}

fn timing_row(label: &'static str, s: Stats, budget_ms: f32, grading: Grading) -> PanelRow {
    PanelRow {
        label,
        values: [fmt_ms(s.last), fmt_ms(s.p95), fmt_ms(s.p99)],
        severity: [
            severity_ms(s.last, budget_ms, grading),
            severity_ms(s.p95, budget_ms, grading),
            severity_ms(s.p99, budget_ms, grading),
        ],
    }
}

fn fmt_pct(frac: f32) -> String {
    let pct = frac * 100.0;
    if pct >= 100.0 {
        "100%".to_string()
    } else if pct >= 10.0 {
        format!("{:.1}%", pct)
    } else {
        format!("{:.2}%", pct)
    }
}

/// Repaint ratio is shown only in the "Last" column — there's no rolling
/// window per-percentile (it's a counter ratio, not a sample). The other
/// two columns repeat the same value so the table stays grid-aligned.
fn repaint_row(frac: f32) -> PanelRow {
    let s = fmt_pct(frac);
    let sev = if frac < 0.1 {
        Severity::Ok
    } else if frac < 0.5 {
        Severity::Warn
    } else {
        Severity::Bad
    };
    PanelRow {
        label: "Repaint",
        values: [s.clone(), "—".to_string(), "—".to_string()],
        severity: [sev, Severity::Ok, Severity::Ok],
    }
}

fn fmt_bytes(b: f32) -> String {
    let b = b.max(0.0);
    if b >= 1_048_576.0 {
        format!("{:.1}M", b / 1_048_576.0)
    } else if b >= 1024.0 {
        format!("{:.1}k", b / 1024.0)
    } else {
        format!("{:.0}", b)
    }
}

fn bytes_row(s: Stats) -> PanelRow {
    PanelRow {
        label: "Bytes",
        values: [fmt_bytes(s.last), fmt_bytes(s.p95), fmt_bytes(s.p99)],
        // Bytes don't have an obvious budget — let the user decide what's "bad".
        severity: [Severity::Ok, Severity::Ok, Severity::Ok],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_ms_picks_precision_by_magnitude() {
        assert_eq!(fmt_ms(0.39), "0.39");
        assert_eq!(fmt_ms(4.08), "4.08");
        assert_eq!(fmt_ms(35.31), "35.3");
        assert_eq!(fmt_ms(150.0), "150");
    }

    #[test]
    fn fmt_bytes_human_readable() {
        assert_eq!(fmt_bytes(248.0), "248");
        assert_eq!(fmt_bytes(4096.0), "4.0k");
        assert_eq!(fmt_bytes(2_500_000.0), "2.4M");
    }

    #[test]
    fn fmt_pct_low_high() {
        assert_eq!(fmt_pct(0.005), "0.50%");
        assert_eq!(fmt_pct(0.446), "44.6%");
        assert_eq!(fmt_pct(1.0), "100%");
    }

    #[test]
    fn severity_cpu_only_is_strict() {
        let budget = 16.0;
        assert!(matches!(
            severity_ms(0.5, budget, Grading::CpuOnly),
            Severity::Ok
        ));
        // 25% of 16ms = 4ms — past warn but under 50% (8ms).
        assert!(matches!(
            severity_ms(5.0, budget, Grading::CpuOnly),
            Severity::Warn
        ));
        assert!(matches!(
            severity_ms(10.0, budget, Grading::CpuOnly),
            Severity::Bad
        ));
    }

    #[test]
    fn severity_vsync_bounded_tolerates_full_budget() {
        let budget = 16.0;
        // Sitting at vsync (≈16ms) on a healthy 60Hz app must NOT be red.
        // It lands in the warn band (≥95% budget, <110%).
        assert!(matches!(
            severity_ms(15.5, budget, Grading::VsyncBounded),
            Severity::Warn
        ));
        // Comfortably under: still ok.
        assert!(matches!(
            severity_ms(8.0, budget, Grading::VsyncBounded),
            Severity::Ok
        ));
        // Past 110% of budget — actually missing frames.
        assert!(matches!(
            severity_ms(20.0, budget, Grading::VsyncBounded),
            Severity::Bad
        ));
    }
}
