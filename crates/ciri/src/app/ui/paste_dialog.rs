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
use ciri_ui::{Div, Layer, Styled, div, text};

const HIT_DIALOG: u64 = 1;
const HIT_PASTE: u64 = 2;
const HIT_CANCEL: u64 = 3;

fn paste_dialog_hit_from_id(hit_id: Option<u64>) -> UiPasteDialogHit {
    match hit_id {
        Some(HIT_PASTE) => UiPasteDialogHit::Paste,
        Some(HIT_CANCEL) => UiPasteDialogHit::Cancel,
        Some(HIT_DIALOG) => UiPasteDialogHit::Dialog,
        _ => UiPasteDialogHit::None,
    }
}

pub(crate) struct PasteDialogComponent {
    dx: f32,
    dy: f32,
    dialog_w: f32,
    dialog_h: f32,
    title: String,
    preview: String,
    hovered_button: Option<super::super::PasteButton>,
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

        Some(Self {
            dx,
            dy,
            dialog_w,
            dialog_h,
            title,
            preview,
            hovered_button: pending.hovered_button,
        })
    }

    pub(super) fn hit_test(&self, mx: f32, my: f32, cx: &UiContext<'_>) -> UiPasteDialogHit {
        let root = self.build_tree(cx);
        let mut shaper = ciri_ui::NullShaper;
        let out = ciri_ui::paint_tree_with_layout(
            &root,
            cx.theme,
            [cx.viewport_w, cx.viewport_h],
            1.0,
            &mut shaper,
        );
        paste_dialog_hit_from_id(out.layout.hit_test(mx, my).and_then(|n| n.hit_id))
    }
}

impl PasteDialogComponent {
    pub(crate) fn click(&self, mx: f32, my: f32, _cx: &UiContext<'_>) -> Option<UiAction> {
        match self.hit_test(mx, my, _cx) {
            UiPasteDialogHit::Paste => Some(UiAction::ConfirmPaste),
            UiPasteDialogHit::Cancel | UiPasteDialogHit::None => Some(UiAction::CancelPaste),
            UiPasteDialogHit::Dialog => None,
        }
    }

    fn build_tree(&self, cx: &UiContext<'_>) -> Div {
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
                    .hit_id(HIT_PASTE)
                    .cursor_pointer()
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
                    .hit_id(HIT_CANCEL)
                    .cursor_pointer()
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
            .hit_id(HIT_DIALOG)
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

        root
    }

    pub(crate) fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let root = self.build_tree(cx);
        paint_ui_tree(&root, cx, scene);
    }

    #[cfg(test)]
    pub(super) fn hit_bounds_for_test(
        &self,
        cx: &UiContext<'_>,
        hit: UiPasteDialogHit,
    ) -> Option<[f32; 4]> {
        let hit_id = match hit {
            UiPasteDialogHit::Paste => HIT_PASTE,
            UiPasteDialogHit::Cancel => HIT_CANCEL,
            UiPasteDialogHit::Dialog => HIT_DIALOG,
            UiPasteDialogHit::None => return None,
        };
        let root = self.build_tree(cx);
        let mut shaper = ciri_ui::NullShaper;
        let out = ciri_ui::paint_tree_with_layout(
            &root,
            cx.theme,
            [cx.viewport_w, cx.viewport_h],
            1.0,
            &mut shaper,
        );
        out.layout
            .nodes()
            .iter()
            .find(|node| node.hit_id == Some(hit_id))
            .map(|node| node.bounds)
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

    #[test]
    fn hit_test_uses_ciri_ui_layout_snapshot() {
        let mut app = make_app();
        app.core.pending_paste = Some(PendingPaste {
            info: PasteInfo {
                text: "hello".into(),
                size: 5,
                line_count: 1,
            },
            preview: "hello".into(),
            hovered_button: None,
            target: super::super::super::PendingPasteTarget::Terminal,
        });
        let theme = ciri_ui::ResolvedTheme::default();
        let cx = UiContext {
            config: &app.core.config,
            theme: &theme,
            viewport_w: 400.0,
            viewport_h: 240.0,
            cell_w: 8.0,
            cell_h: 16.0,
            baseline: 12.0,
            ui_line_h: 16.0,
            ui_shaper: None,
        };
        let dialog = PasteDialogComponent::capture(&app, &cx).expect("dialog visible");
        let paste_bounds = dialog
            .hit_bounds_for_test(&cx, UiPasteDialogHit::Paste)
            .expect("paste button hit bounds");
        let cancel_bounds = dialog
            .hit_bounds_for_test(&cx, UiPasteDialogHit::Cancel)
            .expect("cancel button hit bounds");

        assert_eq!(
            dialog.hit_test(paste_bounds[0] + 2.0, paste_bounds[1] + 2.0, &cx),
            UiPasteDialogHit::Paste
        );
        assert_eq!(
            dialog.hit_test(cancel_bounds[0] + 2.0, cancel_bounds[1] + 2.0, &cx),
            UiPasteDialogHit::Cancel
        );
        assert_eq!(
            dialog.hit_test(dialog.dx + 2.0, dialog.dy + 2.0, &cx),
            UiPasteDialogHit::Dialog
        );
        assert_eq!(dialog.hit_test(1.0, 1.0, &cx), UiPasteDialogHit::None);
    }
}
