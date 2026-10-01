//! Serialize daemon ownership before probing or replacing a stale socket.
use std::fs::{File, OpenOptions, Permissions};
use std::io;
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

pub(super) fn bind(path: &Path) -> io::Result<(UnixListener, File)> {
    // Keep this inode in place even after shutdown. Unlinking a lock file lets
    // competing starters lock different inodes and both believe they own it.
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path.with_extension("lock"))?;
    if !lock.metadata()?.is_file() {
        return Err(io::Error::other("server lock is not a regular file"));
    }
    lock.try_lock().map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => io::Error::new(
            io::ErrorKind::AddrInUse,
            "another loomtty server owns this socket",
        ),
        std::fs::TryLockError::Error(error) => error,
    })?;

    let listener = match UnixListener::bind(path) {
        Ok(listener) => listener,
        Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
            // Older daemons do not hold our lock. A successful probe must
            // preserve their socket and all their existing client sessions.
            match UnixStream::connect(path) {
                Ok(_) => return Err(error),
                Err(probe)
                    if matches!(
                        probe.kind(),
                        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                    ) => {}
                Err(probe) => return Err(probe),
            }
            match path.symlink_metadata() {
                Ok(metadata) if metadata.file_type().is_socket() => {
                    std::fs::remove_file(path)?;
                }
                Ok(_) => return Err(io::Error::other("refusing to replace a non-socket path")),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            UnixListener::bind(path)?
        }
        Err(error) => return Err(error),
    };
    // The parent directory is already private. Avoid changing process-wide
    // umask while Tokio workers can create other files.
    std::fs::set_permissions(path, Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    Ok((listener, lock))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, symlink};
    use std::sync::{Arc, Barrier};

    #[test]
    fn concurrent_starters_have_one_owner() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("loom.sock");
        let barrier = Arc::new(Barrier::new(8));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let barrier = barrier.clone();
                let path = path.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let result = bind(&path);
                    barrier.wait(); // Keep the winner's lock alive during every attempt.
                    result
                })
            })
            .collect();
        let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        for error in results.iter().filter_map(|r| r.as_ref().err()) {
            assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
        }
        let client = UnixStream::connect(&path).unwrap();
        let (listener, _) = results.iter().find_map(|r| r.as_ref().ok()).unwrap();
        assert!(listener.accept().is_ok());
        drop(client);
        assert!(!path.with_extension("tmp").exists());
    }

    #[test]
    fn preserves_live_legacy_listener() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("loom.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let inode = path.metadata().unwrap().ino();
        assert_eq!(bind(&path).unwrap_err().kind(), io::ErrorKind::AddrInUse);
        assert_eq!(path.metadata().unwrap().ino(), inode);
        assert!(listener.accept().is_ok());
    }

    #[test]
    fn replaces_stale_socket_after_owner_exits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("loom.sock");
        drop(bind(&path).unwrap());
        let (listener, _lock) = bind(&path).unwrap();
        let _client = UnixStream::connect(&path).unwrap();
        assert!(listener.accept().is_ok());
        assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn preserves_non_socket_and_symlink_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("loom.sock");
        std::fs::write(&path, "keep").unwrap();
        assert!(bind(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep");
        std::fs::remove_file(&path).unwrap();
        let target = dir.path().join("target");
        std::fs::write(&target, "keep").unwrap();
        symlink(&target, &path).unwrap();
        assert!(bind(&path).is_err());
        assert!(path.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep");
    }

    #[test]
    fn rejects_symlink_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("loom.sock");
        let target = dir.path().join("target");
        std::fs::write(&target, "keep").unwrap();
        symlink(&target, path.with_extension("lock")).unwrap();
        assert!(bind(&path).is_err());
        assert!(!path.exists());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "keep");
    }
}
