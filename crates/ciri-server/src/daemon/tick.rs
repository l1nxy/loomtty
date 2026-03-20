use ciri_protocol::codec;
use ciri_protocol::message::*;
use ciri_protocol::transport;
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};
use tokio::time::{interval, Duration, Instant};

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

    loop {
        ticker.tick().await;

        // ── Phase 1 (locked): process PTY, extract damage, collect snapshots ──
        //
        // Snapshot-then-release pattern (optimization #5): we read all pane
        // data while holding the lock, collect it into lightweight snapshot
        // structs, clone the tx handles we need, then drop the lock before
        // doing any encoding or sending.

        /// Raw snapshot data extracted under the lock for deferred encoding.
        enum Snapshot {
            FullSync {
                sync: FullPaneSync,
                current_history: usize,
                pane_id: u64,
            },
            Delta(CellDelta),
        }

        struct PendingSend {
            client_id: u64,
            session_name: String,
            snapshot: Snapshot,
        }

        let mut pending_sends: Vec<PendingSend> = Vec::new();
        let mut should_shutdown = false;

        {
            let mut s = tick_state.lock().await;

            // Iterate all sessions
            let session_names: Vec<String> = s.sessions.keys().cloned().collect();
            let mut sessions_to_remove: Vec<String> = Vec::new();

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
                    if let Ok(payload) = rmp_serde::to_vec(clip_msg) {
                        let mut frame = frame_pool.pop().unwrap_or_default();
                        frame.clear();
                        frame.reserve(5 + payload.len());
                        frame.push(0x10); // TAG_SERVER_MSG
                        frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                        frame.extend_from_slice(&payload);
                        for client in s.clients.values() {
                            if client.session_name == *session_name {
                                let _ = client.tx.try_send(frame.clone());
                            }
                        }
                        if frame_pool.len() < FRAME_POOL_CAP {
                            frame_pool.push(frame);
                        }
                    }
                }

                // Clean up exited panes
                let dead = session.cleanup_exited_panes(&mut s.clients);
                if !dead.is_empty() {
                    session.mark_session_dirty();
                    // Broadcast close messages to session clients
                    let mut broadcasts = Vec::new();
                    for &id in &dead {
                        let close_payload =
                            rmp_serde::to_vec(&ServerMessage::PaneClosed { pane_id: id })
                                .unwrap_or_default();
                        broadcasts.push(close_payload);
                    }
                    let layout_payload = rmp_serde::to_vec(&ServerMessage::LayoutUpdate {
                        layout: session.layout_state(),
                    })
                    .unwrap_or_default();
                    broadcasts.push(layout_payload);

                    for client in s.clients.values() {
                        if client.session_name == *session_name {
                            for payload in &broadcasts {
                                let mut frame = frame_pool.pop().unwrap_or_default();
                                frame.clear();
                                frame.reserve(5 + payload.len());
                                frame.push(0x10);
                                frame.extend_from_slice(
                                    &(payload.len() as u32).to_le_bytes(),
                                );
                                frame.extend_from_slice(payload);
                                if let Err(e) = client.tx.try_send(frame.clone()) {
                                    log::warn!(
                                        "failed to send close frame to client {}: {e}",
                                        client.id
                                    );
                                }
                                if frame_pool.len() < FRAME_POOL_CAP {
                                    frame_pool.push(frame);
                                }
                            }
                        }
                    }
                }

                session.autosave_if_due(Instant::now());

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

                    // Send shutdown to clients of this session
                    if let Ok(payload) = rmp_serde::to_vec(&ServerMessage::ServerShutdown) {
                        for client in s.clients.values() {
                            if client.session_name == *session_name {
                                let mut frame = frame_pool.pop().unwrap_or_default();
                                frame.clear();
                                frame.reserve(5 + payload.len());
                                frame.push(0x10);
                                frame.extend_from_slice(
                                    &(payload.len() as u32).to_le_bytes(),
                                );
                                frame.extend_from_slice(&payload);
                                if let Err(e) = client.tx.try_send(frame.clone()) {
                                    log::warn!(
                                        "failed to send shutdown to client {}: {e}",
                                        client.id
                                    );
                                }
                                if frame_pool.len() < FRAME_POOL_CAP {
                                    frame_pool.push(frame);
                                }
                            }
                        }
                    }

                    // Remove clients of this session
                    let to_remove: Vec<u64> = s
                        .clients
                        .iter()
                        .filter(|(_, c)| c.session_name == *session_name)
                        .map(|(id, _)| *id)
                        .collect();
                    for cid in to_remove {
                        s.clients.remove(&cid);
                    }

                    sessions_to_remove.push(session_name.clone());
                    // Don't re-insert this session
                } else {
                    // Collect damage snapshots for clients of this session
                    let session_client_ids: Vec<u64> = s
                        .clients
                        .iter()
                        .filter(|(_, c)| c.session_name == *session_name)
                        .map(|(id, _)| *id)
                        .collect();

                    let mut pending: Vec<(u64, u64, DamageAccumulator)> = Vec::new();
                    for &cid in &session_client_ids {
                        if let Some(client) = s.clients.get_mut(&cid) {
                            let pane_ids: Vec<u64> = client.damage.keys().copied().collect();
                            for pane_id in pane_ids {
                                let acc = client.damage.get_mut(&pane_id).unwrap();
                                if !acc.is_empty() {
                                    pending.push((cid, pane_id, acc.take()));
                                }
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
                                let sync = pane.snapshot_incremental(pgen, last_sent);
                                let current_history = pane.history_size();
                                pending_sends.push(PendingSend {
                                    client_id: cid,
                                    session_name: session_name.clone(),
                                    snapshot: Snapshot::FullSync {
                                        sync,
                                        current_history,
                                        pane_id,
                                    },
                                });
                            }
                        } else if let Some(pane) = session.panes.get(&pane_id) {
                            let (cursor_line, cursor_col, cursor_shape, mode_flags) =
                                pane.cursor_info();
                            let mut regions = Vec::new();

                            for (&line, &(left, right)) in &damage.line_damage {
                                let cells = pane.read_cells(line, left, right);
                                regions.push(DamageRegion {
                                    line,
                                    left,
                                    right,
                                    cells,
                                });
                            }

                            let delta = CellDelta {
                                pane_id,
                                generation: pgen,
                                cursor_line,
                                cursor_col,
                                cursor_shape,
                                mode_flags,
                                regions,
                            };
                            pending_sends.push(PendingSend {
                                client_id: cid,
                                session_name: session_name.clone(),
                                snapshot: Snapshot::Delta(delta),
                            });
                        }
                    }

                    // Put session back
                    s.sessions.insert(session_name.clone(), session);
                }
            }

            // Remove dead sessions
            for name in sessions_to_remove {
                s.sessions.remove(&name);
            }

            // Server shutdown: had sessions before but now all gone and no clients
            if s.had_session && s.sessions.is_empty() && s.clients.is_empty() {
                log::info!("all sessions ended and no clients, shutting down server");
                should_shutdown = true;
            }
        } // lock dropped here — Phase 1 complete

        // Signal shutdown
        if should_shutdown {
            let _ = std::fs::remove_file(&transport::server_socket_path());
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
                let mut buf = frame_pool.pop().unwrap_or_default();
                let (history_update, encode_ok) = match &snapshot {
                    Snapshot::FullSync {
                        sync,
                        current_history,
                        pane_id,
                    } => {
                        let ok = codec::encode_full_pane_sync_framed(&mut buf, sync).is_ok();
                        (Some((*pane_id, *current_history)), ok)
                    }
                    Snapshot::Delta(delta) => {
                        let ok = codec::encode_cell_delta_framed(&mut buf, delta).is_ok();
                        (None, ok)
                    }
                };

                if encode_ok {
                    encoded.push(EncodedFrame {
                        client_id,
                        session_name,
                        buf,
                        history_update,
                    });
                } else {
                    // Return buffer to pool on encode failure
                    if frame_pool.len() < FRAME_POOL_CAP {
                        frame_pool.push(buf);
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
                    match client.tx.try_send(buf) {
                        Ok(()) => {
                            // buf consumed by channel — do not return to pool
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
                                client
                                    .damage
                                    .entry(pid)
                                    .or_insert_with(DamageAccumulator::new)
                                    .mark_full();
                            }
                            // Recover the buffer and return it to the pool
                            let buf = e.into_inner();
                            if frame_pool.len() < FRAME_POOL_CAP {
                                frame_pool.push(buf);
                            }
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
