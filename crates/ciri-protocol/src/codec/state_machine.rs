//! State-machine cell encoder and decoder.

use crate::message::*;
use std::io;

// ─── Opcode constants ────────────────────────────────────────────────

pub(super) const OP_SET_FG: u8 = 0x01;
pub(super) const OP_SET_BG: u8 = 0x02;
pub(super) const OP_SET_FLAGS: u8 = 0x03;
pub(super) const OP_SET_FG_BG: u8 = 0x04;
pub(super) const OP_RESET: u8 = 0x05;

pub(super) const OP_CHAR1: u8 = 0x10;
pub(super) const OP_CHARS: u8 = 0x11;
pub(super) const OP_REPEAT: u8 = 0x12;
pub(super) const OP_CHARS_LONG: u8 = 0x13;
/// ASCII run: 1-byte count + N raw ASCII bytes (1 byte each instead of 4).
pub(super) const OP_ASCII: u8 = 0x14;
/// Single ASCII repeat: 2-byte count + 1 ASCII byte.
pub(super) const OP_ASCII_REPEAT: u8 = 0x15;

/// Compact color opcodes: Named/Indexed use 2 bytes instead of 4.
pub(super) const OP_SET_FG_NAMED: u8 = 0x06;
pub(super) const OP_SET_BG_NAMED: u8 = 0x07;
pub(super) const OP_SET_FG_INDEXED: u8 = 0x08;
pub(super) const OP_SET_BG_INDEXED: u8 = 0x09;

pub(super) const OP_END: u8 = 0xFF;

// ─── Encoder ─────────────────────────────────────────────────────────

/// Encodes a stream of `PackedCell`s into a compact opcode stream.
pub struct StateEncoder {
    cur_fg: PackedColor,
    cur_bg: PackedColor,
    cur_flags: u16,
    run_ch: Option<[u8; 4]>,
    run_count: u16,
    char_buf: Vec<[u8; 4]>,
    out: Vec<u8>,
}

fn default_cell_state() -> (PackedColor, PackedColor, u16) {
    (DEFAULT_FOREGROUND, DEFAULT_BACKGROUND, DEFAULT_CELL_FLAGS)
}

/// Check if a char encoded as [u8; 4] is a single ASCII byte (non-NUL).
fn is_ascii_char(ch: &[u8; 4]) -> bool {
    ch[0] > 0 && ch[0] < 0x80 && ch[1] == 0 && ch[2] == 0 && ch[3] == 0
}

/// Emit a compact color-set opcode (Named/Indexed use 2 bytes, RGB uses 5 bytes).
fn emit_compact_fg(out: &mut Vec<u8>, color: &PackedColor) {
    match color.tag {
        COLOR_NAMED => {
            out.push(OP_SET_FG_NAMED);
            out.push(color.b1);
        }
        COLOR_INDEXED => {
            out.push(OP_SET_FG_INDEXED);
            out.push(color.b1);
        }
        _ => {
            out.push(OP_SET_FG);
            out.extend_from_slice(bytemuck::bytes_of(color));
        }
    }
}

fn emit_compact_bg(out: &mut Vec<u8>, color: &PackedColor) {
    match color.tag {
        COLOR_NAMED => {
            out.push(OP_SET_BG_NAMED);
            out.push(color.b1);
        }
        COLOR_INDEXED => {
            out.push(OP_SET_BG_INDEXED);
            out.push(color.b1);
        }
        _ => {
            out.push(OP_SET_BG);
            out.extend_from_slice(bytemuck::bytes_of(color));
        }
    }
}

impl StateEncoder {
    pub fn new() -> Self {
        let (fg, bg, flags) = default_cell_state();
        Self {
            cur_fg: fg,
            cur_bg: bg,
            cur_flags: flags,
            run_ch: None,
            run_count: 0,
            char_buf: Vec::new(),
            out: Vec::with_capacity(256),
        }
    }

    pub fn push_cell(&mut self, cell: &PackedCell) {
        let fg = cell.fg;
        let bg = cell.bg;
        let flags = cell.flags_u16();

        if fg != self.cur_fg || bg != self.cur_bg || flags != self.cur_flags {
            self.flush_run();
            self.flush_char_buf();

            let target_is_default =
                fg == DEFAULT_FOREGROUND && bg == DEFAULT_BACKGROUND && flags == DEFAULT_CELL_FLAGS;

            if target_is_default {
                self.out.push(OP_RESET);
            } else {
                let fg_changed = fg != self.cur_fg;
                let bg_changed = bg != self.cur_bg;

                // Use OP_SET_FG_BG only when both are RGB (saves nothing
                // for Named/Indexed where compact opcodes are shorter).
                if fg_changed && bg_changed && fg.tag == COLOR_RGB && bg.tag == COLOR_RGB {
                    self.out.push(OP_SET_FG_BG);
                    self.out.extend_from_slice(bytemuck::bytes_of(&fg));
                    self.out.extend_from_slice(bytemuck::bytes_of(&bg));
                } else {
                    if fg_changed {
                        emit_compact_fg(&mut self.out, &fg);
                    }
                    if bg_changed {
                        emit_compact_bg(&mut self.out, &bg);
                    }
                }

                if flags != self.cur_flags {
                    self.out.push(OP_SET_FLAGS);
                    self.out.extend_from_slice(&flags.to_le_bytes());
                }
            }

            self.cur_fg = fg;
            self.cur_bg = bg;
            self.cur_flags = flags;
        }

        let ch = cell.ch_bytes;
        if let Some(run_ch) = self.run_ch {
            if run_ch == ch {
                self.run_count += 1;
                if self.run_count == u16::MAX {
                    self.flush_char_buf();
                    self.emit_repeat(self.run_count, &run_ch);
                    self.run_ch = Some(ch);
                    self.run_count = 0;
                }
                return;
            }
            if self.run_count >= 3 {
                self.flush_char_buf();
                self.emit_repeat(self.run_count, &run_ch);
            } else {
                for _ in 0..self.run_count {
                    self.char_buf.push(run_ch);
                }
            }
        }
        self.run_ch = Some(ch);
        self.run_count = 1;
    }

    fn emit_repeat(&mut self, count: u16, ch: &[u8; 4]) {
        if is_ascii_char(ch) {
            // OP_ASCII_REPEAT: 1 opcode + 2 count + 1 char = 4 bytes
            // vs OP_REPEAT:    1 opcode + 2 count + 4 char = 7 bytes
            self.out.push(OP_ASCII_REPEAT);
            self.out.extend_from_slice(&count.to_le_bytes());
            self.out.push(ch[0]);
        } else {
            self.out.push(OP_REPEAT);
            self.out.extend_from_slice(&count.to_le_bytes());
            self.out.extend_from_slice(ch);
        }
    }

    fn flush_run(&mut self) {
        if let Some(run_ch) = self.run_ch.take() {
            if self.run_count >= 3 {
                self.flush_char_buf();
                self.emit_repeat(self.run_count, &run_ch);
            } else {
                for _ in 0..self.run_count {
                    self.char_buf.push(run_ch);
                }
            }
            self.run_count = 0;
        }
    }

    fn flush_char_buf(&mut self) {
        if self.char_buf.is_empty() {
            return;
        }

        // Split the buffer into ASCII and non-ASCII runs for optimal encoding.
        let mut i = 0;
        while i < self.char_buf.len() {
            if is_ascii_char(&self.char_buf[i]) {
                // Gather contiguous ASCII chars
                let start = i;
                while i < self.char_buf.len() && is_ascii_char(&self.char_buf[i]) {
                    i += 1;
                }
                let n = i - start;
                if n == 1 {
                    // Single ASCII: OP_CHAR1 (5B) vs OP_ASCII (2+1=3B min)
                    // Use OP_ASCII even for 1 char (saves 2 bytes).
                    self.out.push(OP_ASCII);
                    self.out.push(1);
                    self.out.push(self.char_buf[start][0]);
                } else {
                    // OP_ASCII: 1 opcode + 1 count + N bytes = N+2 bytes
                    // vs OP_CHARS: 1 opcode + 1 count + N*4 bytes
                    let count = n.min(255);
                    self.out.push(OP_ASCII);
                    self.out.push(count as u8);
                    for j in start..start + count {
                        self.out.push(self.char_buf[j][0]);
                    }
                    // Handle overflow (>255 ASCII chars in a row)
                    if n > 255 {
                        i = start + 255; // will be picked up in next iteration
                        continue;
                    }
                }
            } else {
                // Gather contiguous non-ASCII chars
                let start = i;
                while i < self.char_buf.len() && !is_ascii_char(&self.char_buf[i]) {
                    i += 1;
                }
                let n = i - start;
                if n == 1 {
                    self.out.push(OP_CHAR1);
                    self.out.extend_from_slice(&self.char_buf[start]);
                } else if n <= 255 {
                    self.out.push(OP_CHARS);
                    self.out.push(n as u8);
                    for j in start..start + n {
                        self.out.extend_from_slice(&self.char_buf[j]);
                    }
                } else {
                    self.out.push(OP_CHARS_LONG);
                    self.out.extend_from_slice(&(n as u16).to_le_bytes());
                    for j in start..start + n {
                        self.out.extend_from_slice(&self.char_buf[j]);
                    }
                }
            }
        }
        self.char_buf.clear();
    }

    pub fn finish(&mut self) -> &[u8] {
        self.flush_run();
        self.flush_char_buf();
        self.out.push(OP_END);
        &self.out
    }

    pub fn reset(&mut self) {
        let (fg, bg, flags) = default_cell_state();
        self.cur_fg = fg;
        self.cur_bg = bg;
        self.cur_flags = flags;
        self.run_ch = None;
        self.run_count = 0;
        self.char_buf.clear();
        self.out.clear();
    }
}

impl Default for StateEncoder {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Decoder ─────────────────────────────────────────────────────────

/// Decode an SM opcode stream, writing cells into the provided slice.
pub fn decode_sm_cells(data: &[u8], cells: &mut [PackedCell]) -> io::Result<usize> {
    let (mut fg, mut bg, mut flags) = default_cell_state();
    let mut pos = 0;
    let mut ci = 0;

    while pos < data.len() {
        let op = data[pos];
        pos += 1;

        match op {
            OP_SET_FG => {
                if pos + 4 > data.len() {
                    return Err(truncated_err("SetFg"));
                }
                fg = *bytemuck::from_bytes::<PackedColor>(&data[pos..pos + 4]);
                pos += 4;
            }
            OP_SET_BG => {
                if pos + 4 > data.len() {
                    return Err(truncated_err("SetBg"));
                }
                bg = *bytemuck::from_bytes::<PackedColor>(&data[pos..pos + 4]);
                pos += 4;
            }
            OP_SET_FLAGS => {
                if pos + 2 > data.len() {
                    return Err(truncated_err("SetFlags"));
                }
                flags = u16::from_le_bytes([data[pos], data[pos + 1]]);
                pos += 2;
            }
            OP_SET_FG_BG => {
                if pos + 8 > data.len() {
                    return Err(truncated_err("SetFgBg"));
                }
                fg = *bytemuck::from_bytes::<PackedColor>(&data[pos..pos + 4]);
                bg = *bytemuck::from_bytes::<PackedColor>(&data[pos + 4..pos + 8]);
                pos += 8;
            }
            OP_RESET => {
                (fg, bg, flags) = default_cell_state();
            }
            OP_CHAR1 => {
                if pos + 4 > data.len() {
                    return Err(truncated_err("Char1"));
                }
                if ci >= cells.len() {
                    return Err(overflow_err());
                }
                cells[ci] = PackedCell {
                    ch_bytes: [data[pos], data[pos + 1], data[pos + 2], data[pos + 3]],
                    fg,
                    bg,
                    flags: flags.to_le_bytes(),
                    _pad: [0; 2],
                };
                ci += 1;
                pos += 4;
            }
            OP_CHARS => {
                if pos + 1 > data.len() {
                    return Err(truncated_err("Chars count"));
                }
                let count = data[pos] as usize;
                pos += 1;
                if pos + count * 4 > data.len() {
                    return Err(truncated_err("Chars data"));
                }
                if ci + count > cells.len() {
                    return Err(overflow_err());
                }
                let flags_le = flags.to_le_bytes();
                let dest = &mut cells[ci..ci + count];
                for (i, cell) in dest.iter_mut().enumerate() {
                    let off = pos + i * 4;
                    *cell = PackedCell {
                        ch_bytes: [data[off], data[off + 1], data[off + 2], data[off + 3]],
                        fg,
                        bg,
                        flags: flags_le,
                        _pad: [0; 2],
                    };
                }
                ci += count;
                pos += count * 4;
            }
            OP_REPEAT => {
                if pos + 6 > data.len() {
                    return Err(truncated_err("Repeat"));
                }
                let count = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
                pos += 2;
                let ch_bytes = [data[pos], data[pos + 1], data[pos + 2], data[pos + 3]];
                pos += 4;
                if ci + count > cells.len() {
                    return Err(overflow_err());
                }
                let cell = PackedCell {
                    ch_bytes,
                    fg,
                    bg,
                    flags: flags.to_le_bytes(),
                    _pad: [0; 2],
                };
                fill_cells(&mut cells[ci..ci + count], cell);
                ci += count;
            }
            OP_CHARS_LONG => {
                if pos + 2 > data.len() {
                    return Err(truncated_err("CharsLong count"));
                }
                let count = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
                pos += 2;
                if pos + count * 4 > data.len() {
                    return Err(truncated_err("CharsLong data"));
                }
                if ci + count > cells.len() {
                    return Err(overflow_err());
                }
                let flags_le = flags.to_le_bytes();
                let dest = &mut cells[ci..ci + count];
                for (i, cell) in dest.iter_mut().enumerate() {
                    let off = pos + i * 4;
                    *cell = PackedCell {
                        ch_bytes: [data[off], data[off + 1], data[off + 2], data[off + 3]],
                        fg,
                        bg,
                        flags: flags_le,
                        _pad: [0; 2],
                    };
                }
                ci += count;
                pos += count * 4;
            }
            OP_ASCII => {
                if pos + 1 > data.len() {
                    return Err(truncated_err("Ascii count"));
                }
                let count = data[pos] as usize;
                pos += 1;
                if pos + count > data.len() {
                    return Err(truncated_err("Ascii data"));
                }
                if ci + count > cells.len() {
                    return Err(overflow_err());
                }
                let flags_le = flags.to_le_bytes();
                let dest = &mut cells[ci..ci + count];
                let src = &data[pos..pos + count];
                for (cell, &byte) in dest.iter_mut().zip(src.iter()) {
                    *cell = PackedCell {
                        ch_bytes: [byte, 0, 0, 0],
                        fg,
                        bg,
                        flags: flags_le,
                        _pad: [0; 2],
                    };
                }
                ci += count;
                pos += count;
            }
            OP_ASCII_REPEAT => {
                if pos + 3 > data.len() {
                    return Err(truncated_err("AsciiRepeat"));
                }
                let count = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
                pos += 2;
                let ch_byte = data[pos];
                pos += 1;
                if ci + count > cells.len() {
                    return Err(overflow_err());
                }
                let cell = PackedCell {
                    ch_bytes: [ch_byte, 0, 0, 0],
                    fg,
                    bg,
                    flags: flags.to_le_bytes(),
                    _pad: [0; 2],
                };
                fill_cells(&mut cells[ci..ci + count], cell);
                ci += count;
            }
            OP_SET_FG_NAMED => {
                if pos + 1 > data.len() {
                    return Err(truncated_err("SetFgNamed"));
                }
                fg = PackedColor::named(data[pos]);
                pos += 1;
            }
            OP_SET_BG_NAMED => {
                if pos + 1 > data.len() {
                    return Err(truncated_err("SetBgNamed"));
                }
                bg = PackedColor::named(data[pos]);
                pos += 1;
            }
            OP_SET_FG_INDEXED => {
                if pos + 1 > data.len() {
                    return Err(truncated_err("SetFgIndexed"));
                }
                fg = PackedColor::indexed(data[pos]);
                pos += 1;
            }
            OP_SET_BG_INDEXED => {
                if pos + 1 > data.len() {
                    return Err(truncated_err("SetBgIndexed"));
                }
                bg = PackedColor::indexed(data[pos]);
                pos += 1;
            }
            OP_END => break,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unknown SM opcode: 0x{op:02x}"),
                ));
            }
        }
    }
    Ok(ci)
}

// ─── Helpers ─────────────────────────────────────────────────────────

pub(super) fn sm_encode_cells(cells: &[PackedCell]) -> Vec<u8> {
    let mut enc = StateEncoder::new();
    for cell in cells {
        enc.push_cell(cell);
    }
    enc.finish().to_vec()
}

pub(super) fn sm_decode_cells_vec(data: &[u8], expected: usize) -> io::Result<Vec<PackedCell>> {
    let mut cells = vec![PackedCell::default(); expected];
    let decoded = decode_sm_cells(data, &mut cells)?;
    if decoded != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected {expected} cells but SM decoded {decoded}"),
        ));
    }
    Ok(cells)
}

/// Fill a cell slice using 128-bit stores (PackedCell is 16 bytes = u128).
#[inline]
fn fill_cells(dest: &mut [PackedCell], cell: PackedCell) {
    // SAFETY: PackedCell is 16 bytes, Pod, align(16) — same layout as u128.
    let val: u128 = bytemuck::cast(cell);
    let dest_u128: &mut [u128] = bytemuck::cast_slice_mut(dest);
    dest_u128.fill(val);
}

fn truncated_err(ctx: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("truncated SM opcode: {ctx}"),
    )
}

fn overflow_err() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "SM decoded more cells than output buffer",
    )
}
