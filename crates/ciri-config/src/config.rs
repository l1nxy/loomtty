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
        .map(|d| d.join("ciri").join("config.toml"))
        .unwrap_or_else(|| {
            log::warn!("cannot determine config directory, using fallback");
            PathBuf::from(if cfg!(windows) {
                r"C:\Users\Default\AppData\Roaming"
            } else {
                "/tmp/.config"
            })
            .join("ciri")
            .join("config.toml")
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

    #[test]
    fn validate_rejects_invalid_values() {
        let mut config = CiriConfig::default();
        config.animation.drag_opacity = 1.5;
        assert!(config.validate().is_err());

        let mut config = CiriConfig::default();
        config.appearance.border_width = -2.0;
        assert!(config.validate().is_err());

        let mut config = CiriConfig::default();
        config.appearance.inactive_opacity = 1.5;
        assert!(config.validate().is_err());

        let mut config = CiriConfig::default();
        config.font.size = 0.0;
        assert!(config.validate().is_err());

        let mut config = CiriConfig::default();
        config.terminal.cursor_opacity = -0.25;
        assert!(config.validate().is_err());

        let mut config = CiriConfig::default();
        config.render.frame_interval_ms = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn validate_rejects_invalid_runtime_semantics() {
        let mut config = CiriConfig::default();
        config.animation.overview_zoom_fit = 0.0;
        assert!(config.validate().is_err());

        let mut config = CiriConfig::default();
        config.animation.zoom_threshold = 0.0;
        assert!(config.validate().is_err());

        let mut config = CiriConfig::default();
        config.statusbar.padding_ratio = -0.1;
        assert!(config.validate().is_err());

        let mut config = CiriConfig::default();
        config.terminal.default_cols = 0;
        assert!(config.validate().is_err());

        let mut config = CiriConfig::default();
        config.terminal.default_rows = 0;
        assert!(config.validate().is_err());

        let mut config = CiriConfig::default();
        config.terminal.cursor_blink_interval_ms = 0;
        assert!(config.validate().is_err());

        let mut config = CiriConfig::default();
        config.terminal.scrollback_lines = 0;
        assert!(config.validate().is_err());
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
}
