use std::sync::Arc;

use winnow::combinator::separated;
use winnow::prelude::*;
use winnow::token::take_while;

use crate::esc_scanner;
use crate::pane::ImagePlacement;
use crate::partial_buf::PartialBuf;

const MAX_APC_PARTIAL_SIZE: usize = 16 * 1024 * 1024; // 16MB

/// Kitty image metadata parsed from the first chunk of a transmission.
#[derive(Debug, Clone)]
struct KittyImageMeta {
    format: &'static str,
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

/// Split a kitty payload into (control, base64_data) at the first `;`.
fn split_payload<'a>(input: &mut &'a [u8]) -> winnow::error::ModalResult<(&'a [u8], &'a [u8])> {
    let control = winnow::token::take_till(0.., |b: u8| b == b';').parse_next(input)?;
    let data = if winnow::combinator::opt(b';').parse_next(input)?.is_some() {
        winnow::token::rest.parse_next(input)?
    } else {
        &[] as &[u8]
    };
    Ok((control, data))
}

/// Result of scanning PTY data for Kitty graphics sequences.
pub(crate) struct KittyScanResult {
    /// Newly created image placements in this scan.
    pub placements: Vec<ImagePlacement>,
    /// True if a delete command (`a=d`) was encountered, meaning the caller
    /// should also clear any previously queued pending images.
    pub deleted: bool,
}

// ---------------------------------------------------------------------------
// Winnow parsers for the control portion of a Kitty graphics payload
// ---------------------------------------------------------------------------

/// A single key=value pair from the control string.
#[derive(Debug)]
struct KvPair<'a> {
    key: &'a str,
    value: &'a str,
}

/// Parse one `key=value` pair where key is alphabetic and value runs until `,`
/// or end of input.
fn kv_pair<'a>(input: &mut &'a str) -> ModalResult<KvPair<'a>> {
    let key = take_while(1.., |c: char| c.is_ascii_alphabetic()).parse_next(input)?;
    '='.parse_next(input)?;
    let value = take_while(0.., |c: char| c != ',').parse_next(input)?;
    Ok(KvPair { key, value })
}

/// Parse comma-separated key=value pairs: `a=t,f=100,s=200,...`
fn kv_pairs<'a>(input: &mut &'a str) -> ModalResult<Vec<KvPair<'a>>> {
    separated(0.., kv_pair, ',').parse_next(input)
}

/// Parsed fields extracted from the control string.
struct ControlFields {
    action: char,
    format_val: u32,
    width: u32,
    height: u32,
    cols: u16,
    rows: u16,
    more_chunks: bool,
}

/// Parse a control string into structured fields.
fn parse_control(control: &str) -> ControlFields {
    let mut fields = ControlFields {
        action: 'T',
        format_val: 32,
        width: 0,
        height: 0,
        cols: 0,
        rows: 0,
        more_chunks: false,
    };

    if let Ok(pairs) = kv_pairs.parse(control) {
        for pair in &pairs {
            match pair.key {
                "a" => fields.action = pair.value.chars().next().unwrap_or('T'),
                "f" => fields.format_val = pair.value.parse().unwrap_or(32),
                "s" => fields.width = pair.value.parse().unwrap_or(0),
                "v" => fields.height = pair.value.parse().unwrap_or(0),
                "c" => fields.cols = pair.value.parse().unwrap_or(0),
                "r" => fields.rows = pair.value.parse().unwrap_or(0),
                "m" => fields.more_chunks = pair.value == "1",
                _ => {}
            }
        }
    }

    fields
}

/// Parser for Kitty graphics protocol APC sequences (ESC _ G ... ESC \).
/// Handles multi-chunk transmissions and sequences split across PTY reads.
pub(crate) struct KittyGraphicsParser {
    image_buf: Vec<u8>,
    image_meta: Option<KittyImageMeta>,
    apc_partial: PartialBuf,
    next_image_id: u64,
}

impl ControlFields {
    fn to_meta(&self) -> KittyImageMeta {
        KittyImageMeta {
            format: kitty_format_str(self.format_val),
            width: self.width,
            height: self.height,
            cols: if self.cols > 0 { self.cols } else { 10 },
            rows: if self.rows > 0 { self.rows } else { 5 },
        }
    }
}

impl KittyGraphicsParser {
    pub fn new() -> Self {
        KittyGraphicsParser {
            image_buf: Vec::new(),
            image_meta: None,
            apc_partial: PartialBuf::new(MAX_APC_PARTIAL_SIZE, "kitty APC"),
            next_image_id: 1,
        }
    }

    pub fn scan(
        &mut self,
        data: &[u8],
        cursor_col: u16,
        cursor_row: u16,
        active_images: &mut Vec<ImagePlacement>,
    ) -> KittyScanResult {
        use base64::Engine;

        let mut tmp = Vec::new();
        let data = self.apc_partial.prepend_to(data, &mut tmp);

        let scan = esc_scanner::scan_apc_kitty(data);

        let mut new_placements = Vec::new();
        let mut deleted = false;

        for (_offset, payload) in &scan.sequences {
            // Split payload into control + base64 data via winnow
            let mut input: &[u8] = payload;
            let (control_bytes, img_data_bytes) =
                match split_payload.parse_next(&mut input) {
                    Ok(pair) => pair,
                    Err(_) => continue,
                };

            let control_str = String::from_utf8_lossy(control_bytes);
            let fields = parse_control(&control_str);

            let decoded = base64::engine::general_purpose::STANDARD
                .decode(img_data_bytes)
                .unwrap_or_default();

            match fields.action {
                'T' | 't' => {
                    if fields.more_chunks {
                        if self.image_meta.is_none() {
                            self.image_meta = Some(fields.to_meta());
                        }
                        self.image_buf.extend_from_slice(&decoded);
                    } else {
                        let mut full_data = std::mem::take(&mut self.image_buf);
                        full_data.extend_from_slice(&decoded);

                        let meta = self.image_meta.take().unwrap_or_else(|| fields.to_meta());

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
                            let placement = ImagePlacement {
                                id,
                                row: cursor_row,
                                col: cursor_col,
                                width_cells: meta.cols,
                                height_cells: meta.rows,
                                pixel_width: meta.width,
                                pixel_height: meta.height,
                                format: meta.format.to_string(),
                                data: Arc::new(full_data),
                            };
                            active_images.push(placement.clone());
                            new_placements.push(placement);
                        }
                    }
                }
                'd' => {
                    active_images.clear();
                    new_placements.clear();
                    deleted = true;
                }
                _ => {}
            }
        }

        if let Some(partial_start) = scan.partial_start {
            self.apc_partial.store(&data[partial_start..]);
        }

        KittyScanResult {
            placements: new_placements,
            deleted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragmented_transmit_keeps_image_until_final_chunk() {
        let mut parser = KittyGraphicsParser::new();
        let mut active_images = Vec::new();

        let partial = parser.scan(
            b"\x1b_Ga=T,f=24,s=2,v=2,c=1,r=1,m=1;AQ",
            3,
            4,
            &mut active_images,
        );
        assert!(partial.placements.is_empty());
        assert!(!partial.deleted);
        assert!(!parser.apc_partial.is_empty());
        assert!(active_images.is_empty());

        let complete = parser.scan(b"ID\x1b\\\x1b_Gm=0;BAU=\x1b\\", 3, 4, &mut active_images);
        assert_eq!(complete.placements.len(), 1);
        assert_eq!(active_images.len(), 1);
        let placement = &complete.placements[0];
        assert_eq!(placement.col, 3);
        assert_eq!(placement.row, 4);
        assert_eq!(placement.width_cells, 1);
        assert_eq!(placement.height_cells, 1);
        assert_eq!(placement.pixel_width, 2);
        assert_eq!(placement.pixel_height, 2);
        assert_eq!(placement.format, "rgb");
        assert_eq!(placement.data.as_slice(), &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn delete_clears_active_and_pending_images() {
        let mut parser = KittyGraphicsParser::new();
        let mut active_images = Vec::new();

        let created = parser.scan(
            b"\x1b_Ga=T,f=24,s=1,v=1,c=1,r=1;AQID\x1b\\",
            0,
            0,
            &mut active_images,
        );
        assert_eq!(created.placements.len(), 1);
        assert_eq!(active_images.len(), 1);

        let deleted = parser.scan(b"\x1b_Ga=d\x1b\\", 0, 0, &mut active_images);
        assert!(deleted.deleted);
        assert!(deleted.placements.is_empty());
        assert!(active_images.is_empty());
    }

    #[test]
    fn oversized_partial_is_discarded() {
        let mut parser = KittyGraphicsParser::new();
        let mut active_images = Vec::new();
        let mut data = vec![0x1b, b'_', b'G'];
        data.extend(std::iter::repeat_n(b'a', MAX_APC_PARTIAL_SIZE + 1));

        let result = parser.scan(&data, 0, 0, &mut active_images);
        assert!(result.placements.is_empty());
        assert!(!result.deleted);
        assert!(parser.apc_partial.is_empty());
    }
}
