//! Settings panel — modal overlay surfacing a small subset of the
//! TOML config to GUI-driven controls.
//!
//! v1 scope: a centred modal with one Appearance section, a Banner
//! explaining live-preview semantics, two live controls (theme preset
//! Dropdown + pane opacity NumberField stepper), and an "Open
//! settings.toml" escape hatch. Click on a preset re-resolves the
//! chrome theme via `apply_theme_preset`; clicks on the +/- buttons
//! step `appearance.pane_opacity` clamped to [0.05, 1.0].
//!
//! v1 limitation: changes apply live (`cached_resolved_theme.reload`,
//! `clear_render_caches`, etc.) but are NOT written back to
//! `settings.toml`. The Banner makes that explicit; the footer link
//! opens the file (or its parent directory) so users can persist via
//! manual edit.

use super::text_layout;
use super::tokens;
use super::types::{NudgeDirection, UiAction, UiContext, UiScene, UiSettingsHit, ui_hit_id};
use crate::app::App;
use crate::app::ciri_ui_adapter::paint_element_tree;
use ciri_ui::{
    Banner, Div, Dropdown, NumberField, Severity, Styled, deferred, div, text,
};

const HIT_DIALOG: u64 = 1;
const HIT_CLOSE: u64 = 2;
const HIT_OPEN_TOML: u64 = 3;
const HIT_THEME_DROPDOWN: u64 = 4;
// Opacity stepper hit_ids are `pub(super)` so `frame::active_press_hit_id`
// can return them — the steppers need press feedback (panel stays open
// after each nudge, full repaint between events) unlike dismiss-on-press
// chrome (close button, "Open settings.toml" link).
pub(super) const HIT_OPACITY_DEC: u64 = 5;
pub(super) const HIT_OPACITY_INC: u64 = 6;

/// Test-only accessor for the Dialog hit_id so the
/// `settings_dialog_body_click_is_no_op` regression in `ui/mod.rs` can
/// verify the click landed on the panel body (not the backdrop) without
/// re-publishing the constant for production use.
#[cfg(test)]
pub(super) fn settings_panel_hit_dialog_for_test() -> u64 {
    HIT_DIALOG
}

fn settings_hit_from_id(hit_id: Option<u64>) -> UiSettingsHit {
    match hit_id {
        Some(HIT_CLOSE) => UiSettingsHit::Close,
        Some(HIT_OPEN_TOML) => UiSettingsHit::OpenToml,
        Some(HIT_THEME_DROPDOWN) => UiSettingsHit::ThemeDropdown,
        Some(HIT_OPACITY_DEC) => UiSettingsHit::PaneOpacityDec,
        Some(HIT_OPACITY_INC) => UiSettingsHit::PaneOpacityInc,
        Some(HIT_DIALOG) => UiSettingsHit::Dialog,
        _ => UiSettingsHit::None,
    }
}

pub(crate) struct SettingsPanelComponent {
    dx: f32,
    dy: f32,
    panel_w: f32,
    panel_h: f32,
    /// Snapshot of the current preset name so the placeholder row can
    /// show what's active. Wired to the working Dropdown in stage 2.C.
    current_preset: String,
    /// Snapshot of `appearance.pane_opacity` for the NumberField
    /// display. The +/- buttons dispatch through `UiAction` so the
    /// step size lives on `App` (stage 2.D).
    pane_opacity: f32,
}

impl SettingsPanelComponent {
    pub fn capture(app: &App, cx: &UiContext<'_>) -> Option<Self> {
        if !app.core.settings_panel_visible {
            return None;
        }
        // Settings panel is a real workbench surface, not a pop-up tip
        // — give it room. Aim for ~70% of the viewport with sensible
        // floors and a generous ceiling so the form has breathing space
        // even on a 4K monitor.
        let panel_w = (cx.viewport_w * 0.7).clamp(640.0, 1080.0);
        let panel_h = (cx.viewport_h * 0.75).clamp(440.0, 760.0);
        let dx = ((cx.viewport_w - panel_w) / 2.0).max(0.0);
        let dy = ((cx.viewport_h - panel_h) / 2.0).max(0.0);
        Some(Self {
            dx,
            dy,
            panel_w,
            panel_h,
            current_preset: app.core.config.theme.preset.clone(),
            pane_opacity: app.core.config.appearance.pane_opacity,
        })
    }

    pub(super) fn hit_test(&self, mx: f32, my: f32, cx: &UiContext<'_>) -> UiSettingsHit {
        let root = self.build_tree(cx);
        settings_hit_from_id(ui_hit_id(&root, cx, mx, my))
    }

    /// Raw hit_id under the cursor — used by `App::current_settings_hover`
    /// for chrome-cache-hash dirty tracking. Returning the u64 directly
    /// (rather than the `UiSettingsHit` enum) keeps the hash output
    /// stable across enum reorderings and matches the hit_id surface
    /// the hover walker uses internally.
    pub(crate) fn hover_hit_id(&self, mx: f32, my: f32, cx: &UiContext<'_>) -> Option<u64> {
        let root = self.build_tree(cx);
        ui_hit_id(&root, cx, mx, my)
    }

    pub(crate) fn click(&self, mx: f32, my: f32, cx: &UiContext<'_>) -> Option<UiAction> {
        match self.hit_test(mx, my, cx) {
            UiSettingsHit::Close | UiSettingsHit::None => Some(UiAction::CloseSettings),
            UiSettingsHit::OpenToml => Some(UiAction::OpenSettingsToml),
            UiSettingsHit::ThemeDropdown => Some(UiAction::OpenThemeDropdown),
            UiSettingsHit::PaneOpacityDec => {
                Some(UiAction::NudgePaneOpacity(NudgeDirection::Decrement))
            }
            UiSettingsHit::PaneOpacityInc => {
                Some(UiAction::NudgePaneOpacity(NudgeDirection::Increment))
            }
            UiSettingsHit::Dialog => None,
        }
    }

    fn build_tree(&self, cx: &UiContext<'_>) -> Div {
        let theme = cx.theme;
        let bw = tokens::BORDER_THIN;
        // Generous padding so the title and content don't sit flush
        // against the rounded panel corners.
        let pad_x = tokens::SPACE_4;
        let pad_y = tokens::SPACE_3;
        let content_w = self.panel_w - pad_x * 2.0;

        let title_h = cx.ui_line_h + tokens::SPACE_3;
        let close_size = title_h - tokens::SPACE_1;
        let title_bar = div()
            .w(self.panel_w)
            .h(title_h)
            .flex_row()
            .items_center()
            .justify_between()
            .px(pad_x)
            .child(text("Settings").color(theme.on_surface))
            .child(
                div()
                    .w(close_size)
                    .h(close_size)
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .rounded(theme.radius.sm)
                    .hit_id(HIT_CLOSE)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.element_hover))
                    .active(|s| s.bg(theme.element_active))
                    .child(text("\u{00D7}").color(theme.on_surface_muted)),
            );
        // Hairline divider directly under the title — gives the title
        // row visual weight (otherwise it floats indistinguishably above
        // the content).
        let title_divider = div().w(self.panel_w).h(bw).bg(theme.border);

        let banner = Banner::new(
            "Changes preview live. Edit settings.toml to persist them across launches.",
        )
        .severity(Severity::Info)
        .into_div(theme)
        .w(content_w);

        // Section header: uppercased + accent-muted to read as a label
        // line, with a subtle hairline underline so groups visually
        // separate when more sections land later.
        let section_header = div()
            .w(content_w)
            .flex_col()
            .gap(tokens::SPACE_1)
            .child(
                div()
                    .w(content_w)
                    .h(cx.ui_line_h + tokens::SPACE_1)
                    .flex_row()
                    .items_center()
                    .child(text("APPEARANCE").color(theme.on_surface_muted)),
            )
            .child(div().w(content_w).h(bw).bg(theme.border));

        // Dropdown trigger sized for the longest preset name with a
        // little headroom; the displayed value is pre-truncated by the
        // shape-aware text_layout helper so it never overflows the
        // value column even when the user runs a large UI font.
        const DROPDOWN_W: f32 = 320.0;
        // Mirrors `Dropdown::PAD_X` (12) and the chevron column width
        // (ROW_H, 40). Keep these constants in sync if Dropdown's
        // metrics change, otherwise the pre-truncated value will
        // either underfill or clip into the chevron.
        const DROPDOWN_VALUE_BUDGET: f32 = DROPDOWN_W - 12.0 * 2.0 - 40.0;
        let preset_raw = if self.current_preset.is_empty() {
            "ciri_dark"
        } else {
            self.current_preset.as_str()
        };
        let preset_label = text_layout::truncate_with_ellipsis(cx, preset_raw, DROPDOWN_VALUE_BUDGET);
        let preset_row = self.control_row(
            cx,
            content_w,
            "Theme",
            "Colour scheme for the chrome and terminal palette.",
            Dropdown::new(HIT_THEME_DROPDOWN)
                .value(preset_label)
                .width(DROPDOWN_W)
                .into_div(theme),
        );
        let opacity_row = self.control_row(
            cx,
            content_w,
            "Pane Opacity",
            "Translucency of pane backgrounds over the wallpaper.",
            NumberField::new(HIT_OPACITY_DEC, HIT_OPACITY_INC)
                .value(format!("{:.2}", self.pane_opacity))
                .into_div(theme),
        );

        // Body: section + rows share a smaller intra-group gap; the
        // outer panel uses a larger gap to space title / banner / body
        // / footer.
        let body = div()
            .w(content_w)
            .flex_col()
            .gap(tokens::SPACE_3)
            .child(banner)
            .child(
                div()
                    .w(content_w)
                    .flex_col()
                    .gap(tokens::SPACE_2)
                    .child(section_header)
                    .child(preset_row)
                    .child(opacity_row),
            );

        let footer_divider = div().w(self.panel_w).h(bw).bg(theme.border);
        let footer = div()
            .w(self.panel_w)
            .h(title_h)
            .flex_row()
            .items_center()
            .justify_end()
            .px(pad_x)
            .child(
                div()
                    .h(close_size)
                    .flex_row()
                    .items_center()
                    .px(tokens::SPACE_2)
                    .rounded(theme.radius.sm)
                    .hit_id(HIT_OPEN_TOML)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.element_hover))
                    .active(|s| s.bg(theme.element_active))
                    .child(text("Open settings.toml").color(theme.accent)),
            );

        // Body wrapper carries the side / vertical padding so it
        // doesn't extend over the title / footer dividers.
        let body_wrapper = div()
            .w(self.panel_w)
            .flex_col()
            .px(pad_x)
            .py(pad_y)
            .flex_1()
            .child(body);

        let panel = div()
            .absolute()
            .left(self.dx)
            .top(self.dy)
            .w(self.panel_w)
            .h(self.panel_h)
            .flex_col()
            // Panel bg = chrome surface (light tier). Controls inside
            // use `surface_sunken` so they recess against this lift —
            // gives a 2-tier hierarchy without changing the global
            // ElevationIndex mapping (paste_dialog / palette stay at
            // their sunk Modal tier).
            .bg(theme.surface)
            .rounded(theme.radius.lg)
            .border(bw, theme.border)
            .shadow_lg()
            .hit_id(HIT_DIALOG)
            .child(title_bar)
            .child(title_divider)
            .child(body_wrapper)
            .child(footer_divider)
            .child(footer);

        // Backdrop dim catches outside-clicks; the panel is wrapped in
        // `deferred()` so it stacks above the dim layer without needing
        // a separate Layer enum.
        div()
            .w(cx.viewport_w)
            .h(cx.viewport_h)
            .bg([0.0, 0.0, 0.0, tokens::ALPHA_BACKDROP])
            .child(deferred(panel))
    }

    /// Zed-style settings row: label sits on the same horizontal line
    /// as the control (so the eye reads "this label → this widget"
    /// without scanning vertically), description flows on the line
    /// below as supplementary context. The previous layout stacked
    /// label+description on the left and centred the control against
    /// that whole block, which made the control look visually
    /// detached — floating in the middle of two lines of text.
    fn control_row(
        &self,
        cx: &UiContext<'_>,
        content_w: f32,
        label: &str,
        description: &str,
        control: Div,
    ) -> Div {
        let theme = cx.theme;
        div()
            .w(content_w)
            .flex_col()
            .gap(tokens::SPACE_1)
            .child(
                // Top line: label ←→ control. items_center vertically
                // aligns the single-line label against the taller
                // control (e.g. 40 px Dropdown), so the label's text
                // baseline lands on the control's mid-line.
                div()
                    .w(content_w)
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(text(label.to_string()).color(theme.on_surface))
                    .child(control),
            )
            .child(
                // Description sits beneath, kept muted so the row's
                // primary visual mass stays on the top line. ciri-ui's
                // text engine doesn't wrap — a long description at the
                // 640 px minimum panel width with a proportional UI
                // font would otherwise spill past the panel body
                // (label "Colour scheme for the chrome and terminal
                // palette." rendered ≈ 720 px at 14 px). Pre-truncate
                // against the row's content_w so the rendered string
                // always fits, using the same shaper-backed measure
                // helper paint sees so hit-test stays in sync.
                text(text_layout::truncate_with_ellipsis(cx, description, content_w))
                    .color(theme.on_surface_muted),
            )
    }

    pub(crate) fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let root = self.build_tree(cx);
        paint_element_tree(&root, cx, scene);
    }
}
