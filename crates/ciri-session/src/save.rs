use anyhow::Result;
use std::fs;
use std::path::Path;

use crate::state::SessionState;

pub fn save_session(state: &SessionState, dir: &Path) -> Result<()> {
    fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.json", state.name));
    let json = serde_json::to_string_pretty(state)?;
    fs::write(path, json)?;
    Ok(())
}
