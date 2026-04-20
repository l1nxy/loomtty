//! Paste-guard confirmation dialog.
//!
//! A modal overlay that appears when the user tries to paste content the
//! paste-guard considers risky (large, multi-line, password-shaped). The
//! backdrop dims the entire viewport and a centred panel shows the size
//! summary, a one-line preview, and two buttons (Paste / Cancel).
//!
//! Lifecycle:
//!   - `App.core.pending_paste = Some(..)` — dialog is visible.
//!   - Button clicks produce `UiAction::ConfirmPaste` / `CancelPaste`;
//!     the hovered button is read from `pending.hovered_button` so hover
//!     state survives full repaints.
//!   - Any click outside the dialog cancels, matching the old legacy
//!     path (see `click` below).

use ciri_ui::color::{scale_rgb, with_alpha};
use ciri_ui::{div, text, Div, Layer, ResolvedTheme, Styled};

use super::tokens;
use super::types::{UiAction, UiContext, UiPasteDialogHit};
use crate::app::App;

pub(crate) struct PasteDialogComponent {
    dx: f32,
    dy: f32,
    dialog_w: f32,
    dialog_h: f32,
    title: String,
    preview: String,
    hovered_button: Option<super::super::PasteButton>,
    /// Viewport size — captured so `build_tree` can paint the modal
    /// backdrop at full size without re-reading `UiContext`.
    viewport_w: f32,
    viewport_h: f32,
    /// Button dimensions — `btn_w` is the constant `100.0`, `btn_h`
    /// derives from `cx.cell_h` via `control_height_lg`. Stored so
    /// `build_tree` doesn't need the ui-chrome-specific `UiContext`.
    btn_w: f32,
    btn_h: f32,
    /// Height of the recessed preview row. `cx.cell_h + SPACE_1` in
    /// the legacy layout; captured here for the same reason as `btn_h`.
    preview_h: f32,
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
        let btn_h = tokens::control_height_lg(cx.cell_h);
        let btn_y = dy + dialog_h - tokens::SPACE_4 - btn_h;
        let paste_x = dx + dialog_w / 2.0 - btn_w - tokens::SPACE_4;
        let cancel_x = dx + dialog_w / 2.0 + tokens::SPACE_4;

        Some(Self {
            dx,
            dy,
            dialog_w,
            dialog_h,
            title,
            preview,
            hovered_button: pending.hovered_button,
            viewport_w: cx.viewport_w,
            viewport_h: cx.viewport_h,
            btn_w,
            btn_h,
            preview_h: cx.cell_h + tokens::SPACE_1,
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

    /// Map a click to a `UiAction`. Preserves the legacy semantics:
    /// clicking outside the panel cancels, clicking on the panel body
    /// (but not on a button) is a no-op, and clicking a button fires
    /// the matching action.
    pub(crate) fn click(&self, mx: f32, my: f32, _cx: &UiContext<'_>) -> Option<UiAction> {
        match self.hit_test(mx, my) {
            UiPasteDialogHit::Paste => Some(UiAction::ConfirmPaste),
            UiPasteDialogHit::Cancel | UiPasteDialogHit::None => Some(UiAction::CancelPaste),
            UiPasteDialogHit::Dialog => None,
        }
    }
}

impl PasteDialogComponent {
    /// Build the ciri-ui tree for this captured dialog snapshot.
    ///
    /// The tree is a full-viewport modal root whose own background is
    /// the dim backdrop; a centred panel sits inside via
    /// `items_center`/`justify_center`. All emission happens on
    /// `Layer::Modal` so the dialog out-z-orders any non-modal chrome.
    pub(crate) fn build_tree(&self, theme: &ResolvedTheme) -> Div {
        // Surface is nudged slightly brighter than the terminal
        // background so it reads as a raised panel over the dimmed
        // viewport — matches the legacy `bg + 0.03` recipe closely
        // enough using the multiplicative helper on the theme.
        let surface = scale_rgb(theme.term_bg, 1.15);
        // Recessed preview slot is one step darker than the panel.
        let recessed = scale_rgb(theme.term_bg, 0.85);

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
        let paste_bg = with_alpha(theme.accent, paste_alpha);
        let cancel_bg = with_alpha(theme.on_surface, cancel_alpha);

        // Button row: `justify_center` with `gap = 2 × SPACE_4` places
        // the two buttons at the exact x-offsets `capture` wrote into
        // `paste_button` / `cancel_button`, so the visible tree and
        // the hit-test rects stay in lockstep.
        let button = |label: &'static str, bg: ciri_ui::Color| {
            div()
                .flex_row()
                .items_center()
                .justify_center()
                .w(self.btn_w)
                .h(self.btn_h)
                .bg(bg)
                .child(text(label).color(theme.on_surface))
        };
        let button_row = div()
            .flex_row()
            .justify_center()
            .gap(tokens::SPACE_4 * 2.0)
            .w(self.dialog_w - tokens::SPACE_4 * 2.0)
            .child(button("Paste", paste_bg))
            .child(button("Cancel", cancel_bg));

        let preview_box = div()
            .flex_row()
            .items_center()
            .w(self.dialog_w - tokens::SPACE_4 * 2.0)
            .h(self.preview_h)
            .bg(recessed)
            .child(text(&self.preview).color(theme.on_surface_muted));

        // Panel placement is driven by the parent flex (`items_center`
        // + `justify_center`): at `viewport_w × viewport_h` with a
        // `dialog_w × dialog_h` child this yields exactly the `(dx, dy)`
        // top-left that `capture` computed — no manual `translate` is
        // needed, and adding one would double-offset the panel.
        let panel = div()
            .flex_col()
            .w(self.dialog_w)
            .h(self.dialog_h)
            .bg(surface)
            .border(tokens::BORDER_THIN, theme.border_focus)
            .p(tokens::SPACE_4)
            .gap(tokens::SPACE_1)
            .child(text(&self.title).color(theme.on_surface))
            .child(div().h(tokens::SPACE_2)) // spacer between title and Preview:
            .child(text("Preview:").color(theme.on_surface_muted))
            .child(preview_box)
            .child(div().flex_1()) // push button row to the bottom
            .child(button_row);

        // Modal root = full-viewport backdrop + centred panel. Putting
        // the backdrop on the root's own `bg` saves one child element
        // and avoids a stacking-order footgun (the panel is a sibling
        // of the backdrop in flex order).
        div()
            .in_layer(Layer::Modal)
            .w(self.viewport_w)
            .h(self.viewport_h)
            .flex_col()
            .items_center()
            .justify_center()
            .bg([0.0, 0.0, 0.0, tokens::ALPHA_BACKDROP])
            .child(panel)
    }
}
