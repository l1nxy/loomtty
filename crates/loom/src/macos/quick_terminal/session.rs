//! Quick Terminal uses an ordinary persistent loom session, but must not become
//! the target of an unrelated default launch. Explicit CLI attaches still work.
use std::path::{Path, PathBuf};

fn marker(state_dir: &Path, name: &str) -> Option<PathBuf> {
    loom_session::names::validate_name(name).ok()?;
    Some(state_dir.join("macos-quick-sessions").join(name))
}

pub fn is_quick_session(state_dir: &Path, name: &str) -> bool {
    marker(state_dir, name).is_some_and(|path| path.is_file())
}

pub fn allocate(state_dir: &Path, existing: &[String]) -> std::io::Result<String> {
    let directory = state_dir.join("macos-quick-sessions");
    std::fs::create_dir_all(&directory)?;
    let mut occupied: Vec<String> = existing
        .iter()
        .filter_map(|name| name.strip_prefix("quick-").map(str::to_owned))
        .collect();
    loop {
        let suffix = loom_session::names::unique_name(&occupied);
        let name = format!("quick-{suffix}");
        // create_new coordinates independent client launches without a shared
        // read/modify/write registry. Keep markers as long as saved sessions.
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(&name))
        {
            Ok(_) => return Ok(name),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                occupied.push(suffix);
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_sessions_are_explicitly_marked_and_never_reused() {
        let directory = tempfile::tempdir().unwrap();
        let first = allocate(directory.path(), &[]).unwrap();
        let second = allocate(directory.path(), &[]).unwrap();
        assert_ne!(first, second);
        assert!(is_quick_session(directory.path(), &first));
        assert!(is_quick_session(directory.path(), &second));
        assert!(!is_quick_session(directory.path(), "quick-my-own-session"));
        assert!(!is_quick_session(directory.path(), "../outside"));
    }
}
