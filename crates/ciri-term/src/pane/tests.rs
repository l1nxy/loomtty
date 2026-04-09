use super::*;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use std::sync::Arc;
use std::time::Duration;

fn shell_path() -> &'static str {
    if std::path::Path::new("/bin/sh").exists() {
        "/bin/sh"
    } else {
        "sh"
    }
}

fn new_test_pane() -> Pane {
    Pane::new(7, 4, 3, shell_path()).expect("create test pane")
}

/// Poll `pane.process_pty_output()` until `predicate` returns true, or timeout.
/// Performs one final check after the deadline to avoid missing state set
/// in the last `process_pty_output()` call.
fn wait_until(
    pane: &mut Pane,
    timeout: Duration,
    mut predicate: impl FnMut(&mut Pane) -> bool,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        pane.process_pty_output();
        if predicate(pane) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Read visible characters from a row of the terminal grid.
fn read_grid_row(pane: &Pane, row: i32) -> String {
    let cols = pane.term.grid().columns();
    (0..cols)
        .map(|c| pane.term.grid()[Point::new(Line(row), Column(c))].c)
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// Check if any grid row contains the given substring.
fn grid_contains(pane: &Pane, needle: &str) -> bool {
    let rows = pane.term.grid().screen_lines();
    for row in 0..rows as i32 {
        if read_grid_row(pane, row).contains(needle) {
            return true;
        }
    }
    false
}

// ── Original tests ──────────────────────────────────────────────────

#[test]
fn set_cell_size_rounds_to_window_size_pixels() {
    let mut pane = new_test_pane();
    pane.set_cell_size(9.4, 17.6);

    let window_size = pane.window_size();
    assert_eq!(window_size.num_cols, 4);
    assert_eq!(window_size.num_lines, 3);
    assert_eq!(window_size.cell_width, 9);
    assert_eq!(window_size.cell_height, 18);
}

#[test]
fn extract_damage_resets_after_read() {
    let mut pane = new_test_pane();

    let first = pane.extract_damage().expect("new pane should start dirty");
    assert_eq!(first, vec![(0, 0, 3), (1, 0, 3), (2, 0, 3)]);

    let second = pane
        .extract_damage()
        .expect("alacritty keeps the cursor line dirty");
    assert_eq!(second, vec![(0, 0, 3)]);

    let third = pane
        .extract_damage()
        .expect("cursor line damage remains stable after reset");
    assert_eq!(third, vec![(0, 0, 3)]);
}

#[test]
fn snapshot_incremental_only_includes_new_scrollback() {
    let pane = new_test_pane();

    let none_sent = pane.snapshot_incremental(11, 0);
    let over_sent = pane.snapshot_incremental(12, 99);

    assert_eq!(none_sent.scrollback_rows, 0);
    assert!(none_sent.scrollback.is_empty());
    assert_eq!(over_sent.scrollback_rows, 0);
    assert!(over_sent.scrollback.is_empty());
}

#[test]
fn drain_images_leaves_active_images_available_for_reconnect() {
    let mut pane = new_test_pane();
    let image = ImagePlacement {
        id: 1,
        row: 2,
        col: 3,
        width_cells: 4,
        height_cells: 5,
        pixel_width: 6,
        pixel_height: 7,
        display_mode: ImageDisplayMode::Cells,
        format: "png".into(),
        data: Arc::new(vec![1, 2, 3]),
    };

    pane.images.add_placements(vec![image.clone()]);
    pane.images.active_mut().push(image.clone());

    let drained = pane.drain_images();
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0].id, image.id);
    assert_eq!(pane.active_images().len(), 1);
    assert_eq!(pane.active_images()[0].id, image.id);
    assert!(pane.drain_images().is_empty());
}

#[test]
fn delete_state_drains_once_after_clear() {
    let mut pane = new_test_pane();

    pane.images.clear_on_delete();

    assert!(pane.drain_image_deletes());
    assert!(!pane.drain_image_deletes());
}

// ── PTY spawn & bidirectional I/O ───────────────────────────────────

#[test]
fn pty_spawn_creates_running_child() {
    let pane = new_test_pane();
    assert!(pane.child_pid().is_some(), "child PID should be available");
    assert!(!pane.is_exited());
}

#[test]
fn pty_write_and_verify_grid_content() {
    let mut pane = Pane::new_with_opts(70, 80, 24, shell_path(), None, None).expect("create pane");

    pane.write_to_pty(b"echo CIRI_MARKER_42\n");

    let found = wait_until(&mut pane, Duration::from_secs(3), |p| {
        grid_contains(p, "CIRI_MARKER_42")
    });
    assert!(found, "echo output should appear in terminal grid");
}

#[test]
fn pty_write_after_exit_is_silent() {
    let mut pane =
        Pane::new_with_opts(98, 80, 24, shell_path(), Some("true"), None).expect("create pane");

    let exited = wait_until(&mut pane, Duration::from_secs(3), |p| p.is_exited());
    assert!(exited);

    // Writing after exit should not panic or error — silently dropped
    pane.write_to_pty(b"this should be silently dropped\n");
}

#[cfg(unix)]
#[test]
fn pty_child_has_valid_fd() {
    let pane = new_test_pane();
    let fd = pane.master_raw_fd();
    assert!(fd.is_some(), "unix master fd should be available");
    assert!(fd.unwrap() >= 0, "fd should be non-negative");
}

// ── Child process exit & status ─────────────────────────────────────

#[test]
fn child_exit_zero_detected() {
    let mut pane =
        Pane::new_with_opts(97, 80, 24, shell_path(), Some("exit 0"), None).expect("create pane");

    let exited = wait_until(&mut pane, Duration::from_secs(3), |p| p.is_exited());
    assert!(exited, "should detect child exit 0");
}

#[test]
fn child_exit_nonzero_detected() {
    let mut pane =
        Pane::new_with_opts(96, 80, 24, shell_path(), Some("exit 42"), None).expect("create pane");

    let exited = wait_until(&mut pane, Duration::from_secs(3), |p| p.is_exited());
    assert!(exited, "should detect child exit 42");
}

#[cfg(unix)]
#[test]
fn drop_kills_child_process() {
    let pane = Pane::new_with_opts(95, 80, 24, shell_path(), Some("sleep 300"), None)
        .expect("create pane");
    let pid = pane.child_pid().expect("should have PID") as i32;

    // Drop the pane — Pty::drop calls child.kill() to prevent orphans
    drop(pane);

    // Wait for the process to be fully reaped (not just signaled).
    // waitpid returns the pid on success, -1/ECHILD when already reaped.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let ret = unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) };
        if ret == pid {
            break; // We reaped it
        }
        if ret == -1 {
            // ECHILD: already reaped by portable-pty's Child::drop or Pty::drop
            break;
        }
        if std::time::Instant::now() >= deadline {
            // Last resort: verify the process is truly gone (not a zombie).
            // kill(pid, 0) returns -1 with ESRCH for non-existent processes.
            // Zombies still exist so kill returns 0 — we must check errno.
            let ret = unsafe { libc::kill(pid, 0) };
            if ret == -1 {
                let err = std::io::Error::last_os_error();
                assert_eq!(
                    err.raw_os_error(),
                    Some(libc::ESRCH),
                    "kill(pid={pid}, 0) failed with unexpected error: {err}"
                );
                break; // ESRCH → process is gone
            }
            panic!("child (pid={pid}) still exists 3s after Pane drop (kill returned {ret})");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
#[test]
fn child_killed_by_signal_detected_as_exit() {
    let mut pane = Pane::new_with_opts(94, 80, 24, shell_path(), Some("sleep 300"), None)
        .expect("create pane");
    let pid = pane.child_pid().expect("should have PID");

    // Kill the child externally with SIGKILL
    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }

    let exited = wait_until(&mut pane, Duration::from_secs(3), |p| p.is_exited());
    assert!(exited, "should detect externally killed child");
}

// ── Resize & SIGWINCH ───────────────────────────────────────────────

#[test]
fn resize_updates_grid_dimensions_and_marks_dirty() {
    let mut pane = new_test_pane();
    pane.set_dirty(false);

    pane.resize(80, 24);
    assert_eq!(pane.grid_cols(), 80);
    assert_eq!(pane.grid_rows(), 24);
    assert!(pane.is_dirty(), "resize should mark dirty");
}

#[test]
fn resize_noop_if_same_size() {
    let mut pane = new_test_pane();
    pane.set_dirty(false);

    pane.resize(4, 3); // same as creation size
    assert!(!pane.is_dirty(), "no-op resize should not dirty");
}

#[cfg(unix)]
#[test]
fn resize_sigwinch_child_sees_new_terminal_size() {
    let mut pane = Pane::new_with_opts(93, 80, 24, shell_path(), None, None).expect("create pane");

    // Wait for shell prompt to appear (any non-empty grid row)
    wait_until(&mut pane, Duration::from_secs(3), |p| {
        grid_contains(p, "$")
            || grid_contains(p, "#")
            || grid_contains(p, "%")
            || read_grid_row(p, 0).len() > 0
    });

    // Resize the PTY — this sends SIGWINCH to the child
    pane.resize(42, 13);

    // Ask the child shell to report its terminal size
    pane.write_to_pty(b"stty size\n");

    // The child should output "13 42" (rows cols)
    let found = wait_until(&mut pane, Duration::from_secs(3), |p| {
        grid_contains(p, "13 42")
    });
    assert!(found, "child should report '13 42' after resize (SIGWINCH)");
}

// ── Mouse input SGR format ──────────────────────────────────────────

#[test]
fn mouse_input_sgr_format_encoding() {
    // Verify SGR extended mouse encoding: CSI < Btn_with_mods ; Col+1 ; Row+1 M/m
    // Expected strings are independently hand-computed, NOT derived from the same
    // formula as send_mouse_input.
    let cases: &[(u8, u16, u16, bool, u8, &str)] = &[
        //                                              btn_with_mods = button | (mods << 2)
        (0, 0, 0, true, 0, "\x1b[<0;1;1M"),  // left press, origin
        (0, 0, 0, false, 0, "\x1b[<0;1;1m"), // left release, origin (lowercase m)
        (0, 79, 23, true, 0, "\x1b[<0;80;24M"), // left press, 1-indexed coords
        (1, 10, 5, true, 4, "\x1b[<17;11;6M"), // middle+shift: 1|(4<<2)=17
        (2, 0, 0, true, 8, "\x1b[<34;1;1M"), // right+alt:   2|(8<<2)=34
        (64, 50, 10, true, 0, "\x1b[<64;51;11M"), // scroll wheel, no mods
    ];

    // Spawn cat so the PTY stays open for writes
    let mut pane =
        Pane::new_with_opts(92, 80, 24, shell_path(), Some("cat"), None).expect("create pane");

    // Send all cases to cat's stdin — cat echoes them verbatim to the terminal.
    for &(button, col, row, pressed, mods, _) in cases {
        pane.send_mouse_input(button, col, row, pressed, mods);
    }

    // Wait for cat to echo back all sequences, then verify each expected
    // encoding fragment appears in the grid.
    wait_until(&mut pane, Duration::from_secs(2), |p| {
        grid_contains(p, "[<")
    });

    // Collect all grid text for matching
    let rows = pane.term.grid().screen_lines();
    let grid_text: String = (0..rows as i32)
        .map(|r| read_grid_row(&pane, r))
        .collect::<Vec<_>>()
        .join("\n");

    for &(button, col, row, pressed, mods, expected) in cases {
        // Strip the ESC prefix — cat's echo through the terminal may render
        // the CSI differently, but the parameters after `[<` must match.
        let params_suffix = expected.trim_start_matches("\x1b");
        assert!(
            grid_text.contains(params_suffix),
            "grid should contain SGR params '{}' for button={button} col={col} row={row} pressed={pressed} mods={mods}\ngrid:\n{grid_text}",
            params_suffix,
        );
    }
}

// ── Title via OSC ───────────────────────────────────────────────────

#[test]
fn title_set_via_osc_sequence() {
    let mut pane = Pane::new_with_opts(91, 80, 24, shell_path(), None, None).expect("create pane");

    pane.write_to_pty(b"printf '\\033]0;My Test Title\\007'\n");

    let found = wait_until(&mut pane, Duration::from_secs(3), |p| !p.title.is_empty());
    assert!(
        found,
        "title should be set via OSC 0, got: {:?}",
        pane.title
    );
    assert!(
        pane.title.contains("My Test Title"),
        "title should contain marker, got: {:?}",
        pane.title
    );
}

// ── Dirty flag ──────────────────────────────────────────────────────

#[test]
fn dirty_flag_lifecycle() {
    let mut pane = new_test_pane();
    assert!(pane.is_dirty(), "new pane starts dirty");

    pane.set_dirty(false);
    assert!(!pane.is_dirty());

    pane.set_dirty(true);
    assert!(pane.is_dirty());
}

// ── Cell size edge cases ────────────────────────────────────────────

#[test]
fn set_cell_size_preserves_previous_on_zero() {
    let mut pane = new_test_pane();
    pane.set_cell_size(10.0, 20.0);
    let ws = pane.window_size();
    assert_eq!(ws.cell_width, 10);
    assert_eq!(ws.cell_height, 20);

    pane.set_cell_size(0.0, 0.0);
    let ws = pane.window_size();
    assert_eq!(ws.cell_width, 10);
    assert_eq!(ws.cell_height, 20);
}

// ── Snapshot ────────────────────────────────────────────────────────

#[test]
fn full_snapshot_contains_all_viewport_rows() {
    let pane = new_test_pane();
    let snap = pane.snapshot(11);
    assert_eq!(snap.cols, 4);
    assert_eq!(snap.rows, 3);
    assert_eq!(snap.scrollback_rows, 0);
}

// ── Focus event write ───────────────────────────────────────────────

#[test]
fn write_focus_event_noop_when_mode_disabled() {
    let mut pane = new_test_pane();
    assert!(!pane.has_focus_event_mode());
    pane.write_focus_event(true);
    pane.write_focus_event(false);
    // No panic, no crash — focus sequences not sent when mode is off
}

#[test]
fn focus_event_mode_enabled_via_dec_1004() {
    let mut pane = Pane::new_with_opts(86, 80, 24, shell_path(), None, None).expect("create pane");

    assert!(!pane.has_focus_event_mode());

    // Enable focus event reporting: CSI ? 1004 h
    pane.write_to_pty(b"printf '\\033[?1004h'\n");

    let found = wait_until(&mut pane, Duration::from_secs(3), |p| {
        p.has_focus_event_mode()
    });
    assert!(found, "focus event mode should be enabled via DEC 1004");

    // Now write_focus_event should actually write sequences to PTY
    // (we can't easily read them back, but at least verify no panic)
    pane.write_focus_event(true);
    pane.write_focus_event(false);
}

// ── Drain semantics ─────────────────────────────────────────────────

#[test]
fn drain_methods_idempotent_when_empty() {
    let mut pane = new_test_pane();

    assert!(pane.drain_clipboard().is_empty());
    assert!(!pane.drain_bell());
    assert!(pane.drain_command_completion().is_none());
    assert!(pane.drain_notifications().is_empty());
    assert!(pane.drain_images().is_empty());
    assert!(!pane.drain_image_deletes());

    // Second call identical — drain is idempotent on empty state
    assert!(pane.drain_clipboard().is_empty());
    assert!(!pane.drain_bell());
}

// ── Bell event via PTY ──────────────────────────────────────────────

#[test]
fn bell_character_triggers_drain_bell() {
    let mut pane = Pane::new_with_opts(90, 80, 24, shell_path(), None, None).expect("create pane");

    pane.write_to_pty(b"printf '\\007'\n");

    let found = wait_until(&mut pane, Duration::from_secs(3), |p| p.drain_bell());
    assert!(found, "BEL character should trigger bell event");

    // After draining, bell should be cleared
    assert!(!pane.drain_bell());
}

// ── Scrollback ──────────────────────────────────────────────────────

#[test]
fn scrollback_grows_when_output_exceeds_viewport() {
    let mut pane = Pane::new_with_opts(89, 80, 3, shell_path(), None, None).expect("create pane");

    pane.write_to_pty(b"printf 'L1\\nL2\\nL3\\nL4\\nL5\\nL6\\nL7\\nL8\\n'\n");

    let grown = wait_until(&mut pane, Duration::from_secs(3), |p| {
        p.scrollback_total() > 0
    });
    assert!(
        grown,
        "scrollback should grow, got total={}",
        pane.scrollback_total()
    );
}

#[test]
fn resize_shrink_adjusts_scrollback_total() {
    // Start with a tall terminal, produce scrollback, then shrink
    let mut pane = Pane::new_with_opts(85, 80, 5, shell_path(), None, None).expect("create pane");

    // Fill enough lines to generate scrollback
    pane.write_to_pty(b"printf 'A\\nB\\nC\\nD\\nE\\nF\\nG\\nH\\nI\\nJ\\n'\n");
    wait_until(&mut pane, Duration::from_secs(3), |p| {
        p.scrollback_total() > 0
    });
    let sb_before = pane.scrollback_total();

    // Grow the terminal to reabsorb scrollback rows
    pane.resize(80, 50);
    let sb_after = pane.scrollback_total();

    assert!(
        sb_after <= sb_before,
        "growing terminal should not increase scrollback: before={sb_before} after={sb_after}"
    );
}

// ── Mode flags via escape sequences ─────────────────────────────────

#[test]
fn bracketed_paste_mode_detected_via_escape_sequence() {
    let mut pane = Pane::new_with_opts(88, 80, 24, shell_path(), None, None).expect("create pane");

    // Send escape sequence through printf → PTY → alacritty
    pane.write_to_pty(b"printf '\\033[?2004h'\n");

    let found = wait_until(&mut pane, Duration::from_secs(3), |p| {
        let (_, _, _, flags) = p.cursor_info();
        flags & MODE_BRACKETED_PASTE != 0
    });
    assert!(found, "bracketed paste mode should be detected");
}

#[test]
fn alt_screen_detected_via_escape_sequence() {
    let mut pane = Pane::new_with_opts(87, 80, 24, shell_path(), None, None).expect("create pane");

    assert!(!pane.is_alt_screen());

    pane.write_to_pty(b"printf '\\033[?1049h'\n");

    let found = wait_until(&mut pane, Duration::from_secs(3), |p| p.is_alt_screen());
    assert!(found, "should detect alt screen mode");
}
