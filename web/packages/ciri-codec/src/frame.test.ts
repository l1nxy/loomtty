// Frame reader: stream-oriented header parsing, multi-frame messages,
// straddling WS boundaries, and the tag → kind dispatch table.

import { describe, expect, test } from "vitest";
import {
  TAG_CELL_DELTA,
  TAG_CLIENT_MSG,
  TAG_FULL_PANE_SYNC,
  TAG_SERVER_MSG,
} from "./constants.js";
import { FrameReader } from "./frame.js";

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

  test("oversize control frame throws before allocating", () => {
    const reader = new FrameReader();
    // payload_len > MAX_CONTROL_FRAME_LEN (1 MiB).
    reader.push(new Uint8Array([TAG_CLIENT_MSG, 0, 0, 0x20, 0]));
    expect(() => reader.next()).toThrow(/frame too large/);
  });
});
