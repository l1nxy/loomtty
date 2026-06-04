//! Settings panel — Zed-style modal: left sidebar of category tabs,
//! right body of rows for the active category, top title bar with
//! close, footer with "Open settings.toml".
//!
//! Architecture (avoid the hand-rolled per-row sprawl earlier
//! iterations of this panel had):
//!
//! - [`schema`] holds a static table of every settable row, plus
//!   read/nudge/toggle/set-enum/write helpers. Adding a new row is
//!   one entry.
//! - [`hit`] encodes hit-ids as `(tag, payload, role)` tuples so the
//!   panel doesn't need a constant per control.
//! - [`dispatch`] is the single `App::apply_settings_control`
//!   entrypoint that mutates, persists, and triggers side effects.
//! - This module just paints what the schema describes; click and
//!   hit-test decode the hit-id and forward to the dispatcher.

pub(crate) mod dispatch;
pub(crate) mod hit;
pub(crate) mod schema;

#[cfg(test)]
mod tests;

use super::text_layout;
use super::tokens;
use super::types::{UiAction, UiContext, UiScene, ui_hit_id};
use crate::app::App;
use crate::app::loom_ui_adapter::paint_element_tree;
use loom_app::app::SettingsCategory;
use loom_config::config::LoomConfig;
use loom_ui::{Banner, Div, Dropdown, NumberField, Severity, Styled, Switch, deferred, div, text};

use hit::{ButtonRole, ChromeOp, SettingsHit, decode, encode_chrome, encode_field, encode_sidebar};
use schema::{FIELDS, FieldKind, FieldMeta, SettingsField};

/// Sidebar column width. Wide enough for the longest category label
/// ("Animations Enabled" wraps to two words; "Status Bar" is the
/// widest single label) at the standard UI font.
const SIDEBAR_W: f32 = 184.0;
/// Width of a stepper / dropdown control column in a row. Drives the
/// width of every interactive control in the panel — dropdown
/// trigger, number-field stepper, and the right-aligned switch slot
/// all measure `CONTROL_COL_W × CONTROL_ROW_H` so the right edges
/// align cleanly down the panel, regardless of which widget the row
/// uses.
const CONTROL_COL_W: f32 = 240.0;
/// Height of the control column slot. Matches the `Dropdown` /
/// `NumberField` `ROW_H` constant so the row geometry comes out
/// uniform when the field_row centers its children vertically.
const CONTROL_ROW_H: f32 = 48.0;

pub(crate) struct SettingsPanelComponent {
    dx: f32,
    dy: f32,
    panel_w: f32,
    panel_h: f32,
    /// Snapshot of the active sidebar tab so capture/click/hover all
    /// see the same category even if the model changes mid-frame.
    active_category: SettingsCategory,
    /// Owned config snapshot — `read_*` helpers in the schema module
    /// take `&LoomConfig`, and the panel needs a frozen view across
    /// the multiple build_tree() calls (capture / hit_test / paint).
    config_snapshot: LoomConfig,
}

impl SettingsPanelComponent {
    pub fn capture(app: &App, cx: &UiContext<'_>) -> Option<Self> {
        if !app.core.settings_panel_visible {
            return None;
        }
        let panel_w = (cx.viewport_w * 0.82).clamp(840.0, 1320.0);
        let panel_h = (cx.viewport_h * 0.82).clamp(560.0, 900.0);
        let dx = ((cx.viewport_w - panel_w) / 2.0).max(0.0);
        let dy = ((cx.viewport_h - panel_h) / 2.0).max(0.0);
        Some(Self {
            dx,
            dy,
            panel_w,
            panel_h,
            active_category: app.core.settings_category,
            config_snapshot: app.core.config.clone(),
        })
    }

    pub(super) fn hit_test(&self, mx: f32, my: f32, cx: &UiContext<'_>) -> Option<SettingsHit> {
        self.hover_hit_id(mx, my, cx).and_then(decode)
    }

    pub(crate) fn hover_hit_id(&self, mx: f32, my: f32, cx: &UiContext<'_>) -> Option<u64> {
        let root = self.build_tree(cx);
        ui_hit_id(&root, cx, mx, my)
    }

    pub(crate) fn click(&self, mx: f32, my: f32, cx: &UiContext<'_>) -> Option<UiAction> {
        let hit = self.hit_test(mx, my, cx);
        Some(action_for(hit))
    }

    pub(crate) fn paint(&self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        let root = self.build_tree(cx);
        paint_element_tree(&root, cx, scene);
    }

    fn build_tree(&self, cx: &UiContext<'_>) -> Div {
        let theme = cx.theme;
        let bw = tokens::BORDER_THIN;
        let pad_x = tokens::SPACE_4;
        let pad_y = tokens::SPACE_3;

        let title_bar = self.build_title_bar(cx);
        let title_divider = div().w(self.panel_w).h(bw).bg(theme.border);

        let banner = Banner::new("Changes apply live and are saved to settings.toml.")
            .severity(Severity::Info)
            .into_div(theme)
            .w(self.panel_w - pad_x * 2.0);

        let sidebar = self.build_sidebar(cx);
        let body = self.build_body(cx);

        // Row layout: sidebar | hairline | body. The hairline relies
        // on flex-row's default `align-items: stretch` to fill the
        // body's vertical extent so the seam reaches from banner to
        // footer-divider — no explicit height needed.
        let split = div()
            .w(self.panel_w - pad_x * 2.0)
            .flex_row()
            .flex_1()
            .child(sidebar)
            .child(div().w(bw).bg(theme.border))
            .child(body);

        let body_wrapper = div()
            .w(self.panel_w)
            .flex_col()
            .px(pad_x)
            .py(pad_y)
            .gap(tokens::SPACE_3)
            .flex_1()
            .child(banner)
            .child(split);

        let footer_divider = div().w(self.panel_w).h(bw).bg(theme.border);
        let footer = self.build_footer(cx);

        let panel = div()
            .absolute()
            .left(self.dx)
            .top(self.dy)
            .w(self.panel_w)
            .h(self.panel_h)
            .flex_col()
            .bg(theme.surface)
            .rounded(theme.radius.lg)
            .border(bw, theme.border)
            .shadow_lg()
            .hit_id(encode_chrome(ChromeOp::Dialog))
            .child(title_bar)
            .child(title_divider)
            .child(body_wrapper)
            .child(footer_divider)
            .child(footer);

        div()
            .w(cx.viewport_w)
            .h(cx.viewport_h)
            .bg([0.0, 0.0, 0.0, tokens::ALPHA_BACKDROP])
            .child(deferred(panel))
    }

    fn build_title_bar(&self, cx: &UiContext<'_>) -> Div {
        let theme = cx.theme;
        let title_h = cx.ui_line_h + tokens::SPACE_3;
        let close_size = title_h - tokens::SPACE_1;
        div()
            .w(self.panel_w)
            .h(title_h)
            .flex_row()
            .items_center()
            .justify_between()
            .px(tokens::SPACE_4)
            .child(text("Settings").color(theme.on_surface))
            .child(
                div()
                    .w(close_size)
                    .h(close_size)
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .rounded(theme.radius.sm)
                    .hit_id(encode_chrome(ChromeOp::Close))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.element_hover))
                    .active(|s| s.bg(theme.element_active))
                    .child(text("\u{00D7}").color(theme.on_surface_muted)),
            )
    }

    fn build_sidebar(&self, cx: &UiContext<'_>) -> Div {
        let mut col = div()
            .w(SIDEBAR_W)
            .flex_col()
            .gap(tokens::SPACE_1)
            .pr(tokens::SPACE_2);
        for cat in SettingsCategory::ALL {
            col = col.child(self.sidebar_row(cx, *cat));
        }
        col
    }

    fn sidebar_row(&self, cx: &UiContext<'_>, cat: SettingsCategory) -> Div {
        let theme = cx.theme;
        let active = cat == self.active_category;
        let row_h = cx.ui_line_h + tokens::SPACE_2;
        // Active row uses the accent tint at `ALPHA_SELECTED_BG` so the
        // selected category reads as the dominant cue without a
        // border. Hover layers a lighter tint over inactive rows.
        let bg = if active {
            tokens::tint(theme.accent, tokens::ALPHA_SELECTED_BG)
        } else {
            [0.0, 0.0, 0.0, 0.0]
        };
        let label_color = if active {
            theme.on_surface
        } else {
            theme.on_surface_muted
        };
        div()
            .w(SIDEBAR_W - tokens::SPACE_2)
            .h(row_h)
            .flex_row()
            .items_center()
            .px(tokens::SPACE_3)
            .rounded(theme.radius.sm)
            .bg(bg)
            .hit_id(encode_sidebar(cat))
            .cursor_pointer()
            .hover(|s| s.bg(tokens::tint(theme.accent, tokens::ALPHA_HOVER_BG)))
            .child(text(cat.label()).color(label_color))
    }

    fn build_body(&self, cx: &UiContext<'_>) -> Div {
        let theme = cx.theme;
        let bw = tokens::BORDER_THIN;
        let body_w = self.panel_w - tokens::SPACE_4 * 2.0 - SIDEBAR_W - bw;
        let mut col = div()
            .flex_col()
            .flex_1()
            .pl(tokens::SPACE_3)
            .child(
                // Section header — uppercased category name, hairline
                // underline. Same shape as the previous panel's
                // single section header so the visual rhythm carries.
                div()
                    .flex_col()
                    .gap(tokens::SPACE_1)
                    .child(
                        div()
                            .h(cx.ui_line_h + tokens::SPACE_1)
                            .flex_row()
                            .items_center()
                            .child(
                                text(self.active_category.label().to_uppercase())
                                    .color(theme.on_surface_muted),
                            ),
                    )
                    .child(div().w(body_w).h(bw).bg(theme.border)),
            );
        // Rows + interstitial hairline dividers. Body has no gap so the
        // divider sits directly between rows; each row carries its own
        // vertical padding for breathing room.
        let rows: Vec<&FieldMeta> = FIELDS
            .iter()
            .filter(|m| m.category == self.active_category)
            .collect();
        let last_idx = rows.len().saturating_sub(1);
        for (i, meta) in rows.iter().enumerate() {
            col = col.child(self.field_row(cx, meta, body_w));
            if i != last_idx {
                col = col.child(div().w(body_w).h(bw).bg(theme.border));
            }
        }
        col
    }

    fn field_row(&self, cx: &UiContext<'_>, meta: &FieldMeta, content_w: f32) -> Div {
        let theme = cx.theme;
        let control = self.field_control(cx, meta);
        // Left column: label stacked over description. Width is what's
        // left after reserving the control column + a gutter; the
        // description ellipsises if it doesn't fit. This keeps the
        // control flush against the right edge while the text wraps to
        // the left.
        let label_col_w = (content_w - CONTROL_COL_W - tokens::SPACE_4).max(0.0);
        let label_col = div()
            .flex_col()
            .w(label_col_w)
            .gap(tokens::SPACE_1)
            .child(text(meta.label).color(theme.on_surface))
            .child(
                text(text_layout::truncate_with_ellipsis(
                    cx,
                    meta.description,
                    label_col_w,
                ))
                .color(theme.on_surface_muted),
            );
        // Outer row: `items_center` aligns the control's midpoint with
        // the midpoint of the (label + description) column — Zed-style
        // row geometry rather than baseline-on-label.
        div()
            .w(content_w)
            .flex_row()
            .items_center()
            .justify_between()
            .py(tokens::SPACE_3)
            .child(label_col)
            .child(control)
    }

    fn field_control(&self, cx: &UiContext<'_>, meta: &FieldMeta) -> Div {
        let theme = cx.theme;
        // Build the actual control. NumberField and Dropdown stretch
        // to fill `CONTROL_COL_W`; Switch keeps its native 36×20
        // toggle silhouette and gets right-aligned inside the slot.
        let inner = match meta.kind {
            FieldKind::Float { .. } | FieldKind::Int { .. } => NumberField::new(
                encode_field(meta.field, ButtonRole::StepperDec),
                encode_field(meta.field, ButtonRole::StepperInc),
            )
            .value(schema::display_value(meta.field, &self.config_snapshot))
            .width(CONTROL_COL_W)
            .into_div(theme),
            FieldKind::Bool => Switch::new(encode_field(meta.field, ButtonRole::SwitchToggle))
                .checked(schema::read_bool(meta.field, &self.config_snapshot))
                .into_div(theme),
            FieldKind::Enum { .. } => {
                let raw = schema::display_value(meta.field, &self.config_snapshot);
                // Pre-truncate against the dropdown's value column so
                // long preset names ("loom_dark" + checkmark prefix
                // when picked, "catppuccin_mocha", …) don't bleed
                // into the chevron. Reserves `CONTROL_ROW_H` for the
                // chevron column (matches `Dropdown`'s internal
                // `chevron_col_w = ROW_H`).
                const VALUE_BUDGET: f32 = CONTROL_COL_W - 12.0 * 2.0 - CONTROL_ROW_H;
                let label = text_layout::truncate_with_ellipsis(cx, &raw, VALUE_BUDGET);
                Dropdown::new(encode_field(meta.field, ButtonRole::DropdownOpen))
                    .value(label)
                    .width(CONTROL_COL_W)
                    .into_div(theme)
            }
        };
        // Unified slot: every control sits inside the same
        // `CONTROL_COL_W × CONTROL_ROW_H` box, right-aligned and
        // vertically centered. Dropdown / NumberField already fill the
        // box; Switch hugs the right edge so toggles align with the
        // right edge of the wider controls down the column.
        div()
            .w(CONTROL_COL_W)
            .h(CONTROL_ROW_H)
            .flex_none()
            .flex_row()
            .items_center()
            .justify_end()
            .child(inner)
    }

    fn build_footer(&self, cx: &UiContext<'_>) -> Div {
        let theme = cx.theme;
        let footer_h = cx.ui_line_h + tokens::SPACE_3;
        let close_size = footer_h - tokens::SPACE_1;
        div()
            .w(self.panel_w)
            .h(footer_h)
            .flex_row()
            .items_center()
            .justify_end()
            .px(tokens::SPACE_4)
            .child(
                div()
                    .h(close_size)
                    .flex_row()
                    .items_center()
                    .px(tokens::SPACE_2)
                    .rounded(theme.radius.sm)
                    .hit_id(encode_chrome(ChromeOp::OpenToml))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.element_hover))
                    .active(|s| s.bg(theme.element_active))
                    .child(text("Open settings.toml").color(theme.accent)),
            )
    }

    /// Horizontal center of the field-control column in viewport
    /// coordinates. Callers anchoring an enum-picker popup over a
    /// trigger use this so the popup's midpoint matches the trigger's
    /// midpoint instead of extending sideways from the cursor click.
    ///
    /// Mirrors the geometry produced by `build_body`: each row is
    /// rendered at `body_w` wide inside a body that has `pl(SPACE_3)`,
    /// so the row's right edge sits `SPACE_3` past the body's content
    /// area; the trigger is right-aligned within the row with width
    /// `CONTROL_COL_W`.
    pub(crate) fn control_center_x(&self) -> f32 {
        let trigger_right = self.dx + self.panel_w - tokens::SPACE_4 + tokens::SPACE_3;
        trigger_right - CONTROL_COL_W / 2.0
    }

    /// Whether the click at `(mx, my)` lands on a stepper button —
    /// used by `frame::active_press_hit_id` to opt those buttons into
    /// `.active()` press-state styling. (Toggle / dropdown trigger
    /// dismiss-on-click flows skip press tint, mirroring v1's
    /// dec/inc-only opt-in.)
    pub(super) fn stepper_hit_id_at(&self, mx: f32, my: f32, cx: &UiContext<'_>) -> Option<u64> {
        let root = self.build_tree(cx);
        let raw = ui_hit_id(&root, cx, mx, my)?;
        match decode(raw)? {
            SettingsHit::Field {
                role: ButtonRole::StepperDec | ButtonRole::StepperInc,
                ..
            } => Some(raw),
            _ => None,
        }
    }
}

/// Translate a hit-id decoding into a `UiAction`. `None` (outside the
/// panel) closes the panel — same shape as the v1 backdrop dismiss.
pub(crate) fn action_for(hit: Option<SettingsHit>) -> UiAction {
    match hit {
        None => UiAction::CloseSettings,
        Some(SettingsHit::Chrome(op)) => match op {
            ChromeOp::Dialog => UiAction::SettingsNoOp,
            ChromeOp::Close => UiAction::CloseSettings,
            ChromeOp::OpenToml => UiAction::OpenSettingsToml,
        },
        Some(SettingsHit::Sidebar(cat)) => UiAction::SelectSettingsCategory(cat),
        Some(SettingsHit::Field { field, role }) => match role {
            ButtonRole::StepperDec => UiAction::SettingsControl(SettingsActionPayload::Nudge {
                field,
                delta_sign: -1,
            }),
            ButtonRole::StepperInc => UiAction::SettingsControl(SettingsActionPayload::Nudge {
                field,
                delta_sign: 1,
            }),
            ButtonRole::SwitchToggle => {
                UiAction::SettingsControl(SettingsActionPayload::Toggle { field })
            }
            ButtonRole::DropdownOpen => UiAction::OpenSettingsDropdown(field),
        },
    }
}

/// Wire payload for the panel-side `UiAction::SettingsControl`. Kept
/// separate from `dispatch::SettingsControl` so the UI types module
/// doesn't have to know about the dispatcher's enum (the dispatcher
/// is `pub(crate)` of this module; the UI layer is `pub(super)`-style
/// glue).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SettingsActionPayload {
    Nudge {
        field: SettingsField,
        delta_sign: i32,
    },
    Toggle {
        field: SettingsField,
    },
}

impl SettingsActionPayload {
    pub(crate) fn into_dispatch(self) -> dispatch::SettingsControl {
        match self {
            Self::Nudge { field, delta_sign } => {
                dispatch::SettingsControl::Nudge { field, delta_sign }
            }
            Self::Toggle { field } => dispatch::SettingsControl::Toggle { field },
        }
    }
}
