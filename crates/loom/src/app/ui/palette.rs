//! Command palette (session switcher, actions, remote host list).
//!
//! Layout is pre-computed by `App::command_palette_layout` so capture
//! only has to read it and translate per-entry state into `PaletteRow`s`.

use super::text_layout;
use super::tokens;
use super::types::{UiAction, UiContext, UiPaletteHit, UiScene, ui_hit_id};
use crate::app::App;
use crate::app::loom_ui_adapter::paint_element_tree;
use loom_ui::{
    Div, ElevationIndex, FluentBuilder, IntoElement, Render, RenderCtx, Styled, deferred, div,
    text, uniform_list,
};

const HIT_CLOSE: u64 = 1;
const HIT_PANEL: u64 = 2;
const HIT_ENTRY_BASE: u64 = 1_000_000;

fn entry_hit_id(entry_idx: usize) -> u64 {
    HIT_ENTRY_BASE + entry_idx as u64
}

fn palette_hit_from_id(hit_id: Option<u64>) -> UiPaletteHit {
    match hit_id {
        Some(HIT_CLOSE) => UiPaletteHit::None,
        Some(HIT_PANEL) => UiPaletteHit::Panel,
        Some(id) if id >= HIT_ENTRY_BASE => UiPaletteHit::Entry((id - HIT_ENTRY_BASE) as usize),
        _ => UiPaletteHit::Panel,
    }
}

pub(super) struct PaletteRow {
    pub entry_idx: usize,
    pub label: String,
    pub is_selected: bool,
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
    /// UI font line height in logical pixels. Captured from `UiContext`
    /// at snapshot time so the `Render` impl can build its tree from a
    /// minimal `RenderCtx` without re-borrowing the host shaper. Falls
    /// back to `cell_h` upstream when the shaper is unavailable.
    ui_line_h: f32,
}

fn truncate_label(label: &str, panel_w: f32, cx: &UiContext<'_>) -> String {
    // Reserve `text_pad` worth of inset on each side of the row so the
    // ellipsis never overlaps the panel border. Matches the row's own
    // `.child(div().w(text_pad)...)` left/right gutters in `build_tree`.
    let max_w = (panel_w - tokens::SPACE_2 * 2.0).max(0.0);
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
            ui_line_h: cx.ui_line_h,
        })
    }

    /// Build a minimal `RenderCtx` from the host's `UiContext`. The
    /// `Render` trait keeps its context narrow on purpose; loom's
    /// `UiContext` carries shaper / taffy / mouse fields the trait
    /// shouldn't see, so we project just the three values it needs.
    fn render_cx<'a>(cx: &'a UiContext<'_>) -> RenderCtx<'a> {
        RenderCtx {
            theme: cx.theme,
            viewport: [cx.viewport_w, cx.viewport_h],
            scale: 1.0,
        }
    }

    pub(super) fn hit_test(&self, mx: f32, my: f32, cx: &UiContext<'_>) -> UiPaletteHit {
        let render_cx = Self::render_cx(cx);
        let root = self.build_tree(&render_cx);
        palette_hit_from_id(ui_hit_id(&root, cx, mx, my))
    }

    /// Map a click to a `UiAction`: clicking an entry runs it, clicking on the
    /// panel body (between rows, on section headers, or on the input row) is a
    /// no-op, and clicking outside the panel closes the palette.
    pub(crate) fn click(&self, mx: f32, my: f32, _cx: &UiContext<'_>) -> Option<UiAction> {
        match self.hit_test(mx, my, _cx) {
            UiPaletteHit::Entry(entry_idx) => Some(UiAction::ExecutePaletteEntry(entry_idx)),
            UiPaletteHit::Panel => None,
            UiPaletteHit::None => Some(UiAction::ClosePalette),
        }
    }

    fn build_tree(&self, cx: &RenderCtx<'_>) -> Div {
        let bg_color = cx.theme.surface;
        let accent = cx.theme.accent;
        let border_color = cx.theme.border;
        let dim_color = cx.theme.on_surface_muted;
        let fg_color = cx.theme.on_surface;
        let selected_bg = tokens::tint(accent, tokens::ALPHA_SELECTED_BG);
        // Hover stays neutral (`element_hover`, preset-independent) so
        // the per-theme accent is reserved for the selection cue.
        // Accent-tinted hover painted every preset's chrome a different
        // colour and worked against the stage 0.1 decoupling.
        let hovered_bg = cx.theme.element_hover;

        let px = self.layout.panel_x;
        let pw = self.layout.panel_w;
        let text_pad = tokens::SPACE_2;

        // Panel body sits at the `Panel` elevation tier (sunk surface).
        // Centralises the `surface` → sunk-panel relationship in
        // `ElevationIndex` so the rest of the chrome — context_menu /
        // dialog body / future settings sections — share one source of
        // truth instead of each computing `surface_sink` locally.
        let panel_bg = ElevationIndex::Panel.bg(cx.theme);
        let row_h = self.layout.row_h;
        let input_row_h = (self.layout.sep_y - self.layout.panel_y - tokens::BORDER_THIN).max(0.0);
        // Input pill sits at the natural chrome surface — i.e. one tier
        // *above* the sunken panel body. The pill's rounded edges and
        // the +0.04 channel delta against `panel_bg` give it the
        // Raycast/Linear-style "raised search bar" feel without pushing
        // the colour into the backdrop's brightness range.
        let input_bg = bg_color;
        let input_text = if self.remote_input_mode {
            format!("SSH> {}", self.query)
        } else {
            format!("> {}", self.query)
        };

        let show_remote_placeholder = self.remote_input_mode && self.query.is_empty();
        // Raycast/Linear-style pill: the input is an inset rounded box with
        // its own background, gutters on left/right so the rounded edges
        // don't kiss the panel border.
        let pill_v_inset = tokens::SPACE_1;
        let pill_h_inset = tokens::SPACE_2;
        let pill_h = (input_row_h - pill_v_inset * 2.0).max(self.ui_line_h);
        let input_pill = div()
            .flex_1()
            .h(pill_h)
            .flex_row()
            .items_center()
            .bg(input_bg)
            .rounded(cx.theme.radius.sm)
            .child(div().w(text_pad).h(pill_h))
            .child(text(input_text.clone()).color(fg_color))
            .child(
                div()
                    .w(2.0)
                    .h(self.ui_line_h)
                    .bg(tokens::tint(fg_color, tokens::ALPHA_CURSOR)),
            )
            .when(show_remote_placeholder, |d| {
                d.child(text("user@host[:port]").color(dim_color))
            });
        let input_row = div()
            .w_full()
            .h(input_row_h)
            .flex_row()
            .items_center()
            .child(div().w(pill_h_inset).h(input_row_h))
            .child(input_pill)
            .child(div().w(pill_h_inset).h(input_row_h));

        // `self.rows` is already pre-windowed by `capture()` to the
        // visible slice; uniform_list here gives the column the correct
        // explicit height and folds the per-row build into a single
        // closure invocation. The no-matches placeholder is layered as a
        // sibling so it isn't counted into the virtualised slice.
        let visible_count = self.rows.len();
        let rows_col = uniform_list(visible_count, row_h, 0..visible_count, |range| {
            range
                .map(|i| {
                    let row = &self.rows[i];
                    let is_header = row.style == PaletteRowStyle::SectionHeader;
                    let label_color = if row.style == PaletteRowStyle::ConnectRemotePrompt {
                        accent
                    } else if is_header {
                        dim_color
                    } else {
                        fg_color
                    };
                    let label_text = if is_header {
                        format!("── {} ──", row.label)
                    } else {
                        row.label.clone()
                    };
                    // Selection is data-driven (keyboard nav owns it),
                    // so it stays in the base style. Hover is paint-time
                    // and resolves from `cx.is_hovered(hit_id)` via the
                    // declarative `.hover()` refinement — only applied
                    // to non-selected non-headers, since selected wins
                    // visually and headers don't take pointer events.
                    let mut row_div = div()
                        .w_full()
                        .h(row_h)
                        .flex_row()
                        .items_center()
                        .child(div().w(text_pad).h(row_h));
                    if !is_header {
                        row_div = row_div.hit_id(entry_hit_id(row.entry_idx)).cursor_pointer();
                        if row.is_selected {
                            row_div = row_div.bg(selected_bg).rounded(cx.theme.radius.sm);
                        } else {
                            row_div =
                                row_div.hover(|s| s.bg(hovered_bg).rounded(cx.theme.radius.sm));
                        }
                    }
                    row_div.child(text(label_text).color(label_color))
                })
                .collect()
        });

        let rows_area_h = self.layout.visible_rows as f32 * row_h;
        // Always wrap in a flex_col so the layout-tree shape is identical
        // regardless of `show_no_matches` — Taffy then has no chance of
        // sizing the placeholder differently between the empty and
        // non-empty branches.
        let rows_inner =
            div()
                .w_full()
                .flex_col()
                .child(rows_col)
                .when(self.show_no_matches, |d| {
                    d.child(
                        div()
                            .w_full()
                            .h(row_h)
                            .flex_row()
                            .items_center()
                            .child(div().w(text_pad).h(row_h))
                            .child(text("No matching commands").color(dim_color)),
                    )
                });
        let mut rows_area = div().w_full().h(rows_area_h).flex_row().child(rows_inner);

        let mut panel = div()
            .absolute()
            .left(px)
            .top(self.layout.panel_y)
            .w(pw)
            .h(self.layout.panel_h)
            .flex_col()
            .bg(panel_bg)
            .rounded(cx.theme.radius.lg)
            .border(tokens::BORDER_THIN, border_color)
            .shadow_lg()
            .hit_id(HIT_PANEL)
            .child(input_row)
            .child(div().w_full().h(tokens::BORDER_THIN).bg(border_color));

        if self.total_entries > self.layout.visible_rows {
            let track_w = tokens::SPACE_1;
            let track_h = (self.layout.visible_rows as f32 * row_h - tokens::SPACE_1).max(0.0);
            let thumb_h = (track_h * (self.layout.visible_rows as f32 / self.total_entries as f32))
                .max(row_h * 0.75);
            let denom = self
                .total_entries
                .saturating_sub(self.layout.visible_rows)
                .max(1);
            let thumb_top =
                (track_h - thumb_h).max(0.0) * (self.scroll_offset as f32 / denom as f32);
            rows_area = rows_area.child(
                div()
                    .w(track_w)
                    .h(track_h)
                    .translate(-tokens::SPACE_2, 2.0)
                    .bg(tokens::tint(border_color, tokens::ALPHA_SCROLL_TRACK))
                    .child(
                        div()
                            .w(track_w)
                            .h(thumb_h)
                            .translate(0.0, thumb_top)
                            .bg(tokens::tint(accent, tokens::ALPHA_SCROLL_THUMB)),
                    ),
            );
        }
        panel = panel.child(rows_area).child(div().w_full().flex_1());

        // Loading takes precedence over a stale error so the two strings
        // can't paint at the same Y. The `else if` below — not `capture`,
        // which sets `loading_text` and `error_text` independently — is
        // what enforces the precedence.
        let line_h = self.ui_line_h;
        if let Some(ref loading) = self.loading_text {
            panel = panel.child(
                div()
                    .w_full()
                    .h(line_h)
                    .flex_row()
                    .items_center()
                    .child(div().w(text_pad).h(line_h))
                    .child(text(loading.clone()).color([accent[0], accent[1], accent[2], 0.7])),
            );
        } else if let Some(ref error) = self.error_text {
            let red = cx.theme.error;
            panel = panel.child(
                div()
                    .w_full()
                    .h(line_h)
                    .flex_row()
                    .items_center()
                    .child(div().w(text_pad).h(line_h))
                    .child(text(error.clone()).color([red[0], red[1], red[2], 0.9])),
            );
        }

        let footer = if self.selectable_count > 0 {
            format!("{}/{}", self.selectable_position, self.selectable_count)
        } else {
            "0/0".to_string()
        };
        panel = panel.child(
            div()
                .w_full()
                .h(line_h)
                .flex_row()
                .items_center()
                .justify_end()
                .child(text(footer).color(dim_color))
                .child(div().w(12.0).h(line_h)),
        );

        // Backdrop dim catches outside-clicks (HIT_CLOSE); the panel is
        // wrapped in `deferred()` so the walker drains it after the
        // backdrop, keeping the panel z-on-top without `Layer::Modal`.

        div()
            .w(cx.viewport[0])
            .h(cx.viewport[1])
            .bg([0.0, 0.0, 0.0, tokens::ALPHA_BACKDROP])
            .hit_id(HIT_CLOSE)
            .child(deferred(panel))
    }

    pub(crate) fn paint(&mut self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let render_cx = Self::render_cx(cx);
        // First production usage of `loom_ui::Render`. The trait method
        // returns `impl IntoElement`; coerce explicitly so the rest of
        // the paint path keeps working off a concrete `dyn Element`.
        let root = <Self as Render>::render(self, &render_cx).into_element();
        paint_element_tree(&root, cx, scene);
    }
}

impl Render for PaletteComponent {
    fn render(&mut self, cx: &RenderCtx<'_>) -> impl IntoElement {
        self.build_tree(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_config::config::LoomConfig;
    use loom_render::font_resolver::CmapResolver;
    use loom_render::glyph_cache::FontInitParams;
    use std::sync::Arc;

    fn test_cache() -> loom_render::glyph_cache::GlyphCache {
        let config = LoomConfig::default();
        loom_render::glyph_cache::GlyphCache::new(&FontInitParams {
            font_size_pt: 12.0,
            dpi_scale: 1.0,
            family_name: "",
            ui_family_name: None,
            primary_font_path: None,
            emoji_font_path: None,
            emoji_font_id: None,
            cjk_font_path: None,
            cjk_font_id: None,
            ui_font_path: None,
            ui_font_id: None,
            ui_pixel_size: None,
            render_config: &config.render,
            font_resolver: Arc::new(CmapResolver::new((&[], 0), None, None)),
            #[cfg(windows)]
            dwrite_resolver: None,
            cell_width_scale: None,
            cell_height_scale: None,
        })
    }

    fn test_cx() -> UiContext<'static> {
        let cfg = Box::leak(Box::new(LoomConfig::default()));
        let theme = Box::leak(Box::new(loom_ui::ResolvedTheme::default()));
        UiContext {
            config: cfg,
            theme,
            viewport_w: 800.0,
            viewport_h: 600.0,
            cell_w: 8.0,
            cell_h: 16.0,
            baseline: 12.0,
            ui_line_h: 16.0,
            ui_shaper: None,
            taffy_tree: None,
            mouse_pos: None,
            active_hit_id: None,
            element_states: None,
        }
    }

    #[test]
    fn selected_row_highlight_renders_in_sdf_layer() {
        let cx = test_cx();
        let mut atlas = test_cache();
        let mut glyphs = Vec::new();
        let mut color_glyphs = Vec::new();
        let mut sdf_rects = Vec::new();
        let mut scene = UiScene {
            atlas: &mut atlas,
            glyphs: &mut glyphs,
            color_glyphs: &mut color_glyphs,
            sdf_rects: &mut sdf_rects,
        };
        let mut comp = PaletteComponent {
            layout: crate::app::CommandPaletteLayout {
                panel_x: 100.0,
                panel_y: 80.0,
                panel_w: 240.0,
                panel_h: 140.0,
                row_h: 24.0,
                visible_rows: 3,
                text_x: 0.0,
                text_y: 0.0,
                sep_y: 120.0,
            },
            query: String::new(),
            scroll_offset: 0,
            rows: vec![PaletteRow {
                entry_idx: 0,
                label: String::new(),
                is_selected: true,
                style: PaletteRowStyle::Action,
            }],
            total_entries: 1,
            selectable_position: 1,
            selectable_count: 1,
            show_no_matches: false,
            loading_text: None,
            error_text: None,
            remote_input_mode: false,
            ui_line_h: 16.0,
        };

        comp.paint(&cx, &mut scene);

        let selected_bg = tokens::tint(cx.theme.accent, tokens::ALPHA_SELECTED_BG);
        assert!(
            scene.sdf_rects.iter().any(|r| {
                r.pos == [100.0, 120.0] && r.size == [240.0, 24.0] && r.color == selected_bg
            }),
            "selected row highlight should render in the SDF layer so the panel doesn't cover it"
        );
    }

    #[test]
    fn hit_test_uses_loom_ui_layout_snapshot() {
        let cx = test_cx();
        let comp = PaletteComponent {
            layout: crate::app::CommandPaletteLayout {
                panel_x: 100.0,
                panel_y: 80.0,
                panel_w: 240.0,
                panel_h: 140.0,
                row_h: 24.0,
                visible_rows: 3,
                text_x: 0.0,
                text_y: 0.0,
                sep_y: 120.0,
            },
            query: String::new(),
            scroll_offset: 0,
            rows: vec![
                PaletteRow {
                    entry_idx: 7,
                    label: "Run".into(),
                    is_selected: false,
                    style: PaletteRowStyle::Action,
                },
                PaletteRow {
                    entry_idx: 8,
                    label: "Section".into(),
                    is_selected: false,
                    style: PaletteRowStyle::SectionHeader,
                },
            ],
            total_entries: 2,
            selectable_position: 1,
            selectable_count: 1,
            show_no_matches: false,
            loading_text: None,
            error_text: None,
            remote_input_mode: false,
            ui_line_h: 16.0,
        };

        assert_eq!(comp.hit_test(110.0, 130.0, &cx), UiPaletteHit::Entry(7));
        assert_eq!(comp.hit_test(110.0, 154.0, &cx), UiPaletteHit::Panel);
        assert_eq!(comp.hit_test(10.0, 10.0, &cx), UiPaletteHit::None);
    }
}
