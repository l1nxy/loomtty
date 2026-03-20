use std::sync::Arc;

use crate::pane::ImagePlacement;

const MAX_APC_PARTIAL_SIZE: usize = 16 * 1024 * 1024; // 16MB

/// Kitty image metadata parsed from the first chunk of a transmission.
#[derive(Debug, Clone)]
struct KittyImageMeta {
    format: String, // "png", "rgb", "rgba"
    width: u32,
    height: u32,
    cols: u16,
    rows: u16,
}

/// Map Kitty format value to format string.
fn kitty_format_str(format_val: u32) -> &'static str {
    match format_val {
        24 => "rgb",
        32 => "rgba",
        _ => "png",
    }
}

/// Result of scanning PTY data for Kitty graphics sequences.
pub(crate) struct KittyScanResult {
    /// Newly created image placements in this scan.
    pub placements: Vec<ImagePlacement>,
    /// True if a delete command (`a=d`) was encountered, meaning the caller
    /// should also clear any previously queued pending images.
    pub deleted: bool,
}

/// Parser for Kitty graphics protocol APC sequences (ESC _ G ... ESC \).
/// Handles multi-chunk transmissions and sequences split across PTY reads.
pub(crate) struct KittyGraphicsParser {
    image_buf: Vec<u8>,
    image_meta: Option<KittyImageMeta>,
    apc_partial: Vec<u8>,
    next_image_id: u64,
}

impl KittyGraphicsParser {
    pub fn new() -> Self {
        KittyGraphicsParser {
            image_buf: Vec::new(),
            image_meta: None,
            apc_partial: Vec::new(),
            next_image_id: 1,
        }
    }

    /// Scan data for Kitty APC sequences. Returns new placements and whether
    /// a delete command was encountered.
    /// `active_images` is updated in-place with new placements (for reconnect).
    pub fn scan(
        &mut self,
        data: &[u8],
        cursor_col: u16,
        cursor_row: u16,
        active_images: &mut Vec<ImagePlacement>,
    ) -> KittyScanResult {
        use base64::Engine;

        // If we have a partial APC from a previous read, prepend it
        let working_data;
        let data = if !self.apc_partial.is_empty() {
            self.apc_partial.extend_from_slice(data);
            working_data = std::mem::take(&mut self.apc_partial);
            &working_data[..]
        } else {
            data
        };

        let mut new_placements = Vec::new();
        let mut deleted = false;
        let mut i = 0;
        while i + 3 < data.len() {
            // Look for ESC _ G (APC for Kitty graphics)
            if data[i] == 0x1b && data[i + 1] == b'_' && data[i + 2] == b'G' {
                // Find the string terminator (ESC \)
                let start = i + 3;
                let mut end = start;
                while end + 1 < data.len() {
                    if data[end] == 0x1b && data[end + 1] == b'\\' {
                        break;
                    }
                    end += 1;
                }
                if end + 1 >= data.len() {
                    // Incomplete sequence — buffer from the APC start for next read
                    let partial = &data[i..];
                    if partial.len() > MAX_APC_PARTIAL_SIZE {
                        log::warn!(
                            "kitty APC partial buffer exceeded {}MB limit, discarding",
                            MAX_APC_PARTIAL_SIZE / (1024 * 1024)
                        );
                        self.apc_partial.clear();
                    } else {
                        self.apc_partial = partial.to_vec();
                    }
                    return KittyScanResult {
                        placements: new_placements,
                        deleted,
                    };
                }

                let payload = &data[start..end];

                // Split at first ';' into control and data parts
                let (control, img_data) = if let Some(sep) = payload.iter().position(|&b| b == b';')
                {
                    (&payload[..sep], &payload[sep + 1..])
                } else {
                    (payload, &[][..])
                };

                // Parse key=value pairs from control
                let control_str = String::from_utf8_lossy(control);
                let mut action = 'T'; // default: transmit and display
                let mut format_val = 32u32; // 32=PNG, 24=RGB, 32=RGBA
                let mut width = 0u32;
                let mut height = 0u32;
                let mut cols = 0u16;
                let mut rows = 0u16;
                let mut more_chunks = false;

                for pair in control_str.split(',') {
                    if let Some((k, v)) = pair.split_once('=') {
                        match k {
                            "a" => action = v.chars().next().unwrap_or('T'),
                            "f" => format_val = v.parse().unwrap_or(32),
                            "s" => width = v.parse().unwrap_or(0),
                            "v" => height = v.parse().unwrap_or(0),
                            "c" => cols = v.parse().unwrap_or(0),
                            "r" => rows = v.parse().unwrap_or(0),
                            "m" => more_chunks = v == "1",
                            _ => {}
                        }
                    }
                }

                // Decode base64 image data
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(img_data)
                    .unwrap_or_default();

                match action {
                    'T' | 't' => {
                        // Transmit (and display if 'T')
                        if more_chunks {
                            // First/middle chunk: accumulate
                            if self.image_meta.is_none() {
                                self.image_meta = Some(KittyImageMeta {
                                    format: kitty_format_str(format_val).to_string(),
                                    width,
                                    height,
                                    cols: if cols > 0 { cols } else { 10 },
                                    rows: if rows > 0 { rows } else { 5 },
                                });
                            }
                            self.image_buf.extend_from_slice(&decoded);
                        } else {
                            // Final (or only) chunk
                            let mut full_data = std::mem::take(&mut self.image_buf);
                            full_data.extend_from_slice(&decoded);

                            let meta = self.image_meta.take().unwrap_or(KittyImageMeta {
                                format: kitty_format_str(format_val).to_string(),
                                width,
                                height,
                                cols: if cols > 0 { cols } else { 10 },
                                rows: if rows > 0 { rows } else { 5 },
                            });

                            if !full_data.is_empty() {
                                let id = self.next_image_id;
                                self.next_image_id += 1;
                                log::info!(
                                    "kitty image #{id}: {}x{} pixels, {} cells, {}x{} grid, {} bytes",
                                    meta.width,
                                    meta.height,
                                    meta.format,
                                    meta.cols,
                                    meta.rows,
                                    full_data.len()
                                );
                                let data = Arc::new(full_data);
                                let placement = ImagePlacement {
                                    id,
                                    row: cursor_row,
                                    col: cursor_col,
                                    width_cells: meta.cols,
                                    height_cells: meta.rows,
                                    pixel_width: meta.width,
                                    pixel_height: meta.height,
                                    format: meta.format,
                                    data,
                                };
                                active_images.push(placement.clone());
                                new_placements.push(placement);
                            }
                        }
                    }
                    'd' => {
                        // Delete images: clear active set and any placements
                        // queued earlier in this scan (they're already stale).
                        active_images.clear();
                        new_placements.clear();
                        deleted = true;
                    }
                    _ => {}
                }

                i = end + 2; // skip past ESC \
            } else {
                i += 1;
            }
        }

        KittyScanResult {
            placements: new_placements,
            deleted,
        }
    }
}
