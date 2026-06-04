// Wire-body decoders for the two binary frame payloads — CellDelta
// (tag 0x20) and FullPaneSync (tag 0x21). Layouts mirror the Rust
// encoders in `crates/loom-protocol/src/codec/{cell_delta,full_sync}.rs`
// byte-for-byte; round-trip tests in `./frame-body.test.ts` pin every
// fixture against Rust-emitted hex.
//
// Both inputs are the *decompressed* payload — the FrameReader peels
// the 5-byte tag/length header and decompresses LZ4 variants before
// handing the bytes here. A truncated buffer, oversize length, or
// extra trailing bytes after the structured fields surface as throws;
// callers (`@loom/client.dispatchFrame`) must drop the connection on
// any throw rather than retrying.

import { MAX_DATA_FRAME_LEN } from "./constants.js";
import { decodeSmCellsInto } from "./state-machine.js";
import {
  DEFAULT_CELL,
  type CellDelta,
  type DamageRegion,
  type FullPaneSync,
  type PackedCell,
  type PaneFrameMeta,
} from "./types.js";

export class FrameBodyDecodeError extends Error {
  override name = "FrameBodyDecodeError";
  constructor(message: string, cause?: unknown) {
    super(message, cause !== undefined ? { cause } : undefined);
  }
}

// Soft cap derived from the wire frame cap. Both grids are stored as
// flat row-major arrays; with cells × cols beyond this we'd allocate
// hundreds of MB of JS heap per pane. The Rust side also enforces
// `MAX_GRID_CELLS = 10_000_000`. Picking the smaller of the two so a
// future Rust bump tightens us automatically.
const MAX_GRID_CELLS = 10_000_000;

// A small streaming cursor over a Uint8Array. The Rust side uses
// `SliceCursor` for the same role; this is a near-direct port.
//
// All read methods throw `FrameBodyDecodeError` when the remaining
// buffer is too short — callers don't need to length-check ahead of
// each read.
class Cursor {
  private pos = 0;
  private readonly view: DataView;

  constructor(private readonly data: Uint8Array) {
    this.view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  }

  position(): number {
    return this.pos;
  }

  remaining(): number {
    return this.data.length - this.pos;
  }

  private need(n: number, ctx: string): void {
    if (this.pos + n > this.data.length) {
      throw new FrameBodyDecodeError(
        `truncated: ${ctx} (pos=${this.pos}, need=${n}, len=${this.data.length})`,
      );
    }
  }

  readU8(ctx: string): number {
    this.need(1, ctx);
    const v = this.view.getUint8(this.pos);
    this.pos += 1;
    return v;
  }

  readU16(ctx: string): number {
    this.need(2, ctx);
    const v = this.view.getUint16(this.pos, /* le */ true);
    this.pos += 2;
    return v;
  }

  readI16(ctx: string): number {
    this.need(2, ctx);
    const v = this.view.getInt16(this.pos, /* le */ true);
    this.pos += 2;
    return v;
  }

  readU32(ctx: string): number {
    this.need(4, ctx);
    const v = this.view.getUint32(this.pos, /* le */ true);
    this.pos += 4;
    return v;
  }

  readU64BigInt(ctx: string): bigint {
    this.need(8, ctx);
    const v = this.view.getBigUint64(this.pos, /* le */ true);
    this.pos += 8;
    return v;
  }

  readBytes(n: number, ctx: string): Uint8Array {
    this.need(n, ctx);
    // `subarray` is a view, not a copy — the body decoders only hold
    // these slices as long as the source `payload` lives, which the
    // FrameReader already keeps alive across the decode call. We
    // deliberately avoid the per-read copy a `.slice()` would do; the
    // SM cells produced from the slice are immediately copied into a
    // pre-allocated `PackedCell[]` so the source bytes can be GC'd
    // once `decodeCellDelta` / `decodeFullPaneSync` returns.
    const bytes = this.data.subarray(this.pos, this.pos + n);
    this.pos += n;
    return bytes;
  }

  /** Length-prefixed UTF-8 string with a u16 length prefix. */
  readStrU16(ctx: string): string {
    const len = this.readU16(`${ctx} len`);
    const bytes = this.readBytes(len, `${ctx} bytes`);
    return utf8Decode(bytes, ctx);
  }

  /** Length-prefixed UTF-8 string with a u8 length prefix. */
  readStrU8(ctx: string): string {
    const len = this.readU8(`${ctx} len`);
    const bytes = this.readBytes(len, `${ctx} bytes`);
    return utf8Decode(bytes, ctx);
  }

  expectEnd(ctx: string): void {
    if (this.pos !== this.data.length) {
      const extra = this.data.length - this.pos;
      throw new FrameBodyDecodeError(
        `${ctx}: ${extra} trailing byte(s) after structured fields`,
      );
    }
  }
}

// Strict UTF-8 decode. Mirrors the SM cell-char decoder's `fatal: true`
// posture so a malicious peer can't slip lone surrogates or overlong
// encodings through the renderer. The renderer otherwise has no
// protection against malformed strings — it splices them into
// `<span>` text nodes verbatim.
const utf8DecoderStrict = new TextDecoder("utf-8", { fatal: true });
function utf8Decode(bytes: Uint8Array, ctx: string): string {
  try {
    return utf8DecoderStrict.decode(bytes);
  } catch (e) {
    throw new FrameBodyDecodeError(
      `invalid UTF-8 in ${ctx}: ${e instanceof Error ? e.message : String(e)}`,
      e,
    );
  }
}

// ─── CellDelta ──────────────────────────────────────────────────────

/**
 * Decode the body of a `cell-delta` frame (tag 0x20 / 0x22, after the
 * FrameReader has stripped the 5-byte header and decompressed LZ4
 * variants).
 *
 * Throws `FrameBodyDecodeError` on truncation, oversize geometry,
 * inverted region bounds, malformed SM payloads, or trailing bytes
 * past the last region. The caller (LoomClient) must drop the
 * connection on any throw.
 */
export function decodeCellDelta(payload: Uint8Array): CellDelta {
  const cur = new Cursor(payload);

  const meta = readPaneFrameMeta(cur);
  const cols = cur.readU16("CellDelta cols");
  const numRegions = cur.readU16("CellDelta num_regions");

  const regions: DamageRegion[] = new Array(numRegions);
  for (let i = 0; i < numRegions; i += 1) {
    const line = cur.readU16(`region[${i}] line`);
    const left = cur.readU16(`region[${i}] left`);
    const right = cur.readU16(`region[${i}] right`);
    if (left > right) {
      throw new FrameBodyDecodeError(
        `region[${i}] invalid damage bounds: left=${left} > right=${right}`,
      );
    }
    if (right >= cols) {
      // `right` is inclusive; a region that runs past the column
      // count is wire corruption (the encoder never produces this).
      // Without the guard, the cell-count math below would happily
      // produce a region wider than the grid and the renderer would
      // splice past the row boundary.
      throw new FrameBodyDecodeError(
        `region[${i}] right=${right} >= cols=${cols}`,
      );
    }
    const smLen = cur.readU32(`region[${i}] sm_data_len`);
    if (smLen > MAX_DATA_FRAME_LEN) {
      // A region's SM payload can't be larger than the whole frame —
      // catch the obvious garbage before allocating.
      throw new FrameBodyDecodeError(
        `region[${i}] sm_data_len=${smLen} exceeds MAX_DATA_FRAME_LEN`,
      );
    }
    const smBytes = cur.readBytes(smLen, `region[${i}] sm_data`);
    const cellCount = right - left + 1;
    const cells: PackedCell[] = new Array(cellCount).fill(DEFAULT_CELL);
    let written = 0;
    try {
      written = decodeSmCellsInto(smBytes, cells, cellCount);
    } catch (e) {
      throw new FrameBodyDecodeError(
        `region[${i}] SM decode failed`,
        e,
      );
    }
    if (written !== cellCount) {
      throw new FrameBodyDecodeError(
        `region[${i}] SM decoded ${written} cells, expected ${cellCount}`,
      );
    }
    regions[i] = { line, left, right, cells };
  }

  cur.expectEnd("CellDelta");
  return { meta, cols, regions };
}

// ─── FullPaneSync ───────────────────────────────────────────────────

/**
 * Decode the body of a `full-pane-sync` frame (tag 0x21 / 0x23, after
 * FrameReader header strip + LZ4 decompress).
 *
 * Both the scrollback and viewport SM payloads are decoded eagerly
 * into flat row-major cell arrays. Grapheme overflow and OSC 8
 * hyperlink extras are accumulated into Maps keyed by cell index over
 * the concatenated `[scrollback..., cells...]` stream — the same key
 * space the Rust encoder uses.
 *
 * Throws `FrameBodyDecodeError` on truncation, oversize geometry,
 * malformed UTF-8, malformed SM payloads, or trailing bytes after the
 * cwd field. The caller must drop the connection on any throw.
 */
export function decodeFullPaneSync(payload: Uint8Array): FullPaneSync {
  const cur = new Cursor(payload);

  const meta: PaneFrameMeta = {
    paneId: cur.readU64BigInt("FullPaneSync pane_id"),
    generation: cur.readU64BigInt("FullPaneSync generation"),
    cursorLine: 0,
    cursorCol: 0,
    cursorShape: 0,
    modeFlags: 0,
    receivedAck: 0n,
    echoAck: 0n,
  };
  const cols = cur.readU16("FullPaneSync cols");
  const rows = cur.readU16("FullPaneSync rows");
  meta.cursorLine = cur.readI16("FullPaneSync cursor_line");
  meta.cursorCol = cur.readU16("FullPaneSync cursor_col");
  meta.cursorShape = cur.readU8("FullPaneSync cursor_shape");
  meta.modeFlags = cur.readU16("FullPaneSync mode_flags");
  // received_ack precedes echo_ack in the packed header (both u64).
  meta.receivedAck = cur.readU64BigInt("FullPaneSync received_ack");
  meta.echoAck = cur.readU64BigInt("FullPaneSync echo_ack");
  const title = cur.readStrU16("FullPaneSync title");

  // Both `cols × rows` and `scrollback_rows × cols` are checked
  // against MAX_GRID_CELLS to mirror the Rust decoder's guard. JS
  // `new Array(N).fill(DEFAULT_CELL)` of 10M entries is ~80MB of
  // pointers — already pushing what we want a single pane to use,
  // so anything past that is wire corruption.
  const totalCells = cols * rows;
  if (totalCells > MAX_GRID_CELLS) {
    throw new FrameBodyDecodeError(
      `grid too large: ${cols}x${rows} = ${totalCells} cells (max ${MAX_GRID_CELLS})`,
    );
  }

  const scrollbackRows = cur.readU32("FullPaneSync scrollback_rows");
  const scrollbackReplace = cur.readU8("FullPaneSync scrollback_replace") !== 0;
  const sbExpected = scrollbackRows * cols;
  if (sbExpected > MAX_GRID_CELLS) {
    throw new FrameBodyDecodeError(
      `scrollback too large: ${scrollbackRows} rows x ${cols} cols = ${sbExpected} cells (max ${MAX_GRID_CELLS})`,
    );
  }
  const sbSmLen = cur.readU32("FullPaneSync scrollback sm_len");
  const sbSmBytes = cur.readBytes(sbSmLen, "FullPaneSync scrollback sm_data");
  const vpSmLen = cur.readU32("FullPaneSync viewport sm_len");
  const vpSmBytes = cur.readBytes(vpSmLen, "FullPaneSync viewport sm_data");

  const scrollback: PackedCell[] = new Array(sbExpected).fill(DEFAULT_CELL);
  if (sbExpected > 0) {
    let wrote = 0;
    try {
      wrote = decodeSmCellsInto(sbSmBytes, scrollback, sbExpected);
    } catch (e) {
      throw new FrameBodyDecodeError(
        "FullPaneSync scrollback SM decode failed",
        e,
      );
    }
    if (wrote !== sbExpected) {
      throw new FrameBodyDecodeError(
        `FullPaneSync scrollback SM decoded ${wrote} cells, expected ${sbExpected}`,
      );
    }
  } else if (sbSmBytes.length !== 0) {
    // Zero-row scrollback with non-empty SM bytes is wire corruption —
    // the Rust encoder always emits a single OP_END for an empty
    // cell list, which is 1 byte; the decoder accepts both 0-byte
    // and 1-byte forms (the latter via decodeSmCellsInto, the former
    // by skipping the call) but anything else is a contract violation.
    if (sbSmBytes.length !== 1 || sbSmBytes[0] !== 0xff) {
      throw new FrameBodyDecodeError(
        `FullPaneSync scrollback_rows=0 but SM data is ${sbSmBytes.length}B (not empty/OP_END)`,
      );
    }
  }
  const cells: PackedCell[] = new Array(totalCells).fill(DEFAULT_CELL);
  if (totalCells > 0) {
    let wrote = 0;
    try {
      wrote = decodeSmCellsInto(vpSmBytes, cells, totalCells);
    } catch (e) {
      throw new FrameBodyDecodeError(
        "FullPaneSync viewport SM decode failed",
        e,
      );
    }
    if (wrote !== totalCells) {
      throw new FrameBodyDecodeError(
        `FullPaneSync viewport SM decoded ${wrote} cells, expected ${totalCells}`,
      );
    }
  } else if (vpSmBytes.length !== 0) {
    if (vpSmBytes.length !== 1 || vpSmBytes[0] !== 0xff) {
      throw new FrameBodyDecodeError(
        `FullPaneSync zero viewport but SM data is ${vpSmBytes.length}B (not empty/OP_END)`,
      );
    }
  }

  const graphemeExtras = decodeGraphemeExtras(cur);
  const { cellLinks, linkMap } = decodeHyperlinkExtras(cur);
  const cwd = decodeCwd(cur);
  cur.expectEnd("FullPaneSync");

  return {
    meta,
    cols,
    rows,
    title,
    scrollback,
    scrollbackRows,
    scrollbackReplace,
    cells,
    graphemeExtras,
    cellLinks,
    linkMap,
    cwd,
  };
}

function readPaneFrameMeta(cur: Cursor): PaneFrameMeta {
  const paneId = cur.readU64BigInt("CellDelta pane_id");
  const generation = cur.readU64BigInt("CellDelta generation");
  const cursorLine = cur.readI16("CellDelta cursor_line");
  const cursorCol = cur.readU16("CellDelta cursor_col");
  const cursorShape = cur.readU8("CellDelta cursor_shape");
  const modeFlags = cur.readU16("CellDelta mode_flags");
  // received_ack precedes echo_ack in the packed header (both u64).
  const receivedAck = cur.readU64BigInt("CellDelta received_ack");
  const echoAck = cur.readU64BigInt("CellDelta echo_ack");
  return {
    paneId,
    generation,
    cursorLine,
    cursorCol,
    cursorShape,
    modeFlags,
    receivedAck,
    echoAck,
  };
}

function decodeGraphemeExtras(cur: Cursor): Map<number, string> {
  const count = cur.readU16("grapheme_extras count");
  // The Rust encoder pushes `(cell_index, extra)` pairs to a Vec —
  // a duplicate cell_index is technically possible. The TS Map
  // semantics are last-write-wins; mirror that explicitly so a
  // hand-crafted payload can't surprise us.
  const map = new Map<number, string>();
  for (let i = 0; i < count; i += 1) {
    const cellIdx = cur.readU32(`grapheme_extras[${i}] cell_index`);
    const extra = cur.readStrU8(`grapheme_extras[${i}] extra`);
    map.set(cellIdx, extra);
  }
  return map;
}

function decodeHyperlinkExtras(cur: Cursor): {
  cellLinks: Map<number, number>;
  linkMap: Map<number, string>;
} {
  const cellLinksCount = cur.readU16("hyperlink_extras cell_links count");
  const cellLinks = new Map<number, number>();
  for (let i = 0; i < cellLinksCount; i += 1) {
    const cellIdx = cur.readU32(`cell_links[${i}] cell_index`);
    const linkId = cur.readU16(`cell_links[${i}] link_id`);
    cellLinks.set(cellIdx, linkId);
  }
  const linkMapCount = cur.readU16("hyperlink_extras link_map count");
  const linkMap = new Map<number, string>();
  for (let i = 0; i < linkMapCount; i += 1) {
    const linkId = cur.readU16(`link_map[${i}] link_id`);
    const uri = cur.readStrU16(`link_map[${i}] uri`);
    linkMap.set(linkId, uri);
  }
  return { cellLinks, linkMap };
}

function decodeCwd(cur: Cursor): string | null {
  const len = cur.readU16("cwd len");
  if (len === 0) return null;
  const bytes = cur.readBytes(len, "cwd bytes");
  return utf8Decode(bytes, "cwd");
}
