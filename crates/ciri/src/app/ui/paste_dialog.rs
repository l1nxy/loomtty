use ciri_config::theme::ThemeConfig;
use ciri_render::rect::Rect;

use super::types::{UiAction, UiComponent, UiContext, UiPasteDialogHit, UiScene};
use crate::app::status_bar::{TextEmitParams, emit_status_text};
use crate::app::App;

pub(crate) struct PasteDialogComponent {
    dx: f32,
    dy: f32,
    dialog_w: f32,
    dialog_h: f32,
    title: String,
    preview: String,
    hovered_button: Option<super::super::PasteButton>,
    pub paste_button: (f32, f32, f32, f32),
    pub cancel_button: (f32, f32, f32, f32),
}

impl PasteDialogComponent {
    pub fn capture(app: &App, cx: &UiContext<'_>) -> Option<Self> {
        let pending = app.core.pending_paste.as_ref()?;
        let dialog_w = cx.viewport_w * 0.6;
        let dialog_h = cx.viewport_h * 0.4;
        let dx = (cx.viewport_w - dialog_w) / 2.0;
        let dy = (cx.viewport_h - dialog_h) / 2.0;
        let title = format!(
            "Are you sure you want to paste {} ({} lines)?",
            super::super::paste_guard::format_size(pending.info.size),
            pending.info.line_count
        );
        let max_chars = ((dialog_w - 32.0) / cx.cell_w) as usize;
        let preview = if pending.preview.len() > max_chars {
            format!(
                "{}...",
                &pending.preview[..pending
                    .preview
                    .floor_char_boundary(max_chars.saturating_sub(3))]
            )
        } else {
            pending.preview.clone()
        };
        let btn_w = 100.0;
        let btn_h = cx.cell_h + 12.0;
        let btn_y = dy + dialog_h - 16.0 - btn_h;
        let paste_x = dx + dialog_w / 2.0 - btn_w - 16.0;
        let cancel_x = dx + dialog_w / 2.0 + 16.0;

        Some(Self {
            dx,
            dy,
            dialog_w,
            dialog_h,
            title,
            preview,
            hovered_button: pending.hovered_button,
            paste_button: (paste_x, btn_y, btn_w, btn_h),
            cancel_button: (cancel_x, btn_y, btn_w, btn_h),
        })
    }

    pub(super) fn hit_test(&self, mx: f32, my: f32) -> UiPasteDialogHit {
        let (paste_x, paste_y, paste_w, paste_h) = self.paste_button;
        if mx >= paste_x && mx <= paste_x + paste_w && my >= paste_y && my <= paste_y + paste_h {
            return UiPasteDialogHit::Paste;
        }
        let (cancel_x, cancel_y, cancel_w, cancel_h) = self.cancel_button;
        if mx >= cancel_x
            && mx <= cancel_x + cancel_w
            && my >= cancel_y
            && my <= cancel_y + cancel_h
        {
            return UiPasteDialogHit::Cancel;
        }
        if mx >= self.dx
            && mx <= self.dx + self.dialog_w
            && my >= self.dy
            && my <= self.dy + self.dialog_h
        {
            return UiPasteDialogHit::Dialog;
        }
        UiPasteDialogHit::None
    }
}

impl UiComponent for PasteDialogComponent {
    fn click(&self, mx: f32, my: f32, _cx: &UiContext<'_>) -> Option<UiAction> {
        match self.hit_test(mx, my) {
            UiPasteDialogHit::Paste => Some(UiAction::ConfirmPaste),
            UiPasteDialogHit::Cancel | UiPasteDialogHit::None => Some(UiAction::CancelPaste),
            UiPasteDialogHit::Dialog => None,
        }
    }

    fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        scene.bg_rects.push(Rect {
            x: 0.0,
            y: 0.0,
            w: cx.viewport_w,
            h: cx.viewport_h,
            color: [0.0, 0.0, 0.0, 0.5],
        });
        scene.bg_rects.push(Rect {
            x: self.dx,
            y: self.dy,
            w: self.dialog_w,
            h: self.dialog_h,
            color: [0.12, 0.12, 0.15, 1.0],
        });

        let border_color = ThemeConfig::parse_color(&cx.config.theme.border_active);
        let border = 1.0;
        scene.bg_rects.push(Rect {
            x: self.dx,
            y: self.dy,
            w: self.dialog_w,
            h: border,
            color: border_color,
        });
        scene.bg_rects.push(Rect {
            x: self.dx,
            y: self.dy + self.dialog_h - border,
            w: self.dialog_w,
            h: border,
            color: border_color,
        });
        scene.bg_rects.push(Rect {
            x: self.dx,
            y: self.dy,
            w: border,
            h: self.dialog_h,
            color: border_color,
        });
        scene.bg_rects.push(Rect {
            x: self.dx + self.dialog_w - border,
            y: self.dy,
            w: border,
            h: self.dialog_h,
            color: border_color,
        });

        let text_x = self.dx + 16.0;
        let mut text_y = self.dy + 16.0;
        emit_status_text(
            scene.atlas,
            &self.title,
            &TextEmitParams {
                x_start: text_x,
                y: text_y,
                cell_width: cx.cell_w,
                baseline: cx.baseline,
                color: [0.9, 0.9, 0.9, 1.0],
            },
            scene.glyphs,
        );
        text_y += cx.cell_h + 12.0;
        emit_status_text(
            scene.atlas,
            "Preview:",
            &TextEmitParams {
                x_start: text_x,
                y: text_y,
                cell_width: cx.cell_w,
                baseline: cx.baseline,
                color: [0.6, 0.6, 0.6, 1.0],
            },
            scene.glyphs,
        );
        text_y += cx.cell_h + 4.0;
        scene.bg_rects.push(Rect {
            x: text_x - 4.0,
            y: text_y - 2.0,
            w: self.dialog_w - 24.0,
            h: cx.cell_h + 4.0,
            color: [0.08, 0.08, 0.1, 1.0],
        });
        emit_status_text(
            scene.atlas,
            &self.preview,
            &TextEmitParams {
                x_start: text_x,
                y: text_y,
                cell_width: cx.cell_w,
                baseline: cx.baseline,
                color: [0.6, 0.6, 0.6, 1.0],
            },
            scene.glyphs,
        );

        let accent = ThemeConfig::parse_color(&cx.config.theme.accent);
        let (paste_x, btn_y, btn_w, btn_h) = self.paste_button;
        let (cancel_x, _, _, _) = self.cancel_button;
        let paste_bg = if self.hovered_button == Some(super::super::PasteButton::Paste) {
            [accent[0], accent[1], accent[2], 0.8]
        } else {
            [accent[0], accent[1], accent[2], 0.5]
        };
        scene.bg_rects.push(Rect {
            x: paste_x,
            y: btn_y,
            w: btn_w,
            h: btn_h,
            color: paste_bg,
        });
        emit_status_text(
            scene.atlas,
            "Paste",
            &TextEmitParams {
                x_start: paste_x + (btn_w - cx.cell_w * 5.0) / 2.0,
                y: btn_y + (btn_h - cx.cell_h) / 2.0,
                cell_width: cx.cell_w,
                baseline: cx.baseline,
                color: [1.0, 1.0, 1.0, 1.0],
            },
            scene.glyphs,
        );

        let cancel_bg = if self.hovered_button == Some(super::super::PasteButton::Cancel) {
            [0.4, 0.4, 0.4, 0.8]
        } else {
            [0.3, 0.3, 0.3, 0.5]
        };
        scene.bg_rects.push(Rect {
            x: cancel_x,
            y: btn_y,
            w: btn_w,
            h: btn_h,
            color: cancel_bg,
        });
        emit_status_text(
            scene.atlas,
            "Cancel",
            &TextEmitParams {
                x_start: cancel_x + (btn_w - cx.cell_w * 6.0) / 2.0,
                y: btn_y + (btn_h - cx.cell_h) / 2.0,
                cell_width: cx.cell_w,
                baseline: cx.baseline,
                color: [0.9, 0.9, 0.9, 1.0],
            },
            scene.glyphs,
        );
    }
}
