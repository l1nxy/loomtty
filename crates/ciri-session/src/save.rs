use anyhow::{bail, Result};
use std::fs;
use std::path::Path;

use crate::state::SessionState;

/// Validate session name to prevent path traversal.
pub fn validate_session_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains('\0')
    {
        bail!("invalid session name: {name:?}");
    }
    Ok(())
}

pub fn save_session(state: &SessionState, dir: &Path) -> Result<()> {
    validate_session_name(&state.name)?;
    fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.json", state.name));
    let tmp_path = dir.join(format!(".{}.json.tmp", state.name));
    let json = serde_json::to_string_pretty(state)?;
    // Write to temp file first, then atomic rename to prevent corruption.
    fs::write(&tmp_path, &json)?;
    fs::rename(&tmp_path, &path)?;
    Ok(())
}
