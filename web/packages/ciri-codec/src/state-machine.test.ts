// Per-opcode tests for the SM cell decoder. The bytes here are
// hand-constructed to match `crates/ciri-protocol/src/codec/state_machine.rs`
// exactly; if the Rust encoder ever changes a wire layout, regenerate
// `constants.ts` and update these expectations.

import { describe, expect, test } from "vitest";
import {
  DEFAULT_CELL_FLAGS,
  FLAG_BOLD,
  FLAG_UNDERLINE,
  NAMED_FOREGROUND,
  NAMED_RED,
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
} from "./types.js";
import { decodeSmCells } from "./state-machine.js";

function bytes(...vs: number[]): Uint8Array {
  return new Uint8Array(vs);
}

const NAMED_FG_COLOR: PackedColor = {
  kind: "named",
  index: NAMED_FOREGROUND,
};
const NAMED_RED_COLOR: PackedColor = { kind: "named", index: NAMED_RED };

function expectCell(cell: PackedCell, ch: string, fg = DEFAULT_FG, bg = DEFAULT_BG, flags = 0) {
  expect(cell.ch).toBe(ch);
  expect(cell.fg).toEqual(fg);
  expect(cell.bg).toEqual(bg);
  expect(cell.flags).toBe(flags);
}

describe("decodeSmCells", () => {
  test("OP_CHAR1 emits one cell at default state", () => {
    const data = bytes(OP_CHAR1, 0x61, 0, 0, 0, OP_END);
    const cells = decodeSmCells(data, 1);
    expectCell(cells[0]!, "a");
  });

  test("OP_CHARS emits N cells with carried state", () => {
    const data = bytes(
      OP_CHARS,
      3, // count
      0x61, 0, 0, 0, // 'a'
      0x62, 0, 0, 0, // 'b'
      0x63, 0, 0, 0, // 'c'
      OP_END,
    );
    const cells = decodeSmCells(data, 3);
    expectCell(cells[0]!, "a");
    expectCell(cells[1]!, "b");
    expectCell(cells[2]!, "c");
  });

  test("OP_REPEAT emits the same cell N times (u16 count, LE)", () => {
    const data = bytes(
      OP_REPEAT,
      0x05, 0x00, // count = 5
      0x78, 0, 0, 0, // 'x'
      OP_END,
    );
    const cells = decodeSmCells(data, 5);
    for (const c of cells) expectCell(c, "x");
  });

  test("OP_CHARS_LONG handles counts > 255 (u16 count)", () => {
    const count = 300;
    const data: number[] = [OP_CHARS_LONG, count & 0xff, (count >> 8) & 0xff];
    for (let i = 0; i < count; i += 1) data.push(0x71, 0, 0, 0); // 'q'
    data.push(OP_END);
    const cells = decodeSmCells(bytes(...data), count);
    expect(cells).toHaveLength(count);
    expectCell(cells[0]!, "q");
    expectCell(cells[count - 1]!, "q");
  });

  test("OP_ASCII compresses 1-byte chars to 1 byte each", () => {
    const data = bytes(
      OP_ASCII,
      5,
      ..."hello".split("").map((c) => c.charCodeAt(0)),
      OP_END,
    );
    const cells = decodeSmCells(data, 5);
    expectCell(cells[0]!, "h");
    expectCell(cells[4]!, "o");
  });

  test("OP_ASCII_REPEAT (u16 count + 1 ASCII byte)", () => {
    const data = bytes(
      OP_ASCII_REPEAT,
      0x0a, 0x00, // count = 10
      0x20, // space
      OP_END,
    );
    const cells = decodeSmCells(data, 10);
    for (const c of cells) expectCell(c, " ");
  });

  test("OP_SET_FG_NAMED switches fg for subsequent cells", () => {
    const data = bytes(
      OP_SET_FG_NAMED, NAMED_RED,
      OP_CHAR1, 0x61, 0, 0, 0, // 'a' in red
      OP_END,
    );
    const cells = decodeSmCells(data, 1);
    expectCell(cells[0]!, "a", NAMED_RED_COLOR);
  });

  test("OP_SET_BG_INDEXED + OP_CHAR1", () => {
    const data = bytes(
      OP_SET_BG_INDEXED, 42,
      OP_CHAR1, 0x62, 0, 0, 0,
      OP_END,
    );
    const cells = decodeSmCells(data, 1);
    expectCell(cells[0]!, "b", DEFAULT_FG, { kind: "indexed", index: 42 });
  });

  test("OP_SET_FG (full PackedColor RGB)", () => {
    // tag=1 (RGB), b1..b3 = 0xff,0x88,0x00 → orange
    const data = bytes(
      OP_SET_FG, /* COLOR_RGB */ 1, 0xff, 0x88, 0x00,
      OP_CHAR1, 0x63, 0, 0, 0,
      OP_END,
    );
    const cells = decodeSmCells(data, 1);
    expectCell(cells[0]!, "c", { kind: "rgb", r: 0xff, g: 0x88, b: 0x00 });
  });

  test("OP_SET_FG_BG sets both colors in one opcode", () => {
    const data = bytes(
      OP_SET_FG_BG,
      0, NAMED_RED, 0, 0,        // fg: named red
      0, NAMED_FOREGROUND, 0, 0, // bg: named fg slot (just to test)
      OP_CHAR1, 0x64, 0, 0, 0,
      OP_END,
    );
    const cells = decodeSmCells(data, 1);
    expectCell(cells[0]!, "d", NAMED_RED_COLOR, NAMED_FG_COLOR);
  });

  test("OP_SET_FLAGS carries until reset", () => {
    const f = FLAG_BOLD | FLAG_UNDERLINE;
    const data = bytes(
      OP_SET_FLAGS, f & 0xff, (f >> 8) & 0xff,
      OP_CHAR1, 0x65, 0, 0, 0, // 'e' with flags
      OP_END,
    );
    const cells = decodeSmCells(data, 1);
    expectCell(cells[0]!, "e", DEFAULT_FG, DEFAULT_BG, f);
  });

  test("OP_RESET restores defaults", () => {
    const data = bytes(
      OP_SET_FG_NAMED, NAMED_RED,
      OP_SET_FLAGS, FLAG_BOLD, 0,
      OP_RESET,
      OP_CHAR1, 0x66, 0, 0, 0, // 'f' must be default again
      OP_END,
    );
    const cells = decodeSmCells(data, 1);
    expectCell(cells[0]!, "f", DEFAULT_FG, DEFAULT_BG, DEFAULT_CELL_FLAGS);
  });

  test("multi-byte UTF-8 char survives the 4-byte cell slot", () => {
    // '中' = E4 B8 AD in UTF-8 (3 bytes).
    const data = bytes(
      OP_CHAR1, 0xe4, 0xb8, 0xad, 0x00,
      OP_END,
    );
    const cells = decodeSmCells(data, 1);
    expectCell(cells[0]!, "中");
  });

  test("4-byte UTF-8 codepoint round-trips (emoji)", () => {
    // U+1F600 (grinning face) = F0 9F 98 80.
    const data = bytes(
      OP_CHAR1, 0xf0, 0x9f, 0x98, 0x80,
      OP_END,
    );
    const cells = decodeSmCells(data, 1);
    expectCell(cells[0]!, "\u{1F600}");
  });

  test("truncated opcode raises", () => {
    // OP_CHARS claims 3 cells but only supplies 2.
    const data = bytes(
      OP_CHARS, 3,
      0x61, 0, 0, 0,
      0x62, 0, 0, 0,
      // 3rd cell bytes missing
      OP_END,
    );
    expect(() => decodeSmCells(data, 3)).toThrow(/truncated|SM/);
  });

  test("unknown opcode raises", () => {
    const data = bytes(0xee, OP_END);
    expect(() => decodeSmCells(data, 0)).toThrow(/unknown SM opcode/);
  });

  test("cell-count mismatch raises", () => {
    const data = bytes(OP_CHAR1, 0x61, 0, 0, 0, OP_END);
    expect(() => decodeSmCells(data, 2)).toThrow(/expected 2/);
  });
});
