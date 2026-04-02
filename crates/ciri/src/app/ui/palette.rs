use ciri_config::theme::ThemeConfig;
use ciri_render::rect::Rect;
use unicode_width::UnicodeWidthStr;

use super::types::{UiAction, UiComponent, UiContext, UiPaletteHit, UiScene};
use crate::app::status_bar::{TextEmitParams, emit_status_text};
use crate::app::{App, PaletteToggleLayout};

pub(super) struct PaletteRow {
    pub entry_idx: usize,
    pub label: String,
    pub is_selected: bool,
    pub is_hovered: bool,
    pub style: PaletteRowStyle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PaletteRowStyle {
    Action,
    Session,
    RemoteHost,
    RemoteSession,
    SshShell,
    SwitchSlot,
    DirectConnect,
    SlotSession,
}

pub(crate) struct PaletteComponent {
    layout: super::super::CommandPaletteLayout,
    toggle: Option<PaletteToggleLayout>,
    query: String,
    rows: Vec<PaletteRow>,
    sessions_show_all: bool,
    total_entries: usize,
    selected_idx: usize,
    show_no_matches: bool,
    loading_text: Option<String>,
    error_text: Option<String>,
}

fn truncate_label(label: &str, panel_w: f32, cw: f32) -> String {
    let max_cols = ((panel_w - 16.0) / cw).floor().max(1.0) as usize;
    if UnicodeWidthStr::width(label) > max_cols {
        format!(
            "{}...",
            &label[..label.floor_char_boundary(max_cols.saturating_sub(3))]
        )
    } else {
        label.to_string()
    }
}

impl PaletteComponent {
    pub fn capture(app: &App, cx: &UiContext<'_>) -> Option<Self> {
        let palette = app.core.command_palette.as_ref()?;
        let layout = app.command_palette_layout()?;
        let toggle = app.command_palette_toggle_layout(layout);
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
                    super::super::PaletteEntryKind::Action(_) => PaletteRowStyle::Action,
                    super::super::PaletteEntryKind::SwitchSession(_)
                    | super::super::PaletteEntryKind::KillSession(_) => PaletteRowStyle::Session,
                    super::super::PaletteEntryKind::RemoteHost { .. } => {
                        PaletteRowStyle::RemoteHost
                    }
                    super::super::PaletteEntryKind::RemoteSession { .. } => {
                        PaletteRowStyle::RemoteSession
                    }
                    super::super::PaletteEntryKind::SshShell { .. } => PaletteRowStyle::SshShell,
                    super::super::PaletteEntryKind::SwitchSlot(_) => PaletteRowStyle::SwitchSlot,
                    super::super::PaletteEntryKind::DirectConnect { .. } => {
                        PaletteRowStyle::DirectConnect
                    }
                    super::super::PaletteEntryKind::SlotSession { .. } => {
                        PaletteRowStyle::SlotSession
                    }
                };
                PaletteRow {
                    entry_idx: *filt_idx,
                    label: truncate_label(&entry.label, layout.panel_w, cx.cell_w),
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

        Some(Self {
            layout,
            toggle,
            query: palette.query.clone(),
            rows,
            sessions_show_all: palette.sessions_show_all,
            total_entries: palette.filtered.len(),
            selected_idx: palette.selected_idx,
            show_no_matches: palette.filtered.is_empty() && !palette.query.is_empty(),
            loading_text,
            error_text,
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
        if let Some(toggle) = self.toggle
            && mx >= toggle.bg_x
            && mx <= toggle.bg_x + toggle.bg_w
            && my >= toggle.bg_y
            && my <= toggle.bg_y + toggle.bg_h
        {
            return UiPaletteHit::Toggle;
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
        UiPaletteHit::Entry(self.rows[vis_row].entry_idx)
    }
}

impl UiComponent for PaletteComponent {
    fn click(&self, mx: f32, my: f32, _cx: &UiContext<'_>) -> Option<UiAction> {
        match self.hit_test(mx, my) {
            UiPaletteHit::Toggle => Some(UiAction::ToggleSessionPaletteScope),
            UiPaletteHit::Entry(entry_idx) => Some(UiAction::ExecutePaletteEntry(entry_idx)),
            UiPaletteHit::Panel => None,
            UiPaletteHit::None => Some(UiAction::ClosePalette),
        }
    }

    fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let bg_color = ThemeConfig::parse_color(&cx.config.theme.background);
        let accent = ThemeConfig::parse_color(&cx.config.theme.accent);
        let border_color = ThemeConfig::parse_color(&cx.config.theme.border_active);
        let dim_color = ThemeConfig::parse_color(&cx.config.theme.statusbar_dim);
        let fg_color = ThemeConfig::parse_color(&cx.config.theme.foreground);
        let text_color = fg_color;

        scene.bg_rects.push(Rect {
            x: 0.0,
            y: 0.0,
            w: cx.viewport_w,
            h: cx.viewport_h,
            color: [bg_color[0] * 0.5, bg_color[1] * 0.5, bg_color[2] * 0.5, 0.6],
        });

        let bw = 2.0;
        scene.bg_rects.push(Rect {
            x: self.layout.panel_x - bw,
            y: self.layout.panel_y - bw,
            w: self.layout.panel_w + bw * 2.0,
            h: self.layout.panel_h + bw * 2.0,
            color: border_color,
        });
        scene.bg_rects.push(Rect {
            x: self.layout.panel_x,
            y: self.layout.panel_y,
            w: self.layout.panel_w,
            h: self.layout.panel_h,
            color: bg_color,
        });
        scene.bg_rects.push(Rect {
            x: self.layout.panel_x,
            y: self.layout.panel_y,
            w: self.layout.panel_w,
            h: self.layout.input_row_h,
            color: [
                bg_color[0] + 0.05,
                bg_color[1] + 0.05,
                bg_color[2] + 0.05,
                1.0,
            ],
        });

        let input_text = format!("> {}", self.query);
        emit_status_text(
            scene.atlas,
            &input_text,
            &TextEmitParams {
                x_start: self.layout.text_x,
                y: self.layout.text_y,
                cell_width: cx.cell_w,
                baseline: cx.baseline,
                color: text_color,
            },
            scene.glyphs,
        );

        if let Some(toggle) = self.toggle {
            let toggle_label = if self.sessions_show_all {
                " ALL "
            } else {
                " ACTIVE "
            };
            let toggle_bg = if self.sessions_show_all {
                [accent[0], accent[1], accent[2], 0.22]
            } else {
                [accent[0], accent[1], accent[2], 0.12]
            };
            scene.bg_rects.push(Rect {
                x: toggle.bg_x,
                y: toggle.bg_y,
                w: toggle.bg_w,
                h: toggle.bg_h,
                color: toggle_bg,
            });
            emit_status_text(
                scene.atlas,
                toggle_label,
                &TextEmitParams {
                    x_start: toggle.label_x,
                    y: self.layout.text_y,
                    cell_width: cx.cell_w,
                    baseline: cx.baseline,
                    color: text_color,
                },
                scene.glyphs,
            );
        }

        let cursor_x =
            self.layout.text_x + UnicodeWidthStr::width(input_text.as_str()) as f32 * cx.cell_w;
        scene.bg_rects.push(Rect {
            x: cursor_x,
            y: self.layout.text_y,
            w: 2.0,
            h: cx.cell_h,
            color: [fg_color[0], fg_color[1], fg_color[2], 0.8],
        });
        scene.bg_rects.push(Rect {
            x: self.layout.panel_x,
            y: self.layout.sep_y - 1.0,
            w: self.layout.panel_w,
            h: 1.0,
            color: border_color,
        });

        let selected_bg = [accent[0], accent[1], accent[2], 0.25];
        let hovered_bg = [accent[0], accent[1], accent[2], 0.14];
        let remote_host_color = ThemeConfig::parse_color(&cx.config.theme.cyan);
        let remote_session_color = ThemeConfig::parse_color(&cx.config.theme.blue);
        let ssh_color = ThemeConfig::parse_color(&cx.config.theme.yellow);
        let slot_color = ThemeConfig::parse_color(&cx.config.theme.green);

        for (idx, row) in self.rows.iter().enumerate() {
            let row_y = self.layout.sep_y + idx as f32 * self.layout.row_h;
            if row.is_selected {
                scene.bg_rects.push(Rect {
                    x: self.layout.panel_x,
                    y: row_y,
                    w: self.layout.panel_w,
                    h: self.layout.row_h,
                    color: selected_bg,
                });
            } else if row.is_hovered {
                scene.bg_rects.push(Rect {
                    x: self.layout.panel_x,
                    y: row_y,
                    w: self.layout.panel_w,
                    h: self.layout.row_h,
                    color: hovered_bg,
                });
            }
            let row_color = if row.is_selected || row.is_hovered {
                text_color
            } else {
                match row.style {
                    PaletteRowStyle::Action => dim_color,
                    PaletteRowStyle::Session => dim_color,
                    PaletteRowStyle::RemoteHost => remote_host_color,
                    PaletteRowStyle::RemoteSession => remote_session_color,
                    PaletteRowStyle::SshShell => ssh_color,
                    PaletteRowStyle::SwitchSlot => slot_color,
                    PaletteRowStyle::DirectConnect => remote_host_color,
                    PaletteRowStyle::SlotSession => remote_session_color,
                }
            };
            emit_status_text(
                scene.atlas,
                &row.label,
                &TextEmitParams {
                    x_start: self.layout.text_x,
                    y: row_y + 2.0,
                    cell_width: cx.cell_w,
                    baseline: cx.baseline,
                    color: row_color,
                },
                scene.glyphs,
            );
        }

        if self.total_entries > self.layout.visible_rows {
            let track_w = 4.0;
            let track_x = self.layout.panel_x + self.layout.panel_w - 8.0;
            let track_y = self.layout.sep_y + 2.0;
            let track_h = self.layout.visible_rows as f32 * self.layout.row_h - 4.0;
            scene.bg_rects.push(Rect {
                x: track_x,
                y: track_y,
                w: track_w,
                h: track_h.max(0.0),
                color: [border_color[0], border_color[1], border_color[2], 0.20],
            });

            let thumb_h = (track_h * (self.layout.visible_rows as f32 / self.total_entries as f32))
                .max(self.layout.row_h * 0.75);
            let scroll_offset = if self.selected_idx >= self.layout.visible_rows {
                self.selected_idx - self.layout.visible_rows + 1
            } else {
                0
            };
            let thumb_y = track_y
                + (track_h - thumb_h).max(0.0)
                    * (scroll_offset as f32
                        / (self.total_entries - self.layout.visible_rows) as f32);
            scene.bg_rects.push(Rect {
                x: track_x,
                y: thumb_y,
                w: track_w,
                h: thumb_h,
                color: [accent[0], accent[1], accent[2], 0.65],
            });
        }

        let footer = if self.total_entries > 0 {
            format!("{}/{}", self.selected_idx + 1, self.total_entries)
        } else {
            "0/0".to_string()
        };
        let footer_x =
            self.layout.panel_x + self.layout.panel_w - (footer.len() as f32 * cx.cell_w) - 12.0;
        let footer_y = self.layout.panel_y + self.layout.panel_h - cx.cell_h - 2.0;
        emit_status_text(
            scene.atlas,
            &footer,
            &TextEmitParams {
                x_start: footer_x,
                y: footer_y,
                cell_width: cx.cell_w,
                baseline: cx.baseline,
                color: dim_color,
            },
            scene.glyphs,
        );

        if self.show_no_matches {
            emit_status_text(
                scene.atlas,
                "No matching commands",
                &TextEmitParams {
                    x_start: self.layout.text_x,
                    y: self.layout.sep_y + 4.0,
                    cell_width: cx.cell_w,
                    baseline: cx.baseline,
                    color: dim_color,
                },
                scene.glyphs,
            );
        }

        if let Some(ref loading) = self.loading_text {
            let loading_y = self.layout.panel_y + self.layout.panel_h - cx.cell_h * 2.0 - 4.0;
            emit_status_text(
                scene.atlas,
                loading,
                &TextEmitParams {
                    x_start: self.layout.text_x,
                    y: loading_y,
                    cell_width: cx.cell_w,
                    baseline: cx.baseline,
                    color: [accent[0], accent[1], accent[2], 0.7],
                },
                scene.glyphs,
            );
        }

        if let Some(ref error) = self.error_text {
            let error_y = self.layout.panel_y + self.layout.panel_h - cx.cell_h * 2.0 - 4.0;
            emit_status_text(
                scene.atlas,
                error,
                &TextEmitParams {
                    x_start: self.layout.text_x,
                    y: error_y,
                    cell_width: cx.cell_w,
                    baseline: cx.baseline,
                    color: {
                        let red = ThemeConfig::parse_color(&cx.config.theme.red);
                        [red[0], red[1], red[2], 0.9]
                    },
                },
                scene.glyphs,
            );
        }
    }
}
