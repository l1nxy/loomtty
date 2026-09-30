use bytes::Bytes;
use loom_protocol::codec;
use loom_protocol::message::*;
use loom_protocol::transport;
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};
use tokio::time::{Duration, Instant};

use super::connection;
use super::damage::DamageAccumulator;
use super::server::Server;

/// Wait once per iteration so a PTY notification is never consumed without
/// processing its output. Only pending frames/cursor commits need a timer; an
/// idle server has no periodic wakeups, and new output can interrupt the wait.
async fn wait_for_tick(input_notify: &Notify, retry_at: Option<Instant>) {
    if let Some(deadline) = retry_at {
        tokio::select! {
            _ = input_notify.notified() => {},
            _ = tokio::time::sleep_until(deadline) => {},
        }
    } else {
        input_notify.notified().await;
    }
}

/// Process PTY output when notified, extract damage, encode and send frames.
pub(crate) async fn run_tick_loop(
    tick_state: Arc<Mutex<Server>>,
    tick_shutdown: Arc<Notify>,
    input_notify: Arc<Notify>,
    frame_interval: Duration,
) {
    // No fixed-rate ticker: the loop wakes on input_notify which fires
    // for both client input AND PTY output (event-driven, near-zero idle CPU).
    // Do not sleep after an empty drain: doing so delays the *next* output
    // burst/terminal reply even though there is nothing to render now.
    let mut retry_at = None;
    let mut next_frame = Instant::now();

    // ── Frame buffer pool (optimization #2) ─────────────────────
    // Reusable Vec<u8> buffers to avoid per-frame allocation.
    // Capped at 64 to bound memory usage.
    const FRAME_POOL_CAP: usize = 64;
    let mut frame_pool: Vec<Vec<u8>> = Vec::with_capacity(FRAME_POOL_CAP);

    /// Raw snapshot data extracted under the lock for deferred encoding.
    #[allow(clippy::large_enum_variant)] // Clippy 1.94: snapshot enum avoids boxing in hot tick/send path.
    enum Snapshot {
        FullSync {
            sync: FullPaneSync,
            current_history: usize,
            pane_id: u64,
            force_scrollback_replace: bool,
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

    let mut had_pty_data: bool;
    let mut cursor_held_pending: bool;
    loop {
        had_pty_data = false;
        cursor_held_pending = false;
        wait_for_tick(&input_notify, retry_at.take()).await;
        // PTY parsing and terminal replies above/below never wait on this
        // deadline. Only expensive viewport snapshots and client rendering
        // are coalesced to at most one batch per display frame.
        let send_frame = Instant::now() >= next_frame;
        let mut frame_pending = false;

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
                // Track if any pane had PTY data this tick by checking
                // whether drain produced any chunks (via process_pty_output return).
                if session.last_tick_had_pty_data {
                    had_pty_data = true;
                }

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
                    for &(id, exit_code) in &dead {
                        if let Some(f) = codec::frame_server_msg(&ServerMessage::PaneClosed {
                            pane_id: id,
                            exit_code,
                        }) {
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
                    // Fulfill `run-command --wait`: reply to any client awaiting
                    // one of these panes (it may live in a different session,
                    // e.g. the __control__ CLI client).
                    for &(id, exit_code) in &dead {
                        if let Some(client_id) = s.pending_pane_waits.remove(&id)
                            && let Some(f) = codec::frame_server_msg(&ServerMessage::PaneClosed {
                                pane_id: id,
                                exit_code,
                            })
                            && let Some(c) = s.clients.get(&client_id)
                        {
                            let _ = c.tx.try_send(Bytes::from(f));
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
                    // A failed delete leaves the saved-session file on disk,
                    // which makes a future reconnect under this name look
                    // "restorable" even though the live session is gone —
                    // logged (not just swallowed) so that class of failure
                    // is diagnosable instead of surfacing only as a client
                    // that mysteriously refuses to reattach.
                    if let Err(e) =
                        loom_session::restore::delete_session(session_name, &transport::state_dir())
                    {
                        log::warn!(
                            "session '{}': failed to delete saved-session file: {e}",
                            session_name
                        );
                    }
                    log::info!(
                        "session '{}': all panes exited, removing session",
                        session_name
                    );

                    // Release our local copy so `finalize_session_removal`
                    // sees the session-map state it expects (target already
                    // removed).
                    drop(session);

                    // Any other session we can fall back to? If yes, run the
                    // same auto-switch flow as an explicit KillSession so
                    // clients stay attached and land on a live session. If
                    // not, this was the last session on the server — keep the
                    // old "shut down the client" behavior.
                    let has_fallback = s
                        .sessions
                        .keys()
                        .any(|n| n.as_str() != session_name.as_str() && !n.starts_with("__"));

                    if has_fallback {
                        let mut responses = Vec::new();
                        s.finalize_session_removal(session_name, &mut responses);
                        s.dispatch_responses(responses);
                    } else {
                        // Send shutdown to clients, then drop their tx senders
                        // so the writer tasks flush the frame and exit
                        // naturally. handle_client detects EOF →
                        // cleanup_client removes them.
                        let session_clients: Vec<u64> = s
                            .clients
                            .iter()
                            .filter(|(_, c)| c.session_name == *session_name)
                            .map(|(id, _)| *id)
                            .collect();
                        if let Some(frame) = codec::frame_server_msg(&ServerMessage::ServerShutdown)
                        {
                            let frame = Bytes::from(frame);
                            for &cid in &session_clients {
                                if let Some(client) = s.clients.get(&cid) {
                                    let _ = client.tx.try_send(frame.clone());
                                }
                            }
                        }
                        for cid in session_clients {
                            s.clients.remove(&cid);
                        }
                    }

                    // Don't re-insert this session (already removed above)
                } else {
                    if !send_frame {
                        // Leave accumulated damage intact and arrange a timer
                        // even if this is the last PTY chunk in the burst.
                        // DEC 2026 holds updates until the app ends its batch;
                        // do not poll indefinitely on intentionally held data.
                        frame_pending |= s.clients.values().any(|client| {
                            client.session_name == *session_name
                                && client.damage.iter().any(|(pane_id, damage)| {
                                    !damage.is_empty()
                                        && session
                                            .panes
                                            .get(pane_id)
                                            .is_some_and(|pane| !pane.is_sync_output())
                                })
                        });
                        s.sessions.insert(session_name.clone(), session);
                        continue;
                    }
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
                        let Some(client_ref) = s.clients.get(&cid) else {
                            continue;
                        };
                        let client_echo_ack =
                            client_ref.max_input_seq.get(&pane_id).copied().unwrap_or(0);
                        // Early ack: server has *received* the input but the
                        // PTY may not have echoed yet. Used by the client to
                        // validate cursor predictions promptly even when the
                        // shell silently drops the input (e.g. Backspace at
                        // the prompt boundary, where no PTY output flows back
                        // and `client_echo_ack` would never advance).
                        let client_received_ack = client_ref
                            .received_input_seq
                            .get(&pane_id)
                            .copied()
                            .unwrap_or(0);

                        if damage.full {
                            if let Some(pane) = session.panes.get(&pane_id) {
                                let last_sent = s
                                    .clients
                                    .get(&cid)
                                    .and_then(|c| c.history_sent.get(&pane_id).copied())
                                    .unwrap_or(0);
                                let force_scrollback_replace =
                                    damage.replace_scrollback && !pane.is_alt_screen();
                                // When pane is in alt screen, preserve history_sent
                                // (alt buffer has no scrollback — history_size() returns 0).
                                let current_total =
                                    Server::visible_scrollback_total(pane, last_sent);
                                let mut sync = if force_scrollback_replace {
                                    let mut sync = pane.snapshot_incremental(pgen, 0);
                                    sync.scrollback_replace = true;
                                    sync
                                } else {
                                    build_scrollback_sync(pane, pgen, last_sent, current_total)
                                };
                                sync.meta.received_ack = client_received_ack;
                                sync.meta.echo_ack = client_echo_ack;
                                let ((cline, ccol, cshape), cursor_held) = Server::throttle_cursor(
                                    &mut s.clients,
                                    cid,
                                    pane_id,
                                    (
                                        sync.meta.cursor_line,
                                        sync.meta.cursor_col,
                                        sync.meta.cursor_shape,
                                    ),
                                    sync.meta.mode_flags & MODE_ALT_SCREEN != 0,
                                );
                                sync.meta.cursor_line = cline;
                                sync.meta.cursor_col = ccol;
                                sync.meta.cursor_shape = cshape;
                                pending_sends.push(PendingSend {
                                    client_id: cid,
                                    session_name: session_name.clone(),
                                    snapshot: Snapshot::FullSync {
                                        sync,
                                        current_history: current_total,
                                        pane_id,
                                        force_scrollback_replace,
                                    },
                                });
                                if cursor_held && let Some(client) = s.clients.get_mut(&cid) {
                                    client.damage.entry(pane_id).or_default().cursor_dirty = true;
                                    cursor_held_pending = true;
                                }
                            }
                        } else if let Some(pane) = session.panes.get(&pane_id) {
                            // Track the primary-screen scrollback watermark
                            // visible to this client. It advances with new
                            // output, stays pinned while alt-screen is active,
                            // and may rewind after a resize reabsorbs history.
                            let last_sent = s
                                .clients
                                .get(&cid)
                                .and_then(|c| c.history_sent.get(&pane_id).copied())
                                .unwrap_or(0);
                            let current_total = Server::visible_scrollback_total(pane, last_sent);

                            if current_total != last_sent {
                                // New scrollback — send scrollback-only FullPaneSync
                                // (rows=0, cells=[]) so viewport is not re-encoded.
                                let mut sync = build_scrollback_only_sync(
                                    pane,
                                    pgen,
                                    last_sent,
                                    current_total,
                                );
                                sync.meta.received_ack = client_received_ack;
                                sync.meta.echo_ack = client_echo_ack;
                                let ((cline, ccol, cshape), cursor_held) = Server::throttle_cursor(
                                    &mut s.clients,
                                    cid,
                                    pane_id,
                                    (
                                        sync.meta.cursor_line,
                                        sync.meta.cursor_col,
                                        sync.meta.cursor_shape,
                                    ),
                                    sync.meta.mode_flags & MODE_ALT_SCREEN != 0,
                                );
                                sync.meta.cursor_line = cline;
                                sync.meta.cursor_col = ccol;
                                sync.meta.cursor_shape = cshape;
                                pending_sends.push(PendingSend {
                                    client_id: cid,
                                    session_name: session_name.clone(),
                                    snapshot: Snapshot::FullSync {
                                        sync,
                                        current_history: current_total,
                                        pane_id,
                                        force_scrollback_replace: false,
                                    },
                                });
                                if cursor_held && let Some(client) = s.clients.get_mut(&cid) {
                                    client.damage.entry(pane_id).or_default().cursor_dirty = true;
                                    cursor_held_pending = true;
                                }
                                // Also send CellDelta for any viewport damage.
                                // Reuse the cursor we already throttled above:
                                // `throttle_cursor` MUST be called at most once
                                // per tick per (client, pane) — a second call
                                // with the same `actual` in this same tick would
                                // hit `pending == Some(actual)` and falsely
                                // satisfy the 2-tick dwell, leaking a transient.
                                if !damage.line_damage.is_empty() {
                                    let mode_flags = pane.cursor_info().3;
                                    let regions: Vec<(u16, u16, u16)> = damage
                                        .line_damage
                                        .iter()
                                        .map(|(&line, &(left, right))| (line, left, right))
                                        .collect();
                                    let cols = pane.grid_cols();
                                    let meta = PaneFrameMeta {
                                        pane_id,
                                        generation: pgen,
                                        cursor_line: cline,
                                        cursor_col: ccol,
                                        cursor_shape: cshape,
                                        mode_flags,
                                        received_ack: client_received_ack,
                                        echo_ack: client_echo_ack,
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
                                    // cursor_held was already wired above for
                                    // the FullSync; the CellDelta ships the
                                    // same value so no extra re-mark needed.
                                }
                            } else {
                                // No new scrollback — send lightweight CellDelta
                                let info = pane.cursor_info();
                                let mode_flags = info.3;
                                let ((cursor_line, cursor_col, cursor_shape), cursor_held) =
                                    Server::throttle_cursor(
                                        &mut s.clients,
                                        cid,
                                        pane_id,
                                        (info.0, info.1, info.2),
                                        mode_flags & MODE_ALT_SCREEN != 0,
                                    );

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
                                    received_ack: client_received_ack,
                                    echo_ack: client_echo_ack,
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
                                if cursor_held && let Some(client) = s.clients.get_mut(&cid) {
                                    client.damage.entry(pane_id).or_default().cursor_dirty = true;
                                    cursor_held_pending = true;
                                }
                            }
                        }
                    }

                    // Put session back
                    s.sessions.insert(session_name.clone(), session);
                }
            }

            // Idle shutdown: start a deadline when all sessions/clients are gone.
            if s.had_session && s.sessions.is_empty() && s.clients.is_empty() {
                if s.idle_deadline.is_none() {
                    log::info!(
                        "all sessions ended and no clients, idle shutdown in {}s",
                        s.idle_timeout.as_secs()
                    );
                    s.idle_deadline = Some(std::time::Instant::now() + s.idle_timeout);
                }
                if s.idle_deadline
                    .is_some_and(|d| std::time::Instant::now() >= d)
                {
                    log::info!("idle timeout reached, shutting down server");
                    should_shutdown = true;
                }
            } else if s.idle_deadline.is_some() {
                log::info!("idle shutdown cancelled");
                s.idle_deadline = None;
            }
        } // lock dropped here — Phase 1 complete

        // Signal shutdown
        if should_shutdown {
            // Yield to the executor so writer tasks can flush pending
            // ServerShutdown frames to their sockets before we tear down.
            tokio::time::sleep(Duration::from_millis(50)).await;
            connection::graceful_shutdown(&tick_state).await;
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
            next_frame = Instant::now() + frame_interval;
            struct EncodedFrame {
                client_id: u64,
                session_name: String,
                buf: Vec<u8>,
                history_update: Option<(u64, usize)>,
                force_scrollback_replace: bool,
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
                        force_scrollback_replace,
                    } => {
                        let mut buf = frame_pool.pop().unwrap_or_default();
                        if codec::encode_full_pane_sync_framed(&mut buf, &sync).is_ok() {
                            encoded.push(EncodedFrame {
                                client_id,
                                session_name,
                                buf,
                                history_update: Some((pane_id, current_history)),
                                force_scrollback_replace,
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
                            force_scrollback_replace: false,
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
                force_scrollback_replace,
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
                                let damage = client.damage.entry(pid).or_default();
                                if force_scrollback_replace {
                                    damage.mark_full_with_scrollback_replace();
                                } else {
                                    damage.mark_full();
                                }
                                frame_pending = true;
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

        // Keep draining PTYs promptly; frame pacing must never delay replies.
        //
        // IMPORTANT: do NOT consume input_notify permits here (e.g. via
        // select! { notified => ... }) — doing so races with the PTY reader
        // and can deadlock when the reader thread is blocked on a full channel
        // while the tick loop waits for a notification that was already consumed.
        if frame_pending || cursor_held_pending {
            retry_at = Some(next_frame);
        }
        if had_pty_data {
            tokio::task::yield_now().await;
            input_notify.notify_one();
        }
    }
}

/// Build a scrollback-only FullPaneSync (rows=0, cells=[]).
/// The client appends scrollback without touching the viewport.
fn build_scrollback_only_sync(
    pane: &loom_term::pane::Pane,
    generation: u64,
    last_sent: usize,
    current_total: usize,
) -> FullPaneSync {
    let mut sync = build_scrollback_sync(pane, generation, last_sent, current_total);
    // Zero out viewport — client will skip viewport update when rows==0.
    sync.rows = 0;
    sync.cells.clear();
    sync.grapheme_extras = Default::default();
    sync.hyperlink_extras = Default::default();
    sync
}

/// Build a FullPaneSync with the right scrollback delta, handling both the
/// growing phase and ring-buffer rotation (where history_size() is capped).
///
/// - `last_sent`: the client's `scrollback_total` watermark
/// - `current_total`: the pane's current `scrollback_total()`
///
/// When `current_total < last_sent`, the server's visible primary scrollback
/// rewound (for example after a grow-resize reabsorbed rows from history into
/// the viewport). In that case we must resend the full current history with
/// `scrollback_replace = true` so the client discards stale rows.
///
/// When `delta > history_size()`, the server has rotated past what the client
/// has: send ALL current scrollback with `scrollback_replace = true`.
/// Otherwise, send only the new rows (incremental append).
fn build_scrollback_sync(
    pane: &loom_term::pane::Pane,
    generation: u64,
    last_sent: usize,
    current_total: usize,
) -> FullPaneSync {
    let hs = pane.history_size();
    let (adjusted_sent, replace) = scrollback_sync_plan(hs, last_sent, current_total);

    // Compute the `history_sent` value that snapshot_incremental expects:
    // it will send `history_size - adjusted_sent` rows from the end.
    let mut sync = pane.snapshot_incremental(generation, adjusted_sent);
    sync.scrollback_replace = replace;
    sync
}

fn scrollback_sync_plan(
    history_size: usize,
    last_sent: usize,
    current_total: usize,
) -> (usize, bool) {
    if current_total < last_sent {
        return (0, true);
    }

    let delta = current_total - last_sent;
    let rows_to_send = delta.min(history_size);
    let replace = delta > history_size;
    (history_size.saturating_sub(rows_to_send), replace)
}

#[cfg(test)]
mod tests {
    use super::{scrollback_sync_plan, wait_for_tick};
    use std::sync::Arc;
    use tokio::sync::Notify;
    use tokio::time::{Duration, Instant};

    #[cfg(unix)]
    #[tokio::test(start_paused = true)]
    async fn coalesced_frame_flushes_latest_generation_without_another_notification() {
        use super::run_tick_loop;
        use crate::daemon::{client::ClientState, server::Server, session::Session};
        use loom_protocol::codec::{self, Frame};
        use loom_term::pane::{Pane, TerminalColors};
        use std::collections::HashMap;
        use tokio::sync::{Mutex, mpsc};

        let colors = TerminalColors::default();
        let pane = Pane::new_with_opts(1, 8, 3, "/bin/sh", Some("exec cat"), None).unwrap();
        let mut session = Session::new("frame-pacing-test", "/bin/sh", 0.0, colors.clone());
        session.last_title.insert(1, String::new());
        session.panes.insert(1, pane);
        let (tx, mut rx) = mpsc::channel(16);
        let mut client = ClientState {
            id: 1,
            tx,
            damage: HashMap::new(),
            last_acked_generation: 0,
            max_input_seq: HashMap::new(),
            received_input_seq: HashMap::new(),
            history_sent: HashMap::new(),
            send_failures: 0,
            cell_width: 8.0,
            cell_height: 16.0,
            viewport_width: 64.0,
            viewport_height: 48.0,
            session_name: session.session_name.clone(),
            last_sent_cursor: HashMap::new(),
        };
        client.damage.entry(1).or_default().mark_full();
        let mut server = Server::new("/bin/sh", 0.0, colors);
        server.session_config.restore_agents = false;
        server
            .sessions
            .insert(session.session_name.clone(), session);
        server.clients.insert(1, client);
        let state = Arc::new(Mutex::new(server));
        let notify = Arc::new(Notify::new());
        let interval = Duration::from_millis(8); // honors non-default frame rate
        let task = tokio::spawn(run_tick_loop(
            state.clone(),
            Arc::new(Notify::new()),
            notify.clone(),
            interval,
        ));
        notify.notify_one();
        let first = rx.recv().await.unwrap();
        assert!(matches!(
            codec::read_frame(&mut first.as_ref()).await.unwrap(),
            Frame::FullPaneSync(_)
        ));
        let started = Instant::now();

        // Multiple updates inside one frame interval must be coalesced. No
        // PTY thread sends a later notification in this test, so only the
        // pending-frame timer can deliver the final generation.
        for generation in [98, 99] {
            {
                let mut server = state.lock().await;
                server
                    .sessions
                    .get_mut("frame-pacing-test")
                    .unwrap()
                    .generation
                    .insert(1, generation);
                server
                    .clients
                    .get_mut(&1)
                    .unwrap()
                    .damage
                    .entry(1)
                    .or_default()
                    .mark_full();
            }
            notify.notify_one();
            tokio::task::yield_now().await;
            assert!(rx.try_recv().is_err(), "sent before the frame deadline");
        }
        let final_frame = tokio::time::timeout(Duration::from_millis(30), rx.recv())
            .await
            .unwrap()
            .unwrap();
        let Frame::FullPaneSync(sync) = codec::read_frame(&mut final_frame.as_ref()).await.unwrap()
        else {
            panic!("expected coalesced full sync");
        };
        assert_eq!(sync.meta.generation, 99);
        assert!(started.elapsed() >= interval);
        assert!(started.elapsed() < Duration::from_millis(30));
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(rx.try_recv().is_err(), "idle loop kept sending frames");
        task.abort();
        let _ = task.await;
    }

    #[tokio::test(start_paused = true)]
    async fn idle_wait_has_no_periodic_wakeups() {
        let notify = Arc::new(Notify::new());
        let task_notify = notify.clone();
        let task = tokio::spawn(async move { wait_for_tick(&task_notify, None).await });
        tokio::time::advance(Duration::from_secs(60)).await;
        assert!(!task.is_finished());
        notify.notify_one();
        task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn output_interrupts_cursor_retry_without_losing_next_permit() {
        let notify = Arc::new(Notify::new());
        let started = Instant::now();
        let task_notify = notify.clone();
        let task = tokio::spawn(async move {
            wait_for_tick(&task_notify, Some(started + Duration::from_millis(16))).await;
        });
        tokio::time::advance(Duration::from_millis(5)).await;
        notify.notify_one();
        task.await.unwrap();
        assert_eq!(started.elapsed(), Duration::from_millis(5));
        // A permit queued while processing must survive to the next iteration.
        notify.notify_one();
        wait_for_tick(&notify, None).await;
        assert_eq!(started.elapsed(), Duration::from_millis(5));
    }

    #[tokio::test(start_paused = true)]
    async fn pending_cursor_is_committed_without_more_output() {
        let notify = Notify::new();
        let started = Instant::now();
        wait_for_tick(&notify, Some(started + Duration::from_millis(16))).await;
        assert_eq!(started.elapsed(), Duration::from_millis(16));
    }

    #[test]
    fn scrollback_sync_plan_rewinds_with_replace_when_total_drops() {
        assert_eq!(scrollback_sync_plan(80, 120, 95), (0, true));
    }

    #[test]
    fn scrollback_sync_plan_replaces_when_delta_exceeds_history() {
        assert_eq!(scrollback_sync_plan(80, 10, 120), (0, true));
    }

    #[test]
    fn scrollback_sync_plan_appends_tail_when_delta_fits_history() {
        assert_eq!(scrollback_sync_plan(80, 60, 75), (65, false));
    }
}
