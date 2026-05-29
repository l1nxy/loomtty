// TypeScript mirrors of the wire types in `ciri-protocol`. Field
// layouts and discriminant values are stable wire contracts — keep
// these in sync with `crates/ciri-protocol/src/message.rs`. The numeric
// tags live in `./constants.ts` (auto-generated; do not hand-edit).

import { COLOR_INDEXED, COLOR_NAMED, COLOR_RGB } from "./constants.js";

/// Packed color discriminant. Mirrors the Rust `PackedColor` enum via
/// the (tag, b1, b2, b3) wire layout but exposes a friendlier TS shape.
export type PackedColor =
  | { kind: "named"; index: number }
  | { kind: "indexed"; index: number }
  | { kind: "rgb"; r: number; g: number; b: number };

/// One terminal cell. Mirrors `PackedCell` (16 bytes on the wire). The
/// `ch` field carries the primary codepoint as a JS string; multi-byte
/// graphemes (emoji, ZWJ sequences) live in the sparse `graphemeExtras`
/// map attached to the enclosing FullPaneSync.
export interface PackedCell {
  ch: string;
  fg: PackedColor;
  bg: PackedColor;
  flags: number;
}

/// Build a `PackedColor` from the (tag, b1, b2, b3) wire bytes.
export function decodePackedColor(
  tag: number,
  b1: number,
  b2: number,
  b3: number,
): PackedColor {
  switch (tag) {
    case COLOR_NAMED:
      return { kind: "named", index: b1 };
    case COLOR_INDEXED:
      return { kind: "indexed", index: b1 };
    case COLOR_RGB:
      return { kind: "rgb", r: b1, g: b2, b: b3 };
    default:
      throw new RangeError(`unknown PackedColor tag 0x${tag.toString(16)}`);
  }
}

/// Color-equality on the wire shape. Comparing by deep-equal would
/// allocate; the SM decoder compares colors thousands of times per
/// frame, so this hand-rolled compare is a hot-path concession.
export function packedColorEq(a: PackedColor, b: PackedColor): boolean {
  if (a.kind !== b.kind) return false;
  switch (a.kind) {
    case "named":
    case "indexed":
      return a.index === (b as typeof a).index;
    case "rgb": {
      const r = b as { r: number; g: number; b: number };
      return a.r === r.r && a.g === r.g && a.b === r.b;
    }
  }
}

/// Default fg/bg per `DEFAULT_FOREGROUND` / `DEFAULT_BACKGROUND`.
/// Frozen so a careless `DEFAULT_FG.index = 0` somewhere downstream
/// cannot poison the module-global default for every subsequent decode.
export const DEFAULT_FG: Readonly<PackedColor> = Object.freeze({
  kind: "named",
  index: 16,
});
export const DEFAULT_BG: Readonly<PackedColor> = Object.freeze({
  kind: "named",
  index: 17,
});
export const DEFAULT_CELL: Readonly<PackedCell> = Object.freeze({
  ch: " ",
  fg: DEFAULT_FG,
  bg: DEFAULT_BG,
  flags: 0,
});

/// Per-frame pane metadata shared by CellDelta and FullPaneSync.
export interface PaneFrameMeta {
  paneId: bigint;
  generation: bigint;
  cursorLine: number; // i16
  cursorCol: number; // u16
  cursorShape: number; // u8
  modeFlags: number; // u16
  receivedAck: bigint; // u64 — early packet-level ack (precedes echoAck on the wire)
  echoAck: bigint; // u64
}

/// One contiguous run of changed cells on a single row.
export interface DamageRegion {
  line: number; // u16
  left: number; // u16
  right: number; // u16, inclusive
  cells: PackedCell[];
}

/// Decoded CellDelta (frame tag 0x20 / 0x22).
export interface CellDelta {
  meta: PaneFrameMeta;
  cols: number;
  regions: DamageRegion[];
}

/// Decoded FullPaneSync (frame tag 0x21 / 0x23).
export interface FullPaneSync {
  meta: PaneFrameMeta;
  cols: number;
  rows: number;
  title: string;
  scrollback: PackedCell[]; // row-major, scrollbackRows * cols
  scrollbackRows: number;
  scrollbackReplace: boolean;
  cells: PackedCell[]; // row-major, rows * cols
  /// `cellIndex → full grapheme string`. `cellIndex` runs over the
  /// concatenation `[scrollback..., cells...]` so callers can splice
  /// into either buffer with the same key space.
  graphemeExtras: Map<number, string>;
  /// `cellIndex → linkId`, plus `linkId → uri`. Empty unless the TUI
  /// emitted OSC 8 links.
  cellLinks: Map<number, number>;
  linkMap: Map<number, string>;
  cwd: string | null;
}
