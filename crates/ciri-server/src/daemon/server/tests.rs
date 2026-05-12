use super::*;
use ciri_term::pane::Pane;
use rstest::*;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tokio::sync::mpsc;

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

fn test_shell() -> &'static str {
    #[cfg(windows)]
    {
        "powershell.exe"
    }
    #[cfg(not(windows))]
    {
        if std::path::Path::new("/bin/sh").exists() {
            "/bin/sh"
        } else {
            "sh"
        }
    }
}

fn wait_until(
    pane: &mut Pane,
    timeout: Duration,
    mut predicate: impl FnMut(&Pane) -> bool,
) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        let _ = pane.process_pty_output();
        if predicate(pane) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn resize_ignores_invalid_dimensions() {
    let mut server = Server::new("", 8.0, TerminalColors::default());
    let session_name = "alpha".to_string();
    server.clients.insert(1, test_client(1, &session_name));
    server.get_or_create_session(&session_name);
    let before = server.clients.get(&1).unwrap().viewport_width;

    let responses = server.handle_message(
        ClientMessage::Resize {
            cols: 0,
            rows: 0,
            width: 0,
            height: 768,
            cell_width: 8.0,
            cell_height: 16.0,
        },
        1,
    );

    assert!(responses.is_empty());
    let client = server.clients.get(&1).unwrap();
    assert_eq!(client.viewport_width, before);
    assert_eq!(client.viewport_height, 768.0);
}

#[test]
fn switch_session_resets_client_runtime_state_and_refreshes_attach_time() {
    let mut server = Server::new("", 8.0, TerminalColors::default());
    let old_session = "alpha".to_string();
    let new_session = "beta".to_string();
    server.clients.insert(1, test_client(1, &old_session));
    server.get_or_create_session(&old_session);
    server.get_or_create_session(&new_session);

    let stale_gen = 77;
    let stale_pane = {
        let session = server.sessions.get(&old_session).unwrap();
        session.workspaces.active().active_pane_id().unwrap()
    };
    {
        let client = server.clients.get_mut(&1).unwrap();
        let mut acc = DamageAccumulator::default();
        acc.mark_full();
        client.damage.insert(stale_pane, acc);
        client.history_sent.insert(stale_pane, 99);
        client.last_acked_generation = stale_gen;
        client.send_failures = 5;
    }
    let previous_attach = server.sessions.get(&new_session).unwrap().last_attached;
    std::thread::sleep(std::time::Duration::from_millis(1));

    let responses = server.handle_message(
        ClientMessage::SwitchSession {
            session_name: new_session.clone(),
        },
        1,
    );

    let client = server.clients.get(&1).unwrap();
    assert_eq!(client.session_name, new_session);
    assert_eq!(client.last_acked_generation, 0);
    assert_eq!(client.send_failures, 0);
    assert!(!client.damage.contains_key(&stale_pane));
    assert!(!client.history_sent.contains_key(&stale_pane));
    assert!(!client.damage.is_empty());
    assert!(!client.history_sent.is_empty());
    assert!(
        client.damage.values().all(DamageAccumulator::is_empty),
        "initial full sync should clear pending damage after queuing authoritative frames"
    );

    let refreshed_attach = server.sessions.get(&new_session).unwrap().last_attached;
    assert!(
        refreshed_attach > previous_attach,
        "switching into a session should refresh last_attached ordering"
    );

    assert!(matches!(
        responses.first(),
        Some(ServerResponse::SendToClient(
            1,
            ServerMessage::SessionSwitched { session_name }
        )) if session_name == &new_session
    ));
    assert!(responses.iter().any(|resp| matches!(
        resp,
        ServerResponse::SendToClient(1, ServerMessage::StateSync { .. })
    )));
    assert!(
        responses
            .iter()
            .any(|resp| matches!(resp, ServerResponse::SendFullPaneSync(1, _)))
    );
}

#[test]
fn switch_session_emits_image_deleted_for_attach_sync_after_prior_delete() {
    let mut server = Server::new("", 8.0, TerminalColors::default());
    let old_session = "alpha".to_string();
    let new_session = "beta".to_string();
    server.clients.insert(1, test_client(1, &old_session));
    server.get_or_create_session(&old_session);
    server.get_or_create_session(&new_session);

    let pane_id = {
        let session = server.sessions.get_mut(&new_session).unwrap();
        let pane_id = session.workspaces.active().active_pane_id().unwrap();
        session
            .panes
            .get_mut(&pane_id)
            .expect("pane exists")
            .test_mark_image_deleted();
        pane_id
    };

    let responses = server.handle_message(
        ClientMessage::SwitchSession {
            session_name: new_session.clone(),
        },
        1,
    );

    assert!(responses.iter().any(|resp| matches!(
        resp,
        ServerResponse::SendToClient(1, ServerMessage::ImageDeleted { pane_id: id }) if *id == pane_id
    )));
}

#[test]
fn switch_session_to_new_session_refreshes_attach_ordering_for_list_sessions() {
    let mut server = Server::new("", 8.0, TerminalColors::default());
    let old_session = "alpha".to_string();
    let newer_existing_session = "beta".to_string();
    let brand_new_session = "gamma".to_string();
    server.clients.insert(1, test_client(1, &old_session));
    server.get_or_create_session(&old_session);
    std::thread::sleep(std::time::Duration::from_millis(1));
    server.get_or_create_session(&newer_existing_session);

    let switch_responses = server.handle_message(
        ClientMessage::SwitchSession {
            session_name: brand_new_session.clone(),
        },
        1,
    );

    assert!(matches!(
        switch_responses.first(),
        Some(ServerResponse::SendToClient(
            1,
            ServerMessage::SessionSwitched { session_name }
        )) if session_name == &brand_new_session
    ));

    let list_responses = server.handle_message(ClientMessage::ListSessions { all: false }, 1);
    assert_eq!(list_responses.len(), 1);
    let sessions = match &list_responses[0] {
        ServerResponse::SendToClient(1, ServerMessage::SessionList { sessions }) => sessions,
        _ => panic!("expected a single SessionList response"),
    };
    let names: Vec<_> = sessions.iter().map(|info| info.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            brand_new_session.as_str(),
            newer_existing_session.as_str(),
            old_session.as_str(),
        ],
        "switching into a newly created session should make it the most recent ListSessions entry"
    );
}

#[test]
#[cfg_attr(
    windows,
    ignore = "ConPTY + PowerShell prompt rendering eats the second printf in a small pane; see 9e5d317"
)]
fn alt_screen_full_sync_keeps_primary_scrollback_watermark_unsent() {
    let mut pane = Pane::new_with_opts(1, 80, 5, test_shell(), None, None).expect("create pane");

    pane.write_to_pty(b"printf 'A\\nB\\nC\\nD\\nE\\nF\\nG\\nH\\nI\\nJ\\n'\n");
    let grew = wait_until(&mut pane, Duration::from_secs(3), |p| {
        p.scrollback_total() > 0
    });
    assert!(
        grew,
        "pane should accumulate primary scrollback before alt-screen"
    );
    let primary_total = pane.scrollback_total();

    pane.write_to_pty(b"printf '\\033[?1049h'\n");
    let entered_alt = wait_until(&mut pane, Duration::from_secs(3), |p| p.is_alt_screen());
    assert!(entered_alt, "pane should enter alt-screen");

    assert_eq!(Server::history_sent_after_full_sync(&pane), 0);
    assert_eq!(Server::visible_scrollback_total(&pane, 17), 17);
    assert!(
        primary_total > 0,
        "test setup should preserve hidden primary scrollback while in alt-screen"
    );
}

#[test]
fn close_pane_emits_close_then_layout_update() {
    let mut server = Server::new("", 8.0, TerminalColors::default());
    let session_name = "alpha".to_string();
    server.clients.insert(1, test_client(1, &session_name));
    let pane_id = {
        let session = server.get_or_create_session(&session_name);
        session.workspaces.active().active_pane_id().unwrap()
    };

    let responses = server.handle_message(ClientMessage::ClosePane { pane_id }, 1);

    assert!(matches!(
        responses.as_slice(),
        [
            ServerResponse::BroadcastToSession(first_session, ServerMessage::PaneClosed { pane_id: closed }),
            ServerResponse::BroadcastToSession(second_session, ServerMessage::LayoutUpdate { .. })
        ] if first_session == &session_name && second_session == &session_name && *closed == pane_id
    ));
}

#[test]
fn capture_pane_emits_one_newline_per_grid_row() {
    let mut server = Server::new("", 8.0, TerminalColors::default());
    let session_name = "capture-ok".to_string();
    server.clients.insert(1, test_client(1, "__control__"));
    let target_pane = {
        let session = server.get_or_create_session(&session_name);
        session.workspaces.active().active_pane_id().unwrap()
    };

    let responses = server.handle_message(
        ClientMessage::CapturePane {
            session_name: session_name.clone(),
            pane_id: target_pane,
            opts: Default::default(),
        },
        1,
    );

    // Tighter than just shape: also verify the response carries
    // exactly one row per pane row + newline (i.e. capture_text was
    // actually executed end-to-end, not just stubbed).
    let [ServerResponse::SendToClient(
        1,
        ServerMessage::PaneCapture {
            session_name: sn,
            pane_id: pid,
            text,
            truncated,
        },
    )] = responses.as_slice()
    else {
        panic!(
            "expected exactly one PaneCapture response, got {} response(s)",
            responses.len()
        );
    };
    assert_eq!(sn, &session_name);
    assert_eq!(*pid, target_pane);
    assert!(!truncated, "small capture should not be truncated");
    // capture_text emits exactly one '\n' per visible row when
    // join_wrapped is false (the default). With Default::default() opts
    // on a no-scrollback request, the newline count must equal the
    // viewport row count regardless of what the shell has written.
    let pane = server.sessions[&session_name].panes.get(&target_pane).unwrap();
    let expected_rows = pane.grid_rows() as usize;
    let newlines = text.matches('\n').count();
    assert_eq!(
        newlines, expected_rows,
        "capture text should emit exactly one newline per visible row; \
         text={text:?}"
    );
}

#[test]
fn capture_pane_rejects_unknown_session() {
    let mut server = Server::new("", 8.0, TerminalColors::default());
    server.clients.insert(1, test_client(1, "__control__"));

    let responses = server.handle_message(
        ClientMessage::CapturePane {
            session_name: "ghost".to_string(),
            pane_id: 1,
            opts: Default::default(),
        },
        1,
    );

    assert!(matches!(
        responses.as_slice(),
        [ServerResponse::SendToClient(1, ServerMessage::Error { message })]
            if message.contains("ghost")
    ));
}

#[test]
fn capture_pane_rejects_unknown_pane_id_in_known_session() {
    let mut server = Server::new("", 8.0, TerminalColors::default());
    let session_name = "capture-bad-pane".to_string();
    server.clients.insert(1, test_client(1, "__control__"));
    server.get_or_create_session(&session_name);

    let responses = server.handle_message(
        ClientMessage::CapturePane {
            session_name: session_name.clone(),
            pane_id: 99_999,
            opts: Default::default(),
        },
        1,
    );

    assert!(matches!(
        responses.as_slice(),
        [ServerResponse::SendToClient(1, ServerMessage::Error { message })]
            if message.contains("99999") && message.contains("capture-bad-pane")
    ));
}

#[test]
fn list_prompts_returns_empty_marks_for_fresh_pane() {
    // A brand-new pane has not yet observed any OSC 133. The reply should
    // be an empty marks vector — that's how callers detect "shell
    // integration not active" without needing a separate query.
    let mut server = Server::new("", 8.0, TerminalColors::default());
    let session_name = "prompts-empty".to_string();
    server.clients.insert(1, test_client(1, "__control__"));
    let target_pane = {
        let session = server.get_or_create_session(&session_name);
        session.workspaces.active().active_pane_id().unwrap()
    };

    let responses = server.handle_message(
        ClientMessage::ListPrompts {
            session_name: session_name.clone(),
            pane_id: target_pane,
        },
        1,
    );

    let [ServerResponse::SendToClient(
        1,
        ServerMessage::PromptListReply {
            session_name: sn,
            pane_id: pid,
            marks,
        },
    )] = responses.as_slice()
    else {
        panic!(
            "expected exactly one PromptListReply, got {} response(s)",
            responses.len()
        );
    };
    assert_eq!(sn, &session_name);
    assert_eq!(*pid, target_pane);
    assert!(
        marks.is_empty(),
        "fresh pane should have no prompt marks; got {marks:?}"
    );
}

#[test]
fn list_prompts_rejects_unknown_session() {
    let mut server = Server::new("", 8.0, TerminalColors::default());
    server.clients.insert(1, test_client(1, "__control__"));

    let responses = server.handle_message(
        ClientMessage::ListPrompts {
            session_name: "ghost".to_string(),
            pane_id: 1,
        },
        1,
    );

    assert!(matches!(
        responses.as_slice(),
        [ServerResponse::SendToClient(1, ServerMessage::Error { message })]
            if message.contains("ghost")
    ));
}

#[test]
fn list_prompts_rejects_unknown_pane_id_in_known_session() {
    let mut server = Server::new("", 8.0, TerminalColors::default());
    let session_name = "prompts-bad-pane".to_string();
    server.clients.insert(1, test_client(1, "__control__"));
    server.get_or_create_session(&session_name);

    let responses = server.handle_message(
        ClientMessage::ListPrompts {
            session_name: session_name.clone(),
            pane_id: 88_888,
        },
        1,
    );

    assert!(matches!(
        responses.as_slice(),
        [ServerResponse::SendToClient(1, ServerMessage::Error { message })]
            if message.contains("88888") && message.contains("prompts-bad-pane")
    ));
}

#[test]
fn focus_pane_by_id_returns_layout_update_and_command_result() {
    let mut server = Server::new("", 8.0, TerminalColors::default());
    let session_name = "alpha".to_string();
    server.clients.insert(1, test_client(1, "__control__"));
    let target_pane = {
        let session = server.get_or_create_session(&session_name);
        session.workspaces.active().active_pane_id().unwrap()
    };

    let responses = server.handle_message(
        ClientMessage::FocusPaneById {
            session_name: session_name.clone(),
            pane_id: target_pane,
        },
        1,
    );

    assert!(matches!(
        responses.as_slice(),
        [
            ServerResponse::BroadcastToSession(target, ServerMessage::LayoutUpdate { .. }),
            ServerResponse::SendToClient(1, ServerMessage::CommandResult { success: true, pane_id: Some(pid), .. })
        ] if target == &session_name && *pid == target_pane
    ));
}

#[test]
fn control_attach_does_not_create_runtime_session() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let (client_reader, server_writer) = tokio::io::duplex(4096);
        let (server_reader, client_writer) = tokio::io::duplex(4096);
        let state = Arc::new(Mutex::new(Server::new("", 8.0, TerminalColors::default())));
        let shutdown = Arc::new(tokio::sync::Notify::new());

        let input_notify = Arc::new(tokio::sync::Notify::new());
        let handle = tokio::spawn(super::super::connection::handle_client(
            server_reader,
            server_writer,
            state.clone(),
            shutdown,
            input_notify,
        ));

        let hello = ciri_protocol::codec::ClientHello {
            session_name: "__control__".to_string(),
            width: 1024,
            height: 768,
            cell_width: 8.0,
            cell_height: 16.0,
        };

        let mut client_reader = tokio::io::BufReader::new(client_reader);
        let mut client_writer = tokio::io::BufWriter::new(client_writer);
        ciri_protocol::codec::write_client_hello(&mut client_writer, &hello)
            .await
            .unwrap();
        client_writer.flush().await.unwrap();
        let compat = ciri_protocol::codec::read_server_hello(&mut client_reader)
            .await
            .unwrap();
        assert!(matches!(
            compat,
            ciri_protocol::codec::VersionCompat::Exact(_)
                | ciri_protocol::codec::VersionCompat::PatchMismatch { .. }
                | ciri_protocol::codec::VersionCompat::MinorMismatch { .. }
        ));

        {
            let server = state.lock().await;
            assert!(
                server
                    .clients
                    .values()
                    .any(|c| c.session_name == "__control__")
            );
            assert!(!server.sessions.contains_key("__control__"));
        }

        handle.abort();
    });
}

// ═══════════════════════════════════════════════════════════════════
// rstest fixtures
// ═══════════════════════════════════════════════════════════════════

#[fixture]
fn server() -> Server {
    Server::new("", 8.0, TerminalColors::default())
}

/// Server with one client attached to session "alpha".
#[fixture]
fn server_with_session(mut server: Server) -> (Server, String) {
    let name = "alpha".to_string();
    server.clients.insert(1, test_client(1, &name));
    server.get_or_create_session(&name);
    (server, name)
}

// ═══════════════════════════════════════════════════════════════════
// Session lifecycle tests: create → use → close → restore
// ═══════════════════════════════════════════════════════════════════

#[rstest]
fn session_create_populates_initial_pane(server_with_session: (Server, String)) {
    let (server, session_name) = server_with_session;
    let session = server.sessions.get(&session_name).unwrap();
    assert_eq!(
        session.panes.len(),
        1,
        "new session should have exactly one pane"
    );
    let pane_id = session.workspaces.active().active_pane_id().unwrap();
    assert!(session.panes.contains_key(&pane_id));
}

#[rstest]
fn session_create_pane_adds_to_layout(mut server_with_session: (Server, String)) {
    let (ref mut server, ref session_name) = server_with_session;
    let responses = server.handle_message(ClientMessage::CreatePane, 1);

    // Should broadcast PaneCreated + LayoutUpdate
    assert!(responses.iter().any(|r| matches!(r,
        ServerResponse::BroadcastToSession(name, ServerMessage::PaneCreated { .. })
        if name == session_name
    )));
    assert!(responses.iter().any(|r| matches!(r,
        ServerResponse::BroadcastToSession(name, ServerMessage::LayoutUpdate { .. })
        if name == session_name
    )));

    let session = server.sessions.get(session_name).unwrap();
    assert_eq!(session.panes.len(), 2, "should now have two panes");
}

#[rstest]
fn session_close_last_pane_leaves_empty_session(mut server_with_session: (Server, String)) {
    let (ref mut server, ref session_name) = server_with_session;
    let pane_id = {
        let session = server.sessions.get(session_name).unwrap();
        session.workspaces.active().active_pane_id().unwrap()
    };

    let responses = server.handle_message(ClientMessage::ClosePane { pane_id }, 1);
    assert!(responses.iter().any(|r| matches!(r,
        ServerResponse::BroadcastToSession(_, ServerMessage::PaneClosed { pane_id: id }) if *id == pane_id
    )));

    let session = server.sessions.get(session_name).unwrap();
    assert!(session.panes.is_empty());
}

#[rstest]
fn session_save_and_restore_roundtrip(mut server_with_session: (Server, String)) {
    let (ref mut server, ref session_name) = server_with_session;
    let tmp = tempfile::tempdir().unwrap();

    // Save session
    {
        let session = server.sessions.get(session_name).unwrap();
        let state = ciri_session::state::SessionState {
            name: session_name.clone(),
            workspaces: vec![ciri_session::state::SavedWorkspace {
                columns: vec![ciri_session::state::SavedColumn {
                    tiles: session
                        .panes
                        .keys()
                        .map(|&pid| ciri_session::state::SavedTile {
                            pane_id: pid,
                            weight: 1.0,
                            cwd: Some("/tmp".to_string()),
                            title: Some("test-shell".to_string()),
                            agent: None,
                        })
                        .collect(),
                    active_tile_idx: 0,
                    width_proportion: 1.0,
                    width_fixed_px: None,
                }],
                active_column_idx: 0,
            }],
            active_workspace_idx: 0,
        };
        ciri_session::save::save_session(&state, tmp.path()).unwrap();
    }

    // Verify file was written
    let saved = ciri_session::restore::restore_session(session_name, tmp.path())
        .unwrap()
        .expect("session should be restoreable");
    assert_eq!(saved.name, *session_name);
    assert_eq!(saved.workspaces.len(), 1);
    assert_eq!(saved.workspaces[0].columns[0].tiles.len(), 1);
    assert_eq!(
        saved.workspaces[0].columns[0].tiles[0].cwd,
        Some("/tmp".to_string())
    );
}

#[rstest]
fn session_list_and_delete(mut server_with_session: (Server, String)) {
    let (ref mut _server, ref session_name) = server_with_session;
    let tmp = tempfile::tempdir().unwrap();

    // Save two sessions
    let state1 = ciri_session::state::SessionState {
        name: session_name.clone(),
        workspaces: vec![ciri_session::state::SavedWorkspace {
            columns: vec![ciri_session::state::SavedColumn {
                tiles: vec![ciri_session::state::SavedTile {
                    pane_id: 1,
                    weight: 1.0,
                    cwd: None,
                    title: None,
                    agent: None,
                }],
                active_tile_idx: 0,
                width_proportion: 1.0,
                width_fixed_px: None,
            }],
            active_column_idx: 0,
        }],
        active_workspace_idx: 0,
    };
    ciri_session::save::save_session(&state1, tmp.path()).unwrap();

    let state2 = ciri_session::state::SessionState {
        name: "beta".to_string(),
        ..state1.clone()
    };
    ciri_session::save::save_session(&state2, tmp.path()).unwrap();

    // List
    let names = ciri_session::restore::list_sessions(tmp.path()).unwrap();
    assert!(names.contains(session_name));
    assert!(names.contains(&"beta".to_string()));

    // Delete one
    ciri_session::restore::delete_session("beta", tmp.path()).unwrap();
    let names = ciri_session::restore::list_sessions(tmp.path()).unwrap();
    assert!(!names.contains(&"beta".to_string()));
    assert!(names.contains(session_name));
}

#[rstest]
fn session_kill_auto_switches_attached_clients(mut server: Server) {
    let session1 = "alpha".to_string();
    let session2 = "beta".to_string();
    server.clients.insert(1, test_client(1, &session1));
    server.clients.insert(2, test_client(2, &session2));
    server.get_or_create_session(&session1);
    server.get_or_create_session(&session2);

    // Client 1 kills session "beta"
    let responses = server.handle_message(
        ClientMessage::KillSession {
            session_name: session2.clone(),
        },
        1,
    );

    // Client 2 (was attached to beta) should be auto-switched to alpha,
    // not disconnected — matches tmux/zellij behavior.
    assert!(
        !responses
            .iter()
            .any(|r| matches!(r, ServerResponse::RemoveClient(_))),
        "no client should be removed; killed-session attached clients are auto-switched"
    );
    assert!(
        responses.iter().any(|r| matches!(r,
            ServerResponse::SendToClient(2, ServerMessage::SessionSwitched { session_name })
            if session_name == &session1
        )),
        "client 2 should be auto-switched to surviving session 'alpha'"
    );
    assert_eq!(
        server.clients.get(&2).unwrap().session_name,
        session1,
        "client 2's session affinity moved to alpha"
    );
    // Client 1 (the requester) gets a SessionKilled notification.
    assert!(responses.iter().any(|r| matches!(r,
        ServerResponse::SendToClient(1, ServerMessage::SessionKilled { session_name })
        if session_name == &session2
    )));
    assert!(!server.sessions.contains_key(&session2));
    assert!(server.sessions.contains_key(&session1));
}

#[rstest]
fn session_switch_creates_new_if_needed(mut server_with_session: (Server, String)) {
    let (ref mut server, _) = server_with_session;
    let target = "brand-new".to_string();
    assert!(!server.sessions.contains_key(&target));

    let responses = server.handle_message(
        ClientMessage::SwitchSession {
            session_name: target.clone(),
        },
        1,
    );

    // New session should be created
    assert!(server.sessions.contains_key(&target));
    assert!(matches!(
        responses.first(),
        Some(ServerResponse::SendToClient(1, ServerMessage::SessionSwitched { session_name }))
        if session_name == &target
    ));
    // Should have a state sync
    assert!(responses.iter().any(|r| matches!(
        r,
        ServerResponse::SendToClient(1, ServerMessage::StateSync { .. })
    )));
}

#[rstest]
fn session_switch_rejects_invalid_name(mut server_with_session: (Server, String)) {
    let (ref mut server, _) = server_with_session;
    let responses = server.handle_message(
        ClientMessage::SwitchSession {
            session_name: "INVALID NAME!".to_string(),
        },
        1,
    );

    assert!(matches!(
        responses.first(),
        Some(ServerResponse::SendToClient(1, ServerMessage::Error { .. }))
    ));
}

// ═══════════════════════════════════════════════════════════════════
// Multi-client scenarios
// ═══════════════════════════════════════════════════════════════════

#[rstest]
fn multi_client_same_session_both_get_layout_updates(mut server: Server) {
    let session_name = "shared".to_string();
    server.clients.insert(1, test_client(1, &session_name));
    server.clients.insert(2, test_client(2, &session_name));
    server.get_or_create_session(&session_name);

    // Client 1 creates a pane — both clients should see the update via broadcast
    let responses = server.handle_message(ClientMessage::CreatePane, 1);

    let layout_updates: Vec<_> = responses
        .iter()
        .filter(|r| {
            matches!(r,
                ServerResponse::BroadcastToSession(name, ServerMessage::LayoutUpdate { .. })
                if name == &session_name
            )
        })
        .collect();
    assert!(
        !layout_updates.is_empty(),
        "layout update should be broadcast to session"
    );
}

#[rstest]
fn multi_client_different_sessions_isolated(mut server: Server) {
    let s1 = "alpha".to_string();
    let s2 = "beta".to_string();
    server.clients.insert(1, test_client(1, &s1));
    server.clients.insert(2, test_client(2, &s2));
    server.get_or_create_session(&s1);
    server.get_or_create_session(&s2);

    // Client 1 creates pane in alpha
    let responses = server.handle_message(ClientMessage::CreatePane, 1);

    // All broadcasts should target "alpha", not "beta"
    for resp in &responses {
        if let ServerResponse::BroadcastToSession(name, _) = resp {
            assert_eq!(name, &s1, "broadcast should target client 1's session only");
        }
    }
    // beta should still have 1 pane
    assert_eq!(server.sessions.get(&s2).unwrap().panes.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════
// Ping/Pong and Ack
// ═══════════════════════════════════════════════════════════════════

#[rstest]
fn ping_pong_roundtrip(mut server_with_session: (Server, String)) {
    let (ref mut server, _) = server_with_session;
    let responses = server.handle_message(
        ClientMessage::Ping {
            seq: 42,
            client_time_us: 1234567890,
        },
        1,
    );
    assert!(matches!(
        responses.first(),
        Some(ServerResponse::SendToClient(
            1,
            ServerMessage::Pong {
                seq: 42,
                client_time_us: 1234567890
            }
        ))
    ));
}

#[rstest]
fn ack_updates_client_generation(mut server_with_session: (Server, String)) {
    let (ref mut server, _) = server_with_session;
    assert_eq!(server.clients.get(&1).unwrap().last_acked_generation, 0);

    server.handle_message(ClientMessage::Ack { generation: 42 }, 1);
    assert_eq!(server.clients.get(&1).unwrap().last_acked_generation, 42);
}

#[rstest]
fn detach_removes_client(mut server_with_session: (Server, String)) {
    let (ref mut server, _) = server_with_session;
    let responses = server.handle_message(ClientMessage::Detach, 1);
    assert!(matches!(
        responses.first(),
        Some(ServerResponse::RemoveClient(1))
    ));
}

#[rstest]
fn kill_server_triggers_shutdown(mut server_with_session: (Server, String)) {
    let (ref mut server, _) = server_with_session;
    let responses = server.handle_message(ClientMessage::KillServer, 1);
    assert!(matches!(
        responses.first(),
        Some(ServerResponse::ShutdownServer)
    ));
}

// ═══════════════════════════════════════════════════════════════════
// Layout operations
// ═══════════════════════════════════════════════════════════════════

#[rstest]
fn focus_at_boundary_emits_bounce_edge(mut server_with_session: (Server, String)) {
    let (ref mut server, _) = server_with_session;
    // Only one column, FocusLeft should bounce
    let responses = server.handle_message(ClientMessage::FocusLeft, 1);
    assert!(responses.iter().any(|r| matches!(
        r,
        ServerResponse::SendToClient(1, ServerMessage::BounceEdge { .. })
    )));
}

#[rstest]
fn create_split_down_adds_workspace(mut server_with_session: (Server, String)) {
    let (ref mut server, ref session_name) = server_with_session;
    let ws_count_before = server
        .sessions
        .get(session_name)
        .unwrap()
        .workspaces
        .workspaces
        .len();

    let responses = server.handle_message(ClientMessage::SplitDown, 1);

    let ws_count_after = server
        .sessions
        .get(session_name)
        .unwrap()
        .workspaces
        .workspaces
        .len();
    assert_eq!(ws_count_after, ws_count_before + 1);
    assert!(responses.iter().any(|r| matches!(
        r,
        ServerResponse::BroadcastToSession(_, ServerMessage::PaneCreated { .. })
    )));
}

#[rstest]
fn switch_workspace_out_of_range_does_not_create_empty_workspaces(
    mut server_with_session: (Server, String),
) {
    let (ref mut server, ref session_name) = server_with_session;
    server.handle_message(ClientMessage::SplitDown, 1);
    let (ws_count_before, active_before) = {
        let session = server.sessions.get(session_name).unwrap();
        (
            session.workspaces.workspaces.len(),
            session.workspaces.active_workspace_idx,
        )
    };
    assert_eq!(ws_count_before, 2);

    let responses = server.handle_message(ClientMessage::SwitchWorkspace { workspace_idx: 8 }, 1);

    let session = server.sessions.get(session_name).unwrap();
    assert_eq!(session.workspaces.workspaces.len(), ws_count_before);
    assert_eq!(session.workspaces.active_workspace_idx, active_before);
    assert!(responses.is_empty());
}

// ═══════════════════════════════════════════════════════════════════
// Duplex stream: full handshake + message exchange
// ═══════════════════════════════════════════════════════════════════

// ─── Duplex test helpers ────────────────────────────────────────

/// Connect a duplex client to a server, perform handshake, return reader/writer/state/handle.
async fn duplex_connect(
    session_name: &str,
) -> (
    tokio::io::BufReader<tokio::io::DuplexStream>,
    tokio::io::BufWriter<tokio::io::DuplexStream>,
    Arc<Mutex<Server>>,
    tokio::task::JoinHandle<()>,
) {
    let (client_reader, server_writer) = tokio::io::duplex(8192);
    let (server_reader, client_writer) = tokio::io::duplex(8192);
    let state = Arc::new(Mutex::new(Server::new("", 8.0, TerminalColors::default())));
    let shutdown = Arc::new(tokio::sync::Notify::new());
    let input_notify = Arc::new(tokio::sync::Notify::new());

    let handle = tokio::spawn(super::super::connection::handle_client(
        server_reader,
        server_writer,
        state.clone(),
        shutdown,
        input_notify,
    ));

    let hello = ciri_protocol::codec::ClientHello {
        session_name: session_name.to_string(),
        width: 1024,
        height: 768,
        cell_width: 8.0,
        cell_height: 16.0,
    };

    let mut cr = tokio::io::BufReader::new(client_reader);
    let mut cw = tokio::io::BufWriter::new(client_writer);

    ciri_protocol::codec::write_client_hello(&mut cw, &hello)
        .await
        .unwrap();
    cw.flush().await.unwrap();
    let _ = ciri_protocol::codec::read_server_hello(&mut cr)
        .await
        .unwrap();
    (cr, cw, state, handle)
}

/// Connect a second client to an existing server state.
async fn duplex_connect_to(
    session_name: &str,
    state: Arc<Mutex<Server>>,
) -> (
    tokio::io::BufReader<tokio::io::DuplexStream>,
    tokio::io::BufWriter<tokio::io::DuplexStream>,
    tokio::task::JoinHandle<()>,
) {
    let (client_reader, server_writer) = tokio::io::duplex(8192);
    let (server_reader, client_writer) = tokio::io::duplex(8192);
    let shutdown = Arc::new(tokio::sync::Notify::new());
    let input_notify = Arc::new(tokio::sync::Notify::new());

    let handle = tokio::spawn(super::super::connection::handle_client(
        server_reader,
        server_writer,
        state,
        shutdown,
        input_notify,
    ));

    let hello = ciri_protocol::codec::ClientHello {
        session_name: session_name.to_string(),
        width: 1024,
        height: 768,
        cell_width: 8.0,
        cell_height: 16.0,
    };

    let mut cr = tokio::io::BufReader::new(client_reader);
    let mut cw = tokio::io::BufWriter::new(client_writer);

    ciri_protocol::codec::write_client_hello(&mut cw, &hello)
        .await
        .unwrap();
    cw.flush().await.unwrap();
    let _ = ciri_protocol::codec::read_server_hello(&mut cr)
        .await
        .unwrap();
    (cr, cw, handle)
}

/// Drain the initial attach-sync frames sent by `handle_client` after handshake.
///
/// # Frame sequence (from `Session::build_state_sync` + `connection.rs`):
///   1. `StateSync` — layout + pane_ids (1 frame, always first)
///   2. `FullPaneSync` × N — one per pane (order may vary)
///   3. `ImageDeleted` × N — one per pane with no active images (order may vary)
///
/// Total: 1 + 2N frames for a session with N panes (all newly created).
/// FullPaneSync and ImageDeleted frames are collected by pane_id (order-independent).
///
/// Returns the pane_ids from the StateSync.
async fn drain_initial_sync(cr: &mut tokio::io::BufReader<tokio::io::DuplexStream>) -> Vec<u64> {
    use std::collections::HashSet;

    let pane_ids = match ciri_protocol::codec::read_frame(cr).await.unwrap() {
        ciri_protocol::codec::Frame::ServerMsg(ServerMessage::StateSync { pane_ids, layout }) => {
            assert!(!pane_ids.is_empty());
            assert!(!layout.workspaces.is_empty());
            pane_ids
        }
        other => panic!("expected StateSync as first attach-sync frame, got {other:?}"),
    };

    let expected: HashSet<u64> = pane_ids.iter().copied().collect();

    // Collect FullPaneSync frames (order-independent)
    let mut synced_panes = HashSet::new();
    for _ in &pane_ids {
        match ciri_protocol::codec::read_frame(cr).await.unwrap() {
            ciri_protocol::codec::Frame::FullPaneSync(sync) => {
                assert!(sync.cols > 0 && sync.rows > 0);
                assert!(
                    expected.contains(&sync.meta.pane_id),
                    "unexpected FullPaneSync for pane {} (expected one of {:?})",
                    sync.meta.pane_id,
                    expected
                );
                synced_panes.insert(sync.meta.pane_id);
            }
            other => panic!("expected FullPaneSync, got {other:?}"),
        }
    }
    assert_eq!(synced_panes, expected, "FullPaneSync pane_ids mismatch");

    // Collect ImageDeleted frames (order-independent)
    let mut deleted_panes = HashSet::new();
    for _ in &pane_ids {
        match ciri_protocol::codec::read_frame(cr).await.unwrap() {
            ciri_protocol::codec::Frame::ServerMsg(ServerMessage::ImageDeleted { pane_id }) => {
                assert!(
                    expected.contains(&pane_id),
                    "unexpected ImageDeleted for pane {} (expected one of {:?})",
                    pane_id,
                    expected
                );
                deleted_panes.insert(pane_id);
            }
            other => panic!("expected ImageDeleted, got {other:?}"),
        }
    }
    assert_eq!(deleted_panes, expected, "ImageDeleted pane_ids mismatch");

    pane_ids
}

// ═══════════════════════════════════════════════════════════════════
// Duplex stream integration tests
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn duplex_handshake_and_list_sessions() {
    let (mut cr, mut cw, _state, handle) = duplex_connect("test-session").await;
    let pane_ids = drain_initial_sync(&mut cr).await;
    assert_eq!(pane_ids.len(), 1, "new session should have one pane");

    // Send ListSessions and verify response
    ciri_protocol::codec::encode_client_msg(&mut cw, &ClientMessage::ListSessions { all: false })
        .await
        .unwrap();
    cw.flush().await.unwrap();

    match ciri_protocol::codec::read_frame(&mut cr).await.unwrap() {
        ciri_protocol::codec::Frame::ServerMsg(ServerMessage::SessionList { sessions }) => {
            assert!(sessions.iter().any(|s| s.name == "test-session"));
            assert_eq!(sessions[0].pane_count, 1);
            assert_eq!(sessions[0].client_count, 1);
        }
        other => panic!("expected SessionList, got {other:?}"),
    }

    handle.abort();
}

#[tokio::test]
async fn duplex_ping_pong() {
    let (mut cr, mut cw, _state, handle) = duplex_connect("ping-test").await;
    drain_initial_sync(&mut cr).await;

    ciri_protocol::codec::encode_client_msg(
        &mut cw,
        &ClientMessage::Ping {
            seq: 99,
            client_time_us: 1_000_000,
        },
    )
    .await
    .unwrap();
    cw.flush().await.unwrap();

    match ciri_protocol::codec::read_frame(&mut cr).await.unwrap() {
        ciri_protocol::codec::Frame::ServerMsg(ServerMessage::Pong {
            seq,
            client_time_us,
        }) => {
            assert_eq!(seq, 99);
            assert_eq!(client_time_us, 1_000_000);
        }
        other => panic!("expected Pong, got {other:?}"),
    }

    handle.abort();
}

#[tokio::test]
async fn duplex_disconnect_cleanup() {
    let (cr, cw, state, handle) = duplex_connect("disconnect-test").await;

    // Confirm client registered
    {
        let s = state.lock().await;
        assert!(!s.clients.is_empty());
    }

    // Drop both ends — handle_client sees EOF and returns
    drop(cw);
    drop(cr);

    // Await directly — no timeout needed since EOF is immediate
    let _ = handle.await;

    let s = state.lock().await;
    assert!(
        s.clients.is_empty(),
        "client should be cleaned up after disconnect"
    );
}

#[tokio::test]
async fn duplex_reconnect_preserves_mutated_state() {
    // Client A connects and creates a second pane
    let (mut cr_a, mut cw_a, state, handle_a) = duplex_connect("reconnect-test").await;
    let initial_panes = drain_initial_sync(&mut cr_a).await;
    assert_eq!(initial_panes.len(), 1);

    // Client A creates a second pane
    ciri_protocol::codec::encode_client_msg(&mut cw_a, &ClientMessage::CreatePane)
        .await
        .unwrap();
    cw_a.flush().await.unwrap();

    // Read PaneCreated to get the new pane ID
    let new_pane_id = match ciri_protocol::codec::read_frame(&mut cr_a).await.unwrap() {
        ciri_protocol::codec::Frame::ServerMsg(ServerMessage::PaneCreated { pane_id, .. }) => {
            pane_id
        }
        other => panic!("expected PaneCreated, got {other:?}"),
    };
    // Drain LayoutUpdate
    let _ = ciri_protocol::codec::read_frame(&mut cr_a).await.unwrap();

    // Verify server now has 2 panes
    {
        let s = state.lock().await;
        assert_eq!(s.sessions.get("reconnect-test").unwrap().panes.len(), 2);
    }

    // Client A disconnects
    drop(cw_a);
    drop(cr_a);
    let _ = handle_a.await;

    // Session should persist with 2 panes
    {
        let s = state.lock().await;
        assert!(s.sessions.contains_key("reconnect-test"));
        assert_eq!(s.sessions.get("reconnect-test").unwrap().panes.len(), 2);
        assert!(s.clients.is_empty());
    }

    // Client B reconnects to the same session
    let (mut cr_b, _cw_b, handle_b) = duplex_connect_to("reconnect-test", state).await;

    // Client B should see BOTH panes — proving state was preserved, not recreated
    let reconnect_panes = drain_initial_sync(&mut cr_b).await;
    assert_eq!(
        reconnect_panes.len(),
        2,
        "reconnected client should see both panes"
    );
    assert!(reconnect_panes.contains(&initial_panes[0]));
    assert!(reconnect_panes.contains(&new_pane_id));

    handle_b.abort();
}

#[tokio::test]
async fn duplex_two_clients_same_session() {
    // Client A connects
    let (mut cr_a, mut cw_a, state, handle_a) = duplex_connect("shared-session").await;
    let pane_ids_a = drain_initial_sync(&mut cr_a).await;
    assert_eq!(pane_ids_a.len(), 1);

    // Client B connects to the same session (existing server state)
    let (mut cr_b, _cw_b, handle_b) = duplex_connect_to("shared-session", state.clone()).await;
    let pane_ids_b = drain_initial_sync(&mut cr_b).await;
    assert_eq!(
        pane_ids_b, pane_ids_a,
        "both clients should see the same pane"
    );

    // Verify server tracks 2 clients
    {
        let s = state.lock().await;
        let session_clients = s
            .clients
            .values()
            .filter(|c| c.session_name == "shared-session")
            .count();
        assert_eq!(session_clients, 2);
    }

    // Client A sends CreatePane — both should receive PaneCreated + LayoutUpdate
    ciri_protocol::codec::encode_client_msg(&mut cw_a, &ClientMessage::CreatePane)
        .await
        .unwrap();
    cw_a.flush().await.unwrap();

    // Client A reads PaneCreated
    let frame_a = ciri_protocol::codec::read_frame(&mut cr_a).await.unwrap();
    let new_pane_id = match frame_a {
        ciri_protocol::codec::Frame::ServerMsg(ServerMessage::PaneCreated { pane_id, .. }) => {
            pane_id
        }
        other => panic!("client A: expected PaneCreated, got {other:?}"),
    };

    // Client B should also see PaneCreated (broadcast)
    let frame_b = ciri_protocol::codec::read_frame(&mut cr_b).await.unwrap();
    match frame_b {
        ciri_protocol::codec::Frame::ServerMsg(ServerMessage::PaneCreated { pane_id, .. }) => {
            assert_eq!(pane_id, new_pane_id, "both clients see same new pane ID");
        }
        other => panic!("client B: expected PaneCreated, got {other:?}"),
    }

    // Both should see LayoutUpdate with 2 panes in the layout
    let frame_a2 = ciri_protocol::codec::read_frame(&mut cr_a).await.unwrap();
    match frame_a2 {
        ciri_protocol::codec::Frame::ServerMsg(ServerMessage::LayoutUpdate { layout }) => {
            let total_panes: usize = layout
                .workspaces
                .iter()
                .flat_map(|ws| &ws.columns)
                .map(|col| col.tiles.len())
                .sum();
            assert_eq!(total_panes, 2, "client A layout should show 2 panes");
        }
        other => panic!("client A: expected LayoutUpdate, got {other:?}"),
    }

    let frame_b2 = ciri_protocol::codec::read_frame(&mut cr_b).await.unwrap();
    match frame_b2 {
        ciri_protocol::codec::Frame::ServerMsg(ServerMessage::LayoutUpdate { layout }) => {
            let total_panes: usize = layout
                .workspaces
                .iter()
                .flat_map(|ws| &ws.columns)
                .map(|col| col.tiles.len())
                .sum();
            assert_eq!(total_panes, 2, "client B layout should show 2 panes");
            // Verify the new pane ID appears in the layout
            let all_pane_ids: Vec<u64> = layout
                .workspaces
                .iter()
                .flat_map(|ws| &ws.columns)
                .flat_map(|col| &col.tiles)
                .map(|t| t.pane_id)
                .collect();
            assert!(
                all_pane_ids.contains(&new_pane_id),
                "client B layout should contain new pane {new_pane_id}"
            );
        }
        other => panic!("client B: expected LayoutUpdate, got {other:?}"),
    }

    handle_a.abort();
    handle_b.abort();
}

// ═══════════════════════════════════════════════════════════════════
// Resize parametric tests
// ═══════════════════════════════════════════════════════════════════

#[rstest]
#[case(0, 0, 0, 768)] // zero cols/rows/width
#[case(80, 24, 0, 0)] // zero pixel dimensions
#[case(80, 24, 1024, 0)] // zero height
#[case(80, 24, 20000, 768)] // width too large
fn resize_rejects_invalid_combinations(
    mut server_with_session: (Server, String),
    #[case] _cols: u16,
    #[case] _rows: u16,
    #[case] width: u32,
    #[case] height: u32,
) {
    let (ref mut server, _) = server_with_session;
    let before_w = server.clients.get(&1).unwrap().viewport_width;

    let responses = server.handle_message(
        ClientMessage::Resize {
            cols: _cols,
            rows: _rows,
            width,
            height,
            cell_width: 8.0,
            cell_height: 16.0,
        },
        1,
    );

    assert!(responses.is_empty());
    assert_eq!(server.clients.get(&1).unwrap().viewport_width, before_w);
}

#[rstest]
#[case(80, 24, 1024, 768, 8.0, 16.0)]
#[case(120, 40, 1920, 1080, 9.5, 18.0)]
fn resize_accepts_valid_dimensions(
    mut server_with_session: (Server, String),
    #[case] _cols: u16,
    #[case] _rows: u16,
    #[case] width: u32,
    #[case] height: u32,
    #[case] cw: f32,
    #[case] ch: f32,
) {
    let (ref mut server, _) = server_with_session;
    let _responses = server.handle_message(
        ClientMessage::Resize {
            cols: _cols,
            rows: _rows,
            width,
            height,
            cell_width: cw,
            cell_height: ch,
        },
        1,
    );

    let client = server.clients.get(&1).unwrap();
    assert_eq!(client.viewport_width, width as f32);
    assert_eq!(client.viewport_height, height as f32);
    assert_eq!(client.cell_width, cw);
    assert_eq!(client.cell_height, ch);
}

#[test]
fn resize_marks_next_full_sync_to_replace_scrollback() {
    let mut server = Server::new("", 8.0, TerminalColors::default());
    let session_name = "alpha".to_string();
    server.clients.insert(1, test_client(1, &session_name));
    let pane_id = {
        let session = server.get_or_create_session(&session_name);
        session.workspaces.active().active_pane_id().unwrap()
    };

    let _ = server.handle_message(
        ClientMessage::Resize {
            cols: 120,
            rows: 40,
            width: 1920,
            height: 1080,
            cell_width: 8.0,
            cell_height: 16.0,
        },
        1,
    );

    let damage = server
        .clients
        .get(&1)
        .and_then(|client| client.damage.get(&pane_id))
        .expect("resize should enqueue full damage for the pane");
    assert!(damage.full, "resize should require a full sync");
    assert!(
        damage.replace_scrollback,
        "resize full sync should rebuild visible scrollback instead of appending by watermark"
    );
}
