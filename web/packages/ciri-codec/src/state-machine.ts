// State-machine cell decoder — mirrors `decode_sm_cells` in
// `crates/ciri-protocol/src/codec/state_machine.rs`. The opcode bytes
// and their argument layout are stable wire contract; round-trip tests
// against Rust-generated fixtures live in `./state-machine.test.ts`.
//
// Every opcode reads its arguments from `data` starting at `pos`, may
// produce zero or more `PackedCell`s into the caller's output slice,
// and advances `pos`. On unexpected input the decoder throws — the
// caller (`accept_ws`'s handle path) tears the connection down.

import {
  DEFAULT_CELL_FLAGS,
  OP_ASCII,
  OP_ASCII_REPEAT,
  OP_CHAR1,
  OP_CHARS,
  OP_CHARS_LONG,
  OP_END,
  OP_REPEAT,
  OP_RESET,
  OP_SET_BG,
  OP_SET_BG_INDEXED,
  OP_SET_BG_NAMED,
  OP_SET_FG,
  OP_SET_FG_BG,
  OP_SET_FG_INDEXED,
  OP_SET_FG_NAMED,
  OP_SET_FLAGS,
} from "./constants.js";
import {
  DEFAULT_BG,
  DEFAULT_FG,
  type PackedCell,
  type PackedColor,
  decodePackedColor,
} from "./types.js";

/// Decode a UTF-8 codepoint from a fixed 4-byte cell-character field.
/// Trailing zeros (the common case for ASCII) are stripped before
/// decoding so the resulting string is the single grapheme primary —
/// combining marks live in the FullPaneSync grapheme-extras map.
const utf8Decoder = new TextDecoder("utf-8", { fatal: false });
function decodeCellChar(bytes: Uint8Array): string {
  // Strip trailing NULs. Rust writes the codepoint encoded as UTF-8
  // into a fixed 4-byte slot, zero-padded.
  let len = 4;
  while (len > 0 && bytes[len - 1] === 0) len -= 1;
  if (len === 0) return "\0";
  return utf8Decoder.decode(bytes.subarray(0, len));
}

/// Decode an SM opcode stream into exactly `expected` cells. Throws if
/// the stream is malformed, the cell count mismatches `expected`, or
/// the buffer is truncated mid-opcode.
export function decodeSmCells(
  data: Uint8Array,
  expected: number,
): PackedCell[] {
  const cells: PackedCell[] = new Array(expected);
  const written = decodeSmCellsInto(data, cells, expected);
  if (written !== expected) {
    throw new Error(
      `SM decoded ${written} cells, expected ${expected}`,
    );
  }
  return cells;
}

/// Lower-level entry point: write into a pre-allocated array up to
/// `limit` cells and return the actual count produced. Used by
/// CellDelta (per-region) where each region knows its own width.
export function decodeSmCellsInto(
  data: Uint8Array,
  cells: PackedCell[],
  limit: number,
): number {
  let fg: PackedColor = DEFAULT_FG;
  let bg: PackedColor = DEFAULT_BG;
  let flags = DEFAULT_CELL_FLAGS;
  let pos = 0;
  let ci = 0;

  const view = new DataView(
    data.buffer,
    data.byteOffset,
    data.byteLength,
  );

  // Helpers — read N bytes, advancing `pos`, with a truncation guard.
  const need = (n: number, ctx: string) => {
    if (pos + n > data.length) {
      throw new Error(`truncated SM opcode: ${ctx} (pos=${pos}, need=${n}, len=${data.length})`);
    }
  };
  const writeCell = (ch: string) => {
    if (ci >= limit) {
      throw new Error("SM decoded more cells than output buffer");
    }
    cells[ci] = { ch, fg, bg, flags };
    ci += 1;
  };

  while (pos < data.length) {
    const op = data[pos];
    pos += 1;

    switch (op) {
      case OP_SET_FG: {
        need(4, "SetFg");
        const t = data[pos]!,
          a = data[pos + 1]!,
          b = data[pos + 2]!,
          c = data[pos + 3]!;
        fg = decodePackedColor(t, a, b, c);
        pos += 4;
        break;
      }
      case OP_SET_BG: {
        need(4, "SetBg");
        const t = data[pos]!,
          a = data[pos + 1]!,
          b = data[pos + 2]!,
          c = data[pos + 3]!;
        bg = decodePackedColor(t, a, b, c);
        pos += 4;
        break;
      }
      case OP_SET_FLAGS: {
        need(2, "SetFlags");
        flags = view.getUint16(pos, /* le */ true);
        pos += 2;
        break;
      }
      case OP_SET_FG_BG: {
        need(8, "SetFgBg");
        fg = decodePackedColor(
          data[pos]!,
          data[pos + 1]!,
          data[pos + 2]!,
          data[pos + 3]!,
        );
        bg = decodePackedColor(
          data[pos + 4]!,
          data[pos + 5]!,
          data[pos + 6]!,
          data[pos + 7]!,
        );
        pos += 8;
        break;
      }
      case OP_RESET: {
        fg = DEFAULT_FG;
        bg = DEFAULT_BG;
        flags = DEFAULT_CELL_FLAGS;
        break;
      }
      case OP_CHAR1: {
        need(4, "Char1");
        const ch = decodeCellChar(data.subarray(pos, pos + 4));
        pos += 4;
        writeCell(ch);
        break;
      }
      case OP_CHARS: {
        need(1, "Chars count");
        const count = data[pos]!;
        pos += 1;
        need(count * 4, "Chars data");
        for (let i = 0; i < count; i += 1) {
          const off = pos + i * 4;
          writeCell(decodeCellChar(data.subarray(off, off + 4)));
        }
        pos += count * 4;
        break;
      }
      case OP_REPEAT: {
        need(6, "Repeat");
        const count = view.getUint16(pos, /* le */ true);
        pos += 2;
        const ch = decodeCellChar(data.subarray(pos, pos + 4));
        pos += 4;
        for (let i = 0; i < count; i += 1) writeCell(ch);
        break;
      }
      case OP_CHARS_LONG: {
        need(2, "CharsLong count");
        const count = view.getUint16(pos, /* le */ true);
        pos += 2;
        need(count * 4, "CharsLong data");
        for (let i = 0; i < count; i += 1) {
          const off = pos + i * 4;
          writeCell(decodeCellChar(data.subarray(off, off + 4)));
        }
        pos += count * 4;
        break;
      }
      case OP_ASCII: {
        need(1, "Ascii count");
        const count = data[pos]!;
        pos += 1;
        need(count, "Ascii data");
        for (let i = 0; i < count; i += 1) {
          // Each ASCII byte expands to a one-codepoint string.
          writeCell(String.fromCharCode(data[pos + i]!));
        }
        pos += count;
        break;
      }
      case OP_ASCII_REPEAT: {
        need(3, "AsciiRepeat");
        const count = view.getUint16(pos, /* le */ true);
        pos += 2;
        const ch = String.fromCharCode(data[pos]!);
        pos += 1;
        for (let i = 0; i < count; i += 1) writeCell(ch);
        break;
      }
      case OP_SET_FG_NAMED: {
        need(1, "SetFgNamed");
        fg = { kind: "named", index: data[pos]! };
        pos += 1;
        break;
      }
      case OP_SET_BG_NAMED: {
        need(1, "SetBgNamed");
        bg = { kind: "named", index: data[pos]! };
        pos += 1;
        break;
      }
      case OP_SET_FG_INDEXED: {
        need(1, "SetFgIndexed");
        fg = { kind: "indexed", index: data[pos]! };
        pos += 1;
        break;
      }
      case OP_SET_BG_INDEXED: {
        need(1, "SetBgIndexed");
        bg = { kind: "indexed", index: data[pos]! };
        pos += 1;
        break;
      }
      case OP_END:
        return ci;
      default:
        throw new Error(`unknown SM opcode: 0x${(op ?? 0).toString(16)}`);
    }
  }

  return ci;
}
