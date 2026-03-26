use anyhow::Result;
use std::fs;
use std::path::Path;

use crate::names;
use crate::state::SessionState;

/// Validate session name. Delegates to names::validate_name.
pub fn validate_session_name(name: &str) -> Result<()> {
    names::validate_name(name).map_err(|e| anyhow::anyhow!(e))
}

pub fn save_session(state: &SessionState, dir: &Path) -> Result<()> {
    validate_session_name(&state.name)?;
    fs::create_dir_all(dir)?;
    let json = serde_json::to_string_pretty(state)?;
    let paths = session_paths(dir, &state.name);
    fs::write(&paths.temp, &json)?;
    fs::rename(&paths.temp, &paths.final_path)?;
    Ok(())
}

pub(crate) fn session_path(dir: &Path, name: &str) -> SessionPaths {
    session_paths(dir, name)
}

pub(crate) struct SessionPaths {
    pub final_path: std::path::PathBuf,
    pub temp: std::path::PathBuf,
}

fn session_paths(dir: &Path, name: &str) -> SessionPaths {
    SessionPaths {
        final_path: dir.join(format!("{name}.json")),
        temp: dir.join(format!(".{name}.json.tmp")),
    }
}
