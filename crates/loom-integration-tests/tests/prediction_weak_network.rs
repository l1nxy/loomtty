//! Deterministic reliable-link simulation. Delay/jitter hold framed server
//! updates in flight while input continues; a stall models TCP retransmission
//! (bytes are delayed, never silently dropped or reordered). No wall-clock sleeps.
use std::collections::VecDeque;

use loom_app::{grid::ClientPaneGrid, prediction::PredictionEngine};
use loom_config::PredictionMode;
use loom_protocol::{codec, message::*};

struct Link {
    frames: VecDeque<(u64, Vec<u8>)>,
    delta: bool,
}

impl Link {
    fn new() -> Self {
        Self {
            frames: VecDeque::new(),
            delta: false,
        }
    }

    fn send(&mut self, at: u64, server: &ClientPaneGrid, received: u64, echoed: u64) {
        let sync = FullPaneSync {
            meta: PaneFrameMeta {
                pane_id: 1,
                cursor_col: server.cursor_col,
                cursor_line: server.cursor_line,
                cursor_shape: server.cursor_shape,
                mode_flags: server.mode_flags,
                received_ack: received,
                echo_ack: echoed,
                ..Default::default()
            },
            cols: server.cols,
            rows: server.rows,
            title: String::new(),
            scrollback: vec![],
            scrollback_rows: 0,
            scrollback_replace: false,
            cells: server.viewport.clone(),
            grapheme_extras: GraphemeExtras::new(),
            hyperlink_extras: HyperlinkExtras::new(),
            cwd: None,
        };
        let at = at.max(self.frames.back().map_or(0, |(t, _)| *t));
        let frame = if self.delta {
            let mut frame = Vec::new();
            let regions: Vec<_> = (0..server.rows)
                .map(|row| (row, 0, server.cols - 1))
                .collect();
            codec::encode_cell_delta_streaming_framed(
                &mut frame,
                &sync.meta,
                server.cols,
                &regions,
                |row, left, right, encoder| {
                    for col in left..=right {
                        encoder.push_cell(
                            &server.viewport[row as usize * server.cols as usize + col as usize],
                        );
                    }
                },
            )
            .unwrap();
            frame
        } else {
            codec::frame_full_pane_sync(&sync).unwrap()
        };
        self.delta = !self.delta;
        self.frames.push_back((at, frame));
    }

    async fn deliver(
        &mut self,
        now: u64,
        client: &mut ClientPaneGrid,
        engine: &mut PredictionEngine,
    ) {
        while self.frames.front().is_some_and(|(t, _)| *t <= now) {
            let (_, frame) = self.frames.pop_front().unwrap();
            // Deliberately split headers and payloads through a bounded byte
            // stream, as happens under small reads / bandwidth pressure.
            use tokio::io::AsyncWriteExt;
            let (mut tx, mut rx) = tokio::io::duplex(17);
            let writer = tokio::spawn(async move {
                for chunk in frame.chunks(7) {
                    tx.write_all(chunk).await.unwrap();
                }
            });
            let decoded = codec::read_frame(&mut rx).await.unwrap();
            writer.await.unwrap();
            let meta = match decoded {
                codec::Frame::FullPaneSync(sync) => {
                    client.apply_full_sync(&sync);
                    sync.meta
                }
                codec::Frame::CellDelta(delta) => {
                    client.apply_delta_borrowed(&delta);
                    delta.meta
                }
                _ => panic!("expected pane frame"),
            };
            engine.on_server_sync(1, client, meta.received_ack, meta.echo_ack);
        }
    }
}

#[cfg(unix)]
#[path = "support/prediction_tui.rs"]
mod tui;

fn prompt() -> ClientPaneGrid {
    let mut grid = ClientPaneGrid::new(80, 24, 0);
    for (i, ch) in "$ hello".chars().enumerate() {
        grid.viewport[i].set_ch(ch);
    }
    grid.cursor_col = 7;
    grid
}

#[tokio::test]
async fn delayed_backspace_echo_survives_receipt_and_partial_updates() {
    for rtt in [50, 200, 800] {
        let mut client = prompt();
        let mut server = prompt();
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let mut link = Link::new();
        // Three edits before the first RTT; server receipt is independent of
        // shell processing. The first old frame must not cancel later edits.
        for seq in 1..=3 {
            engine.new_user_input_force_visible(1, b"\x7f", &client, seq);
        }
        assert_eq!(engine.get_overlay_cursor(1), Some((0, 4)));
        link.send(rtt, &server, 3, 0);
        server.cursor_col = 6;
        server.viewport[6].set_ch(' ');
        link.send(rtt + 40, &server, 3, 1);
        server.cursor_col = 4;
        server.viewport[4].set_ch(' ');
        server.viewport[5].set_ch(' ');
        link.send(rtt + 180, &server, 3, 3);
        for time in [rtt, rtt + 40, rtt + 179] {
            link.deliver(time, &mut client, &mut engine).await;
            assert_eq!(
                engine.get_overlay_cursor(1),
                Some((0, 4)),
                "RTT={rtt}, time={time}"
            );
            assert_eq!(engine.get_overlay_cell(1, 0, 4).unwrap().ch(), ' ');
        }
        link.deliver(rtt + 180, &mut client, &mut engine).await;
        assert!(!engine.has_overlay(1));
        assert_eq!(client.cursor_col, 4);
    }
}

#[tokio::test]
async fn delayed_left_arrow_is_not_mistaken_for_a_prompt_boundary() {
    let mut client = prompt();
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    engine.new_user_input_with_min_ack(1, b"\x1b[D", &client, 1);
    let mut link = Link::new();
    link.send(400, &client, 1, 0);
    link.deliver(400, &mut client, &mut engine).await;
    assert_eq!(engine.get_overlay_cursor(1), Some((0, 6)));
    let mut server = prompt();
    server.cursor_col = 6;
    link.send(900, &server, 1, 1);
    link.deliver(900, &mut client, &mut engine).await;
    assert!(!engine.has_overlay(1));
}

#[tokio::test]
async fn coalesced_cursor_history_keeps_unacknowledged_edits() {
    let mut client = prompt();
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    engine.new_user_input_with_min_ack(1, b"a", &client, 1);
    // An epoch boundary creates cursor history. Receipt of a later frame
    // must not treat an older cursor behind the server as a failed deletion.
    engine.new_user_input_with_min_ack(1, b"\x1b", &client, 2);
    engine.new_user_input_with_min_ack(1, b"b", &client, 3);
    engine.new_user_input_with_min_ack(1, b"c", &client, 4);
    let mut server = prompt();
    server.viewport[7].set_ch('a');
    server.viewport[8].set_ch('b');
    server.cursor_col = 9;
    let mut link = Link::new();
    link.send(600, &server, 4, 3);
    link.deliver(600, &mut client, &mut engine).await;
    assert_eq!(engine.get_overlay_cell(1, 0, 9).unwrap().ch(), 'c');
    assert_eq!(engine.get_overlay_cursor(1), Some((0, 10)));
}

// Real PTY + terminal emulator + both wire formats + client prediction.
// Wall time is used only to wait for the child; network time stays deterministic.
#[cfg(unix)]
#[tokio::test]
async fn pty_echo_converges_after_jitter_and_stall() {
    use loom_term::pane::Pane;
    use std::time::{Duration, Instant};

    async fn drain_until(pane: &mut Pane, col: u16) -> ClientPaneGrid {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            pane.process_pty_output();
            let snapshot = pane.snapshot(0);
            if snapshot.meta.cursor_col == col {
                let borrowed = codec::full_pane_sync_to_borrowed(&snapshot).unwrap();
                let mut grid = ClientPaneGrid::new(snapshot.cols, snapshot.rows, 0);
                grid.apply_full_sync(&borrowed);
                return grid;
            }
            assert!(
                Instant::now() < deadline,
                "PTY failed to reach column {col}: {:?}",
                snapshot.meta
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    // An isolated canonical line editor with kernel echo/erase. No user shell
    // startup files, persistent sessions, or changes to the user's terminal.
    let mut pane = Pane::new_with_opts(
        1,
        80,
        24,
        "/bin/sh",
        Some("stty icanon echo echoe erase '^?'; printf '$ '; exec cat >/dev/null"),
        None,
    )
    .unwrap();
    let mut server = drain_until(&mut pane, 2).await;
    let mut client = ClientPaneGrid::new(80, 24, 0);
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let mut link = Link::new();
    link.send(0, &server, 0, 0);
    link.deliver(0, &mut client, &mut engine).await;

    engine.new_user_input_track_hidden(1, b"hello", &client, 1);
    pane.write_to_pty(b"hello");
    server = drain_until(&mut pane, 7).await;
    link.send(200, &server, 1, 1);
    link.deliver(200, &mut client, &mut engine).await;
    assert!(!engine.has_overlay(1));

    for seq in 2..=4 {
        engine.new_user_input_force_visible(1, b"\x7f", &client, seq);
        pane.write_to_pty(b"\x7f");
    }
    // A receipt frame sampled before draining the PTY, then delayed delivery.
    link.send(400, &server, 4, 1);
    server = drain_until(&mut pane, 4).await;
    link.send(1600, &server, 4, 4);
    for now in [400, 800, 1599] {
        link.deliver(now, &mut client, &mut engine).await;
        assert_eq!(engine.get_overlay_cursor(1), Some((0, 4)));
        assert_eq!(engine.get_overlay_cell(1, 0, 4).unwrap().ch(), ' ');
    }
    link.deliver(1600, &mut client, &mut engine).await;
    assert!(!engine.has_overlay(1));
    assert_eq!(client.cursor_col, 4);
    assert_eq!(client.viewport, server.viewport);

    // Hold delete beyond our known local edit anchor: the prompt is retained
    // immediately, even with every subsequent server frame stalled.
    for seq in 5..=25 {
        engine.new_user_input_force_visible(1, b"\x7f", &client, seq);
    }
    assert_eq!(engine.get_overlay_cursor(1), Some((0, 2)));
    assert!(engine.get_overlay_cell(1, 0, 0).is_none());
    assert!(engine.get_overlay_cell(1, 0, 1).is_none());
    assert_eq!(
        engine.expire_predictions(engine.next_expiry().unwrap()),
        vec![1]
    );
    assert!(!engine.has_overlay(1));
}

#[tokio::test]
async fn sustained_edits_match_line_editor_under_delayed_prefix_echoes() {
    use std::borrow::Cow;
    for rtt in [50, 200, 800] {
        let mut client = ClientPaneGrid::new(240, 24, 0);
        client.viewport[0].set_ch('$');
        client.cursor_col = 2;
        let mut server = ClientPaneGrid::new(240, 24, 0);
        server.viewport[0].set_ch('$');
        server.cursor_col = 2;
        let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
        let mut link = Link::new();
        let mut line = Vec::new();
        let mut cursor = 0usize;
        let script: &[&[u8]] = &[b"a", b"b", b"\x1b[D", b"x", b"\x1b[C", b"\x7f"];
        for step in 0..120 {
            let now = step * 11;
            let seq = step + 1;
            let data = script[step as usize % script.len()];
            match data {
                b"\x7f" => engine.new_user_input_force_visible(1, data, &client, seq),
                b"\x1b[D" | b"\x1b[C" => engine.new_user_input_with_min_ack(1, data, &client, seq),
                _ => engine.new_user_input_track_hidden(1, data, &client, seq),
            }
            link.send(now + rtt, &server, seq, seq - 1);
            match data {
                b"\x7f" => {
                    cursor -= 1;
                    line.remove(cursor);
                }
                b"\x1b[D" => cursor -= 1,
                b"\x1b[C" => cursor += 1,
                _ => {
                    line.insert(cursor, data[0] as char);
                    cursor += 1;
                }
            }
            server.viewport.fill(PackedCell::default());
            server.viewport[0].set_ch('$');
            for (i, &ch) in line.iter().enumerate() {
                server.viewport[2 + i].set_ch(ch);
            }
            server.cursor_col = 2 + cursor as u16;
            // Uneven delivery spacing and a 300 ms retransmission-like stall.
            let jitter = [0, 40, 5, 300, 10, 75][step as usize % 6];
            link.send(now + rtt + 20 + jitter, &server, seq, seq);
            link.deliver(now, &mut client, &mut engine).await;
            let actual = engine.apply_overlay(1, Cow::Borrowed(&client.viewport), client.cols, 0);
            for col in 0..240 {
                assert_eq!(
                    actual[col].ch(),
                    server.viewport[col].ch(),
                    "RTT={rtt}, step={step}, col={col}"
                );
            }
            assert_eq!(
                engine
                    .get_overlay_cursor(1)
                    .unwrap_or((client.cursor_line, client.cursor_col)),
                (0, server.cursor_col),
                "RTT={rtt}, step={step}"
            );
        }
        link.deliver(u64::MAX, &mut client, &mut engine).await;
        assert!(!engine.has_overlay(1));
        assert_eq!(client.viewport, server.viewport);
        assert_eq!(client.cursor_col, server.cursor_col);
    }
}
