// Smoke + happy-path coverage for WebSocketTransport. Drives the
// fake socket synchronously and asserts state transitions, queue
// behaviour, and reconnect timing via vitest's fake timers.

import { describe, expect, test, vi } from "vitest";
import { makeFakeFactory } from "./__tests__/fake-ws.js";
import { WebSocketTransport } from "./transport.js";

describe("WebSocketTransport", () => {
  test("state machine: idle → connecting → open → closed", () => {
    const { factory, sockets } = makeFakeFactory();
    const events: string[] = [];
    const t = new WebSocketTransport(
      { url: "ws://example.test", reconnect: false, webSocketFactory: factory },
      {
        onOpen: () => events.push("open"),
        onClose: (info) => events.push(`close:${info.code}:${info.reconnecting}`),
      },
    );
    expect(t.state).toBe("idle");
    t.start();
    expect(t.state).toBe("connecting");
    sockets[0]!.simulateOpen();
    expect(t.state).toBe("open");
    t.close(1000, "bye");
    expect(t.state).toBe("closed");
    expect(events).toEqual(["open", "close:1000:false"]);
  });

  test("token is appended as `?token=` (URL-encoded)", () => {
    const { factory, sockets } = makeFakeFactory();
    const t = new WebSocketTransport(
      {
        url: "ws://example.test/ws",
        token: "hex 16 bytes/!@#",
        webSocketFactory: factory,
      },
    );
    t.start();
    expect(sockets[0]!.url).toBe(
      "ws://example.test/ws?token=hex%2016%20bytes%2F!%40%23",
    );
  });

  test("token is appended with `&` when URL already has a query", () => {
    const { factory, sockets } = makeFakeFactory();
    const t = new WebSocketTransport(
      {
        url: "ws://example.test/ws?foo=1",
        token: "abc",
        webSocketFactory: factory,
      },
    );
    t.start();
    expect(sockets[0]!.url).toBe("ws://example.test/ws?foo=1&token=abc");
  });

  test("send buffers payloads before open and flushes on open", () => {
    const { factory, sockets } = makeFakeFactory();
    const t = new WebSocketTransport(
      { url: "ws://example.test", webSocketFactory: factory },
    );
    t.start();
    t.send(new Uint8Array([0x01, 0x02]));
    t.send(new Uint8Array([0x03]));
    expect(sockets[0]!.sent).toHaveLength(0);
    sockets[0]!.simulateOpen();
    expect(sockets[0]!.sent).toHaveLength(2);
    expect(sockets[0]!.sent[0]!.bytes).toEqual(new Uint8Array([0x01, 0x02]));
    expect(sockets[0]!.sent[1]!.bytes).toEqual(new Uint8Array([0x03]));
  });

  test("oldest queued message is dropped when cap is exceeded", () => {
    const { factory, sockets } = makeFakeFactory();
    const t = new WebSocketTransport(
      {
        url: "ws://example.test",
        maxQueuedBytes: 4,
        webSocketFactory: factory,
      },
    );
    t.start();
    t.send(new Uint8Array([0xaa, 0xbb])); // 2B queued, total=2
    t.send(new Uint8Array([0xcc, 0xdd])); // 2B queued, total=4
    t.send(new Uint8Array([0xee, 0xff])); // would total=6 → drops oldest, total=4
    sockets[0]!.simulateOpen();
    // First payload was evicted; remaining two flush in order.
    expect(sockets[0]!.sent.map((s) => Array.from(s.bytes))).toEqual([
      [0xcc, 0xdd],
      [0xee, 0xff],
    ]);
  });

  test("payload larger than cap is rejected outright", () => {
    const { factory, sockets } = makeFakeFactory();
    const errors: Error[] = [];
    const t = new WebSocketTransport(
      {
        url: "ws://example.test",
        maxQueuedBytes: 4,
        webSocketFactory: factory,
      },
      { onError: (e) => errors.push(e) },
    );
    t.start();
    t.send(new Uint8Array([0x01, 0x02, 0x03, 0x04, 0x05])); // 5B > cap 4
    sockets[0]!.simulateOpen();
    expect(sockets[0]!.sent).toHaveLength(0);
    expect(errors).toHaveLength(1);
    expect(errors[0]!.message).toMatch(/exceeds maxQueuedBytes/);
  });

  test("binary data is delivered to onData as Uint8Array", () => {
    const { factory, sockets } = makeFakeFactory();
    const seen: Uint8Array[] = [];
    const t = new WebSocketTransport(
      { url: "ws://example.test", webSocketFactory: factory },
      { onData: (b) => seen.push(b) },
    );
    t.start();
    sockets[0]!.simulateOpen();
    sockets[0]!.simulateData(new Uint8Array([0x10, 0x20, 0x30]));
    expect(seen).toHaveLength(1);
    expect(seen[0]).toEqual(new Uint8Array([0x10, 0x20, 0x30]));
  });

  test("text frames are rejected as protocol error and do NOT trigger reconnect", () => {
    vi.useFakeTimers();
    try {
      const { factory, sockets } = makeFakeFactory();
      const errors: Error[] = [];
      const closes: { reconnecting: boolean }[] = [];
      const t = new WebSocketTransport(
        {
          url: "ws://example.test",
          // Leave `reconnect` at its default (true) — that's exactly
          // the configuration where the bug bites if the policy is
          // wrong.
          initialReconnectDelayMs: 50,
          webSocketFactory: factory,
        },
        {
          onError: (e) => errors.push(e),
          onClose: (i) => closes.push({ reconnecting: i.reconnecting }),
        },
      );
      t.start();
      sockets[0]!.simulateOpen();
      sockets[0]!.simulateText("nope");
      expect(errors[0]!.message).toMatch(/text frame/);
      expect(sockets[0]!.closeCalls).toHaveLength(1);
      expect(closes).toEqual([{ reconnecting: false }]);
      // Wait through several would-be reconnect intervals — no new
      // sockets must be created.
      vi.advanceTimersByTime(5_000);
      expect(sockets).toHaveLength(1);
      expect(t.state).toBe("closed");
    } finally {
      vi.useRealTimers();
    }
  });

  test("text-frame rejection doesn't double-fire onError if more frames are buffered", () => {
    const { factory, sockets } = makeFakeFactory();
    const errors: Error[] = [];
    const t = new WebSocketTransport(
      {
        url: "ws://example.test",
        reconnect: false,
        webSocketFactory: factory,
      },
      { onError: (e) => errors.push(e) },
    );
    t.start();
    sockets[0]!.simulateOpen();
    sockets[0]!.simulateText("first");
    // Simulate a stale buffered message arriving after we called
    // close() — real browsers can deliver onmessage events between
    // close() and the eventual onclose. The transport must have
    // detached listeners by this point so the second text frame is
    // dropped silently.
    sockets[0]!.simulateText("buffered after close");
    sockets[0]!.simulateData(new Uint8Array([0x01, 0x02]));
    expect(errors).toHaveLength(1);
  });

  test("reconnect: schedules retry on transient close, exponential backoff", () => {
    vi.useFakeTimers();
    try {
      const { factory, sockets } = makeFakeFactory();
      const t = new WebSocketTransport(
        {
          url: "ws://example.test",
          reconnect: true,
          initialReconnectDelayMs: 100,
          maxReconnectDelayMs: 800,
          webSocketFactory: factory,
        },
      );
      t.start();
      sockets[0]!.simulateOpen();
      sockets[0]!.simulateClose(1006, "drop"); // transient
      // First retry after 100ms.
      vi.advanceTimersByTime(99);
      expect(sockets).toHaveLength(1);
      vi.advanceTimersByTime(1);
      expect(sockets).toHaveLength(2);
      sockets[1]!.simulateClose(1006, "drop again");
      // Next retry doubled: 200ms.
      vi.advanceTimersByTime(199);
      expect(sockets).toHaveLength(2);
      vi.advanceTimersByTime(1);
      expect(sockets).toHaveLength(3);
    } finally {
      vi.useRealTimers();
    }
  });

  test("backoff resets to initial after a successful open", () => {
    vi.useFakeTimers();
    try {
      const { factory, sockets } = makeFakeFactory();
      const t = new WebSocketTransport(
        {
          url: "ws://example.test",
          reconnect: true,
          initialReconnectDelayMs: 100,
          maxReconnectDelayMs: 800,
          webSocketFactory: factory,
        },
      );
      t.start();
      sockets[0]!.simulateOpen();
      sockets[0]!.simulateClose(1006, "drop"); // → 100ms retry
      vi.advanceTimersByTime(100);
      sockets[1]!.simulateClose(1006, "drop"); // → 200ms retry
      vi.advanceTimersByTime(200);
      sockets[2]!.simulateOpen(); // success: backoff resets
      sockets[2]!.simulateClose(1006, "drop"); // → 100ms again (not 400)
      vi.advanceTimersByTime(99);
      expect(sockets).toHaveLength(3);
      vi.advanceTimersByTime(1);
      expect(sockets).toHaveLength(4);
    } finally {
      vi.useRealTimers();
    }
  });

  test("close() prevents further reconnect attempts", () => {
    vi.useFakeTimers();
    try {
      const { factory, sockets } = makeFakeFactory();
      const t = new WebSocketTransport(
        {
          url: "ws://example.test",
          reconnect: true,
          initialReconnectDelayMs: 50,
          webSocketFactory: factory,
        },
      );
      t.start();
      sockets[0]!.simulateOpen();
      t.close(); // intentional
      vi.advanceTimersByTime(1000);
      expect(sockets).toHaveLength(1);
    } finally {
      vi.useRealTimers();
    }
  });

  test("close() during reconnect backoff still fires terminal onClose", () => {
    // Without the synthesized onClose in transport.close(), callers
    // staring at a "reconnecting…" UI would never learn the lifecycle
    // ended when the user navigated away mid-backoff.
    vi.useFakeTimers();
    try {
      const { factory, sockets } = makeFakeFactory();
      const closes: { reconnecting: boolean }[] = [];
      const t = new WebSocketTransport(
        {
          url: "ws://example.test",
          reconnect: true,
          initialReconnectDelayMs: 200,
          webSocketFactory: factory,
        },
        { onClose: (i) => closes.push({ reconnecting: i.reconnecting }) },
      );
      t.start();
      sockets[0]!.simulateOpen();
      sockets[0]!.simulateClose(1006, "drop"); // transient → reconnecting:true
      expect(closes).toEqual([{ reconnecting: true }]);
      // Mid-backoff (timer not yet fired): caller calls close().
      vi.advanceTimersByTime(50);
      t.close();
      expect(closes).toEqual([
        { reconnecting: true },
        { reconnecting: false },
      ]);
      // And the pending reconnect timer is cancelled.
      vi.advanceTimersByTime(10_000);
      expect(sockets).toHaveLength(1);
    } finally {
      vi.useRealTimers();
    }
  });

  test("close() clears the pre-open send queue (no stale flush on restart)", () => {
    const { factory, sockets } = makeFakeFactory();
    const t = new WebSocketTransport(
      { url: "ws://example.test", reconnect: false, webSocketFactory: factory },
    );
    t.start();
    // Queue bytes before the (first) socket ever opens.
    t.send(new Uint8Array([0xaa, 0xbb]));
    t.send(new Uint8Array([0xcc, 0xdd]));
    expect(sockets[0]!.sent).toHaveLength(0);
    // Intentional close before any open: those queued payloads MUST
    // NOT be replayed against the next socket the user opens.
    t.close();
    t.start();
    sockets[1]!.simulateOpen();
    expect(sockets[1]!.sent).toHaveLength(0);
  });

  test("close() is idempotent — second call is a no-op", () => {
    const { factory, sockets } = makeFakeFactory();
    const closes: unknown[] = [];
    const t = new WebSocketTransport(
      { url: "ws://example.test", reconnect: false, webSocketFactory: factory },
      { onClose: (i) => closes.push(i) },
    );
    t.start();
    sockets[0]!.simulateOpen();
    t.close();
    const after = closes.length;
    t.close();
    expect(closes.length).toBe(after);
  });
});
