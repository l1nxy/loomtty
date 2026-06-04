// End-to-end smoke: FrameReader → SM decoder. Validates that the two
// layers compose — a real wire stream is built by framing an SM
// byte sequence produced by the Rust encoder (loaded from sm.json),
// pushed through FrameReader, and then the extracted payload is
// decoded back into cells.
//
// This isn't a CellDelta-shape test (the CellDelta payload has its
// own header that loom-codec doesn't yet decode — Phase 2.2). It is
// a frame-routing + SM-decoder integration check.

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";
import { TAG_CELL_DELTA } from "./constants.js";
import { FrameReader } from "./frame.js";
import { hexToBytes } from "./__fixtures__/hex.js";
import { decodeSmCells } from "./state-machine.js";
import type { PackedColor } from "./types.js";

interface SmFixtureCase {
  name: string;
  sm_bytes_hex: string;
  expected_cells: {
    ch: string;
    fg: [string, ...number[]];
    bg: [string, ...number[]];
    flags: number;
  }[];
}

const __dirname = dirname(fileURLToPath(import.meta.url));
const cases: SmFixtureCase[] = JSON.parse(
  readFileSync(join(__dirname, "__fixtures__", "sm.json"), "utf-8"),
);

function frame(tag: number, payload: Uint8Array): Uint8Array {
  const out = new Uint8Array(5 + payload.length);
  out[0] = tag;
  new DataView(out.buffer).setUint32(1, payload.length, /* le */ true);
  out.set(payload, 5);
  return out;
}

function colorFromJson(arr: [string, ...number[]]): PackedColor {
  function num(i: number): number {
    const v = arr[i];
    if (typeof v !== "number") {
      throw new Error(
        `malformed color fixture: expected number at index ${i} of ${JSON.stringify(arr)}`,
      );
    }
    return v;
  }
  switch (arr[0]) {
    case "named":
      return { kind: "named", index: num(1) };
    case "indexed":
      return { kind: "indexed", index: num(1) };
    case "rgb":
      return { kind: "rgb", r: num(1), g: num(2), b: num(3) };
    default:
      throw new Error(`unknown color kind: ${arr[0]}`);
  }
}

describe("pipeline: FrameReader → decodeSmCells", () => {
  for (const c of cases) {
    test(`round-trip via framed wire bytes: ${c.name}`, () => {
      const smBytes = hexToBytes(c.sm_bytes_hex);
      const reader = new FrameReader();
      reader.push(frame(TAG_CELL_DELTA, smBytes));
      const got = reader.next();
      expect(got).not.toBeNull();
      expect(got!.kind).toBe("cell-delta");
      const cells = decodeSmCells(got!.payload, c.expected_cells.length);
      expect(cells).toHaveLength(c.expected_cells.length);
      for (let i = 0; i < cells.length; i += 1) {
        expect(cells[i]!.ch).toBe(c.expected_cells[i]!.ch);
        expect(cells[i]!.fg).toEqual(colorFromJson(c.expected_cells[i]!.fg));
        expect(cells[i]!.bg).toEqual(colorFromJson(c.expected_cells[i]!.bg));
        expect(cells[i]!.flags).toBe(c.expected_cells[i]!.flags);
      }
    });
  }

  test("multiple framed payloads in one push round-trip without state leak", () => {
    // Guard rather than just non-null-asserting: if a future fixture
    // regeneration produces fewer than two cases, fail with a clear
    // assertion instead of a "cannot read properties of undefined"
    // runtime crash from the `!` operator.
    expect(cases.length).toBeGreaterThanOrEqual(2);
    const reader = new FrameReader();
    const c0 = cases[0]!;
    const c1 = cases[1]!;
    const a = hexToBytes(c0.sm_bytes_hex);
    const b = hexToBytes(c1.sm_bytes_hex);
    const concat = new Uint8Array(5 + a.length + 5 + b.length);
    concat.set(frame(TAG_CELL_DELTA, a), 0);
    concat.set(frame(TAG_CELL_DELTA, b), 5 + a.length);
    reader.push(concat);

    const fa = reader.next();
    const fb = reader.next();
    expect(fa).not.toBeNull();
    expect(fb).not.toBeNull();
    expect(reader.next()).toBeNull();

    const cellsA = decodeSmCells(fa!.payload, c0.expected_cells.length);
    const cellsB = decodeSmCells(fb!.payload, c1.expected_cells.length);
    expect(cellsA[0]!.ch).toBe(c0.expected_cells[0]!.ch);
    expect(cellsB[0]!.ch).toBe(c1.expected_cells[0]!.ch);
  });
});
