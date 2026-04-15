//! Persistence for the "recent remote hosts" list.
//!
//! Stored as JSON next to the session state files (`state_dir()/recent_hosts.json`)
//! so it survives restarts. Kept separate from the user-edited config.toml so
//! ciri can rewrite it freely.

use std::path::PathBuf;

use ciri_app::app::RecentHost;
use ciri_protocol::transport;

fn recent_hosts_path() -> PathBuf {
    transport::state_dir().join("recent_hosts.json")
}

/// Load recent hosts from disk. Returns an empty list on any error
/// (missing file, corrupt JSON) — recents are best-effort, not critical state.
pub fn load() -> Vec<RecentHost> {
    let path = recent_hosts_path();
    let Ok(bytes) = std::fs::read(&path) else {
        return Vec::new();
    };
    match serde_json::from_slice::<Vec<RecentHost>>(&bytes) {
        Ok(list) => list,
        Err(e) => {
            log::warn!("recent_hosts: failed to parse {}: {e}", path.display());
            Vec::new()
        }
    }
}

/// Persist the recent-hosts list. Errors are logged but not propagated —
/// failing to save shouldn't break the connection flow.
pub fn save(hosts: &[RecentHost]) {
    let path = recent_hosts_path();
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        log::warn!("recent_hosts: cannot create dir {}: {e}", parent.display());
        return;
    }
    let bytes = match serde_json::to_vec_pretty(hosts) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("recent_hosts: serialize failed: {e}");
            return;
        }
    };
    if let Err(e) = std::fs::write(&path, bytes) {
        log::warn!("recent_hosts: write {} failed: {e}", path.display());
    }
}
