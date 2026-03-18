use anyhow::Result;
use std::fs;
use std::path::Path;

use crate::state::SessionState;

pub fn restore_session(name: &str, dir: &Path) -> Result<Option<SessionState>> {
    crate::save::validate_session_name(name)?;
    let path = dir.join(format!("{name}.json"));
    if !path.exists() {
        return Ok(None);
    }
    let json = fs::read_to_string(path)?;
    let state: SessionState = serde_json::from_str(&json)?;
    Ok(Some(state))
}

pub fn list_sessions(dir: &Path) -> Result<Vec<String>> {
    let mut sessions = Vec::new();
    if !dir.exists() {
        return Ok(sessions);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            if let Some(name) = path.file_stem() {
                sessions.push(name.to_string_lossy().to_string());
            }
        }
    }
    sessions.sort();
    Ok(sessions)
}

pub fn delete_session(name: &str, dir: &Path) -> Result<()> {
    crate::save::validate_session_name(name)?;
    let path = dir.join(format!("{name}.json"));
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
