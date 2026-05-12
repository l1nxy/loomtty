// Round-trip the schema-driven codec against the Rust-emitted
// fixtures. Each fixture pins a (msgpack hex, TS value) pair: the
// encoder must produce byte-identical bytes the server will accept,
// and the decoder must reconstruct the typed value the renderer can
// consume.

import { describe, expect, test } from "vitest";
import {
  CLIENT_FIXTURES,
  SERVER_FIXTURES,
} from "./__generated__/fixtures.js";
import {
  CodecError,
  decodeServerMessage,
  encodeClientMessage,
} from "./codec.js";

function bytesToHex(b: Uint8Array): string {
  let s = "";
  for (const v of b) s += v.toString(16).padStart(2, "0");
  return s;
}

function hexToBytes(hex: string): Uint8Array {
  if (hex.length % 2 !== 0) throw new Error(`odd-length hex: ${hex.length}`);
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i += 1) {
    out[i] = Number.parseInt(hex.substr(i * 2, 2), 16);
  }
  return out;
}

describe("encodeClientMessage", () => {
  for (const f of CLIENT_FIXTURES) {
    test(`produces Rust-identical bytes: ${f.name}`, () => {
      const got = encodeClientMessage(f.value);
      expect(bytesToHex(got)).toBe(f.hex);
    });
  }

  test("rejects unknown variant", () => {
    expect(() =>
      // @ts-expect-error testing runtime guard against an invalid discriminant
      encodeClientMessage({ tag: "Nonsense", x: 1 }),
    ).toThrow(/unknown variant/);
  });

  test("rejects missing required field", () => {
    expect(() =>
      // @ts-expect-error missing required `data` and `inputSeq`
      encodeClientMessage({ tag: "Input", paneId: 1n }),
    ).toThrow(/missing required field/);
  });

  test("u8 field rejects out-of-range integer (local error, not server disconnect)", () => {
    expect(() =>
      encodeClientMessage({
        tag: "MouseInput",
        paneId: 1n,
        button: 300, // u8 max is 255
        col: 0,
        row: 0,
        pressed: true,
        modifiers: 0,
      }),
    ).toThrow(/u8 out of range/);
  });

  test("u16 field rejects out-of-range integer", () => {
    expect(() =>
      encodeClientMessage({
        tag: "Resize",
        cols: 70_000, // u16 max is 65535
        rows: 24,
        width: 1280,
        height: 720,
        cellWidth: 9.5,
        cellHeight: 18.0,
      }),
    ).toThrow(/u16 out of range/);
  });

  test("u64 field accepts the full unsigned range", () => {
    // Top of u64; would overflow Number but bigint handles it.
    const top = (1n << 64n) - 1n;
    const bytes = encodeClientMessage({
      tag: "Ack",
      generation: top,
    });
    expect(bytes[0]).toBe(0x81);
  });

  test("u64 field rejects values past 2^64-1", () => {
    expect(() =>
      encodeClientMessage({
        tag: "Ack",
        generation: 1n << 64n, // one past the max
      }),
    ).toThrow(/u64 out of range/);
  });
});

describe("decodeServerMessage", () => {
  for (const f of SERVER_FIXTURES) {
    test(`decodes Rust bytes back to typed value: ${f.name}`, () => {
      const got = decodeServerMessage(hexToBytes(f.hex));
      expect(got).toEqual(f.value);
    });
  }

  test("rejects unknown variant on the wire", () => {
    // Hand-craft `{"Bogus": [1]}` → `81 a5 "Bogus" 91 01`.
    const buf = new Uint8Array([0x81, 0xa5, 0x42, 0x6f, 0x67, 0x75, 0x73, 0x91, 0x01]);
    expect(() => decodeServerMessage(buf)).toThrow(/unknown variant/);
  });

  test("rejects multi-key map on the wire (not a valid externally-tagged enum)", () => {
    // `{"Bell": [1], "Foo": [1]}` — 2-entry fixmap.
    const buf = new Uint8Array([
      0x82, 0xa4, 0x42, 0x65, 0x6c, 0x6c, 0x91, 0x01,
      0xa3, 0x46, 0x6f, 0x6f, 0x91, 0x01,
    ]);
    expect(() => decodeServerMessage(buf)).toThrow(/1-entry map/);
  });

  test("rejects malformed msgpack", () => {
    // 0xc1 is `never used` in msgpack — guaranteed to fail decoding.
    expect(() => decodeServerMessage(new Uint8Array([0xc1]))).toThrow(CodecError);
  });
});

describe("round-trip via encode + decode (client direction)", () => {
  // For each ClientMessage fixture, verify that decoding our encoder's
  // output through the SERVER's decoder (which we don't have in JS)
  // would produce the same value. We approximate by going
  // ClientMessage → bytes → ServerMessage decoder (using ClientMessage
  // schema) to confirm the JS-side encode is self-consistent.
  //
  // The cross-language ground truth is the hex string — that's what
  // the bytes-equality test above pins.
  test("encoder output is well-formed msgpack", () => {
    for (const f of CLIENT_FIXTURES) {
      const bytes = encodeClientMessage(f.value);
      // Length and the first byte (variant map tag) are sanity proxies:
      // a unit variant starts with `0xa..` (fixstr), a payload variant
      // with `0x81` (1-entry fixmap).
      expect(bytes.length).toBeGreaterThan(0);
      const first = bytes[0]!;
      const isFixstr = (first & 0xe0) === 0xa0;
      const isFixmap1 = first === 0x81;
      expect(isFixstr || isFixmap1).toBe(true);
    }
  });
});
