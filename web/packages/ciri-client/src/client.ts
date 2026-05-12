// High-level ciritty WebSocket client.
//
// Lifecycle (simplified):
//
//   idle ──start()─▶ connecting ──ws.open──▶ handshaking
//      ▲                                          │
//      │                                  ServerHello received
//      │                                          ▼
//      │                                        open ──ws.close (transient)──▶ connecting
//      │                                          │
//      ╰────────────close()─────────────closing──▶ closed
//
// Inbound on the wire is a TCP-like byte stream. The first 8 bytes of
// each connection are the `ServerHello`; everything after is framed by
// the codec. We accumulate inbound chunks across WebSocket messages
// (one binary message can carry zero, one, or many frames, and a
// frame can straddle messages) — the existing `FrameReader` already
// owns that buffering once handshake is past.
//
// Reconnect is handled in the transport. On every new `open` we
// re-send the same `ClientHello` so the server treats reconnects the
// way it treats fresh attaches.

import {
  FrameReader,
  MAX_CONTROL_FRAME_LEN,
  TAG_CLIENT_MSG,
  type RawFrame,
} from "@ciri/codec";
import {
  CodecError,
  decodeServerMessage,
  encodeClientMessage,
} from "./codec.js";
import {
  HandshakeError,
  decodeServerHello,
  encodeClientHello,
  type ClientHello,
  type VersionCompat,
} from "./hello.js";
import { SERVER_HELLO_LEN } from "./__generated__/fixtures.js";
import type {
  ClientMessage,
  ServerMessage,
} from "./__generated__/types.js";
import {
  WebSocketTransport,
  type WebSocketTransportOptions,
} from "./transport.js";

export type CiriClientState =
  | "idle"
  | "connecting"
  | "open"
  | "closed";

export type CiriEvent =
  | { kind: "open"; compat: VersionCompat }
  | { kind: "server-msg"; msg: ServerMessage }
  /** Raw CellDelta frame payload — Phase 2.3 will own decoding. */
  | { kind: "cell-delta"; payload: Uint8Array }
  | { kind: "full-pane-sync"; payload: Uint8Array }
  | { kind: "close"; reason: string; reconnecting: boolean }
  | { kind: "error"; error: Error };

export interface CiriClientOptions {
  url: string;
  token?: string;
  protocols?: string | string[];
  sessionName: string;
  viewport: ClientHello;
  reconnect?: boolean;
  initialReconnectDelayMs?: number;
  maxReconnectDelayMs?: number;
  maxQueuedBytes?: number;
  /** Test seam — pass through to the underlying transport. */
  webSocketFactory?: WebSocketTransportOptions["webSocketFactory"];
}

export class CiriClient {
  private _state: CiriClientState = "idle";
  private readonly transport: WebSocketTransport;
  // Frame reader is non-readonly because we replace it on reconnect
  // (the reader has no in-place reset; once it's seen a fault it
  // stays poisoned for the life of the instance).
  private reader: FrameReader = new FrameReader();
  // The handshake state is mutable so reconnects after a viewport
  // resize replay the CURRENT dimensions, not the constructor-time
  // snapshot. The `send()` path keeps it in lockstep with the
  // server's view via `Resize` messages.
  private hello: ClientHello;
  private handshakeBuf: Uint8Array = new Uint8Array(0);
  private nextInputSeq = 1n;

  constructor(
    private readonly opts: CiriClientOptions,
    private readonly onEvent: (e: CiriEvent) => void,
  ) {
    this.hello = {
      sessionName: opts.sessionName,
      width: opts.viewport.width,
      height: opts.viewport.height,
      cellWidth: opts.viewport.cellWidth,
      cellHeight: opts.viewport.cellHeight,
    };
    this.transport = new WebSocketTransport(
      {
        url: opts.url,
        // Only forward the optional fields when present so the
        // `exactOptionalPropertyTypes` lint stays happy without
        // having to widen the transport type.
        ...(opts.token !== undefined ? { token: opts.token } : {}),
        ...(opts.protocols !== undefined ? { protocols: opts.protocols } : {}),
        ...(opts.reconnect !== undefined ? { reconnect: opts.reconnect } : {}),
        ...(opts.initialReconnectDelayMs !== undefined
          ? { initialReconnectDelayMs: opts.initialReconnectDelayMs }
          : {}),
        ...(opts.maxReconnectDelayMs !== undefined
          ? { maxReconnectDelayMs: opts.maxReconnectDelayMs }
          : {}),
        ...(opts.maxQueuedBytes !== undefined
          ? { maxQueuedBytes: opts.maxQueuedBytes }
          : {}),
        ...(opts.webSocketFactory !== undefined
          ? { webSocketFactory: opts.webSocketFactory }
          : {}),
      },
      {
        onOpen: () => this.handleSocketOpen(),
        onClose: (info) => this.handleSocketClose(info),
        onData: (bytes) => this.handleData(bytes),
        onError: (err) => this.onEvent({ kind: "error", error: err }),
      },
    );
  }

  get state(): CiriClientState {
    return this._state;
  }

  /** Monotonic echo-ack sequence number the next `sendInput` call
   *  will use. Exposed so callers building input predictions can
   *  pre-compute the seq before the actual send. */
  get currentInputSeq(): bigint {
    return this.nextInputSeq;
  }

  start(): void {
    if (this._state !== "idle" && this._state !== "closed") {
      throw new Error(`CiriClient.start: bad state ${this._state}`);
    }
    this._state = "connecting";
    this.resetHandshakeState();
    this.transport.start();
  }

  close(): void {
    this._state = "closed";
    this.transport.close();
  }

  /** Send a typed ClientMessage. If we're not yet open, the encoded
   *  frame bytes go through the transport's send queue and flush on
   *  the next successful handshake. Throws if the encoded payload
   *  exceeds the server's control-frame cap — better a local error
   *  than letting the server drop the connection on the oversize
   *  frame header. */
  send(msg: ClientMessage): void {
    const payload = encodeClientMessage(msg);
    if (payload.length > MAX_CONTROL_FRAME_LEN) {
      throw new Error(
        `ClientMessage payload ${payload.length}B exceeds MAX_CONTROL_FRAME_LEN ${MAX_CONTROL_FRAME_LEN}; the server would reject this frame and disconnect`,
      );
    }
    // `Detach` is a one-way goodbye: the server reaps the client and
    // closes the socket, but the underlying WS close still looks
    // transient from the transport's POV — without this guard, the
    // automatic reconnect would replay `ClientHello` against the
    // server and silently re-attach. Mark the transport intentional
    // BEFORE the bytes go out so the close path can't race ahead.
    if (msg.tag === "Detach") {
      // `close()` is idempotent (round 5) and synchronously sends
      // its own close — but we WANT the server to read our Detach
      // first. Drop the close *after* the send completes so the
      // bytes land in the WS write buffer; the close immediately
      // after still takes effect (real browsers flush pending
      // sends before honoring the close).
      this.transport.send(frameClientMsg(payload));
      this.transport.close(1000, "client detached");
      this._state = "closed";
      return;
    }
    // Track the latest accepted viewport so a later reconnect's
    // replayed ClientHello carries the current dimensions, not the
    // constructor-time snapshot. The server reads ClientHello before
    // generating the initial sync, so stale dimensions there cause a
    // visible re-flow.
    //
    // Validate first: the Rust server tolerates a malformed `Resize`
    // (it ignores out-of-range values), but `encodeClientHello`
    // doesn't — if we silently cached `width: 0` (which happens when
    // the canvas is hidden) or a non-finite cell dimension, the next
    // reconnect would `throw HandshakeError` and the transport would
    // close, stranding the client. Round-trip through
    // `encodeClientHello` so we use the exact same validation path
    // the handshake will later use.
    if (msg.tag === "Resize") {
      const candidate: ClientHello = {
        sessionName: this.hello.sessionName,
        width: msg.width,
        height: msg.height,
        cellWidth: msg.cellWidth,
        cellHeight: msg.cellHeight,
      };
      try {
        encodeClientHello(candidate);
        this.hello = candidate;
      } catch {
        // Skip the update. The bad Resize still goes on the wire —
        // the server tolerates and ignores it — but the replay
        // hello keeps the last-known-good dims so reconnect still
        // succeeds.
      }
    }
    this.transport.send(frameClientMsg(payload));
  }

  /** Send `Input` for a pane, automatically advancing the
   *  monotonically-increasing `input_seq` the server uses for
   *  echo-ack tracking. Returns the sequence number assigned to this
   *  payload so callers driving local-echo prediction can hook the
   *  later ack. */
  sendInput(paneId: bigint, data: Uint8Array): bigint {
    const seq = this.nextInputSeq;
    this.nextInputSeq += 1n;
    this.send({ tag: "Input", paneId, data, inputSeq: seq });
    return seq;
  }

  // ─── Inbound plumbing ────────────────────────────────────────────

  private handleSocketOpen(): void {
    this.resetHandshakeState();
    // Replay ClientHello on every open — fresh connect AND reconnect
    // share the same first move, so the server's session-attach code
    // path doesn't need to know which case it's in.
    try {
      const bytes = encodeClientHello(this.hello);
      this.transport.send(bytes);
    } catch (e) {
      const err = e instanceof Error ? e : new Error(String(e));
      this.onEvent({ kind: "error", error: err });
      this.transport.close(1002, "client hello failed to encode");
    }
  }

  private handleSocketClose(info: {
    code: number;
    reason: string;
    reconnecting: boolean;
  }): void {
    // After any close the frame-level reader is meaningless — drop
    // its buffer; the next reconnect's handshake starts from scratch.
    this.resetHandshakeState();
    if (!info.reconnecting) this._state = "closed";
    else this._state = "connecting";
    this.onEvent({
      kind: "close",
      reason: info.reason || `code ${info.code}`,
      reconnecting: info.reconnecting,
    });
  }

  private handleData(bytes: Uint8Array): void {
    let cursor = bytes;
    if (this._state !== "open") {
      cursor = this.consumeHandshake(cursor);
      if (cursor.length === 0) return;
    }
    try {
      this.reader.push(cursor);
    } catch (e) {
      // FrameReader poisons on the first decode fault — drop the
      // connection so reconnect (if enabled) starts clean rather
      // than retrying against a corrupted byte stream.
      this.emitFatalDecodeError(e);
      return;
    }
    this.drainFrames();
  }

  private consumeHandshake(chunk: Uint8Array): Uint8Array {
    const need = SERVER_HELLO_LEN - this.handshakeBuf.length;
    if (chunk.length < need) {
      const combined = new Uint8Array(this.handshakeBuf.length + chunk.length);
      combined.set(this.handshakeBuf, 0);
      combined.set(chunk, this.handshakeBuf.length);
      this.handshakeBuf = combined;
      return new Uint8Array(0);
    }
    const helloBytes = new Uint8Array(SERVER_HELLO_LEN);
    helloBytes.set(this.handshakeBuf, 0);
    helloBytes.set(chunk.subarray(0, need), this.handshakeBuf.length);
    let info;
    try {
      info = decodeServerHello(helloBytes);
    } catch (e) {
      const err =
        e instanceof HandshakeError
          ? e
          : new HandshakeError("ServerHello decode failed", e);
      this.onEvent({ kind: "error", error: err });
      this.transport.close(1002, "bad ServerHello");
      return new Uint8Array(0);
    }
    this._state = "open";
    this.handshakeBuf = new Uint8Array(0);
    this.onEvent({ kind: "open", compat: info.compat });
    return chunk.subarray(need);
  }

  private drainFrames(): void {
    for (;;) {
      let frame: RawFrame | null;
      try {
        frame = this.reader.next();
      } catch (e) {
        this.emitFatalDecodeError(e);
        return;
      }
      if (frame === null) return;
      this.dispatchFrame(frame);
    }
  }

  private dispatchFrame(frame: RawFrame): void {
    switch (frame.kind) {
      case "server-msg": {
        let msg;
        try {
          msg = decodeServerMessage(frame.payload);
        } catch (e) {
          const err =
            e instanceof CodecError
              ? e
              : new CodecError("ServerMessage decode failed", e);
          this.onEvent({ kind: "error", error: err });
          return;
        }
        // Keep the replay ClientHello in sync with the server's view
        // of which session we're attached to. After `SwitchSession`
        // (or an auto-switch following `KillSession`), the server
        // sends `SessionSwitched`; without this update, a later
        // reconnect would silently reattach to the OLD session.
        if (msg.tag === "SessionSwitched") {
          this.hello = { ...this.hello, sessionName: msg.sessionName };
        }
        this.onEvent({ kind: "server-msg", msg });
        return;
      }
      case "cell-delta":
        this.onEvent({ kind: "cell-delta", payload: frame.payload });
        return;
      case "full-pane-sync":
        this.onEvent({ kind: "full-pane-sync", payload: frame.payload });
        return;
      case "client-msg":
        // Server should never forward a `TAG_CLIENT_MSG` frame at us.
        // Treat as a protocol error — log and continue rather than
        // crashing the loop so a hostile peer can't trivially DoS us.
        this.onEvent({
          kind: "error",
          error: new Error(
            "received client-msg frame from server (protocol violation)",
          ),
        });
        return;
      default:
        this.onEvent({
          kind: "error",
          error: new Error(`unknown frame kind: ${String(frame.kind)}`),
        });
    }
  }

  private emitFatalDecodeError(e: unknown): void {
    const err = e instanceof Error ? e : new Error(String(e));
    this.onEvent({ kind: "error", error: err });
    this.transport.close(1002, "frame decode error");
  }

  private resetHandshakeState(): void {
    this.handshakeBuf = new Uint8Array(0);
    // The FrameReader has no in-place `reset` — once poisoned, it
    // stays poisoned for the instance's life. Replace on every
    // reconnect so the new handshake starts from a clean buffer.
    this.reader = new FrameReader();
  }
}

const FRAME_HEADER_LEN = 5;

function frameClientMsg(payload: Uint8Array): Uint8Array {
  const buf = new Uint8Array(FRAME_HEADER_LEN + payload.length);
  buf[0] = TAG_CLIENT_MSG;
  new DataView(buf.buffer).setUint32(1, payload.length, /* littleEndian */ true);
  buf.set(payload, FRAME_HEADER_LEN);
  return buf;
}
