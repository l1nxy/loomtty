use ciri_protocol::codec;
use ciri_protocol::message::*;
use ciri_protocol::transport;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::{mpsc, Mutex};

use super::client::ClientState;
use super::damage::DamageAccumulator;
use super::server::{Server, ServerResponse};

/// Perform graceful shutdown: save all sessions, notify all clients, remove socket.
pub(crate) async fn graceful_shutdown(state: &Arc<Mutex<Server>>) {
    let s = state.lock().await;
    // Save all sessions
    for session in s.sessions.values() {
        let _ = session.save_session();
    }
    log::info!("shutting down gracefully");

    // Send shutdown to all clients
    if let Ok(payload) = rmp_serde::to_vec(&ServerMessage::ServerShutdown) {
        for client in s.clients.values() {
            let mut frame = Vec::with_capacity(5 + payload.len());
            frame.push(0x10);
            frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            frame.extend_from_slice(&payload);
            if let Err(e) = client.tx.try_send(frame) {
                log::warn!("failed to send shutdown to client {}: {e}", client.id);
            }
        }
    }
    drop(s);
    let _ = std::fs::remove_file(&transport::server_socket_path());
}

/// Ensures client is removed from server state. Safe to call multiple times.
pub(crate) async fn cleanup_client(state: &Arc<Mutex<Server>>, client_id: u64) {
    let mut s = state.lock().await;
    if s.clients.remove(&client_id).is_some() {
        log::info!("client {client_id} disconnected");
    }
}

/// Handle a single client connection (handshake, reader loop, writer task).
pub(crate) async fn handle_client<R, W>(
    reader: R,
    writer: W,
    state: Arc<Mutex<Server>>,
    client_shutdown: Arc<tokio::sync::Notify>,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let mut reader = tokio::io::BufReader::new(reader);
    let mut writer = BufWriter::new(writer);

    // Read ClientHello (version + viewport + session name)
    let hello = match codec::read_client_hello(&mut reader).await {
        Ok((codec::VersionCompat::Exact(v), h)) => {
            log::info!("client handshake ok (v{v}), session={}", h.session_name);
            h
        }
        Ok((codec::VersionCompat::PatchMismatch { peer, local }, h)) => {
            log::warn!(
                "client version {peer} differs from server {local} (patch mismatch)"
            );
            h
        }
        Ok((codec::VersionCompat::MinorMismatch { peer, local }, h)) => {
            log::warn!(
                "client version {peer} differs from server {local} (minor mismatch, may be unstable)"
            );
            h
        }
        Err(e) => {
            log::debug!("client hello rejected: {e}");
            return;
        }
    };

    let requested_session = hello.session_name.clone();
    log::info!("client requested session: {}", requested_session);

    // Validate session name (allow __control__ for CLI commands)
    let is_control = requested_session == "__control__";
    if !is_control && ciri_session::save::validate_session_name(&requested_session).is_err() {
        log::error!(
            "invalid session name from client: {:?}",
            requested_session
        );
        return;
    }

    let client_viewport_w = hello.width as f32;
    let client_viewport_h = hello.height as f32;
    let client_cell_w = hello.cell_width;
    let client_cell_h = hello.cell_height;

    // Send ServerHello
    if let Err(e) = codec::write_server_hello(&mut writer).await {
        log::error!("failed to send server hello: {e}");
        return;
    }

    // Register client and get/create session
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(256);
    let client_id;
    let initial_frames: Vec<Vec<u8>>;
    {
        let mut s = state.lock().await;
        client_id = s.next_client_id;
        s.next_client_id += 1;

        // Register client with session affinity
        s.clients.insert(
            client_id,
            ClientState {
                id: client_id,
                tx: tx.clone(),
                damage: HashMap::new(),
                last_acked_generation: 0,
                history_sent: HashMap::new(),
                send_failures: 0,
                cell_width: client_cell_w,
                cell_height: client_cell_h,
                viewport_width: client_viewport_w,
                viewport_height: client_viewport_h,
                session_name: requested_session.clone(),
            },
        );

        // Skip session creation for control clients (CLI commands)
        if !is_control {
            s.get_or_create_session(&requested_session);
        }

        // Temporarily remove session to avoid borrow conflicts
        let session_opt = s.sessions.remove(&requested_session);

        if let Some(ref session) = session_opt {
            // Mark all session panes for full sync for this client
            let pane_keys: Vec<u64> = session.panes.keys().copied().collect();
            if let Some(client) = s.clients.get_mut(&client_id) {
                for pane_id in &pane_keys {
                    let mut acc = DamageAccumulator::new();
                    acc.mark_full();
                    client.damage.insert(*pane_id, acc);
                }
            }
        }

        // Recompute effective viewport and build initial sync frames
        let mut frames = Vec::new();
        if let Some(mut session) = session_opt {
            session.resize_all_panes(&mut s.clients);

            log::info!(
                "client {client_id} connected to session '{}'",
                requested_session
            );

            let (sync_msg, pane_syncs) = session.build_state_sync();
            if let Ok(payload) = rmp_serde::to_vec(&sync_msg) {
                let mut frame = Vec::with_capacity(5 + payload.len());
                frame.push(0x10);
                frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                frame.extend_from_slice(&payload);
                frames.push(frame);
            }
            for sync in &pane_syncs {
                if let Ok(payload) = codec::encode_full_pane_sync_payload(sync) {
                    let mut frame = Vec::with_capacity(5 + payload.len());
                    frame.push(0x21);
                    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                    frame.extend_from_slice(&payload);
                    frames.push(frame);
                }
            }

            let pane_histories: Vec<(u64, usize)> = session
                .panes
                .iter()
                .map(|(&pid, pane)| (pid, pane.history_size()))
                .collect();

            if let Some(client) = s.clients.get_mut(&client_id) {
                for acc in client.damage.values_mut() {
                    *acc = DamageAccumulator::new();
                }
                for (pane_id, history) in pane_histories {
                    client.history_sent.insert(pane_id, history);
                }
            }

            s.sessions.insert(requested_session.clone(), session);
        } else {
            log::info!("control client {client_id} connected (no session)");
        }

        initial_frames = frames;
    } // lock dropped here

    // Send frames without holding the lock
    for frame in initial_frames {
        let _ = tx.send(frame).await;
    }

    // Spawn writer task
    let write_handle = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            if writer.write_all(&frame).await.is_err() {
                break;
            }
            if writer.flush().await.is_err() {
                break;
            }
        }
    });

    // Reader loop with guaranteed cleanup on panic or error
    let reader_result = std::panic::AssertUnwindSafe(async {
        loop {
            match codec::read_frame(&mut reader).await {
                Ok(codec::Frame::ClientMsg(msg)) => {
                    let mut s = state.lock().await;
                    let responses = s.handle_message(msg, client_id);

                    for resp in responses {
                        match resp {
                            ServerResponse::BroadcastToSession(session_name, server_msg) => {
                                s.broadcast_to_session(&session_name, &server_msg);
                            }
                            ServerResponse::SendToClient(cid, server_msg) => {
                                s.send_to_client(cid, &server_msg);
                            }
                            ServerResponse::SendFullPaneSync(cid, sync) => {
                                if let Some(client) = s.clients.get(&cid) {
                                    if let Ok(payload) =
                                        codec::encode_full_pane_sync_payload(&sync)
                                    {
                                        let mut frame =
                                            Vec::with_capacity(5 + payload.len());
                                        frame.push(0x21);
                                        frame.extend_from_slice(
                                            &(payload.len() as u32).to_le_bytes(),
                                        );
                                        frame.extend_from_slice(&payload);
                                        if let Err(e) = client.tx.try_send(frame) {
                                            log::warn!(
                                                "failed to send full pane sync to client {cid}: {e}"
                                            );
                                        }
                                    }
                                }
                            }
                            ServerResponse::RemoveClient(cid) => {
                                s.clients.remove(&cid);
                                log::info!("client {cid} detached");
                                if cid == client_id {
                                    return;
                                }
                            }
                            ServerResponse::ShutdownServer => {
                                // Save all sessions, notify all clients
                                for session in s.sessions.values() {
                                    let _ = session.save_session();
                                }
                                if let Ok(payload) =
                                    rmp_serde::to_vec(&ServerMessage::ServerShutdown)
                                {
                                    for client in s.clients.values() {
                                        let mut frame =
                                            Vec::with_capacity(5 + payload.len());
                                        frame.push(0x10);
                                        frame.extend_from_slice(
                                            &(payload.len() as u32).to_le_bytes(),
                                        );
                                        frame.extend_from_slice(&payload);
                                        let _ = client.tx.try_send(frame);
                                    }
                                }
                                drop(s);
                                let _ = std::fs::remove_file(
                                    &transport::server_socket_path(),
                                );
                                // Signal the accept loop and tick loop to shut down
                                client_shutdown.notify_one();
                                return;
                            }
                        }
                    }
                }
                Ok(_) => {
                    log::warn!("unexpected frame type from client {client_id}");
                }
                Err(e) => {
                    if e.kind() != std::io::ErrorKind::UnexpectedEof {
                        log::warn!("client {client_id} read error: {e}");
                    }
                    break;
                }
            }
        }
    })
    .await;

    // Cleanup always runs
    let _ = reader_result;
    cleanup_client(&state, client_id).await;
    write_handle.abort();
}
