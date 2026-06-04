// In-memory WebSocket mock — implements the subset of the WHATWG
// WebSocket interface our `WebSocketTransport` consumes
// (`binaryType`, `send`, `close`, `on{open,close,message,error}`).
//
// Each `FakeWebSocket` is single-shot. The test driver constructs
// one (via `makeFakeFactory`) and then calls
// `fake.simulateOpen() / simulateData(...) / simulateClose(...)` to
// step the transport through its state transitions deterministically.
//
// We don't try to simulate latency, framing rules, or the browser's
// async tick ordering — every callback fires synchronously inside the
// driver's call. Tests that need the transport's own `setTimeout`
// (reconnect backoff) drive it via vitest's `vi.useFakeTimers`.

import type { WebSocketLike } from "../transport.js";

export interface SentPayload {
  bytes: Uint8Array;
}

export class FakeWebSocket implements WebSocketLike {
  binaryType: "arraybuffer" | "blob" = "arraybuffer";
  readonly sent: SentPayload[] = [];
  readonly url: string;
  readonly protocols: string | string[] | undefined;
  closeCalls: { code?: number; reason?: string }[] = [];

  onopen: ((ev: unknown) => void) | null = null;
  onclose: ((ev: { code: number; reason: string }) => void) | null = null;
  onmessage: ((ev: { data: ArrayBuffer | string }) => void) | null = null;
  onerror: ((ev: unknown) => void) | null = null;

  constructor(url: string, protocols?: string | string[]) {
    this.url = url;
    this.protocols = protocols;
  }

  send(data: ArrayBuffer | ArrayBufferView): void {
    if (data instanceof ArrayBuffer) {
      this.sent.push({ bytes: new Uint8Array(data) });
    } else {
      // Copy out so the test sees the bytes captured at send-time,
      // not whatever the caller's buffer holds at assertion-time.
      this.sent.push({
        bytes: new Uint8Array(
          data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength),
        ),
      });
    }
  }

  close(code?: number, reason?: string): void {
    this.closeCalls.push({ code, reason });
    // Fire the close event synchronously so the transport
    // transitions immediately — real browsers fire asynchronously
    // but tests want determinism, and our transport doesn't depend
    // on async-close ordering anywhere.
    this.onclose?.({ code: code ?? 1000, reason: reason ?? "" });
  }

  // ── Test driver helpers ─────────────────────────────────────────

  simulateOpen(): void {
    this.onopen?.({});
  }

  simulateData(bytes: Uint8Array): void {
    // Match what a real browser delivers: an ArrayBuffer of the
    // exact payload (no extra headroom). `slice` copies — keep the
    // test's `bytes` independent of the buffer the transport reads.
    const ab = bytes.buffer.slice(
      bytes.byteOffset,
      bytes.byteOffset + bytes.byteLength,
    );
    this.onmessage?.({ data: ab });
  }

  simulateText(s: string): void {
    this.onmessage?.({ data: s });
  }

  simulateClose(code = 1006, reason = "abnormal"): void {
    this.onclose?.({ code, reason });
  }

  simulateError(): void {
    this.onerror?.(new Event("error"));
  }
}

export function makeFakeFactory(): {
  factory: (url: string, protocols?: string | string[]) => WebSocketLike;
  sockets: FakeWebSocket[];
} {
  const sockets: FakeWebSocket[] = [];
  const factory = (url: string, protocols?: string | string[]): WebSocketLike => {
    const s = new FakeWebSocket(url, protocols);
    sockets.push(s);
    return s;
  };
  return { factory, sockets };
}

// Polyfill for Event used by simulateError — node may not have a
// global `Event` constructor depending on version. Pull from globalThis
// when present, otherwise stub. Tests just need *something* truthy.
const Event: typeof globalThis.Event =
  typeof globalThis.Event !== "undefined"
    ? globalThis.Event
    : (class Event {
        readonly type: string;
        constructor(type: string) {
          this.type = type;
        }
      } as unknown as typeof globalThis.Event);
