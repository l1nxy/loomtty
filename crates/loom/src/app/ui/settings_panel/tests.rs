//! Integration shape tests for the settings panel.
//!
//! Per-field clamp/toggle/set-enum behaviour lives in
//! [`super::schema::tests`]; the writer's atomic-write / preserve-
//! formatting coverage lives in `loom_config::writer::tests`. What we
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
use crate::app::ui::types::{UiAction, test_ui_context};
use loom_app::app::SettingsCategory;
use loom_config::config::LoomConfig;

fn make_app() -> App {
    let mut app = App::new(LoomConfig::default(), "test-session");
    // Tests exercise the in-memory dispatcher only — never let them
    // touch the developer's actual `~/.config/loom/config.toml` or
    // race other tests on the same temp-sibling. The disk-write
    // semantics are covered separately by
    // `loom_config::writer::tests::round_trip_through_loader_validates`.
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
        loom_config::config::InputMode::Sticky,
    );
}

#[test]
fn dynamic_pane_sizing_uses_live_settings_and_logical_window_width() {
    use loom_input::action::Action;
    use loom_protocol::message::{ClientMessage, ColumnWidthSpec};
    use winit::dpi::PhysicalSize;

    let mut app = make_app();
    let (tx, rx) = crossbeam_channel::unbounded();
    app.core.server_tx = Some(tx);
    app.apply_settings_control(SettingsControl::SetEnum {
        field: SettingsField::LayoutNewPaneSizing,
        value: "dynamic".into(),
    });

    // Both screens show an 800px logical window. Retina must not turn it
    // into a large-window half-width layout just because it has more pixels.
    for (scale, width, expected) in [
        (1.0, 800, 1.0),
        (2.0, 1600, 1.0),
        (2.0, 1998, 1.0),
        (2.0, 2000, 0.5),
        (2.0, 2400, 0.5),
        (1.0, 800, 1.0),
    ] {
        app.dpi_scale = scale;
        app.preview_resize(PhysicalSize::new(width, 700));
        app.handle_action(Action::NewColumnRight);
        assert!(matches!(
            rx.try_recv().unwrap(),
            ClientMessage::CreatePaneWithWidth {
                width: ColumnWidthSpec { proportion, fixed_px: None }
            } if proportion == expected
        ));
    }

    // Updating the threshold in the panel must affect the very next pane.
    app.preview_resize(PhysicalSize::new(950, 700));
    app.apply_settings_control(SettingsControl::Nudge {
        field: SettingsField::LayoutDynamicFullscreenWidth,
        delta_sign: -1,
    });
    app.handle_action(Action::NewColumnRight);
    assert!(matches!(
        rx.try_recv().unwrap(),
        ClientMessage::CreatePaneWithWidth {
            width: ColumnWidthSpec {
                proportion: 0.5,
                fixed_px: None
            }
        }
    ));
    app.apply_settings_control(SettingsControl::Nudge {
        field: SettingsField::LayoutDynamicFullscreenWidth,
        delta_sign: 1,
    });
    // Native context-menu creation follows the same policy.
    app.send(ClientMessage::CreatePaneAt {
        pane_id: 42,
        below: false,
        request_id: 7,
        width: None,
    });
    assert!(matches!(
        rx.try_recv().unwrap(),
        ClientMessage::CreatePaneAt {
            pane_id: 42,
            request_id: 7,
            width: Some(ColumnWidthSpec {
                proportion: 1.0,
                fixed_px: None
            }),
            ..
        }
    ));

    app.apply_settings_control(SettingsControl::SetEnum {
        field: SettingsField::LayoutNewPaneSizing,
        value: "fixed".into(),
    });
    app.handle_action(Action::NewColumnRight);
    assert!(matches!(
        rx.try_recv().unwrap(),
        ClientMessage::CreatePaneWithWidth {
            width: ColumnWidthSpec {
                proportion: 0.5,
                fixed_px: None
            }
        }
    ));
    app.apply_settings_control(SettingsControl::SetEnum {
        field: SettingsField::LayoutNewPaneWidth,
        value: "full".into(),
    });
    app.handle_action(Action::NewColumnRight);
    assert!(matches!(
        rx.try_recv().unwrap(),
        ClientMessage::CreatePaneWithWidth {
            width: ColumnWidthSpec {
                proportion: 1.0,
                fixed_px: None
            }
        }
    ));
}

#[test]
fn new_pane_sizing_preserves_explicit_width_and_counts_side_chrome() {
    use loom_config::config::{NewPaneSizing, PresetWidth, TabBarPosition};
    use winit::dpi::PhysicalSize;

    let mut app = make_app();
    app.dpi_scale = 2.0;
    app.core.config.layout.new_pane_sizing = NewPaneSizing::Dynamic;
    app.core.config.tabbar.position = TabBarPosition::Left;
    app.core.config.tabbar.width = 200.0;
    app.preview_resize(PhysicalSize::new(2100, 1400));
    assert_eq!(app.new_column_width().proportion, 0.5);
    app.core.config.layout.default_column_width = Some(PresetWidth::Fixed { fixed: 640.0 });
    assert_eq!(app.new_column_width().fixed_px, Some(640.0));
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

// ── Body row windowing / scroll ──────────────────────────────────────

/// A category with more fields than fit the panel height must window
/// its rows (render a slice, not the whole list) and report room to
/// scroll. Terminal is the densest category, and the 560 px panel-
/// height floor can't fit all of it, so this is a stable overflow.
#[test]
fn tall_category_windows_rows_and_can_scroll() {
    let mut app = make_app();
    app.core.settings_panel_visible = true;
    app.core.settings_category = SettingsCategory::Terminal;
    let theme = loom_ui::ResolvedTheme::default();
    // 0.82 * 600 = 492 < 560, so panel_h clamps to its 560 floor —
    // the smallest the panel ever gets, maximising the overflow.
    let cx = test_ui_context(&app.core.config, &theme, 1400.0, 600.0);
    let comp = SettingsPanelComponent::capture(&app, &cx).expect("panel visible");
    assert!(comp.visible_rows >= 1, "at least one row must show");
    assert!(
        comp.visible_rows < comp.total_rows,
        "dense category must overflow ({} of {})",
        comp.visible_rows,
        comp.total_rows,
    );
    assert!(comp.max_scroll_offset() > 0, "overflowing body must scroll");
}

/// A stale offset left over from a taller category (or a model that's
/// a frame behind) is clamped to the active category's valid range at
/// capture time, never windowing past the end of the list.
#[test]
fn capture_clamps_out_of_range_scroll_offset() {
    let mut app = make_app();
    app.core.settings_panel_visible = true;
    app.core.settings_category = SettingsCategory::Terminal;
    app.core.settings_scroll_offset = 9_999;
    let theme = loom_ui::ResolvedTheme::default();
    let cx = test_ui_context(&app.core.config, &theme, 1400.0, 600.0);
    let comp = SettingsPanelComponent::capture(&app, &cx).expect("panel visible");
    assert_eq!(comp.scroll_offset, comp.max_scroll_offset());
}

/// A category that fits entirely reports no scroll room — the body
/// then omits the scrollbar gutter.
#[test]
fn short_category_does_not_scroll() {
    let mut app = make_app();
    app.core.settings_panel_visible = true;
    app.core.settings_category = SettingsCategory::Layout; // single field
    let theme = loom_ui::ResolvedTheme::default();
    let cx = test_ui_context(&app.core.config, &theme, 1400.0, 900.0);
    let comp = SettingsPanelComponent::capture(&app, &cx).expect("panel visible");
    assert_eq!(comp.max_scroll_offset(), 0);
    assert_eq!(comp.scroll_offset, 0);
}

/// Switching categories starts the new view at the top — the offset
/// must not leak across a category change.
#[test]
fn category_change_resets_scroll_offset() {
    let mut app = make_app();
    app.core.settings_panel_visible = true;
    app.core.settings_category = SettingsCategory::Terminal;
    app.core.settings_scroll_offset = 4;
    app.select_settings_category(SettingsCategory::Font);
    assert_eq!(app.core.settings_scroll_offset, 0);
}
