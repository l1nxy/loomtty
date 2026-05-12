// Round-trip test: load the JSON fixtures emitted by
// `cargo run -p ciri-protocol --example sm_fixtures` and verify the TS
// decoder produces byte-identical output to what the Rust encoder
// committed to. The fixture file is checked in to give the TS test
// suite a stable target; regenerate it after any encoder change.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { describe, expect, test } from "vitest";
import { decodeSmCells } from "./state-machine.js";
import type { PackedColor } from "./types.js";

interface FixtureCase {
  name: string;
  sm_bytes_hex: string;
  expected_cells: {
    ch: string;
    fg: [string, ...number[]];
    bg: [string, ...number[]];
    flags: number;
  }[];
}

import { hexToBytes } from "./__fixtures__/hex.js";

function colorFromJson(arr: [string, ...number[]]): PackedColor {
  const kind = arr[0];
  switch (kind) {
    case "named":
      return { kind: "named", index: arr[1]! };
    case "indexed":
      return { kind: "indexed", index: arr[1]! };
    case "rgb":
      return { kind: "rgb", r: arr[1]!, g: arr[2]!, b: arr[3]! };
    default:
      throw new Error(`unknown color kind: ${kind}`);
  }
}

const __dirname = dirname(fileURLToPath(import.meta.url));
const fixturePath = join(__dirname, "__fixtures__", "sm.json");
const cases: FixtureCase[] = JSON.parse(readFileSync(fixturePath, "utf-8"));

describe("SM decoder round-trip against Rust fixtures", () => {
  for (const c of cases) {
    test(c.name, () => {
      const bytes = hexToBytes(c.sm_bytes_hex);
      const decoded = decodeSmCells(bytes, c.expected_cells.length);

      expect(decoded).toHaveLength(c.expected_cells.length);
      for (let i = 0; i < decoded.length; i += 1) {
        const got = decoded[i]!;
        const want = c.expected_cells[i]!;
        expect(got.ch, `case=${c.name} cell=${i} char`).toBe(want.ch);
        expect(got.fg, `case=${c.name} cell=${i} fg`).toEqual(
          colorFromJson(want.fg),
        );
        expect(got.bg, `case=${c.name} cell=${i} bg`).toEqual(
          colorFromJson(want.bg),
        );
        expect(got.flags, `case=${c.name} cell=${i} flags`).toBe(want.flags);
      }
    });
  }
});
