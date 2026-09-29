use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

const MAX_STATE_BYTES: u64 = 256 * 1024;
const MAX_WINDOWS: usize = 128;

/// Native frame in Cocoa points, independent of a display's backing scale.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame(pub [f64; 4]);

impl Frame {
    fn valid(&self) -> bool {
        self.0
            .iter()
            .all(|value| value.is_finite() && value.abs() <= 1_000_000.0)
            && (64.0..=32768.0).contains(&self.0[2])
            && (64.0..=32768.0).contains(&self.0[3])
    }

    /// Keep the whole window reachable after unplugging/rearranging displays.
    pub fn fit(&self, screens: &[Frame]) -> Self {
        let Some(screen) = screens
            .iter()
            .max_by(|a, b| self.intersection(a).total_cmp(&self.intersection(b)))
        else {
            return self.clone();
        };
        let screen = if self.intersection(screen) > 0.0 {
            screen
        } else {
            &screens[0]
        };
        let [sx, sy, sw, sh] = screen.0;
        let width = self.0[2].min(sw);
        let height = self.0[3].min(sh);
        Frame([
            self.0[0].clamp(sx, sx + sw - width),
            self.0[1].clamp(sy, sy + sh - height),
            width,
            height,
        ])
    }

    fn intersection(&self, other: &Self) -> f64 {
        let [x, y, w, h] = self.0;
        let [ox, oy, ow, oh] = other.0;
        ((x + w).min(ox + ow) - x.max(ox)).max(0.0) * ((y + h).min(oy + oh) - y.max(oy)).max(0.0)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Remote {
    pub host: String,
    pub port: u16,
    pub ssh_port: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tab {
    pub session: String,
    pub remote: Option<Remote>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub frame: Frame,
    pub tabs: Vec<Tab>,
    pub selected: usize,
    pub minimized: bool,
    pub zoomed: bool,
    pub fullscreen: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u8,
    pub groups: Vec<Group>,
    pub active_group: Option<usize>,
}

impl Snapshot {
    pub fn valid(&self) -> bool {
        self.version == 1
            && self
                .groups
                .iter()
                .map(|group| group.tabs.len())
                .sum::<usize>()
                <= MAX_WINDOWS
            && self
                .active_group
                .is_none_or(|index| index < self.groups.len())
            && self.groups.iter().all(|group| {
                group.frame.valid()
                    && !group.tabs.is_empty()
                    && group.selected < group.tabs.len()
                    && group.tabs.iter().all(|tab| {
                        loom_session::names::validate_name(&tab.session).is_ok()
                            && tab.remote.as_ref().is_none_or(|remote| {
                                crate::remote_validate::validate_fields(
                                    &remote.host,
                                    remote.port,
                                    remote.ssh_port,
                                )
                                .is_ok()
                            })
                    })
            })
    }
}

/// Only one default-launch process owns restoration. Explicit CLI clients do
/// not open this store; a second default launch cannot overwrite the owner.
pub struct Store {
    _lock: File,
    directory: PathBuf,
    committed: Option<Snapshot>,
    pending: Option<(Snapshot, Instant)>,
    pub frozen: bool,
}

impl Store {
    pub fn open(state_dir: &Path) -> std::io::Result<Option<Self>> {
        let directory = state_dir.join("macos");
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&directory)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(directory.join("windows.lock"))?;
        // SAFETY: fd is live and owned for this Store's lifetime. Closing it
        // releases the advisory lock, including after a process crash.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let error = std::io::Error::last_os_error();
            return if error.kind() == std::io::ErrorKind::WouldBlock {
                Ok(None)
            } else {
                Err(error)
            };
        }
        let committed = load(&directory.join("windows.json"));
        Ok(Some(Self {
            _lock: lock,
            directory,
            committed,
            pending: None,
            frozen: false,
        }))
    }

    pub fn loaded(&self) -> Option<Snapshot> {
        self.committed.clone()
    }

    pub fn observe(&mut self, state: Snapshot, now: Instant) {
        if self.frozen || !state.valid() {
            return;
        }
        if self.committed.as_ref() == Some(&state) {
            self.pending = None;
        } else if self
            .pending
            .as_ref()
            .is_none_or(|(pending, _)| pending != &state)
        {
            self.pending = Some((state, now + Duration::from_millis(250)));
        }
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.pending.as_ref().map(|(_, deadline)| *deadline)
    }

    pub fn flush(&mut self, force: bool) {
        let now = Instant::now();
        if self
            .pending
            .as_ref()
            .is_none_or(|(_, deadline)| !force && *deadline > now)
        {
            return;
        }
        let (state, _) = self.pending.take().expect("pending snapshot");
        match self.write(&state) {
            Ok(()) => self.committed = Some(state),
            Err(error) => {
                log::warn!("could not save macOS windows: {error}");
                self.pending = Some((state, now + Duration::from_secs(5)));
            }
        }
    }

    fn write(&self, state: &Snapshot) -> std::io::Result<()> {
        let bytes = serde_json::to_vec(state)?;
        let temporary = self.directory.join("windows.tmp");
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(temporary, self.directory.join("windows.json"))?;
        Ok(())
    }
}

fn load(path: &Path) -> Option<Snapshot> {
    let file = File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_STATE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return None;
    }
    let state: Snapshot = serde_json::from_slice(&bytes).ok()?;
    state.valid().then_some(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> Snapshot {
        Snapshot {
            version: 1,
            active_group: Some(0),
            groups: vec![Group {
                frame: Frame([120.0, 200.0, 800.0, 600.0]),
                tabs: vec![Tab {
                    session: "work".into(),
                    remote: None,
                }],
                selected: 0,
                minimized: false,
                zoomed: false,
                fullscreen: false,
            }],
        }
    }

    #[test]
    fn unchanged_events_do_not_delay_save_and_quit_freezes_the_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(directory.path()).unwrap().unwrap();
        let now = Instant::now();
        store.observe(sample(), now);
        let deadline = store.deadline();
        store.observe(sample(), now + Duration::from_millis(100));
        assert_eq!(store.deadline(), deadline);
        store.flush(true);
        store.frozen = true;
        store.observe(
            Snapshot {
                version: 1,
                groups: vec![],
                active_group: None,
            },
            now,
        );
        store.flush(true);
        drop(store);
        assert_eq!(
            Store::open(directory.path()).unwrap().unwrap().loaded(),
            Some(sample())
        );
        // App state must not become a phantom server session.
        assert!(
            loom_session::restore::list_sessions(directory.path())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn owner_lock_atomic_roundtrip_and_corrupt_state_fallback() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(directory.path()).unwrap().unwrap();
        assert!(Store::open(directory.path()).unwrap().is_none());
        store.observe(sample(), Instant::now());
        store.flush(true);
        assert_eq!(store.loaded(), Some(sample()));
        drop(store);
        let store = Store::open(directory.path()).unwrap().unwrap();
        assert_eq!(store.loaded(), Some(sample()));
        drop(store);
        std::fs::write(directory.path().join("macos/windows.json"), "{broken").unwrap();
        assert!(
            Store::open(directory.path())
                .unwrap()
                .unwrap()
                .loaded()
                .is_none()
        );
    }

    #[test]
    fn removed_display_and_scale_independent_negative_origins_remain_reachable() {
        let primary = Frame([0.0, 30.0, 1440.0, 850.0]);
        let left = Frame([-1920.0, 0.0, 1920.0, 1080.0]);
        let old = Frame([-1800.0, 100.0, 1000.0, 700.0]);
        assert_eq!(old.fit(&[primary.clone(), left]), old);
        assert_eq!(
            old.fit(std::slice::from_ref(&primary)),
            Frame([0.0, 100.0, 1000.0, 700.0])
        );
        assert_eq!(
            Frame([900.0, 600.0, 2000.0, 1500.0]).fit(std::slice::from_ref(&primary)),
            primary
        );
    }

    #[test]
    fn invalid_geometry_remote_or_tab_indices_cannot_restore() {
        let mut state = sample();
        state.groups[0].selected = 2;
        assert!(!state.valid());
        state.groups[0].selected = 0;
        state.groups[0].frame.0[0] = f64::NAN;
        assert!(!state.valid());
        state.groups[0].frame.0[0] = 0.0;
        state.groups[0].tabs[0].remote = Some(Remote {
            host: "-oProxyCommand=bad".into(),
            port: 9999,
            ssh_port: 22,
        });
        assert!(!state.valid());
    }
}
