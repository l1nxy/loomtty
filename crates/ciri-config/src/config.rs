//! Configuration loading, validation, and path resolution.

use anyhow::Result;
use garde::Validate;
use std::path::PathBuf;

pub use crate::schema::*;

impl CiriConfig {
    pub fn load() -> Result<Self> {
        let path = config_path();
        let mut config = load_from_path(&path)?;
        config.theme.resolve_preset();
        config.validate()?;
        Ok(config)
    }
}

fn load_from_path(path: &PathBuf) -> Result<CiriConfig> {
    if path.exists() {
        let content = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    } else {
        Ok(CiriConfig::default())
    }
}

pub fn config_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            let legacy = home.join(".config").join("ciri").join("config.toml");
            if legacy.exists() {
                return legacy;
            }
        }
    }

    dirs::config_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .map(|d| d.join("ciri").join("config.toml"))
        .unwrap_or_else(|| {
            // No config dir and no home dir — truly degenerate environment.
            // Use a path that won't exist, so load() returns default config.
            log::warn!("cannot determine config directory; config will not be loaded");
            PathBuf::from(if cfg!(windows) {
                r"C:\nonexistent\.ciri\config.toml"
            } else {
                "/nonexistent/.ciri/config.toml"
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn load_returns_defaults_when_config_is_missing() {
        let path = temp_config_path("missing");

        let config = load_from_path(&path).unwrap();

        assert_eq!(config.font.family, CiriConfig::default().font.family);
        assert_eq!(config.render.backend, CiriConfig::default().render.backend);
    }

    #[test]
    fn load_applies_overrides_and_theme_preset_resolution() {
        let path = temp_config_path("override");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r##"
                [font]
                family = "Iosevka"
                size = 17.5

                [theme]
                preset = "nord"
                accent = "#123456"

                [render]
                backend = "gl"
            "##,
        )
        .unwrap();

        let mut config = load_from_path(&path).unwrap();
        config.theme.resolve_preset();
        config.validate().unwrap();

        assert_eq!(config.font.family, "Iosevka");
        assert_eq!(config.font.size, 17.5);
        assert_eq!(config.render.backend, RenderBackend::Gl);
        assert_eq!(config.theme.preset, "nord");
        assert_eq!(config.theme.accent, "#123456");
        assert_eq!(config.theme.background, "#2E3440");
    }

    #[test]
    fn load_preserves_empty_string_theme_overrides() {
        let path = temp_config_path("empty-theme-override");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r##"
                [theme]
                preset = "nord"
                accent = ""
            "##,
        )
        .unwrap();

        let mut config = load_from_path(&path).unwrap();
        config.theme.resolve_preset();

        assert_eq!(config.theme.preset, "nord");
        assert_eq!(config.theme.accent, "");
    }

    /// Helper: mutate one field of a default config and assert validation fails.
    fn assert_validation_rejects(label: &str, mutate: fn(&mut CiriConfig)) {
        let mut config = CiriConfig::default();
        mutate(&mut config);
        assert!(
            config.validate().is_err(),
            "validation should reject {label}"
        );
    }

    #[test]
    fn validate_rejects_out_of_range_values() {
        let cases: &[(&str, fn(&mut CiriConfig))] = &[
            ("drag_opacity=1.5", |c| c.animation.drag_opacity = 1.5),
            ("border_width=-2", |c| c.appearance.border_width = -2.0),
            ("inactive_opacity=1.5", |c| c.appearance.inactive_opacity = 1.5),
            ("font.size=0", |c| c.font.size = 0.0),
            ("cursor_opacity=-0.25", |c| c.terminal.cursor_opacity = -0.25),
            ("frame_interval_ms=0", |c| c.render.frame_interval_ms = 0),
            ("overview_zoom_fit=0", |c| c.animation.overview_zoom_fit = 0.0),
            ("zoom_threshold=0", |c| c.animation.zoom_threshold = 0.0),
            ("padding_ratio=-0.1", |c| c.statusbar.padding_ratio = -0.1),
            ("default_cols=0", |c| c.terminal.default_cols = 0),
            ("default_rows=0", |c| c.terminal.default_rows = 0),
            ("cursor_blink_interval_ms=0", |c| c.terminal.cursor_blink_interval_ms = 0),
            ("scrollback_lines=0", |c| c.terminal.scrollback_lines = 0),
        ];
        for (label, mutate) in cases {
            assert_validation_rejects(label, *mutate);
        }
    }

    #[test]
    fn default_config_passes_validation() {
        let config = CiriConfig::default();
        config.validate().unwrap();
    }

    fn temp_config_path(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join("ciri-config-tests")
            .join(format!("{label}-{nanos}.toml"))
    }

    #[test]
    fn load_rejects_invalid_toml_syntax() {
        let path = temp_config_path("bad-syntax");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "this is not [valid toml ===").unwrap();

        let result = load_from_path(&path);
        assert!(result.is_err());
    }

    #[test]
    fn load_ignores_unknown_fields() {
        let path = temp_config_path("unknown-fields");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"
                [font]
                family = "Iosevka"
                this_field_does_not_exist = 42

                [some_future_section]
                key = "value"
            "#,
        )
        .unwrap();

        let config = load_from_path(&path).unwrap();
        assert_eq!(config.font.family, "Iosevka");
        // All other fields should still be defaults
        assert_eq!(config.font.size, CiriConfig::default().font.size);
    }

    #[test]
    fn load_rejects_wrong_type_for_field() {
        let path = temp_config_path("wrong-type");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"
                [font]
                size = "not a number"
            "#,
        )
        .unwrap();

        let result = load_from_path(&path);
        assert!(result.is_err());
    }

    #[test]
    fn load_rejects_invalid_enum_variant() {
        let path = temp_config_path("bad-enum");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"
                [render]
                backend = "vulkan-9000"
            "#,
        )
        .unwrap();

        let result = load_from_path(&path);
        assert!(result.is_err());
    }

    #[test]
    fn validate_accepts_boundary_values() {
        // All fields at their exact minimum/maximum should pass
        let mut config = CiriConfig::default();
        config.font.size = 1.0;
        config.appearance.border_width = 0.0;
        config.appearance.inactive_opacity = 0.0;
        config.animation.drag_opacity = 0.0;
        config.terminal.cursor_opacity = 0.0;
        config.render.frame_interval_ms = 1;
        config.terminal.default_cols = 1;
        config.terminal.default_rows = 1;
        config.terminal.scrollback_lines = 1;
        config.terminal.cursor_blink_interval_ms = 1;
        config.validate().unwrap();

        let mut config = CiriConfig::default();
        config.font.size = 200.0;
        config.appearance.inactive_opacity = 1.0;
        config.animation.drag_opacity = 1.0;
        config.animation.overview_zoom_fit = 1.0;
        config.animation.zoom_threshold = 1.0;
        config.terminal.cursor_opacity = 1.0;
        config.validate().unwrap();
    }

    #[test]
    fn load_partial_sections_preserve_other_defaults() {
        let path = temp_config_path("partial-sections");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"
                [terminal]
                scrollback_lines = 50000

                [animation]
                enabled = false
            "#,
        )
        .unwrap();

        let config = load_from_path(&path).unwrap();
        let defaults = CiriConfig::default();

        assert_eq!(config.terminal.scrollback_lines, 50000);
        assert!(!config.animation.enabled);
        // Untouched sections remain default
        assert_eq!(config.font.family, defaults.font.family);
        assert_eq!(config.font.size, defaults.font.size);
        assert_eq!(config.window.width, defaults.window.width);
        assert_eq!(config.render.backend, defaults.render.backend);
        // Untouched fields within touched sections remain default
        assert_eq!(config.terminal.default_cols, defaults.terminal.default_cols);
        assert_eq!(config.animation.preset, defaults.animation.preset);
    }

    #[test]
    fn all_enum_variants_deserialize_from_toml() {
        // Verify every kebab-case enum variant parses via toml::from_str
        // (no disk I/O needed — this tests serde, not file loading)
        let cases: &[(&str, &str, &str)] = &[
            ("render", "backend", "auto"),
            ("render", "backend", "blade"),
            ("render", "backend", "gl"),
            ("render", "present_mode", "fifo"),
            ("render", "present_mode", "mailbox"),
            ("render", "present_mode", "immediate"),
            ("statusbar", "position", "top"),
            ("statusbar", "position", "bottom"),
            ("input", "mode", "prefix"),
            ("input", "mode", "sticky"),
            ("animation", "preset", "snappy"),
            ("animation", "preset", "default"),
            ("animation", "preset", "smooth"),
            ("animation", "preset", "gentle"),
            ("animation", "pane_open_style", "fade"),
            ("animation", "pane_open_style", "slide-up"),
            ("animation", "pane_open_style", "slide-down"),
            ("animation", "pane_open_style", "slide-left"),
            ("animation", "pane_open_style", "fade-slide-up"),
            ("prediction", "mode", "never"),
            ("prediction", "mode", "always"),
            ("prediction", "mode", "adaptive"),
        ];
        for (section, key, value) in cases {
            let toml_str = format!("[{section}]\n{key} = \"{value}\"");
            let result: Result<CiriConfig, _> = toml::from_str(&toml_str);
            result.unwrap_or_else(|e| {
                panic!("[{section}] {key} = \"{value}\" should parse, got: {e}")
            });
        }
    }

    #[test]
    fn load_remote_hosts_config() {
        let path = temp_config_path("remote-hosts");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"
                [remote]
                enabled = true
                port = 9999

                [[remote.hosts]]
                name = "dev-box"
                host = "192.168.1.100"

                [[remote.hosts]]
                name = "prod"
                host = "10.0.0.1"
                port = 8888
                ssh_port = 2222
            "#,
        )
        .unwrap();

        let config = load_from_path(&path).unwrap();
        assert!(config.remote.enabled);
        assert_eq!(config.remote.port, 9999);
        assert_eq!(config.remote.hosts.len(), 2);
        assert_eq!(config.remote.hosts[0].name, "dev-box");
        assert_eq!(config.remote.hosts[0].port, 7890); // default
        assert_eq!(config.remote.hosts[0].ssh_port, 22); // default
        assert_eq!(config.remote.hosts[1].port, 8888);
        assert_eq!(config.remote.hosts[1].ssh_port, 2222);
    }

    #[test]
    fn load_layout_preset_widths() {
        let path = temp_config_path("layout-presets");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"
                [layout]
                center_focused_column = "never"
                preset_widths = [
                    { proportion = 0.33 },
                    { fixed = 800.0 },
                    { proportion = 1.0 },
                ]
            "#,
        )
        .unwrap();

        let config = load_from_path(&path).unwrap();
        assert_eq!(
            config.layout.center_focused_column,
            CenterStrategy::Never
        );
        assert_eq!(config.layout.preset_widths.len(), 3);
        match &config.layout.preset_widths[1] {
            PresetWidth::Fixed { fixed } => assert_eq!(*fixed, 800.0),
            _ => panic!("expected Fixed variant"),
        }
    }
}
