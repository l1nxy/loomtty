//! Scrollbar rendering.

use ciri_config::config::CiriConfig;
use ciri_config::theme::ThemeConfig;

use crate::rect::Rect;

/// Visual scrollbar width in pixels.
pub const SCROLLBAR_WIDTH: f32 = 4.0;
/// Margin between scrollbar and pane edge in pixels.
pub const SCROLLBAR_MARGIN: f32 = 2.0;

/// Visual state of the scrollbar thumb.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollbarState {
    Idle,
    Hovered,
    Pressed,
}

/// Build a scrollbar rect for a pane with scrollback.
/// Returns `None` if scrollback is empty (nothing to scroll).
pub fn build_scrollbar(
    scroll_offset: usize,
    total_lines: usize,
    visible_rows: u16,
    pane_width: f32,
    pane_height: f32,
    state: ScrollbarState,
    config: &CiriConfig,
) -> Option<Rect> {
    let visible = visible_rows as usize;
    if total_lines <= visible {
        return None;
    }

    let thumb_height = scrollbar_thumb_height(visible, total_lines, pane_height);
    let y = scrollbar_thumb_y(
        scroll_offset,
        total_lines,
        visible,
        pane_height,
        thumb_height,
    );
    let [r, g, b, _] = scrollbar_thumb_color(state, config);

    Some(Rect {
        x: pane_width - SCROLLBAR_WIDTH - SCROLLBAR_MARGIN,
        y,
        w: SCROLLBAR_WIDTH,
        h: thumb_height,
        color: [r, g, b, scrollbar_thumb_alpha(state)],
    })
}

fn scrollbar_thumb_height(visible: usize, total_lines: usize, pane_height: f32) -> f32 {
    let ratio = visible as f32 / total_lines as f32;
    (ratio * pane_height).max(10.0)
}

fn scrollbar_thumb_y(
    scroll_offset: usize,
    total_lines: usize,
    visible: usize,
    pane_height: f32,
    thumb_height: f32,
) -> f32 {
    let max_offset = total_lines - visible;
    let position_ratio = if max_offset == 0 {
        1.0
    } else {
        1.0 - (scroll_offset as f32 / max_offset as f32)
    };
    position_ratio * (pane_height - thumb_height)
}

fn scrollbar_thumb_color(state: ScrollbarState, config: &CiriConfig) -> [f32; 4] {
    match state {
        ScrollbarState::Idle => ThemeConfig::parse_color(&config.theme.bright_black),
        ScrollbarState::Hovered | ScrollbarState::Pressed => {
            ThemeConfig::parse_color(&config.theme.foreground)
        }
    }
}

fn scrollbar_thumb_alpha(state: ScrollbarState) -> f32 {
    match state {
        ScrollbarState::Idle => 0.4,
        ScrollbarState::Hovered => 0.45,
        ScrollbarState::Pressed => 0.6,
    }
}
