use std::sync::Arc;

use crate::pane::ImagePlacement;
use crate::partial_buf::PartialBuf;

/// Maximum size of a partial DCS buffer (4MB — Sixel images can be large).
const MAX_DCS_PARTIAL_SIZE: usize = 4 * 1024 * 1024;

/// Maximum number of colors in a Sixel palette.
const MAX_PALETTE: usize = 256;

/// Result of scanning PTY data for Sixel sequences.
pub(crate) struct SixelScanResult {
    /// Newly created image placements in this scan.
    pub placements: Vec<ImagePlacement>,
}

/// Parser for Sixel graphics sequences (DCS P1;P2;P3 q ... ST).
///
/// Sixel data arrives inside a DCS (Device Control String):
/// - Start: `ESC P [params] q`
/// - Data: sixel pixel data with color commands
/// - End: `ESC \` (ST) or `0x9C`
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

    /// Scan PTY output data for Sixel DCS sequences. Returns new placements.
    /// `active_images` is updated in-place for reconnecting clients.
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
            // Decode Sixel data to RGBA pixels
            if let Some(image) = decode_sixel(payload) {
                let id = self.next_image_id;
                self.next_image_id += 1;

                // Estimate cell dimensions (6 pixels per sixel row)
                // These are rough — the client will position based on cell grid
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

/// Decoded Sixel image in RGBA format.
struct SixelImage {
    width: u32,
    height: u32,
    data: Vec<u8>, // RGBA
}

/// Decode a Sixel data stream (everything between `q` and `ST`) into RGBA pixels.
fn decode_sixel(data: &[u8]) -> Option<SixelImage> {
    let mut palette = [[0u8; 3]; MAX_PALETTE];
    // Initialize default VT340-compatible palette (16 basic colors)
    init_default_palette(&mut palette);

    let mut max_x: u32 = 0;
    let mut max_y: u32 = 0;

    // First pass: determine image dimensions
    {
        let mut px = 0u32;
        let mut py = 0u32;
        let mut i = 0;
        while i < data.len() {
            let b = data[i];
            match b {
                // Raster attributes: " Pan ; Pad ; Ph ; Pv
                b'"' => {
                    i += 1;
                    // Skip Pan;Pad
                    let mut params = [0u32; 4];
                    let mut pi = 0;
                    while i < data.len() && pi < 4 {
                        if data[i].is_ascii_digit() {
                            let mut n = 0u32;
                            while i < data.len() && data[i].is_ascii_digit() {
                                n = n.saturating_mul(10).saturating_add((data[i] - b'0') as u32);
                                i += 1;
                            }
                            params[pi] = n;
                            pi += 1;
                            if i < data.len() && data[i] == b';' {
                                i += 1;
                            }
                        } else {
                            break;
                        }
                    }
                    // Ph and Pv are the declared image width and height
                    if pi >= 4 && params[2] > 0 && params[3] > 0 {
                        max_x = max_x.max(params[2]);
                        max_y = max_y.max(params[3]);
                    }
                    continue;
                }
                // Color definition/selection
                b'#' => {
                    i += 1;
                    // Just skip over the color command for dimension calculation
                    while i < data.len() && (data[i].is_ascii_digit() || data[i] == b';') {
                        i += 1;
                    }
                    continue;
                }
                // RLE: !<count><char>
                b'!' => {
                    i += 1;
                    let mut count = 0u32;
                    while i < data.len() && data[i].is_ascii_digit() {
                        count = count
                            .saturating_mul(10)
                            .saturating_add((data[i] - b'0') as u32);
                        i += 1;
                    }
                    if i < data.len() && (0x3F..=0x7E).contains(&data[i]) {
                        px = px.saturating_add(count);
                        i += 1;
                    }
                    max_x = max_x.max(px);
                    continue;
                }
                // Carriage return (go to start of current sixel row)
                b'$' => {
                    px = 0;
                }
                // New line (advance to next sixel row = 6 pixels down)
                b'-' => {
                    px = 0;
                    py += 6;
                }
                // Sixel data character (0x3F to 0x7E)
                c if (0x3F..=0x7E).contains(&c) => {
                    px += 1;
                    max_x = max_x.max(px);
                    max_y = max_y.max(py + 6);
                }
                _ => {}
            }
            i += 1;
        }
        max_y = max_y.max(py + 6);
    }

    if max_x == 0 || max_y == 0 {
        return None;
    }

    // Cap image size to prevent OOM
    if max_x > 4096 || max_y > 4096 {
        log::warn!("sixel image too large: {max_x}x{max_y}, capping to 4096x4096");
        max_x = max_x.min(4096);
        max_y = max_y.min(4096);
    }

    let width = max_x;
    let height = max_y;
    let mut pixels = vec![0u8; (width * height * 4) as usize]; // RGBA, initialized to transparent black

    // Second pass: render pixels
    let mut i = 0;
    let mut x: u32 = 0;
    let mut y: u32 = 0;
    let mut current_color: usize = 0;

    while i < data.len() {
        let b = data[i];
        match b {
            // Raster attributes
            b'"' => {
                i += 1;
                while i < data.len() && (data[i].is_ascii_digit() || data[i] == b';') {
                    i += 1;
                }
                continue;
            }
            // Color definition/selection: #<color_number>[;Pu;Px;Py;Pz]
            b'#' => {
                i += 1;
                let mut color_num = 0u32;
                while i < data.len() && data[i].is_ascii_digit() {
                    color_num = color_num
                        .saturating_mul(10)
                        .saturating_add((data[i] - b'0') as u32);
                    i += 1;
                }
                current_color = (color_num as usize).min(MAX_PALETTE - 1);

                // Check if this is a color definition (has ;Pu;Px;Py;Pz)
                if i < data.len() && data[i] == b';' {
                    i += 1;
                    let mut params = [0u32; 4];
                    let mut pi = 0;
                    while i < data.len() && pi < 4 {
                        if data[i].is_ascii_digit() {
                            let mut n = 0u32;
                            while i < data.len() && data[i].is_ascii_digit() {
                                n = n.saturating_mul(10).saturating_add((data[i] - b'0') as u32);
                                i += 1;
                            }
                            params[pi] = n;
                            pi += 1;
                            if i < data.len() && data[i] == b';' {
                                i += 1;
                            }
                        } else {
                            break;
                        }
                    }

                    if pi >= 4 {
                        let pu = params[0]; // 1=HLS, 2=RGB
                        match pu {
                            2 => {
                                // RGB: values 0-100
                                let r = (params[1].min(100) * 255 / 100) as u8;
                                let g = (params[2].min(100) * 255 / 100) as u8;
                                let b_val = (params[3].min(100) * 255 / 100) as u8;
                                palette[current_color] = [r, g, b_val];
                            }
                            1 => {
                                // HLS: Hue (0-360), Lightness (0-100), Saturation (0-100)
                                let (r, g, b_val) = hls_to_rgb(params[1], params[2], params[3]);
                                palette[current_color] = [r, g, b_val];
                            }
                            _ => {}
                        }
                    }
                }
                continue;
            }
            // RLE: !<count><sixel_char>
            b'!' => {
                i += 1;
                let mut count = 0u32;
                while i < data.len() && data[i].is_ascii_digit() {
                    count = count
                        .saturating_mul(10)
                        .saturating_add((data[i] - b'0') as u32);
                    i += 1;
                }
                if i < data.len() && (0x3F..=0x7E).contains(&data[i]) {
                    let sixel_val = data[i] - 0x3F;
                    for _ in 0..count {
                        put_sixel(
                            &mut pixels,
                            width,
                            height,
                            x,
                            y,
                            sixel_val,
                            &palette[current_color],
                        );
                        x += 1;
                    }
                    i += 1;
                }
                continue;
            }
            // Carriage return
            b'$' => {
                x = 0;
            }
            // New line
            b'-' => {
                x = 0;
                y += 6;
            }
            // Sixel data character
            c if (0x3F..=0x7E).contains(&c) => {
                let sixel_val = c - 0x3F;
                put_sixel(
                    &mut pixels,
                    width,
                    height,
                    x,
                    y,
                    sixel_val,
                    &palette[current_color],
                );
                x += 1;
            }
            _ => {}
        }
        i += 1;
    }

    Some(SixelImage {
        width,
        height,
        data: pixels,
    })
}

/// Write a single sixel column (6 vertical pixels) into the RGBA buffer.
#[inline]
fn put_sixel(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    sixel_val: u8,
    color: &[u8; 3],
) {
    if x >= width {
        return;
    }
    for bit in 0..6u32 {
        if sixel_val & (1 << bit) != 0 {
            let py = y + bit;
            if py < height {
                let offset = ((py * width + x) * 4) as usize;
                if offset + 3 < pixels.len() {
                    pixels[offset] = color[0];
                    pixels[offset + 1] = color[1];
                    pixels[offset + 2] = color[2];
                    pixels[offset + 3] = 255; // fully opaque
                }
            }
        }
    }
}

/// Convert HLS (Hue 0-360, Lightness 0-100, Saturation 0-100) to RGB (0-255).
/// Sixel uses HLS parameter order; colorsys expects HSL, so we swap L and S.
fn hls_to_rgb(h: u32, l: u32, s: u32) -> (u8, u8, u8) {
    use colorsys::{Hsl, Rgb};
    let hsl = Hsl::new((h % 360) as f64, s.min(100) as f64, l.min(100) as f64, None);
    let rgb = Rgb::from(hsl);
    (rgb.red() as u8, rgb.green() as u8, rgb.blue() as u8)
}

/// Initialize the default VT340-compatible 16-color palette.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_simple_sixel() {
        // A minimal sixel: 1 pixel column, color 0 (black), all 6 bits set
        // '#0;2;100;0;0' = define color 0 as red (RGB 100,0,0)
        // '~' = 0x7E - 0x3F = 0x3F = all 6 bits set
        let data = b"#0;2;100;0;0#0~";
        let img = decode_sixel(data).unwrap();
        assert_eq!(img.width, 1);
        assert_eq!(img.height, 6);
        // Check that all 6 pixels are red (255,0,0,255)
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
        // RLE: repeat '~' (all bits) 3 times with color 0 (default black)
        let data = b"#0;2;0;100;0#0!3~";
        let img = decode_sixel(data).unwrap();
        assert_eq!(img.width, 3);
        assert_eq!(img.height, 6);
    }

    #[test]
    fn decode_newline() {
        // Two sixel rows: first and second, each 1 pixel wide
        let data = b"#0;2;100;0;0#0~-~";
        let img = decode_sixel(data).unwrap();
        assert_eq!(img.width, 1);
        assert_eq!(img.height, 12); // 2 sixel rows × 6 pixels
    }

    #[test]
    fn decode_empty_returns_none() {
        let data = b"";
        assert!(decode_sixel(data).is_none());
    }

    #[test]
    fn hls_to_rgb_basic() {
        // Red: H=0, L=50, S=100
        let (r, g, b) = hls_to_rgb(0, 50, 100);
        assert!(r > 200);
        assert!(g < 20);
        assert!(b < 20);
    }

    #[test]
    fn parser_scan_sixel_sequence() {
        // Full DCS sequence: ESC P q <sixel_data> ESC \
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
        // Split the DCS across two reads
        let mut parser = SixelParser::new();
        let mut active = Vec::new();

        // First read: incomplete
        let result = parser.scan(b"\x1bPq#0;2;100;0;0#0", 0, 0, &mut active);
        assert_eq!(result.placements.len(), 0);
        assert!(!parser.dcs_partial.is_empty());

        // Second read: completes the sequence
        let result = parser.scan(b"~\x1b\\", 0, 0, &mut active);
        assert_eq!(result.placements.len(), 1);
    }
}
