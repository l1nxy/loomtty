//! Keybindings help overlay — a read-only cheat-sheet listing the live
//! keymap (leader bindings, mode tables, and direct/leaderless bindings).
//!
//! Modeled on the settings panel (centered modal + scrim backdrop) and
//! the info-box (key→label rows via [`super::info_box::build_infobox_rows`]).
//! Unlike the settings panel it owns no interactive controls: any key or
//! click dismisses it, so there is no hit-id encoding here — `UiFrame`
//! turns any click into [`UiAction::CloseHelp`] while it is visible.
//!
//! Sections are built from `config.keys` (data-driven) and packed into a
//! responsive number of columns by a balanced greedy fill, so the panel
//! stays compact regardless of how many bindings the user has.

use super::info_box::build_infobox_rows;
use super::text_layout;
use super::tokens;
use super::types::{UiContext, UiScene};
use crate::app::App;
use crate::app::loom_ui_adapter::paint_element_tree;
use loom_ui::{Div, IntoElement, Render, RenderCtx, Styled, deferred, div, text};

/// A titled group of key→label rows (e.g. "Leader (alt)", "Resize").
struct HelpSection {
    title: String,
    rows: Vec<(String, String)>,
}

impl HelpSection {
    /// Pixel height of this section's block: title row + each binding row.
    fn height(&self, row_h: f32, title_h: f32) -> f32 {
        title_h + tokens::SPACE_1 + self.rows.len() as f32 * row_h
    }
}

pub(crate) struct HelpOverlayComponent {
    dx: f32,
    dy: f32,
    panel_w: f32,
    panel_h: f32,
    sections: Vec<HelpSection>,
    /// Column assignment: `columns[i]` holds indices into `sections`.
    columns: Vec<Vec<usize>>,
    /// Shared right-aligned key column width (shaped), so keys line up
    /// down every column identically.
    key_col_w: f32,
    /// Width of one section/column's content box.
    col_content_w: f32,
    cell_w: f32,
    cell_h: f32,
    ui_line_h: f32,
}

/// Mode tables shown first and in this order; any other user-defined
/// modes follow alphabetically. Keeps the common resize/scroll/move
/// trio in a predictable place while staying open to custom tables.
const PREFERRED_MODE_ORDER: [&str; 3] = ["resize", "scroll", "move"];

impl HelpOverlayComponent {
    pub fn capture(app: &App, cx: &UiContext<'_>) -> Option<Self> {
        if !app.core.help_visible {
            return None;
        }

        let keys = &app.core.config.keys;

        // ── Build sections from the live keymap ──
        let mut sections: Vec<HelpSection> = Vec::new();
        sections.push(HelpSection {
            title: format!("Leader ({})", keys.leader),
            rows: build_infobox_rows(&keys.bindings),
        });

        // Known modes first (in PREFERRED_MODE_ORDER), then any extras.
        let mut mode_names: Vec<&String> = keys.modes.keys().collect();
        mode_names.sort_by_key(|name| {
            PREFERRED_MODE_ORDER
                .iter()
                .position(|p| *p == name.as_str())
                .unwrap_or(PREFERRED_MODE_ORDER.len())
        });
        for name in mode_names {
            if let Some(bindings) = keys.modes.get(name) {
                let rows = build_infobox_rows(bindings);
                if rows.is_empty() {
                    continue;
                }
                sections.push(HelpSection {
                    title: capitalize(name),
                    rows,
                });
            }
        }

        if !keys.direct_bindings.is_empty() {
            sections.push(HelpSection {
                title: "Direct (no leader)".to_string(),
                rows: build_infobox_rows(&keys.direct_bindings),
            });
        }

        // ── Measure column geometry from shaped glyph advances ──
        let key_col_w = sections
            .iter()
            .flat_map(|s| s.rows.iter())
            .map(|(k, _)| text_layout::measure(cx, k))
            .fold(0.0_f32, f32::max);
        let label_col_w = sections
            .iter()
            .flat_map(|s| s.rows.iter())
            .map(|(_, v)| text_layout::measure(cx, v))
            .fold(0.0_f32, f32::max);
        let max_title_w = sections
            .iter()
            .map(|s| text_layout::measure(cx, &s.title))
            .fold(0.0_f32, f32::max);

        let gap_w = cx.cell_w * 2.0;
        let col_content_w = (key_col_w + gap_w + label_col_w).max(max_title_w);

        let row_h = cx.cell_h + tokens::SPACE_1 * 2.0;
        let title_h = cx.cell_h + tokens::SPACE_1;
        let col_gap = tokens::SPACE_6;
        let pad_x = tokens::SPACE_4;
        let pad_y = tokens::SPACE_3;

        // ── Pick a column count that fits the viewport, then balance ──
        let avail_w = cx.viewport_w * 0.94 - pad_x * 2.0;
        let max_fit = ((avail_w + col_gap) / (col_content_w + col_gap)).floor() as i32;
        let n_cols = max_fit.clamp(1, 3).min(sections.len().max(1) as i32) as usize;

        let weights: Vec<f32> = sections
            .iter()
            .map(|s| s.height(row_h, title_h) + tokens::SPACE_3)
            .collect();
        let columns = distribute(&weights, n_cols);

        // Tallest column drives the panel body height.
        let content_h = columns
            .iter()
            .map(|col| {
                let mut h = 0.0_f32;
                for (i, &sec_idx) in col.iter().enumerate() {
                    if i > 0 {
                        h += tokens::SPACE_3;
                    }
                    h += sections[sec_idx].height(row_h, title_h);
                }
                h
            })
            .fold(0.0_f32, f32::max);

        let inner_w = n_cols as f32 * col_content_w + (n_cols.saturating_sub(1)) as f32 * col_gap;
        let title_bar_h = cx.ui_line_h + tokens::SPACE_3;

        let panel_w = (inner_w + pad_x * 2.0).min(cx.viewport_w * 0.96);
        let panel_h =
            (title_bar_h + tokens::BORDER_THIN + content_h + pad_y * 2.0).min(cx.viewport_h * 0.94);
        let dx = ((cx.viewport_w - panel_w) / 2.0).max(0.0);
        let dy = ((cx.viewport_h - panel_h) / 2.0).max(0.0);

        Some(Self {
            dx,
            dy,
            panel_w,
            panel_h,
            sections,
            columns,
            key_col_w,
            col_content_w,
            cell_w: cx.cell_w,
            cell_h: cx.cell_h,
            ui_line_h: cx.ui_line_h,
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
        let theme = cx.theme;
        let bw = tokens::BORDER_THIN;
        let pad_x = tokens::SPACE_4;
        let pad_y = tokens::SPACE_3;
        let col_gap = tokens::SPACE_6;

        let title_bar = self.build_title_bar(cx);
        let title_divider = div().w(self.panel_w).h(bw).bg(theme.border);

        // Columns of section blocks.
        let mut body = div().flex_row().gap(col_gap).px(pad_x).py(pad_y).flex_1();
        for col in &self.columns {
            let mut column = div().flex_col().gap(tokens::SPACE_3).w(self.col_content_w);
            for &sec_idx in col {
                column = column.child(self.build_section(cx, &self.sections[sec_idx]));
            }
            body = body.child(column);
        }

        let panel = div()
            .absolute()
            .left(self.dx)
            .top(self.dy)
            .w(self.panel_w)
            .h(self.panel_h)
            .flex_col()
            .bg(theme.surface)
            .rounded(theme.radius.lg)
            .border(bw, theme.border)
            .shadow_lg()
            .child(title_bar)
            .child(title_divider)
            .child(body);

        // Full-viewport scrim. The whole overlay is click-to-close, so
        // no hit-ids are needed — `UiFrame::click` returns `CloseHelp`
        // whenever this component is present.
        div()
            .w(cx.viewport[0])
            .h(cx.viewport[1])
            .bg([0.0, 0.0, 0.0, tokens::ALPHA_BACKDROP])
            .child(deferred(panel))
    }

    fn build_title_bar(&self, cx: &RenderCtx<'_>) -> Div {
        let theme = cx.theme;
        let title_h = self.ui_line_h + tokens::SPACE_3;
        div()
            .w(self.panel_w)
            .h(title_h)
            .flex_row()
            .items_center()
            .justify_between()
            .px(tokens::SPACE_4)
            .child(text("Keybindings").color(theme.on_surface))
            .child(text("press any key to close").color(theme.on_surface_muted))
    }

    fn build_section(&self, cx: &RenderCtx<'_>, section: &HelpSection) -> Div {
        let theme = cx.theme;
        let accent = theme.accent;
        let fg = theme.on_surface;
        let dim = theme.on_surface_muted;
        let row_h = self.cell_h + tokens::SPACE_1 * 2.0;
        let title_h = self.cell_h + tokens::SPACE_1;
        let gap_w = self.cell_w * 2.0;

        let mut block = div()
            .flex_col()
            .w(self.col_content_w)
            .child(
                // Section header: muted label.
                div()
                    .w(self.col_content_w)
                    .h(title_h)
                    .flex_row()
                    .items_center()
                    .child(text(section.title.clone()).color(dim)),
            )
            // Hairline under the header (loom_ui borders are uniform, so
            // an explicit 1px div is used for a bottom-only rule).
            .child(
                div()
                    .w(self.col_content_w)
                    .h(tokens::BORDER_THIN)
                    .bg(theme.border),
            )
            .child(div().w(self.col_content_w).h(tokens::SPACE_1));

        for (key, desc) in &section.rows {
            block = block.child(
                div()
                    .w(self.col_content_w)
                    .h(row_h)
                    .flex_row()
                    .items_center()
                    .child(
                        div()
                            .w(self.key_col_w)
                            .h(row_h)
                            .flex_row()
                            .items_center()
                            .justify_end()
                            .child(text(key.clone()).color(accent)),
                    )
                    .child(div().w(gap_w).h(row_h))
                    .child(text(desc.clone()).color(if key == "esc" { dim } else { fg })),
            );
        }

        block
    }
}

impl Render for HelpOverlayComponent {
    fn render(&mut self, cx: &RenderCtx<'_>) -> impl IntoElement {
        self.build_tree(cx)
    }
}

/// Greedy balanced fill: assign each section (in order) to the currently
/// shortest column. Keeps column heights close without splitting a
/// section across columns.
fn distribute(weights: &[f32], n_cols: usize) -> Vec<Vec<usize>> {
    let n_cols = n_cols.max(1);
    let mut columns: Vec<Vec<usize>> = vec![Vec::new(); n_cols];
    let mut heights = vec![0.0_f32; n_cols];
    for (i, &w) in weights.iter().enumerate() {
        let shortest = heights
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        columns[shortest].push(i);
        heights[shortest] += w;
    }
    columns
}

/// Title-case a single mode name (`"resize"` → `"Resize"`).
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distribute_balances_into_columns() {
        // Three equal weights across three columns → one each.
        let cols = distribute(&[1.0, 1.0, 1.0], 3);
        assert_eq!(cols.len(), 3);
        assert!(cols.iter().all(|c| c.len() == 1));
    }

    #[test]
    fn distribute_puts_small_after_big_in_shortest() {
        // Big first section claims col0; the two small ones fill col1/col2.
        let cols = distribute(&[10.0, 1.0, 1.0], 3);
        assert_eq!(cols[0], vec![0]);
        assert_eq!(cols[1], vec![1]);
        assert_eq!(cols[2], vec![2]);
    }

    #[test]
    fn distribute_single_column_keeps_order() {
        let cols = distribute(&[3.0, 1.0, 2.0], 1);
        assert_eq!(cols, vec![vec![0, 1, 2]]);
    }

    #[test]
    fn capitalize_basics() {
        assert_eq!(capitalize("resize"), "Resize");
        assert_eq!(capitalize(""), "");
        assert_eq!(capitalize("a"), "A");
    }
}
