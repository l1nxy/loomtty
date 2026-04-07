use ciri_protocol::message::ImageDisplayMode;
use std::sync::Arc;

pub type PaneId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticZone {
    Prompt,
    Input,
    Output,
}

#[derive(Debug, Clone)]
pub struct ImagePlacement {
    pub id: u64,
    pub row: u16,
    pub col: u16,
    pub width_cells: u16,
    pub height_cells: u16,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub display_mode: ImageDisplayMode,
    pub format: String,
    pub data: Arc<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct ShellState {
    pub zone: SemanticZone,
    pub last_exit_code: Option<i32>,
    pub prompt_line: Option<i32>,
    pub output_line: Option<i32>,
    pub command_start: Option<std::time::Instant>,
}
