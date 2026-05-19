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

    pub fn set_terminal_cursor_blink(&mut self, v: bool) {
        self.section_mut("terminal")["cursor_blink"] = value(v);
    }
    pub fn set_terminal_copy_on_select(&mut self, v: bool) {
        self.section_mut("terminal")["copy_on_select"] = value(v);
    }
    pub fn set_terminal_scrollback_lines(&mut self, v: usize) {
        self.section_mut("terminal")["scrollback_lines"] = value(v as i64);
    }

    pub fn set_layout_center_focused_column(&mut self, v: &str) {
        self.section_mut("layout")["center_focused_column"] = value(v);
    }

    pub fn set_animation_enabled(&mut self, v: bool) {
        self.section_mut("animation")["enabled"] = value(v);
    }
    pub fn set_animation_preset(&mut self, v: &str) {
        self.section_mut("animation")["preset"] = value(v);
    }

    pub fn set_input_mode(&mut self, v: &str) {
        self.section_mut("input")["mode"] = value(v);
    }
    pub fn set_input_focus_follows_mouse(&mut self, v: bool) {
        self.section_mut("input")["focus_follows_mouse"] = value(v);
    }

    pub fn set_statusbar_position(&mut self, v: &str) {
        self.section_mut("statusbar")["position"] = value(v);
    }

    pub fn set_prediction_mode(&mut self, v: &str) {
        self.section_mut("prediction")["mode"] = value(v);
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
            let mut f =
                fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
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
        let dir = std::env::temp_dir().join("ciri-config-writer-tests");
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
            .join("ciri-config-writer-tests")
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
        let loaded: crate::CiriConfig = toml::from_str(&raw)
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
}
