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
//     `@loom/codec` do it.
//   * reconcile a stale `generation` — the app layer is expected to
//     pair generations with its own bookkeeping. The grid applies
//     whatever it's handed.

import {
  DEFAULT_CELL,
  FLAG_WIDE_CHAR,
  FLAG_WIDE_CHAR_SPACER,
  type CellDelta,
  type FullPaneSync,
  type PackedCell,
  type PaneFrameMeta,
} from "@loom/codec";

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

/// One end of a selection range in the concatenated
/// `[scrollback..., viewport]` cell stream. `col` is the column
/// (0..cols), `srcRow` the row in the combined buffer (0..totalRows).
export interface SelectionAnchor {
  col: number;
  srcRow: number;
}

/// A selection in a pane's combined buffer space. `start` and `end`
/// may arrive in either order; consumers should normalize before
/// reasoning about which end is the anchor vs the cursor. The
/// `active` flag is `true` while the user is actively dragging.
export interface SelectionRange {
  start: SelectionAnchor;
  end: SelectionAnchor;
  active: boolean;
}

/// One search hit from `PaneGrid.search`. `srcRow` is in the combined
/// `[scrollback..., viewport]` space (same as `SelectionAnchor.srcRow`);
/// `startCol`/`endCol` are inclusive columns within that row.
export interface SearchMatch {
  srcRow: number;
  startCol: number;
  endCol: number;
}

/// True when `a` and `b` describe the same span (regardless of which
/// end is anchor vs cursor). Used by the renderer to skip a needless
/// overlay rebuild when a mousemove produced no movement.
export function selectionRangeEquals(
  a: SelectionRange | null,
  b: SelectionRange | null,
): boolean {
  if (a === null && b === null) return true;
  if (a === null || b === null) return false;
  if (a.active !== b.active) return false;
  return (
    (anchorsEqual(a.start, b.start) && anchorsEqual(a.end, b.end)) ||
    (anchorsEqual(a.start, b.end) && anchorsEqual(a.end, b.start))
  );
}

function anchorsEqual(a: SelectionAnchor, b: SelectionAnchor): boolean {
  return a.col === b.col && a.srcRow === b.srcRow;
}

/// Default cap on scrollback rows kept in the browser-side buffer.
/// Long-running sessions can produce millions of rows of history; the
/// server keeps a bounded buffer (typically 10k rows) but a fresh
/// client could see incremental append syncs forever without ever
/// receiving a `scrollback_replace = true` reset. Without a cap on
/// the client side the JS heap would grow without bound. 10k rows
/// at 80 cols × 16 bytes ≈ 12 MB — comfortable headroom for normal
/// terminal use.
export const DEFAULT_MAX_SCROLLBACK_ROWS = 10_000;

export interface PaneGridOptions {
  /// Maximum number of scrollback rows to retain. Older rows are
  /// evicted from the front (oldest history first) when an append
  /// would push past this limit. Defaults to
  /// [`DEFAULT_MAX_SCROLLBACK_ROWS`].
  maxScrollbackRows?: number;
}

export class PaneGrid {
  readonly paneId: bigint;

  cols: number;
  rows: number;
  /// Hard cap on retained scrollback rows. See `PaneGridOptions`.
  readonly maxScrollbackRows: number;

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
  /// Cumulative count of rows evicted from the FRONT of scrollback by
  /// the `maxScrollbackRows` cap. Every front-trim shifts the absolute
  /// `srcRow` of all surviving rows down by the trim amount; consumers
  /// holding absolute `srcRow` anchors (e.g. the renderer's inline
  /// images) diff this counter across renders to rebase them. Append
  /// growth does NOT change it — appended rows keep existing srcRows
  /// stable. Monotonic.
  scrollbackTrimmed = 0;
  /// Bumped whenever scrollback is replaced wholesale
  /// (`scrollbackReplace`), which redefines the absolute index space
  /// entirely (alt-screen enter/exit, resize, …). Anchors from a prior
  /// epoch are meaningless and must be dropped, not rebased.
  scrollbackEpoch = 0;

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

  constructor(
    paneId: bigint,
    cols: number,
    rows: number,
    opts: PaneGridOptions = {},
  ) {
    if (cols < 0 || rows < 0 || !Number.isInteger(cols) || !Number.isInteger(rows)) {
      throw new GridShapeError(`invalid grid shape: cols=${cols}, rows=${rows}`);
    }
    const max = opts.maxScrollbackRows ?? DEFAULT_MAX_SCROLLBACK_ROWS;
    if (max < 0 || !Number.isInteger(max)) {
      throw new GridShapeError(`invalid maxScrollbackRows: ${max}`);
    }
    this.paneId = paneId;
    this.cols = cols;
    this.rows = rows;
    this.maxScrollbackRows = max;
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
      receivedAck: 0n,
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
      // Mirrors `apply_delta_borrowed` in `crates/loom-app/src/grid/sync.rs:265-268`.
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
  /// `apply_full_sync` (`crates/loom-app/src/grid/sync.rs`).
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
      this.scrollbackEpoch += 1;
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

    // Cap scrollback growth. Incremental append syncs (the common case
    // during long-running output) never carry `scrollbackReplace`, so
    // without an explicit trim the JS heap would grow without bound.
    // Drop the oldest rows from the front and rebase extras into the
    // new absolute index space.
    if (this.scrollbackRows > this.maxScrollbackRows) {
      const trimRows = this.scrollbackRows - this.maxScrollbackRows;
      const trimCells = trimRows * this.cols;
      this.scrollback = this.scrollback.slice(trimCells);
      this.scrollbackRows = this.maxScrollbackRows;
      this.graphemeExtras = shiftExtrasDown(this.graphemeExtras, trimCells);
      this.cellLinks = shiftExtrasDown(this.cellLinks, trimCells);
      this.scrollbackTrimmed += trimRows;
    }

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

  /// Total addressable rows when treating scrollback and viewport as a
  /// single buffer. Used by the renderer to map a scroll position into
  /// the concatenated `[scrollback..., cells]` index space.
  totalRows(): number {
    return this.scrollbackRows + this.rows;
  }

  /// Read one row's worth of cells from the concatenated
  /// `[scrollback..., cells]` buffer. `srcRow` in
  /// `0..scrollbackRows+rows`. Scrollback rows occupy the lower half;
  /// viewport rows occupy the upper. Same no-mutate rule as `rowCells`.
  combinedRowCells(srcRow: number): PackedCell[] {
    const total = this.totalRows();
    if (srcRow < 0 || srcRow >= total) {
      throw new GridShapeError(
        `srcRow=${srcRow} out of range 0..${total}`,
      );
    }
    if (srcRow < this.scrollbackRows) {
      return this.scrollback.slice(srcRow * this.cols, (srcRow + 1) * this.cols);
    }
    const vpRow = srcRow - this.scrollbackRows;
    return this.cells.slice(vpRow * this.cols, (vpRow + 1) * this.cols);
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

  /// Extract plain text for a selection range in the combined buffer.
  /// Mirrors the Rust client's `text_in_range`
  /// (`crates/loom-app/src/grid/cell_ops.rs:110`):
  /// - normalize start/end so start is the earlier of the pair,
  /// - per row, take `[startCol..=endCol]` (first row from start, last
  ///   row up to end, intermediate rows the full width),
  /// - skip cells whose flags have `FLAG_WIDE_CHAR_SPACER` (the
  ///   companion to a preceding wide cell — its glyph is already in
  ///   the wide cell's `ch`),
  /// - apply grapheme extras when present,
  /// - trim each row's trailing whitespace,
  /// - join rows with `\n`.
  extractText(start: SelectionAnchor, end: SelectionAnchor): string {
    const [a, b] = normalizeSelectionEnds(start, end);
    if (this.cols === 0) return "";
    const total = this.totalRows();
    const out: string[] = [];
    for (let row = a.srcRow; row <= b.srcRow; row += 1) {
      if (row < 0 || row >= total) continue;
      const cells = this.combinedRowCells(row);
      const left = row === a.srcRow ? clamp(a.col, 0, this.cols - 1) : 0;
      const right = row === b.srcRow ? clamp(b.col, 0, this.cols - 1) : this.cols - 1;
      const rowGlobalStart = row * this.cols;
      let line = "";
      for (let col = left; col <= right; col += 1) {
        const cell = cells[col];
        if (cell === undefined) break;
        if ((cell.flags & FLAG_WIDE_CHAR_SPACER) !== 0) continue;
        const ch = cell.ch;
        if (ch === " ") continue;
        const extra = this.graphemeExtras.get(rowGlobalStart + col);
        line += extra === undefined ? ch : ch + extra;
      }
      // Trim trailing whitespace — Rust does the same so a copy of a
      // line with only the first few cells filled doesn't pull in
      // hundreds of pad spaces.
      out.push(line.replace(/[ \t]+$/u, ""));
    }
    return out.join("\n");
  }

  /// Find every occurrence of `query` in the combined buffer
  /// (scrollback + viewport). Case-insensitive. Matches are returned in
  /// reading order (top row first, left-to-right within a row) and
  /// never span a row boundary — terminal lines are independent on the
  /// wire even when visually wrapped. Wide-char spacer cells are
  /// skipped when building each row's text so a column maps back to the
  /// glyph's real cell. Mirrors the desktop client's `grid.search`
  /// (`update_search_results` in `crates/loom/src/app/action.rs`).
  search(query: string): SearchMatch[] {
    if (query.length === 0 || this.cols === 0) return [];
    const needle = query.toLowerCase();
    const out: SearchMatch[] = [];
    const total = this.totalRows();
    for (let srcRow = 0; srcRow < total; srcRow += 1) {
      const cells = this.combinedRowCells(srcRow);
      // Build the row's lowercased text plus a per-code-unit → column
      // map. A cell whose `ch` (or its lowercase) is more than one code
      // unit pushes its column once per unit, so `indexOf` offsets map
      // back to columns even across case folding / multi-unit glyphs.
      let text = "";
      const colOf: number[] = [];
      const rowGlobalStart = srcRow * this.cols;
      for (let col = 0; col < this.cols; col += 1) {
        const cell = cells[col];
        if (cell === undefined) break;
        if ((cell.flags & FLAG_WIDE_CHAR_SPACER) !== 0) continue;
        // Use the full grapheme (primary codepoint + combining-mark /
        // ZWJ extras) so a cell that renders as `é` or a multi-codepoint
        // emoji is searchable, matching what `extractText` produces.
        const base = cell.ch.length === 0 ? " " : cell.ch;
        const lower = this.globalGrapheme(rowGlobalStart + col, base).toLowerCase();
        for (let k = 0; k < lower.length; k += 1) colOf.push(col);
        text += lower;
      }
      let from = 0;
      for (;;) {
        const idx = text.indexOf(needle, from);
        if (idx < 0) break;
        const startCol = colOf[idx];
        let endCol = colOf[idx + needle.length - 1];
        if (startCol !== undefined && endCol !== undefined) {
          // Extend the end over a double-wide glyph's trailing spacer so
          // the highlight covers the full 2-column cell, not just its
          // lead column.
          const endCell = cells[endCol];
          if (
            endCell !== undefined &&
            (endCell.flags & FLAG_WIDE_CHAR) !== 0 &&
            endCol + 1 < this.cols
          ) {
            endCol += 1;
          }
          out.push({ srcRow, startCol, endCol });
        }
        from = idx + 1; // allow overlapping matches
      }
    }
    return out;
  }
}

/// Normalize a (start, end) pair so the returned `[first, last]`
/// always has `first` no later than `last` in reading order
/// (top-to-bottom, then left-to-right). Used by `extractText` and
/// the selection-overlay renderer.
export function normalizeSelectionEnds(
  start: SelectionAnchor,
  end: SelectionAnchor,
): [SelectionAnchor, SelectionAnchor] {
  if (
    start.srcRow < end.srcRow ||
    (start.srcRow === end.srcRow && start.col <= end.col)
  ) {
    return [start, end];
  }
  return [end, start];
}

function clamp(n: number, lo: number, hi: number): number {
  if (hi < lo) return lo;
  return n < lo ? lo : n > hi ? hi : n;
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
/// Drop extras whose key is below `dropFloor` (the cells got trimmed
/// from the front of the buffer) and shift the rest down by
/// `dropFloor` to land in the new absolute index space.
function shiftExtrasDown<V>(m: Map<number, V>, dropFloor: number): Map<number, V> {
  if (dropFloor === 0) return m;
  const out = new Map<number, V>();
  for (const [k, v] of m) {
    if (k >= dropFloor) out.set(k - dropFloor, v);
  }
  return out;
}

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
    // old viewport entries. Used only for `cellLinks`: hyperlink IDs
    // are server-allocated per-sync from 1, and the matching
    // `linkMap` is wholesale replaced on every sync (see the call
    // site in `applyFullPaneSync`). So even an old *scrollback*
    // cellLink, whose cell content is unchanged, points to an ID
    // that may now resolve to a completely different URI in the new
    // linkMap (or to nothing) — drop EVERY old entry. Round-5 codex
    // fix: previously this branch kept `k < oldSbCells` entries,
    // which silently re-pointed historical links at new sync IDs.
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
