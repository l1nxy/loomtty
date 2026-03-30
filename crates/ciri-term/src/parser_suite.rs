//! Aggregated parser suite for all terminal escape sequence protocols.

use crate::dec_mode_parser::DecModeParser;
use crate::kitty_graphics::{KittyGraphicsParser, KittyScanResult};
use crate::osc7_parser::Osc7Parser;
use crate::osc8_parser::Osc8Parser;
use crate::pane::{ImagePlacement, ShellState};
use crate::shell_integration::Osc133Parser;
use crate::sixel::SixelParser;

/// All protocol parsers used by a Pane, grouped for clean ownership.
pub(crate) struct ParserSuite {
    pub osc133: Osc133Parser,
    pub dec_mode: DecModeParser,
    pub osc8: Osc8Parser,
    pub osc7: Osc7Parser,
    pub kitty: KittyGraphicsParser,
    pub sixel: SixelParser,
}

impl ParserSuite {
    pub fn new() -> Self {
        Self {
            osc133: Osc133Parser::new(),
            dec_mode: DecModeParser::new(),
            osc8: Osc8Parser::new(),
            osc7: Osc7Parser::new(),
            kitty: KittyGraphicsParser::new(),
            sixel: SixelParser::new(),
        }
    }

    /// Run non-image parsers (OSC 133, DEC mode, OSC 8, OSC 7) on a data chunk.
    pub fn scan_control(
        &mut self,
        chunk: &[u8],
        shell_state: &mut ShellState,
        last_command_duration: &mut Option<std::time::Duration>,
    ) {
        self.osc133.scan(chunk, shell_state, last_command_duration);
        self.dec_mode.scan(chunk);
        self.osc8.scan(chunk);
        self.osc7.scan(chunk);
    }

    /// Run image parsers (Kitty + Sixel) on a data chunk at the given cursor position.
    /// Returns (kitty_result, sixel_placements).
    pub fn scan_images(
        &mut self,
        chunk: &[u8],
        cursor_col: u16,
        cursor_row: u16,
        active_images: &mut Vec<ImagePlacement>,
    ) -> (KittyScanResult, Vec<ImagePlacement>) {
        let kitty_result = self
            .kitty
            .scan(chunk, cursor_col, cursor_row, active_images);
        let sixel_result = self
            .sixel
            .scan(chunk, cursor_col, cursor_row, active_images);
        (kitty_result, sixel_result.placements)
    }
}
