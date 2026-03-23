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
    // Follow XDG convention
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".config")
        });
    config_dir.join("ciri").join("templates")
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
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                names.push(stem.to_string());
            }
        }
    }
    names.sort();
    Ok(names)
}

/// Load a template by name.
pub fn load_template(name: &str) -> Result<LayoutTemplate> {
    // Validate name (prevent path traversal)
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        anyhow::bail!("invalid template name: {name:?}");
    }
    let path = templates_dir().join(format!("{name}.toml"));
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read template '{name}' at {}", path.display()))?;
    let template: LayoutTemplate = toml::from_str(&content)
        .with_context(|| format!("failed to parse template '{name}'"))?;
    // Validate: at least one workspace with at least one column with at least one tile
    if template.workspaces.is_empty() {
        anyhow::bail!("template '{name}' has no workspaces");
    }
    for (i, ws) in template.workspaces.iter().enumerate() {
        if ws.columns.is_empty() {
            anyhow::bail!("template '{name}' workspace {i} has no columns");
        }
        for (j, col) in ws.columns.iter().enumerate() {
            if col.tiles.is_empty() {
                anyhow::bail!("template '{name}' workspace {i} column {j} has no tiles");
            }
        }
    }
    Ok(template)
}

/// Save a template to disk.
pub fn save_template(name: &str, template: &LayoutTemplate) -> Result<()> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        anyhow::bail!("invalid template name: {name:?}");
    }
    let dir = templates_dir();
    std::fs::create_dir_all(&dir).context("failed to create templates directory")?;
    let path = dir.join(format!("{name}.toml"));
    let content = toml::to_string_pretty(template).context("failed to serialize template")?;
    std::fs::write(&path, content)
        .with_context(|| format!("failed to write template to {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
