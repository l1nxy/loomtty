//! Finder Services and directory launches. The directory is session startup
//! data, never text injected into an already-running shell.
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use objc2::runtime::{AnyObject, Sel};
use objc2::{ClassType, Message, rc::Retained};
use objc2_app_kit::{NSApplication, NSPasteboard};
use objc2_foundation::{NSArray, NSString, NSURL};

use super::{Command, native};

fn directory_for_path(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("Choose an absolute local file or folder.".into());
    }
    let metadata = path
        .metadata()
        .map_err(|error| format!("Cannot open {}: {error}", path.display()))?;
    if metadata.is_dir() {
        Ok(path.to_owned())
    } else if metadata.is_file() {
        path.parent()
            .map(Path::to_owned)
            .ok_or_else(|| "The file has no parent folder.".into())
    } else {
        Err("Choose a regular file or folder.".into())
    }
}

fn directories(urls: &NSArray<NSURL>) -> Result<Vec<PathBuf>, String> {
    let mut directories = Vec::new();
    for url in urls {
        if !url.isFileURL() {
            return Err("Only local file URLs can be opened in loomtty.".into());
        }
        let path = url
            .path()
            .ok_or_else(|| "The file URL has no path.".to_owned())?;
        let directory = directory_for_path(Path::new(&path.to_string()))?;
        if !directories.contains(&directory) {
            directories.push(directory);
        }
        if directories.len() > 32 {
            return Err("Choose at most 32 folders at a time.".into());
        }
    }
    if directories.is_empty() {
        return Err("Choose a local file or folder.".into());
    }
    Ok(directories)
}

pub(super) extern "C" fn open_directory_service(
    _this: &AnyObject,
    _selector: Sel,
    pasteboard: &NSPasteboard,
    user_data: Option<&NSString>,
    error: *mut *mut NSString,
) {
    let tab = user_data.is_some_and(|data| data.to_string() == "tab");
    let result = (|| {
        // SAFETY: NSURL implements NSPasteboardReading. No option values or
        // dynamically chosen classes are passed to the pasteboard decoder.
        let objects = unsafe {
            pasteboard.readObjectsForClasses_options(&NSArray::from_slice(&[NSURL::class()]), None)
        }
        .ok_or_else(|| "Choose local files or folders in Finder.".to_owned())?;
        let urls: Vec<_> = objects
            .iter()
            .filter_map(|object| object.downcast_ref::<NSURL>().map(|url| url.retain()))
            .collect();
        let refs: Vec<_> = urls.iter().map(|url| &**url).collect();
        directories(&NSArray::from_slice(&refs))
    })();
    match result {
        Ok(directories) => {
            for directory in directories {
                native::dispatch(Command::OpenDirectory { directory, tab });
            }
        }
        Err(message) => {
            // SAFETY: Services supplies an optional autoreleasing NSString**
            // result. The autorelease pool outlives this synchronous callback.
            if !error.is_null() {
                unsafe {
                    *error = Retained::autorelease_ptr(NSString::from_str(&message));
                }
            }
        }
    }
}

pub(super) extern "C" fn open_urls(
    _this: &AnyObject,
    _selector: Sel,
    _app: &NSApplication,
    urls: &NSArray<NSURL>,
) {
    match directories(urls) {
        Ok(directories) => {
            for directory in directories {
                native::dispatch(Command::OpenDirectory {
                    directory,
                    tab: false,
                });
            }
        }
        Err(error) => log::warn!("could not open Finder directory: {error}"),
    }
}

/// Reserve an unused, fully written session snapshot before the client
/// connects. The server already restores cwd from this format. A hard-link
/// publish is atomic and never replaces an existing saved session.
pub fn seed_session(
    state_dir: &Path,
    existing: &[String],
    cwd: &Path,
    config: &loom_config::LoomConfig,
) -> anyhow::Result<String> {
    use loom_session::state::{SavedColumn, SavedTile, SavedWorkspace, SessionState};
    let cwd = directory_for_path(cwd).map_err(anyhow::Error::msg)?;
    let cwd = cwd
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("directory name is not UTF-8"))?;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(state_dir)?;
    let staging = state_dir.join("macos");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&staging)?;
    let mut names = existing.to_vec();
    names.extend(loom_session::restore::list_sessions(state_dir)?);
    let (width_proportion, width_fixed_px) =
        match config.layout.column_sizing().width_at(config.window.width) {
            loom_config::PresetWidth::Proportion { proportion } => (proportion, None),
            loom_config::PresetWidth::Fixed { fixed } => (1.0, Some(fixed)),
        };
    loop {
        let name = loom_session::names::unique_name(&names);
        let state = SessionState {
            name: name.clone(),
            active_workspace_idx: 0,
            workspaces: vec![SavedWorkspace {
                active_column_idx: 0,
                columns: vec![SavedColumn {
                    width_proportion,
                    width_fixed_px,
                    active_tile_idx: 0,
                    tiles: vec![SavedTile {
                        pane_id: 0,
                        weight: 1.0,
                        cwd: Some(cwd.to_owned()),
                        title: None,
                        agent: None,
                    }],
                }],
            }],
        };
        state.validate_structure()?;
        let temporary = staging.join(format!("seed-{}-{name}.tmp", std::process::id()));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        let result = (|| -> std::io::Result<()> {
            file.write_all(&serde_json::to_vec(&state)?)?;
            file.sync_all()?;
            std::fs::hard_link(&temporary, state_dir.join(format!("{name}.json")))
        })();
        let _ = std::fs::remove_file(&temporary);
        match result {
            Ok(()) => return Ok(name),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => names.push(name),
            Err(error) => return Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_paths_choose_parent_without_interpreting_shell_characters() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("spaces ' and $(echo unsafe)");
        std::fs::create_dir(&directory).unwrap();
        let file = directory.join("source.rs");
        std::fs::write(&file, "").unwrap();
        assert_eq!(directory_for_path(&file).unwrap(), directory);
        assert_eq!(directory_for_path(&directory).unwrap(), directory);
        assert!(directory_for_path(Path::new("relative")).is_err());
        assert!(directory_for_path(&root.path().join("missing")).is_err());
    }

    #[test]
    fn seeded_sessions_roundtrip_cwd_and_do_not_replace_existing_sessions() {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join("中文 folder");
        std::fs::create_dir(&cwd).unwrap();
        let state_dir = root.path().join("state");
        let first = seed_session(&state_dir, &[], &cwd, &Default::default()).unwrap();
        let saved = std::fs::read(state_dir.join(format!("{first}.json"))).unwrap();
        let second = seed_session(
            &state_dir,
            std::slice::from_ref(&first),
            &cwd,
            &Default::default(),
        )
        .unwrap();
        assert_ne!(first, second);
        assert_eq!(
            std::fs::read(state_dir.join(format!("{first}.json"))).unwrap(),
            saved
        );
        let restored = loom_session::restore::restore_session(&first, &state_dir)
            .unwrap()
            .unwrap();
        assert_eq!(
            restored.workspaces[0].columns[0].tiles[0].cwd.as_deref(),
            cwd.to_str()
        );
        assert!(
            std::fs::read_dir(state_dir.join("macos"))
                .unwrap()
                .next()
                .is_none()
        );
    }
}
