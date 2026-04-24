//! Command palette (session switcher, actions, remote host list).
//!
//! Layout is pre-computed by `App::command_palette_layout` so capture
//! only has to read it and translate per-entry state into `PaletteRow`s`.


use super::builder::UiBuilder;
use super::text_layout;
use super::tokens;
use super::types::{UiAction, UiContext, UiPaletteHit, UiScene};
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

    pub(crate) fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let bg_color = cx.theme.surface;
        let accent = cx.theme.accent;
        let border_color = cx.theme.border_focus;
        let dim_color = cx.theme.on_surface_muted;
        let fg_color = cx.theme.on_surface;
        let selected_bg = tokens::tint(accent, tokens::ALPHA_SELECTED_BG);
        let hovered_bg = tokens::tint(accent, tokens::ALPHA_HOVER_BG);

        let px = self.layout.panel_x;
        let pw = self.layout.panel_w;
        let text_pad = tokens::SPACE_2;

        // Outer panel via SDF: rounded bg + hairline border + drop
        // shadow. Panel bg is `surface` *lifted one step* so it reads
        // as raised above the (dimmed) terminal backdrop — with the
        // sRGB-passthrough pipeline there's no accidental gamma lift
        // to rely on (commit 003b123). The input row below stacks a
        // second raise on top of this to keep the visual hierarchy.
        let panel_bg = tokens::surface_raise(
            [bg_color[0], bg_color[1], bg_color[2], 1.0],
            tokens::SURFACE_LIFT,
        );
        scene.sdf_rects.push(ciri_render::sdf_rect::SdfRect {
            pos: [px, self.layout.panel_y],
            size: [pw, self.layout.panel_h],
            color: panel_bg,
            radii: [tokens::SPACE_1; 4],
            border_color,
            border_width: tokens::BORDER_THIN,
            shadow_blur: tokens::SPACE_3,
            shadow_offset: [0.0, tokens::SPACE_1],
            shadow_color: [0.0, 0.0, 0.0, 0.35],
        });

        let row_h = self.layout.row_h;
        for (idx, row) in self.rows.iter().enumerate() {
            let tint = if row.is_selected {
                Some(selected_bg)
            } else if row.is_hovered {
                Some(hovered_bg)
            } else {
                None
            };
            if let Some(color) = tint {
                scene.sdf_rects.push(ciri_render::sdf_rect::SdfRect {
                    pos: [px, self.layout.sep_y + idx as f32 * row_h],
                    size: [pw, row_h],
                    color,
                    radii: [tokens::SPACE_1; 4],
                    ..Default::default()
                });
            }
        }

        let mut ui = UiBuilder::new_vertical(
            px,
            self.layout.panel_y,
            pw,
            self.layout.panel_h,
            0.0,
            0.0,
            0.0,
            false,
            cx,
            scene,
        );

        ui.modal_backdrop([0.0, 0.0, 0.0, tokens::ALPHA_BACKDROP]);
        // (Panel bg/border/shadow already emitted as SdfRect above.)

        let input_row_h = tokens::control_height_md(cx.ui_line_h);
        ui.horizontal(Some(pw), input_row_h, 0.0, |ui| {
            let (rx, ry) = ui.cursor_pos();
            ui.abs_rect(
                rx,
                ry,
                pw,
                input_row_h,
                tokens::surface_raise(
                    [bg_color[0], bg_color[1], bg_color[2], 1.0],
                    tokens::SURFACE_LIFT_HIGH,
                ),
            );
            let text_y = ry + (input_row_h - cx.ui_line_h) * 0.5;

            let input_text = if self.remote_input_mode {
                format!("SSH> {}", self.query)
            } else {
                format!("> {}", self.query)
            };
            ui.abs_text(&input_text, rx + text_pad, text_y, fg_color);

            if self.remote_input_mode && self.query.is_empty() {
                let hint_x = rx + text_pad + ui.text_width(&input_text);
                ui.abs_text("user@host[:port]", hint_x, text_y, dim_color);
            }

            let cursor_x = rx + text_pad + ui.text_width(&input_text);
            ui.abs_rect(
                cursor_x,
                text_y,
                2.0,
                cx.ui_line_h,
                tokens::tint(fg_color, tokens::ALPHA_CURSOR),
            );
        });

        ui.separator_h(border_color, 0.0);
        for row in &self.rows {
            ui.horizontal(Some(pw), row_h, 0.0, |ui| {
                let (rx, ry) = ui.cursor_pos();
                let text_y = ry + (row_h - cx.ui_line_h) * 0.5;
                if row.style == PaletteRowStyle::SectionHeader {
                    let header_text = format!("── {} ──", row.label);
                    ui.abs_text(&header_text, rx + text_pad, text_y, dim_color);
                } else {
                    let color = if row.style == PaletteRowStyle::ConnectRemotePrompt {
                        accent
                    } else {
                        fg_color
                    };
                    ui.abs_text(&row.label, rx + text_pad, text_y, color);
                }
            });
        }

        if self.total_entries > self.layout.visible_rows {
            let track_w = tokens::SPACE_1;
            let track_x = px + pw - tokens::SPACE_2;
            let track_y = self.layout.sep_y + 2.0;
            let track_h = (self.layout.visible_rows as f32 * row_h - tokens::SPACE_1).max(0.0);
            ui.abs_rect(
                track_x,
                track_y,
                track_w,
                track_h,
                tokens::tint(border_color, tokens::ALPHA_SCROLL_TRACK),
            );

            let thumb_h = (track_h * (self.layout.visible_rows as f32 / self.total_entries as f32))
                .max(row_h * 0.75);
            let denom = self
                .total_entries
                .saturating_sub(self.layout.visible_rows)
                .max(1);
            let thumb_y =
                track_y + (track_h - thumb_h).max(0.0) * (self.scroll_offset as f32 / denom as f32);
            ui.abs_rect(
                track_x,
                thumb_y,
                track_w,
                thumb_h,
                tokens::tint(accent, tokens::ALPHA_SCROLL_THUMB),
            );
        }

        let footer = if self.selectable_count > 0 {
            format!("{}/{}", self.selectable_position, self.selectable_count)
        } else {
            "0/0".to_string()
        };
        let footer_x = px + pw - ui.text_width(&footer) - 12.0;
        let footer_y = self.layout.panel_y + self.layout.panel_h - cx.ui_line_h - 2.0;
        ui.abs_text(&footer, footer_x, footer_y, dim_color);

        if self.show_no_matches {
            ui.abs_text(
                "No matching commands",
                px + text_pad,
                self.layout.sep_y + 4.0,
                dim_color,
            );
        }
        // Loading takes precedence over a stale error so the two strings
        // can't paint at the same Y. The `else if` in `capture` already
        // enforces this, but guard defensively here.
        if let Some(ref loading) = self.loading_text {
            let y = self.layout.panel_y + self.layout.panel_h - cx.ui_line_h * 2.0 - 4.0;
            ui.abs_text(
                loading,
                px + text_pad,
                y,
                [accent[0], accent[1], accent[2], 0.7],
            );
        } else if let Some(ref error) = self.error_text {
            let y = self.layout.panel_y + self.layout.panel_h - cx.ui_line_h * 2.0 - 4.0;
            let red = cx.theme.error;
            ui.abs_text(error, px + text_pad, y, [red[0], red[1], red[2], 0.9]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ciri_config::config::CiriConfig;
    use ciri_render::font_resolver::CmapResolver;
    use ciri_render::glyph_cache::FontInitParams;
    use std::sync::Arc;

    fn test_cache() -> ciri_render::glyph_cache::GlyphCache {
        let config = CiriConfig::default();
        ciri_render::glyph_cache::GlyphCache::new(&FontInitParams {
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
        })
    }

    fn test_cx() -> UiContext<'static> {
        let cfg = Box::leak(Box::new(CiriConfig::default()));
        let theme = Box::leak(Box::new(ciri_ui::ResolvedTheme::default()));
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
        }
    }

    #[test]
    fn selected_row_highlight_renders_in_sdf_layer() {
        let cx = test_cx();
        let mut atlas = test_cache();
        let mut bg_rects = Vec::new();
        let mut glyphs = Vec::new();
        let mut color_glyphs = Vec::new();
        let mut sdf_rects = Vec::new();
        let mut scene = UiScene {
            atlas: &mut atlas,
            bg_rects: &mut bg_rects,
            glyphs: &mut glyphs,
            color_glyphs: &mut color_glyphs,
            sdf_rects: &mut sdf_rects,
        };
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
            rows: vec![PaletteRow {
                entry_idx: 0,
                label: String::new(),
                is_selected: true,
                is_hovered: false,
                style: PaletteRowStyle::Action,
            }],
            total_entries: 1,
            selectable_position: 1,
            selectable_count: 1,
            show_no_matches: false,
            loading_text: None,
            error_text: None,
            remote_input_mode: false,
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
}
