//! Cross-crate integration tests.
//!
//! Only tests that genuinely exercise multiple crates together belong here.
//! Per-crate tests (session persistence, name validation, agent detection)
//! live in each crate's own test modules.

use ciri_protocol::codec;
use ciri_protocol::message::*;

// ═══════════════════════════════════════════════════════════════════
// Protocol ↔ Session: LayoutState survives encode→decode
// ═══════════════════════════════════════════════════════════════════

#[test]
fn layout_state_roundtrips_through_protocol_encoding() {
    let layout = LayoutState {
        workspaces: vec![WorkspaceState {
            columns: vec![
                ColumnState {
                    tiles: vec![
                        TileState {
                            pane_id: 1,
                            weight: 1.0,
                        },
                        TileState {
                            pane_id: 2,
                            weight: 2.0,
                        },
                    ],
                    active_tile_idx: 0,
                    width_proportion: 0.5,
                    width_fixed_px: None,
                },
                ColumnState {
                    tiles: vec![TileState {
                        pane_id: 3,
                        weight: 1.0,
                    }],
                    active_tile_idx: 0,
                    width_proportion: 0.5,
                    width_fixed_px: Some(400.0),
                },
            ],
            active_column_idx: 1,
        }],
        active_workspace_idx: 0,
    };

    let msg = ServerMessage::LayoutUpdate {
        layout: layout.clone(),
    };
    let frame = codec::frame_server_msg(&msg).unwrap();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        match codec::read_frame(&mut &frame[..]).await.unwrap() {
            codec::Frame::ServerMsg(ServerMessage::LayoutUpdate { layout: decoded }) => {
                assert_eq!(decoded.workspaces.len(), 1);
                assert_eq!(decoded.workspaces[0].columns.len(), 2);
                assert_eq!(decoded.workspaces[0].columns[0].tiles.len(), 2);
                assert_eq!(decoded.workspaces[0].columns[0].tiles[1].weight, 2.0);
                assert_eq!(decoded.workspaces[0].columns[1].width_fixed_px, Some(400.0));
                assert_eq!(decoded.workspaces[0].active_column_idx, 1);
            }
            other => panic!("expected LayoutUpdate, got {other:?}"),
        }
    });
}

// ═══════════════════════════════════════════════════════════════════
// Protocol: FullPaneSync with hyperlinks, graphemes, and CWD
// ═══════════════════════════════════════════════════════════════════

#[test]
fn full_pane_sync_preserves_rich_metadata() {
    let mut sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 42,
            generation: 7,
            cursor_line: -5,
            cursor_col: 5,
            cursor_shape: CURSOR_BEAM,
            mode_flags: MODE_ALT_SCREEN | MODE_MOUSE_REPORT,
            received_ack: 99,
            echo_ack: 99,
        },
        cols: 10,
        rows: 2,
        title: "links-test".to_string(),
        scrollback: Vec::new(),
        scrollback_rows: 0,
        scrollback_replace: false,
        cells: vec![PackedCell::default(); 20],
        grapheme_extras: GraphemeExtras::new(),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: Some("/home/user/project".to_string()),
    };
    sync.grapheme_extras.push(3, "é");
    sync.hyperlink_extras.cell_links.push((5, 1));
    sync.hyperlink_extras
        .link_map
        .push((1, "https://example.com".to_string()));

    let payload = codec::encode_full_pane_sync_payload(&sync).unwrap();
    let decoded = codec::decode_full_pane_sync(&payload).unwrap();

    assert_eq!(decoded.meta.pane_id, 42);
    assert_eq!(decoded.meta.cursor_line, -5);
    assert_eq!(decoded.meta.echo_ack, 99);
    assert_eq!(decoded.meta.mode_flags, MODE_ALT_SCREEN | MODE_MOUSE_REPORT);
    assert_eq!(decoded.meta.cursor_shape, CURSOR_BEAM);
    assert_eq!(decoded.title, "links-test");
    assert_eq!(decoded.cwd, Some("/home/user/project".to_string()));
    assert_eq!(decoded.grapheme_extras.0.len(), 1);
    assert_eq!(decoded.hyperlink_extras.cell_links, vec![(5, 1)]);
    assert_eq!(
        decoded.hyperlink_extras.link_map,
        vec![(1, "https://example.com".to_string())]
    );
}

// ═══════════════════════════════════════════════════════════════════
// Protocol: agent state persists through session save→restore→protocol
// ═══════════════════════════════════════════════════════════════════

#[test]
fn agent_state_survives_session_save_and_layout_encoding() {
    use ciri_session::agent::*;
    use ciri_session::state::*;

    let tmp = tempfile::tempdir().unwrap();

    // Save session with agent info
    let state = SessionState {
        name: "agent-roundtrip".to_string(),
        workspaces: vec![SavedWorkspace {
            columns: vec![SavedColumn {
                tiles: vec![SavedTile {
                    pane_id: 1,
                    weight: 1.0,
                    cwd: Some("/home".to_string()),
                    title: None,
                    agent: Some(SavedAgent {
                        kind: AgentKind::ClaudeCode,
                    }),
                }],
                active_tile_idx: 0,
                width_proportion: 1.0,
                width_fixed_px: None,
            }],
            active_column_idx: 0,
        }],
        active_workspace_idx: 0,
    };
    ciri_session::save::save_session(&state, tmp.path()).unwrap();

    // Restore and verify agent survived
    let restored = ciri_session::restore::restore_session("agent-roundtrip", tmp.path())
        .unwrap()
        .unwrap();
    let agent = restored.workspaces[0].columns[0].tiles[0]
        .agent
        .as_ref()
        .unwrap();
    assert!(matches!(agent.kind, AgentKind::ClaudeCode));
    assert_eq!(resume_command(agent.kind), Some("claude --continue"));

    // Verify the restored layout can be encoded as a protocol LayoutState
    let layout = LayoutState {
        workspaces: vec![WorkspaceState {
            columns: vec![ColumnState {
                tiles: vec![TileState {
                    pane_id: 1,
                    weight: 1.0,
                }],
                active_tile_idx: 0,
                width_proportion: 1.0,
                width_fixed_px: None,
            }],
            active_column_idx: 0,
        }],
        active_workspace_idx: 0,
    };
    let msg = ServerMessage::StateSync {
        layout,
        pane_ids: vec![1],
    };
    let frame = codec::frame_server_msg(&msg).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        match codec::read_frame(&mut &frame[..]).await.unwrap() {
            codec::Frame::ServerMsg(ServerMessage::StateSync { pane_ids, .. }) => {
                assert_eq!(pane_ids, vec![1]);
            }
            other => panic!("expected StateSync, got {other:?}"),
        }
    });
}

// ═══════════════════════════════════════════════════════════════════
// Protocol: handshake + bidirectional message exchange over duplex
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn protocol_handshake_and_bidirectional_exchange() {
    let (client_r, server_w) = tokio::io::duplex(4096);
    let (server_r, client_w) = tokio::io::duplex(4096);

    // Server echoes Pings as Pongs
    let server_task = tokio::spawn(async move {
        let mut r = tokio::io::BufReader::new(server_r);
        let mut w = tokio::io::BufWriter::new(server_w);

        let (compat, received) = codec::read_client_hello(&mut r).await.unwrap();
        assert!(matches!(
            compat,
            codec::VersionCompat::Exact(_)
                | codec::VersionCompat::PatchMismatch { .. }
                | codec::VersionCompat::MinorMismatch { .. }
        ));
        assert_eq!(received.session_name, "e2e-bidirectional");
        assert_eq!(received.width, 1920);

        codec::write_server_hello(&mut w).await.unwrap();
        tokio::io::AsyncWriteExt::flush(&mut w).await.unwrap();

        // Echo 10 Pings
        for _ in 0..10 {
            match codec::read_frame(&mut r).await.unwrap() {
                codec::Frame::ClientMsg(ClientMessage::Ping {
                    seq,
                    client_time_us,
                }) => {
                    codec::encode_server_msg(
                        &mut w,
                        &ServerMessage::Pong {
                            seq,
                            client_time_us,
                        },
                    )
                    .await
                    .unwrap();
                    tokio::io::AsyncWriteExt::flush(&mut w).await.unwrap();
                }
                other => panic!("expected Ping, got {other:?}"),
            }
        }
    });

    // Client sends 10 Pings, reads 10 Pongs
    let client_task = tokio::spawn(async move {
        let mut r = tokio::io::BufReader::new(client_r);
        let mut w = tokio::io::BufWriter::new(client_w);

        codec::write_client_hello(
            &mut w,
            &codec::ClientHello {
                session_name: "e2e-bidirectional".to_string(),
                width: 1920,
                height: 1080,
                cell_width: 9.0,
                cell_height: 18.0,
            },
        )
        .await
        .unwrap();
        tokio::io::AsyncWriteExt::flush(&mut w).await.unwrap();
        let _ = codec::read_server_hello(&mut r).await.unwrap();

        // Send burst
        for i in 0..10u64 {
            codec::encode_client_msg(
                &mut w,
                &ClientMessage::Ping {
                    seq: i,
                    client_time_us: i * 1000,
                },
            )
            .await
            .unwrap();
        }
        tokio::io::AsyncWriteExt::flush(&mut w).await.unwrap();

        // Read all Pongs
        for i in 0..10u64 {
            match codec::read_frame(&mut r).await.unwrap() {
                codec::Frame::ServerMsg(ServerMessage::Pong {
                    seq,
                    client_time_us,
                }) => {
                    assert_eq!(seq, i);
                    assert_eq!(client_time_us, i * 1000);
                }
                other => panic!("expected Pong seq={i}, got {other:?}"),
            }
        }
    });

    server_task.await.unwrap();
    client_task.await.unwrap();
}

// ═══════════════════════════════════════════════════════════════════
// Protocol: large FullPaneSync through fragmented duplex transport
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn large_full_pane_sync_through_small_duplex_buffer() {
    // Build a 120×40 terminal with varied content (forces non-trivial SM encoding)
    let mut cells = Vec::new();
    for row in 0u8..40 {
        for col in 0u8..120 {
            let mut c = PackedCell::with_ch(char::from(b'A' + (col % 26)));
            c.fg = PackedColor::rgb(row * 6, col * 2, 128);
            if col % 5 == 0 {
                c.flags = FLAG_BOLD.to_le_bytes();
            }
            cells.push(c);
        }
    }
    let sync = FullPaneSync {
        meta: PaneFrameMeta {
            pane_id: 77,
            generation: 33,
            cursor_line: 20,
            cursor_col: 60,
            cursor_shape: CURSOR_UNDERLINE,
            mode_flags: MODE_MOUSE_REPORT,
            received_ack: 42,
            echo_ack: 42,
        },
        cols: 120,
        rows: 40,
        title: "large-terminal".to_string(),
        scrollback: Vec::new(),
        scrollback_rows: 0,
        scrollback_replace: false,
        cells: cells.clone(),
        grapheme_extras: GraphemeExtras::new(),
        hyperlink_extras: HyperlinkExtras::new(),
        cwd: Some("/home/user".to_string()),
    };

    let framed = codec::frame_full_pane_sync(&sync).unwrap();
    assert!(
        framed.len() > 500,
        "frame should be sizeable: {}B",
        framed.len()
    );

    // Send through a tiny duplex buffer to force fragmentation
    let (mut writer, reader) = tokio::io::duplex(64);
    let mut reader = tokio::io::BufReader::new(reader);

    let write_task = tokio::spawn(async move {
        tokio::io::AsyncWriteExt::write_all(&mut writer, &framed)
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::shutdown(&mut writer)
            .await
            .unwrap();
    });

    match codec::read_frame(&mut reader).await.unwrap() {
        codec::Frame::FullPaneSync(decoded) => {
            assert_eq!(decoded.meta.pane_id, 77);
            assert_eq!(decoded.meta.generation, 33);
            assert_eq!(decoded.meta.cursor_line, 20);
            assert_eq!(decoded.meta.echo_ack, 42);
            assert_eq!(decoded.cols, 120);
            assert_eq!(decoded.rows, 40);
        }
        other => panic!("expected FullPaneSync, got {other:?}"),
    }

    write_task.await.unwrap();
}
