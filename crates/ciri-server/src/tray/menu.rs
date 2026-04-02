use tray_icon::menu::{
    CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu,
};

use super::TraySnapshot;

pub struct MenuHandles {
    pub new_session_id: MenuId,
    pub quit_id: MenuId,
    pub autostart_id: MenuId,
    pub autostart_check: CheckMenuItem,
    pub session_ids: Vec<(MenuId, String)>,
}

#[derive(Debug)]
pub enum TrayAction {
    AttachSession(String),
    NewSession,
    ToggleAutostart,
    Quit,
}

pub fn build_menu(state: &TraySnapshot) -> (Menu, MenuHandles) {
    let menu = Menu::new();
    let mut session_ids = Vec::new();

    if state.server_up {
        let running: Vec<_> = state.sessions.iter().filter(|s| s.running).collect();
        if running.is_empty() {
            let _ = menu.append(&MenuItem::with_id("_no_sessions", "No sessions", false, None));
        } else {
            for s in &running {
                let id = format!("session:{}", s.name);
                let label = if s.client_count > 0 {
                    // Attached — user clicking will switch, not open new window
                    format!("{} ({} pane{}) - attached", s.name, s.pane_count,
                        if s.pane_count == 1 { "" } else { "s" })
                } else {
                    // No clients — clicking will open a new window
                    format!("{} ({} pane{})", s.name, s.pane_count,
                        if s.pane_count == 1 { "" } else { "s" })
                };
                let item = MenuItem::with_id(&id, &label, true, None);
                let _ = menu.append(&item);
                session_ids.push((item.id().clone(), s.name.clone()));
            }
        }

        let saved: Vec<_> = state.sessions.iter().filter(|s| !s.running).collect();
        if !saved.is_empty() {
            let saved_sub = Submenu::new("Saved Sessions", true);
            for s in &saved {
                let id = format!("session:{}", s.name);
                let item = MenuItem::with_id(&id, &s.name, true, None);
                let _ = saved_sub.append(&item);
                session_ids.push((item.id().clone(), s.name.clone()));
            }
            let _ = menu.append(&saved_sub);
        }
    } else {
        let _ = menu.append(&MenuItem::with_id(
            "_server_down",
            "Server not running",
            false,
            None,
        ));
    }

    let _ = menu.append(&PredefinedMenuItem::separator());

    let new_session = MenuItem::with_id("new_session", "New Session", true, None);
    let _ = menu.append(&new_session);

    let _ = menu.append(&PredefinedMenuItem::separator());

    let autostart_enabled = super::autostart::is_enabled();
    let autostart_check =
        CheckMenuItem::with_id("autostart", "Start on Login", true, autostart_enabled, None);
    let _ = menu.append(&autostart_check);

    let _ = menu.append(&PredefinedMenuItem::separator());

    let quit = MenuItem::with_id("quit", "Quit", true, None);
    let _ = menu.append(&quit);

    let handles = MenuHandles {
        new_session_id: new_session.id().clone(),
        quit_id: quit.id().clone(),
        autostart_id: autostart_check.id().clone(),
        autostart_check,
        session_ids,
    };

    (menu, handles)
}

pub fn handle_menu_event(event: &MenuEvent, handles: &MenuHandles) -> Option<TrayAction> {
    let id = event.id();

    if *id == handles.quit_id {
        return Some(TrayAction::Quit);
    }
    if *id == handles.new_session_id {
        return Some(TrayAction::NewSession);
    }
    if *id == handles.autostart_id {
        return Some(TrayAction::ToggleAutostart);
    }

    for (session_id, name) in &handles.session_ids {
        if *id == *session_id {
            return Some(TrayAction::AttachSession(name.clone()));
        }
    }

    None
}

/// Order-independent hash to detect session list changes (FNV-style).
pub fn sessions_hash(state: &TraySnapshot) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    h = h.wrapping_mul(0x100000001b3) ^ state.server_up as u64;

    let mut names: Vec<&str> = state.sessions.iter().map(|s| s.name.as_str()).collect();
    names.sort_unstable();

    for name in &names {
        for b in name.bytes() {
            h = h.wrapping_mul(0x100000001b3) ^ b as u64;
        }
        if let Some(s) = state.sessions.iter().find(|s| s.name == *name) {
            h = h.wrapping_mul(0x100000001b3) ^ s.pane_count as u64;
            h = h.wrapping_mul(0x100000001b3) ^ s.client_count as u64;
            h = h.wrapping_mul(0x100000001b3) ^ s.running as u64;
        }
        h = h.wrapping_mul(0x100000001b3) ^ 0xff;
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tray::TraySessionInfo;

    fn session(name: &str, panes: usize, clients: usize, running: bool) -> TraySessionInfo {
        TraySessionInfo {
            name: name.to_string(),
            pane_count: panes,
            client_count: clients,
            running,
        }
    }

    fn snap(sessions: Vec<TraySessionInfo>, server_up: bool) -> TraySnapshot {
        TraySnapshot {
            sessions,
            server_up,
        }
    }

    // ─── sessions_hash tests ────────────────────────────────────────

    #[test]
    fn hash_empty_sessions() {
        let a = snap(vec![], true);
        let b = snap(vec![], true);
        assert_eq!(sessions_hash(&a), sessions_hash(&b));
    }

    #[test]
    fn hash_differs_server_up_vs_down() {
        let a = snap(vec![], true);
        let b = snap(vec![], false);
        assert_ne!(sessions_hash(&a), sessions_hash(&b));
    }

    #[test]
    fn hash_differs_for_different_names() {
        let a = snap(vec![session("alpha", 1, 0, true)], true);
        let b = snap(vec![session("beta", 1, 0, true)], true);
        assert_ne!(sessions_hash(&a), sessions_hash(&b));
    }

    #[test]
    fn hash_identical_for_same_sessions() {
        let a = snap(vec![session("main", 2, 1, true)], true);
        let b = snap(vec![session("main", 2, 1, true)], true);
        assert_eq!(sessions_hash(&a), sessions_hash(&b));
    }

    #[test]
    fn hash_order_independent() {
        let a = snap(
            vec![session("alpha", 1, 0, true), session("beta", 2, 1, true)],
            true,
        );
        let b = snap(
            vec![session("beta", 2, 1, true), session("alpha", 1, 0, true)],
            true,
        );
        assert_eq!(sessions_hash(&a), sessions_hash(&b));
    }

    #[test]
    fn hash_differs_on_pane_count_change() {
        let a = snap(vec![session("main", 1, 0, true)], true);
        let b = snap(vec![session("main", 2, 0, true)], true);
        assert_ne!(sessions_hash(&a), sessions_hash(&b));
    }

    #[test]
    fn hash_differs_on_client_count_change() {
        let a = snap(vec![session("main", 1, 0, true)], true);
        let b = snap(vec![session("main", 1, 1, true)], true);
        assert_ne!(sessions_hash(&a), sessions_hash(&b));
    }

    #[test]
    fn hash_differs_on_running_change() {
        let a = snap(vec![session("main", 1, 0, true)], true);
        let b = snap(vec![session("main", 1, 0, false)], true);
        assert_ne!(sessions_hash(&a), sessions_hash(&b));
    }

    #[test]
    fn hash_differs_on_session_added() {
        let a = snap(vec![session("main", 1, 0, true)], true);
        let b = snap(
            vec![session("main", 1, 0, true), session("dev", 1, 0, true)],
            true,
        );
        assert_ne!(sessions_hash(&a), sessions_hash(&b));
    }

    #[test]
    fn hash_differs_on_session_removed() {
        let a = snap(
            vec![session("main", 1, 0, true), session("dev", 1, 0, true)],
            true,
        );
        let b = snap(vec![session("main", 1, 0, true)], true);
        assert_ne!(sessions_hash(&a), sessions_hash(&b));
    }

    // ─── build_menu tests ───────────────────────────────────────────

    #[test]
    fn build_menu_with_running_sessions_creates_session_entries() {
        let state = snap(
            vec![
                session("alpha", 2, 1, true),
                session("beta", 1, 0, true),
            ],
            true,
        );
        let (_, handles) = build_menu(&state);
        assert_eq!(handles.session_ids.len(), 2);
        let names: Vec<&str> = handles.session_ids.iter().map(|(_, n)| n.as_str()).collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
    }

    #[test]
    fn build_menu_no_sessions_has_empty_session_ids() {
        let state = snap(vec![], true);
        let (_, handles) = build_menu(&state);
        assert!(handles.session_ids.is_empty());
    }

    #[test]
    fn build_menu_server_down_has_empty_session_ids() {
        let state = snap(vec![], false);
        let (_, handles) = build_menu(&state);
        assert!(handles.session_ids.is_empty());
    }

    #[test]
    fn build_menu_saved_sessions_in_submenu() {
        let state = snap(
            vec![
                session("running", 1, 0, true),
                session("saved", 0, 0, false),
            ],
            true,
        );
        let (_, handles) = build_menu(&state);
        // Both running and saved should appear in session_ids
        assert_eq!(handles.session_ids.len(), 2);
    }

    // ─── handle_menu_event tests ────────────────────────────────────

    #[test]
    fn handle_quit_event() {
        let state = snap(vec![session("main", 1, 0, true)], true);
        let (_, handles) = build_menu(&state);
        let event = MenuEvent { id: handles.quit_id.clone() };
        let action = handle_menu_event(&event, &handles);
        assert!(matches!(action, Some(TrayAction::Quit)));
    }

    #[test]
    fn handle_new_session_event() {
        let state = snap(vec![], true);
        let (_, handles) = build_menu(&state);
        let event = MenuEvent { id: handles.new_session_id.clone() };
        let action = handle_menu_event(&event, &handles);
        assert!(matches!(action, Some(TrayAction::NewSession)));
    }

    #[test]
    fn handle_autostart_event() {
        let state = snap(vec![], true);
        let (_, handles) = build_menu(&state);
        let event = MenuEvent { id: handles.autostart_id.clone() };
        let action = handle_menu_event(&event, &handles);
        assert!(matches!(action, Some(TrayAction::ToggleAutostart)));
    }

    #[test]
    fn handle_session_click_event() {
        let state = snap(vec![session("myapp", 3, 1, true)], true);
        let (_, handles) = build_menu(&state);
        let (session_menu_id, _) = &handles.session_ids[0];
        let event = MenuEvent { id: session_menu_id.clone() };
        let action = handle_menu_event(&event, &handles);
        match action {
            Some(TrayAction::AttachSession(name)) => assert_eq!(name, "myapp"),
            other => panic!("expected AttachSession, got {other:?}"),
        }
    }

    #[test]
    fn hash_no_prefix_collision() {
        let a = snap(
            vec![session("a", 1, 0, true), session("bc", 1, 0, true)],
            true,
        );
        let b = snap(
            vec![session("ab", 1, 0, true), session("c", 1, 0, true)],
            true,
        );
        assert_ne!(sessions_hash(&a), sessions_hash(&b));
    }

    #[test]
    fn build_menu_server_down_ignores_sessions() {
        let state = snap(vec![session("alpha", 2, 1, true)], false);
        let (_, handles) = build_menu(&state);
        assert!(handles.session_ids.is_empty());
    }

    #[test]
    fn handle_unknown_event_returns_none() {
        let state = snap(vec![], true);
        let (_, handles) = build_menu(&state);
        let event = MenuEvent { id: MenuId::new("unknown_id_12345") };
        assert!(handle_menu_event(&event, &handles).is_none());
    }
}
