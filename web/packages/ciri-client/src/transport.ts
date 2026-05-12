// Low-level WebSocket adapter for the ciritty web gateway.
//
// Surface deliberately tiny:
//   - `start()` opens the socket (and re-opens it on transient drops
//     unless `reconnect: false`).
//   - `send(bytes)` queues bytes when the socket isn't yet open and
//     flushes them on the next `open` event. The queue is bounded so
//     a long disconnect doesn't blow the heap.
//   - `close()` ends the conversation; no further reconnect attempts.
//
// Higher-level concerns — handshake replay, frame parsing, event
// routing — live in `CiriClient`. This adapter doesn't speak the
// ciritty wire protocol; it just shuttles bytes.

export type TransportState =
  | "idle"
  | "connecting"
  | "open"
  | "closing"
  | "closed";

export interface TransportCallbacks {
  /** Fires after the underlying WebSocket transitions to OPEN. */
  onOpen?: () => void;
  /** Inbound binary payload (one WebSocket message). Always
   *  Uint8Array, never a string — the gateway is binary-only. */
  onData?: (bytes: Uint8Array) => void;
  /** Fires whenever the underlying socket closes, whether or not we
   *  intend to reconnect. `reconnecting` tells the caller which one. */
  onClose?: (info: { code: number; reason: string; reconnecting: boolean }) => void;
  /** Surface a non-fatal error (browser fires an opaque `Event` for
   *  some failure modes; we still want the caller to be able to log
   *  and decide on UX). */
  onError?: (err: Error) => void;
}

/** Minimal subset of the WHATWG `WebSocket` interface this transport
 *  uses — kept small so unit tests can inject a hand-rolled mock
 *  without re-implementing every getter the spec defines. */
export interface WebSocketLike {
  binaryType: "arraybuffer" | "blob";
  send(data: ArrayBuffer | ArrayBufferView): void;
  close(code?: number, reason?: string): void;
  onopen: ((ev: unknown) => void) | null;
  onclose: ((ev: { code: number; reason: string }) => void) | null;
  onmessage: ((ev: { data: ArrayBuffer | string }) => void) | null;
  onerror: ((ev: unknown) => void) | null;
}

export interface WebSocketTransportOptions {
  /** ws:// or wss:// URL. If `token` is set, it's appended as a
   *  `?token=...` query parameter (mirrors what `ciri-server`'s WS
   *  handshake accepts). */
  url: string;
  /** Bearer token for the gateway's `?token=` query. Optional — omit
   *  for unauthenticated test gateways. */
  token?: string;
  /** Extra WebSocket subprotocol(s) — usually unused. */
  protocols?: string | string[];
  /** Re-open on transient drops (default `true`). When `false`, a
   *  single close ends the transport. */
  reconnect?: boolean;
  /** Reconnect backoff starts here and doubles up to
   *  `maxReconnectDelayMs`. */
  initialReconnectDelayMs?: number;
  maxReconnectDelayMs?: number;
  /** Hard cap on bytes held in the pre-open send queue. When the cap
   *  is reached, the OLDEST queued message is dropped — preferring
   *  fresher input over stale catchup. */
  maxQueuedBytes?: number;
  /** Constructor override for tests. Production passes the global
   *  `WebSocket` indirectly through the default. */
  webSocketFactory?: (url: string, protocols?: string | string[]) => WebSocketLike;
}

const DEFAULT_INITIAL_RECONNECT_MS = 1_000;
const DEFAULT_MAX_RECONNECT_MS = 30_000;
const DEFAULT_MAX_QUEUED_BYTES = 4 * 1024 * 1024;

export class WebSocketTransport {
  private _state: TransportState = "idle";
  private ws: WebSocketLike | null = null;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private currentBackoffMs: number;
  private readonly sendQueue: Uint8Array[] = [];
  private queuedBytes = 0;
  private intentionallyClosed = false;
  private readonly factory: NonNullable<
    WebSocketTransportOptions["webSocketFactory"]
  >;

  constructor(
    private readonly opts: WebSocketTransportOptions,
    private readonly cbs: TransportCallbacks = {},
  ) {
    this.currentBackoffMs =
      opts.initialReconnectDelayMs ?? DEFAULT_INITIAL_RECONNECT_MS;
    this.factory = opts.webSocketFactory ?? defaultWebSocketFactory;
  }

  get state(): TransportState {
    return this._state;
  }

  /** Begin connecting. Throws if called from a non-`idle`/`closed`
   *  state — the caller is expected to drive the lifecycle. */
  start(): void {
    if (this._state !== "idle" && this._state !== "closed") {
      throw new Error(`WebSocketTransport.start: bad state ${this._state}`);
    }
    this.intentionallyClosed = false;
    this.openSocket();
  }

  send(bytes: Uint8Array): void {
    if (this._state === "open" && this.ws) {
      // `send` accepts an ArrayBufferView — pass the Uint8Array
      // directly so the browser slices our backing buffer into one
      // Binary WS frame (no copy).
      this.ws.send(bytes);
      return;
    }
    this.enqueue(bytes);
  }

  /** Close the socket and stop reconnecting. Idempotent — keyed on
   *  `intentionallyClosed` (NOT `_state === "closed"`) so we can
   *  still synthesize a terminal `onClose` when the user calls
   *  `close()` mid-reconnect-backoff (state is already "closed" at
   *  that point, but the lifecycle hasn't been finalized). */
  close(code = 1000, reason = "client closed"): void {
    if (this.intentionallyClosed) return;
    this.intentionallyClosed = true;
    this.clearReconnectTimer();
    if (this.ws && (this._state === "open" || this._state === "connecting")) {
      this._state = "closing";
      this.ws.close(code, reason);
      // Real-browser `ws.close()` fires `onclose` asynchronously, so
      // `handleClose` will deliver the `onClose` callback on its own.
      return;
    }
    // No live socket — either we never started, or close() was
    // called between a transient drop and the pending reconnect
    // timer firing. Synthesize a terminal `reconnecting: false`
    // event so callers stop pretending a reconnect is still
    // pending.
    this._state = "closed";
    this.cbs.onClose?.({ code, reason, reconnecting: false });
  }

  // ─── Internal lifecycle ──────────────────────────────────────────

  private openSocket(): void {
    const url = this.composeUrl();
    let ws: WebSocketLike;
    try {
      ws = this.factory(url, this.opts.protocols);
    } catch (e) {
      this.cbs.onError?.(asError(e, "WebSocket constructor threw"));
      // Treat constructor failure like a closed socket so reconnect
      // semantics apply uniformly — otherwise a bad URL would leave
      // us pinned in `connecting` forever.
      this.handleClose(1006, "constructor failed");
      return;
    }
    ws.binaryType = "arraybuffer";
    this.ws = ws;
    this._state = "connecting";

    ws.onopen = () => this.handleOpen();
    ws.onclose = (ev) => this.handleClose(ev.code, ev.reason);
    ws.onmessage = (ev) => this.handleMessage(ev.data);
    ws.onerror = (ev) => {
      // Browsers fire `onerror` as an opaque `Event` — there's no
      // useful payload to forward. Synthesize a placeholder so
      // callers can log "something went wrong" without dereferencing
      // a value of unknown shape.
      this.cbs.onError?.(asError(ev, "WebSocket error"));
    };
  }

  private handleOpen(): void {
    this._state = "open";
    this.currentBackoffMs =
      this.opts.initialReconnectDelayMs ?? DEFAULT_INITIAL_RECONNECT_MS;
    this.cbs.onOpen?.();
    this.flushQueue();
  }

  private handleClose(code: number, reason: string): void {
    const wasOpen = this._state === "open";
    this.detachListeners();
    this.ws = null;

    const willReconnect = !this.intentionallyClosed && this.opts.reconnect !== false;
    this._state = "closed";
    this.cbs.onClose?.({ code, reason, reconnecting: willReconnect });
    if (willReconnect) {
      this.scheduleReconnect();
    }
    // `wasOpen` reserved for future telemetry — keep the local until
    // we actually need to distinguish "never opened" from "drop after
    // open" to the caller.
    void wasOpen;
  }

  private handleMessage(data: ArrayBuffer | string): void {
    if (typeof data === "string") {
      // The ciri gateway never speaks text frames — protocol violation.
      // Treat as PERMANENT: pummeling a misbehaving server with
      // identical handshakes hoping for a different outcome would
      // chew through the reconnect budget and trip the server's
      // per-token handshake semaphore.
      this.cbs.onError?.(
        new Error("WebSocket: unexpected text frame from server"),
      );
      this.intentionallyClosed = true;
      // Detach listeners synchronously so any frames the browser has
      // already buffered between `close()` and the eventual
      // `onclose` event can't re-fire `onError`/`onData`.
      this.detachListeners();
      this.ws?.close(1003, "text frame rejected");
      this.handleClose(1003, "text frame rejected");
      return;
    }
    this.cbs.onData?.(new Uint8Array(data));
  }

  private detachListeners(): void {
    if (!this.ws) return;
    this.ws.onopen = null;
    this.ws.onclose = null;
    this.ws.onmessage = null;
    this.ws.onerror = null;
  }

  private scheduleReconnect(): void {
    const delay = this.currentBackoffMs;
    const max = this.opts.maxReconnectDelayMs ?? DEFAULT_MAX_RECONNECT_MS;
    // Double for next time, capped — classical exponential with
    // ceiling. We add no jitter; the server has its own handshake
    // semaphore that absorbs synchronized retries.
    this.currentBackoffMs = Math.min(this.currentBackoffMs * 2, max);
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      if (!this.intentionallyClosed) this.openSocket();
    }, delay);
  }

  private clearReconnectTimer(): void {
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
  }

  private enqueue(bytes: Uint8Array): void {
    const cap = this.opts.maxQueuedBytes ?? DEFAULT_MAX_QUEUED_BYTES;
    // Drop oldest until the new payload fits. A single payload
    // larger than the cap is rejected outright — better to surface
    // the error than silently lose a recently-queued message.
    if (bytes.length > cap) {
      this.cbs.onError?.(
        new Error(
          `WebSocketTransport: payload ${bytes.length}B exceeds maxQueuedBytes ${cap}`,
        ),
      );
      return;
    }
    while (this.queuedBytes + bytes.length > cap && this.sendQueue.length > 0) {
      const dropped = this.sendQueue.shift()!;
      this.queuedBytes -= dropped.length;
    }
    this.sendQueue.push(bytes);
    this.queuedBytes += bytes.length;
  }

  private flushQueue(): void {
    if (this.sendQueue.length === 0) return;
    const drained = this.sendQueue.splice(0);
    this.queuedBytes = 0;
    for (const m of drained) {
      this.ws?.send(m);
    }
  }

  private composeUrl(): string {
    if (this.opts.token === undefined) return this.opts.url;
    const sep = this.opts.url.includes("?") ? "&" : "?";
    return `${this.opts.url}${sep}token=${encodeURIComponent(this.opts.token)}`;
  }
}

function defaultWebSocketFactory(
  url: string,
  protocols?: string | string[],
): WebSocketLike {
  if (typeof WebSocket === "undefined") {
    throw new Error(
      "no global WebSocket — pass `webSocketFactory` (e.g. the `ws` package on Node)",
    );
  }
  return new WebSocket(url, protocols) as unknown as WebSocketLike;
}

function asError(value: unknown, fallback: string): Error {
  if (value instanceof Error) return value;
  return new Error(fallback);
}
