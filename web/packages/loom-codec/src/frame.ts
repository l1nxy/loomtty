// Streaming frame reader for the loomtty wire format.
//
//   Wire: [u8 tag] [u32 LE payload_len] [payload bytes]
//
// One WebSocket Binary message can carry zero, one, or many frames (and
// a frame can straddle WS message boundaries — the AsyncRead adapter
// on the server is stream-oriented). The reader accumulates bytes via
// `push()` and yields complete `Frame` values via `next()` until the
// buffer doesn't hold a full frame.

import {
  MAX_CONTROL_FRAME_LEN,
  MAX_DATA_FRAME_LEN,
  TAG_CELL_DELTA,
  TAG_CELL_DELTA_LZ4,
  TAG_CLIENT_MSG,
  TAG_FULL_PANE_SYNC,
  TAG_FULL_PANE_SYNC_LZ4,
  TAG_SERVER_MSG,
} from "./constants.js";
import { decompressLz4Payload } from "./lz4.js";

const FRAME_HEADER_LEN = 5;

/// Frame direction tags.
///
/// In the typical browser-client deployment the gateway is a one-way
/// receiver: `server-msg`, `cell-delta`, and `full-pane-sync` are the
/// only kinds the client should receive from the wire. `client-msg`
/// (tag 0x01) is what the *client* sends *to* the server — receiving
/// it on the client side means the server is buggy or hostile, and
/// callers should treat it as an error rather than dispatching to the
/// server-message handler. The decoder still surfaces the kind so the
/// caller can implement that policy (e.g., disconnect on bad
/// direction).
export type FrameKind =
  | "client-msg"
  | "server-msg"
  | "cell-delta"
  | "full-pane-sync";

/// A successfully-extracted frame. The msgpack/SM payloads are kept as
/// raw bytes here; higher-level decoders in `./message.ts` and
/// `./cell-delta.ts` / `./full-sync.ts` consume them.
export interface RawFrame {
  kind: FrameKind;
  /// Always the **decompressed** payload — the LZ4 variants
  /// (`TAG_*_LZ4`) are unwrapped before `next()` returns the frame.
  payload: Uint8Array;
}

/// Stateful frame reader. Buffers bytes pushed in via `push()` and
/// returns one frame at a time from `next()` until the buffer no longer
/// contains a full frame.
export class FrameReader {
  // Single growable buffer. We slice the head off after each consumed
  // frame; for small frames this is fine, and large frames (CellDelta
  // / FullPaneSync) are usually one-per-WS-message anyway.
  private buf: Uint8Array = new Uint8Array(0);
  // Once a parse fault is observed (oversize frame, unknown tag, LZ4
  // garbage) the reader cannot continue: it has no way to advance
  // past the offending bytes safely. Stay in a poisoned state so a
  // caller that retries `next()` gets a deterministic error rather
  // than spinning on the same fault.
  private poisonReason: string | null = null;
  private poisonCause: unknown = undefined;

  /// Append raw bytes (typically the payload of one WS Binary message).
  /// Always copies the inbound chunk (the caller may reuse its buffer
  /// — e.g. a pooled WS receive backing — so we must own the bytes);
  /// when the internal buffer is empty the copy collapses to a single
  /// `.slice()` instead of an alloc-then-copy of `old + new`.
  push(chunk: Uint8Array): void {
    if (this.poisonReason !== null) {
      throw new Error(`FrameReader is poisoned: ${this.poisonReason}`, {
        cause: this.poisonCause,
      });
    }
    if (chunk.length === 0) return;
    if (this.buf.length === 0) {
      this.buf = chunk.slice();
      return;
    }
    const next = new Uint8Array(this.buf.length + chunk.length);
    next.set(this.buf, 0);
    next.set(chunk, this.buf.length);
    this.buf = next;
  }

  /// Pull the next complete frame, or `null` if the buffer doesn't hold
  /// at least one full frame yet. Throws on a frame whose declared
  /// length exceeds the protocol cap or whose tag is unknown — both
  /// indicate a peer the caller should disconnect from. After any
  /// throw the reader is poisoned and all subsequent calls will
  /// re-throw immediately with a "poisoned" message.
  next(): RawFrame | null {
    if (this.poisonReason !== null) {
      throw new Error(`FrameReader is poisoned: ${this.poisonReason}`, {
        cause: this.poisonCause,
      });
    }
    if (this.buf.length < FRAME_HEADER_LEN) return null;
    const tag = this.buf[0]!;
    const view = new DataView(
      this.buf.buffer,
      this.buf.byteOffset,
      this.buf.byteLength,
    );
    const payloadLen = view.getUint32(1, /* littleEndian */ true);

    try {
      const limit = limitForTag(tag);
      if (payloadLen > limit) {
        throw new Error(
          `frame too large: tag=0x${tag.toString(16)}, len=${payloadLen}, limit=${limit}`,
        );
      }
      const total = FRAME_HEADER_LEN + payloadLen;
      if (this.buf.length < total) return null;

      // Take an owned copy of the payload BEFORE advancing the buffer,
      // and advance via subarray on the original — that releases the
      // old ArrayBuffer once both `payload` and the next-frame slice
      // are independent. Previously the uncompressed branches did
      // `this.buf.slice(total)` AND then `payload.slice()` inside
      // decodeFrame, briefly holding two full copies of the payload
      // (peak 2× the frame size for the ~ms duration of decodeFrame).
      const payload = this.buf.slice(FRAME_HEADER_LEN, total);
      this.buf =
        this.buf.length === total
          ? new Uint8Array(0)
          : this.buf.slice(total);
      return decodeFrame(tag, payload);
    } catch (e) {
      // Preserve the original error and stash both its message (for
      // subsequent `isPoisoned`/throw diagnostics) and the error
      // itself (as `cause` on later re-throws) so root-cause analysis
      // survives across the poisoned boundary.
      this.poisonReason =
        e instanceof Error ? e.message : "unknown frame decode error";
      this.poisonCause = e;
      // Drop the buffer too: there is no recovery path that could
      // ever consume those bytes, and keeping them alive misleads
      // `pending()` callers into thinking work is outstanding.
      this.buf = new Uint8Array(0);
      throw e;
    }
  }

  /// Number of bytes still buffered (incomplete frame or empty). A
  /// poisoned reader always reports 0; the buffer is discarded on the
  /// fault to keep this method's contract simple (`> 0` ⇒ a `next()`
  /// call may still produce a frame).
  pending(): number {
    return this.buf.length;
  }

  /// True after any parse fault has terminated the reader. Once
  /// poisoned, every `push` / `next` throws.
  isPoisoned(): boolean {
    return this.poisonReason !== null;
  }
}

function limitForTag(tag: number): number {
  switch (tag) {
    case TAG_CLIENT_MSG:
    case TAG_SERVER_MSG:
      return MAX_CONTROL_FRAME_LEN;
    case TAG_CELL_DELTA:
    case TAG_FULL_PANE_SYNC:
    case TAG_CELL_DELTA_LZ4:
    case TAG_FULL_PANE_SYNC_LZ4:
      return MAX_DATA_FRAME_LEN;
    default:
      // Unknown tag — reject at the header without ever allocating a
      // payload buffer. Mirrors the server-side guard.
      throw new Error(`unknown frame tag: 0x${tag.toString(16)}`);
  }
}

function decodeFrame(tag: number, payload: Uint8Array): RawFrame {
  // `payload` is already an owned copy of the bytes (taken via
  // `this.buf.slice(...)` in FrameReader.next before this is called),
  // so the uncompressed branches don't need to re-copy. The LZ4
  // branches always allocate their own decompressed buffer.
  switch (tag) {
    case TAG_CLIENT_MSG:
      return { kind: "client-msg", payload };
    case TAG_SERVER_MSG:
      return { kind: "server-msg", payload };
    case TAG_CELL_DELTA:
      return { kind: "cell-delta", payload };
    case TAG_FULL_PANE_SYNC:
      return { kind: "full-pane-sync", payload };
    case TAG_CELL_DELTA_LZ4:
      return { kind: "cell-delta", payload: decompressLz4Payload(payload) };
    case TAG_FULL_PANE_SYNC_LZ4:
      return {
        kind: "full-pane-sync",
        payload: decompressLz4Payload(payload),
      };
    default:
      // limitForTag above already rejects unknown tags; this branch is
      // unreachable but keeps the type checker happy.
      throw new Error(`unknown frame tag: 0x${tag.toString(16)}`);
  }
}
