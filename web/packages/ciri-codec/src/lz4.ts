// Thin wrapper around lz4js's raw-block decoder. The Rust gateway uses
// `lz4_flex::compress` which emits raw LZ4 block format (no LZ4 frame
// header), and prepends a `u32 LE uncompressed_len` itself — see the
// `decompress_lz4_payload` function in `ciri-protocol/src/codec/frame.rs`.
//
// `lz4js.decodeBlock(input, output, sIdx, eIdx)` reads the raw block and
// writes into the caller-supplied `output` slice. We pre-allocate based
// on the wire-supplied uncompressed length (which the Rust side has
// already validated against `MAX_DATA_FRAME_LEN` + a bomb-ratio guard,
// but we cap again here so a misbehaving server can't OOM the browser).

// @ts-expect-error — lz4js ships without type defs.
import lz4 from "lz4js";

import { MAX_DATA_FRAME_LEN } from "./constants.js";

const MAX_LZ4_RATIO = 64;

/// Decode an LZ4 frame payload of the shape
/// `[u32 LE uncompressed_len][lz4 block bytes…]` into the uncompressed
/// bytes. Mirrors `decompress_lz4_payload` in the Rust codec.
export function decompressLz4Payload(payload: Uint8Array): Uint8Array {
  if (payload.length < 4) {
    throw new Error("LZ4 payload too short (need 4-byte size header)");
  }
  // Header is little-endian u32; pull via DataView to be explicit.
  const view = new DataView(
    payload.buffer,
    payload.byteOffset,
    payload.byteLength,
  );
  const uncompressedLen = view.getUint32(0, /* littleEndian */ true);

  if (uncompressedLen > MAX_DATA_FRAME_LEN) {
    throw new Error(
      `LZ4 uncompressed length ${uncompressedLen} exceeds MAX_DATA_FRAME_LEN ${MAX_DATA_FRAME_LEN}`,
    );
  }
  const compressed = payload.subarray(4);
  // Mirror the bomb-ratio + zero-compressed-len guards from the Rust
  // side so a malicious server-side codec swap can't slip past.
  if (compressed.length === 0 && uncompressedLen > 0) {
    throw new Error(
      "LZ4 payload claims non-zero uncompressed length with zero compressed bytes",
    );
  }
  if (
    compressed.length > 0 &&
    uncompressedLen > MAX_LZ4_RATIO * compressed.length
  ) {
    throw new Error(
      `LZ4 compression ratio ${
        Math.round(uncompressedLen / compressed.length)
      }:1 exceeds limit ${MAX_LZ4_RATIO}:1`,
    );
  }

  const output = new Uint8Array(uncompressedLen);
  // lz4js exposes a few overlapping decode functions across releases;
  // probe for whichever exists at runtime.
  if (typeof lz4.decodeBlock === "function") {
    const written: number = lz4.decodeBlock(
      compressed,
      output,
      0,
      compressed.length,
    );
    if (written !== uncompressedLen) {
      throw new Error(
        `lz4 decode produced ${written} bytes, expected ${uncompressedLen}`,
      );
    }
    return output;
  }
  if (typeof lz4.uncompressBlock === "function") {
    const written: number = lz4.uncompressBlock(compressed, output);
    if (written !== uncompressedLen) {
      throw new Error(
        `lz4 decode produced ${written} bytes, expected ${uncompressedLen}`,
      );
    }
    return output;
  }
  throw new Error("lz4js has no decodeBlock/uncompressBlock — version mismatch?");
}
