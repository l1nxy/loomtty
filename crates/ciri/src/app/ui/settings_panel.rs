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

use super::tokens;
use super::types::{NudgeDirection, UiAction, UiContext, UiScene, UiSettingsHit, ui_hit_id};
use crate::app::App;
use crate::app::ciri_ui_adapter::paint_element_tree;
use ciri_ui::{
    Banner, Div, Dropdown, ElevationIndex, NumberField, Severity, Styled, deferred, div, text,
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
        // Clamp the panel to a comfortable reading width — too narrow on
        // small windows reads as cramped, too wide on big monitors makes
        // the form feel sparse.
        let panel_w = (cx.viewport_w * 0.6).clamp(480.0, 720.0);
        let panel_h = (cx.viewport_h * 0.7).clamp(360.0, 560.0);
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
        let pad = tokens::SPACE_4;
        let content_w = self.panel_w - pad * 2.0;

        let title_row = div()
            .w(content_w)
            .h(cx.ui_line_h + tokens::SPACE_2)
            .flex_row()
            .items_center()
            .justify_between()
            .child(text("Settings").color(theme.on_surface))
            // Close button — square sized for a single glyph; uses neutral
            // hover from `theme.element_hover` so the close target is
            // discoverable without an accent shout.
            .child(
                div()
                    .w(cx.ui_line_h + tokens::SPACE_2)
                    .h(cx.ui_line_h + tokens::SPACE_2)
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .rounded(theme.radius.sm)
                    .hit_id(HIT_CLOSE)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.element_hover))
                    .child(text("\u{00D7}").color(theme.on_surface_muted)),
            );

        // Width-constrain so the banner text doesn't overflow the panel
        // border at the minimum panel width (480 px → content_w ≈ 448 px).
        // ciri-ui's text layout overflows rather than wraps, so without
        // an explicit width the banner string runs past the panel edge.
        // Every other panel row sets `.w(content_w)`; this matches.
        let banner = Banner::new(
            "Changes preview live. Edit settings.toml to persist them across launches.",
        )
        .severity(Severity::Info)
        .into_div(theme)
        .w(content_w);

        let section_header = div()
            .w(content_w)
            .h(cx.ui_line_h + tokens::SPACE_1)
            .flex_row()
            .items_center()
            .child(text("Appearance").color(theme.on_surface_muted));

        // Theme preset row — real Dropdown trigger; click opens a
        // context_menu with preset list (handler in interaction.rs
        // populates `app.core.context_menu` at last-known mouse pos).
        let preset_label = if self.current_preset.is_empty() {
            "ciri_dark".to_string()
        } else {
            self.current_preset.clone()
        };
        let preset_row = self.control_row(
            cx,
            content_w,
            "Theme",
            Dropdown::new(HIT_THEME_DROPDOWN)
                .value(preset_label)
                .width(180.0)
                .into_div(theme),
        );
        // Pane opacity stepper — `[ - 0.85 + ]`. The display string is
        // formatted to two decimals so the same row width works for any
        // value in [0, 1]. Step size lives on the App handler (stage 2.D
        // chose 0.05) so settings rows stay value-display-only.
        let opacity_row = self.control_row(
            cx,
            content_w,
            "Pane Opacity",
            NumberField::new(HIT_OPACITY_DEC, HIT_OPACITY_INC)
                .value(format!("{:.2}", self.pane_opacity))
                .into_div(theme),
        );

        let footer = div()
            .w(content_w)
            .h(cx.ui_line_h + tokens::SPACE_2)
            .flex_row()
            .items_center()
            .justify_end()
            .child(
                div()
                    .h(cx.ui_line_h + tokens::SPACE_2)
                    .flex_row()
                    .items_center()
                    .px(tokens::SPACE_2)
                    .rounded(theme.radius.sm)
                    .hit_id(HIT_OPEN_TOML)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.element_hover))
                    .child(text("Open settings.toml").color(theme.accent)),
            );

        let panel = div()
            .absolute()
            .left(self.dx)
            .top(self.dy)
            .w(self.panel_w)
            .h(self.panel_h)
            .flex_col()
            .p(pad)
            .gap(tokens::SPACE_3)
            .bg(ElevationIndex::Modal.bg(theme))
            .rounded(theme.radius.lg)
            .border(bw, theme.border)
            .shadow_lg()
            .hit_id(HIT_DIALOG)
            .child(title_row)
            .child(banner)
            .child(section_header)
            .child(preset_row)
            .child(opacity_row)
            .child(div().w(content_w).flex_1())
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

    /// Live-control row: label on the left, supplied control element on
    /// the right. All Appearance rows share this layout so they line up
    /// vertically as more controls land.
    fn control_row(
        &self,
        cx: &UiContext<'_>,
        content_w: f32,
        label: &str,
        control: Div,
    ) -> Div {
        let row_h = cx.ui_line_h + tokens::SPACE_2 * 2.0;
        div()
            .w(content_w)
            .h(row_h)
            .flex_row()
            .items_center()
            .justify_between()
            .child(text(label.to_string()).color(cx.theme.on_surface))
            .child(control)
    }

    pub(crate) fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let root = self.build_tree(cx);
        paint_element_tree(&root, cx, scene);
    }
}
