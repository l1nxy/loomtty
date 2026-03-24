use anyhow::Result;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use crate::state::SessionState;

pub fn restore_session(name: &str, dir: &Path) -> Result<Option<SessionState>> {
    crate::save::validate_session_name(name)?;
    let path = crate::save::session_path(dir, name).final_path;
    if !path.is_file() {
        return Ok(None);
    }

    let json = fs::read_to_string(path)?;
    serde_json::from_str(&json).map(Some).map_err(Into::into)
}

pub fn list_sessions(dir: &Path) -> Result<Vec<String>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if let Some(name) = session_name_from_path(&entry.path()) {
            sessions.push(name);
        }
    }
    sessions.sort();
    Ok(sessions)
}

pub fn delete_session(name: &str, dir: &Path) -> Result<()> {
    crate::save::validate_session_name(name)?;
    match fs::remove_file(crate::save::session_path(dir, name).final_path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn session_name_from_path(path: &Path) -> Option<String> {
    (path.extension().is_some_and(|ext| ext == "json"))
        .then(|| path.file_stem())
        .flatten()
        .map(|name| name.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save::save_session;
    use crate::state::{SavedColumn, SavedTile, SavedWorkspace, SessionState};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn save_restore_list_and_delete_round_trip() {
        let dir = unique_test_dir("session-round-trip");
        let alpha = sample_session("alpha");
        let beta = sample_session("beta");

        save_session(&beta, &dir).unwrap();
        save_session(&alpha, &dir).unwrap();

        assert_eq!(
            restore_session("alpha", &dir).unwrap().unwrap().name,
            "alpha"
        );
        assert_eq!(list_sessions(&dir).unwrap(), vec!["alpha", "beta"]);

        delete_session("alpha", &dir).unwrap();
        assert!(restore_session("alpha", &dir).unwrap().is_none());
        assert_eq!(list_sessions(&dir).unwrap(), vec!["beta"]);

        delete_session("missing", &dir).unwrap();
    }

    #[test]
    fn list_sessions_propagates_directory_entry_errors() {
        let dir = unique_test_dir("session-entry-errors");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("alpha.json"), "{}").unwrap();

        let blocked_dir = dir.join("blocked");
        fs::create_dir(&blocked_dir).unwrap();
        fs::write(blocked_dir.join("beta.json"), "{}").unwrap();
        fs::set_permissions(&blocked_dir, fs::Permissions::from_mode(0o0)).unwrap();

        let err = list_sessions(&blocked_dir).unwrap_err();
        assert_eq!(err.downcast_ref::<std::io::Error>().unwrap().kind(), ErrorKind::PermissionDenied);

        fs::set_permissions(&blocked_dir, fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn sample_session(name: &str) -> SessionState {
        SessionState {
            name: name.to_string(),
            workspaces: vec![SavedWorkspace {
                columns: vec![SavedColumn {
                    tiles: vec![SavedTile {
                        pane_id: 1,
                        weight: 1.0,
                        cwd: Some("/tmp".to_string()),
                        title: Some("shell".to_string()),
                    }],
                    active_tile_idx: 0,
                    width_proportion: 1.0,
                    width_fixed_px: None,
                }],
                active_column_idx: 0,
            }],
            active_workspace_idx: 0,
        }
    }

    fn unique_test_dir(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ciri-session-{label}-{unique}"))
    }
}
