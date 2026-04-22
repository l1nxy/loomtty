use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A layout template defining workspace/column structure.
/// Stored as TOML in $XDG_CONFIG_HOME/ciri/templates/<name>.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutTemplate {
    /// Optional description shown in template list.
    #[serde(default)]
    pub description: Option<String>,
    pub workspaces: Vec<TemplateWorkspace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateWorkspace {
    pub columns: Vec<TemplateColumn>,
    /// Which column index to focus initially (default 0).
    #[serde(default)]
    pub active_column: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateColumn {
    pub tiles: Vec<TemplateTile>,
    #[serde(default)]
    pub width: Option<TemplateWidth>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TemplateWidth {
    Proportion { proportion: f64 },
    Fixed { fixed: f64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateTile {
    /// Shell command to run (empty string = default shell).
    #[serde(default)]
    pub command: String,
    /// Working directory (empty = inherit).
    #[serde(default)]
    pub cwd: String,
    /// Tile weight for vertical stacking.
    #[serde(default = "default_weight")]
    pub weight: f64,
}

fn default_weight() -> f64 {
    1.0
}

/// Get the templates directory path.
pub fn templates_dir() -> PathBuf {
    config_dir().join("ciri").join("templates")
}

/// List all available templates (name without .toml extension).
pub fn list_templates() -> Result<Vec<String>> {
    let dir = templates_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut names = Vec::new();
    for entry in std::fs::read_dir(&dir).context("failed to read templates directory")? {
        let entry = entry?;
        if let Some(name) = template_name_from_path(entry.path()) {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// Load a template by name.
pub fn load_template(name: &str) -> Result<LayoutTemplate> {
    validate_template_name(name)?;
    let path = template_path(name);
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read template '{name}' at {}", path.display()))?;
    let template: LayoutTemplate =
        toml::from_str(&content).with_context(|| format!("failed to parse template '{name}'"))?;
    validate_template_shape(name, &template)?;
    Ok(template)
}

/// Save a template to disk.
pub fn save_template(name: &str, template: &LayoutTemplate) -> Result<()> {
    validate_template_name(name)?;
    let path = template_path(name);
    let parent = path.parent().context("template path missing parent")?;
    std::fs::create_dir_all(parent).context("failed to create templates directory")?;
    let content = toml::to_string_pretty(template).context("failed to serialize template")?;
    std::fs::write(&path, content)
        .with_context(|| format!("failed to write template to {}", path.display()))?;
    Ok(())
}

fn config_dir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| fallback_config_dir())
}

fn fallback_config_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".config")
}

fn template_path(name: &str) -> PathBuf {
    templates_dir().join(format!("{name}.toml"))
}

fn template_name_from_path(path: PathBuf) -> Option<String> {
    (path.extension().and_then(|e| e.to_str()) == Some("toml"))
        .then(|| path.file_stem().and_then(|s| s.to_str()))
        .flatten()
        .map(str::to_string)
}

fn validate_template_name(name: &str) -> Result<()> {
    crate::is_plain_name(name)
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("invalid template name: {name:?}"))
}

fn validate_template_shape(name: &str, template: &LayoutTemplate) -> Result<()> {
    if template.workspaces.is_empty() {
        anyhow::bail!("template '{name}' has no workspaces");
    }

    for (workspace_idx, workspace) in template.workspaces.iter().enumerate() {
        validate_workspace(name, workspace_idx, workspace)?;
    }

    Ok(())
}

fn validate_workspace(
    name: &str,
    workspace_idx: usize,
    workspace: &TemplateWorkspace,
) -> Result<()> {
    if workspace.columns.is_empty() {
        anyhow::bail!("template '{name}' workspace {workspace_idx} has no columns");
    }

    if workspace.active_column >= workspace.columns.len() {
        anyhow::bail!(
            "template '{name}' workspace {workspace_idx} active column {} out of bounds for {} columns",
            workspace.active_column,
            workspace.columns.len()
        );
    }

    for (column_idx, column) in workspace.columns.iter().enumerate() {
        if column.tiles.is_empty() {
            anyhow::bail!(
                "template '{name}' workspace {workspace_idx} column {column_idx} has no tiles"
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn roundtrip_template() {
        let template = LayoutTemplate {
            description: Some("test layout".to_string()),
            workspaces: vec![TemplateWorkspace {
                columns: vec![
                    TemplateColumn {
                        tiles: vec![TemplateTile {
                            command: String::new(),
                            cwd: String::new(),
                            weight: 1.0,
                        }],
                        width: Some(TemplateWidth::Proportion { proportion: 0.5 }),
                    },
                    TemplateColumn {
                        tiles: vec![
                            TemplateTile {
                                command: "htop".to_string(),
                                cwd: "/tmp".to_string(),
                                weight: 2.0,
                            },
                            TemplateTile {
                                command: String::new(),
                                cwd: String::new(),
                                weight: 1.0,
                            },
                        ],
                        width: None,
                    },
                ],
                active_column: 0,
            }],
        };
        let toml_str = toml::to_string_pretty(&template).unwrap();
        let parsed: LayoutTemplate = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.workspaces.len(), 1);
        assert_eq!(parsed.workspaces[0].columns.len(), 2);
        assert_eq!(parsed.workspaces[0].columns[1].tiles.len(), 2);
        assert_eq!(parsed.workspaces[0].columns[1].tiles[0].command, "htop");
    }

    #[test]
    fn invalid_template_name() {
        assert!(load_template("").is_err());
        assert!(load_template("../etc/passwd").is_err());
        assert!(load_template("foo/bar").is_err());
    }

    #[test]
    fn save_template_allows_invalid_shape_until_load_time() {
        let template = LayoutTemplate {
            description: None,
            workspaces: Vec::new(),
        };

        save_template("empty", &template).unwrap();

        let err = load_template("empty").unwrap_err();
        assert!(err.to_string().contains("has no workspaces"));
    }

    #[cfg(unix)]
    #[test]
    fn list_templates_propagates_directory_entry_errors() {
        let config_home = unique_config_home("template-entry-errors");
        let templates = config_home.join("ciri").join("templates");
        fs::create_dir_all(&templates).unwrap();
        fs::write(templates.join("alpha.toml"), "").unwrap();
        fs::set_permissions(&templates, fs::Permissions::from_mode(0o0)).unwrap();

        let previous = std::env::var_os("XDG_CONFIG_HOME");
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", &config_home);
        }

        let err = list_templates().unwrap_err();
        assert_eq!(
            err.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::PermissionDenied
        );

        unsafe {
            match previous {
                Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
        }
        fs::set_permissions(&templates, fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[test]
    fn load_template_rejects_templates_without_columns() {
        let template = LayoutTemplate {
            description: None,
            workspaces: vec![TemplateWorkspace {
                columns: Vec::new(),
                active_column: 0,
            }],
        };

        save_template("empty-columns", &template).unwrap();
        let err = load_template("empty-columns").unwrap_err();
        assert!(err.to_string().contains("workspace 0 has no columns"));
    }

    #[test]
    fn load_template_rejects_templates_without_tiles() {
        let template = LayoutTemplate {
            description: None,
            workspaces: vec![TemplateWorkspace {
                columns: vec![TemplateColumn {
                    tiles: Vec::new(),
                    width: None,
                }],
                active_column: 0,
            }],
        };

        save_template("empty-tiles", &template).unwrap();
        let err = load_template("empty-tiles").unwrap_err();
        assert!(err.to_string().contains("column 0 has no tiles"));
    }

    #[test]
    fn load_template_rejects_out_of_bounds_active_column() {
        let template = LayoutTemplate {
            description: None,
            workspaces: vec![TemplateWorkspace {
                columns: vec![TemplateColumn {
                    tiles: vec![TemplateTile {
                        command: String::new(),
                        cwd: String::new(),
                        weight: 1.0,
                    }],
                    width: None,
                }],
                active_column: 1,
            }],
        };

        save_template("active-column-oob", &template).unwrap();
        let err = load_template("active-column-oob").unwrap_err();
        assert!(err.to_string().contains("active column 1 out of bounds"));
    }

    fn unique_config_home(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ciri-session-{label}-{}", unique_suffix()))
    }

    fn unique_suffix() -> u128 {
        use std::time::{SystemTime, UNIX_EPOCH};

        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }
}
