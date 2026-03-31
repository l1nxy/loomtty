//! Sixel graphics decoder (DCS P1;P2;P3 q ... ST).

use std::sync::Arc;

use winnow::prelude::*;
use winnow::token::{any, take_while};

use crate::pane::ImagePlacement;
use crate::partial_buf::PartialBuf;

const MAX_DCS_PARTIAL_SIZE: usize = 4 * 1024 * 1024;
const MAX_PALETTE: usize = 256;
const MAX_IMAGE_DIM: u32 = 4096;

// ─── Sixel command AST ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum ColorSpace {
    Rgb,
    Hls,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SixelCmd {
    RasterAttrs {
        width: u32,
        height: u32,
    },
    ColorDef {
        idx: u8,
        space: ColorSpace,
        p1: u32,
        p2: u32,
        p3: u32,
    },
    ColorSelect(u8),
    Repeat {
        count: u32,
        sixel: u8,
    },
    Data(u8),
    CarriageReturn,
    NewLine,
}

// ─── winnow combinators ──────────────────────────────────────────────

/// Parse a saturating decimal u32.
fn decimal_u32(input: &mut &[u8]) -> winnow::error::ModalResult<u32> {
    take_while(1.., |b: u8| b.is_ascii_digit())
        .map(|digits: &[u8]| {
            digits.iter().fold(0u32, |acc, &b| {
                acc.saturating_mul(10).saturating_add((b - b'0') as u32)
            })
        })
        .parse_next(input)
}

/// Skip a `;` separator.
fn semi(input: &mut &[u8]) -> winnow::error::ModalResult<()> {
    b';'.void().parse_next(input)
}

/// Parse raster attributes: `" Pan ; Pad ; Ph ; Pv`
fn raster_attrs(input: &mut &[u8]) -> winnow::error::ModalResult<SixelCmd> {
    let _pan = decimal_u32.parse_next(input)?;
    semi.parse_next(input)?;
    let _pad = decimal_u32.parse_next(input)?;
    semi.parse_next(input)?;
    let width = decimal_u32.parse_next(input)?;
    semi.parse_next(input)?;
    let height = decimal_u32.parse_next(input)?;
    Ok(SixelCmd::RasterAttrs { width, height })
}

/// Parse color command: `# <number> [; Pu ; Px ; Py ; Pz]`
fn color_cmd(input: &mut &[u8]) -> winnow::error::ModalResult<SixelCmd> {
    let idx = decimal_u32.parse_next(input)?;
    let idx = idx.min(MAX_PALETTE as u32 - 1) as u8;

    if winnow::combinator::opt(semi).parse_next(input)?.is_none() {
        return Ok(SixelCmd::ColorSelect(idx));
    }
    let pu = decimal_u32.parse_next(input)?;
    semi.parse_next(input)?;
    let p1 = decimal_u32.parse_next(input)?;
    semi.parse_next(input)?;
    let p2 = decimal_u32.parse_next(input)?;
    semi.parse_next(input)?;
    let p3 = decimal_u32.parse_next(input)?;

    let space = match pu {
        1 => ColorSpace::Hls,
        _ => ColorSpace::Rgb,
    };
    Ok(SixelCmd::ColorDef {
        idx,
        space,
        p1,
        p2,
        p3,
    })
}

/// Parse RLE: `! <count> <sixel_char>`
fn rle_cmd(input: &mut &[u8]) -> winnow::error::ModalResult<SixelCmd> {
    let count = decimal_u32.parse_next(input)?;
    let ch = any.parse_next(input)?;
    if !(0x3F..=0x7E).contains(&ch) {
        return Err(winnow::error::ErrMode::Backtrack(
            winnow::error::ContextError::new(),
        ));
    }
    Ok(SixelCmd::Repeat {
        count,
        sixel: ch - 0x3F,
    })
}

/// Parse one sixel command from the stream.
fn sixel_cmd(input: &mut &[u8]) -> winnow::error::ModalResult<SixelCmd> {
    let b = any.parse_next(input)?;
    match b {
        b'"' => raster_attrs.parse_next(input),
        b'#' => color_cmd.parse_next(input),
        b'!' => rle_cmd.parse_next(input),
        b'$' => Ok(SixelCmd::CarriageReturn),
        b'-' => Ok(SixelCmd::NewLine),
        0x3F..=0x7E => Ok(SixelCmd::Data(b - 0x3F)),
        _ => Err(winnow::error::ErrMode::Backtrack(
            winnow::error::ContextError::new(),
        )),
    }
}

// ─── Command stream iterator ─────────────────────────────────────────

struct SixelCmdIter<'a> {
    data: &'a [u8],
}

impl<'a> SixelCmdIter<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data }
    }
}

impl Iterator for SixelCmdIter<'_> {
    type Item = SixelCmd;

    fn next(&mut self) -> Option<SixelCmd> {
        while !self.data.is_empty() {
            match sixel_cmd(&mut self.data) {
                Ok(cmd) => return Some(cmd),
                Err(_) => continue, // skip unknown bytes
            }
        }
        None
    }
}

// ─── Dimension calculator (pass 1) ──────────────────────────────────

fn compute_dimensions(data: &[u8]) -> (u32, u32) {
    let mut max_x: u32 = 0;
    let mut max_y: u32 = 0;
    let mut px: u32 = 0;
    let mut py: u32 = 0;

    for cmd in SixelCmdIter::new(data) {
        match cmd {
            SixelCmd::RasterAttrs { width, height } => {
                if width > 0 && height > 0 {
                    max_x = max_x.max(width);
                    max_y = max_y.max(height);
                }
            }
            SixelCmd::Data(_) => {
                px = px.saturating_add(1);
                max_x = max_x.max(px);
                max_y = max_y.max(py.saturating_add(6));
            }
            SixelCmd::Repeat { count, .. } => {
                px = px.saturating_add(count);
                max_x = max_x.max(px);
            }
            SixelCmd::CarriageReturn => px = 0,
            SixelCmd::NewLine => {
                px = 0;
                py = py.saturating_add(6);
            }
            _ => {}
        }
    }
    max_y = max_y.max(py.saturating_add(6));
    (max_x, max_y)
}

// ─── Pixel renderer (pass 2) ────────────────────────────────────────

struct SixelRenderer {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    palette: [[u8; 3]; MAX_PALETTE],
    x: u32,
    y: u32,
    current_color: usize,
}

impl SixelRenderer {
    fn new(width: u32, height: u32) -> Self {
        let mut palette = [[0u8; 3]; MAX_PALETTE];
        init_default_palette(&mut palette);
        Self {
            pixels: vec![0u8; (width * height * 4) as usize],
            width,
            height,
            palette,
            x: 0,
            y: 0,
            current_color: 0,
        }
    }

    fn execute(&mut self, cmd: SixelCmd) {
        match cmd {
            SixelCmd::RasterAttrs { .. } => {}
            SixelCmd::ColorDef {
                idx,
                space,
                p1,
                p2,
                p3,
            } => {
                let i = idx as usize;
                self.palette[i] = match space {
                    ColorSpace::Rgb => [
                        (p1.min(100) * 255 / 100) as u8,
                        (p2.min(100) * 255 / 100) as u8,
                        (p3.min(100) * 255 / 100) as u8,
                    ],
                    ColorSpace::Hls => {
                        let (r, g, b) = hls_to_rgb(p1, p2, p3);
                        [r, g, b]
                    }
                };
                self.current_color = i;
            }
            SixelCmd::ColorSelect(idx) => {
                self.current_color = idx as usize;
            }
            SixelCmd::Data(sixel) => {
                self.put_sixel(sixel);
                self.x += 1;
            }
            SixelCmd::Repeat { count, sixel } => {
                let count = count.min(self.width.saturating_sub(self.x));
                for _ in 0..count {
                    self.put_sixel(sixel);
                    self.x += 1;
                }
            }
            SixelCmd::CarriageReturn => self.x = 0,
            SixelCmd::NewLine => {
                self.x = 0;
                self.y += 6;
            }
        }
    }

    #[inline]
    fn put_sixel(&mut self, sixel: u8) {
        if self.x >= self.width {
            return;
        }
        let color = &self.palette[self.current_color];
        for bit in 0..6u32 {
            if sixel & (1 << bit) != 0 {
                let py = self.y + bit;
                if py < self.height {
                    let offset = ((py * self.width + self.x) * 4) as usize;
                    if offset + 3 < self.pixels.len() {
                        self.pixels[offset] = color[0];
                        self.pixels[offset + 1] = color[1];
                        self.pixels[offset + 2] = color[2];
                        self.pixels[offset + 3] = 255;
                    }
                }
            }
        }
    }

    fn finish(self) -> SixelImage {
        SixelImage {
            width: self.width,
            height: self.height,
            data: self.pixels,
        }
    }
}

// ─── Public API ──────────────────────────────────────────────────────

pub(crate) struct SixelScanResult {
    pub placements: Vec<ImagePlacement>,
}

pub(crate) struct SixelParser {
    dcs_partial: PartialBuf,
    next_image_id: u64,
}

impl SixelParser {
    pub fn new() -> Self {
        Self {
            dcs_partial: PartialBuf::new(MAX_DCS_PARTIAL_SIZE, "sixel DCS"),
            next_image_id: 1_000_000,
        }
    }

    pub fn scan(
        &mut self,
        data: &[u8],
        cursor_col: u16,
        cursor_row: u16,
        active_images: &mut Vec<ImagePlacement>,
    ) -> SixelScanResult {
        let mut tmp = Vec::new();
        let data = self.dcs_partial.prepend_to(data, &mut tmp);

        let mut placements = Vec::new();
        let scan = crate::esc_scanner::scan_dcs(data);

        for (_offset, payload) in &scan.sequences {
            if let Some(image) = decode_sixel(payload) {
                let id = self.next_image_id;
                self.next_image_id += 1;

                let width_cells = (image.width as u16 / 8).max(1);
                let height_cells = (image.height as u16 / 16).max(1);

                log::info!(
                    "sixel image #{id}: {}x{} pixels, {width_cells}x{height_cells} cells, {} bytes",
                    image.width,
                    image.height,
                    image.data.len()
                );

                let placement = ImagePlacement {
                    id,
                    row: cursor_row,
                    col: cursor_col,
                    width_cells,
                    height_cells,
                    pixel_width: image.width,
                    pixel_height: image.height,
                    format: "rgba".to_string(),
                    data: Arc::new(image.data),
                };
                active_images.push(placement.clone());
                placements.push(placement);
            }
        }

        if let Some(partial_start) = scan.partial_start {
            self.dcs_partial.store(&data[partial_start..]);
        }

        SixelScanResult { placements }
    }
}

// ─── Internal ────────────────────────────────────────────────────────

struct SixelImage {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

fn decode_sixel(data: &[u8]) -> Option<SixelImage> {
    let (mut width, mut height) = compute_dimensions(data);
    if width == 0 || height == 0 {
        return None;
    }

    if width > MAX_IMAGE_DIM || height > MAX_IMAGE_DIM {
        log::warn!(
            "sixel image too large: {width}x{height}, capping to {MAX_IMAGE_DIM}x{MAX_IMAGE_DIM}"
        );
        width = width.min(MAX_IMAGE_DIM);
        height = height.min(MAX_IMAGE_DIM);
    }

    let mut renderer = SixelRenderer::new(width, height);
    for cmd in SixelCmdIter::new(data) {
        renderer.execute(cmd);
    }
    Some(renderer.finish())
}

fn hls_to_rgb(h: u32, l: u32, s: u32) -> (u8, u8, u8) {
    use colorsys::{Hsl, Rgb};
    let hsl = Hsl::new((h % 360) as f64, s.min(100) as f64, l.min(100) as f64, None);
    let rgb = Rgb::from(hsl);
    (rgb.red() as u8, rgb.green() as u8, rgb.blue() as u8)
}

fn init_default_palette(palette: &mut [[u8; 3]; MAX_PALETTE]) {
    let defaults: [[u8; 3]; 16] = [
        [0, 0, 0],       // 0: black
        [51, 51, 204],   // 1: blue
        [204, 33, 33],   // 2: red
        [51, 204, 51],   // 3: green
        [204, 51, 204],  // 4: magenta
        [51, 204, 204],  // 5: cyan
        [204, 204, 51],  // 6: yellow
        [135, 135, 135], // 7: gray 50%
        [51, 51, 51],    // 8: gray 25%
        [84, 84, 255],   // 9: light blue
        [255, 84, 84],   // 10: light red
        [84, 255, 84],   // 11: light green
        [255, 84, 255],  // 12: light magenta
        [84, 255, 255],  // 13: light cyan
        [255, 255, 84],  // 14: light yellow
        [255, 255, 255], // 15: white
    ];
    for (i, c) in defaults.iter().enumerate() {
        palette[i] = *c;
    }
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sixel_data_cmd() {
        let mut input: &[u8] = &[0x7E]; // '~' = 0x3F all bits set
        let cmd = sixel_cmd(&mut input).unwrap();
        assert_eq!(cmd, SixelCmd::Data(0x3F));
    }

    #[test]
    fn parse_rle_cmd() {
        let mut input: &[u8] = b"!3~";
        let cmd = sixel_cmd(&mut input).unwrap();
        assert_eq!(
            cmd,
            SixelCmd::Repeat {
                count: 3,
                sixel: 0x3F
            }
        );
    }

    #[test]
    fn parse_color_def_rgb() {
        let mut input: &[u8] = b"#0;2;100;0;0";
        let cmd = sixel_cmd(&mut input).unwrap();
        assert_eq!(
            cmd,
            SixelCmd::ColorDef {
                idx: 0,
                space: ColorSpace::Rgb,
                p1: 100,
                p2: 0,
                p3: 0,
            }
        );
    }

    #[test]
    fn parse_color_select() {
        let mut input: &[u8] = b"#5~";
        let cmd = sixel_cmd(&mut input).unwrap();
        assert_eq!(cmd, SixelCmd::ColorSelect(5));
        // '~' remains unconsumed
        assert_eq!(input, b"~");
    }

    #[test]
    fn cmd_iter_basic() {
        let data = b"#0;2;100;0;0#0~-~";
        let cmds: Vec<_> = SixelCmdIter::new(data).collect();
        assert_eq!(cmds.len(), 5); // ColorDef, ColorSelect, Data, NewLine, Data
    }

    #[test]
    fn decode_simple_sixel() {
        let data = b"#0;2;100;0;0#0~";
        let img = decode_sixel(data).unwrap();
        assert_eq!(img.width, 1);
        assert_eq!(img.height, 6);
        for py in 0..6 {
            let offset = (py * 4) as usize;
            assert_eq!(img.data[offset], 255, "pixel {py} red channel");
            assert_eq!(img.data[offset + 1], 0, "pixel {py} green channel");
            assert_eq!(img.data[offset + 2], 0, "pixel {py} blue channel");
            assert_eq!(img.data[offset + 3], 255, "pixel {py} alpha channel");
        }
    }

    #[test]
    fn decode_rle_sixel() {
        let data = b"#0;2;0;100;0#0!3~";
        let img = decode_sixel(data).unwrap();
        assert_eq!(img.width, 3);
        assert_eq!(img.height, 6);
    }

    #[test]
    fn decode_newline() {
        let data = b"#0;2;100;0;0#0~-~";
        let img = decode_sixel(data).unwrap();
        assert_eq!(img.width, 1);
        assert_eq!(img.height, 12);
    }

    #[test]
    fn decode_empty_returns_none() {
        assert!(decode_sixel(b"").is_none());
    }

    #[test]
    fn hls_to_rgb_basic() {
        let (r, g, b) = hls_to_rgb(0, 50, 100);
        assert!(r > 200);
        assert!(g < 20);
        assert!(b < 20);
    }

    #[test]
    fn parser_scan_sixel_sequence() {
        let mut data = Vec::new();
        data.extend_from_slice(b"\x1bPq");
        data.extend_from_slice(b"#0;2;100;0;0#0~");
        data.extend_from_slice(b"\x1b\\");

        let mut parser = SixelParser::new();
        let mut active = Vec::new();
        let result = parser.scan(&data, 0, 0, &mut active);
        assert_eq!(result.placements.len(), 1);
        assert_eq!(result.placements[0].format, "rgba");
        assert!(result.placements[0].pixel_width > 0);
    }

    #[test]
    fn parser_partial_sequence() {
        let mut parser = SixelParser::new();
        let mut active = Vec::new();

        let result = parser.scan(b"\x1bPq#0;2;100;0;0#0", 0, 0, &mut active);
        assert_eq!(result.placements.len(), 0);
        assert!(!parser.dcs_partial.is_empty());

        let result = parser.scan(b"~\x1b\\", 0, 0, &mut active);
        assert_eq!(result.placements.len(), 1);
    }
}
