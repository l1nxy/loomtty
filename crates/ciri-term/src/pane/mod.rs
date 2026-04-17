pub mod cell;
pub mod colors;
mod notify;
mod snapshot;
pub mod types;

pub use cell::{pack_cell, pack_color};
pub use colors::TerminalColors;
pub use types::{ImagePlacement, PaneId, SemanticZone, ShellState};

use alacritty_terminal::event::{Event, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::Config as TermConfig;
use alacritty_terminal::term::Term;
use alacritty_terminal::term::color::COUNT as COLOR_COUNT;
use alacritty_terminal::vte::ansi::{CursorShape, Handler, Processor};
use anyhow::Result;
use ciri_protocol::message::*;
use std::sync::mpsc;

use crate::event::PtyEventListener;
use crate::image_store::ImageStore;
use crate::parser_suite::ParserSuite;
use crate::pending_events::PendingEvents;
use crate::pty::Pty;
use cell::round_cell_size;
use colors::default_color;

struct TermSize {
    cols: usize,
    rows: usize,
}

const DEFAULT_CELL_WIDTH: u16 = 8;
const DEFAULT_CELL_HEIGHT: u16 = 16;

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

fn cursor_shape_to_u8(shape: CursorShape) -> u8 {
    match shape {
        CursorShape::Block => CURSOR_BLOCK,
        CursorShape::Underline => CURSOR_UNDERLINE,
        CursorShape::Beam => CURSOR_BEAM,
        CursorShape::HollowBlock => CURSOR_HOLLOW_BLOCK,
        CursorShape::Hidden => CURSOR_HIDDEN,
    }
}

#[derive(Debug, Clone, Copy)]
struct SnapshotScrollback {
    rows: usize,
}

impl SnapshotScrollback {
    fn full(history_size: usize) -> Self {
        Self { rows: history_size }
    }

    fn incremental(current_history: usize, history_sent: usize) -> Self {
        let retained_history = history_sent.min(current_history);
        Self {
            rows: current_history - retained_history,
        }
    }
}

pub struct Pane {
    pub id: PaneId,
    pub title: String,
    pub shell_state: ShellState,

    term: Term<PtyEventListener>,
    processor: Processor,
    event_rx: mpsc::Receiver<Event>,
    pty: Pty,

    cols: u16,
    rows: u16,
    cell_width: u16,
    cell_height: u16,

    dirty: bool,
    exited: bool,

    parsers: ParserSuite,
    images: ImageStore,
    events: PendingEvents,
    notifications_pending: Vec<(String, String)>,
    last_notification_time: Option<std::time::Instant>,
    password_input: bool,
}

impl Pane {
    pub fn new(id: PaneId, cols: u16, rows: u16, shell: &str) -> Result<Self> {
        Self::new_with_opts(id, cols, rows, shell, None, None)
    }

    pub fn new_with_cwd(
        id: PaneId,
        cols: u16,
        rows: u16,
        shell: &str,
        cwd: Option<&std::path::Path>,
    ) -> Result<Self> {
        Self::new_with_opts(id, cols, rows, shell, None, cwd)
    }

    pub fn new_with_opts(
        id: PaneId,
        cols: u16,
        rows: u16,
        shell: &str,
        command: Option<&str>,
        cwd: Option<&std::path::Path>,
    ) -> Result<Self> {
        Self::new_with_notify(id, cols, rows, shell, command, cwd, None)
    }

    pub fn new_with_notify(
        id: PaneId,
        cols: u16,
        rows: u16,
        shell: &str,
        command: Option<&str>,
        cwd: Option<&std::path::Path>,
        output_notify: Option<crate::pty::PtyOutputNotify>,
    ) -> Result<Self> {
        let pty = match output_notify {
            Some(notify) => Pty::spawn_with_notify(cols, rows, shell, command, cwd, notify)?,
            None => Pty::spawn_with_opts(cols, rows, shell, command, cwd)?,
        };
        let size = TermSize {
            cols: cols as usize,
            rows: rows as usize,
        };
        let config = TermConfig {
            kitty_keyboard: true,
            ..TermConfig::default()
        };
        let (event_listener, event_rx) = PtyEventListener::new();
        let term = Term::new(config, &size, event_listener);

        Ok(Pane {
            id,
            title: String::new(),
            notifications_pending: Vec::new(),
            last_notification_time: None,
            password_input: false,
            shell_state: ShellState {
                zone: SemanticZone::Prompt,
                last_exit_code: None,
                prompt_line: None,
                output_line: None,
                command_start: None,
            },
            term,
            processor: Processor::new(),
            event_rx,
            pty,
            cols,
            rows,
            cell_width: DEFAULT_CELL_WIDTH,
            cell_height: DEFAULT_CELL_HEIGHT,
            dirty: true,
            exited: false,
            parsers: ParserSuite::new(),
            images: ImageStore::new(),
            events: PendingEvents::new(),
        })
    }

    pub fn init_colors(&mut self, colors: &TerminalColors) {
        use alacritty_terminal::vte::ansi::NamedColor;
        for (i, &color) in colors.ansi.iter().enumerate() {
            self.term.set_color(i, color);
        }
        self.term
            .set_color(NamedColor::Foreground as usize, colors.foreground);
        self.term
            .set_color(NamedColor::Background as usize, colors.background);
        self.term
            .set_color(NamedColor::Cursor as usize, colors.cursor);
    }

    // ── PTY I/O ──────────────────────────────────────────────────────

    pub fn process_pty_output(&mut self) -> bool {
        if self.exited {
            return false;
        }

        let data_processed = self.drain_and_parse_pty();
        self.process_terminal_events();
        self.check_pty_exit();

        if data_processed {
            self.dirty = true;
        }

        let pw = self.pty.is_password_input();
        if pw != self.password_input {
            self.password_input = pw;
            self.dirty = true;
        }

        data_processed
    }

    fn drain_and_parse_pty(&mut self) -> bool {
        let chunks = self.pty.drain_output();
        if chunks.is_empty() {
            return false;
        }

        for chunk in &chunks {
            self.parsers.scan_control(
                chunk,
                &mut self.shell_state,
                &mut self.events.command_completion,
            );
            self.scan_osc_notifications(chunk);
        }

        let cursors: Vec<(u16, u16)> = chunks
            .iter()
            .map(|chunk| {
                self.processor.advance(&mut self.term, chunk);
                let cursor = self.term.grid().cursor.point;
                (cursor.column.0 as u16, cursor.line.0.max(0) as u16)
            })
            .collect();

        for (chunk, &(cursor_col, cursor_row)) in chunks.iter().zip(cursors.iter()) {
            let (kitty_result, sixel_placements) =
                self.parsers
                    .scan_images(chunk, cursor_col, cursor_row, self.images.active_mut());
            if kitty_result.deleted {
                self.images.clear_on_delete();
            }
            self.images.add_placements(kitty_result.placements);
            self.images.add_placements(sixel_placements);
        }

        self.images.cap_active();
        true
    }

    fn process_terminal_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                Event::PtyWrite(text) => self.write_to_pty(text.as_bytes()),
                Event::Title(t) => self.title = t,
                Event::ResetTitle => self.title.clear(),
                Event::ClipboardStore(_, text) => self.events.clipboard.push(text),
                Event::ClipboardLoad(_, formatter) => {
                    let response = formatter("");
                    self.write_to_pty(response.as_bytes());
                }
                Event::TextAreaSizeRequest(formatter) => {
                    let response = formatter(self.window_size());
                    self.write_to_pty(response.as_bytes());
                }
                Event::ColorRequest(index, formatter) => {
                    if index < COLOR_COUNT {
                        let color =
                            self.term.colors()[index].unwrap_or_else(|| default_color(index));
                        let response = formatter(color);
                        self.write_to_pty(response.as_bytes());
                    }
                }
                Event::Bell => self.events.bell = true,
                Event::Exit | Event::ChildExit(_) => self.exited = true,
                _ => {}
            }
        }
    }

    fn check_pty_exit(&mut self) {
        if !self.exited && self.pty.reader_eof() {
            self.exited = true;
        }
        if !self.exited && self.pty.try_wait() {
            self.exited = true;
        }
    }

    // ── Drain methods ────────────────────────────────────────────────

    pub fn drain_clipboard(&mut self) -> Vec<String> {
        self.events.drain_clipboard()
    }

    pub fn drain_bell(&mut self) -> bool {
        self.events.drain_bell()
    }

    pub fn drain_command_completion(&mut self) -> Option<std::time::Duration> {
        self.events.drain_command_completion()
    }

    pub fn drain_notifications(&mut self) -> Vec<(String, String)> {
        let mut all = std::mem::take(&mut self.notifications_pending);
        all.truncate(5);
        all
    }

    pub fn drain_images(&mut self) -> Vec<ImagePlacement> {
        self.images.drain_pending()
    }

    pub fn drain_image_deletes(&mut self) -> bool {
        self.images.drain_deleted()
    }

    pub fn test_mark_image_deleted(&mut self) {
        self.images.clear_on_delete();
    }

    pub fn test_add_active_image(&mut self, image: ImagePlacement) {
        self.images.active_mut().push(image);
    }

    pub fn active_images(&self) -> &[ImagePlacement] {
        self.images.active()
    }

    // ── Shell integration ────────────────────────────────────────────

    pub fn shell_zone(&self) -> SemanticZone {
        self.shell_state.zone
    }

    pub fn last_exit_code(&self) -> Option<i32> {
        self.shell_state.last_exit_code
    }

    pub fn cwd(&self) -> Option<&str> {
        self.parsers.osc7.cwd()
    }

    pub fn child_pid(&self) -> Option<u32> {
        self.pty.child_pid()
    }

    #[cfg(unix)]
    pub fn master_raw_fd(&self) -> Option<std::os::unix::io::RawFd> {
        self.pty.master_raw_fd()
    }

    // ── PTY write ────────────────────────────────────────────────────

    pub fn write_to_pty(&mut self, data: &[u8]) {
        if self.exited {
            return;
        }
        if let Err(e) = self.pty.write(data) {
            log::warn!("pty write failed (pane {}): {e}", self.id);
        }
    }

    pub fn is_password_input(&self) -> bool {
        self.password_input
    }

    // ── Mouse ────────────────────────────────────────────────────────

    pub fn has_mouse_mode(&self) -> bool {
        self.mode_flags_from_term(&self.term) & MODE_MOUSE_REPORT != 0
    }

    pub fn send_mouse_input(
        &mut self,
        button: u8,
        col: u16,
        row: u16,
        pressed: bool,
        modifiers: u8,
    ) {
        let btn_with_mods = button as u32 | ((modifiers as u32) << 2);
        let suffix = if pressed { b'M' } else { b'm' };
        let mut buf = [0u8; 32];
        let len = {
            use std::io::Write;
            let mut cursor = std::io::Cursor::new(&mut buf[..]);
            write!(
                cursor,
                "\x1b[<{};{};{}{}",
                btn_with_mods,
                col + 1,
                row + 1,
                suffix as char
            )
            .unwrap();
            cursor.position() as usize
        };
        self.write_to_pty(&buf[..len]);
    }

    // ── Resize & grid ────────────────────────────────────────────────

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if cols == self.cols && rows == self.rows {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        self.pty.resize(cols, rows);
        self.term.resize(TermSize {
            cols: cols as usize,
            rows: rows as usize,
        });
        self.dirty = true;
    }

    pub fn set_cell_size(&mut self, cell_width: f32, cell_height: f32) {
        if let Some(cell_width) = round_cell_size(cell_width) {
            self.cell_width = cell_width;
        }
        if let Some(cell_height) = round_cell_size(cell_height) {
            self.cell_height = cell_height;
        }
    }

    pub fn grid_cols(&self) -> u16 {
        self.cols
    }
    pub fn grid_rows(&self) -> u16 {
        self.rows
    }

    fn window_size(&self) -> WindowSize {
        WindowSize {
            num_lines: self.rows,
            num_cols: self.cols,
            cell_width: self.cell_width,
            cell_height: self.cell_height,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
    pub fn is_exited(&self) -> bool {
        self.exited
    }
    pub fn set_dirty(&mut self, dirty: bool) {
        self.dirty = dirty;
    }

    // ── Damage & cursor ──────────────────────────────────────────────

    pub fn cursor_info(&self) -> (i16, u16, u8, u16) {
        let content = self.term.renderable_content();
        let cursor_line = content.cursor.point.line.0 as i16;
        let cursor_col = content.cursor.point.column.0 as u16;
        let cursor_shape = cursor_shape_to_u8(content.cursor.shape);
        let mode_flags = self.mode_flags_from_term(&self.term);
        (cursor_line, cursor_col, cursor_shape, mode_flags)
    }

    pub fn history_size(&self) -> usize {
        self.term.grid().history_size()
    }

    /// Monotonic count of rows that have ever entered primary-screen
    /// scrollback: current `history_size` + rows evicted by ring-buffer
    /// saturation (`scrolled_past_limit`), read from the primary grid even
    /// while alt-screen is active. Used as the watermark for per-client
    /// incremental scrollback delivery.
    pub fn scrollback_total(&self) -> usize {
        self.term.primary_scrollback_total()
    }

    pub fn is_alt_screen(&self) -> bool {
        use alacritty_terminal::term::TermMode;
        self.term.mode().contains(TermMode::ALT_SCREEN)
    }

    // ── Mode flags ───────────────────────────────────────────────────

    pub(crate) fn mode_flags_from_term(&self, term: &Term<PtyEventListener>) -> u16 {
        use alacritty_terminal::term::TermMode;
        use ciri_protocol::message::{
            MODE_KITTY_REPORT_ALL, MODE_KITTY_REPORT_ALTERNATES, MODE_KITTY_REPORT_EVENTS,
            MODE_KITTY_REPORT_TEXT,
        };
        let mode = term.mode();
        let mut flags = 0u16;
        if mode.contains(TermMode::MOUSE_REPORT_CLICK)
            || mode.contains(TermMode::MOUSE_DRAG)
            || mode.contains(TermMode::MOUSE_MOTION)
        {
            flags |= MODE_MOUSE_REPORT;
        }
        if mode.contains(TermMode::ALT_SCREEN) {
            flags |= MODE_ALT_SCREEN;
        }
        if self.shell_state.prompt_line.is_some() {
            flags |= MODE_SHELL_INTEGRATION;
        }
        if mode.contains(TermMode::DISAMBIGUATE_ESC_CODES) {
            flags |= MODE_KITTY_KEYBOARD;
        }
        if mode.contains(TermMode::REPORT_EVENT_TYPES) {
            flags |= MODE_KITTY_REPORT_EVENTS;
        }
        if mode.contains(TermMode::REPORT_ALTERNATE_KEYS) {
            flags |= MODE_KITTY_REPORT_ALTERNATES;
        }
        if mode.contains(TermMode::REPORT_ALL_KEYS_AS_ESC) {
            flags |= MODE_KITTY_REPORT_ALL;
        }
        if mode.contains(TermMode::REPORT_ASSOCIATED_TEXT) {
            flags |= MODE_KITTY_REPORT_TEXT;
        }
        if mode.contains(TermMode::BRACKETED_PASTE) {
            flags |= MODE_BRACKETED_PASTE;
        }
        if mode.contains(TermMode::APP_CURSOR) {
            flags |= MODE_APP_CURSOR;
        }
        if mode.contains(TermMode::APP_KEYPAD) {
            flags |= MODE_APP_KEYPAD;
        }
        if mode.contains(TermMode::ALTERNATE_SCROLL) {
            flags |= MODE_ALTERNATE_SCROLL;
        }
        if self.parsers.dec_mode.focus_event_mode {
            flags |= MODE_FOCUS_EVENT;
        }
        if self.parsers.dec_mode.sync_output_mode {
            flags |= MODE_SYNCHRONIZED_OUTPUT;
        }
        if self.password_input {
            flags |= MODE_PASSWORD_INPUT;
        }
        flags
    }

    pub fn write_focus_event(&mut self, focused: bool) {
        if self.parsers.dec_mode.focus_event_mode {
            let seq = if focused { b"\x1b[I" } else { b"\x1b[O" };
            self.write_to_pty(seq);
        }
    }

    pub fn has_focus_event_mode(&self) -> bool {
        self.parsers.dec_mode.focus_event_mode
    }

    pub fn is_sync_output(&self) -> bool {
        self.parsers.dec_mode.sync_output_mode
    }
}

#[cfg(test)]
mod tests;
