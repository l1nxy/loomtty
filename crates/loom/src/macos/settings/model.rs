use crate::app::ui::settings_panel::schema::{self, FieldKind, SettingsField};
use loom_config::{config::LoomConfig, writer::EditableConfig};

#[derive(Clone, Debug)]
pub(in crate::macos) enum Value {
    Bool(bool),
    Number(String),
    Choice(String),
}

pub(super) fn apply(
    config: &mut LoomConfig,
    field: SettingsField,
    value: &Value,
) -> Result<bool, String> {
    match (value, &schema::meta(field).kind) {
        (Value::Bool(value), FieldKind::Bool) => {
            if schema::read_bool(field, config) == *value {
                return Ok(false);
            }
            schema::toggle(field, config);
            Ok(true)
        }
        (Value::Number(value), FieldKind::Int { .. } | FieldKind::Float { .. }) => {
            let parsed = value
                .trim()
                .parse()
                .map_err(|_| "Enter a number.".to_string())?;
            schema::set_number(field, config, parsed)
        }
        (Value::Choice(value), FieldKind::Enum { .. }) => {
            if !schema::enum_variants(field).contains(value) {
                return Err("Choose one of the available values.".into());
            }
            Ok(schema::set_enum(field, config, value))
        }
        _ => Err("Invalid setting value.".into()),
    }
}

/// Read fresh config for every edit, then change only this field in the
/// comment-preserving writer. A failed save never applies a false preview.
pub(in crate::macos) fn save(field: SettingsField, value: &Value) -> Result<LoomConfig, String> {
    let mut config = LoomConfig::load().map_err(|error| error.to_string())?;
    if apply(&mut config, field, value)? {
        config
            .validate_schema()
            .map_err(|error| error.to_string())?;
        let mut writer = EditableConfig::load().map_err(|error| error.to_string())?;
        schema::write_to_disk(field, &config, &mut writer);
        writer.save().map_err(|error| error.to_string())?;
    }
    LoomConfig::load().map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_number_edits_preserve_the_previous_value() {
        let mut config = LoomConfig::default();
        let original = config.font.size;
        for input in ["", "no", "NaN", "inf", "-1", "999999"] {
            assert!(
                apply(
                    &mut config,
                    SettingsField::FontSize,
                    &Value::Number(input.into())
                )
                .is_err()
            );
            assert_eq!(config.font.size, original);
        }
        assert!(
            apply(
                &mut config,
                SettingsField::TerminalScrollbackLines,
                &Value::Number("100.5".into())
            )
            .is_err()
        );
    }
    #[test]
    fn typed_values_use_the_shared_schema() {
        let mut config = LoomConfig::default();
        apply(
            &mut config,
            SettingsField::FontSize,
            &Value::Number("18.5".into()),
        )
        .unwrap();
        assert_eq!(config.font.size, 18.5);
        apply(
            &mut config,
            SettingsField::TerminalCopyOnSelect,
            &Value::Bool(true),
        )
        .unwrap();
        assert!(schema::read_bool(
            SettingsField::TerminalCopyOnSelect,
            &config
        ));
        apply(
            &mut config,
            SettingsField::ThemePreset,
            &Value::Choice("dracula".into()),
        )
        .unwrap();
        assert_eq!(config.theme.preset, "dracula");
        assert!(
            apply(
                &mut config,
                SettingsField::ThemePreset,
                &Value::Choice("missing-theme".into())
            )
            .is_err()
        );
    }
}
