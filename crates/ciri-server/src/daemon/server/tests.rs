use super::*;
use std::sync::Arc;
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
fn resize_ignores_invalid_dimensions() {
    let mut server = Server::new("/bin/sh", 8.0, TerminalColors::default());
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
    let mut server = Server::new("/bin/sh", 8.0, TerminalColors::default());
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
    let mut server = Server::new("/bin/sh", 8.0, TerminalColors::default());
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
    let mut server = Server::new("/bin/sh", 8.0, TerminalColors::default());
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
fn close_pane_emits_close_then_layout_update() {
    let mut server = Server::new("/bin/sh", 8.0, TerminalColors::default());
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
fn focus_pane_by_id_returns_layout_update_and_command_result() {
    let mut server = Server::new("/bin/sh", 8.0, TerminalColors::default());
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
        let state = Arc::new(Mutex::new(Server::new(
            "/bin/sh",
            8.0,
            TerminalColors::default(),
        )));
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
