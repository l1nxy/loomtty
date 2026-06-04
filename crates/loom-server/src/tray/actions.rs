use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};

use super::autostart;
use super::menu::{MenuHandles, TrayAction};
use crate::daemon::server::Server;

/// Execute a tray action. Returns `true` if the server should shut down.
pub fn execute(
    action: TrayAction,
    handles: &MenuHandles,
    state: &Arc<Mutex<Server>>,
    shutdown: &Arc<Notify>,
) -> bool {
    match action {
        TrayAction::AttachSession(name) => {
            attach_or_switch(state, &name);
            false
        }
        TrayAction::NewSession => {
            new_session(state);
            false
        }
        TrayAction::Quit => {
            log::info!("tray: quit requested, shutting down server");
            shutdown.notify_one();
            true
        }
        TrayAction::ToggleAutostart => {
            let new_state = !autostart::is_enabled();
            match autostart::set_enabled(new_state) {
                Ok(()) => handles.autostart_check.set_checked(new_state),
                Err(e) => log::error!("failed to toggle autostart: {e}"),
            }
            false
        }
    }
}

/// Click on a session: switch existing client or open new window.
fn attach_or_switch(state: &Arc<Mutex<Server>>, session_name: &str) {
    // blocking_lock is designed for calling from a std thread — it parks
    // correctly without requiring a tokio runtime context.
    let mut server = state.blocking_lock();

    if let Some(client_id) = server.most_recent_client() {
        log::info!("switching client {client_id} to session '{session_name}'");
        server.tray_switch_client_to_session(client_id, session_name);
    } else {
        drop(server);
        spawn_client(&[session_name]);
    }
}

/// New Session: create in existing client or open new window.
fn new_session(state: &Arc<Mutex<Server>>) {
    let mut server = state.blocking_lock();
    let name = server.generate_session_name();

    if let Some(client_id) = server.most_recent_client() {
        log::info!("creating session '{name}' in client {client_id}");
        server.tray_switch_client_to_session(client_id, &name);
    } else {
        drop(server);
        spawn_client(&["new"]);
    }
}

fn client_binary() -> PathBuf {
    if let Ok(self_exe) = std::env::current_exe()
        && let Some(dir) = self_exe.parent()
    {
        let sibling = dir.join(if cfg!(windows) {
            "loomtty.exe"
        } else {
            "loomtty"
        });
        if sibling.exists() {
            return sibling;
        }
    }
    PathBuf::from("loomtty")
}

fn spawn_client(args: &[&str]) {
    use std::process::Stdio;
    let exe = client_binary();
    log::info!("spawning: {} {}", exe.display(), args.join(" "));
    let mut cmd = std::process::Command::new(&exe);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    if let Err(e) = cmd.spawn() {
        log::error!("failed to spawn loomtty: {e}");
    }
}
