//! Opt-in real TUI tests: isolated Vim/Neovim buffers and an unsubmitted Codex
//! composer. External applications must be installed to run these tests.
use std::{
    borrow::Cow,
    time::{Duration, Instant},
};

use super::Link;
use loom_app::{grid::ClientPaneGrid, prediction::PredictionEngine};
use loom_config::PredictionMode;
use loom_protocol::{codec, message::*};
use loom_term::pane::Pane;

async fn wait_grid(pane: &mut Pane, ready: impl Fn(&FullPaneSync) -> bool) -> ClientPaneGrid {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let had_output = pane.process_pty_output();
        let snapshot = pane.snapshot(0);
        if had_output
            && snapshot.meta.mode_flags & MODE_SYNCHRONIZED_OUTPUT == 0
            && ready(&snapshot)
        {
            let mut grid = ClientPaneGrid::new(snapshot.cols, snapshot.rows, 0);
            grid.apply_full_sync(&codec::full_pane_sync_to_borrowed(&snapshot).unwrap());
            return grid;
        }
        assert!(
            Instant::now() < deadline,
            "TUI wait timed out: {:?}",
            snapshot.meta
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

fn predict(engine: &mut PredictionEngine, client: &ClientPaneGrid, bytes: &[u8], seq: u64) {
    // The production keyboard dispatch uses these three paths, including the
    // force-track path that bypasses new_user_input's normal alt-screen gate.
    match bytes {
        b"\x7f" => engine.new_user_input_force_visible(1, bytes, client, seq),
        _ if bytes.iter().all(|b| *b >= b' ' && *b != 0x7f) => {
            engine.new_user_input_track_hidden(1, bytes, client, seq)
        }
        _ => engine.new_user_input_with_min_ack(1, bytes, client, seq),
    }
}

fn assert_authoritative(engine: &PredictionEngine, grid: &ClientPaneGrid, context: &str) {
    let composite = engine.apply_overlay(1, Cow::Borrowed(&grid.viewport), grid.cols, 0);
    assert_eq!(
        composite.as_ref(),
        &grid.viewport,
        "{context}: prediction overwrote TUI cells"
    );
    assert!(
        engine
            .get_overlay_cursor(1)
            .is_none_or(|pos| pos == (grid.cursor_line, grid.cursor_col)),
        "{context}: speculative TUI cursor"
    );
}

fn editor(program: &str) -> (tempfile::TempDir, Pane) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("buffer.txt"), "alpha\nbeta\n").unwrap();
    let command = match program {
        "vim" => {
            "exec vim -Nu NONE -U NONE --noplugin -i NONE -n -N -X -T xterm-256color -c 'set noshowmode noruler laststatus=0 backspace=indent,eol,start' buffer.txt"
        }
        "nvim" => {
            "exec nvim -u NONE --noplugin -i NONE -n -c 'set noshowmode noruler laststatus=0 backspace=indent,eol,start' buffer.txt"
        }
        _ => unreachable!(),
    };
    let pane = Pane::new_with_opts(1, 80, 24, "/bin/sh", Some(command), Some(dir.path())).unwrap();
    (dir, pane)
}

#[tokio::test]
#[ignore = "requires Vim and Neovim; run with --ignored --nocapture"]
async fn editors_normal_commands_do_not_render_as_text_during_network_delay() {
    for program in ["vim", "nvim"] {
        for mode in [
            PredictionMode::Never,
            PredictionMode::Always,
            PredictionMode::Adaptive,
        ] {
            let (_dir, mut pane) = editor(program);
            let server = wait_grid(&mut pane, |s| {
                s.meta.mode_flags & MODE_ALT_SCREEN != 0 && s.cells[0].ch() == 'a'
            })
            .await;
            let mut client = ClientPaneGrid::new(80, 24, 0);
            let mut engine = network_engine(mode);
            let mut link = Link::new();
            link.send(0, &server, 0, 0);
            link.deliver(0, &mut client, &mut engine).await;
            assert_ne!(client.mode_flags & MODE_ALT_SCREEN, 0);

            // j is a normal-mode command; it never inserts a literal j. Simulate
            // typing it followed by Backspace before any echo reaches the client.
            predict(&mut engine, &client, b"j", 1);
            assert_authoritative(&engine, &client, "normal-mode j before echo");
            pane.write_to_pty(b"j");
            let moved = wait_grid(&mut pane, |s| s.meta.cursor_line == 1).await;
            predict(&mut engine, &client, b"\x7f", 2);
            assert_authoritative(&engine, &client, "normal-mode Backspace before echo");
            link.send(800, &moved, 1, 1);
            link.deliver(799, &mut client, &mut engine).await;
            assert_authoritative(&engine, &client, "stalled command echo");
            link.deliver(800, &mut client, &mut engine).await;
            assert_authoritative(&engine, &client, "normal-mode redraw");
        }
    }
}

fn row_text(grid: &ClientPaneGrid, row: usize) -> String {
    grid.viewport[row * grid.cols as usize..(row + 1) * grid.cols as usize]
        .iter()
        .map(|c| c.ch())
        .collect()
}

#[tokio::test]
#[ignore = "requires Vim and Neovim; run with --ignored --nocapture"]
async fn editors_insert_backspace_and_escape_under_delayed_echo() {
    for program in ["vim", "nvim"] {
        for mode in [PredictionMode::Always, PredictionMode::Adaptive] {
            for rtt in [50, 200, 800] {
                let (_dir, mut pane) = editor(program);
                let server = wait_grid(&mut pane, |s| {
                    s.meta.mode_flags & MODE_ALT_SCREEN != 0 && s.cells[0].ch() == 'a'
                })
                .await;
                let mut client = ClientPaneGrid::new(80, 24, 0);
                let mut engine = network_engine(mode);
                let mut link = Link::new();
                link.send(0, &server, 0, 0);
                link.deliver(0, &mut client, &mut engine).await;
                predict(&mut engine, &client, b"i", 1);
                pane.write_to_pty(b"i");
                predict(&mut engine, &client, b"A", 2);
                pane.write_to_pty(b"A");
                let server = wait_grid(&mut pane, |s| {
                    s.cells[0].ch() == 'A' && s.meta.cursor_col == 1
                })
                .await;
                link.send(rtt, &server, 2, 2);
                link.deliver(rtt, &mut client, &mut engine).await;
                assert_authoritative(&engine, &client, "mode-switch prefix reconciles");

                predict(&mut engine, &client, b"B", 3);
                pane.write_to_pty(b"B");
                let server = wait_grid(&mut pane, |s| {
                    s.cells[1].ch() == 'B' && s.meta.cursor_col == 2
                })
                .await;
                link.send(2 * rtt, &server, 3, 3);
                link.deliver(2 * rtt, &mut client, &mut engine).await;
                // With real evidence of insert mode, prediction must remain available.
                predict(&mut engine, &client, b"C", 4);
                assert_eq!(engine.get_overlay_cell(1, 0, 2).unwrap().ch(), 'C');
                assert_eq!(engine.get_overlay_cursor(1), Some((0, 3)));
                pane.write_to_pty(b"C");
                let with_c = wait_grid(&mut pane, |s| {
                    s.cells[2].ch() == 'C' && s.meta.cursor_col == 3
                })
                .await;
                predict(&mut engine, &client, b"\x7f", 5);
                assert_eq!(engine.get_overlay_cursor(1), Some((0, 2)));
                pane.write_to_pty(b"\x7f");
                let deleted = wait_grid(&mut pane, |s| {
                    s.cells[2].ch() == 'a' && s.meta.cursor_col == 2
                })
                .await;
                link.send(3 * rtt, &with_c, 5, 4);
                link.send(4 * rtt + 300, &deleted, 5, 5);
                link.deliver(3 * rtt, &mut client, &mut engine).await;
                assert_eq!(engine.get_overlay_cursor(1), Some((0, 2)));
                let composite = engine.apply_overlay(1, Cow::Borrowed(&client.viewport), 80, 0);
                assert_eq!(
                    composite.iter().take(7).map(|c| c.ch()).collect::<String>(),
                    "ABalpha"
                );
                link.deliver(4 * rtt + 300, &mut client, &mut engine).await;
                assert!(!engine.has_overlay(1));
                assert!(row_text(&client, 0).starts_with("ABalpha"));
                predict(&mut engine, &client, b"\x1b", 6);
                pane.write_to_pty(b"\x1b");
                predict(&mut engine, &client, b"j", 7);
                assert_authoritative(&engine, &client, "Escape ends insert-mode confidence");
                pane.write_to_pty(b"\x1b:q!\r");
                let exited =
                    wait_grid(&mut pane, |s| s.meta.mode_flags & MODE_ALT_SCREEN == 0).await;
                link.send(5 * rtt + 400, &exited, 7, 5);
                link.deliver(5 * rtt + 400, &mut client, &mut engine).await;
                assert!(
                    !engine.has_overlay(1),
                    "leaving alternate screen clears pending TUI input"
                );
            }
        }
    }
}

#[tokio::test]
#[ignore = "requires Codex CLI; set LOOM_TEST_CODEX_BIN if not on PATH"]
async fn codex_inline_typing_backspace_paste_and_resize_under_delay() {
    for mode in [
        PredictionMode::Never,
        PredictionMode::Always,
        PredictionMode::Adaptive,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let exe = std::env::var("LOOM_TEST_CODEX_BIN").unwrap_or_else(|_| "codex".to_owned());
        let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
        let cwd = std::env::var_os("LOOM_TEST_CODEX_CWD")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| dir.path().to_owned());
        let command = format!(
            "exec {} --no-daemon --no-alt-screen --sandbox read-only",
            quote(&exe)
        );
        let mut pane =
            Pane::new_with_opts(1, 100, 32, "/bin/sh", Some(&command), Some(&cwd)).unwrap();
        let server = wait_grid(&mut pane, |s| {
            let text: String = s.cells.iter().map(|c| c.ch()).collect();
            text.contains("OpenAI Codex") && text.contains("Ask Codex")
        })
        .await;
        assert_eq!(
            server.mode_flags & MODE_ALT_SCREEN,
            0,
            "must exercise inline mode"
        );
        assert_ne!(server.mode_flags & MODE_BRACKETED_PASTE, 0);
        let mut client = ClientPaneGrid::new(100, 32, 0);
        let mut engine = network_engine(mode);
        let mut link = Link::new();
        link.send(0, &server, 0, 0);
        link.deliver(0, &mut client, &mut engine).await;

        // This text is never submitted: no Enter is sent to the composer.
        let prefix = "loom-tui-check-";
        predict(&mut engine, &client, prefix.as_bytes(), 1);
        pane.write_to_pty(prefix.as_bytes());
        let server = wait_grid(&mut pane, |s| {
            s.cells
                .iter()
                .map(|c| c.ch())
                .collect::<String>()
                .contains(prefix)
        })
        .await;
        link.send(200, &server, 1, 1);
        link.deliver(200, &mut client, &mut engine).await;
        predict(&mut engine, &client, b"a", 2);
        pane.write_to_pty(b"a");
        let server = wait_grid(&mut pane, |s| {
            s.cells
                .iter()
                .map(|c| c.ch())
                .collect::<String>()
                .contains("loom-tui-check-a")
        })
        .await;
        link.send(400, &server, 2, 2);
        link.deliver(400, &mut client, &mut engine).await;
        let row = client.cursor_line;
        let col = client.cursor_col;
        predict(&mut engine, &client, b"b", 3);
        if mode == PredictionMode::Never {
            assert_eq!(engine.get_overlay_cell(1, row as u16, col), None);
            assert_eq!(engine.get_overlay_cursor(1), None);
        } else {
            assert_eq!(
                engine.get_overlay_cell(1, row as u16, col).unwrap().ch(),
                'b',
                "verified Codex composer must keep prediction"
            );
            assert_eq!(engine.get_overlay_cursor(1), Some((row, col + 1)));
        }
        pane.write_to_pty(b"b");
        let with_b = wait_grid(&mut pane, |s| {
            s.cells
                .iter()
                .map(|c| c.ch())
                .collect::<String>()
                .contains("loom-tui-check-ab")
        })
        .await;
        predict(&mut engine, &client, b"\x7f", 4);
        pane.write_to_pty(b"\x7f");
        let deleted = wait_grid(&mut pane, |s| {
            let text: String = s.cells.iter().map(|c| c.ch()).collect();
            text.contains("loom-tui-check-a") && !text.contains("loom-tui-check-ab")
        })
        .await;
        link.send(800, &with_b, 4, 3);
        link.send(1600, &deleted, 4, 4);
        link.deliver(800, &mut client, &mut engine).await;
        assert_eq!(engine.get_overlay_cursor(1), Some((row, col)));
        let composite = engine.apply_overlay(1, Cow::Borrowed(&client.viewport), client.cols, 0);
        let text: String = composite.iter().map(|c| c.ch()).collect();
        assert!(text.contains("loom-tui-check-a") && !text.contains("loom-tui-check-ab"));
        link.deliver(1600, &mut client, &mut engine).await;
        assert!(!engine.has_overlay(1));

        // Bracketed multiline paste edits the composer; it never sends a message.
        let paste = b"\x1b[200~\nsecond-line\x1b[201~";
        predict(&mut engine, &client, paste, 5);
        assert_authoritative(&engine, &client, "paste ends old edit run");
        pane.write_to_pty(paste);
        let multiline = wait_grid(&mut pane, |s| {
            s.cells
                .iter()
                .map(|c| c.ch())
                .collect::<String>()
                .contains("second-line")
        })
        .await;
        link.send(1800, &multiline, 5, 5);
        link.deliver(1800, &mut client, &mut engine).await;
        assert_authoritative(&engine, &client, "multiline composer redraw");
        predict(&mut engine, &client, b"z", 6);
        pane.resize(72, 24);
        let resized = wait_grid(&mut pane, |s| {
            s.cols == 72
                && s.rows == 24
                && s.cells
                    .iter()
                    .map(|c| c.ch())
                    .collect::<String>()
                    .contains("second-line")
        })
        .await;
        // Resize requires a full sync, as in the daemon.
        link.delta = false;
        link.send(2200, &resized, 5, 5);
        link.deliver(2200, &mut client, &mut engine).await;
        assert!(!engine.has_overlay(1));
        assert_authoritative(&engine, &client, "resize clears old composer coordinates");
        pane.write_to_pty(b"\x03\x03");
    }
}

fn network_engine(mode: PredictionMode) -> PredictionEngine {
    let mut engine = PredictionEngine::new(mode, 30, false);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64;
    engine.on_pong(0, now - 200_000);
    engine
}
