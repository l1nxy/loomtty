// End-to-end: LoomClient → fake WebSocket. Drives the client through
// hello replay, framed message routing, error paths, and reconnect.

import { describe, expect, test, vi } from "vitest";
import { TAG_SERVER_MSG } from "@loom/codec";
import { makeFakeFactory } from "./__tests__/fake-ws.js";
import { LoomClient, type LoomEvent } from "./client.js";
import { encodeClientMessage } from "./codec.js";
import {
  LOOM_PKG_VERSION,
  SERVER_HELLO_LEN,
} from "./__generated__/fixtures.js";
import { CLIENT_FIXTURES, SERVER_FIXTURES } from "./__generated__/fixtures.js";

function serverHelloBytes(version: number = LOOM_PKG_VERSION): Uint8Array {
  const buf = new Uint8Array(SERVER_HELLO_LEN);
  buf.set([0x4c, 0x4f, 0x4f, 0x4d], 0);
  new DataView(buf.buffer).setUint32(4, version, true);
  return buf;
}

function frameOf(tag: number, payload: Uint8Array): Uint8Array {
  const out = new Uint8Array(5 + payload.length);
  out[0] = tag;
  new DataView(out.buffer).setUint32(1, payload.length, true);
  out.set(payload, 5);
  return out;
}

function hexToBytes(hex: string): Uint8Array {
  if (hex.length % 2 !== 0) throw new Error(`odd-length hex`);
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i += 1) {
    out[i] = Number.parseInt(hex.substr(i * 2, 2), 16);
  }
  return out;
}

function setup(opts: Partial<{ reconnect: boolean }> = {}) {
  const { factory, sockets } = makeFakeFactory();
  const events: LoomEvent[] = [];
  const c = new LoomClient(
    {
      url: "ws://example.test",
      sessionName: "main",
      viewport: {
        sessionName: "main",
        width: 1280,
        height: 720,
        cellWidth: 9.5,
        cellHeight: 18.0,
      },
      reconnect: opts.reconnect,
      webSocketFactory: factory,
    },
    (e) => events.push(e),
  );
  return { c, sockets, events };
}

describe("LoomClient handshake", () => {
  test("sends ClientHello on WebSocket open", () => {
    const { c, sockets } = setup({ reconnect: false });
    c.start();
    sockets[0]!.simulateOpen();
    expect(sockets[0]!.sent).toHaveLength(1);
    // First byte must be the magic "L".
    expect(sockets[0]!.sent[0]!.bytes[0]).toBe(0x4c);
  });

  test("emits `open` after receiving 8-byte ServerHello", () => {
    const { c, sockets, events } = setup({ reconnect: false });
    c.start();
    sockets[0]!.simulateOpen();
    sockets[0]!.simulateData(serverHelloBytes());
    expect(events.map((e) => e.kind)).toContain("open");
    expect(c.state).toBe("open");
  });

  test("handshake split across multiple WS messages still completes", () => {
    const { c, sockets, events } = setup({ reconnect: false });
    c.start();
    sockets[0]!.simulateOpen();
    const hello = serverHelloBytes();
    sockets[0]!.simulateData(hello.subarray(0, 3));
    sockets[0]!.simulateData(hello.subarray(3, 6));
    sockets[0]!.simulateData(hello.subarray(6));
    expect(c.state).toBe("open");
    expect(events.filter((e) => e.kind === "open")).toHaveLength(1);
  });

  test("final hello chunk + leading frame bytes in the same WS message", () => {
    // Exercises the leftover-byte path in consumeHandshake: the
    // chunk that completes the 8-byte ServerHello also carries the
    // start of the first framed payload. The transport must hand
    // those leftover bytes straight to the FrameReader.
    const { c, sockets, events } = setup({ reconnect: false });
    c.start();
    sockets[0]!.simulateOpen();
    const hello = serverHelloBytes();
    sockets[0]!.simulateData(hello.subarray(0, 5)); // partial hello
    const bell = SERVER_FIXTURES.find((s) => s.name === "Bell")!;
    const framed = frameOf(TAG_SERVER_MSG, hexToBytes(bell.hex));
    const tail = new Uint8Array(hello.length - 5 + framed.length);
    tail.set(hello.subarray(5), 0);
    tail.set(framed, hello.length - 5);
    sockets[0]!.simulateData(tail);
    const kinds = events.map((e) => e.kind);
    expect(kinds).toContain("open");
    expect(kinds.filter((k) => k === "server-msg")).toHaveLength(1);
    void c;
  });

  test("frame bytes that arrive in the same chunk as ServerHello are still parsed", () => {
    const { c, sockets, events } = setup({ reconnect: false });
    c.start();
    sockets[0]!.simulateOpen();
    const serverMsg = encodeClientMessage; // unused — placeholder for type import
    void serverMsg;
    // Use a real ServerMessage fixture for the framed payload.
    const f = SERVER_FIXTURES.find((s) => s.name === "Bell")!;
    const framed = frameOf(TAG_SERVER_MSG, hexToBytes(f.hex));
    const combined = new Uint8Array(SERVER_HELLO_LEN + framed.length);
    combined.set(serverHelloBytes(), 0);
    combined.set(framed, SERVER_HELLO_LEN);
    sockets[0]!.simulateData(combined);
    // Expect both open and server-msg events.
    const kinds = events.map((e) => e.kind);
    expect(kinds).toContain("open");
    expect(kinds).toContain("server-msg");
  });

  test("rejects bad-magic ServerHello and closes the transport", () => {
    const { c, sockets, events } = setup({ reconnect: false });
    c.start();
    sockets[0]!.simulateOpen();
    const bad = new Uint8Array(SERVER_HELLO_LEN);
    bad.set([0x44, 0x49, 0x52, 0x49], 0); // wrong magic
    sockets[0]!.simulateData(bad);
    expect(events.some((e) => e.kind === "error")).toBe(true);
    expect(sockets[0]!.closeCalls).toHaveLength(1);
    void c;
  });
});

describe("LoomClient inbound frames", () => {
  test("routes ServerMsg frames to onEvent as decoded ServerMessage", () => {
    const { c, sockets, events } = setup({ reconnect: false });
    c.start();
    sockets[0]!.simulateOpen();
    sockets[0]!.simulateData(serverHelloBytes());
    const bell = SERVER_FIXTURES.find((s) => s.name === "Bell")!;
    sockets[0]!.simulateData(frameOf(TAG_SERVER_MSG, hexToBytes(bell.hex)));
    const evt = events.find((e) => e.kind === "server-msg");
    expect(evt).toBeDefined();
    if (evt && evt.kind === "server-msg") {
      expect(evt.msg).toEqual(bell.value);
    }
    void c;
  });
});

describe("LoomClient outbound send", () => {
  test("send(ClientMessage) frames the payload with TAG_CLIENT_MSG", () => {
    const { c, sockets } = setup({ reconnect: false });
    c.start();
    sockets[0]!.simulateOpen();
    sockets[0]!.simulateData(serverHelloBytes());
    const input = CLIENT_FIXTURES.find((f) => f.name === "Input bytes")!;
    c.send(input.value);
    // First sent payload was the ClientHello — skip it; then we expect
    // exactly one TAG_CLIENT_MSG frame.
    const frames = sockets[0]!.sent.slice(1);
    expect(frames).toHaveLength(1);
    const frame = frames[0]!.bytes;
    expect(frame[0]).toBe(0x01); // TAG_CLIENT_MSG
    const len = new DataView(frame.buffer, frame.byteOffset).getUint32(1, true);
    expect(frame.length).toBe(5 + len);
    // Payload bytes match the Rust-generated fixture.
    const payload = frame.subarray(5);
    expect(payload).toEqual(hexToBytes(input.hex));
  });

  test("sendInput bumps input_seq monotonically", () => {
    const { c, sockets } = setup({ reconnect: false });
    c.start();
    sockets[0]!.simulateOpen();
    sockets[0]!.simulateData(serverHelloBytes());
    const seq1 = c.sendInput(7n, new Uint8Array([0x61]));
    const seq2 = c.sendInput(7n, new Uint8Array([0x62]));
    expect(seq1).toBe(1n);
    expect(seq2).toBe(2n);
  });

  test("send before open queues the payload via the transport's buffer", () => {
    const { c, sockets } = setup({ reconnect: false });
    c.start();
    // No simulateOpen yet — send() goes through transport queue.
    // Pick a unit variant that DOESN'T trigger the Detach
    // special-case (which intentionally closes the transport); a
    // close before open would flush nothing.
    c.send({ tag: "EqualizeColumnSplit" });
    sockets[0]!.simulateOpen();
    sockets[0]!.simulateData(serverHelloBytes());
    // sent: [ClientHello, EqualizeColumnSplit frame].
    expect(sockets[0]!.sent.length).toBeGreaterThanOrEqual(2);
    const eqFrame = sockets[0]!.sent[sockets[0]!.sent.length - 1]!.bytes;
    expect(eqFrame[0]).toBe(0x01);
    // Bare msgpack fixstr "EqualizeColumnSplit": 0xb3 + 19 ASCII = 20.
    expect(eqFrame.length).toBe(5 + 20);
  });
});

describe("LoomClient frame-size cap", () => {
  test("rejects ClientMessage payloads larger than MAX_CONTROL_FRAME_LEN", async () => {
    const { c, sockets } = setup({ reconnect: false });
    c.start();
    sockets[0]!.simulateOpen();
    sockets[0]!.simulateData(serverHelloBytes());
    // Construct an Input payload whose msgpack encoding will overflow
    // the 1 MiB control-frame cap. Each byte in `data` encodes as a
    // single msgpack fixint, so just over 1 MiB of bytes is enough
    // to trip the guard without resorting to giant headers.
    const huge = new Uint8Array(2 * 1024 * 1024);
    expect(() =>
      c.send({ tag: "Input", paneId: 1n, data: huge, inputSeq: 1n }),
    ).toThrow(/MAX_CONTROL_FRAME_LEN/);
    void c;
  });
});

describe("LoomClient viewport tracking", () => {
  test("Resize updates stored ClientHello so reconnect carries fresh dims", () => {
    vi.useFakeTimers();
    try {
      const { c, sockets } = setup({ reconnect: true });
      c.start();
      sockets[0]!.simulateOpen();
      sockets[0]!.simulateData(serverHelloBytes());

      // User resizes the browser → app sends Resize. The client must
      // mirror the new dims into its replay ClientHello.
      c.send({
        tag: "Resize",
        cols: 100,
        rows: 30,
        width: 1600,
        height: 900,
        cellWidth: 10,
        cellHeight: 18,
      });

      // Drop and reconnect.
      sockets[0]!.simulateClose(1006, "drop");
      vi.advanceTimersByTime(1100);
      expect(sockets).toHaveLength(2);
      sockets[1]!.simulateOpen();

      // The replayed ClientHello carries width=1600 / height=900,
      // not the constructor's 1280×720. Parse the bytes the client
      // sent: header(11) + session "main"(4) + width u32 LE at
      // offset 15.
      const hello = sockets[1]!.sent[0]!.bytes;
      const view = new DataView(
        hello.buffer,
        hello.byteOffset,
        hello.byteLength,
      );
      // Layout: magic(4) + pkgVersion(4) + wire(1) + nameLen(2) +
      // name(N) + width(4) + height(4) + cellW(4) + cellH(4).
      const nameLen = view.getUint16(9, true);
      const widthOffset = 11 + nameLen;
      expect(view.getUint32(widthOffset, true)).toBe(1600);
      expect(view.getUint32(widthOffset + 4, true)).toBe(900);
      void c;
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("LoomClient session tracking", () => {
  test("SessionSwitched updates stored ClientHello so reconnect attaches to new session", () => {
    vi.useFakeTimers();
    try {
      const { c, sockets } = setup({ reconnect: true });
      c.start();
      sockets[0]!.simulateOpen();
      sockets[0]!.simulateData(serverHelloBytes());

      // Hand-build the msgpack bytes for
      // `ServerMessage::SessionSwitched { session_name: "other-session" }`.
      // The schema-driven encoder only knows ClientMessage variants,
      // so going through it would fail; pinning the wire bytes
      // directly is the smallest and clearest fixture for this case.
      //   81             = fixmap-1
      //   af "SessionSwitched" (15 bytes, fixstr-15)
      //   91             = fixarray-1
      //   ad "other-session" (13 bytes, fixstr-13)
      const enc = new TextEncoder();
      const variant = enc.encode("SessionSwitched");
      const sessName = enc.encode("other-session");
      const wire = new Uint8Array(
        1 + 1 + variant.length + 1 + 1 + sessName.length,
      );
      let p = 0;
      wire[p++] = 0x81;
      wire[p++] = 0xa0 | variant.length;
      wire.set(variant, p);
      p += variant.length;
      wire[p++] = 0x91;
      wire[p++] = 0xa0 | sessName.length;
      wire.set(sessName, p);
      sockets[0]!.simulateData(frameOf(TAG_SERVER_MSG, wire));

      // Drop and reconnect — replayed ClientHello must carry the new
      // session name `other-session` (13 bytes, fixstr).
      sockets[0]!.simulateClose(1006, "drop");
      vi.advanceTimersByTime(1100);
      expect(sockets).toHaveLength(2);
      sockets[1]!.simulateOpen();
      const hello = sockets[1]!.sent[0]!.bytes;
      const view = new DataView(
        hello.buffer,
        hello.byteOffset,
        hello.byteLength,
      );
      const nameLen = view.getUint16(9, true);
      expect(nameLen).toBe("other-session".length);
      const name = new TextDecoder().decode(hello.subarray(11, 11 + nameLen));
      expect(name).toBe("other-session");
      void c;
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("LoomClient detach", () => {
  test("Detach disables reconnect — server's socket close does NOT bounce us back", () => {
    vi.useFakeTimers();
    try {
      const { c, sockets } = setup({ reconnect: true });
      c.start();
      sockets[0]!.simulateOpen();
      sockets[0]!.simulateData(serverHelloBytes());

      c.send({ tag: "Detach" });

      // Server-side Detach handler reaps the client and closes the
      // socket. Simulate that.
      sockets[0]!.simulateClose(1000, "server reaped detached client");

      // Without the round-14 fix the transport would treat this as
      // transient and schedule a reconnect. With it, no further
      // sockets are created.
      vi.advanceTimersByTime(10_000);
      expect(sockets).toHaveLength(1);
      expect(c.state).toBe("closed");
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("LoomClient resize-replay safety", () => {
  test("invalid Resize doesn't poison the replay ClientHello", () => {
    vi.useFakeTimers();
    try {
      const { c, sockets, events } = setup({ reconnect: true });
      c.start();
      sockets[0]!.simulateOpen();
      sockets[0]!.simulateData(serverHelloBytes());

      // First Resize: valid, lands in the replay hello.
      c.send({
        tag: "Resize",
        cols: 100,
        rows: 30,
        width: 1600,
        height: 900,
        cellWidth: 10,
        cellHeight: 18,
      });

      // Second Resize: width=0 (canvas hidden) — server tolerates,
      // but encodeClientHello would reject. The replay hello must
      // hold onto the previous good values.
      c.send({
        tag: "Resize",
        cols: 0,
        rows: 0,
        width: 0,
        height: 0,
        cellWidth: 10,
        cellHeight: 18,
      });

      sockets[0]!.simulateClose(1006, "drop");
      vi.advanceTimersByTime(1100);
      expect(sockets).toHaveLength(2);
      sockets[1]!.simulateOpen();
      // Reconnect succeeded (Hello bytes on the wire start with magic
      // "L"). If the bad Resize had been cached, encodeClientHello
      // inside `handleSocketOpen` would have thrown.
      expect(sockets[1]!.sent[0]!.bytes[0]).toBe(0x4c);
      // No error events were emitted from the reconnect path.
      expect(events.filter((e) => e.kind === "error")).toHaveLength(0);
      void c;
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("LoomClient reconnect", () => {
  test("replays ClientHello on each reconnect open", () => {
    vi.useFakeTimers();
    try {
      const { c, sockets } = setup({ reconnect: true });
      c.start();
      sockets[0]!.simulateOpen();
      sockets[0]!.simulateData(serverHelloBytes());
      // Drop and let the transport's backoff scheduler reconnect.
      sockets[0]!.simulateClose(1006, "drop");
      vi.advanceTimersByTime(1100);
      expect(sockets).toHaveLength(2);
      sockets[1]!.simulateOpen();
      // First payload after open MUST be a fresh ClientHello (magic).
      expect(sockets[1]!.sent[0]!.bytes[0]).toBe(0x4c);
      void c;
    } finally {
      vi.useRealTimers();
    }
  });
});
