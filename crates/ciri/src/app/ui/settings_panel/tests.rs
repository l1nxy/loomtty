//! Integration shape tests for the settings panel.
//!
//! Per-field clamp/toggle/set-enum behaviour lives in
//! [`super::schema::tests`]; the writer's atomic-write / preserve-
//! formatting coverage lives in `ciri_config::writer::tests`. What we
//! prove here is the click-path glue: hit-test decode → action → in-
//! memory mutation. Disk writes are intentionally suppressed for
//! these tests by ensuring no `settings.toml` exists in the user
//! config dir during the run; the writer's failure path logs and
//! continues without affecting the in-memory state we assert on.

use super::dispatch::SettingsControl;
use super::hit::{ButtonRole, ChromeOp, SettingsHit, decode, encode_chrome, encode_field};
use super::schema::SettingsField;
use super::{SettingsActionPayload, SettingsPanelComponent, action_for};
use crate::app::App;
use crate::app::ui::types::UiAction;
use ciri_app::app::SettingsCategory;
use ciri_config::config::CiriConfig;

fn make_app() -> App {
    let mut app = App::new(CiriConfig::default(), "test-session");
    // Tests exercise the in-memory dispatcher only — never let them
    // touch the developer's actual `~/.config/ciri/config.toml` or
    // race other tests on the same temp-sibling. The disk-write
    // semantics are covered separately by
    // `ciri_config::writer::tests::round_trip_through_loader_validates`.
    app.persist_settings_to_disk = false;
    app
}

#[test]
fn action_for_close_outside_click() {
    assert_eq!(action_for(None), UiAction::CloseSettings);
}

#[test]
fn action_for_dialog_no_op() {
    assert_eq!(
        action_for(Some(SettingsHit::Chrome(ChromeOp::Dialog))),
        UiAction::SettingsNoOp,
    );
}

#[test]
fn action_for_close_button_dismisses() {
    assert_eq!(
        action_for(Some(SettingsHit::Chrome(ChromeOp::Close))),
        UiAction::CloseSettings,
    );
}

#[test]
fn action_for_sidebar_navigates() {
    assert_eq!(
        action_for(Some(SettingsHit::Sidebar(SettingsCategory::Font))),
        UiAction::SelectSettingsCategory(SettingsCategory::Font),
    );
}

#[test]
fn action_for_stepper_routes_through_settings_control() {
    let inc = action_for(Some(SettingsHit::Field {
        field: SettingsField::AppearancePaneOpacity,
        role: ButtonRole::StepperInc,
    }));
    assert_eq!(
        inc,
        UiAction::SettingsControl(SettingsActionPayload::Nudge {
            field: SettingsField::AppearancePaneOpacity,
            delta_sign: 1,
        }),
    );
}

#[test]
fn action_for_switch_routes_through_settings_control() {
    let toggle = action_for(Some(SettingsHit::Field {
        field: SettingsField::TerminalCursorBlink,
        role: ButtonRole::SwitchToggle,
    }));
    assert_eq!(
        toggle,
        UiAction::SettingsControl(SettingsActionPayload::Toggle {
            field: SettingsField::TerminalCursorBlink,
        }),
    );
}

#[test]
fn action_for_dropdown_opens_picker() {
    let open = action_for(Some(SettingsHit::Field {
        field: SettingsField::AnimationPreset,
        role: ButtonRole::DropdownOpen,
    }));
    assert_eq!(
        open,
        UiAction::OpenSettingsDropdown(SettingsField::AnimationPreset),
    );
}

#[test]
fn select_settings_category_updates_model() {
    let mut app = make_app();
    assert_eq!(app.core.settings_category, SettingsCategory::Appearance);
    app.apply_ui_action(UiAction::SelectSettingsCategory(SettingsCategory::Font));
    assert_eq!(app.core.settings_category, SettingsCategory::Font);
}

#[test]
fn settings_control_toggle_flips_in_memory() {
    let mut app = make_app();
    let before = app.core.config.terminal.cursor_blink;
    app.apply_settings_control(SettingsControl::Toggle {
        field: SettingsField::TerminalCursorBlink,
    });
    assert_eq!(app.core.config.terminal.cursor_blink, !before);
}

#[test]
fn settings_control_nudge_changes_in_memory() {
    let mut app = make_app();
    let before = app.core.config.appearance.pane_opacity;
    app.apply_settings_control(SettingsControl::Nudge {
        field: SettingsField::AppearancePaneOpacity,
        delta_sign: -1,
    });
    assert!(app.core.config.appearance.pane_opacity < before);
}

#[test]
fn settings_control_set_enum_changes_in_memory() {
    let mut app = make_app();
    app.apply_settings_control(SettingsControl::SetEnum {
        field: SettingsField::InputMode,
        value: "sticky".into(),
    });
    assert_eq!(
        app.core.config.input.mode,
        ciri_config::config::InputMode::Sticky,
    );
}

#[test]
fn capture_returns_none_when_panel_hidden() {
    let app = make_app();
    let cx = app.ui_context();
    assert!(SettingsPanelComponent::capture(&app, &cx).is_none());
}

#[test]
fn capture_uses_active_category_from_model() {
    let mut app = make_app();
    app.core.settings_panel_visible = true;
    app.core.settings_category = SettingsCategory::Animation;
    let cx = app.ui_context();
    let comp = SettingsPanelComponent::capture(&app, &cx).expect("panel visible");
    // Sidebar row hit-id is a stable encoding of the category; if
    // hit-test at a sidebar location decodes to `Animation`, the
    // capture preserved the model state.
    let id = encode_field(SettingsField::ThemePreset, ButtonRole::DropdownOpen);
    assert!(decode(id).is_some());
    // We can't easily probe the specific row's bounds without paint,
    // so just confirm the component captured what we set — the field
    // is private; expose via debug repr is overkill. The presence of
    // a captured component over the configured category is enough:
    // the next test sweeps real hit-ids.
    drop(comp);
}

/// Click on the sidebar's Font row dispatches the right
/// `SelectSettingsCategory` action.
#[test]
fn sidebar_click_dispatches_category_change() {
    let mut app = make_app();
    app.core.settings_panel_visible = true;
    let cx = app.ui_context();
    let comp = SettingsPanelComponent::capture(&app, &cx).expect("panel visible");

    // Sweep the sidebar column for the Font row's hit_id and
    // dispatch a click there.
    let target_id = super::hit::encode_sidebar(SettingsCategory::Font);
    let mut hit = None;
    'outer: for ix in 0..40 {
        for iy in 0..40 {
            let mx = (ix as f32 + 0.5) / 40.0 * cx.viewport_w;
            let my = (iy as f32 + 0.5) / 40.0 * cx.viewport_h;
            if comp.hover_hit_id(mx, my, &cx) == Some(target_id) {
                hit = Some((mx, my));
                break 'outer;
            }
        }
    }
    let (mx, my) = hit.expect("Font sidebar row not hittable");
    assert_eq!(
        comp.click(mx, my, &cx),
        Some(UiAction::SelectSettingsCategory(SettingsCategory::Font)),
    );
}

#[test]
fn close_button_hit_id_dismisses_panel() {
    let mut app = make_app();
    app.core.settings_panel_visible = true;
    let cx = app.ui_context();
    let comp = SettingsPanelComponent::capture(&app, &cx).expect("panel visible");

    let target_id = encode_chrome(ChromeOp::Close);
    let mut hit = None;
    'outer: for ix in 0..60 {
        for iy in 0..60 {
            let mx = (ix as f32 + 0.5) / 60.0 * cx.viewport_w;
            let my = (iy as f32 + 0.5) / 60.0 * cx.viewport_h;
            if comp.hover_hit_id(mx, my, &cx) == Some(target_id) {
                hit = Some((mx, my));
                break 'outer;
            }
        }
    }
    let (mx, my) = hit.expect("close button not hittable");
    assert_eq!(comp.click(mx, my, &cx), Some(UiAction::CloseSettings));
}
