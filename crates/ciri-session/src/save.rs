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
    let path = dir.join(format!("{}.json", state.name));
    let tmp_path = dir.join(format!(".{}.json.tmp", state.name));
    let json = serde_json::to_string_pretty(state)?;
    fs::write(&tmp_path, &json)?;
    fs::rename(&tmp_path, &path)?;
    Ok(())
}
