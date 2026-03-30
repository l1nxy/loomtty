use bytes::Bytes;
use ciri_protocol::codec;
use ciri_protocol::message::*;
use ciri_protocol::transport;
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};
use tokio::time::{Duration, Instant, interval};

use super::damage::DamageAccumulator;
use super::server::Server;

/// Spawn the tick loop (16ms = ~60fps). Processes PTY output, extracts damage,
/// encodes frames, and sends to clients.
pub(crate) async fn run_tick_loop(tick_state: Arc<Mutex<Server>>, tick_shutdown: Arc<Notify>) {
    let mut ticker = interval(Duration::from_millis(16));

    // ── Frame buffer pool (optimization #2) ─────────────────────
    // Reusable Vec<u8> buffers to avoid per-frame allocation.
    // Capped at 64 to bound memory usage.
    const FRAME_POOL_CAP: usize = 64;
    let mut frame_pool: Vec<Vec<u8>> = Vec::with_capacity(FRAME_POOL_CAP);

    /// Raw snapshot data extracted under the lock for deferred encoding.
    enum Snapshot {
        FullSync {
            sync: FullPaneSync,
            current_history: usize,
            pane_id: u64,
        },
        /// Pre-encoded delta frame (cells streamed directly into frame buffer).
        DeltaEncoded(Vec<u8>),
    }

    struct PendingSend {
        client_id: u64,
        session_name: String,
        snapshot: Snapshot,
    }

    // Reusable buffers to avoid per-tick allocations
    let mut pending_sends: Vec<PendingSend> = Vec::new();
    let mut session_names: Vec<String> = Vec::new();

    loop {
        ticker.tick().await;

        // ── Phase 1 (locked): process PTY, extract damage, collect snapshots ──
        //
        // Snapshot-then-release pattern (optimization #5): we read all pane
        // data while holding the lock, collect it into lightweight snapshot
        // structs, clone the tx handles we need, then drop the lock before
        // doing any encoding or sending.

        pending_sends.clear();
        let mut should_shutdown = false;

        {
            let mut s = tick_state.lock().await;

            // Iterate all sessions
            session_names.clear();
            session_names.extend(s.sessions.keys().cloned());

            for session_name in &session_names {
                // Temporarily remove session from the map to avoid borrow conflicts
                // between session and s.clients.
                let mut session = match s.sessions.remove(session_name) {
                    Some(sess) => sess,
                    None => continue,
                };

                // Process PTY output and extract damage
                let clipboard_msgs = session.process_pty_and_damage(&mut s.clients);

                // Send OSC 52 clipboard writes to clients of this session
                for clip_msg in &clipboard_msgs {
                    if let Some(frame) = codec::frame_server_msg(clip_msg) {
                        let frame = Bytes::from(frame);
                        for client in s.clients.values() {
                            if client.session_name == *session_name {
                                let _ = client.tx.try_send(frame.clone());
                            }
                        }
                    }
                }

                // Clean up exited panes
                let dead = session.cleanup_exited_panes(&mut s.clients);
                if !dead.is_empty() {
                    session.mark_session_dirty();
                    // Resize remaining panes to fill the freed space
                    session.resize_all_panes(&mut s.clients);
                    // Build broadcast frames for close + layout update
                    let mut broadcast_frames: Vec<Bytes> = Vec::new();
                    for &id in &dead {
                        if let Some(f) =
                            codec::frame_server_msg(&ServerMessage::PaneClosed { pane_id: id })
                        {
                            broadcast_frames.push(Bytes::from(f));
                        }
                    }
                    if let Some(f) = codec::frame_server_msg(&ServerMessage::LayoutUpdate {
                        layout: session.layout_state(),
                    }) {
                        broadcast_frames.push(Bytes::from(f));
                    }
                    for client in s.clients.values() {
                        if client.session_name == *session_name {
                            for frame in &broadcast_frames {
                                if let Err(e) = client.tx.try_send(frame.clone()) {
                                    log::warn!(
                                        "failed to send close frame to client {}: {e}",
                                        client.id
                                    );
                                }
                            }
                        }
                    }
                }

                session.autosave_if_due(Instant::now());

                // Agent detection on slower timer (default 30s)
                if s.session_config.restore_agents {
                    let now = Instant::now();
                    if session.agent_detection_due(now, s.session_config.agent_save_interval_secs) {
                        let changed = session.detect_agents();
                        session.last_agent_save = Some(now);
                        if changed {
                            session.mark_session_dirty();
                        }
                    }
                }

                // If session has no panes left, mark for removal
                if session.panes.is_empty() {
                    let _ = ciri_session::restore::delete_session(
                        session_name,
                        &transport::state_dir(),
                    );
                    log::info!(
                        "session '{}': all panes exited, removing session",
                        session_name
                    );

                    // Send shutdown to clients, then drop their tx senders so
                    // the writer tasks flush the frame and exit naturally.
                    // handle_client detects EOF → cleanup_client removes them.
                    let session_clients: Vec<u64> = s
                        .clients
                        .iter()
                        .filter(|(_, c)| c.session_name == *session_name)
                        .map(|(id, _)| *id)
                        .collect();
                    if let Some(frame) = codec::frame_server_msg(&ServerMessage::ServerShutdown) {
                        let frame = Bytes::from(frame);
                        for &cid in &session_clients {
                            if let Some(client) = s.clients.get(&cid) {
                                let _ = client.tx.try_send(frame.clone());
                            }
                        }
                    }
                    // Remove clients: drops tx → writer flushes remaining frames → closes socket
                    for cid in session_clients {
                        s.clients.remove(&cid);
                    }

                    // Don't re-insert this session (already removed above)
                } else {
                    // Collect damage snapshots for clients of this session.
                    // Skip panes with synchronized output (DEC 2026) active —
                    // damage accumulates and is sent when sync mode is turned off.
                    let mut pending: Vec<(u64, u64, DamageAccumulator)> = Vec::new();
                    for (&cid, client) in s.clients.iter_mut() {
                        if client.session_name != *session_name {
                            continue;
                        }
                        for (&pane_id, acc) in client.damage.iter_mut() {
                            if !acc.is_empty() {
                                // DEC 2026: defer sending while pane is in sync mode
                                if let Some(pane) = session.panes.get(&pane_id)
                                    && pane.is_sync_output()
                                {
                                    continue;
                                }
                                pending.push((cid, pane_id, acc.take()));
                            }
                        }
                    }

                    // Phase 1: read pane data under lock, build snapshot structs,
                    // clone tx handles for deferred sending.
                    for (cid, pane_id, damage) in pending {
                        let pgen = session.generation.get(&pane_id).copied().unwrap_or(0);
                        if !s.clients.contains_key(&cid) {
                            continue;
                        }

                        if damage.full {
                            if let Some(pane) = session.panes.get(&pane_id) {
                                let last_sent = s
                                    .clients
                                    .get(&cid)
                                    .and_then(|c| c.history_sent.get(&pane_id).copied())
                                    .unwrap_or(0);
                                // When pane is in alt screen, preserve history_sent
                                // (alt buffer has no scrollback — history_size() returns 0).
                                let current_total = if pane.is_alt_screen() {
                                    last_sent
                                } else {
                                    pane.scrollback_total()
                                };
                                let sync =
                                    build_scrollback_sync(pane, pgen, last_sent, current_total);
                                pending_sends.push(PendingSend {
                                    client_id: cid,
                                    session_name: session_name.clone(),
                                    snapshot: Snapshot::FullSync {
                                        sync,
                                        current_history: current_total,
                                        pane_id,
                                    },
                                });
                            }
                        } else if let Some(pane) = session.panes.get(&pane_id) {
                            // Use monotonic scrollback_total to detect new scrollback,
                            // including ring buffer rotations after the buffer is full.
                            let current_total = pane.scrollback_total();
                            let last_sent = s
                                .clients
                                .get(&cid)
                                .and_then(|c| c.history_sent.get(&pane_id).copied())
                                .unwrap_or(0);

                            if current_total > last_sent {
                                // New scrollback — send FullPaneSync with delta history
                                let sync =
                                    build_scrollback_sync(pane, pgen, last_sent, current_total);
                                pending_sends.push(PendingSend {
                                    client_id: cid,
                                    session_name: session_name.clone(),
                                    snapshot: Snapshot::FullSync {
                                        sync,
                                        current_history: current_total,
                                        pane_id,
                                    },
                                });
                            } else {
                                // No new scrollback — send lightweight CellDelta
                                let (cursor_line, cursor_col, cursor_shape, mode_flags) =
                                    pane.cursor_info();

                                let regions: Vec<(u16, u16, u16)> = damage
                                    .line_damage
                                    .iter()
                                    .map(|(&line, &(left, right))| (line, left, right))
                                    .collect();

                                let cols = pane.grid_cols();
                                let meta = PaneFrameMeta {
                                    pane_id,
                                    generation: pgen,
                                    cursor_line,
                                    cursor_col,
                                    cursor_shape,
                                    mode_flags,
                                };
                                let mut buf = frame_pool.pop().unwrap_or_default();
                                let ok = codec::encode_cell_delta_streaming_framed(
                                    &mut buf,
                                    &meta,
                                    cols,
                                    &regions,
                                    |line, left, right, enc| {
                                        pane.write_cells_into_sm(line, left, right, enc)
                                    },
                                )
                                .is_ok();

                                if ok {
                                    pending_sends.push(PendingSend {
                                        client_id: cid,
                                        session_name: session_name.clone(),
                                        snapshot: Snapshot::DeltaEncoded(buf),
                                    });
                                } else if frame_pool.len() < FRAME_POOL_CAP {
                                    frame_pool.push(buf);
                                }
                            }
                        }
                    }

                    // Put session back
                    s.sessions.insert(session_name.clone(), session);
                }
            }

            // Server shutdown: had sessions before but now all gone and no clients
            if s.had_session && s.sessions.is_empty() && s.clients.is_empty() {
                log::info!("all sessions ended and no clients, shutting down server");
                should_shutdown = true;
            }
        } // lock dropped here — Phase 1 complete

        // Signal shutdown
        if should_shutdown {
            // Yield to the executor so writer tasks can flush pending
            // ServerShutdown frames to their sockets before we tear down.
            tokio::time::sleep(Duration::from_millis(50)).await;
            let _ = std::fs::remove_file(transport::server_socket_path());
            tick_shutdown.notify_one();
            return;
        }

        // ── Phase 2 (unlocked): encode snapshots, then re-lock to validate + send ──
        //
        // Encoding (RLE, bytemuck serialization) happens without holding
        // the server lock. We use the frame pool to avoid per-frame alloc.
        //
        // After encoding, we re-acquire the lock to validate client session
        // affinity before sending. A client may have switched sessions
        // (SwitchSession) between Phase 1 and Phase 2; sending old-session
        // data to such a client would corrupt its state.
        if !pending_sends.is_empty() {
            struct EncodedFrame {
                client_id: u64,
                session_name: String,
                buf: Vec<u8>,
                history_update: Option<(u64, usize)>,
            }
            let mut encoded: Vec<EncodedFrame> = Vec::new();

            for PendingSend {
                client_id,
                session_name,
                snapshot,
            } in pending_sends.drain(..)
            {
                match snapshot {
                    Snapshot::FullSync {
                        sync,
                        current_history,
                        pane_id,
                    } => {
                        let mut buf = frame_pool.pop().unwrap_or_default();
                        if codec::encode_full_pane_sync_framed(&mut buf, &sync).is_ok() {
                            encoded.push(EncodedFrame {
                                client_id,
                                session_name,
                                buf,
                                history_update: Some((pane_id, current_history)),
                            });
                        } else if frame_pool.len() < FRAME_POOL_CAP {
                            frame_pool.push(buf);
                        }
                    }
                    Snapshot::DeltaEncoded(buf) => {
                        // Already encoded in Phase 1 — no work to do
                        encoded.push(EncodedFrame {
                            client_id,
                            session_name,
                            buf,
                            history_update: None,
                        });
                    }
                }
            }

            // Re-lock to validate affinity and send
            let mut s = tick_state.lock().await;
            let mut to_disconnect = Vec::new();
            for EncodedFrame {
                client_id,
                session_name,
                buf,
                history_update,
            } in encoded.drain(..)
            {
                if let Some(client) = s.clients.get_mut(&client_id) {
                    // Revalidate client affinity: if the client switched sessions
                    // between Phase 1 and now, drop the stale frame.
                    if client.session_name != session_name {
                        log::debug!(
                            "client {client_id} switched session ({session_name} -> {}), dropping stale frame",
                            client.session_name
                        );
                        if frame_pool.len() < FRAME_POOL_CAP {
                            frame_pool.push(buf);
                        }
                        continue;
                    }
                    match client.tx.try_send(Bytes::from(buf)) {
                        Ok(()) => {
                            client.send_failures = 0;
                            if let Some((pid, hist)) = history_update {
                                client.history_sent.insert(pid, hist);
                            }
                        }
                        Err(e) => {
                            client.send_failures += 1;
                            if client.send_failures >= 100 {
                                log::warn!(
                                    "disconnecting slow client {client_id}: {} consecutive failures",
                                    client.send_failures
                                );
                                to_disconnect.push(client_id);
                            } else {
                                log::debug!(
                                    "send to client {client_id} failed (#{})",
                                    client.send_failures
                                );
                            }
                            // On failure for FullPaneSync, re-mark full so it retries next tick
                            if let Some((pid, _)) = history_update {
                                client.damage.entry(pid).or_default().mark_full();
                            }
                            // Bytes consumed the Vec; cannot recover for pool
                            drop(e);
                        }
                    }
                } else {
                    // Client not found — return buffer to pool
                    if frame_pool.len() < FRAME_POOL_CAP {
                        frame_pool.push(buf);
                    }
                }
            }
            for cid in to_disconnect {
                s.clients.remove(&cid);
            }
        }
    }
}

/// Build a FullPaneSync with the right scrollback delta, handling both the
/// growing phase and ring-buffer rotation (where history_size() is capped).
///
/// - `last_sent`: the client's `scrollback_total` watermark
/// - `current_total`: the pane's current `scrollback_total()`
///
/// When `delta > history_size()`, the server has rotated past what the client
/// has: send ALL current scrollback with `scrollback_replace = true`.
/// Otherwise, send only the new rows (incremental append).
fn build_scrollback_sync(
    pane: &ciri_term::pane::Pane,
    generation: u64,
    last_sent: usize,
    current_total: usize,
) -> FullPaneSync {
    let hs = pane.history_size();
    let delta = current_total.saturating_sub(last_sent);
    let rows_to_send = delta.min(hs);
    let replace = delta > hs;

    // Compute the `history_sent` value that snapshot_incremental expects:
    // it will send `history_size - adjusted_sent` rows from the end.
    let adjusted_sent = hs.saturating_sub(rows_to_send);
    let mut sync = pane.snapshot_incremental(generation, adjusted_sent);
    sync.scrollback_replace = replace;
    sync
}
