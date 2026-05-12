// Frame reader: stream-oriented header parsing, multi-frame messages,
// straddling WS boundaries, and the tag → kind dispatch table.

import { describe, expect, test } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  TAG_CELL_DELTA,
  TAG_CELL_DELTA_LZ4,
  TAG_CLIENT_MSG,
  TAG_FULL_PANE_SYNC,
  TAG_FULL_PANE_SYNC_LZ4,
  TAG_SERVER_MSG,
} from "./constants.js";
import { FrameReader } from "./frame.js";

interface Lz4Fixture {
  name: string;
  original_hex: string;
  payload_hex: string;
}
import { hexToBytes } from "./__fixtures__/hex.js";
const __dirname = dirname(fileURLToPath(import.meta.url));
const LZ4_FIXTURES: Lz4Fixture[] = JSON.parse(
  readFileSync(join(__dirname, "__fixtures__", "lz4.json"), "utf-8"),
);

function frame(tag: number, payload: Uint8Array): Uint8Array {
  const out = new Uint8Array(5 + payload.length);
  out[0] = tag;
  const view = new DataView(out.buffer);
  view.setUint32(1, payload.length, /* le */ true);
  out.set(payload, 5);
  return out;
}

function concat(...chunks: Uint8Array[]): Uint8Array {
  const len = chunks.reduce((s, c) => s + c.length, 0);
  const out = new Uint8Array(len);
  let off = 0;
  for (const c of chunks) {
    out.set(c, off);
    off += c.length;
  }
  return out;
}

describe("FrameReader", () => {
  test("returns null until at least one full frame is buffered", () => {
    const reader = new FrameReader();
    expect(reader.next()).toBeNull();

    // Push only the 5-byte header — payload still missing.
    reader.push(new Uint8Array([TAG_SERVER_MSG, 5, 0, 0, 0]));
    expect(reader.next()).toBeNull();

    // Add the 5 payload bytes.
    reader.push(new Uint8Array([1, 2, 3, 4, 5]));
    const f = reader.next();
    expect(f).not.toBeNull();
    expect(f!.kind).toBe("server-msg");
    expect(Array.from(f!.payload)).toEqual([1, 2, 3, 4, 5]);
    expect(reader.next()).toBeNull();
  });

  test("yields multiple frames packed into one push", () => {
    const reader = new FrameReader();
    reader.push(concat(
      frame(TAG_CLIENT_MSG, new Uint8Array([0xab])),
      frame(TAG_CELL_DELTA, new Uint8Array([1, 2, 3])),
      frame(TAG_FULL_PANE_SYNC, new Uint8Array([])),
    ));
    const a = reader.next();
    const b = reader.next();
    const c = reader.next();
    expect(a?.kind).toBe("client-msg");
    expect(b?.kind).toBe("cell-delta");
    expect(c?.kind).toBe("full-pane-sync");
    expect(reader.next()).toBeNull();
  });

  test("reassembles a frame that straddles two pushes", () => {
    const reader = new FrameReader();
    const full = frame(TAG_SERVER_MSG, new Uint8Array([1, 2, 3, 4, 5, 6, 7]));
    // Split in the middle of the payload.
    reader.push(full.subarray(0, 8));
    expect(reader.next()).toBeNull();
    reader.push(full.subarray(8));
    const f = reader.next();
    expect(f?.kind).toBe("server-msg");
    expect(Array.from(f!.payload)).toEqual([1, 2, 3, 4, 5, 6, 7]);
  });

  test("unknown tag throws", () => {
    const reader = new FrameReader();
    reader.push(new Uint8Array([0x7f, 0, 0, 0, 0]));
    expect(() => reader.next()).toThrow(/unknown frame tag/);
  });

  test("reader is poisoned after a throw — subsequent calls re-throw", () => {
    const reader = new FrameReader();
    reader.push(new Uint8Array([0x7f, 0, 0, 0, 0]));
    expect(() => reader.next()).toThrow(/unknown frame tag/);
    expect(reader.isPoisoned()).toBe(true);
    expect(() => reader.next()).toThrow(/poisoned/);
    expect(() => reader.push(new Uint8Array([1]))).toThrow(/poisoned/);
  });

  test("pending() tracks the unconsumed byte count", () => {
    const reader = new FrameReader();
    expect(reader.pending()).toBe(0);

    // Only the 5-byte header, payload missing.
    reader.push(new Uint8Array([TAG_SERVER_MSG, 5, 0, 0, 0]));
    expect(reader.pending()).toBe(5);
    expect(reader.next()).toBeNull();
    expect(reader.pending()).toBe(5);

    // Complete the frame; next() consumes it.
    reader.push(new Uint8Array([1, 2, 3, 4, 5]));
    expect(reader.pending()).toBe(10);
    expect(reader.next()).not.toBeNull();
    expect(reader.pending()).toBe(0);
  });

  test("oversize control frame throws before allocating", () => {
    const reader = new FrameReader();
    // payload_len bytes encode u32 LE = 0x00200000 = 2 MiB, which
    // exceeds the 1 MiB MAX_CONTROL_FRAME_LEN limit. Layout:
    //   [tag][len byte 0][len byte 1][len byte 2][len byte 3]
    //     01      00         00          20         00
    reader.push(new Uint8Array([TAG_CLIENT_MSG, 0, 0, 0x20, 0]));
    expect(() => reader.next()).toThrow(/frame too large/);
  });

  // ─── LZ4 frame variants ──────────────────────────────────
  // Driven by `lz4_fixture.rs` (the same `lz4_flex::compress` the
  // server uses). The JS encoders we tested (lz4js block APIs) round
  // to subtly different formats; round-tripping against the real
  // server encoder is the only meaningful coverage.

  for (const fx of LZ4_FIXTURES) {
    test(`LZ4 frame round-trips Rust fixture: ${fx.name}`, () => {
      const original = hexToBytes(fx.original_hex);
      const payload = hexToBytes(fx.payload_hex);
      const reader = new FrameReader();
      reader.push(frame(TAG_CELL_DELTA_LZ4, payload));
      const f = reader.next();
      expect(f?.kind).toBe("cell-delta");
      expect(Array.from(f!.payload)).toEqual(Array.from(original));
    });
  }

  test("TAG_FULL_PANE_SYNC_LZ4 dispatches to full-pane-sync kind", () => {
    const fx = LZ4_FIXTURES[0]!;
    const reader = new FrameReader();
    reader.push(frame(TAG_FULL_PANE_SYNC_LZ4, hexToBytes(fx.payload_hex)));
    const f = reader.next();
    expect(f?.kind).toBe("full-pane-sync");
    expect(Array.from(f!.payload)).toEqual(Array.from(hexToBytes(fx.original_hex)));
  });

  test("LZ4 payload with non-zero uncompressed length but empty compressed bytes is rejected", () => {
    // 4-byte header claiming 100 uncompressed bytes, zero compressed
    // bytes. Mirrors the zero-len bomb guard in the Rust codec.
    const payload = new Uint8Array([100, 0, 0, 0]);
    const reader = new FrameReader();
    reader.push(frame(TAG_CELL_DELTA_LZ4, payload));
    expect(() => reader.next()).toThrow(/zero compressed bytes/);
  });

  test("LZ4 payload with absurd compression ratio is rejected", () => {
    // Header claims 10 MiB uncompressed; only 2 bytes of compressed
    // data. Ratio is 5 MB:1, well above the 64:1 bomb guard.
    const payload = new Uint8Array(6);
    new DataView(payload.buffer).setUint32(0, 10 * 1024 * 1024, true);
    payload[4] = 0xff;
    payload[5] = 0xff;
    const reader = new FrameReader();
    reader.push(frame(TAG_CELL_DELTA_LZ4, payload));
    expect(() => reader.next()).toThrow(/exceeds limit 64:1/);
  });

  test("hex helper rejects odd-length input", () => {
    expect(() => hexToBytes("abc")).toThrow(/odd length/);
  });

  test("LZ4 payload claiming an oversize uncompressed length is rejected", () => {
    const payload = new Uint8Array(8);
    // 32 MiB > MAX_DATA_FRAME_LEN (16 MiB).
    new DataView(payload.buffer).setUint32(0, 32 * 1024 * 1024, true);
    const reader = new FrameReader();
    reader.push(frame(TAG_CELL_DELTA_LZ4, payload));
    expect(() => reader.next()).toThrow(/exceeds MAX_DATA_FRAME_LEN/);
  });
});
