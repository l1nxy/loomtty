use ciri_config::theme::ThemeConfig;

use super::builder::UiBuilder;
use super::types::{UiAction, UiComponent, UiContext, UiPasteDialogHit, UiScene};
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
        let border_color = ThemeConfig::parse_color(&cx.config.theme.border_active);
        let accent = ThemeConfig::parse_color(&cx.config.theme.accent);
        let bw = 1.0;
        let pad = 16.0;
        let content_w = self.dialog_w - pad * 2.0;
        let btn_w = 100.0;
        let btn_h = cx.cell_h + 12.0;

        let mut ui = UiBuilder::new_vertical(
            self.dx + pad,
            self.dy + pad,
            content_w,
            self.dialog_h - pad * 2.0,
            0.0,
            0.0,
            0.0,
            false,
            cx,
            scene,
        );

        // Full-screen dimmed backdrop + dialog frame
        ui.modal_backdrop([0.0, 0.0, 0.0, 0.5]);
        ui.bordered_panel_inset(
            self.dx,
            self.dy,
            self.dialog_w,
            self.dialog_h,
            [0.12, 0.12, 0.15, 1.0],
            border_color,
            bw,
            false,
        );

        // Title
        ui.label(&self.title, [0.9, 0.9, 0.9, 1.0]);
        ui.bg_rect(content_w, 12.0, [0.0; 4]); // spacing

        // "Preview:" label
        ui.label("Preview:", [0.6, 0.6, 0.6, 1.0]);
        ui.bg_rect(content_w, 4.0, [0.0; 4]); // spacing

        // Preview box
        let (_, preview_y) = ui.cursor_pos();
        ui.abs_rect(
            self.dx + pad - 4.0,
            preview_y - 2.0,
            content_w + 8.0,
            cx.cell_h + 4.0,
            [0.08, 0.08, 0.1, 1.0],
        );
        ui.label(&self.preview, [0.6, 0.6, 0.6, 1.0]);

        // Push buttons to bottom
        ui.spacer();

        // Button row — centered horizontally
        ui.horizontal(Some(content_w), btn_h, 0.0, |ui| {
            let total_btn_w = btn_w * 2.0 + pad;
            let left_pad = (content_w - total_btn_w) / 2.0;
            ui.bg_rect(left_pad, btn_h, [0.0; 4]); // center offset

            let paste_bg = if self.hovered_button == Some(super::super::PasteButton::Paste) {
                [accent[0], accent[1], accent[2], 0.8]
            } else {
                [accent[0], accent[1], accent[2], 0.5]
            };
            let (px, py) = ui.cursor_pos();
            ui.abs_rect(px, py, btn_w, btn_h, paste_bg);
            let text_y = py + (btn_h - cx.cell_h) / 2.0;
            let text_x = px + (btn_w - ui.text_width("Paste")) / 2.0;
            ui.abs_text("Paste", text_x, text_y, [1.0, 1.0, 1.0, 1.0]);
            ui.bg_rect(btn_w, btn_h, [0.0; 4]); // advance past paste button

            ui.bg_rect(pad, btn_h, [0.0; 4]); // gap between buttons

            let cancel_bg = if self.hovered_button == Some(super::super::PasteButton::Cancel) {
                [0.4, 0.4, 0.4, 0.8]
            } else {
                [0.3, 0.3, 0.3, 0.5]
            };
            let (cx2, cy2) = ui.cursor_pos();
            ui.abs_rect(cx2, cy2, btn_w, btn_h, cancel_bg);
            let text_y = cy2 + (btn_h - cx.cell_h) / 2.0;
            let text_x = cx2 + (btn_w - ui.text_width("Cancel")) / 2.0;
            ui.abs_text("Cancel", text_x, text_y, [0.9, 0.9, 0.9, 1.0]);
        });

        ui.bg_rect(content_w, 8.0, [0.0; 4]); // bottom spacing
    }
}
