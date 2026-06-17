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
use loom_app::app::SettingsCategory;
use loom_config::web_token_is_usable;
use loom_protocol::message::ClientMessage;

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

impl SettingsControl {
    /// The field this control mutates.
    fn field(&self) -> SettingsField {
        match self {
            SettingsControl::Nudge { field, .. }
            | SettingsControl::Toggle { field }
            | SettingsControl::SetEnum { field, .. } => *field,
        }
    }
}

impl App {
    /// Apply a settings-panel mutation. Persists to disk and triggers
    /// all the live-preview side effects the changed field needs.
    /// Silent no-op when the mutation didn't actually change the value
    /// (e.g. clicking `+` on a stepper at its ceiling).
    pub(crate) fn apply_settings_control(&mut self, control: SettingsControl) {
        let field = control.field();
        // Serialize web edits: while one web change awaits the daemon's reply,
        // refuse another. `pending_web_rollback` is a single slot we can't
        // correlate to a specific reply (`CommandResult` carries no request id),
        // so overlapping edits could let an early success clear a later edit's
        // rollback. Replies are near-instant for a local daemon, so this is only
        // a momentary guard. (Cleared on the reply, and on disconnect.)
        if matches!(
            field,
            SettingsField::WebEnabled | SettingsField::WebBind | SettingsField::WebPort
        ) && self.pending_web_rollback.is_some()
        {
            self.send_desktop_notification(
                "Web settings",
                "A web change is still pending — try again in a moment.",
            );
            return;
        }
        // Snapshot the web sub-config before mutating so a bind/port edit that
        // fails validation while the gateway is live can be rolled back whole.
        let web_snapshot = matches!(field, SettingsField::WebBind | SettingsField::WebPort)
            .then(|| self.core.config.web.clone());

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
        if matches!(field, SettingsField::WebEnabled) {
            self.apply_web_enabled_toggle();
            return;
        }
        if let Some(prev_web) = web_snapshot {
            self.apply_web_runtime_field_change(field, prev_web);
            return;
        }
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
            | SettingsField::UiFontSize
            | SettingsField::FontCellWidth
            | SettingsField::FontDisableLigatures => {
                // Any of these changes the cell metrics or the active
                // face / shaping — `apply_font_config_change` re-runs
                // font discovery and rasterisation, which is the only
                // path that picks up a new family/weight/line-height/
                // cell-width/ligature policy (and it also rebuilds the
                // UI shaper). Underline / strikethrough adjustments are
                // render-time decorations, so those fall through to the
                // default cache-clear arm instead.
                self.apply_font_config_change();
                self.schedule_redraw();
            }
            SettingsField::InputMode
            | SettingsField::InputLeaderTimeout
            | SettingsField::InputDoubleTapWindow => {
                self.reload_input_config();
                self.clear_render_caches();
                self.schedule_redraw();
            }
            SettingsField::PredictionMode
            | SettingsField::PredictionThreshold
            | SettingsField::PredictionShowUnderline => {
                self.update_prediction_config();
                self.clear_render_caches();
                self.schedule_redraw();
            }
            SettingsField::WebEnabled => {
                self.send_current_web_settings();
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
    fn persist_field(&mut self, field: SettingsField) -> bool {
        // Test isolation: when persistence is off, exercise only the
        // in-memory mutation path. Without this gate the dispatcher
        // would call `EditableConfig::load()` -> `config_path()`,
        // creating / mutating the developer's real `~/.config/loom/
        // config.toml` and racing the user-wide temp-sibling path
        // across parallel tests.
        if !self.persist_settings_to_disk {
            return true;
        }
        match loom_config::writer::EditableConfig::load() {
            Ok(mut w) => {
                schema::write_to_disk(field, &self.core.config, &mut w);
                if let Err(e) = w.save() {
                    log::warn!("settings panel: failed to persist {field:?}: {e}",);
                    return false;
                }
                // Suppress the imminent file-watcher tick: we already
                // hold the new value in memory, and reload_config()
                // would re-parse the file we just wrote (cheap, but
                // would also clobber any other in-flight live-preview
                // edits the user is mid-stepping through).
                self.suppress_next_config_reload();
                true
            }
            Err(e) => {
                log::warn!("settings panel: failed to open settings.toml for write: {e}",);
                false
            }
        }
    }

    /// Persist the whole `[web]` sub-config (enabled/bind/port/token) in one
    /// open→write→save, returning whether the on-disk write stuck. Several
    /// callers need the `[web]` table written atomically — the rollback path (a
    /// failed daemon-side start) reverts several fields at once, and the enable
    /// toggle writes token + enabled together so a partial write can't leave
    /// them out of sync — so a per-field write isn't enough. Returns `true`
    /// when persistence is disabled (test path), mirroring `persist_field`.
    pub(crate) fn persist_web_settings(&mut self) -> bool {
        if !self.persist_settings_to_disk {
            return true;
        }
        match loom_config::writer::EditableConfig::load() {
            Ok(mut w) => {
                for field in [
                    SettingsField::WebEnabled,
                    SettingsField::WebBind,
                    SettingsField::WebPort,
                    SettingsField::WebToken,
                ] {
                    schema::write_to_disk(field, &self.core.config, &mut w);
                }
                if let Err(e) = w.save() {
                    log::warn!("settings panel: failed to persist web settings: {e}");
                    false
                } else {
                    self.suppress_next_config_reload();
                    true
                }
            }
            Err(e) => {
                log::warn!("settings panel: failed to open settings.toml for web write: {e}");
                false
            }
        }
    }

    fn apply_web_enabled_toggle(&mut self) {
        let enabled = self.core.config.web.enabled;
        let previous_enabled = !enabled;
        let previous_token = self.core.config.web.token.clone();

        if enabled {
            if let Some(reason) = self.web_enable_validation_error() {
                self.reject_web_enabled_toggle(previous_enabled, previous_token, &reason);
                return;
            }

            let token = self.core.config.web.token.trim().to_string();
            if web_token_is_usable(&token) {
                if token != self.core.config.web.token {
                    self.core.config.web.token = token;
                }
            } else {
                match generate_web_token() {
                    Ok(token) => {
                        self.core.config.web.token = token;
                    }
                    Err(reason) => {
                        self.reject_web_enabled_toggle(previous_enabled, previous_token, &reason);
                        return;
                    }
                }
            }
        }

        // Persist token + enabled (and the unchanged bind/port) in a single
        // atomic write so a save that fails partway can't leave settings.toml's
        // token and enabled out of sync with each other or with memory.
        if !self.persist_web_settings() {
            self.reject_web_enabled_toggle(
                previous_enabled,
                previous_token,
                "Could not save web settings to settings.toml.",
            );
            return;
        }

        // Arm a rollback to the pre-toggle state so a daemon-side start
        // failure (port taken, etc.) restores settings.toml + the panel.
        self.pending_web_rollback = Some(loom_config::schema::WebConfig {
            enabled: previous_enabled,
            token: previous_token,
            ..self.core.config.web.clone()
        });
        self.apply_settings_side_effects(SettingsField::WebEnabled);
    }

    /// A live web-gateway setting (bind/port) changed via the panel. When the
    /// gateway is enabled the new value must pass the same checks the enable
    /// toggle enforces — otherwise we'd persist a config that won't reload
    /// (e.g. a non-loopback bind with empty `allowed_origins`) and the running
    /// listener would silently drift from the on-disk values — and the daemon
    /// must be told the new settings so the live gateway actually moves. When
    /// the gateway is off it's a plain persist: `enabled = false` always
    /// reloads, and there's no listener to update. On any failure the whole
    /// web sub-config rolls back to `prev_web`.
    fn apply_web_runtime_field_change(
        &mut self,
        field: SettingsField,
        prev_web: loom_config::schema::WebConfig,
    ) {
        if self.core.config.web.enabled
            && let Some(reason) = self.web_enable_validation_error()
        {
            self.core.config.web = prev_web;
            self.send_desktop_notification("Web settings", &reason);
            self.schedule_redraw();
            return;
        }
        if !self.persist_field(field) {
            self.core.config.web = prev_web;
            self.send_desktop_notification(
                "Web settings",
                "Could not save web settings to settings.toml.",
            );
            self.schedule_redraw();
            return;
        }
        if self.core.config.web.enabled {
            // Arm a rollback to the pre-change web config so a daemon-side
            // start failure on the new bind/port restores settings.toml.
            self.pending_web_rollback = Some(prev_web);
            self.send_current_web_settings();
        }
        self.schedule_redraw();
    }

    fn reject_web_enabled_toggle(
        &mut self,
        previous_enabled: bool,
        previous_token: String,
        reason: &str,
    ) {
        self.core.config.web.enabled = previous_enabled;
        self.core.config.web.token = previous_token;
        self.send_desktop_notification("Web settings", reason);
        self.schedule_redraw();
    }

    fn web_enable_validation_error(&self) -> Option<String> {
        let web = &self.core.config.web;
        if self.core.config.remote.enabled && self.core.config.remote.port == web.port {
            return Some(format!(
                "remote.port and web.port are both {}. Pick distinct ports before enabling Web.",
                web.port
            ));
        }
        if web_bind_is_non_loopback(&web.bind) && web.allowed_origins.is_empty() {
            let bind = web.bind.trim();
            return Some(format!(
                "bind {bind} is not loopback, but web.allowed_origins is empty."
            ));
        }
        // A `["null"]`-only allowlist accepts only the `null` browser Origin
        // (file:// / sandboxed), but the self-hosted UI is opened over http(s),
        // whose Origin is never `null` — so every `/ws` login would 401. Mirror
        // the `loomtty web` CLI guard. (`web_access_url` would also surface this
        // `http://…` URL, reinforcing the mismatch.)
        if !web.allowed_origins.is_empty() && web.allowed_origins.iter().all(|o| o == "null") {
            return Some(format!(
                "web.allowed_origins is only [\"null\"], which the browser UI can't \
                 authenticate with. Add the origin you'll open (e.g. \"http://127.0.0.1:{}\").",
                web.port
            ));
        }
        None
    }

    pub(crate) fn send_current_web_settings(&mut self) {
        // The settings panel edits the LOCAL `[web]` config, but in a remote
        // session the active `server_tx` is the REMOTE daemon — sending this
        // there would start/stop the wrong machine's gateway and leave the
        // local config + copied URL describing a server that was never changed.
        // So skip the runtime send for remote slots: the local config is still
        // persisted (it applies when running locally), drop any armed rollback
        // (no reply is coming), and tell the user.
        if self.core.remote_config.is_some() {
            self.pending_web_rollback = None;
            self.send_desktop_notification(
                "Web settings",
                "Saved to local config. The browser gateway runs on the local server, \
                 so this doesn't affect the remote session you're connected to.",
            );
            return;
        }
        let web = &self.core.config.web;
        let message = ClientMessage::SetWebEnabled {
            enabled: web.enabled,
            token: web.token.trim().to_string(),
            bind: web.bind.clone(),
            port: web.port,
            allowed_origins: web.allowed_origins.clone(),
            static_dir: web.static_dir.clone(),
        };
        self.send(message);
    }

    pub(crate) fn copy_web_url_to_clipboard(&mut self) {
        let url = schema::web_access_url(&self.core.config);
        match self.clipboard.as_mut() {
            Some(cb) => match cb.set_text(&url) {
                Ok(()) => self.send_desktop_notification("Web", "URL copied to clipboard."),
                Err(e) => {
                    log::warn!("failed to copy web URL to clipboard: {e}");
                    self.send_desktop_notification("Web", "Could not copy URL to clipboard.");
                }
            },
            None => {
                self.send_desktop_notification("Web", "Clipboard is not available.");
            }
        }
    }

    pub(crate) fn regenerate_web_token(&mut self) {
        // Serialize with any in-flight web change (see `apply_settings_control`)
        // so the single rollback slot always matches one pending request.
        if self.pending_web_rollback.is_some() {
            self.send_desktop_notification(
                "Web settings",
                "A web change is still pending — try again in a moment.",
            );
            return;
        }
        let previous_token = self.core.config.web.token.clone();
        let token = match generate_web_token() {
            Ok(token) => token,
            Err(reason) => {
                self.send_desktop_notification("Web settings", &reason);
                return;
            }
        };
        let prev_web = loom_config::schema::WebConfig {
            token: previous_token.clone(),
            ..self.core.config.web.clone()
        };
        self.core.config.web.token = token;
        if !self.persist_field(SettingsField::WebToken) {
            self.core.config.web.token = previous_token;
            self.send_desktop_notification("Web settings", "Could not save the new web token.");
            self.schedule_redraw();
            return;
        }

        if self.core.config.web.enabled {
            // Restarting the live gateway with the new token can fail; arm a
            // rollback to the prior token so a failure doesn't leave the saved
            // token out of sync with the still-running gateway.
            self.pending_web_rollback = Some(prev_web);
            self.send_current_web_settings();
        }
        self.send_desktop_notification("Web settings", "Web token regenerated.");
        self.schedule_redraw();
    }

    /// Sidebar nav — change which category the panel renders.
    pub(crate) fn select_settings_category(&mut self, cat: SettingsCategory) {
        if self.core.settings_category == cat {
            return;
        }
        self.core.settings_category = cat;
        // Each category is its own scroll context — start the new view
        // at the top rather than inheriting the previous category's
        // offset (which could be past the end of a shorter list).
        self.core.settings_scroll_offset = 0;
        self.schedule_redraw();
    }
}

fn web_bind_is_non_loopback(bind: &str) -> bool {
    let bind = bind.trim();
    if bind.is_empty() {
        return false;
    }
    bind.parse::<std::net::IpAddr>()
        .map(|ip| !ip.is_loopback())
        .unwrap_or(false)
}

fn generate_web_token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| format!("RNG failure generating web token: {e}"))?;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from_digit((b >> 4) as u32, 16).expect("nibble < 16"));
        s.push(char::from_digit((b & 0x0f) as u32, 16).expect("nibble < 16"));
    }
    Ok(s)
}
