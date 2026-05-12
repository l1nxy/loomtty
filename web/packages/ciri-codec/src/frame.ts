// Streaming frame reader for the ciritty wire format.
//
//   Wire: [u8 tag] [u32 LE payload_len] [payload bytes]
//
// One WebSocket Binary message can carry zero, one, or many frames (and
// a frame can straddle WS message boundaries — the AsyncRead adapter
// on the server is stream-oriented). The reader accumulates bytes via
// `push()` and yields complete `Frame` values via `next()` until the
// buffer doesn't hold a full frame.

import {
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

/// Control-message frames carry a 1 MiB cap on the Rust side (see
/// `MAX_CONTROL_FRAME_LEN` in `codec/frame.rs`). Mirrored here as a
/// local literal — the Rust constant is `pub(super)`, but the value is
/// part of the wire contract and won't move without a protocol bump.
const MAX_CONTROL_FRAME_LEN = 1024 * 1024;

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

  /// Append raw bytes (typically the payload of one WS Binary message).
  push(chunk: Uint8Array): void {
    if (chunk.length === 0) return;
    const next = new Uint8Array(this.buf.length + chunk.length);
    next.set(this.buf, 0);
    next.set(chunk, this.buf.length);
    this.buf = next;
  }

  /// Pull the next complete frame, or `null` if the buffer doesn't hold
  /// at least one full frame yet. Throws on a frame whose declared
  /// length exceeds the protocol cap or whose tag is unknown — both
  /// indicate a peer the caller should disconnect from.
  next(): RawFrame | null {
    if (this.buf.length < FRAME_HEADER_LEN) return null;
    const tag = this.buf[0];
    if (tag === undefined) return null;
    const view = new DataView(
      this.buf.buffer,
      this.buf.byteOffset,
      this.buf.byteLength,
    );
    const payloadLen = view.getUint32(1, /* littleEndian */ true);

    const limit = limitForTag(tag);
    if (payloadLen > limit) {
      throw new Error(
        `frame too large: tag=0x${tag.toString(16)}, len=${payloadLen}, limit=${limit}`,
      );
    }
    const total = FRAME_HEADER_LEN + payloadLen;
    if (this.buf.length < total) return null;

    const payload = this.buf.subarray(FRAME_HEADER_LEN, total);
    const frame = decodeFrame(tag, payload);

    // Advance: slice off everything we just consumed. Use slice (not
    // subarray) so the underlying ArrayBuffer can shrink eventually.
    this.buf = this.buf.slice(total);
    return frame;
  }

  /// Number of bytes still buffered (incomplete frame or empty).
  pending(): number {
    return this.buf.length;
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
  switch (tag) {
    case TAG_CLIENT_MSG:
      return { kind: "client-msg", payload: payload.slice() };
    case TAG_SERVER_MSG:
      return { kind: "server-msg", payload: payload.slice() };
    case TAG_CELL_DELTA:
      return { kind: "cell-delta", payload: payload.slice() };
    case TAG_FULL_PANE_SYNC:
      return { kind: "full-pane-sync", payload: payload.slice() };
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
