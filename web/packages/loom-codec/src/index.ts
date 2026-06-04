// Public entry point of `@loom/codec`.
//
// The codec mirrors the wire side of `loom-protocol` (Rust). Stable
// numeric constants are auto-generated from the Rust source by the
// `dump_constants` example — see `./constants.ts`. Type definitions
// (`./types.ts`) and decoders (`./frame.ts`, `./state-machine.ts`,
// `./lz4.ts`) are hand-written to mirror the Rust layouts; integration
// tests against Rust-emitted fixtures guard against drift.

export * from "./constants.js";
export * from "./types.js";
export { FrameReader, type FrameKind, type RawFrame } from "./frame.js";
export { decodeSmCells, decodeSmCellsInto } from "./state-machine.js";
export { decompressLz4Payload } from "./lz4.js";
export {
  FrameBodyDecodeError,
  decodeCellDelta,
  decodeFullPaneSync,
} from "./frame-body.js";
