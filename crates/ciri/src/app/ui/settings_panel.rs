//! Settings panel — modal overlay surfacing a small subset of the
//! TOML config to GUI-driven controls.
//!
//! v1 scope is intentionally narrow: a centred modal with a single
//! Appearance section, a Banner explaining live-preview semantics,
//! and an "Open settings.toml" escape hatch. Actual control wiring
//! (theme preset Dropdown, pane_opacity NumberField) lands in
//! follow-up turns; this file owns the chrome / capture / hit-test
//! plumbing the controls plug into.
//!
//! v1 limitation: changes apply live (`cached_resolved_theme.reload()`
//! and friends) but are NOT written back to `settings.toml`. The
//! Banner makes that explicit.

use super::tokens;
use super::types::{UiAction, UiContext, UiScene, UiSettingsHit, ui_hit_id};
use crate::app::App;
use crate::app::ciri_ui_adapter::paint_element_tree;
use ciri_ui::{Banner, Div, ElevationIndex, Severity, Styled, deferred, div, text};

const HIT_DIALOG: u64 = 1;
const HIT_CLOSE: u64 = 2;
const HIT_OPEN_TOML: u64 = 3;

fn settings_hit_from_id(hit_id: Option<u64>) -> UiSettingsHit {
    match hit_id {
        Some(HIT_CLOSE) => UiSettingsHit::Close,
        Some(HIT_OPEN_TOML) => UiSettingsHit::OpenToml,
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
        })
    }

    pub(super) fn hit_test(&self, mx: f32, my: f32, cx: &UiContext<'_>) -> UiSettingsHit {
        let root = self.build_tree(cx);
        settings_hit_from_id(ui_hit_id(&root, cx, mx, my))
    }

    pub(crate) fn click(&self, mx: f32, my: f32, cx: &UiContext<'_>) -> Option<UiAction> {
        match self.hit_test(mx, my, cx) {
            UiSettingsHit::Close | UiSettingsHit::None => Some(UiAction::CloseSettings),
            UiSettingsHit::OpenToml => Some(UiAction::OpenSettingsToml),
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

        let banner = Banner::new(
            "Changes preview live. Edit settings.toml to persist them across launches.",
        )
        .severity(Severity::Info)
        .into_div(theme);

        let section_header = div()
            .w(content_w)
            .h(cx.ui_line_h + tokens::SPACE_1)
            .flex_row()
            .items_center()
            .child(text("Appearance").color(theme.on_surface_muted));

        // Placeholder rows — replaced with live Dropdown / NumberField in
        // stages 2.C / 2.D. Lays out the row geometry now so the panel
        // proportions don't change when controls land.
        let preset_row = self.placeholder_row(
            cx,
            content_w,
            "Theme",
            &format!(
                "Currently {}",
                if self.current_preset.is_empty() {
                    "ciri_dark"
                } else {
                    &self.current_preset
                }
            ),
        );
        let opacity_row = self.placeholder_row(cx, content_w, "Pane Opacity", "Stepper coming…");

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

    fn placeholder_row(
        &self,
        cx: &UiContext<'_>,
        content_w: f32,
        label: &str,
        hint: &str,
    ) -> Div {
        let row_h = cx.ui_line_h + tokens::SPACE_2 * 2.0;
        div()
            .w(content_w)
            .h(row_h)
            .flex_row()
            .items_center()
            .justify_between()
            .child(text(label.to_string()).color(cx.theme.on_surface))
            .child(text(hint.to_string()).color(cx.theme.on_surface_muted))
    }

    pub(crate) fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let root = self.build_tree(cx);
        paint_element_tree(&root, cx, scene);
    }
}
