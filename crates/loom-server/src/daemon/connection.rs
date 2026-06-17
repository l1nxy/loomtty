use bytes::Bytes;
use loom_protocol::codec;
use loom_protocol::message::*;
use loom_protocol::transport;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::{Mutex, mpsc};
use tokio::time::Instant;

use super::client::ClientState;
use super::damage::DamageAccumulator;
use super::server::{Server, ServerResponse};

const CONTROL_SESSION: &str = "__control__";
/// Handshake sentinel: "attach me to whatever session makes sense"
/// (most-recently-attached, else a fresh one). Resolved server-side so
/// clients that can't run the desktop's pre-handshake session-picking
/// (notably the browser) still get bare-`loomtty` behavior. The real
/// name is reported back via `SessionSwitched` once resolved.
const AUTO_SESSION: &str = "__auto__";

/// Perform graceful shutdown: save all sessions, notify all clients, remove socket.
/// Idempotent — safe to call multiple times (second call is a no-op).
pub(crate) async fn graceful_shutdown(state: &Arc<Mutex<Server>>) {
    let mut s = state.lock().await;
    if s.shut_down {
        return;
    }
    s.shut_down = true;
    // Stop the web gateway, if one is running, so the daemon exits cleanly.
    // Flipping the latch true also cancels every live WebSocket session.
    if let Some(gateway_shutdown) = s.web_gateway_shutdown.take() {
        let _ = gateway_shutdown.send(true);
    }
    // Detach the accept-loop task; the latch above stops it. No await here —
    // we hold the Server lock, and daemon exit doesn't need the port freed.
    s.web_gateway_task = None;
    s.web_gateway_addr = None;
    // Detect agents and save all sessions
    let restore_agents = s.session_config.restore_agents;
    for session in s.sessions.values_mut() {
        if restore_agents {
            session.detect_agents();
        }
        let _ = session.save_session();
    }
    log::info!("shutting down gracefully");

    // Send shutdown to all clients
    if let Some(frame) = codec::frame_server_msg(&ServerMessage::ServerShutdown) {
        let frame = Bytes::from(frame);
        for client in s.clients.values() {
            if let Err(e) = client.tx.try_send(frame.clone()) {
                log::warn!("failed to send shutdown to client {}: {e}", client.id);
            }
        }
    }
    drop(s);
    let _ = std::fs::remove_file(transport::server_socket_path());
}

/// Ensures client is removed from server state. Safe to call multiple times.
pub(crate) async fn cleanup_client(state: &Arc<Mutex<Server>>, client_id: u64) {
    let mut s = state.lock().await;
    if s.clients.remove(&client_id).is_some() {
        log::info!("client {client_id} disconnected");
    }
}

/// Handle a single client connection (handshake, reader loop, writer task).
///
/// `cancel` is the per-gateway cancel latch for web (WebSocket) clients —
/// `None` for the desktop/TCP/pipe paths. When it flips, the reader loop
/// exits through the SAME cleanup as a disconnect (deregister + writer abort),
/// so disabling Web / rotating the token actually revokes the browser instead
/// of leaving a registered client that keeps receiving broadcast output.
pub(crate) async fn handle_client<R, W>(
    reader: R,
    writer: W,
    state: Arc<Mutex<Server>>,
    client_shutdown: Arc<tokio::sync::Notify>,
    input_notify: Arc<tokio::sync::Notify>,
    mut cancel: Option<tokio::sync::watch::Receiver<bool>>,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let mut reader = tokio::io::BufReader::new(reader);
    let mut writer = BufWriter::new(writer);

    // Read ClientHello (version + viewport + session name). Web clients race
    // the read against the per-gateway cancel latch, so a socket that upgraded
    // just before — or whose hello is delayed until after — a disable / token
    // rotation is dropped here, before it's registered or sent any sync.
    let hello_read = match cancel.as_mut() {
        Some(rx) => tokio::select! {
            biased;
            _ = rx.wait_for(|cancelled| *cancelled) => return,
            r = codec::read_client_hello(&mut reader) => r,
        },
        None => codec::read_client_hello(&mut reader).await,
    };
    let hello = match hello_read {
        Ok((codec::VersionCompat::Exact(v), h)) => {
            log::info!("client handshake ok (v{v}), session={}", h.session_name);
            h
        }
        Ok((codec::VersionCompat::PatchMismatch { peer, local }, h)) => {
            log::warn!("client version {peer} differs from server {local} (patch mismatch)");
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

    let mut requested_session = hello.session_name.clone();
    log::info!("client requested session: {}", requested_session);

    // Validate session name (allow __control__ for CLI commands, and the
    // __auto__ sentinel which is resolved to a real name below). Both are
    // `__`-prefixed and would otherwise be rejected by `validate_name`.
    let is_control = requested_session == CONTROL_SESSION;
    let is_auto = requested_session == AUTO_SESSION;
    if !is_control && !is_auto && loom_session::names::validate_name(&requested_session).is_err() {
        log::error!("invalid session name from client: {:?}", requested_session);
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
    let (tx, mut rx) = mpsc::channel::<Bytes>(256);
    let client_id;
    let initial_frames: Vec<Vec<u8>>;
    {
        let mut s = state.lock().await;
        // Final revocation check under the lock: if the gateway was disabled or
        // its token rotated between the hello race above and here, drop the
        // connection without registering the client or building a sync — so a
        // revoked socket can't grab even one terminal snapshot.
        if cancel.as_ref().is_some_and(|rx| *rx.borrow()) {
            return;
        }
        client_id = s.next_client_id;
        s.next_client_id += 1;

        // Resolve the __auto__ sentinel to a concrete session before it's
        // used as a key anywhere below. The client is told the real name
        // via the SessionSwitched frame prepended to the initial sync.
        if is_auto {
            requested_session = s.resolve_auto_session();
            log::info!("auto-attach resolved to session: {}", requested_session);
        }

        // Register client with session affinity
        s.clients.insert(
            client_id,
            ClientState {
                id: client_id,
                tx: tx.clone(),
                damage: HashMap::new(),
                last_acked_generation: 0,
                max_input_seq: HashMap::new(),
                received_input_seq: HashMap::new(),
                history_sent: HashMap::new(),
                send_failures: 0,
                cell_width: client_cell_w,
                cell_height: client_cell_h,
                viewport_width: client_viewport_w,
                viewport_height: client_viewport_h,
                session_name: requested_session.clone(),
                last_sent_cursor: HashMap::new(),
            },
        );

        // Cancel any pending idle shutdown (real clients only — control/probe
        // clients should not prevent the server from winding down).
        if !is_control {
            s.idle_deadline = None;
        }

        // Skip session creation for control clients (CLI commands)
        if !is_control {
            let session = s.get_or_create_session(&requested_session);
            session.last_attached = Instant::now();
        }

        // Temporarily remove session to avoid borrow conflicts
        let session_opt = s.sessions.remove(&requested_session);

        if let Some(ref session) = session_opt {
            // Mark all session panes for full sync for this client
            let pane_keys: Vec<u64> = session.panes.keys().copied().collect();
            if let Some(client) = s.clients.get_mut(&client_id) {
                for pane_id in &pane_keys {
                    let mut acc = DamageAccumulator::default();
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

            // For auto-attach, tell the client the real session name up
            // front (before any pane paints) so it can pin its replay
            // ClientHello and update its URL. SessionSwitched is the same
            // message the live `SwitchSession` path uses, so the client
            // already knows how to handle it.
            if is_auto {
                if let Some(frame) = codec::frame_server_msg(&ServerMessage::SessionSwitched {
                    session_name: requested_session.clone(),
                }) {
                    frames.push(frame);
                }
            }

            let (sync_msg, pane_syncs, image_events) = session.build_state_sync();
            if let Some(frame) = codec::frame_server_msg(&sync_msg) {
                frames.push(frame);
            }
            for sync in &pane_syncs {
                if let Some(frame) = codec::frame_full_pane_sync(sync) {
                    frames.push(frame);
                }
            }
            for msg in &image_events {
                if let Some(frame) = codec::frame_server_msg(msg) {
                    frames.push(frame);
                }
            }

            let pane_histories: Vec<(u64, usize)> = session
                .panes
                .iter()
                .map(|(&pid, pane)| {
                    (
                        pid,
                        super::server::Server::history_sent_after_full_sync(pane),
                    )
                })
                .collect();

            if let Some(client) = s.clients.get_mut(&client_id) {
                for acc in client.damage.values_mut() {
                    *acc = DamageAccumulator::default();
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

    // Flush ServerHello (still buffered in BufWriter) before handing off to writer task.
    // Without this, control clients (no initial frames) never receive the hello.
    if let Err(e) = writer.flush().await {
        log::error!("failed to flush server hello: {e}");
        cleanup_client(&state, client_id).await;
        return;
    }

    // Send frames without holding the lock
    for frame in initial_frames {
        let _ = tx.send(Bytes::from(frame)).await;
    }
    // Drop original sender — only the clone in ClientState should keep the
    // channel alive.  When the tick loop removes this client from s.clients,
    // the last sender is dropped, rx.recv() returns None, the writer exits,
    // and the socket closes so the client-side reader detects EOF.
    drop(tx);

    // Spawn writer task — batch multiple pending frames before flushing
    // to reduce syscall overhead on high-throughput output.
    let write_handle = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            if writer.write_all(&frame).await.is_err() {
                break;
            }
            // Drain any additional frames that are already queued
            while let Ok(frame) = rx.try_recv() {
                if writer.write_all(&frame).await.is_err() {
                    return;
                }
            }
            // Single flush for the entire batch
            if writer.flush().await.is_err() {
                break;
            }
        }
    });

    // Reader loop with guaranteed cleanup on panic or error
    let reader_result = std::panic::AssertUnwindSafe(async {
        loop {
            let frame = match cancel.as_mut() {
                // Web clients carry the per-gateway cancel latch. Breaking the
                // loop here (instead of dropping this future from outside) lets
                // the cleanup below run, so the client is deregistered and its
                // writer aborted — i.e. the browser is actually revoked.
                Some(rx) => tokio::select! {
                    biased;
                    _ = rx.changed() => break,
                    f = codec::read_frame(&mut reader) => f,
                },
                None => codec::read_frame(&mut reader).await,
            };
            match frame {
                Ok(codec::Frame::ClientMsg(msg)) => {
                    let is_input = matches!(msg, ClientMessage::Input { .. });
                    let mut s = state.lock().await;
                    let responses = s.handle_message(msg, client_id);
                    if is_input {
                        input_notify.notify_one();
                    }

                    for resp in responses {
                        match resp {
                            ServerResponse::BroadcastToSession(session_name, server_msg) => {
                                s.broadcast_to_session(&session_name, &server_msg);
                            }
                            ServerResponse::SendToClient(cid, server_msg) => {
                                s.send_to_client(cid, &server_msg);
                            }
                            ServerResponse::SendFullPaneSync(cid, sync) => {
                                if let (Some(client), Some(frame)) =
                                    (s.clients.get(&cid), codec::frame_full_pane_sync(&sync))
                                    && let Err(e) = client.tx.try_send(Bytes::from(frame))
                                {
                                    log::warn!(
                                        "failed to send full pane sync to client {cid}: {e}"
                                    );
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
                                drop(s);
                                graceful_shutdown(&state).await;
                                client_shutdown.notify_one();
                                return;
                            }
                            ServerResponse::StartWebGateway(mut params) => {
                                // Ordering depends on whether a gateway is already
                                // bound to the requested PORT (not the full addr —
                                // `0.0.0.0:p` overlaps `127.0.0.1:p`, so a same-port
                                // loopback↔all-interfaces switch still clashes):
                                //   • same port (token rotation, bind switch): the
                                //     port is already ours, so stop + await the old
                                //     gateway before we can rebind it.
                                //   • different port: bind the NEW listener first
                                //     (no clash with the still-running old one) so a
                                //     failed bind leaves the old, working gateway
                                //     untouched; retire the old only once the new
                                //     one is live.
                                // Bind + spawn `.await`, so the lock is released
                                // across all of it and re-acquired afterward.
                                let port_in_use = matches!(
                                    s.web_gateway_addr,
                                    Some(cur) if cur.port() == params.port
                                );
                                if port_in_use {
                                    let old_shutdown = s.web_gateway_shutdown.take();
                                    let old_task = s.web_gateway_task.take();
                                    s.web_gateway_addr = None;
                                    drop(s);
                                    if let Some(old) = old_shutdown {
                                        let _ = old.send(true);
                                    }
                                    if let Some(task) = old_task {
                                        // Returns once `serve_web` has dropped
                                        // its listener, i.e. the port is free.
                                        let _ = task.await;
                                    }
                                } else {
                                    drop(s);
                                }
                                let result = super::start_web_gateway(
                                    &params.token,
                                    &params.bind,
                                    params.port,
                                    &params.allowed_origins,
                                    &params.static_dir,
                                    state.clone(),
                                    client_shutdown.clone(),
                                    input_notify.clone(),
                                )
                                .await;
                                // Wipe the token we received over the socket.
                                {
                                    use zeroize::Zeroize;
                                    params.token.zeroize();
                                }
                                s = state.lock().await;
                                let reply = match result {
                                    Ok((addr, gateway_shutdown, task)) => {
                                        // Different-port success: the old gateway is
                                        // still installed — retire it now (stopping
                                        // it also aborts its live WS sessions). Its
                                        // port differs, so there's nothing to await.
                                        if !port_in_use {
                                            if let Some(old) = s.web_gateway_shutdown.take() {
                                                let _ = old.send(true);
                                            }
                                            let _ = s.web_gateway_task.take();
                                        }
                                        s.web_gateway_shutdown = Some(gateway_shutdown);
                                        s.web_gateway_task = Some(task);
                                        s.web_gateway_addr = Some(addr);
                                        ServerMessage::CommandResult {
                                            success: true,
                                            message: format!("web gateway listening on {addr}"),
                                            pane_id: None,
                                        }
                                    }
                                    Err(e) => ServerMessage::CommandResult {
                                        success: false,
                                        message: format!("failed to start web gateway: {e}"),
                                        pane_id: None,
                                    },
                                };
                                s.send_to_client(client_id, &reply);
                            }
                            ServerResponse::StopWebGateway => {
                                // Flip the latch (stops the accept loop and
                                // cancels live WS sessions), then join the accept
                                // loop so the port is actually released before we
                                // reply — a quick disable→enable rebinds cleanly.
                                let old_shutdown = s.web_gateway_shutdown.take();
                                let old_task = s.web_gateway_task.take();
                                s.web_gateway_addr = None;
                                drop(s);
                                if let Some(old) = old_shutdown {
                                    let _ = old.send(true);
                                }
                                if let Some(task) = old_task {
                                    let _ = task.await;
                                }
                                s = state.lock().await;
                                s.send_to_client(
                                    client_id,
                                    &ServerMessage::CommandResult {
                                        success: true,
                                        message: "web gateway stopped".to_string(),
                                        pane_id: None,
                                    },
                                );
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
    let () = reader_result;
    cleanup_client(&state, client_id).await;
    write_handle.abort();
}
