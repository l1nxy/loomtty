// Public entry point of `@ciri/codec`.
//
// The codec mirrors the wire side of `ciri-protocol` (Rust). Stable
// numeric constants are auto-generated from the Rust source by the
// `dump_constants` example — see `./constants.ts`. Type definitions
// (`./types.ts`) and decoders (`./frame.ts`, `./state-machine.ts`,
// `./lz4.ts`) are hand-written to mirror the Rust layouts; integration
// tests against Rust-emitted fixtures guard against drift.

export * from "./constants.js";
export * from "./types.js";
export { FrameReader, type RawFrame, type FrameKind } from "./frame.js";
export { decodeSmCells, decodeSmCellsInto } from "./state-machine.js";
export { decompressLz4Payload } from "./lz4.js";
