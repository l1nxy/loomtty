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
  decodeClientHello,
  decodeServerHello,
  encodeClientHello,
} from "./hello.js";
import { WIRE_PROTOCOL_VERSION } from "./__generated__/fixtures.js";

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

  test("rejects empty session name (server's validate_name does too)", () => {
    expect(() =>
      encodeClientHello({
        sessionName: "",
        width: 800,
        height: 600,
        cellWidth: 9,
        cellHeight: 18,
      }),
    ).toThrow(/empty/);
  });

  test("rejects uppercase / non-ASCII characters in session name", () => {
    for (const bad of ["Main", "session_1", "项目", "with space", "uri/path"]) {
      expect(
        () =>
          encodeClientHello({
            sessionName: bad,
            width: 800,
            height: 600,
            cellWidth: 9,
            cellHeight: 18,
          }),
        `should reject ${JSON.stringify(bad)}`,
      ).toThrow(/lowercase letters, digits, and hyphens/);
    }
  });

  test("rejects session name starting with '-'", () => {
    expect(() =>
      encodeClientHello({
        sessionName: "-leading-dash",
        width: 800,
        height: 600,
        cellWidth: 9,
        cellHeight: 18,
      }),
    ).toThrow(/start with '-'/);
  });

  test("accepts the control-session escape (`__control__`)", () => {
    // The Rust server exempts `__control__` from the lowercase rule
    // so CLI/IPC callers can issue `ListSessions`, `RunCommand`,
    // etc. without attaching to a pane. The TS validator must too.
    const bytes = encodeClientHello({
      sessionName: "__control__",
      width: 800,
      height: 600,
      cellWidth: 9,
      cellHeight: 18,
    });
    expect(bytes[0]).toBe(0x43); // magic "C"
  });

  test("accepts the auto-attach escape (`__auto__`)", () => {
    // The server resolves `__auto__` to a concrete session at handshake
    // time (most-recent-or-new). Like `__control__` it's `__`-prefixed,
    // so the lowercase-ASCII validator must let it through unencoded.
    const bytes = encodeClientHello({
      sessionName: "__auto__",
      width: 800,
      height: 600,
      cellWidth: 9,
      cellHeight: 18,
    });
    expect(bytes[0]).toBe(0x43); // magic "C"
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

describe("ClientHello decoder", () => {
  for (const f of HELLO_FIXTURES) {
    test(`round-trip ${JSON.stringify(f.name)}`, () => {
      const bytes = hexToBytes(f.hex);
      const out = decodeClientHello(bytes);
      expect(out.hello).toEqual(f.hello);
      expect(out.wireVersion).toBe(WIRE_PROTOCOL_VERSION);
    });
  }

  test("encode → decode round-trip preserves all fields", () => {
    const original = {
      sessionName: "fresh-fox-12",
      width: 1920,
      height: 1080,
      cellWidth: 8.0,
      cellHeight: 16.0,
    };
    const bytes = encodeClientHello(original);
    const { hello } = decodeClientHello(bytes);
    expect(hello).toEqual(original);
  });

  test("accepts `__control__` (encoder doesn't gate on charset for it)", () => {
    // The decoder must be at least as lenient as `decode_client_hello_parts`
    // on the Rust side — that path leaves session-name validation to
    // `validate_name` in connection.rs. A diagnostic that rejects
    // `__control__` would mis-flag legitimate IPC clients.
    const bytes = encodeClientHello({
      sessionName: "__control__",
      width: 800,
      height: 600,
      cellWidth: 9,
      cellHeight: 18,
    });
    const { hello } = decodeClientHello(bytes);
    expect(hello.sessionName).toBe("__control__");
  });

  test("rejects bad magic", () => {
    const bytes = encodeClientHello({
      sessionName: "main",
      width: 800,
      height: 600,
      cellWidth: 9,
      cellHeight: 18,
    });
    bytes[0] = 0x44; // "DIRI"
    expect(() => decodeClientHello(bytes)).toThrow(/bad magic/);
  });

  test("rejects wrong wire version", () => {
    const bytes = encodeClientHello({
      sessionName: "main",
      width: 800,
      height: 600,
      cellWidth: 9,
      cellHeight: 18,
    });
    bytes[8] = 0xff;
    expect(() => decodeClientHello(bytes)).toThrow(/wire protocol/);
  });

  test("rejects truncated header", () => {
    expect(() => decodeClientHello(new Uint8Array(5))).toThrow(/truncated/);
  });

  test("rejects truncated body (header says more bytes than buffer holds)", () => {
    const bytes = encodeClientHello({
      sessionName: "main",
      width: 800,
      height: 600,
      cellWidth: 9,
      cellHeight: 18,
    });
    const cut = bytes.subarray(0, bytes.length - 4);
    expect(() => decodeClientHello(cut)).toThrow(/truncated/);
  });

  test("rejects invalid utf-8 in session name", () => {
    const bytes = encodeClientHello({
      sessionName: "main",
      width: 800,
      height: 600,
      cellWidth: 9,
      cellHeight: 18,
    });
    // Overwrite the first session-name byte with a lone continuation
    // byte (0x80) — invalid UTF-8.
    bytes[11] = 0x80;
    expect(() => decodeClientHello(bytes)).toThrow(/utf8/);
  });

  test("rejects out-of-range viewport on decode (mirrors encoder)", () => {
    const bytes = encodeClientHello({
      sessionName: "main",
      width: 800,
      height: 600,
      cellWidth: 9,
      cellHeight: 18,
    });
    // Stomp the width field (offset = 11 + nameLen) with 0.
    const widthOff = 11 + 4;
    new DataView(bytes.buffer).setUint32(widthOff, 0, true);
    expect(() => decodeClientHello(bytes)).toThrow(/width/);
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
