//! Terminal cell → GPU rendering primitives.
//!
//! Two entry points serve different contexts:
//! - [`build_terminal_view`]: reads directly from alacritty's `Term` (server-side)
//! - [`build_view_from_grid`]: reads from [`PackedCell`] grid received over the wire (client-side)
//!
//! Both extract per-cell properties into [`CellProps`], then share a single rendering
//! path for backgrounds, text glyphs, decorations (underline/strikeout), and cursor.

mod box_drawing;
mod cell;
pub mod color;
mod cursor;
mod decoration;
mod glyph;
pub mod scrollbar;
mod shaping;
mod view;

pub use color::ColorTable;
pub use glyph::RelativeGlyph;
pub use scrollbar::{SCROLLBAR_MARGIN, SCROLLBAR_WIDTH, ScrollbarState, build_scrollbar};
pub use view::{
    PackedViewInputs, TerminalView, build_terminal_view, build_view_from_grid,
    update_view_from_grid,
};

#[cfg(test)]
mod tests;
