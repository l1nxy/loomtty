mod actions;
mod autostart;
mod menu;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use tokio::sync::{Mutex, Notify};
use tray_icon::menu::MenuEvent;
use tray_icon::{Icon, TrayIconBuilder};

use crate::daemon::server::Server;

const POLL_INTERVAL: Duration = Duration::from_secs(5);
const TICK_INTERVAL: Duration = Duration::from_millis(100);

/// Snapshot of server state for the tray menu.
#[derive(Debug, Clone)]
pub(crate) struct TraySessionInfo {
    pub name: String,
    pub pane_count: usize,
    pub client_count: usize,
    pub running: bool,
}

#[derive(Debug, Clone)]
struct TraySnapshot {
    sessions: Vec<TraySessionInfo>,
    server_up: bool,
}

fn load_icon() -> Icon {
    let bytes = include_bytes!("../../../../assets/icons/icon-32x32.png");
    let img = image::load_from_memory(bytes)
        .expect("failed to decode icon PNG")
        .into_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).expect("failed to create tray icon")
}

/// Query session info directly from the server state (no IPC needed).
/// Returns `None` if the mutex is contended (skip this poll cycle).
fn snapshot_sessions(state: &Arc<Mutex<Server>>) -> Option<TraySnapshot> {
    let server = state.try_lock().ok()?;
    let sessions = server
        .sessions
        .iter()
        .filter(|(name, _)| !name.starts_with("__"))
        .map(|(name, sess)| {
            let client_count = server
                .clients
                .values()
                .filter(|c| c.session_name == *name)
                .count();
            TraySessionInfo {
                name: name.clone(),
                pane_count: sess.panes.len(),
                client_count,
                running: true,
            }
        })
        .collect();
    Some(TraySnapshot {
        sessions,
        server_up: true,
    })
}

/// Spawn a background thread that periodically snapshots server state
/// and sends results to the tray event loop.
fn spawn_poller(state: Arc<Mutex<Server>>, interval: Duration) -> mpsc::Receiver<TraySnapshot> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("tray-poller".into())
        .spawn(move || {
            // Initial delay — the tray already does a snapshot at startup.
            std::thread::sleep(interval);
            loop {
                if let Some(snap) = snapshot_sessions(&state)
                    && tx.send(snap).is_err()
                {
                    break;
                }
                std::thread::sleep(interval);
            }
        })
        .expect("failed to spawn tray poller thread");
    rx
}

/// Run the tray event loop on the main thread.
/// Returns when Quit is selected or the server shuts down externally (e.g. SIGTERM).
pub fn run_tray(state: Arc<Mutex<Server>>, shutdown: Arc<Notify>, server_exited: Arc<AtomicBool>) {
    #[cfg(target_os = "linux")]
    gtk::init().expect("failed to init GTK (required for tray icon on Linux)");

    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
        use objc2_foundation::MainThreadMarker;
        let mtm = MainThreadMarker::new().expect("tray must run on the main thread");
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }

    let icon = load_icon();

    // Initial snapshot (may fail on contention, use empty fallback)
    let initial = snapshot_sessions(&state).unwrap_or(TraySnapshot {
        sessions: Vec::new(),
        server_up: true,
    });
    let initial_hash = menu::sessions_hash(&initial);

    let (tray_menu, mut handles) = menu::build_menu(&initial);

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("Ciri")
        .with_icon(icon)
        .build()
        .expect("failed to build tray icon");

    let mut last_hash = initial_hash;

    let rx = spawn_poller(state.clone(), POLL_INTERVAL);

    log::info!("tray running");

    loop {
        // Drain poller results (only update menu on actual successful snapshots)
        while let Ok(snap) = rx.try_recv() {
            let hash = menu::sessions_hash(&snap);
            if hash != last_hash {
                last_hash = hash;
                let (new_menu, new_handles) = menu::build_menu(&snap);
                _tray.set_menu(Some(Box::new(new_menu)));
                handles = new_handles;
            }
        }

        // Drain menu events
        let mut should_exit = false;
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some(action) = menu::handle_menu_event(&event, &handles)
                && actions::execute(action, &handles, &state, &shutdown)
            {
                should_exit = true;
            }
        }

        // Exit if user clicked Quit or server shut down externally (SIGTERM, idle timeout).
        if should_exit || server_exited.load(Ordering::Relaxed) {
            return;
        }

        pump_events();
    }
}

#[cfg(target_os = "linux")]
fn pump_events() {
    while gtk::events_pending() {
        gtk::main_iteration();
    }
    std::thread::sleep(TICK_INTERVAL);
}

#[cfg(target_os = "macos")]
fn pump_events() {
    use objc2_app_kit::{NSApplication, NSEventMask};
    use objc2_foundation::{MainThreadMarker, NSDate, NSDefaultRunLoopMode};
    let mtm = MainThreadMarker::new().unwrap();
    let app = NSApplication::sharedApplication(mtm);
    let until = NSDate::dateWithTimeIntervalSinceNow(TICK_INTERVAL.as_secs_f64());
    loop {
        let event = unsafe {
            app.nextEventMatchingMask_untilDate_inMode_dequeue(
                NSEventMask::Any,
                Some(&until),
                NSDefaultRunLoopMode,
                true,
            )
        };
        match event {
            Some(event) => app.sendEvent(&event),
            None => break,
        }
    }
}

#[cfg(windows)]
fn pump_events() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
    };
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    std::thread::sleep(TICK_INTERVAL);
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn pump_events() {
    std::thread::sleep(TICK_INTERVAL);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::client::ClientState;
    use crate::daemon::server::Server;
    use ciri_term::pane::TerminalColors;
    use std::collections::HashMap;
    use tokio::sync::mpsc;

    fn test_server() -> Server {
        Server::new("", 8.0, TerminalColors::default())
    }

    fn test_client(id: u64, session_name: &str) -> ClientState {
        let (tx, _rx) = mpsc::channel(1);
        ClientState {
            id,
            tx,
            damage: HashMap::new(),
            last_acked_generation: 0,
            max_input_seq: HashMap::new(),
            history_sent: HashMap::new(),
            send_failures: 0,
            cell_width: 8.0,
            cell_height: 16.0,
            viewport_width: 1024.0,
            viewport_height: 768.0,
            session_name: session_name.to_string(),
        }
    }

    #[test]
    fn snapshot_empty_server() {
        let server = test_server();
        let state = Arc::new(Mutex::new(server));
        let snap = snapshot_sessions(&state).expect("should succeed");
        assert!(snap.sessions.is_empty());
        assert!(snap.server_up);
    }

    #[test]
    fn snapshot_filters_control_sessions() {
        let mut server = test_server();
        server.get_or_create_session("main");
        server.get_or_create_session("__control__");
        let state = Arc::new(Mutex::new(server));
        let snap = snapshot_sessions(&state).unwrap();
        assert_eq!(snap.sessions.len(), 1);
        assert_eq!(snap.sessions[0].name, "main");
    }

    #[test]
    fn snapshot_counts_clients_per_session() {
        let mut server = test_server();
        server.get_or_create_session("alpha");
        server.get_or_create_session("beta");
        server.clients.insert(1, test_client(1, "alpha"));
        server.clients.insert(2, test_client(2, "alpha"));
        server.clients.insert(3, test_client(3, "beta"));
        let state = Arc::new(Mutex::new(server));
        let snap = snapshot_sessions(&state).unwrap();
        let alpha = snap.sessions.iter().find(|s| s.name == "alpha").unwrap();
        let beta = snap.sessions.iter().find(|s| s.name == "beta").unwrap();
        assert_eq!(alpha.client_count, 2);
        assert_eq!(beta.client_count, 1);
    }

    #[test]
    fn snapshot_returns_none_on_contention() {
        let server = test_server();
        let state = Arc::new(Mutex::new(server));
        // Hold the lock so try_lock fails
        let _guard = state.try_lock().unwrap();
        let result = snapshot_sessions(&state);
        assert!(result.is_none());
    }

    #[test]
    fn snapshot_pane_count_matches_session() {
        let mut server = test_server();
        server.get_or_create_session("dev");
        // get_or_create_session creates one pane by default
        let state = Arc::new(Mutex::new(server));
        let snap = snapshot_sessions(&state).unwrap();
        let dev = snap.sessions.iter().find(|s| s.name == "dev").unwrap();
        assert_eq!(dev.pane_count, 1); // get_or_create_session creates exactly one pane
        assert!(dev.running);
    }

    #[test]
    fn snapshot_filters_all_dunder_prefix_sessions() {
        let mut server = test_server();
        server.get_or_create_session("visible");
        server.get_or_create_session("__control__");
        server.get_or_create_session("__admin__");
        let state = Arc::new(Mutex::new(server));
        let snap = snapshot_sessions(&state).unwrap();
        assert_eq!(snap.sessions.len(), 1);
        assert_eq!(snap.sessions[0].name, "visible");
    }

    #[test]
    fn snapshot_all_sessions_marked_running() {
        let mut server = test_server();
        server.get_or_create_session("a");
        server.get_or_create_session("b");
        let state = Arc::new(Mutex::new(server));
        let snap = snapshot_sessions(&state).unwrap();
        assert!(snap.sessions.iter().all(|s| s.running));
    }
}
