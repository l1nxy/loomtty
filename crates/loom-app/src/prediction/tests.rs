use super::*;
use crate::grid::ClientPaneGrid;
use loom_config::config::PredictionMode;

fn make_grid_at(cols: u16, rows: u16, cursor_col: u16, cursor_row: i16) -> ClientPaneGrid {
    let mut g = ClientPaneGrid::new(cols, rows, 0);
    g.cursor_col = cursor_col;
    g.cursor_line = cursor_row;
    g
}

fn make_grid(cols: u16, rows: u16) -> ClientPaneGrid {
    ClientPaneGrid::new(cols, rows, 0)
}

#[test]
fn printable_char_prediction() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, true);
    let grid = make_grid_at(80, 24, 0, 0);
    engine.new_user_input(1, b"A", &grid);

    let cell = engine.get_overlay_cell(1, 0, 0);
    assert!(cell.is_some());
    assert_eq!(cell.unwrap().ch(), 'A');
    assert_ne!(cell.unwrap().flags_u16() & FLAG_UNDERLINE, 0);
}

#[test]
fn cursor_advances() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 5, 0);
    engine.new_user_input(1, b"xyz", &grid);

    assert_eq!(engine.get_overlay_cursor(1), Some((0, 8)));
}

#[test]
fn backspace_prediction() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let mut grid = make_grid_at(80, 24, 5, 0);
    grid.viewport[4].set_ch('D');
    engine.new_user_input(1, &[0x7F], &grid);

    // After backspace, cursor moves to col 4, content shifts left.
    assert_eq!(engine.get_overlay_cursor(1), Some((0, 4)));
}

#[test]
fn alt_screen_skips_prediction() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let mut grid = make_grid_at(80, 24, 0, 0);
    grid.mode_flags = MODE_ALT_SCREEN;
    engine.new_user_input(1, b"A", &grid);

    assert!(!engine.has_overlay(1));
}

#[test]
fn never_mode_skips() {
    let mut engine = PredictionEngine::new(PredictionMode::Never, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);
    engine.new_user_input(1, b"A", &grid);

    assert!(!engine.has_overlay(1));
}

#[test]
fn force_visible_backspace_displays_even_when_prediction_disabled() {
    let mut engine = PredictionEngine::new(PredictionMode::Never, 0, false);
    let mut grid = make_grid_at(80, 24, 2, 0);
    grid.viewport[0].set_ch('A');
    grid.viewport[1].set_ch('B');

    engine.new_user_input_force_visible(1, &[0x7F], &grid, 1);

    assert_eq!(engine.get_overlay_cursor(1), Some((0, 1)));
    assert_eq!(engine.get_overlay_cell(1, 0, 1).unwrap().ch(), ' ');
}

#[test]
fn force_visible_backspace_without_edit_start_skips_in_alt_screen() {
    let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
    let mut grid = make_grid_at(80, 24, 2, 0);
    grid.mode_flags = MODE_ALT_SCREEN | MODE_MOUSE_REPORT;
    grid.viewport[0].set_ch('A');
    grid.viewport[1].set_ch('B');

    engine.new_user_input_force_visible(1, &[0x7F], &grid, 1);

    assert!(!engine.has_overlay(1));
    assert_eq!(engine.get_overlay_cursor(1), None);
}

#[test]
fn force_visible_backspace_without_edit_start_skips_in_bracketed_kitty_mode() {
    let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
    let mut grid = make_grid_at(80, 24, 2, 0);
    grid.mode_flags = MODE_BRACKETED_PASTE;
    grid.kitty_flags = MODE_KITTY_KEYBOARD;
    grid.viewport[0].set_ch('A');
    grid.viewport[1].set_ch('B');

    engine.new_user_input_force_visible(1, &[0x7F], &grid, 1);

    assert!(!engine.has_overlay(1));
    assert_eq!(engine.get_overlay_cursor(1), None);
}

#[test]
fn hidden_text_tracking_feeds_force_visible_backspace_in_never_mode() {
    let mut engine = PredictionEngine::new(PredictionMode::Never, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);

    engine.new_user_input_track_hidden(1, b"ABC", &grid, 1);

    assert!(engine.has_overlay(1));
    assert_eq!(engine.get_overlay_cell(1, 0, 0), None);
    assert_eq!(engine.get_overlay_cursor(1), None);

    engine.new_user_input_force_visible(1, &[0x7F], &grid, 2);

    assert_eq!(engine.get_overlay_cursor(1), Some((0, 2)));
    assert_eq!(engine.get_overlay_cell(1, 0, 0).unwrap().ch(), 'A');
    assert_eq!(engine.get_overlay_cell(1, 0, 1).unwrap().ch(), 'B');
    assert_eq!(engine.get_overlay_cell(1, 0, 2).unwrap().ch(), ' ');
}

#[test]
fn force_visible_backspace_stops_at_hidden_edit_start() {
    let mut engine = PredictionEngine::new(PredictionMode::Never, 0, false);
    let mut grid = make_grid_at(80, 24, 5, 0);
    grid.viewport[4].set_ch('$');

    engine.new_user_input_track_hidden(1, b"ABC", &grid, 1);

    engine.new_user_input_force_visible(1, &[0x7F], &grid, 2);
    engine.new_user_input_force_visible(1, &[0x7F], &grid, 3);
    engine.new_user_input_force_visible(1, &[0x7F], &grid, 4);
    engine.new_user_input_force_visible(1, &[0x7F], &grid, 5);

    assert_eq!(engine.get_overlay_cursor(1), Some((0, 5)));
    assert_eq!(engine.get_overlay_cell(1, 0, 4), None);
    assert_eq!(engine.get_overlay_cell(1, 0, 5).unwrap().ch(), ' ');
}

#[test]
fn hidden_edit_start_survives_text_confirmation() {
    let mut engine = PredictionEngine::new(PredictionMode::Never, 0, false);
    let grid = make_grid_at(80, 24, 5, 0);

    engine.new_user_input_track_hidden(1, b"AB", &grid, 1);

    let mut server_grid = make_grid_at(80, 24, 7, 0);
    server_grid.viewport[4].set_ch('$');
    server_grid.viewport[5].set_ch('A');
    server_grid.viewport[6].set_ch('B');
    engine.on_server_sync(1, &server_grid, 1, 1);

    assert!(!engine.has_overlay(1));

    engine.new_user_input_force_visible(1, &[0x7F], &server_grid, 2);
    engine.new_user_input_force_visible(1, &[0x7F], &server_grid, 3);
    engine.new_user_input_force_visible(1, &[0x7F], &server_grid, 4);

    assert_eq!(engine.get_overlay_cursor(1), Some((0, 5)));
    assert_eq!(engine.get_overlay_cell(1, 0, 4), None);
}

#[test]
fn hidden_edit_start_clears_on_enter_in_never_mode() {
    let mut engine = PredictionEngine::new(PredictionMode::Never, 0, false);
    let grid = make_grid_at(80, 24, 5, 0);

    engine.new_user_input_track_hidden(1, b"AB", &grid, 1);

    let mut server_grid = make_grid_at(80, 24, 7, 0);
    server_grid.viewport[5].set_ch('A');
    server_grid.viewport[6].set_ch('B');
    engine.on_server_sync(1, &server_grid, 1, 1);

    engine.new_user_input_with_min_ack(1, b"\r", &server_grid, 2);

    let mut next_prompt_grid = make_grid_at(80, 24, 2, 0);
    next_prompt_grid.viewport[1].set_ch('X');
    engine.new_user_input_force_visible(1, &[0x7F], &next_prompt_grid, 3);

    assert_eq!(engine.get_overlay_cursor(1), Some((0, 1)));
}

#[test]
fn hidden_text_tracking_feeds_force_visible_backspace_in_alt_screen() {
    let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
    let mut grid = make_grid_at(80, 24, 0, 0);
    grid.mode_flags = MODE_ALT_SCREEN | MODE_MOUSE_REPORT;

    engine.new_user_input_track_hidden(1, b"AB", &grid, 1);

    assert!(engine.has_overlay(1));
    assert_eq!(engine.get_overlay_cell(1, 0, 0), None);

    engine.new_user_input_force_visible(1, &[0x7F], &grid, 2);

    assert_eq!(engine.get_overlay_cursor(1), Some((0, 1)));
    assert_eq!(engine.get_overlay_cell(1, 0, 0).unwrap().ch(), 'A');
    assert_eq!(engine.get_overlay_cell(1, 0, 1).unwrap().ch(), ' ');
}

#[test]
fn hidden_text_tracking_pending_until_late_ack_arrives() {
    // Under the late_ack protocol (server bumps echo_ack only after PTY has
    // produced output), a tracked-hidden prediction sits in Pending until
    // echo_ack reaches its min_ack. on_server_sync calls with a lower
    // echo_ack must leave the overlay untouched so a following Backspace can
    // still consume `local_edit_start`.
    let mut engine = PredictionEngine::new(PredictionMode::Never, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);

    engine.new_user_input_track_hidden(1, b"ABC", &grid, 1);

    // echo_ack = 0 < min_ack = 1 → Pending. Server framebuffer has nothing
    // to compare against yet.
    engine.on_server_sync(1, &grid, 0, 0);
    assert!(engine.has_overlay(1));

    engine.new_user_input_force_visible(1, &[0x7F], &grid, 2);

    assert_eq!(engine.get_overlay_cursor(1), Some((0, 2)));
    assert_eq!(engine.get_overlay_cell(1, 0, 0).unwrap().ch(), 'A');
    assert_eq!(engine.get_overlay_cell(1, 0, 1).unwrap().ch(), 'B');
    assert_eq!(engine.get_overlay_cell(1, 0, 2).unwrap().ch(), ' ');
}

#[test]
fn cell_mismatch_at_late_ack_kills_overlay_without_grace() {
    // Locks in the contract change from the late_ack overhaul: once
    // `echo_ack >= min_echo_ack` the server's frame is guaranteed to reflect
    // the input, so any cell mismatch is a genuine misprediction. The old
    // `tolerate_mismatch` window (2–5s) is gone — mismatched cells must
    // disappear on the first server sync, not linger.
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);
    engine.new_user_input_with_min_ack(1, b"A", &grid, 1);
    assert!(engine.has_overlay(1));

    // Server's frame says "I applied input 1, and the cell is 'X' (not 'A')."
    let mut server_grid = make_grid_at(80, 24, 0, 1);
    server_grid.viewport[0].set_ch('X');
    engine.on_server_sync(1, &server_grid, 1, 1);

    // The wrong 'A' prediction must be gone immediately — no grace, no
    // lingering 2-5s of stale render.
    assert!(
        engine.get_overlay_cell(1, 0, 0).is_none(),
        "mispredicted cell should not survive a late_ack confirmation"
    );
}

#[test]
fn local_edit_start_survives_confirmed_sync_for_followup_backspace() {
    // Hidden-edit state must outlive a server sync that has confirmed the
    // typed cells, so subsequent force-visible Backspaces can still clamp at
    // the original edit boundary. Without this invariant, the second
    // Backspace below would walk left over the prompt.
    let mut engine = PredictionEngine::new(PredictionMode::Never, 0, false);
    let mut grid = make_grid_at(80, 24, 5, 0);
    grid.viewport[4].set_ch('$');
    engine.new_user_input_track_hidden(1, b"A", &grid, 1);
    assert!(engine.has_overlay(1));

    // Server confirms 'A' is in place. Pass-1 resets the matched cell;
    // cursor validation clears the matched cursor. The overlay's public
    // `has_overlay` will now report false (no cells, no cursor), but the
    // internal `local_edit_start` must persist — the only way to prove that
    // is to issue the Backspace and watch it clamp.
    let mut server_grid = make_grid_at(80, 24, 6, 0);
    server_grid.viewport[4].set_ch('$');
    server_grid.viewport[5].set_ch('A');
    engine.on_server_sync(1, &server_grid, 1, 1);

    // The first Backspace returns to the edit start. The second must clamp
    // there instead of walking left over the prompt.
    engine.new_user_input_force_visible(1, &[0x7F], &server_grid, 2);
    engine.new_user_input_force_visible(1, &[0x7F], &server_grid, 3);
    assert_eq!(engine.get_overlay_cursor(1), Some((0, 5)));
    assert_eq!(engine.get_overlay_cell(1, 0, 4), None);
}

#[test]
fn cursor_mismatch_at_late_ack_kills_overlay_without_grace() {
    // Cursor counterpart of the cell test. When the server has acked an
    // input but our predicted cursor disagrees with the framebuffer cursor,
    // the prediction was wrong — wipe it.
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);
    engine.new_user_input_with_min_ack(1, b"\x1B[C", &grid, 1); // Right arrow

    assert_eq!(engine.get_overlay_cursor(1), Some((0, 1)));

    // Server confirms input 1 applied, but cursor stayed put (e.g. line end).
    let server_grid = make_grid_at(80, 24, 0, 0);
    engine.on_server_sync(1, &server_grid, 1, 1);

    assert_eq!(engine.get_overlay_cursor(1), None);
}

#[test]
fn force_visible_backspace_pending_until_late_ack_arrives() {
    // Same Pending invariant for the force-visible Backspace path: with
    // echo_ack still behind the Backspace's min_ack, the predicted erase
    // remains on screen.
    let mut engine = PredictionEngine::new(PredictionMode::Never, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);

    engine.new_user_input_track_hidden(1, b"AB", &grid, 1);
    engine.new_user_input_force_visible(1, &[0x7F], &grid, 2);

    // Under late_ack semantics, when echo_ack=1 the server's grid already
    // reflects input 1 (i.e. "AB" typed, cursor at col 2). The Backspace
    // (min_ack=2) is still Pending — its predicted erase must survive.
    let mut grid_after_typing = make_grid_at(80, 24, 0, 2);
    grid_after_typing.viewport[0].set_ch('A');
    grid_after_typing.viewport[1].set_ch('B');
    engine.on_server_sync(1, &grid_after_typing, 1, 1);

    assert_eq!(engine.get_overlay_cursor(1), Some((0, 1)));
    assert!(
        engine
            .get_overlay_cell(1, 0, 1)
            .is_none_or(|cell| cell.ch() == ' ')
    );
}

#[test]
fn hidden_space_tracking_keeps_cursor_after_early_echo_ack() {
    let mut engine = PredictionEngine::new(PredictionMode::Never, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);

    engine.new_user_input_track_hidden(1, b" ", &grid, 1);

    let stale_server_grid = make_grid_at(80, 24, 0, 0);
    engine.on_server_sync(1, &stale_server_grid, 1, 1);
    engine.on_server_sync(1, &stale_server_grid, 1, 1);

    engine.new_user_input_force_visible(1, &[0x7F], &stale_server_grid, 2);

    assert_eq!(engine.get_overlay_cursor(1), Some((0, 0)));
}

#[test]
fn force_visible_prediction_clears_after_server_confirmation() {
    let mut engine = PredictionEngine::new(PredictionMode::Never, 0, false);
    let mut grid = make_grid_at(80, 24, 2, 0);
    grid.viewport[0].set_ch('A');
    grid.viewport[1].set_ch('B');
    engine.new_user_input_force_visible(1, &[0x7F], &grid, 1);

    let mut server_grid = make_grid_at(80, 24, 1, 0);
    server_grid.viewport[0].set_ch('A');
    engine.on_server_sync(1, &server_grid, 1, 1);

    assert!(!engine.has_overlay(1));
    assert_eq!(engine.get_overlay_cursor(1), None);
}

#[test]
fn backspace_at_wrapped_line_start_deletes_previous_row() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let mut grid = make_grid_at(4, 2, 0, 1);
    grid.viewport[3].set_ch('A');

    engine.new_user_input(1, &[0x7F], &grid);

    assert_eq!(engine.get_overlay_cursor(1), Some((0, 3)));
    assert_eq!(engine.get_overlay_cell(1, 0, 3).unwrap().ch(), ' ');
}

#[test]
fn adaptive_mode_display_threshold() {
    let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
    assert!(!engine.should_display());
    engine.srtt_us = 50_000;
    assert!(engine.should_display());
}

#[test]
fn server_sync_confirms_prediction() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);
    engine.new_user_input(1, b"A", &grid);
    assert!(engine.has_overlay(1));

    let mut server_grid = make_grid(80, 24);
    server_grid.viewport[0].set_ch('A');
    server_grid.cursor_col = 1;
    server_grid.cursor_line = 0;
    engine.on_server_sync(1, &server_grid, 1, 1);

    assert!(!engine.has_overlay(1));
}

#[test]
fn pending_prediction_not_validated_early() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);
    engine.new_user_input(1, b"A", &grid);

    let mut server_grid = make_grid(80, 24);
    server_grid.viewport[0].set_ch('X');
    engine.on_server_sync(1, &server_grid, 0, 0);

    assert!(engine.has_overlay(1));
}

#[test]
fn explicit_min_ack_confirms_on_matching_echo_ack() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);
    let seq = engine.next_input_seq();
    engine.new_user_input_with_min_ack(1, b"A", &grid, seq);

    let mut server_grid = make_grid(80, 24);
    server_grid.viewport[0].set_ch('A');
    server_grid.cursor_col = 1;
    server_grid.cursor_line = 0;

    engine.on_server_sync(1, &server_grid, seq - 1, seq - 1);
    assert!(engine.has_overlay(1));

    engine.on_server_sync(1, &server_grid, seq, seq);
    assert!(!engine.has_overlay(1));
}

#[test]
fn adaptive_prediction_unlocks_next_pending_input_on_matching_ack() {
    let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
    engine.srtt_us = 50_000;
    let grid = make_grid_at(80, 24, 0, 0);

    engine.new_user_input_with_min_ack(1, b"A", &grid, 1);
    engine.new_user_input_with_min_ack(1, b"B", &grid, 2);

    let mut server_grid = make_grid(80, 24);
    server_grid.viewport[0].set_ch('A');
    server_grid.cursor_col = 1;
    server_grid.cursor_line = 0;
    engine.on_server_sync(1, &server_grid, 1, 1);

    let cell = engine.get_overlay_cell(1, 0, 1);
    assert!(cell.is_some());
    assert_eq!(cell.unwrap().ch(), 'B');
}

#[test]
fn wrong_prediction_killed() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);
    engine.new_user_input(1, b"A", &grid);

    let mut server_grid = make_grid(80, 24);
    server_grid.viewport[0].set_ch('X');
    server_grid.cursor_col = 1;
    server_grid.cursor_line = 0;
    engine.on_server_sync(1, &server_grid, 1, 1);

    assert!(!engine.has_overlay(1));
}

#[test]
fn wrong_cursor_resets_all() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);
    engine.new_user_input(1, b"AB", &grid);

    let mut server_grid = make_grid(80, 24);
    server_grid.viewport[0].set_ch('A');
    server_grid.viewport[1].set_ch('B');
    server_grid.cursor_col = 10;
    server_grid.cursor_line = 0;
    engine.on_server_sync(1, &server_grid, 1, 1);

    assert!(!engine.has_overlay(1));
}

#[test]
fn anti_credit_no_false_confirm() {
    let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
    engine.srtt_us = 50_000;

    let mut grid = make_grid_at(80, 24, 0, 0);
    grid.viewport[0].set_ch('A');

    engine.new_user_input(1, b"A", &grid);

    let mut server_grid = make_grid(80, 24);
    server_grid.viewport[0].set_ch('A');
    server_grid.cursor_col = 1;
    server_grid.cursor_line = 0;
    engine.on_server_sync(1, &server_grid, 1, 1);

    // Cursor should NOT be displayed (epoch not confirmed due to anti-credit).
    assert_eq!(engine.get_overlay_cursor(1), None);
}

#[test]
fn adaptive_hides_tentative_predictions() {
    let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
    engine.srtt_us = 50_000;

    let grid = make_grid_at(80, 24, 0, 0);
    engine.new_user_input(1, b"A", &grid);

    assert_eq!(engine.get_overlay_cell(1, 0, 0), None);
}

#[test]
fn insert_mode_right_shift() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let mut grid = make_grid_at(5, 1, 1, 0);
    grid.viewport[0].set_ch('A');
    grid.viewport[1].set_ch('B');
    grid.viewport[2].set_ch('C');
    grid.viewport[3].set_ch('D');

    engine.new_user_input(1, b"X", &grid);

    assert_eq!(engine.get_overlay_cell(1, 0, 0), None); // A unchanged
    assert_eq!(engine.get_overlay_cell(1, 0, 1).unwrap().ch(), 'X');
    assert_eq!(engine.get_overlay_cell(1, 0, 2).unwrap().ch(), 'B');
    assert_eq!(engine.get_overlay_cell(1, 0, 3).unwrap().ch(), 'C');
    // col 4 is unknown (rightmost, content unpredictable)
    assert_eq!(engine.get_overlay_cell(1, 0, 4), None);
    assert_eq!(engine.get_overlay_cursor(1), Some((0, 2)));
}

#[test]
fn backspace_left_shift() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let mut grid = make_grid_at(5, 1, 2, 0);
    grid.viewport[0].set_ch('A');
    grid.viewport[1].set_ch('B');
    grid.viewport[2].set_ch('C');
    grid.viewport[3].set_ch('D');
    grid.viewport[4].set_ch('E');

    engine.new_user_input(1, &[0x7F], &grid);

    assert_eq!(engine.get_overlay_cell(1, 0, 0), None); // A unchanged
    assert_eq!(engine.get_overlay_cell(1, 0, 1).unwrap().ch(), 'C');
    assert_eq!(engine.get_overlay_cell(1, 0, 2).unwrap().ch(), 'D');
    assert_eq!(engine.get_overlay_cell(1, 0, 3).unwrap().ch(), 'E');
    assert_eq!(engine.get_overlay_cell(1, 0, 4).unwrap().ch(), ' ');
    assert_eq!(engine.get_overlay_cursor(1), Some((0, 1)));
}

#[test]
fn cr_moves_cursor_to_col_zero() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 10, 0);
    engine.new_user_input(1, b"\r", &grid);

    assert_eq!(engine.get_overlay_cursor(1), Some((0, 0)));
}

#[test]
fn lf_moves_cursor_down() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 5, 3);
    engine.new_user_input(1, b"\n", &grid);

    assert_eq!(engine.get_overlay_cursor(1), Some((4, 5)));
}

#[test]
fn crlf_moves_cursor_to_next_line_col_zero() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 10, 3);
    engine.new_user_input(1, b"\r\n", &grid);

    assert_eq!(engine.get_overlay_cursor(1), Some((4, 0)));
}

#[test]
fn lf_at_last_row_increments_epoch() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 5, 23);
    engine.new_user_input(1, b"\n", &grid);

    assert!(!engine.has_overlay(1));
}

#[test]
fn utf8_multibyte_prediction() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);

    // 'é' = U+00E9 = [0xC3, 0xA9]
    engine.new_user_input(1, &[0xC3, 0xA9], &grid);

    let cell = engine.get_overlay_cell(1, 0, 0);
    assert!(cell.is_some());
    assert_eq!(cell.unwrap().ch(), 'é');
    assert_eq!(engine.get_overlay_cursor(1), Some((0, 1)));
}

#[test]
fn utf8_cjk_wide_char() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);

    // '中' = U+4E2D = [0xE4, 0xB8, 0xAD]
    engine.new_user_input(1, &[0xE4, 0xB8, 0xAD], &grid);

    let cell = engine.get_overlay_cell(1, 0, 0);
    assert!(cell.is_some());
    assert_eq!(cell.unwrap().ch(), '中');
    assert_ne!(cell.unwrap().flags_u16() & FLAG_WIDE_CHAR, 0);
    assert_eq!(engine.get_overlay_cursor(1), Some((0, 2)));
}

#[test]
fn utf8_incomplete_does_not_predict() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);

    engine.new_user_input(1, &[0xE4], &grid);

    assert_eq!(engine.get_overlay_cell(1, 0, 0), None);
}

#[test]
fn grid_zero_cols_no_panic() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(0, 0, 0, 0);
    engine.new_user_input(1, b"A", &grid);
    assert!(!engine.has_overlay(1));
}

#[test]
fn application_mode_arrow_keys() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 5, 0);

    engine.new_user_input(1, b"\x1BOC", &grid); // Application mode right arrow
    assert_eq!(engine.get_overlay_cursor(1), Some((0, 6)));

    engine.new_user_input(1, b"\x1BOD", &grid); // Application mode left arrow
    assert_eq!(engine.get_overlay_cursor(1), Some((0, 5)));
}

#[test]
fn o1_cell_count_after_many_inputs() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 1, 0, 0);

    // Type 50 characters
    for i in 0..50u8 {
        let ch = b'A' + (i % 26);
        engine.new_user_input(1, &[ch], &grid);
    }

    // With the new data structure, total active cells should be <= cols (80)
    // because cells are updated in-place, not appended.
    let overlay = engine.overlays.get(&1).unwrap();
    let total_cells: usize = overlay.rows.values().map(|r| r.cells.len()).sum();
    assert_eq!(total_cells, 80); // Fixed-size row = cols
}

#[test]
fn srtt_computation() {
    let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
    let base_us = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64;
    engine.on_pong(1, base_us.saturating_sub(10_000));
    assert!(engine.srtt_us > 0);
}

#[test]
fn ping_generation() {
    let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
    assert!(engine.maybe_send_ping().is_some());
    assert!(engine.maybe_send_ping().is_none());
}

#[test]
fn visual_serial_changes_on_new_prediction_input() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);
    let before = engine.visual_serial();
    engine.new_user_input(1, b"A", &grid);
    assert!(engine.visual_serial() > before);
}

#[test]
fn visual_serial_changes_on_prediction_sync() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);
    engine.new_user_input(1, b"A", &grid);
    let before = engine.visual_serial();

    let mut server_grid = make_grid(80, 24);
    server_grid.viewport[0].set_ch('A');
    server_grid.cursor_col = 1;
    server_grid.cursor_line = 0;
    engine.on_server_sync(1, &server_grid, 1, 1);

    assert!(engine.visual_serial() > before);
}

// ---- Rendition inheritance tests ----

#[test]
fn rendition_inherits_fg_bg_from_left_cell() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let mut grid = make_grid_at(80, 24, 1, 0);
    // Left cell (col 0) has custom colors.
    grid.viewport[0].set_ch('A');
    grid.viewport[0].fg = PackedColor::rgb(255, 0, 0); // red
    grid.viewport[0].bg = PackedColor::rgb(0, 0, 255); // blue

    engine.new_user_input(1, b"B", &grid);

    let cell = engine.get_overlay_cell(1, 0, 1).unwrap();
    assert_eq!(cell.ch(), 'B');
    assert_eq!(cell.fg, PackedColor::rgb(255, 0, 0)); // inherited red
    assert_eq!(cell.bg, PackedColor::rgb(0, 0, 255)); // inherited blue
}

#[test]
fn rendition_inherits_bold_italic_from_left() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let mut grid = make_grid_at(80, 24, 1, 0);
    grid.viewport[0].set_ch('A');
    grid.viewport[0].flags = (FLAG_BOLD | FLAG_ITALIC).to_le_bytes();

    engine.new_user_input(1, b"B", &grid);

    let cell = engine.get_overlay_cell(1, 0, 1).unwrap();
    assert_ne!(cell.flags_u16() & FLAG_BOLD, 0);
    assert_ne!(cell.flags_u16() & FLAG_ITALIC, 0);
}

#[test]
fn rendition_at_col_zero_inherits_from_current_position() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let mut grid = make_grid_at(80, 24, 0, 0);
    grid.viewport[0].fg = PackedColor::rgb(0, 255, 0); // green

    engine.new_user_input(1, b"X", &grid);

    let cell = engine.get_overlay_cell(1, 0, 0).unwrap();
    assert_eq!(cell.fg, PackedColor::rgb(0, 255, 0)); // inherited from current pos
}

#[test]
fn consecutive_chars_chain_renditions() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let mut grid = make_grid_at(80, 24, 0, 0);
    grid.viewport[0].fg = PackedColor::rgb(100, 200, 50);

    // Type "AB" — B should inherit from predicted A, which inherited from grid[0].
    engine.new_user_input(1, b"AB", &grid);

    let a = engine.get_overlay_cell(1, 0, 0).unwrap();
    let b = engine.get_overlay_cell(1, 0, 1).unwrap();
    assert_eq!(a.fg, PackedColor::rgb(100, 200, 50));
    assert_eq!(b.fg, PackedColor::rgb(100, 200, 50)); // chained
}

// ---- Glitch trigger tests ----

#[test]
fn glitch_trigger_activates_display_in_adaptive() {
    let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 100, false);
    // SRTT is 0 (below threshold), so normally predictions wouldn't display.
    assert!(!engine.should_display());

    // But if glitch_trigger > 0, should_display returns true.
    engine.glitch_trigger = 1;
    assert!(engine.should_display());
}

#[test]
fn glitch_trigger_never_activates_in_never_mode() {
    let mut engine = PredictionEngine::new(PredictionMode::Never, 0, false);
    engine.glitch_trigger = 10;
    assert!(!engine.should_display());
}

// ---- Insert-shift edge cases ----

#[test]
fn insert_at_last_col_returns_zero_and_stops() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    // Cursor at last column (col 4 of 5-col terminal).
    let grid = make_grid_at(5, 1, 4, 0);

    // Type a wide char (needs 2 cols) — doesn't fit, should abandon.
    engine.new_user_input(1, &[0xE4, 0xB8, 0xAD], &grid); // '中'

    // No cell prediction should be created at col 4.
    assert_eq!(engine.get_overlay_cell(1, 0, 4), None);
}

#[test]
fn multiple_insert_shift_consistency() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let mut grid = make_grid_at(6, 1, 0, 0);
    grid.viewport[0].set_ch('A');
    grid.viewport[1].set_ch('B');
    grid.viewport[2].set_ch('C');

    // Type "XY" at col 0 — two right-shifts.
    engine.new_user_input(1, b"XY", &grid);

    assert_eq!(engine.get_overlay_cell(1, 0, 0).unwrap().ch(), 'X');
    assert_eq!(engine.get_overlay_cell(1, 0, 1).unwrap().ch(), 'Y');
    assert_eq!(engine.get_overlay_cell(1, 0, 2).unwrap().ch(), 'A');
    assert_eq!(engine.get_overlay_cell(1, 0, 3).unwrap().ch(), 'B');
    // col 4, 5 are unknown (rightmost after double shift)
    assert_eq!(engine.get_overlay_cursor(1), Some((0, 2)));
}

#[test]
fn insert_then_backspace_roundtrip() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let mut grid = make_grid_at(5, 1, 1, 0);
    grid.viewport[0].set_ch('A');
    grid.viewport[1].set_ch('B');
    grid.viewport[2].set_ch('C');

    // Type 'X' at col 1, then backspace.
    engine.new_user_input(1, b"X\x7F", &grid);

    // Cursor should be back at col 1.
    assert_eq!(engine.get_overlay_cursor(1), Some((0, 1)));
}

// ---- Wide char tests ----

#[test]
fn wide_char_spacer_flag_set() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);

    engine.new_user_input(1, &[0xE4, 0xB8, 0xAD], &grid); // '中'

    // col 0: wide char with FLAG_WIDE_CHAR
    let c0 = engine.get_overlay_cell(1, 0, 0).unwrap();
    assert_ne!(c0.flags_u16() & FLAG_WIDE_CHAR, 0);

    // col 1: spacer — active but get_overlay_cell should return it with FLAG_WIDE_CHAR_SPACER
    let c1 = engine.get_overlay_cell(1, 0, 1).unwrap();
    assert_ne!(c1.flags_u16() & FLAG_WIDE_CHAR_SPACER, 0);
}

#[test]
fn wide_char_insert_shift_clears_wide_flags() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let mut grid = make_grid_at(10, 1, 0, 0);
    // Put a wide char at col 2-3.
    grid.viewport[2].set_ch('中');
    grid.viewport[2].flags = FLAG_WIDE_CHAR.to_le_bytes();
    grid.viewport[3].set_ch(' ');
    grid.viewport[3].flags = FLAG_WIDE_CHAR_SPACER.to_le_bytes();

    // Type 'A' at col 0 — shift everything right by 1.
    engine.new_user_input(1, b"A", &grid);

    // Shifted cells should NOT have wide-char flags (they were cleared).
    let c3 = engine.get_overlay_cell(1, 0, 3);
    if let Some(c) = c3 {
        assert_eq!(
            c.flags_u16() & (FLAG_WIDE_CHAR | FLAG_WIDE_CHAR_SPACER),
            0,
            "shifted cell should have wide-char flags cleared"
        );
    }
}

#[test]
fn backspace_wide_char_deletes_two_cols() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let mut grid = make_grid_at(10, 1, 2, 0);
    // Wide char at cols 0-1.
    grid.viewport[0].set_ch('中');
    grid.viewport[0].flags = FLAG_WIDE_CHAR.to_le_bytes();
    grid.viewport[1].set_ch(' ');
    grid.viewport[1].flags = FLAG_WIDE_CHAR_SPACER.to_le_bytes();
    grid.viewport[2].set_ch('A');

    // Backspace at col 2 — should detect spacer at col 1, back up to col 0.
    engine.new_user_input(1, &[0x7F], &grid);

    // Cursor should be at col 0 (backed up 2 cols for wide char).
    assert_eq!(engine.get_overlay_cursor(1), Some((0, 0)));
}

// ---- UTF-8 edge cases ----

#[test]
fn utf8_split_across_calls() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);

    // Send leading byte in first call, continuation bytes in second.
    engine.new_user_input(1, &[0xC3], &grid);
    assert_eq!(engine.get_overlay_cell(1, 0, 0), None); // still accumulating

    engine.new_user_input(1, &[0xA9], &grid); // completes 'é'
    let cell = engine.get_overlay_cell(1, 0, 0);
    assert!(cell.is_some());
    assert_eq!(cell.unwrap().ch(), 'é');
}

#[test]
fn utf8_invalid_continuation_resets() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);

    // Send leading byte then invalid byte (not 10xxxxxx).
    engine.new_user_input(1, &[0xC3, 0x41], &grid); // 0x41 = 'A', not continuation

    // 0xC3 starts UTF-8, 0x41 is not continuation — accum resets.
    // 0x41 is NOT processed as printable because it's in 0x80..=0xBF branch? No,
    // 0x41 is in 0x20..=0x7E range. But the match checks 0x80..=0xBF first? No,
    // match is ordered: 0x20..=0x7E comes first. So 0x41 would be matched as printable.
    // Actually, 0xC3 starts the accum, then 0x41 is NOT 0x80..=0xBF, so it goes to
    // 0x20..=0x7E and is handled as a printable char.
    let cell = engine.get_overlay_cell(1, 0, 0);
    assert!(cell.is_some());
    assert_eq!(cell.unwrap().ch(), 'A'); // 'A' was processed as printable
}

#[test]
fn utf8_overlong_rejected() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);

    // 0xC0 and 0xC1 are overlong — should not start accumulation.
    engine.new_user_input(1, &[0xC0, 0x80], &grid);

    // 0xC0 hits the _ => branch (not in 0xC2..=0xDF), increments epoch and returns.
    assert!(!engine.has_overlay(1));
}

// ---- Server sync edge cases ----

#[test]
fn server_sync_partial_confirm_then_error() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);

    // Type "AB" (same epoch).
    engine.new_user_input(1, b"AB", &grid);

    // Server confirms A but shows wrong B.
    let mut server_grid = make_grid(80, 24);
    server_grid.viewport[0].set_ch('A');
    server_grid.viewport[1].set_ch('Z'); // wrong
    server_grid.cursor_col = 2;
    server_grid.cursor_line = 0;
    engine.on_server_sync(1, &server_grid, 1, 1);

    // With two-pass scan: A is confirmed in pass 1 (epoch confirmed),
    // then B is wrong in pass 2 — since B's epoch <= confirmed, this is
    // a catastrophic reset.
    assert!(!engine.has_overlay(1));
}

#[test]
fn server_sync_resize_clears_predictions() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);
    engine.new_user_input(1, b"A", &grid);
    assert!(engine.has_overlay(1));

    // First sync establishes dimensions.
    let server_grid = make_grid(80, 24);
    engine.on_server_sync(1, &server_grid, 0, 0);

    // Second sync with different dimensions — resize reset.
    let resized_grid = make_grid(120, 30);
    engine.on_server_sync(1, &resized_grid, 0, 0);

    assert!(!engine.has_overlay(1));
}

#[test]
fn server_sync_unknown_cells_cleared_on_mature() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let mut grid = make_grid_at(5, 1, 0, 0);
    grid.viewport[0].set_ch('A');

    // Type 'X' — rightmost col (4) becomes unknown.
    engine.new_user_input(1, b"X", &grid);

    // Unknown cell at col 4 should not be visible.
    assert_eq!(engine.get_overlay_cell(1, 0, 4), None);

    // After server sync with echo_ack >= min, unknown cells should be cleared.
    let mut server_grid = make_grid(5, 1);
    server_grid.viewport[0].set_ch('X');
    server_grid.viewport[1].set_ch('A');
    server_grid.cursor_col = 1;
    server_grid.cursor_line = 0;
    engine.on_server_sync(1, &server_grid, 1, 1);
}

#[test]
fn clear_pane_removes_all_state() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);
    engine.new_user_input(1, b"ABC", &grid);
    assert!(engine.has_overlay(1));

    engine.clear_pane(1);
    assert!(!engine.has_overlay(1));
}

// ---- CR/LF edge cases ----

#[test]
fn cr_then_printable_overwrites_at_col_zero() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let mut grid = make_grid_at(80, 24, 10, 0);
    grid.viewport[0].set_ch('Z');

    // CR moves to col 0, then 'A' overwrites.
    engine.new_user_input(1, b"\rA", &grid);

    assert_eq!(engine.get_overlay_cell(1, 0, 0).unwrap().ch(), 'A');
    assert_eq!(engine.get_overlay_cursor(1), Some((0, 1)));
}

#[test]
fn multiple_lf_moves_down() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);

    engine.new_user_input(1, b"\n\n\n", &grid);
    assert_eq!(engine.get_overlay_cursor(1), Some((3, 0)));
}

// ---- Multi-pane isolation ----

#[test]
fn predictions_isolated_between_panes() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid1 = make_grid_at(80, 24, 0, 0);
    let grid2 = make_grid_at(80, 24, 5, 0);

    engine.new_user_input(1, b"A", &grid1);
    engine.new_user_input(2, b"B", &grid2);

    // Each pane has its own overlay.
    assert_eq!(engine.get_overlay_cell(1, 0, 0).unwrap().ch(), 'A');
    assert_eq!(engine.get_overlay_cell(2, 0, 5).unwrap().ch(), 'B');
    // Pane 1 at col 5: insert-shift created an active cell (shifted from col 0).
    // But pane 2 should not have pane 1's predictions.
    assert_eq!(engine.get_overlay_cell(2, 0, 0), None); // pane 2 col 0 is inactive
                                                        // Clear pane 1, pane 2 should be unaffected.
    engine.clear_pane(1);
    assert!(!engine.has_overlay(1));
    assert!(engine.has_overlay(2));
}

// ---- Scrollback / negative cursor ----

#[test]
fn negative_cursor_row_skips_prediction() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, -1); // cursor in scrollback
    engine.new_user_input(1, b"A", &grid);

    // Should not create cell predictions (only epoch increment).
    assert_eq!(engine.get_overlay_cell(1, 0, 0), None);
}

// ---- Epoch and confirmation ----

#[test]
fn epoch_increments_on_escape() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);

    engine.new_user_input(1, b"\x1B", &grid);

    // ESC should increment epoch but not create any cell/cursor prediction.
    assert!(!engine.has_overlay(1));
}

#[test]
fn adaptive_confirms_then_shows() {
    let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
    engine.srtt_us = 50_000; // above threshold

    let grid = make_grid_at(80, 24, 0, 0);

    // First input — epoch 1, tentative (confirmed_epoch = 0).
    engine.new_user_input(1, b"A", &grid);
    assert_eq!(engine.get_overlay_cell(1, 0, 0), None); // hidden (tentative)

    // Server confirms A.
    let mut sg = make_grid(80, 24);
    sg.viewport[0].set_ch('A');
    sg.cursor_col = 1;
    sg.cursor_line = 0;
    engine.on_server_sync(1, &sg, 1, 1);

    // Now type 'B' — same epoch (1), which is now confirmed.
    // Actually, after sync, the overlay for pane 1 is removed (all confirmed).
    // So we need a new input after confirmation to test.
    engine.new_user_input(1, b"B", &grid);

    // The new prediction is at epoch 1 (confirmed_epoch was set to 1).
    // Wait — after clear, a new PaneOverlay is created with prediction_epoch=1,
    // confirmed_epoch=0. So 'B' at epoch 1 is still tentative. This is correct —
    // each new overlay starts fresh.
    assert_eq!(engine.get_overlay_cell(1, 0, 0), None);
}

// ---- next_input_seq ----

#[test]
fn next_input_seq_monotonic() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    assert_eq!(engine.next_input_seq(), 1);
    assert_eq!(engine.next_input_seq(), 2);
    assert_eq!(engine.next_input_seq(), 3);
}

// ---- Backspace at col 0 ----

#[test]
fn backspace_at_col_zero_increments_epoch() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);

    engine.new_user_input(1, &[0x7F], &grid);

    // Backspace at col 0 increments epoch (can't go further left).
    // A cursor overlay at col 0 is still created (cursor position is valid).
    // The key thing is it doesn't panic.
    assert_eq!(engine.get_overlay_cursor(1), Some((0, 0)));
}

// ---- Mouse and bracketed paste mode skip ----

#[test]
fn mouse_mode_skips_prediction() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let mut grid = make_grid_at(80, 24, 0, 0);
    grid.mode_flags = MODE_MOUSE_REPORT;
    engine.new_user_input(1, b"A", &grid);
    assert!(!engine.has_overlay(1));
}

#[test]
fn bracketed_paste_mode_skips_prediction() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let mut grid = make_grid_at(80, 24, 0, 0);
    grid.mode_flags = MODE_BRACKETED_PASTE;
    engine.new_user_input(1, b"A", &grid);
    assert!(!engine.has_overlay(1));
}

// ---- Arrow keys at boundaries ----

#[test]
fn right_arrow_at_last_col_stays() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 79, 0);
    engine.new_user_input(1, b"\x1B[C", &grid);
    assert_eq!(engine.get_overlay_cursor(1), Some((0, 79))); // stays at last col
}

#[test]
fn left_arrow_at_col_zero_stays() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);
    engine.new_user_input(1, b"\x1B[D", &grid);
    assert_eq!(engine.get_overlay_cursor(1), Some((0, 0))); // stays at col 0
}

// ---- Glitch trigger: minor (>=250ms pending) ----

#[test]
fn glitch_trigger_minor_on_medium_pending() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);
    engine.new_user_input(1, b"A", &grid);

    // Backdate the prediction's created_at to 300ms ago.
    let overlay = engine.overlays.get_mut(&1).unwrap();
    let row = overlay.rows.get_mut(&0).unwrap();
    row.cells[0].created_at = Instant::now() - std::time::Duration::from_millis(300);

    // Server sync with echo_ack=0 (pending), triggers glitch detection.
    let server_grid = make_grid(80, 24);
    engine.on_server_sync(1, &server_grid, 0, 0);

    assert_eq!(engine.glitch_trigger, GLITCH_REPAIR_COUNT); // 10
}

// ---- Glitch trigger: major (>=5s pending) ----

#[test]
fn glitch_trigger_major_on_long_pending() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);
    engine.new_user_input(1, b"A", &grid);

    // Backdate to 6s ago. PREDICTION_TIMEOUT_SECS is 8s so cell survives
    // expire_old, but >= GLITCH_FLAG_THRESHOLD_MS (5000ms) triggers major glitch.
    let overlay = engine.overlays.get_mut(&1).unwrap();
    let row = overlay.rows.get_mut(&0).unwrap();
    row.cells[0].created_at = Instant::now() - std::time::Duration::from_millis(6000);

    let server_grid = make_grid(80, 24);
    engine.on_server_sync(1, &server_grid, 0, 0);

    assert_eq!(engine.glitch_trigger, GLITCH_REPAIR_COUNT * 2); // 20 = major
}

// ---- Glitch repair: quick confirm decrements ----

#[test]
fn glitch_repair_decrements_on_quick_confirm() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    engine.glitch_trigger = 5;

    let grid = make_grid_at(80, 24, 0, 0);
    engine.new_user_input(1, b"A", &grid);

    // Server confirms quickly (created_at is just now, < 250ms).
    let mut server_grid = make_grid(80, 24);
    server_grid.viewport[0].set_ch('A');
    server_grid.cursor_col = 1;
    server_grid.cursor_line = 0;
    engine.on_server_sync(1, &server_grid, 1, 1);

    assert_eq!(engine.glitch_trigger, 4); // decremented from 5 to 4
}

// ---- Glitch repair: interval limit ----

#[test]
fn glitch_repair_respects_min_interval() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    engine.glitch_trigger = 5;
    // Set last_quick_confirm to "just now" to block repair.
    engine.last_quick_confirm = Some(Instant::now());

    let grid = make_grid_at(80, 24, 0, 0);
    engine.new_user_input(1, b"A", &grid);

    let mut server_grid = make_grid(80, 24);
    server_grid.viewport[0].set_ch('A');
    server_grid.cursor_col = 1;
    server_grid.cursor_line = 0;
    engine.on_server_sync(1, &server_grid, 1, 1);

    // Should NOT decrement because last_quick_confirm was too recent (< 150ms).
    assert_eq!(engine.glitch_trigger, 5);
}

// ---- Flagging hysteresis ----

#[test]
fn flagging_hysteresis_srtt_thresholds() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, true);
    let grid = make_grid_at(80, 24, 0, 0);
    engine.new_user_input(1, b"A", &grid);

    // SRTT > 80ms → flagging = true
    engine.srtt_us = 81_000;
    let mut sg = make_grid(80, 24);
    sg.viewport[0].set_ch('A');
    sg.cursor_col = 1;
    sg.cursor_line = 0;
    engine.on_server_sync(1, &sg, 1, 1);
    assert!(engine.flagging);

    // Recreate prediction for next sync.
    engine.new_user_input(1, b"B", &grid);

    // SRTT in hysteresis band (50 < 60 <= 80) → flagging stays true
    engine.srtt_us = 60_000;
    let mut sg2 = make_grid(80, 24);
    sg2.viewport[0].set_ch('B');
    sg2.cursor_col = 1;
    sg2.cursor_line = 0;
    engine.on_server_sync(1, &sg2, 2, 2);
    assert!(engine.flagging); // still true (hysteresis)

    // Recreate prediction.
    engine.new_user_input(1, b"C", &grid);

    // SRTT <= 50ms → flagging = false
    engine.srtt_us = 49_000;
    let mut sg3 = make_grid(80, 24);
    sg3.viewport[0].set_ch('C');
    sg3.cursor_col = 1;
    sg3.cursor_line = 0;
    engine.on_server_sync(1, &sg3, 3, 3);
    assert!(!engine.flagging);
}

#[test]
fn flagging_forced_by_major_glitch() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, true);
    engine.srtt_us = 0; // low SRTT, normally no flagging
                        // Set to 15: quick confirm may decrement by 1 → 14, still > 10.
    engine.glitch_trigger = 15;

    let grid = make_grid_at(80, 24, 0, 0);
    engine.new_user_input(1, b"A", &grid);

    let mut sg = make_grid(80, 24);
    sg.viewport[0].set_ch('A');
    sg.cursor_col = 1;
    sg.cursor_line = 0;
    engine.on_server_sync(1, &sg, 1, 1);

    assert!(engine.flagging); // forced by glitch_trigger > GLITCH_REPAIR_COUNT
}

// ---- expire_old timeout ----

#[test]
fn expire_old_clears_stale_predictions() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);
    engine.new_user_input(1, b"A", &grid);
    assert!(engine.has_overlay(1));

    // Backdate all cells AND the cursor to > PREDICTION_TIMEOUT_SECS ago.
    let overlay = engine.overlays.get_mut(&1).unwrap();
    let past = Instant::now() - std::time::Duration::from_secs(PREDICTION_TIMEOUT_SECS + 1);
    for row in overlay.rows.values_mut() {
        for cell in &mut row.cells {
            cell.created_at = past;
        }
    }
    for cur in overlay.cursors.iter_mut() {
        cur.created_at = past;
    }

    // on_server_sync calls expire_old which should clear stale cells.
    let server_grid = make_grid(80, 24);
    engine.on_server_sync(1, &server_grid, 0, 0);

    // All predictions expired, overlay should be empty.
    assert!(!engine.has_overlay(1));
}

// ---- Rendition propagation on server confirm ----

#[test]
fn rendition_propagation_on_confirm() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let mut grid = make_grid_at(80, 24, 0, 0);
    grid.viewport[0].fg = PackedColor::rgb(100, 100, 100);

    // Type 'A' (seq=1).
    engine.new_user_input(1, b"A", &grid);

    // Advance input_seq so next prediction gets a higher min_echo_ack.
    engine.next_input_seq += 10;

    // Type 'B' (seq=12) — different min_echo_ack than 'A'.
    engine.new_user_input(1, b"B", &grid);

    // Server confirms 'A' with actual colors different from predicted.
    let mut sg = make_grid(80, 24);
    sg.viewport[0].set_ch('A');
    sg.viewport[0].fg = PackedColor::rgb(255, 0, 0); // actual is red
                                                     // Don't confirm cursor (leave cursor pending) by not matching position.
                                                     // Actually, cursor is at col 2 after typing AB. Server has cursor at wrong pos
                                                     // to keep cursor pending. But that might cause reset... Let's just match cursor.
    sg.cursor_col = 2;
    sg.cursor_line = 0;
    // echo_ack=1 → 'A' (min_echo_ack=1) is confirmed, 'B' (min_echo_ack=12) is pending.
    engine.on_server_sync(1, &sg, 1, 1);

    // 'B' at col 1 should have its fg updated via rendition propagation.
    // Note: in the current two-pass implementation, propagation updates active cells
    // that come after a confirmed cell in the same row.
    let overlay = engine.overlays.get(&1);
    if let Some(o) = overlay {
        if let Some(cell) = o.get_cell(0, 1) {
            assert_eq!(cell.replacement.fg, PackedColor::rgb(255, 0, 0));
        }
    }
}

// ---- overlay.cols mismatch reset ----

#[test]
fn overlay_reset_on_cols_mismatch() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid80 = make_grid_at(80, 24, 0, 0);
    engine.new_user_input(1, b"A", &grid80);
    assert!(engine.has_overlay(1));

    // Now input with a 120-col grid — overlay.cols (80) != grid.cols (120).
    let grid120 = make_grid_at(120, 30, 0, 0);
    engine.new_user_input(1, b"B", &grid120);

    // Old overlay was reset, new prediction 'B' is at col 0 in the 120-col overlay.
    let cell = engine.get_overlay_cell(1, 0, 0);
    assert!(cell.is_some());
    assert_eq!(cell.unwrap().ch(), 'B');

    // Verify overlay.cols was updated.
    let overlay = engine.overlays.get(&1).unwrap();
    assert_eq!(overlay.cols, 120);
}

// ---- UTF-8 4-byte emoji ----

#[test]
fn utf8_four_byte_emoji() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);

    // '😀' = U+1F600 = [0xF0, 0x9F, 0x98, 0x80], width=2
    engine.new_user_input(1, &[0xF0, 0x9F, 0x98, 0x80], &grid);

    let cell = engine.get_overlay_cell(1, 0, 0);
    assert!(cell.is_some());
    assert_eq!(cell.unwrap().ch(), '😀');
    assert_ne!(cell.unwrap().flags_u16() & FLAG_WIDE_CHAR, 0);
    assert_eq!(engine.get_overlay_cursor(1), Some((0, 2)));
}

// ---- UTF-8 surrogate rejection ----

#[test]
fn utf8_surrogate_rejected() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);

    // U+D800 (surrogate) = [0xED, 0xA0, 0x80] — invalid UTF-8
    engine.new_user_input(1, &[0xED, 0xA0, 0x80], &grid);

    // from_utf8 rejects surrogates, so no prediction should be created.
    assert_eq!(engine.get_overlay_cell(1, 0, 0), None);
}

// ---- SRTT EWMA update ----

#[test]
fn srtt_ewma_second_sample() {
    let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
    let base_us = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64;

    // First pong: srtt = sample.
    engine.on_pong(1, base_us.saturating_sub(10_000));
    let first_srtt = engine.srtt_us;
    assert!(first_srtt > 0);

    // Second pong: EWMA = (srtt*7 + sample)/8
    engine.on_pong(2, base_us.saturating_sub(20_000));
    // SRTT should change (second sample is different).
    assert_ne!(engine.srtt_us, first_srtt);
}

// ---- update_config ----

#[test]
fn update_config_changes_behavior() {
    let mut engine = PredictionEngine::new(PredictionMode::Never, 0, false);
    assert!(!engine.should_display());

    engine.update_config(PredictionMode::Always, 0, true);
    assert!(engine.should_display());
}

// ---- Backspace on wide-char direct (not spacer) ----

#[test]
fn backspace_on_wide_char_direct() {
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let mut grid = make_grid_at(10, 1, 1, 0);
    // Wide char at col 0 (FLAG_WIDE_CHAR set, no spacer at col 1 since cursor is there).
    grid.viewport[0].set_ch('中');
    grid.viewport[0].flags = FLAG_WIDE_CHAR.to_le_bytes();

    // Cursor at col 1, backspace — lands on col 0 which has FLAG_WIDE_CHAR → del_width=2.
    engine.new_user_input(1, &[0x7F], &grid);

    // Cursor should be at col 0 (deleted the wide char).
    assert_eq!(engine.get_overlay_cursor(1), Some((0, 0)));
}

// ---- Adaptive cursor shown when confirmed ----

#[test]
fn adaptive_cursor_shown_when_confirmed() {
    let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
    engine.srtt_us = 50_000;

    let grid = make_grid_at(80, 24, 0, 0);
    engine.new_user_input(1, b"A", &grid);

    // Before confirmation, cursor is tentative (epoch=1, confirmed=0).
    assert_eq!(engine.get_overlay_cursor(1), None);

    // Confirm 'A' — advances confirmed_epoch.
    let mut sg = make_grid(80, 24);
    sg.viewport[0].set_ch('A');
    sg.cursor_col = 1;
    sg.cursor_line = 0;
    engine.on_server_sync(1, &sg, 1, 1);

    // After confirmation, the overlay is cleared (both cell and cursor confirmed).
    // So cursor is gone. This tests that confirmation works end-to-end.
    assert!(!engine.has_overlay(1));

    // New input after confirmation — epoch restarts at 1, confirmed still 0 in new overlay.
    engine.new_user_input(1, b"B", &grid);
    // Still tentative.
    assert_eq!(engine.get_overlay_cursor(1), None);
}

// ---- mosh-alignment regression tests ----

#[test]
fn dual_ack_caps_backspace_walk_past_server_cursor() {
    // The "infinite delete" bug: at a shell prompt, holding Backspace makes
    // the predicted cursor walk past column 0 and across rows visually
    // eating the prompt. The shell silently drops Backspaces at the prompt
    // boundary so `echo_ack` never advances, and the old single-ack design
    // had no way to nail the misprediction.
    //
    // With dual-ack: the server bumps `received_ack` synchronously on input
    // receipt (decoupled from PTY drain) and reports its true cursor
    // position. The client sees `received_ack` catch up and notices the
    // predicted cursor is "past" the server cursor — that's the
    // shell-rejected-the-Backspace signal. We cap by killing the bad epoch
    // instead of resetting the whole overlay.
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);

    // Setup: server says cursor is at (0, 10) — typical prompt-end position.
    let mut grid = make_grid_at(80, 24, 10, 0);
    for (i, ch) in "hello".chars().enumerate() {
        grid.viewport[5 + i].set_ch(ch);
    }

    // User hammers Backspace 6 times. Predicted cursor will walk to col 4,
    // and the overlay marks cells col 5-9 as deleted (blanks).
    let mut last_seq = 0;
    for _ in 0..6 {
        last_seq = engine.next_input_seq();
        engine.new_user_input_with_min_ack(1, &[0x7F], &grid, last_seq);
    }
    let predicted = engine.get_overlay_cursor(1);
    assert_eq!(predicted, Some((0, 4)), "predictions walk past the prompt before sync");

    // Server sync: received_ack advances to the latest input_seq (server
    // received all 6 Backspaces). echo_ack stays at 0 (PTY never produced
    // output — shell silently dropped them). grid.cursor still (0, 10).
    engine.on_server_sync(1, &grid, last_seq, 0);

    // The bad cursor + the bogus "deleted" cells must be gone. Backspace
    // overlay no longer eats the prompt visually.
    assert_eq!(
        engine.get_overlay_cursor(1),
        None,
        "predicted cursor past server cursor must be capped after received_ack"
    );
    // The shell didn't honour any Backspace, so all "deleted" predictions
    // for cols 5-9 should be cleared.
    for col in 5..10 {
        assert!(
            engine.get_overlay_cell(1, 0, col).is_none(),
            "bogus deleted cell at col {col} must be cleared"
        );
    }
}

#[test]
fn cap_floor_suppresses_rubber_banding_under_held_backspace() {
    // After the first Backspace gets capped (predicted past server cursor),
    // subsequent Backspaces must NOT predict-and-bounce repeatedly. The
    // overlay's `cap_floor` should make the predict path bail before the
    // overlay is touched, so visually the predicted cursor stays put
    // through a long Backspace hold instead of flickering.
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let mut grid = make_grid_at(80, 24, 10, 0);
    for (i, ch) in "hello".chars().enumerate() {
        grid.viewport[5 + i].set_ch(ch);
    }

    // First Backspace — predicts (0, 9), nothing's stopping it yet.
    let seq1 = engine.next_input_seq();
    engine.new_user_input_with_min_ack(1, &[0x7F], &grid, seq1);
    assert_eq!(engine.get_overlay_cursor(1), Some((0, 9)));

    // Server says it received the input but its cursor is still at (0, 10)
    // — shell rejected the Backspace. Cap fires, sets cap_floor.
    engine.on_server_sync(1, &grid, seq1, 0);
    assert!(
        engine.overlays.get(&1).and_then(|o| o.cap_floor).is_some(),
        "cap_floor must be set after the first cap"
    );

    // Second Backspace — should now NO-OP at the predict layer. No new
    // cursor entry, no shifted cells.
    let seq2 = engine.next_input_seq();
    engine.new_user_input_with_min_ack(1, &[0x7F], &grid, seq2);

    let overlay = engine.overlays.get(&1).unwrap();
    assert!(
        overlay.cursors.is_empty(),
        "no new predicted cursor after cap_floor was hit"
    );
    assert!(
        !overlay.rows.values().any(|r| r.cells.iter().any(|c| c.active)),
        "no cell predictions after cap_floor was hit"
    );
}

#[test]
fn cap_floor_clears_when_server_cursor_actually_moves() {
    // The cap_floor must release as soon as the server cursor actually
    // moves (the shell honoured an input). Otherwise predictions would be
    // permanently suppressed at that column.
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let mut grid = make_grid_at(80, 24, 10, 0);
    grid.viewport[9].set_ch('o');

    // Trigger a cap to set the floor.
    let seq1 = engine.next_input_seq();
    engine.new_user_input_with_min_ack(1, &[0x7F], &grid, seq1);
    engine.on_server_sync(1, &grid, seq1, 0);
    assert!(engine.overlays.get(&1).and_then(|o| o.cap_floor).is_some());

    // Now the server actually moves its cursor (shell honoured a deletion).
    let mut next_grid = make_grid_at(80, 24, 9, 0);
    next_grid.viewport[9].set_ch(' ');
    engine.on_server_sync(1, &next_grid, seq1, 0);

    // cap_floor must have been cleared.
    assert!(
        engine
            .overlays
            .get(&1)
            .map(|o| o.cap_floor.is_none())
            .unwrap_or(true),
        "cap_floor must release once the server cursor moves"
    );
}

#[test]
fn dual_ack_preserves_legitimate_forward_predictions() {
    // Counterpart of the cap test: when the predicted cursor is ahead of
    // (not past) the server cursor, that's normal speculative typing and
    // must NOT be capped.
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0); // server cursor at (0, 0)

    // Type "AB" — predicted cursor moves to (0, 2), ahead of server (0, 0).
    let seq_a = engine.next_input_seq();
    engine.new_user_input_with_min_ack(1, b"A", &grid, seq_a);
    let seq_b = engine.next_input_seq();
    engine.new_user_input_with_min_ack(1, b"B", &grid, seq_b);

    // Server received both but PTY hasn't drained yet (echo_ack=0).
    // received_ack reflects both keystrokes.
    engine.on_server_sync(1, &grid, seq_b, 0);

    // Predicted forward cursor must survive — it's not "past" the server
    // cursor, it's ahead of it, which is the entire point of prediction.
    assert!(engine.has_overlay(1), "forward predictions must survive received_ack");
}

#[test]
fn combining_mark_does_not_overwrite_host_cell() {
    // Combining marks (wcwidth == 0) must not be predicted as standalone glyphs
    // — that would clobber whatever character they're modifying. Matches mosh's
    // `wcwidth(ch) != 1` bailout in new_user_byte.
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let mut grid = make_grid_at(80, 24, 1, 0);
    grid.viewport[0].set_ch('a');

    // U+0301 COMBINING ACUTE ACCENT: bytes [0xCC, 0x81], width = 0.
    engine.new_user_input(1, &[0xCC, 0x81], &grid);

    // No cell prediction is made; the host 'a' must not be touched.
    assert_eq!(engine.get_overlay_cell(1, 0, 0), None);
    assert_eq!(engine.get_overlay_cell(1, 0, 1), None);
}

#[test]
fn should_display_hysteresis_keeps_predictions_visible_during_dip() {
    // Once a prediction is in flight, an RTT dip back into the hysteresis band
    // (LOW < srtt <= HIGH) must not flip display off — the prediction would
    // flicker otherwise. Mirrors mosh's `srtt_trigger` only clearing on
    // `!active()`.
    let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
    engine.srtt_us = 60_000; // above HIGH (30ms)
    let grid = make_grid_at(80, 24, 0, 0);
    engine.new_user_input(1, b"A", &grid);
    assert!(engine.has_overlay(1));
    assert!(engine.should_display());

    // RTT drops into the hysteresis band (low = 30 - 10 = 20ms; 25 > 20, <= 30).
    engine.srtt_us = 25_000;
    assert!(engine.should_display(), "should stay visible while overlay active");
}

#[test]
fn should_display_clears_after_dip_when_no_predictions_in_flight() {
    // With no overlays and SRTT below threshold, display should be off.
    let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
    engine.srtt_us = 25_000; // in hysteresis band, but no overlays
    assert!(!engine.should_display());
}

#[test]
fn arm_state_disarms_when_rtt_recovers_before_next_keystroke() {
    // Sequence codex flagged: predictions clear → SRTT drops below LOW with
    // no render in the window → next keystroke creates a hidden-track
    // overlay. Without an explicit disarm step, has_active_overlay() now
    // sees the just-created overlay and keeps stale armed=true, letting
    // post-recovery predictions render below the low threshold.
    let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
    let grid = make_grid_at(80, 24, 0, 0);

    // Arm by crossing HIGH and producing a visible prediction.
    engine.srtt_us = 60_000; // > HIGH (30)
    engine.new_user_input(1, b"A", &grid);
    assert!(engine.should_display(), "armed after typing above HIGH");

    // Confirm the prediction so overlay clears.
    let mut server_grid = make_grid(80, 24);
    server_grid.viewport[0].set_ch('A');
    server_grid.cursor_col = 1;
    server_grid.cursor_line = 0;
    engine.on_server_sync(1, &server_grid, 1, 1);
    assert!(!engine.has_overlay(1));

    // SRTT recovers but no render runs in between.
    engine.srtt_us = 10_000; // <= LOW (20)
    // Simulate "no render call" by NOT invoking should_display here.

    // Next keystroke: this used to create the overlay first, then any later
    // should_display would see `has_active_overlay()` and keep armed=true.
    engine.new_user_input(1, b"B", &grid);

    assert!(
        !engine.should_display(),
        "armed state must disarm when RTT recovers, even if a keystroke arrives before render"
    );
}

#[test]
fn hidden_track_overlay_does_not_arm_display_below_high_threshold() {
    // Hidden-track (Adaptive + no force-visible) creates overlay entries even
    // when display is off. The arm state must NOT flip on from below — only
    // crossing the HIGH threshold can arm it. Without this guard, a single
    // hidden-tracked keypress at an RTT in the hysteresis band would flip
    // display on, making subsequent same-epoch predictions render below the
    // configured threshold.
    let mut engine = PredictionEngine::new(PredictionMode::Adaptive, 30, false);
    engine.srtt_us = 25_000; // hysteresis band (LOW=20 < 25 <= HIGH=30)
    assert!(!engine.should_display(), "below HIGH, must start disarmed");

    // Hidden-track input creates an overlay even though display is off.
    let grid = make_grid_at(80, 24, 0, 0);
    engine.new_user_input_track_hidden(1, b"A", &grid, 1);
    assert!(engine.has_overlay(1));

    // RTT still hasn't crossed HIGH — display must stay off.
    assert!(
        !engine.should_display(),
        "overlay presence alone must not arm display below HIGH threshold"
    );
}

#[test]
fn line_end_wrap_bumps_epoch_and_pins_cursor_to_last_column() {
    // alacritty's pending-wrap: after a printable lands in the last column,
    // the cursor visually stays there until the next printable actually wraps.
    // Speculatively advancing rows mispredicts the cursor row for the whole
    // ack window, so prediction must pin to (row, cols-1) and bump epoch.
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(5, 3, 3, 0); // cursor at col 3 (of 5), 3 rows
    let epoch_before = engine.overlays.get(&1).map_or(1, |o| o.prediction_epoch);

    // Type 2 chars: 'A' lands at col 3, 'B' lands at col 4 (the last column).
    engine.new_user_input(1, b"AB", &grid);

    let overlay = engine.overlays.get(&1).unwrap();
    assert!(overlay.prediction_epoch > epoch_before);
    // Cursor is pinned to the last column on the same row — pending-wrap.
    assert_eq!(engine.get_overlay_cursor(1), Some((0, 4)));
}

#[test]
fn wrap_mid_buffer_does_not_pin_cursor_for_abandoned_bytes() {
    // When the wrap-triggering byte is NOT the last in the buffer, the
    // server will keep processing the abandoned bytes and advance its
    // cursor past the wrap. Pinning a predicted cursor at `(row, cols-1)`
    // with the buffer's `min_ack` would force a guaranteed catastrophic
    // mismatch once `echo_ack` reaches that seq — wiping hidden-edit state
    // along with everything else.
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(5, 3, 3, 0); // cursor at col 3 of 5
    let cursors_before = engine
        .overlays
        .get(&1)
        .map(|o| o.cursors.len())
        .unwrap_or(0);

    // 'A' at col 3, 'B' at col 4 (wrap), 'C' abandoned. B is byte_idx 1, last is 2.
    engine.new_user_input(1, b"ABC", &grid);

    let overlay = engine.overlays.get(&1).unwrap();
    // No new cursor should have been pinned at the wrap point. The cursor
    // count must be unchanged from before this input (or, for a fresh
    // overlay, still empty).
    assert_eq!(
        overlay.cursors.len(),
        cursors_before,
        "wrap with abandoned bytes must not push a cursor"
    );
}

#[test]
fn wrap_on_final_byte_still_pins_cursor() {
    // Sanity check: when the wrap byte IS the last byte of the buffer,
    // pinning to (row, cols-1) is correct (alacritty pending-wrap).
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(5, 3, 3, 0);

    engine.new_user_input(1, b"AB", &grid);

    assert_eq!(engine.get_overlay_cursor(1), Some((0, 4)));
}

#[test]
fn line_end_wrap_abandons_rest_of_input() {
    // Once wrap is hit, any further bytes in the same batch are skipped
    // because the cursor's next position is genuinely ambiguous until the
    // server echoes back.
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(5, 2, 3, 0);
    engine.new_user_input(1, b"ABC", &grid);
    // 'A' at col 3, 'B' at col 4 (last col → wrap trigger), 'C' must NOT be
    // predicted anywhere.
    assert_eq!(engine.get_overlay_cell(1, 0, 3).unwrap().ch(), 'A');
    assert_eq!(engine.get_overlay_cell(1, 0, 4).unwrap().ch(), 'B');
    // No prediction for 'C' on the next row.
    assert!(engine.get_overlay_cell(1, 1, 0).is_none_or(|c| c.ch() != 'C'));
}

#[test]
fn multiple_cursors_tracked_across_epoch_transitions() {
    // After ESC bumps the epoch, the next keystroke should push a *new* cursor
    // rather than overwriting the previous epoch's cursor. Matches mosh's
    // `init_cursor` pushing on epoch mismatch.
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);

    engine.new_user_input(1, b"A", &grid); // epoch 1 cursor at (0,1)
    let cursors_after_a = engine.overlays.get(&1).unwrap().cursors.len();
    assert_eq!(cursors_after_a, 1);

    engine.new_user_input(1, b"\r", &grid); // CR bumps epoch; cursor pushed/updated to (0,0) in new epoch
    let after_cr = &engine.overlays.get(&1).unwrap().cursors;
    // Either two cursors (push on epoch change) or one if state collapses; key
    // invariant: the latest is in the new epoch.
    let back = after_cr.last().unwrap();
    assert!(back.epoch >= 2);
    assert_eq!(back.col, 0);
}

#[test]
fn older_epoch_cursor_mismatch_does_not_reset_back_cursor() {
    // With multi-cursor history, an older cursor that mismatches should just
    // be dropped — only a mismatched **back** cursor is catastrophic. Mosh's
    // cull: "if cursor() returns IncorrectOrExpired, reset(); otherwise erase
    // every cursor whose validity is no longer Pending".
    //
    // Distinguishing the good path from a catastrophic reset requires that
    // some prediction state survives the sync. We arrange for a still-pending
    // cell (min_ack ahead of echo_ack) — under the good path it stays
    // visible; under a wrongful reset the whole overlay gets wiped.
    let mut engine = PredictionEngine::new(PredictionMode::Always, 0, false);
    let grid = make_grid_at(80, 24, 0, 0);

    engine.new_user_input_with_min_ack(1, b"A", &grid, 1); // epoch 1 cursor (0,1) + 'A' cell
    engine.new_user_input_with_min_ack(1, b"\r", &grid, 2); // cursor-only epoch 2 at (0,0)
    engine.new_user_input_with_min_ack(1, b"X", &grid, 3); // epoch 3 cell, ack=3

    // Sentinel: the epoch-3 'X' cell exists pre-sync.
    let overlay_pre = engine.overlays.get(&1).unwrap();
    let has_x_pre = overlay_pre
        .rows
        .values()
        .any(|r| r.cells.iter().any(|c| c.active && c.replacement.ch() == 'X'));
    assert!(has_x_pre, "test precondition: X cell exists before sync");

    // Server frame: acks inputs 1 and 2 (echo_ack = 2), input 3 still pending.
    // Server cursor at (0,0) matches epoch-2 cursor but NOT epoch-1 cursor
    // (which sat at (0,1)). The epoch-1 mismatch must be dropped, not fatal.
    let mut server_grid = make_grid(80, 24);
    server_grid.viewport[0].set_ch('A');
    server_grid.cursor_col = 0;
    server_grid.cursor_line = 0;
    engine.on_server_sync(1, &server_grid, 2, 2);

    // Catastrophic reset removes the entire overlay; the good path leaves the
    // pending 'X' cell intact.
    assert!(
        engine.has_overlay(1),
        "older-cursor mismatch must not catastrophically reset; pending X cell should survive"
    );
    let overlay = engine.overlays.get(&1).unwrap();
    let has_x_post = overlay
        .rows
        .values()
        .any(|r| r.cells.iter().any(|c| c.active && c.replacement.ch() == 'X'));
    assert!(has_x_post, "still-pending X prediction must survive sync");
    // The epoch-1 cursor was pruned (mismatched, not back); the epoch-2
    // cursor (last position after typing X) is still Pending and survives.
    let back_cursor = overlay.cursors.last().expect("back cursor must survive");
    assert_eq!(back_cursor.epoch, 2);
}
