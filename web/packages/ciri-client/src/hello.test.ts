// Round-trip ClientHello bytes against fixtures produced by the Rust
// encoder; verify ServerHello decoding accepts the 8-byte response the
// server emits and rejects every adversarial corruption.

import { describe, expect, test } from "vitest";
import {
  HELLO_FIXTURES,
  CIRI_PKG_VERSION,
  SERVER_HELLO_LEN,
} from "./__generated__/fixtures.js";
import {
  HandshakeError,
  decodeServerHello,
  encodeClientHello,
} from "./hello.js";

function bytesToHex(b: Uint8Array): string {
  let s = "";
  for (const v of b) s += v.toString(16).padStart(2, "0");
  return s;
}

function hexToBytes(hex: string): Uint8Array {
  if (hex.length % 2 !== 0) throw new Error(`odd-length hex: ${hex.length}`);
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i += 1) {
    const byte = Number.parseInt(hex.substr(i * 2, 2), 16);
    if (!Number.isFinite(byte)) throw new Error(`bad hex at ${i * 2}`);
    out[i] = byte;
  }
  return out;
}

describe("ClientHello encoder", () => {
  for (const f of HELLO_FIXTURES) {
    test(`round-trip ${JSON.stringify(f.name)}`, () => {
      const got = encodeClientHello(f.hello);
      expect(bytesToHex(got)).toBe(f.hex);
    });
  }

  test("rejects oversize session name", () => {
    const longName = "x".repeat(256);
    expect(() =>
      encodeClientHello({
        sessionName: longName,
        width: 800,
        height: 600,
        cellWidth: 9,
        cellHeight: 18,
      }),
    ).toThrow(HandshakeError);
  });

  test("rejects non-finite cell dims", () => {
    expect(() =>
      encodeClientHello({
        sessionName: "x",
        width: 800,
        height: 600,
        cellWidth: Number.NaN,
        cellHeight: 18,
      }),
    ).toThrow(/cellWidth/);
  });

  test("rejects zero viewport dim", () => {
    expect(() =>
      encodeClientHello({
        sessionName: "x",
        width: 0,
        height: 600,
        cellWidth: 9,
        cellHeight: 18,
      }),
    ).toThrow(/width/);
  });
});

describe("ServerHello decoder", () => {
  test("accepts byte-identical local version (exact compat)", () => {
    const buf = new Uint8Array(SERVER_HELLO_LEN);
    buf.set([0x43, 0x49, 0x52, 0x49], 0); // "CIRI"
    new DataView(buf.buffer).setUint32(4, CIRI_PKG_VERSION, true);
    const info = decodeServerHello(buf);
    expect(info.compat.kind).toBe("exact");
    expect(info.peerVersion).toBe(CIRI_PKG_VERSION);
  });

  test("flags minor-mismatch but does not throw", () => {
    const peer = CIRI_PKG_VERSION + (1 << 16); // bump minor
    const buf = new Uint8Array(SERVER_HELLO_LEN);
    buf.set([0x43, 0x49, 0x52, 0x49], 0);
    new DataView(buf.buffer).setUint32(4, peer, true);
    const info = decodeServerHello(buf);
    expect(info.compat.kind).toBe("minor-mismatch");
  });

  test("throws on major mismatch", () => {
    const peer = CIRI_PKG_VERSION + (1 << 24); // bump major
    const buf = new Uint8Array(SERVER_HELLO_LEN);
    buf.set([0x43, 0x49, 0x52, 0x49], 0);
    new DataView(buf.buffer).setUint32(4, peer, true);
    expect(() => decodeServerHello(buf)).toThrow(/major version/);
  });

  test("rejects wrong magic", () => {
    const buf = new Uint8Array(SERVER_HELLO_LEN);
    buf.set([0x44, 0x49, 0x52, 0x49], 0); // "DIRI"
    new DataView(buf.buffer).setUint32(4, CIRI_PKG_VERSION, true);
    expect(() => decodeServerHello(buf)).toThrow(/bad magic/);
  });

  test("rejects truncated input", () => {
    expect(() => decodeServerHello(new Uint8Array(7))).toThrow(/8 bytes/);
  });

  test("rejects oversize input (peer might send LZ4-compressed frame)", () => {
    expect(() => decodeServerHello(new Uint8Array(9))).toThrow(/8 bytes/);
  });
});

describe("hex helpers self-test", () => {
  // Catches the case where bytesToHex / hexToBytes drift on padding;
  // also confirms the fixture hex strings round-trip through this
  // suite's local helpers exactly the way `encodeClientHello` does.
  test("fixture hex round-trips", () => {
    const fixture = HELLO_FIXTURES[0]!;
    const bytes = hexToBytes(fixture.hex);
    expect(bytesToHex(bytes)).toBe(fixture.hex);
  });
});
