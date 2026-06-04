//! Cell property extraction from alacritty Term and protocol PackedCell.

use alacritty_terminal::term::cell::Flags as CellFlags;
use loom_config::config::LoomConfig;
use loom_config::theme::ThemeConfig;
use loom_protocol::message::{
    FLAG_BOLD, FLAG_DIM, FLAG_HIDDEN, FLAG_INVERSE, FLAG_ITALIC, FLAG_STRIKEOUT, FLAG_UNDERLINE,
    FLAG_UNDERLINE_CURLY, FLAG_UNDERLINE_DASHED, FLAG_UNDERLINE_DOTTED, FLAG_UNDERLINE_DOUBLE,
    FLAG_UNDERLINE_STYLE_MASK, FLAG_WIDE_CHAR, FLAG_WIDE_CHAR_SPACER, PackedCell,
};

use crate::glyph_cache::{FontStyle, GlyphCache};

use super::color::{ColorTable, ansi_color_to_rgba};

// ─── Common types ────────────────────────────────────────────────────

/// Underline decoration style, abstracted from source-specific flag formats.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum UnderlineStyle {
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

/// Cell properties extracted from either alacritty `Term` or protocol `PackedCell`.
/// Colors are pre-processed: bold-brighten, dim, and inverse already applied.
pub(super) struct CellProps {
    pub(super) ch: char,
    pub(super) fg: [f32; 4],
    pub(super) bg: [f32; 4],
    pub(super) style: FontStyle,
    pub(super) is_wide: bool,
    pub(super) is_hidden: bool,
    pub(super) underline: UnderlineStyle,
    pub(super) is_strikeout: bool,
}

/// Pre-computed cell layout values from the glyph atlas.
pub(super) struct CellMetrics {
    pub(super) cw: f32,         // cell width in pixels
    pub(super) ch: f32,         // cell height in pixels
    pub(super) baseline: f32,   // baseline offset from cell top
    pub(super) face_width: f32, // unrounded face advance width (for centering compensation)
    pub(super) default_bg: [f32; 4],
    /// Pixel offset added to the underline's vertical position.
    pub(super) underline_offset: f32,
    /// Underline thickness in pixels (clamped to ≥ 1px so the line is visible).
    pub(super) underline_thickness: f32,
    /// Pixel offset added to the strikethrough's vertical position.
    pub(super) strikethrough_offset: f32,
    /// Strikethrough thickness in pixels (clamped to ≥ 1px).
    pub(super) strikethrough_thickness: f32,
}

impl CellMetrics {
    pub(super) fn new(atlas: &GlyphCache, config: &LoomConfig) -> Self {
        let base_thickness = 1.0_f32;
        let ul_thick = (base_thickness * config.font.adjust_underline_thickness.max(0.0)).max(1.0);
        let st_thick =
            (base_thickness * config.font.adjust_strikethrough_thickness.max(0.0)).max(1.0);
        CellMetrics {
            cw: atlas.cell_width,
            ch: atlas.cell_height,
            baseline: atlas.ascent,
            face_width: atlas.face_width,
            default_bg: ThemeConfig::parse_color(&config.theme.background),
            underline_offset: config.font.adjust_underline_position,
            underline_thickness: ul_thick,
            strikethrough_offset: config.font.adjust_strikethrough_position,
            strikethrough_thickness: st_thick,
        }
    }
}

// ─── Cell property extraction ────────────────────────────────────────

impl CellProps {
    /// Extract from an alacritty terminal cell. Returns `None` for wide-char spacers.
    pub(super) fn from_term_cell(
        cell: &alacritty_terminal::term::cell::Cell,
        config: &LoomConfig,
    ) -> Option<Self> {
        if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
            return None;
        }

        let is_bold = cell.flags.contains(CellFlags::BOLD);
        let is_italic = cell.flags.contains(CellFlags::ITALIC);
        let mut fg = ansi_color_to_rgba(cell.fg, config);
        let mut bg = ansi_color_to_rgba(cell.bg, config);
        apply_color_modifiers(
            &mut fg,
            &mut bg,
            is_bold,
            cell.flags.contains(CellFlags::DIM),
            cell.flags.contains(CellFlags::INVERSE),
        );

        Some(CellProps {
            ch: cell.c,
            fg,
            bg,
            style: FontStyle::from_bold_italic(is_bold, is_italic),
            is_wide: cell.flags.contains(CellFlags::WIDE_CHAR),
            is_hidden: cell.flags.contains(CellFlags::HIDDEN),
            underline: UnderlineStyle::from_term_flags(cell.flags),
            is_strikeout: cell.flags.contains(CellFlags::STRIKEOUT),
        })
    }

    /// Extract from a protocol `PackedCell` using pre-computed color table.
    pub(super) fn from_packed_cell_fast(cell: &PackedCell, ct: &ColorTable) -> Option<Self> {
        let f = cell.flags_u16();
        if f & FLAG_WIDE_CHAR_SPACER != 0 {
            return None;
        }

        let is_bold = f & FLAG_BOLD != 0;
        let is_italic = f & FLAG_ITALIC != 0;
        let mut fg = ct.resolve_packed(cell.fg);
        let mut bg = ct.resolve_packed(cell.bg);
        apply_color_modifiers(
            &mut fg,
            &mut bg,
            is_bold,
            f & FLAG_DIM != 0,
            f & FLAG_INVERSE != 0,
        );

        let underline = if f & FLAG_UNDERLINE != 0 {
            UnderlineStyle::from_packed_flags(f & FLAG_UNDERLINE_STYLE_MASK)
        } else {
            UnderlineStyle::None
        };

        Some(CellProps {
            ch: cell.ch(),
            fg,
            bg,
            style: FontStyle::from_bold_italic(is_bold, is_italic),
            is_wide: f & FLAG_WIDE_CHAR != 0,
            is_hidden: f & FLAG_HIDDEN != 0,
            underline,
            is_strikeout: f & FLAG_STRIKEOUT != 0,
        })
    }
}

/// Apply bold-brighten, dim, and inverse color modifications in-place.
fn apply_color_modifiers(
    fg: &mut [f32; 4],
    bg: &mut [f32; 4],
    bold: bool,
    dim: bool,
    inverse: bool,
) {
    // Bold no longer brightens colors — font weight change is sufficient.
    // Previous behavior (*1.3) made bold text visually heavier than expected.
    let _ = bold;
    if dim {
        for c in &mut fg[..3] {
            *c *= 0.67;
        }
    }
    if inverse {
        std::mem::swap(fg, bg);
    }
}

impl UnderlineStyle {
    /// Detect underline style from alacritty's `CellFlags`.
    pub(super) fn from_term_flags(flags: CellFlags) -> Self {
        if !flags.contains(CellFlags::ALL_UNDERLINES) {
            Self::None
        } else if flags.contains(CellFlags::DOUBLE_UNDERLINE) {
            Self::Double
        } else if flags.contains(CellFlags::UNDERCURL) {
            Self::Curly
        } else if flags.contains(CellFlags::DOTTED_UNDERLINE) {
            Self::Dotted
        } else if flags.contains(CellFlags::DASHED_UNDERLINE) {
            Self::Dashed
        } else {
            Self::Single
        }
    }

    /// Decode underline style from packed protocol flags (masked bits).
    pub(super) fn from_packed_flags(style_bits: u16) -> Self {
        match style_bits {
            FLAG_UNDERLINE_DOUBLE => Self::Double,
            FLAG_UNDERLINE_CURLY => Self::Curly,
            FLAG_UNDERLINE_DOTTED => Self::Dotted,
            FLAG_UNDERLINE_DASHED => Self::Dashed,
            _ => Self::Single,
        }
    }
}
