//! Command palette (session switcher, actions, remote host list).
//!
//! A full-viewport modal with a search input at the top, a scrollable
//! list of entries, an optional scrollbar, a footer counter, and
//! transient status messages (no-matches / loading / error). The
//! layout is pre-computed by `App::command_palette_layout` so capture
//! only has to read it and translate per-entry state into `PaletteRow`s.
//!
//! Paint goes through several sibling ciri-ui trees rather than one
//! nested tree because ciri-ui has no `position: absolute` yet and the
//! scrollbar / footer / status messages sit on top of the row list
//! area in viewport coordinates. Each is a small translated root that
//! emits into the shared `Modal` layer, so layer bucketing keeps the
//! final z-order stable.

use ciri_ui::color::{scale_rgb, with_alpha};
use ciri_ui::{div, text, Div, Layer, ResolvedTheme, Styled};

use super::text_layout;
use super::tokens;
use super::types::{UiAction, UiContext, UiPaletteHit};
use crate::app::App;

pub(super) struct PaletteRow {
    pub entry_idx: usize,
    pub label: String,
    pub is_selected: bool,
    pub is_hovered: bool,
    pub style: PaletteRowStyle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PaletteRowStyle {
    SectionHeader,
    Action,
    Session,
    RemoteHost,
    RemoteSession,
    SshShell,
    DirectConnect,
    ConnectRemotePrompt,
}

pub(crate) struct PaletteComponent {
    layout: super::super::CommandPaletteLayout,
    query: String,
    scroll_offset: usize,
    rows: Vec<PaletteRow>,
    total_entries: usize,
    /// 1-based position among selectable entries (for footer display).
    selectable_position: usize,
    /// Total number of selectable entries (for footer display).
    selectable_count: usize,
    show_no_matches: bool,
    loading_text: Option<String>,
    error_text: Option<String>,
    remote_input_mode: bool,
    // ── Captured from UiContext so build_trees can draw without needing it ──
    viewport_w: f32,
    viewport_h: f32,
    ui_line_h: f32,
    input_row_h: f32,
}

fn truncate_label(label: &str, panel_w: f32, cx: &UiContext<'_>) -> String {
    let max_w = (panel_w - 16.0).max(0.0);
    text_layout::truncate_with_ellipsis(cx, label, max_w)
}

impl PaletteComponent {
    pub fn capture(app: &App, cx: &UiContext<'_>) -> Option<Self> {
        let palette = app.core.command_palette.as_ref()?;
        let layout = app.command_palette_layout()?;
        let scroll_offset = app.command_palette_scroll_offset(layout.visible_rows);
        let rows = palette
            .filtered
            .iter()
            .skip(scroll_offset)
            .take(layout.visible_rows)
            .enumerate()
            .map(|(vis_row, filt_idx)| {
                let entry = &palette.entries[*filt_idx];
                let style = match &entry.kind {
                    super::super::PaletteEntryKind::SectionHeader(_) => {
                        PaletteRowStyle::SectionHeader
                    }
                    super::super::PaletteEntryKind::Action(_) => PaletteRowStyle::Action,
                    super::super::PaletteEntryKind::GoToSession { .. }
                    | super::super::PaletteEntryKind::KillSession(_) => PaletteRowStyle::Session,
                    super::super::PaletteEntryKind::RemoteHost { .. } => {
                        PaletteRowStyle::RemoteHost
                    }
                    super::super::PaletteEntryKind::RemoteSession { .. } => {
                        PaletteRowStyle::RemoteSession
                    }
                    super::super::PaletteEntryKind::SshShell { .. } => PaletteRowStyle::SshShell,
                    super::super::PaletteEntryKind::DirectConnect { .. } => {
                        PaletteRowStyle::DirectConnect
                    }
                    super::super::PaletteEntryKind::ConnectRemotePrompt => {
                        PaletteRowStyle::ConnectRemotePrompt
                    }
                };
                PaletteRow {
                    entry_idx: *filt_idx,
                    label: truncate_label(&entry.label, layout.panel_w, cx),
                    is_selected: scroll_offset + vis_row == palette.selected_idx,
                    is_hovered: palette.hovered_idx == Some(scroll_offset + vis_row),
                    style,
                }
            })
            .collect();

        let loading_text = palette
            .remote_loading
            .as_ref()
            .map(|name| format!("Loading sessions from {}...", name));
        let error_text = palette
            .remote_error
            .as_ref()
            .map(|(name, err)| format!("{}: {}", name, err));

        // Compute selectable-only position and count for footer
        let selectable_indices: Vec<usize> = palette
            .filtered
            .iter()
            .enumerate()
            .filter(|(_, i)| palette.entries[**i].kind.is_selectable())
            .map(|(pos, _)| pos)
            .collect();
        let selectable_count = selectable_indices.len();
        let selectable_position = selectable_indices
            .iter()
            .position(|&pos| pos == palette.selected_idx)
            .map(|p| p + 1)
            .unwrap_or(0);

        Some(Self {
            layout,
            query: palette.query.clone(),
            rows,
            scroll_offset,
            total_entries: palette.filtered.len(),
            selectable_position,
            selectable_count,
            show_no_matches: palette.filtered.is_empty()
                && !palette.query.is_empty()
                && !palette.remote_input_mode,
            loading_text,
            error_text,
            remote_input_mode: palette.remote_input_mode,
            viewport_w: cx.viewport_w,
            viewport_h: cx.viewport_h,
            ui_line_h: cx.ui_line_h,
            input_row_h: tokens::control_height_md(cx.ui_line_h),
        })
    }

    pub(super) fn hit_test(&self, mx: f32, my: f32) -> UiPaletteHit {
        if mx < self.layout.panel_x
            || mx > self.layout.panel_x + self.layout.panel_w
            || my < self.layout.panel_y
            || my > self.layout.panel_y + self.layout.panel_h
        {
            return UiPaletteHit::None;
        }
        if my < self.layout.sep_y {
            return UiPaletteHit::Panel;
        }
        let vis_row = ((my - self.layout.sep_y) / self.layout.row_h)
            .floor()
            .max(0.0) as usize;
        if vis_row >= self.rows.len() {
            return UiPaletteHit::Panel;
        }
        // Section headers are not clickable
        if self.rows[vis_row].style == PaletteRowStyle::SectionHeader {
            return UiPaletteHit::Panel;
        }
        UiPaletteHit::Entry(self.rows[vis_row].entry_idx)
    }

    /// Map a click to a `UiAction`. Preserves the legacy semantics:
    /// clicking an entry runs it, clicking on the panel body (between
    /// rows, on section headers, or on the input row) is a no-op, and
    /// clicking outside the panel closes the palette.
    pub(crate) fn click(&self, mx: f32, my: f32, _cx: &UiContext<'_>) -> Option<UiAction> {
        match self.hit_test(mx, my) {
            UiPaletteHit::Entry(entry_idx) => Some(UiAction::ExecutePaletteEntry(entry_idx)),
            UiPaletteHit::Panel => None,
            UiPaletteHit::None => Some(UiAction::ClosePalette),
        }
    }
}

impl PaletteComponent {
    /// Build the set of ciri-ui trees for this palette snapshot.
    ///
    /// Returns one tree per logical overlay — backdrop, panel (frame
    /// + input + rows), scrollbar, footer, status message. Each is
    /// positioned in viewport coords via `translate` on its root and
    /// rendered onto `Layer::Modal`; paint order within that layer is
    /// Vec order, so callers should iterate in sequence.
    ///
    /// Splitting into multiple trees rather than a single nested one
    /// sidesteps the lack of CSS-style absolute positioning in ciri-ui:
    /// each overlay just anchors at its own absolute origin and its
    /// internals compose via flex.
    pub(crate) fn build_trees(&self, theme: &ResolvedTheme) -> Vec<Div> {
        let mut trees = Vec::with_capacity(6);

        trees.push(self.backdrop());
        trees.push(self.panel(theme));

        if self.total_entries > self.layout.visible_rows {
            trees.push(self.scrollbar(theme));
        }
        trees.push(self.footer(theme));

        if self.show_no_matches {
            trees.push(self.no_matches(theme));
        }
        // Loading and error are mutually exclusive in practice (set by
        // different remote-fetch phases) but the legacy path painted
        // whichever was Some in the same bottom slot — preserve that.
        if let Some(ref msg) = self.loading_text {
            trees.push(self.bottom_status(msg, with_alpha(theme.accent, 0.7)));
        }
        if let Some(ref msg) = self.error_text {
            trees.push(self.bottom_status(msg, with_alpha(theme.error, 0.9)));
        }

        trees
    }

    fn backdrop(&self) -> Div {
        div()
            .in_layer(Layer::Modal)
            .w(self.viewport_w)
            .h(self.viewport_h)
            .bg([0.0, 0.0, 0.0, tokens::ALPHA_BACKDROP])
    }

    fn panel(&self, theme: &ResolvedTheme) -> Div {
        // Surface is lifted one step over `term_bg` so the panel reads
        // as a raised rectangle over the dimmed backdrop. The legacy
        // path fed raw sRGB hex into the flat-rect pipeline which got
        // a "free" double-gamma lift on write, making the panel visibly
        // brighter than the terminal bg by accident. ciri-ui is
        // gamma-correct (linear values → sRGB on write), so when we
        // used `theme.term_bg` directly the panel painted the exact
        // same colour as the pane content and looked transparent.
        // `scale_rgb(term_bg, 1.15)` matches paste_dialog's surface.
        let bg_color = scale_rgb(theme.term_bg, 1.15);
        let input_bg = scale_rgb(theme.term_bg, 1.35);
        let selected_bg = with_alpha(theme.accent, tokens::ALPHA_SELECTED_BG);
        let hovered_bg = with_alpha(theme.accent, tokens::ALPHA_HOVER_BG);
        let cursor_color = with_alpha(theme.on_surface, tokens::ALPHA_CURSOR);
        let pad = tokens::SPACE_2;

        // ── Input row: "> query" (or "SSH> query" + placeholder) + cursor.
        // Flex-row lays text then cursor in sequence; text-first /
        // placeholder-between ordering is handled by conditional
        // children so the cursor always sits after the typed text
        // regardless of which mode we're in.
        let input_text = if self.remote_input_mode {
            format!("SSH> {}", self.query)
        } else {
            format!("> {}", self.query)
        };

        let mut input_row = div()
            .flex_row()
            .items_center()
            .w(self.layout.panel_w)
            .h(self.input_row_h)
            .px(pad)
            .bg(input_bg)
            .child(text(input_text).color(theme.on_surface));
        if self.remote_input_mode && self.query.is_empty() {
            input_row =
                input_row.child(text("user@host[:port]").color(theme.on_surface_muted));
        }
        input_row = input_row.child(
            div()
                .w(2.0)
                .h(self.ui_line_h)
                .bg(cursor_color),
        );

        // ── Separator — a 1px line under the input row.
        let separator = div()
            .w(self.layout.panel_w)
            .h(tokens::BORDER_THIN)
            .bg(theme.border_focus);

        // ── Row list.
        let mut row_list = div().flex_col().w(self.layout.panel_w);
        for row in &self.rows {
            let row_h = self.layout.row_h;
            let row_div = if row.style == PaletteRowStyle::SectionHeader {
                div()
                    .flex_row()
                    .items_center()
                    .w(self.layout.panel_w)
                    .h(row_h)
                    .px(pad)
                    .child(
                        text(format!("── {} ──", row.label)).color(theme.on_surface_muted),
                    )
            } else {
                let (bg, label_color) = if row.is_selected {
                    (selected_bg, theme.on_surface)
                } else if row.is_hovered {
                    (hovered_bg, theme.on_surface)
                } else if row.style == PaletteRowStyle::ConnectRemotePrompt {
                    ([0.0; 4], theme.accent)
                } else {
                    ([0.0; 4], theme.on_surface)
                };
                div()
                    .flex_row()
                    .items_center()
                    .w(self.layout.panel_w)
                    .h(row_h)
                    .px(pad)
                    .bg(bg)
                    .child(text(&row.label).color(label_color))
            };
            row_list = row_list.child(row_div);
        }

        // ── Panel frame: input + separator + rows stacked inside a
        // bordered container anchored at `(panel_x, panel_y)`.
        div()
            .in_layer(Layer::Modal)
            .translate(self.layout.panel_x, self.layout.panel_y)
            .w(self.layout.panel_w)
            .h(self.layout.panel_h)
            .flex_col()
            .bg(bg_color)
            .border(tokens::BORDER_THIN, theme.border_focus)
            .child(input_row)
            .child(separator)
            .child(row_list)
    }

    fn scrollbar(&self, theme: &ResolvedTheme) -> Div {
        let track_w = tokens::SPACE_1;
        let track_x = self.layout.panel_x + self.layout.panel_w - tokens::SPACE_2;
        let track_y = self.layout.sep_y + 2.0;
        let track_h = (self.layout.visible_rows as f32 * self.layout.row_h - tokens::SPACE_1)
            .max(0.0);
        let thumb_h = (track_h * (self.layout.visible_rows as f32 / self.total_entries as f32))
            .max(self.layout.row_h * 0.75);
        let denom = self
            .total_entries
            .saturating_sub(self.layout.visible_rows)
            .max(1);
        let thumb_offset =
            (track_h - thumb_h).max(0.0) * (self.scroll_offset as f32 / denom as f32);
        let track_color = with_alpha(theme.border_focus, tokens::ALPHA_SCROLL_TRACK);
        let thumb_color = with_alpha(theme.accent, tokens::ALPHA_SCROLL_THUMB);

        // Track is the container's own bg; thumb is a single child
        // whose translate offsets it inside the track.
        div()
            .in_layer(Layer::Modal)
            .translate(track_x, track_y)
            .w(track_w)
            .h(track_h)
            .bg(track_color)
            .child(
                div()
                    .w(track_w)
                    .h(thumb_h)
                    .translate(0.0, thumb_offset)
                    .bg(thumb_color),
            )
    }

    fn footer(&self, theme: &ResolvedTheme) -> Div {
        let footer_str = if self.selectable_count > 0 {
            format!("{}/{}", self.selectable_position, self.selectable_count)
        } else {
            "0/0".to_string()
        };
        // `justify_end` right-aligns the text inside a panel-wide row;
        // `pr(12.0)` replicates the legacy 12px right inset.
        let row_y = self.layout.panel_y + self.layout.panel_h - self.ui_line_h - 2.0;
        div()
            .in_layer(Layer::Modal)
            .translate(self.layout.panel_x, row_y)
            .w(self.layout.panel_w)
            .h(self.ui_line_h)
            .flex_row()
            .justify_end()
            .pr(12.0)
            .child(text(footer_str).color(theme.on_surface_muted))
    }

    fn no_matches(&self, theme: &ResolvedTheme) -> Div {
        div()
            .in_layer(Layer::Modal)
            .translate(self.layout.panel_x, self.layout.sep_y + 4.0)
            .w(self.layout.panel_w)
            .h(self.ui_line_h)
            .flex_row()
            .items_center()
            .px(tokens::SPACE_2)
            .child(text("No matching commands").color(theme.on_surface_muted))
    }

    fn bottom_status(&self, msg: &str, color: ciri_ui::Color) -> Div {
        let y = self.layout.panel_y + self.layout.panel_h - self.ui_line_h * 2.0 - 4.0;
        div()
            .in_layer(Layer::Modal)
            .translate(self.layout.panel_x, y)
            .w(self.layout.panel_w)
            .h(self.ui_line_h)
            .flex_row()
            .items_center()
            .px(tokens::SPACE_2)
            .child(text(msg.to_string()).color(color))
    }
}
