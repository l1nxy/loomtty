//! Single dispatcher for every settings-panel mutation.
//!
//! The panel emits one of three semantic actions: nudge a stepper,
//! toggle a switch, pick an enum value. Each lands here, runs through
//! the schema's read/write helpers, persists to disk via the writer,
//! and triggers the side-effects the changed field requires (theme
//! re-resolve, render-cache invalidate, redraw).
//!
//! Why one function instead of N `UiAction` variants and N dispatcher
//! arms: every settings action shares the same envelope —
//! `(field, mutate, persist, redraw)`. Splitting it per field would
//! mean dozens of boilerplate dispatcher arms for what is structurally
//! one operation. This keeps the per-action overhead in `apply_ui_action`
//! to two arms (`SettingsControl` + `SelectSettingsCategory`).

use super::schema::{self, SettingsField};
use crate::app::App;
use ciri_app::app::SettingsCategory;

/// Semantic action for any settings mutation. Originates either from
/// a click on a settings row (panel side) or from the context_menu
/// dispatcher (theme dropdown / enum picker).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SettingsControl {
    /// Stepper press — `delta_sign` is +1 or -1.
    Nudge {
        field: SettingsField,
        delta_sign: i32,
    },
    /// Switch tap — flip the bool.
    Toggle { field: SettingsField },
    /// Enum picker resolved — set the field to the given variant.
    SetEnum { field: SettingsField, value: String },
}

impl App {
    /// Apply a settings-panel mutation. Persists to disk and triggers
    /// all the live-preview side effects the changed field needs.
    /// Silent no-op when the mutation didn't actually change the value
    /// (e.g. clicking `+` on a stepper at its ceiling).
    pub(crate) fn apply_settings_control(&mut self, control: SettingsControl) {
        let changed = match &control {
            SettingsControl::Nudge { field, delta_sign } => {
                schema::nudge(*field, &mut self.core.config, *delta_sign)
            }
            SettingsControl::Toggle { field } => {
                schema::toggle(*field, &mut self.core.config).is_some()
            }
            SettingsControl::SetEnum { field, value } => {
                if let SettingsField::ThemePreset = field {
                    // Theme preset has its own dedicated apply path
                    // that re-resolves the chrome (palette colours,
                    // SDF rect cache, etc.); reuse it instead of
                    // duplicating the side-effect chain here.
                    self.apply_theme_preset(value.clone());
                    self.persist_field(SettingsField::ThemePreset);
                    return;
                }
                schema::set_enum(*field, &mut self.core.config, value)
            }
        };
        if !changed {
            return;
        }
        let field = match &control {
            SettingsControl::Nudge { field, .. }
            | SettingsControl::Toggle { field }
            | SettingsControl::SetEnum { field, .. } => *field,
        };
        self.apply_settings_side_effects(field);
        self.persist_field(field);
    }

    /// Trigger the live-preview side effects a field change requires
    /// (cache invalidates, font reload, etc.). Most fields just need
    /// a render-cache clear + redraw; the exceptions are listed here.
    fn apply_settings_side_effects(&mut self, field: SettingsField) {
        match field {
            SettingsField::FontSize
            | SettingsField::FontFamily
            | SettingsField::FontWeight
            | SettingsField::FontLineHeight
            | SettingsField::UiFontFamily
            | SettingsField::UiFontSize => {
                // Any of these changes the cell metrics or the active
                // face — `apply_font_config_change` re-runs font
                // discovery and rasterisation, which is the only path
                // that picks up a new family/weight/line-height (and
                // it also rebuilds the UI shaper).
                self.apply_font_config_change();
                self.schedule_redraw();
            }
            SettingsField::InputMode => {
                self.reload_input_config();
                self.clear_render_caches();
                self.schedule_redraw();
            }
            SettingsField::PredictionMode => {
                self.update_prediction_config();
                self.clear_render_caches();
                self.schedule_redraw();
            }
            _ => {
                self.clear_render_caches();
                self.schedule_redraw();
            }
        }
    }

    /// Write a single field back to settings.toml. Logs and continues
    /// on error — the in-memory live preview is independent of the
    /// disk write, so a transient I/O failure shouldn't roll back the
    /// user's UI change.
    fn persist_field(&mut self, field: SettingsField) {
        // Test isolation: when persistence is off, exercise only the
        // in-memory mutation path. Without this gate the dispatcher
        // would call `EditableConfig::load()` -> `config_path()`,
        // creating / mutating the developer's real `~/.config/ciri/
        // config.toml` and racing the user-wide temp-sibling path
        // across parallel tests.
        if !self.persist_settings_to_disk {
            return;
        }
        match ciri_config::writer::EditableConfig::load() {
            Ok(mut w) => {
                schema::write_to_disk(field, &self.core.config, &mut w);
                if let Err(e) = w.save() {
                    log::warn!("settings panel: failed to persist {field:?}: {e}",);
                    return;
                }
                // Suppress the imminent file-watcher tick: we already
                // hold the new value in memory, and reload_config()
                // would re-parse the file we just wrote (cheap, but
                // would also clobber any other in-flight live-preview
                // edits the user is mid-stepping through).
                self.suppress_next_config_reload();
            }
            Err(e) => {
                log::warn!("settings panel: failed to open settings.toml for write: {e}",);
            }
        }
    }

    /// Sidebar nav — change which category the panel renders.
    pub(crate) fn select_settings_category(&mut self, cat: SettingsCategory) {
        if self.core.settings_category == cat {
            return;
        }
        self.core.settings_category = cat;
        self.schedule_redraw();
    }
}
