// End-to-end: CiriClient → fake WebSocket. Drives the client through
// hello replay, framed message routing, error paths, and reconnect.

import { describe, expect, test, vi } from "vitest";
import { TAG_SERVER_MSG } from "@ciri/codec";
import { makeFakeFactory } from "./__tests__/fake-ws.js";
import { CiriClient, type CiriEvent } from "./client.js";
import { encodeClientMessage } from "./codec.js";
import {
  CIRI_PKG_VERSION,
  SERVER_HELLO_LEN,
} from "./__generated__/fixtures.js";
import { CLIENT_FIXTURES, SERVER_FIXTURES } from "./__generated__/fixtures.js";

function serverHelloBytes(version: number = CIRI_PKG_VERSION): Uint8Array {
  const buf = new Uint8Array(SERVER_HELLO_LEN);
  buf.set([0x43, 0x49, 0x52, 0x49], 0);
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
  const events: CiriEvent[] = [];
  const c = new CiriClient(
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

describe("CiriClient handshake", () => {
  test("sends ClientHello on WebSocket open", () => {
    const { c, sockets } = setup({ reconnect: false });
    c.start();
    sockets[0]!.simulateOpen();
    expect(sockets[0]!.sent).toHaveLength(1);
    // First byte must be the magic "C".
    expect(sockets[0]!.sent[0]!.bytes[0]).toBe(0x43);
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

describe("CiriClient inbound frames", () => {
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

describe("CiriClient outbound send", () => {
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
    // No simulateOpen yet — send() goes through transport queue
    // (since this fires before ClientHello, the *first* flushed
    // payload after open is hello, *then* the queued Detach).
    c.send({ tag: "Detach" });
    sockets[0]!.simulateOpen();
    sockets[0]!.simulateData(serverHelloBytes());
    // sent: [ClientHello, Detach frame].
    expect(sockets[0]!.sent.length).toBeGreaterThanOrEqual(2);
    const detachFrame = sockets[0]!.sent[sockets[0]!.sent.length - 1]!.bytes;
    expect(detachFrame[0]).toBe(0x01);
    // Bare msgpack fixstr "Detach": 0xa6 + 6 ASCII bytes = 7 bytes.
    expect(detachFrame.length).toBe(5 + 7);
  });
});

describe("CiriClient reconnect", () => {
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
      expect(sockets[1]!.sent[0]!.bytes[0]).toBe(0x43);
      void c;
    } finally {
      vi.useRealTimers();
    }
  });
});
