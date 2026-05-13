// Pane state model. Holds a flat row-major cell buffer for the
// viewport, a separate scrollback buffer, and the sparse grapheme +
// hyperlink extras attached to specific cell indices.
//
// Mutates in place — `applyCellDelta` splices each region into the
// existing row, `applyFullPaneSync` replaces the whole world. Dirty
// row tracking is a simple `Set<number>` over viewport-relative rows;
// the renderer consumes it via `takeDirtyRows()` and clears.
//
// What this DOES NOT do:
//   * render anything — that's `renderer.ts`
//   * decode wire bytes — `decodeCellDelta` / `decodeFullPaneSync` in
//     `@ciri/codec` do it.
//   * reconcile a stale `generation` — the app layer is expected to
//     pair generations with its own bookkeeping. The grid applies
//     whatever it's handed.

import {
  DEFAULT_CELL,
  type CellDelta,
  type FullPaneSync,
  type PackedCell,
  type PaneFrameMeta,
} from "@ciri/codec";

export class GridShapeError extends Error {
  override name = "GridShapeError";
}

/// Result of `takeDirtyRows()` — the viewport-row indices that need
/// to be re-rendered, plus a `fullRedraw` flag set when the entire
/// viewport must be re-painted (FullPaneSync replaced everything).
export interface DirtyRows {
  /// Sorted ascending. May contain duplicates if a single render cycle
  /// captures multiple deltas to the same row — pre-deduped by Set.
  rows: number[];
  /// True when the renderer should also clear stale row containers
  /// (e.g., after a resize that shrank the grid).
  fullRedraw: boolean;
}

export class PaneGrid {
  readonly paneId: bigint;

  cols: number;
  rows: number;

  /// Row-major viewport: index = row * cols + col. Length = rows * cols.
  /// Always a flat `PackedCell[]` so the renderer can take row slices
  /// with `cells.slice(row*cols, (row+1)*cols)` without bookkeeping.
  cells: PackedCell[];

  /// Row-major scrollback. `scrollback.length === scrollbackRows * cols`.
  /// Not rendered in v1 (Phase 2.3 only paints viewport rows); the
  /// buffer exists so a future scrollback UI can read it without a
  /// re-attach.
  scrollback: PackedCell[];
  scrollbackRows: number;

  /// Sparse grapheme extras keyed by cell index over the concatenated
  /// `[scrollback..., cells...]` stream. The renderer's per-row reader
  /// rebases indices into the row's own slice.
  graphemeExtras: Map<number, string>;

  /// OSC 8 hyperlinks. `cellLinks` maps cell-index → link-id;
  /// `linkMap` maps link-id → URI. Both keyed in the same global
  /// index space as `graphemeExtras`.
  cellLinks: Map<number, number>;
  linkMap: Map<number, string>;

  /// Latest meta from the server. Updated on every CellDelta and
  /// FullPaneSync. The renderer uses cursor* and modeFlags for
  /// cursor + status displays in later phases.
  meta: PaneFrameMeta;

  title: string;
  cwd: string | null;

  /// Viewport-relative dirty row indices accumulated since the last
  /// `takeDirtyRows()` call. Kept as a Set so concurrent deltas to
  /// the same row don't cause repeated re-renders.
  private dirtyRows: Set<number> = new Set();
  /// Set by `applyFullPaneSync` (or `resize`); means "rebuild every
  /// row from scratch, also drop any stale row containers."
  private pendingFullRedraw = false;

  constructor(paneId: bigint, cols: number, rows: number) {
    if (cols < 0 || rows < 0 || !Number.isInteger(cols) || !Number.isInteger(rows)) {
      throw new GridShapeError(`invalid grid shape: cols=${cols}, rows=${rows}`);
    }
    this.paneId = paneId;
    this.cols = cols;
    this.rows = rows;
    this.cells = new Array(cols * rows).fill(DEFAULT_CELL);
    this.scrollback = [];
    this.scrollbackRows = 0;
    this.graphemeExtras = new Map();
    this.cellLinks = new Map();
    this.linkMap = new Map();
    this.meta = {
      paneId,
      generation: 0n,
      cursorLine: 0,
      cursorCol: 0,
      cursorShape: 0,
      modeFlags: 0,
      echoAck: 0n,
    };
    this.title = "";
    this.cwd = null;
    this.pendingFullRedraw = true;
  }

  /// Apply a CellDelta: splice each region's cells into the matching
  /// row range, mark each touched row dirty. Out-of-range regions
  /// throw — the wire decoder already rejects them before they reach
  /// here, but the grid double-checks because callers may bypass the
  /// decoder (tests, replay).
  applyCellDelta(delta: CellDelta): void {
    if (delta.meta.paneId !== this.paneId) {
      throw new GridShapeError(
        `CellDelta.paneId=${delta.meta.paneId} does not match grid.paneId=${this.paneId}`,
      );
    }
    if (delta.cols !== this.cols) {
      // The Rust server always sends a FullPaneSync before a delta
      // that uses a different cols/rows. Receiving a mismatched delta
      // here means the client got out of sync — drop it and let the
      // app layer drive a re-attach.
      throw new GridShapeError(
        `CellDelta.cols=${delta.cols} does not match grid.cols=${this.cols}`,
      );
    }
    for (const region of delta.regions) {
      if (region.line >= this.rows) {
        throw new GridShapeError(
          `region.line=${region.line} >= rows=${this.rows}`,
        );
      }
      if (region.right >= this.cols) {
        throw new GridShapeError(
          `region.right=${region.right} >= cols=${this.cols}`,
        );
      }
      const expected = region.right - region.left + 1;
      if (region.cells.length !== expected) {
        throw new GridShapeError(
          `region cells length=${region.cells.length}, expected ${expected} from bounds`,
        );
      }
      const rowStart = region.line * this.cols;
      // Evict stale extras for the cells we're about to overwrite.
      // Without this, a new character that lands on a cell that
      // previously had a combining grapheme or an OSC 8 hyperlink
      // would silently inherit them (renderer looks up
      // `viewportGlobalIndex(line, col)` in `graphemeExtras` and
      // `cellLinks`, and those maps still hold the old bindings).
      // Mirrors `apply_delta_borrowed` in `crates/ciri-app/src/grid/sync.rs:265-268`.
      const sbLen = this.scrollback.length;
      const globalStart = sbLen + rowStart + region.left;
      const globalEnd = globalStart + region.cells.length;
      for (let g = globalStart; g < globalEnd; g += 1) {
        this.graphemeExtras.delete(g);
        this.cellLinks.delete(g);
      }
      for (let i = 0; i < region.cells.length; i += 1) {
        this.cells[rowStart + region.left + i] = region.cells[i]!;
      }
      this.dirtyRows.add(region.line);
    }
    this.meta = delta.meta;
  }

  /// Apply a FullPaneSync. Three cases:
  ///
  /// 1. `sync.rows > 0` and dimensions changed → full resize + replace.
  /// 2. `sync.rows > 0` and dimensions match  → replace viewport in place.
  /// 3. `sync.rows === 0`                     → scrollback-only sync:
  ///    the server is appending (or replacing) scrollback rows without
  ///    touching the live viewport. The viewport `cells`, `cols`, and
  ///    `rows` are preserved; only scrollback, extras, and meta update.
  ///
  /// In case 3, mutating `this.cols`/`this.rows`/`this.cells` would
  /// blank the screen on every normal scrolling tick — that was the
  /// bug round-1 codex review flagged. Mirrors the Rust client's
  /// `apply_full_sync` (`crates/ciri-app/src/grid/sync.rs`).
  ///
  /// Grapheme + hyperlink extras are rebased into the post-application
  /// global cell-index space. The server keys both maps relative to
  /// `[sync.scrollback, sync.cells]`; after we append/replace the
  /// scrollback, those keys need shifting so they still point at the
  /// right cells.
  applyFullPaneSync(sync: FullPaneSync): void {
    if (sync.meta.paneId !== this.paneId) {
      throw new GridShapeError(
        `FullPaneSync.paneId=${sync.meta.paneId} does not match grid.paneId=${this.paneId}`,
      );
    }
    const expectedCells = sync.cols * sync.rows;
    if (sync.cells.length !== expectedCells) {
      throw new GridShapeError(
        `FullPaneSync.cells.length=${sync.cells.length}, expected ${expectedCells}`,
      );
    }
    const expectedSb = sync.scrollbackRows * sync.cols;
    if (sync.scrollback.length !== expectedSb) {
      throw new GridShapeError(
        `FullPaneSync.scrollback.length=${sync.scrollback.length}, expected ${expectedSb}`,
      );
    }

    // Snapshot pre-sync state used by the extras rebase math below.
    const oldSbCells = this.scrollback.length;
    const colsChanged = sync.cols !== this.cols;
    const isScrollbackOnly = sync.rows === 0;

    if (!isScrollbackOnly) {
      // `slice()` rather than borrowing the input — the decoder hands us
      // a freshly-allocated array, but a caller building syncs by hand
      // might pass a shared reference. Copying keeps the grid's
      // internal state isolated from the decoder's transient buffers.
      this.cols = sync.cols;
      this.rows = sync.rows;
      this.cells = sync.cells.slice();
    }
    // else: keep this.cols, this.rows, this.cells from before the sync.

    // Scrollback handling. Append (default) vs replace (full-buffer
    // overwrite) is decided here, not by the caller.
    if (sync.scrollbackReplace) {
      this.scrollback = sync.scrollback.slice();
      this.scrollbackRows = sync.scrollbackRows;
    } else if (sync.scrollback.length > 0) {
      // The server's `scrollback_replace = false` means "the
      // following rows are NEW history to append on top of what the
      // client already has." Pre-allocate and copy in place. Skip
      // the alloc when there's no incoming scrollback (the most
      // common case — viewport-replacing syncs typically carry no
      // history).
      const prev = this.scrollback;
      this.scrollback = new Array(prev.length + sync.scrollback.length);
      for (let i = 0; i < prev.length; i += 1) this.scrollback[i] = prev[i]!;
      const off = prev.length;
      for (let i = 0; i < sync.scrollback.length; i += 1) {
        this.scrollback[off + i] = sync.scrollback[i]!;
      }
      this.scrollbackRows = this.scrollbackRows + sync.scrollbackRows;
    }

    this.graphemeExtras = rebaseExtras(
      this.graphemeExtras,
      sync.graphemeExtras,
      oldSbCells,
      sync.scrollback.length,
      sync.scrollbackReplace,
      colsChanged,
      isScrollbackOnly,
      // Preserve old viewport entries on scrollback-only syncs —
      // grapheme overflows refer to the cell glyph (unchanged), not
      // to a sync-allocated handle.
      /* preserveOldViewportOnScrollbackOnly */ true,
    );
    // `cellLinks` reference into `linkMap`, which the server allocates
    // from 1 for each FullPaneSync — IDs are NOT globally stable.
    // If we preserved old cellLinks across syncs, the next sync's
    // linkMap might re-use the same ID for a different URI, making
    // stale cells point at the wrong target (or nowhere, if the new
    // sync omits the ID). Drop wholesale on every sync; the renderer
    // re-derives hyperlinks from the new sync's data. Mirrors the
    // Rust client's `apply_full_sync` (`hyperlink_cell_map.clear()`).
    this.cellLinks = rebaseExtras(
      this.cellLinks,
      sync.cellLinks,
      oldSbCells,
      sync.scrollback.length,
      sync.scrollbackReplace,
      colsChanged,
      isScrollbackOnly,
      /* preserveOldViewportOnScrollbackOnly */ false,
    );
    // linkMap is keyed by link-id (a global handle), not by cell
    // index, so no rebase is needed. Replace wholesale.
    this.linkMap = new Map(sync.linkMap);
    this.meta = sync.meta;
    this.title = sync.title;
    this.cwd = sync.cwd;

    // Even a scrollback-only sync may shift viewport extras forward
    // (because Rust drops old viewport extras on every sync — see
    // `rebase_grapheme_lookup`); to stay safe the renderer must
    // repaint all viewport rows after every FullPaneSync.
    this.dirtyRows.clear();
    this.pendingFullRedraw = true;
  }

  /// Pull the accumulated dirty-row set and clear it for the next
  /// frame. Always returns at least an empty array. After a
  /// FullPaneSync `fullRedraw=true` AND `rows` contains every viewport
  /// row (so a renderer that ignores the flag still repaints
  /// correctly).
  takeDirtyRows(): DirtyRows {
    if (this.pendingFullRedraw) {
      const rows = new Array(this.rows);
      for (let i = 0; i < this.rows; i += 1) rows[i] = i;
      this.dirtyRows.clear();
      this.pendingFullRedraw = false;
      return { rows, fullRedraw: true };
    }
    const rows = Array.from(this.dirtyRows).sort((a, b) => a - b);
    this.dirtyRows.clear();
    return { rows, fullRedraw: false };
  }

  /// Read one row's worth of cells without copying — for the renderer's
  /// per-row scan. The caller must NOT mutate; the returned slice
  /// shares the underlying `cells` array.
  rowCells(row: number): PackedCell[] {
    if (row < 0 || row >= this.rows) {
      throw new GridShapeError(`row=${row} out of range 0..${this.rows}`);
    }
    return this.cells.slice(row * this.cols, (row + 1) * this.cols);
  }

  /// Look up the full grapheme string for a cell. Falls back to
  /// `cell.ch` when no extra is present. The lookup is over the
  /// [scrollback..., viewport] index space so callers can hand in any
  /// global cell index.
  globalGrapheme(globalCellIdx: number, fallbackCh: string): string {
    const extra = this.graphemeExtras.get(globalCellIdx);
    if (extra === undefined) return fallbackCh;
    return fallbackCh + extra;
  }

  /// Translate a viewport row+col to the global cell index that
  /// `graphemeExtras` / `cellLinks` use as their key.
  viewportGlobalIndex(row: number, col: number): number {
    return this.scrollback.length + row * this.cols + col;
  }
}

/// Produce the post-sync map by rebasing sync entries into the
/// concatenated `[scrollback..., viewport]` global index space.
///
/// Sync map keys live in `[sync.scrollback, sync.cells]`:
///   - sync sb entries (key < syncSbCells) → new key = newScrollbackBase + key
///   - sync vp entries (key >= syncSbCells) → new key = newScrollbackTotal + (key - syncSbCells)
///
/// Where:
///   - `newScrollbackBase`  = where sync's *first* scrollback cell lands in the
///                            post-sync absolute index space.
///   - `newScrollbackTotal` = where the viewport starts in the post-sync
///                            absolute index space (i.e., total sb cells).
///
/// Old-entry handling depends on whether the viewport was replaced:
///   - **Normal sync** (rows > 0): the new viewport replaces the old, so
///     old viewport entries are stale; preserve only the old scrollback
///     portion (and only when neither `scrollbackReplace` nor
///     `colsChanged` invalidates the index space).
///   - **Scrollback-only sync** (rows === 0): the viewport cells are
///     intentionally preserved, so old viewport entries must follow them
///     — shifted forward by the appended scrollback length, or rebased
///     onto the new scrollback base when `scrollbackReplace` is true.
function rebaseExtras<V>(
  oldMap: Map<number, V>,
  syncMap: Map<number, V>,
  oldSbCells: number,
  syncSbCells: number,
  scrollbackReplace: boolean,
  colsChanged: boolean,
  isScrollbackOnly: boolean,
  preserveOldViewportOnScrollbackOnly: boolean,
): Map<number, V> {
  const out = new Map<number, V>();

  if (isScrollbackOnly && preserveOldViewportOnScrollbackOnly) {
    // Viewport cells were NOT replaced by this sync — their extras
    // must survive, just shifted to their new absolute position
    // after the scrollback delta.
    if (!colsChanged) {
      if (scrollbackReplace) {
        // Old scrollback is wiped; viewport now starts at syncSbCells.
        // Old scrollback-indexed entries (keys < oldSbCells) are dropped;
        // old viewport-indexed entries (keys >= oldSbCells) rebase to
        // `syncSbCells + (key - oldSbCells)`.
        for (const [k, v] of oldMap) {
          if (k >= oldSbCells) {
            out.set(syncSbCells + (k - oldSbCells), v);
          }
        }
      } else {
        // Append: old scrollback stays at its current keys; the new
        // scrollback cells push the viewport forward by `syncSbCells`.
        for (const [k, v] of oldMap) {
          if (k < oldSbCells) {
            out.set(k, v);
          } else {
            out.set(k + syncSbCells, v);
          }
        }
      }
    }
    // colsChanged + isScrollbackOnly shouldn't happen (a scrollback-
    // only sync doesn't carry a new geometry); fall through to drop
    // everything if it does.
  } else if (isScrollbackOnly && !preserveOldViewportOnScrollbackOnly) {
    // Scrollback-only sync, but the caller asked us NOT to preserve
    // old viewport entries (e.g. cellLinks, where link IDs can collide
    // across syncs). Old scrollback entries still survive at their
    // original keys when scrollback wasn't wiped or reflowed; the rest
    // is dropped.
    if (!scrollbackReplace && !colsChanged) {
      for (const [k, v] of oldMap) {
        if (k < oldSbCells) out.set(k, v);
      }
    }
  } else if (!scrollbackReplace && !colsChanged) {
    // Normal viewport-replacing sync, no scrollback wipe, no reflow.
    // Old scrollback entries keep their absolute indices; old viewport
    // entries are dropped because sync.cells replaces them.
    for (const [k, v] of oldMap) {
      if (k < oldSbCells) out.set(k, v);
    }
  }
  // else: scrollbackReplace or colsChanged with a real viewport sync —
  // drop everything; sync's entries below are the new world.

  // Where sync's first sb cell lands in absolute index space.
  const newScrollbackBase = scrollbackReplace ? 0 : oldSbCells;
  const newScrollbackTotal = newScrollbackBase + syncSbCells;

  for (const [k, v] of syncMap) {
    if (k < syncSbCells) {
      out.set(newScrollbackBase + k, v);
    } else {
      out.set(newScrollbackTotal + (k - syncSbCells), v);
    }
  }
  return out;
}
