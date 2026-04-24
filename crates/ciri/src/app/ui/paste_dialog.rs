//! Paste-guard confirmation dialog.
//!
//! A modal overlay that appears when the user tries to paste content the
//! paste-guard considers risky. The backdrop dims the viewport and a centred
//! panel shows the summary, preview, and action buttons.

use super::text_layout;
use super::tokens;
use super::types::{UiAction, UiContext, UiPasteDialogHit, UiScene};
use crate::app::App;
use crate::app::ciri_ui_bridge::paint_ui_tree;
use ciri_ui::{Layer, Styled, div, text};

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
        let preview = text_layout::truncate_with_ellipsis(cx, &pending.preview, dialog_w - 32.0);
        let btn_w = 100.0;
        let btn_h = tokens::control_height_lg(cx.cell_h);
        // Mirror the paint pipeline exactly:
        //  * content starts at (dx + SPACE_4, dy + SPACE_4) with content_w = dialog_w - SPACE_4*2
        //  * the button row is horizontally centred inside content by a left-pad of
        //    (content_w - (2*btn_w + SPACE_4)) / 2
        //  * vertically, a trailing `bg_rect(_, SPACE_2)` sits below the row, so the
        //    row's bottom sits at (dy + dialog_h - SPACE_4 - SPACE_2)
        let pad = tokens::SPACE_4;
        let content_w = dialog_w - pad * 2.0;
        let left_pad = (content_w - (btn_w * 2.0 + pad)) / 2.0;
        let paste_x = dx + pad + left_pad;
        let cancel_x = paste_x + btn_w + pad;
        let btn_y = dy + dialog_h - pad - tokens::SPACE_2 - btn_h;

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
        // Half-open ranges on the right/bottom edges (`<` not `<=`), to
        // match the hit-test convention across the other modals
        // (context menu, palette).
        // Using `<=` here treated a click exactly on the trailing-edge
        // pixel as inside, which could double-book at the boundary
        // between the dialog body and an adjacent button edge.
        let (paste_x, paste_y, paste_w, paste_h) = self.paste_button;
        if mx >= paste_x && mx < paste_x + paste_w && my >= paste_y && my < paste_y + paste_h {
            return UiPasteDialogHit::Paste;
        }
        let (cancel_x, cancel_y, cancel_w, cancel_h) = self.cancel_button;
        if mx >= cancel_x && mx < cancel_x + cancel_w && my >= cancel_y && my < cancel_y + cancel_h
        {
            return UiPasteDialogHit::Cancel;
        }
        if mx >= self.dx
            && mx < self.dx + self.dialog_w
            && my >= self.dy
            && my < self.dy + self.dialog_h
        {
            return UiPasteDialogHit::Dialog;
        }
        UiPasteDialogHit::None
    }
}

impl PasteDialogComponent {
    pub(crate) fn click(&self, mx: f32, my: f32, _cx: &UiContext<'_>) -> Option<UiAction> {
        match self.hit_test(mx, my) {
            UiPasteDialogHit::Paste => Some(UiAction::ConfirmPaste),
            UiPasteDialogHit::Cancel | UiPasteDialogHit::None => Some(UiAction::CancelPaste),
            UiPasteDialogHit::Dialog => None,
        }
    }

    pub(crate) fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let border_color = cx.theme.border_focus;
        let accent = cx.theme.accent;
        let bg = cx.theme.surface;
        let fg = cx.theme.on_surface;
        let dim = cx.theme.on_surface_muted;
        let bw = tokens::BORDER_THIN;
        let pad = tokens::SPACE_4;
        let content_w = self.dialog_w - pad * 2.0;
        let btn_w = 100.0;
        let btn_h = tokens::control_height_lg(cx.cell_h);

        let surface =
            tokens::surface_raise([bg[0], bg[1], bg[2], 1.0], tokens::SURFACE_LIFT_SUBTLE);

        let recessed = tokens::surface_sink([bg[0], bg[1], bg[2], 1.0], tokens::SURFACE_SINK);
        let paste_alpha = if self.hovered_button == Some(super::super::PasteButton::Paste) {
            tokens::ALPHA_PRIMARY_HOVER
        } else {
            tokens::ALPHA_PRIMARY_REST
        };
        let cancel_alpha = if self.hovered_button == Some(super::super::PasteButton::Cancel) {
            tokens::ALPHA_SECONDARY_HOVER
        } else {
            tokens::ALPHA_SECONDARY_REST
        };

        let preview_h = cx.ui_line_h + tokens::SPACE_1 * 2.0;
        let button_row = div()
            .w_full()
            .h(btn_h)
            .flex_row()
            .items_center()
            .justify_center()
            .gap(pad)
            .child(
                div()
                    .w(btn_w)
                    .h(btn_h)
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .bg(tokens::tint(accent, paste_alpha))
                    .child(text("Paste").color(fg)),
            )
            .child(
                div()
                    .w(btn_w)
                    .h(btn_h)
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .bg(tokens::tint(fg, cancel_alpha))
                    .child(text("Cancel").color(fg)),
            );

        let panel = div()
            .in_layer(Layer::Modal)
            .w(self.dialog_w)
            .h(self.dialog_h)
            .translate(self.dx, self.dy)
            .flex_col()
            .p(pad)
            .bg(surface)
            .rounded(tokens::SPACE_1)
            .border(bw, border_color)
            .shadow_lg()
            .child(text(self.title.clone()).color(fg))
            .child(div().w(content_w).h(tokens::SPACE_3))
            .child(text("Preview:").color(dim))
            .child(div().w(content_w).h(tokens::SPACE_1))
            .child(
                div()
                    .w(content_w + tokens::SPACE_2)
                    .h(preview_h)
                    .translate(-tokens::SPACE_1, -2.0)
                    .flex_row()
                    .items_center()
                    .bg(recessed)
                    .child(div().w(tokens::SPACE_1).h(preview_h))
                    .child(text(self.preview.clone()).color(dim)),
            )
            .child(div().w(content_w).h(cx.ui_line_h + tokens::SPACE_1 * 2.0))
            .child(div().w(content_w).flex_1())
            .child(button_row)
            .child(div().w(content_w).h(tokens::SPACE_2));

        let root = div()
            .w(cx.viewport_w)
            .h(cx.viewport_h)
            .in_layer(Layer::Modal)
            .bg([0.0, 0.0, 0.0, tokens::ALPHA_BACKDROP])
            .child(panel);

        paint_ui_tree(&root, cx, scene);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::paste_guard::PasteInfo;
    use crate::app::{App, PendingPaste};
    use ciri_config::config::CiriConfig;

    fn make_app() -> App {
        App::new(CiriConfig::default(), "test-session")
    }

    #[test]
    fn preview_is_truncated_to_dialog_width() {
        let mut app = make_app();
        app.core.pending_paste = Some(PendingPaste {
            info: PasteInfo {
                text: "hello".into(),
                size: 1024,
                line_count: 1,
            },
            preview: "这是一段很长很长很长很长很长很长的预览文本".into(),
            hovered_button: None,
            target: super::super::super::PendingPasteTarget::Terminal,
        });
        let theme = ciri_ui::ResolvedTheme::default();
        let cx = UiContext {
            config: &app.core.config,
            theme: &theme,
            viewport_w: 240.0,
            viewport_h: 160.0,
            cell_w: 8.0,
            cell_h: 16.0,
            baseline: 12.0,
            ui_line_h: 16.0,
            ui_shaper: None,
        };
        let dialog = PasteDialogComponent::capture(&app, &cx).expect("dialog visible");
        assert!(text_layout::measure(&cx, &dialog.preview) <= dialog.dialog_w - 32.0 + 0.001);
    }
}
