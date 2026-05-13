// Wire-body decoder tests for CellDelta and FullPaneSync. Every test
// pins against Rust-emitted fixtures via `dump_frame_fixtures` —
// hand-built byte cases at the bottom exercise edge cases the curated
// fixtures don't cover (truncation, oversize geometry, malformed UTF-8,
// inverted bounds).

import { describe, expect, test } from "vitest";
import {
  CELL_DELTA_FIXTURES,
  FULL_PANE_SYNC_FIXTURES,
} from "./__generated__/frame-fixtures.js";
import {
  FrameBodyDecodeError,
  decodeCellDelta,
  decodeFullPaneSync,
} from "./frame-body.js";
import { DEFAULT_BG, DEFAULT_FG } from "./types.js";

function hexToBytes(hex: string): Uint8Array {
  if (hex.length % 2 !== 0) throw new Error(`odd-length hex: ${hex.length}`);
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i += 1) {
    out[i] = Number.parseInt(hex.substr(i * 2, 2), 16);
  }
  return out;
}

describe("decodeCellDelta", () => {
  for (const f of CELL_DELTA_FIXTURES) {
    test(`round-trips Rust bytes: ${f.name}`, () => {
      const got = decodeCellDelta(hexToBytes(f.hex));
      expect(got).toEqual(f.value);
    });
  }

  test("rejects truncated header", () => {
    expect(() => decodeCellDelta(new Uint8Array(20))).toThrow(
      FrameBodyDecodeError,
    );
  });

  test("rejects inverted region bounds (left > right)", () => {
    // Take fixture A, splice region.left=5 / region.right=4.
    const f = CELL_DELTA_FIXTURES[0]!;
    const bytes = hexToBytes(f.hex);
    // Header is 35 bytes, then region header at offset 35: line(2), left(2), right(2), sm_len(4).
    // left at offset 37, right at offset 39.
    bytes[37] = 5;
    bytes[38] = 0;
    bytes[39] = 4;
    bytes[40] = 0;
    expect(() => decodeCellDelta(bytes)).toThrow(/invalid damage bounds/);
  });

  test("rejects region extending past cols", () => {
    // Take fixture B (cols=80, region 0 right=11). Move right to 80
    // (>= cols) so the row would splice past the column boundary.
    const f = CELL_DELTA_FIXTURES[1]!;
    const bytes = hexToBytes(f.hex);
    // Same offsets as above — header 35 bytes, region 0 right at +6.
    bytes[39] = 80;
    bytes[40] = 0;
    expect(() => decodeCellDelta(bytes)).toThrow(/right=80/);
  });

  test("rejects trailing bytes after last region", () => {
    const f = CELL_DELTA_FIXTURES[0]!;
    const bytes = hexToBytes(f.hex);
    const padded = new Uint8Array(bytes.length + 2);
    padded.set(bytes, 0);
    padded[bytes.length] = 0xff;
    padded[bytes.length + 1] = 0xff;
    expect(() => decodeCellDelta(padded)).toThrow(/trailing byte/);
  });

  test("rejects SM payload length past frame cap", () => {
    // Header + region header with sm_data_len = 2^32-1 → bigger than
    // any legitimate frame, must surface before any byte allocation.
    const f = CELL_DELTA_FIXTURES[0]!;
    const bytes = hexToBytes(f.hex);
    // sm_data_len lives at header(35) + line(2) + left(2) + right(2) = +41.
    bytes[41] = 0xff;
    bytes[42] = 0xff;
    bytes[43] = 0xff;
    bytes[44] = 0xff;
    expect(() => decodeCellDelta(bytes)).toThrow(/MAX_DATA_FRAME_LEN/);
  });
});

describe("decodeFullPaneSync", () => {
  for (const f of FULL_PANE_SYNC_FIXTURES) {
    test(`round-trips Rust bytes: ${f.name}`, () => {
      const got = decodeFullPaneSync(hexToBytes(f.hex));
      expect(got).toEqual(f.value);
    });
  }

  test("rejects truncated header", () => {
    expect(() => decodeFullPaneSync(new Uint8Array(20))).toThrow(
      FrameBodyDecodeError,
    );
  });

  test("rejects oversize grid", () => {
    // cols=u16max, rows=u16max → way past MAX_GRID_CELLS.
    // Build a minimal header: meta(35) + cols(2) + rows(2) + cursor_line(2) + cursor_col(2)
    //                       + cursor_shape(1) + mode_flags(2) + echo_ack(8) + title_len(2) = 56.
    // Wait — re-check layout:
    //   pane_id(8), generation(8), cols(2), rows(2),
    //   cursor_line(2), cursor_col(2), cursor_shape(1), mode_flags(2),
    //   echo_ack(8), title_len(2) = 37 bytes.
    const bytes = new Uint8Array(37);
    // pane_id, generation = zeros (offsets 0..16).
    bytes[16] = 0xff;
    bytes[17] = 0xff; // cols = 65535
    bytes[18] = 0xff;
    bytes[19] = 0xff; // rows = 65535
    // 65535 * 65535 = 4_294_836_225 → way past 10M cap.
    expect(() => decodeFullPaneSync(bytes)).toThrow(/grid too large/);
  });

  test("rejects oversize scrollback", () => {
    // Same as above but a sane cols/rows + huge scrollback_rows.
    // The check (`scrollback_rows * cols > MAX_GRID_CELLS`) runs after
    // both fields have been read; the buffer must extend through
    // `scrollback_replace` so the readU8 doesn't throw first.
    //   header(37) + title_len(0) + scrollback_rows(4)
    //              + scrollback_replace(1) = 42 bytes minimum.
    const bytes = new Uint8Array(42);
    bytes[16] = 10; // cols low byte = 10
    bytes[18] = 1; // rows low byte = 1
    // scrollback_rows = u32::MAX → 4_294_967_295 * 10 ≫ 10M cap.
    bytes[37] = 0xff;
    bytes[38] = 0xff;
    bytes[39] = 0xff;
    bytes[40] = 0xff;
    // scrollback_replace = 0 (last byte). Decoder reads it AFTER
    // scrollback_rows but BEFORE the cap check; without this slot
    // present the test would trip the truncation guard instead.
    expect(() => decodeFullPaneSync(bytes)).toThrow(/scrollback too large/);
  });

  test("rejects invalid UTF-8 in title", () => {
    // Build a minimal header with title_len=1 + a lone 0x80 byte
    // (invalid first byte of any UTF-8 sequence).
    const bytes = new Uint8Array(37 + 1 + 4 + 1 + 4 + 4 + 1 + 2 + 2 + 2 + 2);
    bytes[16] = 1; // cols low = 1
    bytes[18] = 1; // rows low = 1
    bytes[35] = 1; // title_len low = 1
    bytes[37] = 0x80; // invalid UTF-8 start
    // The rest: scrollback_rows=0, replace=0, sb_sm_len=0, vp_sm_len=1, vp_sm=[OP_END=0xff],
    //          grapheme=0, link.cells=0, link.map=0, cwd_len=0.
    // We only need decoder to reach the title decode; UTF-8 throw bails out before the rest matters.
    expect(() => decodeFullPaneSync(bytes)).toThrow(/invalid UTF-8/);
  });
});

describe("decodeFullPaneSync — grapheme extras + hyperlinks", () => {
  // Spot-check the second fixture's structural fields rather than just
  // `toEqual`-ing the whole value (which the earlier loop already does).
  // This catches drift where the Map shape is silently broken in
  // a way the deep-equal happens to tolerate.
  test("fixture 2 carries hyperlink extras as Map<number, ...>", () => {
    const f = FULL_PANE_SYNC_FIXTURES[1]!;
    const got = decodeFullPaneSync(hexToBytes(f.hex));
    expect(got.graphemeExtras).toBeInstanceOf(Map);
    expect(got.cellLinks).toBeInstanceOf(Map);
    expect(got.linkMap).toBeInstanceOf(Map);
    expect(got.graphemeExtras.get(1)).toBe("‍\u{1F4BB}");
    expect(got.cellLinks.get(2)).toBe(1);
    expect(got.linkMap.get(1)).toBe("https://example.com");
  });

  test("fixture 3 distinguishes scrollback from viewport SM streams", () => {
    const f = FULL_PANE_SYNC_FIXTURES[2]!;
    const got = decodeFullPaneSync(hexToBytes(f.hex));
    expect(got.scrollback).toHaveLength(2);
    expect(got.scrollback[0]!.ch).toBe("o");
    expect(got.cells).toHaveLength(2);
    expect(got.cells[0]!.ch).toBe("h");
    expect(got.cells[0]!.fg).toEqual(DEFAULT_FG);
    expect(got.cells[0]!.bg).toEqual(DEFAULT_BG);
  });
});
