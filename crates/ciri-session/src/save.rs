use anyhow::Result;
use std::fs;
use std::io::Write;
use std::path::Path;

use crate::names;
use crate::state::SessionState;

/// Validate session name. Delegates to names::validate_name.
pub fn validate_session_name(name: &str) -> Result<()> {
    names::validate_name(name).map_err(|e| anyhow::anyhow!(e))
}

pub fn save_session(state: &SessionState, dir: &Path) -> Result<()> {
    validate_session_name(&state.name)?;
    state.validate_structure()?;
    create_dir_private(dir)?;
    let json = serde_json::to_string_pretty(state)?;
    let paths = session_paths(dir, &state.name);
    write_private(&paths.temp, json.as_bytes())?;
    fs::rename(&paths.temp, &paths.final_path)?;
    Ok(())
}

/// Create directory with mode 0o700 on Unix (private to current user).
fn create_dir_private(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Write file with mode 0o600 on Unix (readable only by owner).
fn write_private(path: &Path, data: &[u8]) -> Result<()> {
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    f.write_all(data)?;
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
