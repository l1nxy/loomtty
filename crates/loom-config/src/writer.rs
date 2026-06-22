//! Edit-in-place writer for `settings.toml`.
//!
//! The settings panel uses this to persist user changes after a live
//! preview lands. Goals:
//!
//! - Preserve comments, formatting, and ordering from the on-disk file
//!   (users hand-edit this; clobbering their layout is unacceptable).
//! - Atomic writes — write to a sibling temp file, then rename, so a
//!   crash mid-write can never leave a half-written `settings.toml`.
//! - Auto-create parent directory and an empty file if the user has
//!   never touched the config before.
//!
//! Loop-back handling lives at the call site: see
//! `App::set_expecting_self_config_write` — this module just writes.

use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Table, value};

/// In-memory edit handle for `settings.toml`. Open with [`Self::load`],
/// mutate via the `set_*` methods, and persist with [`Self::save`].
pub struct EditableConfig {
    path: PathBuf,
    doc: DocumentMut,
}

impl EditableConfig {
    /// Open the user's `settings.toml` for editing. A missing file
    /// resolves to an empty document — the on-save path will create the
    /// file (and any missing parent directories).
    pub fn load() -> Result<Self> {
        let path = crate::config::config_path();
        Self::load_from_path(&path)
    }

    pub fn load_from_path(path: &Path) -> Result<Self> {
        let doc = if path.exists() {
            let raw =
                fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
            raw.parse::<DocumentMut>()
                .with_context(|| format!("parsing {}", path.display()))?
        } else {
            DocumentMut::new()
        };
        Ok(Self {
            path: path.to_path_buf(),
            doc,
        })
    }

    /// Borrow / create the `[section]` table at the document root.
    /// New tables are inserted as standard (non-inline) tables so the
    /// resulting file reads naturally (`[appearance]\nfield = value`)
    /// rather than as a one-line dictionary.
    ///
    /// If the section already exists as an inline table — valid TOML
    /// like `font = { size = 12.0 }` — it gets promoted to a standard
    /// table in place, preserving the existing key/value pairs.
    /// Without this promotion `as_table_mut()` returns `None` for
    /// inline tables and the setter below would panic on the
    /// `expect`, even though the user's config is well-formed.
    fn section_mut(&mut self, name: &str) -> &mut Table {
        let root = self.doc.as_table_mut();
        if let Some(existing) = root.get_mut(name)
            && !existing.is_table()
            && let Some(inline) = existing.as_inline_table()
        {
            let mut promoted = Table::new();
            promoted.set_implicit(false);
            for (k, v) in inline.iter() {
                promoted.insert(k, Item::Value(v.clone()));
            }
            *existing = Item::Table(promoted);
        }
        if !root.contains_key(name) {
            let mut tbl = Table::new();
            tbl.set_implicit(false);
            root.insert(name, Item::Table(tbl));
        }
        root.get_mut(name)
            .and_then(Item::as_table_mut)
            .expect("section was just inserted or promoted from inline")
    }

    // ── Typed setters ──────────────────────────────────────────────
    //
    // Add new setters here as the panel grows. Keep them narrow: one
    // setter per leaf. Group by `[section]` for legibility.

    /// Switch the active theme preset. Also clears every other key in
    /// `[theme]` so per-colour overrides (`theme.accent`, `theme.red`,
    /// …) left over from the previous preset don't leak through on
    /// reload. The live-preview path (`App::apply_theme_preset`) wipes
    /// the in-memory theme down to `ThemeConfig::default()` plus the
    /// new preset, so the persistence model has to match — otherwise
    /// the panel shows the new preset's palette during the session
    /// and the user's old override snaps back after restart.
    pub fn set_theme_preset(&mut self, preset: &str) {
        let table = self.section_mut("theme");
        let stale: Vec<String> = table
            .iter()
            .map(|(k, _)| k.to_string())
            .filter(|k| k != "preset")
            .collect();
        for k in stale {
            table.remove(&k);
        }
        table["preset"] = value(preset);
    }

    pub fn set_appearance_pane_opacity(&mut self, v: f32) {
        self.section_mut("appearance")["pane_opacity"] = float_item(v);
    }
    pub fn set_appearance_pane_corner_radius(&mut self, v: f32) {
        self.section_mut("appearance")["pane_corner_radius"] = float_item(v);
    }
    pub fn set_appearance_padding(&mut self, v: f32) {
        self.section_mut("appearance")["padding"] = float_item(v);
    }
    pub fn set_appearance_column_gap(&mut self, v: f32) {
        self.section_mut("appearance")["column_gap"] = float_item(v);
    }
    pub fn set_appearance_inactive_opacity(&mut self, v: f32) {
        self.section_mut("appearance")["inactive_opacity"] = float_item(v);
    }
    pub fn set_appearance_background_dim(&mut self, v: f32) {
        self.section_mut("appearance")["background_dim"] = float_item(v);
    }
    pub fn set_appearance_border_width(&mut self, v: f32) {
        self.section_mut("appearance")["border_width"] = float_item(v);
    }
    /// Focus-ring style lives in the nested `[appearance.focus_ring]`
    /// table; create it if absent so a freshly-written config still
    /// loads the chosen style.
    pub fn set_appearance_focus_ring_style(&mut self, v: &str) {
        self.ensure_subtable("appearance", "focus_ring")["style"] = value(v);
    }

    pub fn set_font_family(&mut self, v: &str) {
        self.section_mut("font")["family"] = value(v);
    }
    pub fn set_font_size(&mut self, v: f32) {
        self.section_mut("font")["size"] = float_item(v);
    }
    pub fn set_font_weight(&mut self, v: u16) {
        self.section_mut("font")["weight"] = value(v as i64);
    }
    /// Persists the line-height multiplier as `font.adjust_cell_height`.
    /// `1.0` keeps the round-trip clean by removing the override — the
    /// default re-asserts on next load.
    pub fn set_font_line_height(&mut self, v: f32) {
        if (v - 1.0).abs() < f32::EPSILON {
            self.section_mut("font").remove("adjust_cell_height");
        } else {
            self.section_mut("font")["adjust_cell_height"] = float_item(v);
        }
    }

    /// Set or clear `[font.ui].family`. An empty `family` removes the
    /// whole `[font.ui]` table so the loader falls back to the system
    /// sans-serif — same shape as the no-override resting state.
    pub fn set_font_ui_family(&mut self, family: &str) {
        if family.is_empty() {
            self.section_mut("font").remove("ui");
        } else {
            self.ensure_font_ui_table()["family"] = value(family);
        }
    }
    /// Set `[font.ui].size`. Creates the `[font.ui]` table if it
    /// doesn't exist yet (with an empty family — the loader treats
    /// that as "system sans-serif" so the size still applies).
    pub fn set_font_ui_size(&mut self, v: f32) {
        self.ensure_font_ui_table()["size"] = float_item(v);
    }
    pub fn set_font_cell_width(&mut self, v: f32) {
        self.section_mut("font")["adjust_cell_width"] = float_item(v);
    }
    pub fn set_font_disable_ligatures(&mut self, v: &str) {
        self.section_mut("font")["disable_ligatures"] = value(v);
    }
    pub fn set_font_underline_position(&mut self, v: f32) {
        self.section_mut("font")["adjust_underline_position"] = float_item(v);
    }
    pub fn set_font_underline_thickness(&mut self, v: f32) {
        self.section_mut("font")["adjust_underline_thickness"] = float_item(v);
    }
    pub fn set_font_strikethrough_position(&mut self, v: f32) {
        self.section_mut("font")["adjust_strikethrough_position"] = float_item(v);
    }
    pub fn set_font_strikethrough_thickness(&mut self, v: f32) {
        self.section_mut("font")["adjust_strikethrough_thickness"] = float_item(v);
    }

    fn ensure_font_ui_table(&mut self) -> &mut Table {
        let font = self.section_mut("font");
        if !font.contains_key("ui") {
            let mut tbl = Table::new();
            tbl.set_implicit(false);
            font.insert("ui", Item::Table(tbl));
        }
        font.get_mut("ui")
            .and_then(Item::as_table_mut)
            .expect("font.ui was just inserted")
    }

    /// Borrow / create a nested standard table `[section.child]`. Used
    /// for the handful of settings that live one level down (e.g.
    /// `[appearance.focus_ring]`). Mirrors [`Self::ensure_font_ui_table`]
    /// but parameterised over the section + child name.
    fn ensure_subtable(&mut self, section: &str, child: &str) -> &mut Table {
        let parent = self.section_mut(section);
        if !parent.contains_key(child) {
            let mut tbl = Table::new();
            tbl.set_implicit(false);
            parent.insert(child, Item::Table(tbl));
        }
        parent
            .get_mut(child)
            .and_then(Item::as_table_mut)
            .expect("subtable was just inserted")
    }

    pub fn set_terminal_cursor_blink(&mut self, v: bool) {
        self.section_mut("terminal")["cursor_blink"] = value(v);
    }
    pub fn set_terminal_copy_on_select(&mut self, v: bool) {
        self.section_mut("terminal")["copy_on_select"] = value(v);
    }
    pub fn set_terminal_scrollback_lines(&mut self, v: usize) {
        self.section_mut("terminal")["scrollback_lines"] = value(v as i64);
    }
    pub fn set_terminal_cursor_shape(&mut self, v: &str) {
        self.section_mut("terminal")["cursor_shape"] = value(v);
    }
    pub fn set_terminal_cursor_opacity(&mut self, v: f32) {
        self.section_mut("terminal")["cursor_opacity"] = float_item(v);
    }
    pub fn set_terminal_cursor_blink_interval(&mut self, v: u64) {
        self.section_mut("terminal")["cursor_blink_interval_ms"] = value(v as i64);
    }
    pub fn set_terminal_clear_selection_on_type(&mut self, v: bool) {
        self.section_mut("terminal")["clear_selection_on_type"] = value(v);
    }
    pub fn set_terminal_bell_urgency(&mut self, v: bool) {
        self.section_mut("terminal")["bell_urgency"] = value(v);
    }
    pub fn set_terminal_paste_warn_threshold(&mut self, v: usize) {
        self.section_mut("terminal")["paste_warn_threshold"] = value(v as i64);
    }
    pub fn set_terminal_default_cols(&mut self, v: u16) {
        self.section_mut("terminal")["default_cols"] = value(v as i64);
    }
    pub fn set_terminal_default_rows(&mut self, v: u16) {
        self.section_mut("terminal")["default_rows"] = value(v as i64);
    }
    pub fn set_terminal_notify_command_threshold(&mut self, v: u64) {
        self.section_mut("terminal")["notify_command_threshold_secs"] = value(v as i64);
    }

    pub fn set_layout_center_focused_column(&mut self, v: &str) {
        self.section_mut("layout")["center_focused_column"] = value(v);
    }
    pub fn set_layout_new_pane_sizing(&mut self, v: &str) {
        self.section_mut("layout")["new_pane_sizing"] = value(v);
    }
    pub fn set_layout_new_pane_width(&mut self, v: &str) {
        self.section_mut("layout")["new_pane_width"] = value(v);
    }
    pub fn set_layout_dynamic_fullscreen_max_width(&mut self, v: f64) {
        self.section_mut("layout")["dynamic_fullscreen_max_width"] = value(v);
    }

    pub fn set_tabbar_position(&mut self, v: &str) {
        self.section_mut("tabbar")["position"] = value(v);
    }
    pub fn set_tabbar_width(&mut self, v: f32) {
        self.section_mut("tabbar")["width"] = float_item(v);
    }
    pub fn set_tabbar_tab_height(&mut self, v: f32) {
        self.section_mut("tabbar")["tab_height"] = float_item(v);
    }
    pub fn set_tabbar_tab_gap(&mut self, v: f32) {
        self.section_mut("tabbar")["tab_gap"] = float_item(v);
    }
    pub fn set_tabbar_pane_tab_width_chars(&mut self, v: usize) {
        self.section_mut("tabbar")["pane_tab_width_chars"] = value(v as i64);
    }

    pub fn set_animation_enabled(&mut self, v: bool) {
        self.section_mut("animation")["enabled"] = value(v);
    }
    pub fn set_animation_preset(&mut self, v: &str) {
        self.section_mut("animation")["preset"] = value(v);
    }
    pub fn set_animation_pane_open_style(&mut self, v: &str) {
        self.section_mut("animation")["pane_open_style"] = value(v);
    }
    pub fn set_animation_drag_opacity(&mut self, v: f32) {
        self.section_mut("animation")["drag_opacity"] = float_item(v);
    }
    pub fn set_animation_overview_zoom_fit(&mut self, v: f32) {
        self.section_mut("animation")["overview_zoom_fit"] = float_item(v);
    }

    pub fn set_input_mode(&mut self, v: &str) {
        self.section_mut("input")["mode"] = value(v);
    }
    pub fn set_input_focus_follows_mouse(&mut self, v: bool) {
        self.section_mut("input")["focus_follows_mouse"] = value(v);
    }
    pub fn set_input_leader_timeout(&mut self, v: u64) {
        self.section_mut("input")["leader_timeout_ms"] = value(v as i64);
    }
    pub fn set_input_double_tap_window(&mut self, v: u64) {
        self.section_mut("input")["double_tap_window_ms"] = value(v as i64);
    }
    pub fn set_input_scroll_multiplier(&mut self, v: f64) {
        self.section_mut("input")["scroll_multiplier"] = float_item_f64(v);
    }

    pub fn set_gesture_enabled(&mut self, v: bool) {
        self.section_mut("gesture")["enabled"] = value(v);
    }
    pub fn set_gesture_natural_scroll(&mut self, v: bool) {
        self.section_mut("gesture")["natural_scroll"] = value(v);
    }
    pub fn set_gesture_smooth_scroll(&mut self, v: bool) {
        self.section_mut("gesture")["smooth_scroll"] = value(v);
    }
    pub fn set_gesture_pinch_sensitivity(&mut self, v: f64) {
        self.section_mut("gesture")["pinch_sensitivity"] = float_item_f64(v);
    }
    pub fn set_gesture_scroll_pixels_per_line(&mut self, v: f64) {
        self.section_mut("gesture")["scroll_pixels_per_line"] = float_item_f64(v);
    }

    pub fn set_render_backend(&mut self, v: &str) {
        self.section_mut("render")["backend"] = value(v);
    }
    pub fn set_render_present_mode(&mut self, v: &str) {
        self.section_mut("render")["present_mode"] = value(v);
    }
    pub fn set_render_alpha_blending(&mut self, v: &str) {
        self.section_mut("render")["alpha_blending"] = value(v);
    }
    pub fn set_render_softness(&mut self, v: f32) {
        self.section_mut("render")["softness"] = float_item(v);
    }
    pub fn set_render_frame_interval(&mut self, v: u64) {
        self.section_mut("render")["frame_interval_ms"] = value(v as i64);
    }

    pub fn set_statusbar_position(&mut self, v: &str) {
        self.section_mut("statusbar")["position"] = value(v);
    }
    pub fn set_statusbar_padding_ratio(&mut self, v: f32) {
        self.section_mut("statusbar")["padding_ratio"] = float_item(v);
    }

    pub fn set_prediction_mode(&mut self, v: &str) {
        self.section_mut("prediction")["mode"] = value(v);
    }
    pub fn set_prediction_threshold(&mut self, v: u64) {
        self.section_mut("prediction")["threshold_ms"] = value(v as i64);
    }
    pub fn set_prediction_show_underline(&mut self, v: bool) {
        self.section_mut("prediction")["show_underline"] = value(v);
    }

    pub fn set_session_restore_agents(&mut self, v: bool) {
        self.section_mut("session")["restore_agents"] = value(v);
    }
    pub fn set_session_agent_save_interval(&mut self, v: u64) {
        self.section_mut("session")["agent_save_interval_secs"] = value(v as i64);
    }
    pub fn set_server_idle_timeout(&mut self, v: u64) {
        self.section_mut("server")["idle_timeout_secs"] = value(v as i64);
    }

    /// Enable/disable the browser web gateway. Written by `loomtty web`
    /// (which also persists a token) so the desktop daemon and the web
    /// command share one `[web]` configuration.
    pub fn set_web_enabled(&mut self, v: bool) {
        self.section_mut("web")["enabled"] = value(v);
    }
    /// Persist the web auth token. `toml_edit`'s `value` escapes the
    /// string, so a token containing quotes/backslashes round-trips
    /// safely.
    pub fn set_web_token(&mut self, v: &str) {
        self.section_mut("web")["token"] = value(v);
    }
    /// Persist the web listen port. `loomtty web` writes this only for an
    /// explicit `--port`, so the saved `[web]` stays internally consistent
    /// with `enabled = true` — the spawned server and every later
    /// desktop/server launch reload and re-validate the whole file.
    pub fn set_web_port(&mut self, v: u16) {
        self.section_mut("web")["port"] = value(v as i64);
    }
    /// Persist the web bind address. `loomtty web` writes this only for an
    /// explicit `--bind`. See [`Self::set_web_port`] for why an acted-on
    /// override must land on disk rather than stay transient.
    pub fn set_web_bind(&mut self, v: &str) {
        self.section_mut("web")["bind"] = value(v);
    }
    /// Persist the web static-asset directory. `loomtty web` writes this
    /// only for an explicit `--static-dir`, so the path still applies after
    /// the user restarts an already-running server (which won't see the
    /// transient `--web-static-dir` flag). See [`Self::set_web_port`].
    pub fn set_web_static_dir(&mut self, v: &str) {
        self.section_mut("web")["static_dir"] = value(v);
    }

    /// Render the document back to disk. Atomic: writes to
    /// `<file>.tmp`, fsyncs, then renames over the target.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let serialized = self.doc.to_string();
        let tmp = tmp_sibling(&self.path);
        {
            // Create owner-only: the config can hold the web bearer token,
            // and the atomic rename below carries the temp file's mode onto
            // the final file. Without this, a default umask (022) leaves a
            // freshly created settings.toml world-readable and any local
            // user could lift the token and reach the loopback gateway.
            let mut f =
                create_owner_only(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
            f.write_all(serialized.as_bytes())
                .with_context(|| format!("writing {}", tmp.display()))?;
            // Best-effort fsync. Some filesystems / Windows handles
            // reject sync_all on the just-created file; log and move on
            // — the rename is still atomic on POSIX, and Windows's
            // ReplaceFile semantics give us crash-safety anyway.
            let _ = f.sync_all();
        }
        // `rename` clobbers the target on both POSIX and Windows
        // (Rust normalises this since 1.5; we're on a much newer
        // toolchain). Logged failure surfaces to the panel as "Open
        // settings.toml" still working — the file is unchanged.
        fs::rename(&tmp, &self.path)
            .with_context(|| format!("renaming {} → {}", tmp.display(), self.path.display(),))?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn rendered(&self) -> String {
        self.doc.to_string()
    }
}

/// Wrap an `f32` for `toml_edit` insertion. We round-trip through the
/// short Rust `Display` form before widening to `f64` so a stepper that
/// produces `0.7_f32` writes out as `0.7` instead of
/// `0.699999988079071` — the literal `as f64` cast surfaces the f32's
/// sub-bit-precision noise into the rendered TOML, which users would
/// (rightly) read as garbage.
fn float_item(v: f32) -> Item {
    let s = format!("{v}");
    let f: f64 = s.parse().unwrap_or(v as f64);
    value(f)
}

/// Same idea as [`float_item`] for the handful of natively-`f64` config
/// leaves (gesture sensitivities, scroll multiplier). The value already
/// has full `f64` precision, so we insert it directly; the `Display`
/// round-trip the f32 path needs (to scrub `0.7_f32` → `0.699999…`
/// noise) isn't required here.
fn float_item_f64(v: f64) -> Item {
    value(v)
}

/// Create (truncating) a file readable/writable only by its owner.
///
/// On Unix the mode is set at creation (so there's no world-readable
/// window) and re-asserted in case a leftover temp from a crashed write
/// pre-existed with looser bits. On other platforms the user profile
/// directory is already per-user, so the default applies.
#[cfg(unix)]
fn create_owner_only(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(f)
}

#[cfg(not(unix))]
fn create_owner_only(path: &Path) -> std::io::Result<fs::File> {
    fs::File::create(path)
}

fn tmp_sibling(target: &Path) -> PathBuf {
    let mut name = target
        .file_name()
        .map(|s| s.to_owned())
        .unwrap_or_else(|| std::ffi::OsString::from("settings.toml"));
    name.push(".tmp");
    target.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join("loom-config-writer-tests");
        fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{label}-{nanos}.toml"))
    }

    #[test]
    fn missing_file_loads_empty_document() {
        let path = temp_path("missing");
        assert!(!path.exists());
        let cfg = EditableConfig::load_from_path(&path).unwrap();
        assert_eq!(cfg.rendered(), "");
    }

    #[test]
    fn save_creates_parent_directories() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let nested = std::env::temp_dir()
            .join("loom-config-writer-tests")
            .join(format!("nested-{nanos}"))
            .join("settings.toml");
        assert!(!nested.parent().unwrap().exists());
        let mut cfg = EditableConfig::load_from_path(&nested).unwrap();
        cfg.set_theme_preset("dracula");
        cfg.save().unwrap();
        let raw = fs::read_to_string(&nested).unwrap();
        assert!(raw.contains("preset = \"dracula\""));
    }

    #[test]
    fn round_trip_preserves_comments_and_unrelated_keys() {
        let path = temp_path("preserve");
        fs::write(
            &path,
            "# user comment\n[font]\nfamily = \"Iosevka\"\n\n[theme]\npreset = \"nord\"\n",
        )
        .unwrap();
        let mut cfg = EditableConfig::load_from_path(&path).unwrap();
        cfg.set_theme_preset("dracula");
        cfg.save().unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("# user comment"));
        assert!(raw.contains("family = \"Iosevka\""));
        assert!(raw.contains("preset = \"dracula\""));
    }

    /// A user's config that uses TOML inline-table syntax for a
    /// section (`font = { size = 12.0 }`) must promote to a standard
    /// `[font]` table when the panel writes a field, instead of
    /// panicking on `as_table_mut()`. Both the existing inline keys
    /// and the new setter's value need to survive.
    #[test]
    fn promotes_inline_table_when_writing_section_field() {
        let path = temp_path("promote-inline-font");
        fs::write(&path, "font = { size = 12.0, family = \"Iosevka\" }\n").unwrap();
        let mut cfg = EditableConfig::load_from_path(&path).unwrap();
        cfg.set_font_size(14.0);
        cfg.save().unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("size = 14"), "new value missing: {raw}");
        assert!(raw.contains("Iosevka"), "inline key dropped: {raw}");
    }

    #[test]
    fn creates_section_when_missing() {
        let path = temp_path("create-section");
        fs::write(&path, "[font]\nsize = 13.0\n").unwrap();
        let mut cfg = EditableConfig::load_from_path(&path).unwrap();
        cfg.set_appearance_pane_opacity(0.85);
        cfg.set_animation_enabled(false);
        cfg.set_terminal_scrollback_lines(20000);
        cfg.save().unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("[appearance]"));
        assert!(raw.contains("pane_opacity = 0.85"));
        assert!(raw.contains("[animation]"));
        assert!(raw.contains("enabled = false"));
        assert!(raw.contains("[terminal]"));
        assert!(raw.contains("scrollback_lines = 20000"));
    }

    #[test]
    fn updates_existing_value_in_place() {
        let path = temp_path("update-in-place");
        fs::write(
            &path,
            "[appearance]\npane_opacity = 1.0\nborder_width = 2.0\n",
        )
        .unwrap();
        let mut cfg = EditableConfig::load_from_path(&path).unwrap();
        cfg.set_appearance_pane_opacity(0.7);
        cfg.save().unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        // Order preserved
        let opacity_pos = raw.find("pane_opacity").unwrap();
        let border_pos = raw.find("border_width").unwrap();
        assert!(opacity_pos < border_pos, "ordering not preserved: {raw}");
        assert!(raw.contains("pane_opacity = 0.7"));
    }

    /// `loomtty web` persists enabled + token, and (only) an explicit
    /// `--bind`/`--port`. The written `[web]` must load AND pass schema
    /// validation, since the spawned server and every later launch reload
    /// and re-validate the whole file.
    #[test]
    fn web_overrides_round_trip_and_validate() {
        let path = temp_path("web-overrides");
        let mut cfg = EditableConfig::load_from_path(&path).unwrap();
        cfg.set_web_enabled(true);
        cfg.set_web_token("0123456789abcdef0123456789abcdef");
        cfg.set_web_bind("127.0.0.1");
        cfg.set_web_port(7891);
        cfg.set_web_static_dir("/opt/loom/web");
        cfg.save().unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("enabled = true"), "{raw}");
        assert!(raw.contains("bind = \"127.0.0.1\""), "{raw}");
        assert!(raw.contains("port = 7891"), "{raw}");
        assert!(raw.contains("static_dir = \"/opt/loom/web\""), "{raw}");

        let loaded: crate::LoomConfig = toml::from_str(&raw).unwrap();
        assert!(loaded.web.enabled);
        assert_eq!(loaded.web.bind, "127.0.0.1");
        assert_eq!(loaded.web.port, 7891);
        assert_eq!(loaded.web.static_dir, "/opt/loom/web");
        loaded
            .validate_schema()
            .expect("persisted web config must pass schema validation");
    }

    /// Regression for the persist-the-override fix: a stored non-loopback
    /// `bind` with no `allowed_origins` is valid only while web is
    /// DISABLED. Enabling it AND writing the loopback `--bind` override
    /// yields a loadable config; enabling WITHOUT writing the override
    /// (the old behaviour) leaves a config that fails to load — which is
    /// exactly why an acted-on override must land on disk.
    #[test]
    fn enabling_web_must_persist_a_loopback_override_to_stay_valid() {
        // Stored, pre-existing: LAN bind, no origins, web off → valid.
        let base = "[web]\nbind = \"0.0.0.0\"\nallowed_origins = []\n";

        // Old behaviour: flip enabled + token but DON'T fix the bind.
        let path_bad = temp_path("web-stale-bind");
        fs::write(&path_bad, base).unwrap();
        let mut bad = EditableConfig::load_from_path(&path_bad).unwrap();
        bad.set_web_enabled(true);
        bad.set_web_token("0123456789abcdef0123456789abcdef");
        bad.save().unwrap();
        let bad_cfg: crate::LoomConfig =
            toml::from_str(&fs::read_to_string(&path_bad).unwrap()).unwrap();
        assert!(
            bad_cfg.validate_schema().is_err(),
            "enabled web with a stale non-loopback bind + empty origins must be invalid",
        );

        // New behaviour: also persist the explicit loopback `--bind`.
        let path_ok = temp_path("web-fixed-bind");
        fs::write(&path_ok, base).unwrap();
        let mut ok = EditableConfig::load_from_path(&path_ok).unwrap();
        ok.set_web_enabled(true);
        ok.set_web_token("0123456789abcdef0123456789abcdef");
        ok.set_web_bind("127.0.0.1");
        ok.save().unwrap();
        let ok_cfg: crate::LoomConfig =
            toml::from_str(&fs::read_to_string(&path_ok).unwrap()).unwrap();
        assert_eq!(ok_cfg.web.bind, "127.0.0.1");
        ok_cfg
            .validate_schema()
            .expect("enabling web with a persisted loopback override must be valid");
    }

    /// The config can hold the web bearer token, so a written file must be
    /// owner-only (0600) on Unix — otherwise a default umask leaves the
    /// secret world-readable to other local users.
    #[cfg(unix)]
    #[test]
    fn saved_config_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_path("perms");
        let mut cfg = EditableConfig::load_from_path(&path).unwrap();
        cfg.set_web_enabled(true);
        cfg.set_web_token("0123456789abcdef0123456789abcdef");
        cfg.save().unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o600,
            "config holding a token must be 0600, was {mode:o}"
        );

        // A second save (file already exists) must keep it 0600.
        cfg.set_web_port(7891);
        cfg.save().unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "rewrite must preserve 0600, was {mode:o}");
    }

    #[test]
    fn round_trip_through_loader_validates() {
        let path = temp_path("loader-validates");
        let mut cfg = EditableConfig::load_from_path(&path).unwrap();
        cfg.set_theme_preset("dracula");
        cfg.set_appearance_pane_opacity(0.85);
        cfg.set_appearance_pane_corner_radius(8.0);
        cfg.set_appearance_padding(6.0);
        cfg.set_appearance_column_gap(12.0);
        cfg.set_appearance_inactive_opacity(0.5);
        cfg.set_appearance_background_dim(0.4);
        cfg.set_font_size(12.5);
        cfg.set_terminal_cursor_blink(false);
        cfg.set_terminal_copy_on_select(true);
        cfg.set_terminal_scrollback_lines(5000);
        cfg.set_layout_center_focused_column("on-overflow");
        cfg.set_animation_enabled(true);
        cfg.set_animation_preset("smooth");
        cfg.set_input_mode("sticky");
        cfg.set_input_focus_follows_mouse(true);
        cfg.set_statusbar_position("bottom");
        cfg.set_prediction_mode("adaptive");
        cfg.save().unwrap();
        // Reload via the production schema loader to ensure each
        // setter wrote a value the deserializer accepts.
        let raw = fs::read_to_string(&path).unwrap();
        let loaded: crate::LoomConfig = toml::from_str(&raw)
            .unwrap_or_else(|e| panic!("validating round-trip failed: {e}\n---\n{raw}"));
        assert_eq!(loaded.theme.preset, "dracula");
        assert!((loaded.appearance.pane_opacity - 0.85).abs() < 1e-6);
        assert!((loaded.font.size - 12.5).abs() < 1e-6);
        assert!(!loaded.terminal.cursor_blink);
        assert!(loaded.terminal.copy_on_select);
        assert_eq!(loaded.terminal.scrollback_lines, 5000);
        assert!(loaded.animation.enabled);
        assert_eq!(loaded.animation.preset, crate::AnimationPreset::Smooth);
        assert_eq!(loaded.input.mode, crate::InputMode::Sticky);
        assert!(loaded.input.focus_follows_mouse);
        assert_eq!(loaded.statusbar.position, crate::StatusBarPosition::Bottom);
        assert_eq!(
            loaded.layout.center_focused_column,
            crate::CenterStrategy::OnOverflow
        );
        assert_eq!(loaded.prediction.mode, crate::PredictionMode::Adaptive);
    }

    /// Exercise the settings-panel setters added for the "expose every
    /// config in the panel" pass. Each writes into a (possibly brand
    /// new) section; the file must reload through the production schema
    /// loader AND pass schema validation — catching a mistyped key or
    /// out-of-range value that the panic-free schema test wouldn't.
    #[test]
    fn extended_setters_round_trip_and_validate() {
        let path = temp_path("extended-setters");
        let mut cfg = EditableConfig::load_from_path(&path).unwrap();
        // Appearance / font advanced
        cfg.set_appearance_border_width(3.0);
        cfg.set_appearance_focus_ring_style("glow");
        cfg.set_font_cell_width(1.05);
        cfg.set_font_disable_ligatures("cursor");
        cfg.set_font_underline_thickness(2.0);
        cfg.set_font_underline_position(1.0);
        cfg.set_font_strikethrough_thickness(1.5);
        cfg.set_font_strikethrough_position(-1.0);
        // Terminal
        cfg.set_terminal_cursor_shape("block");
        cfg.set_terminal_cursor_opacity(0.5);
        cfg.set_terminal_cursor_blink_interval(750);
        cfg.set_terminal_clear_selection_on_type(false);
        cfg.set_terminal_bell_urgency(false);
        cfg.set_terminal_paste_warn_threshold(2000);
        cfg.set_terminal_default_cols(100);
        cfg.set_terminal_default_rows(40);
        cfg.set_terminal_notify_command_threshold(15);
        // TabBar (new section)
        cfg.set_tabbar_position("left");
        cfg.set_tabbar_width(220.0);
        cfg.set_tabbar_tab_height(40.0);
        cfg.set_tabbar_tab_gap(6.0);
        cfg.set_tabbar_pane_tab_width_chars(30);
        // StatusBar / animation
        cfg.set_statusbar_padding_ratio(0.3);
        cfg.set_animation_pane_open_style("slide-up");
        cfg.set_animation_drag_opacity(0.5);
        cfg.set_animation_overview_zoom_fit(0.8);
        // Input
        cfg.set_input_leader_timeout(1200);
        cfg.set_input_double_tap_window(250);
        cfg.set_input_scroll_multiplier(40.0);
        // Gesture (new section)
        cfg.set_gesture_enabled(false);
        cfg.set_gesture_natural_scroll(false);
        cfg.set_gesture_smooth_scroll(false);
        cfg.set_gesture_pinch_sensitivity(1.5);
        cfg.set_gesture_scroll_pixels_per_line(25.0);
        // Render
        cfg.set_render_backend("gl");
        cfg.set_render_present_mode("mailbox");
        cfg.set_render_alpha_blending("linear");
        cfg.set_render_softness(0.2);
        cfg.set_render_frame_interval(8);
        // Prediction / session / server (new sections)
        cfg.set_prediction_threshold(40);
        cfg.set_prediction_show_underline(false);
        cfg.set_session_restore_agents(false);
        cfg.set_session_agent_save_interval(60);
        cfg.set_server_idle_timeout(120);
        // Layout new-pane sizing
        cfg.set_layout_new_pane_sizing("dynamic");
        cfg.set_layout_new_pane_width("full");
        cfg.set_layout_dynamic_fullscreen_max_width(1200.0);
        cfg.save().unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        let loaded: crate::LoomConfig = toml::from_str(&raw)
            .unwrap_or_else(|e| panic!("extended round-trip failed to parse: {e}\n---\n{raw}"));
        loaded
            .validate_schema()
            .unwrap_or_else(|e| panic!("extended round-trip failed validation: {e}\n---\n{raw}"));

        // Spot-check a representative leaf from each new section to
        // confirm the key names match what the loader expects.
        assert!((loaded.appearance.border_width - 3.0).abs() < 1e-6);
        assert_eq!(
            loaded.font.disable_ligatures,
            crate::DisableLigatures::Cursor
        );
        assert_eq!(loaded.terminal.cursor_shape, "block");
        assert_eq!(loaded.terminal.default_cols, 100);
        assert_eq!(loaded.tabbar.position, crate::config::TabBarPosition::Left);
        assert!((loaded.tabbar.width - 220.0).abs() < 1e-6);
        assert_eq!(loaded.input.leader_timeout_ms, 1200);
        assert!(!loaded.gesture.enabled);
        assert!((loaded.gesture.pinch_sensitivity - 1.5).abs() < 1e-9);
        assert_eq!(loaded.render.backend, crate::RenderBackend::Gl);
        assert_eq!(loaded.render.present_mode, crate::PresentMode::Mailbox);
        assert_eq!(loaded.prediction.threshold_ms, 40);
        assert!(!loaded.session.restore_agents);
        assert_eq!(loaded.server.idle_timeout_secs, 120);
        assert_eq!(
            loaded.layout.new_pane_sizing,
            crate::config::NewPaneSizing::Dynamic
        );
        assert_eq!(
            loaded.layout.new_pane_width,
            crate::config::NewPaneWidth::Full
        );
        assert!((loaded.layout.dynamic_fullscreen_max_width - 1200.0).abs() < 1e-6);
    }

    #[test]
    fn column_sizing_resolves_policy() {
        use crate::config::{ColumnSizing, NewPaneSizing, NewPaneWidth, PresetWidth};
        let mut layout = crate::config::LayoutConfig::default();
        // Default: Fixed + Half → constant 0.5, matching historic behaviour.
        assert_eq!(
            layout.column_sizing(),
            ColumnSizing::Fixed(PresetWidth::Proportion { proportion: 0.5 })
        );
        // Fixed + Full → constant 1.0.
        layout.new_pane_width = NewPaneWidth::Full;
        assert_eq!(
            layout.column_sizing(),
            ColumnSizing::Fixed(PresetWidth::Proportion { proportion: 1.0 })
        );
        // Dynamic: full below threshold, half at/above.
        layout.new_pane_sizing = NewPaneSizing::Dynamic;
        layout.dynamic_fullscreen_max_width = 1000.0;
        let sizing = layout.column_sizing();
        assert_eq!(sizing, ColumnSizing::Dynamic { threshold: 1000.0 });
        assert_eq!(
            sizing.width_at(800.0),
            PresetWidth::Proportion { proportion: 1.0 }
        );
        assert_eq!(
            sizing.width_at(1000.0),
            PresetWidth::Proportion { proportion: 0.5 }
        );
        // An explicit override always wins, regardless of policy.
        layout.default_column_width = Some(PresetWidth::Fixed { fixed: 640.0 });
        assert_eq!(
            layout.column_sizing(),
            ColumnSizing::Fixed(PresetWidth::Fixed { fixed: 640.0 })
        );
    }
}
