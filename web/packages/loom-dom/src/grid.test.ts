// PaneGrid state model tests. Drive the grid with hand-built
// CellDelta / FullPaneSync values + the curated Rust-emitted fixtures,
// verify cell state, dirty-row tracking, and shape guards.

import { describe, expect, test } from "vitest";
import {
  DEFAULT_BG,
  DEFAULT_FG,
  type CellDelta,
  type FullPaneSync,
  type PackedCell,
} from "@loom/codec";
import {
  GridShapeError,
  PaneGrid,
  normalizeSelectionEnds,
  selectionRangeEquals,
} from "./grid.js";
import { FLAG_WIDE_CHAR, FLAG_WIDE_CHAR_SPACER } from "@loom/codec";

function cell(ch: string, fg = DEFAULT_FG, bg = DEFAULT_BG, flags = 0): PackedCell {
  return { ch, fg, bg, flags };
}

function makeSync(overrides: Partial<FullPaneSync> = {}): FullPaneSync {
  const base: FullPaneSync = {
    meta: {
      paneId: 1n,
      generation: 0n,
      cursorLine: 0,
      cursorCol: 0,
      cursorShape: 0,
      modeFlags: 0,
      receivedAck: 0n,
      echoAck: 0n,
    },
    cols: 3,
    rows: 2,
    title: "",
    scrollback: [],
    scrollbackRows: 0,
    scrollbackReplace: false,
    cells: [
      cell("H"),
      cell("i"),
      cell(" "),
      cell(" "),
      cell(" "),
      cell(" "),
    ],
    graphemeExtras: new Map(),
    cellLinks: new Map(),
    linkMap: new Map(),
    cwd: null,
  };
  return { ...base, ...overrides };
}

function makeDelta(overrides: Partial<CellDelta> = {}): CellDelta {
  const base: CellDelta = {
    meta: {
      paneId: 1n,
      generation: 1n,
      cursorLine: 0,
      cursorCol: 2,
      cursorShape: 0,
      modeFlags: 0,
      receivedAck: 0n,
      echoAck: 0n,
    },
    cols: 3,
    regions: [],
  };
  return { ...base, ...overrides };
}

describe("PaneGrid constructor", () => {
  test("starts with default cells and a pending full redraw", () => {
    const g = new PaneGrid(1n, 4, 2);
    expect(g.cols).toBe(4);
    expect(g.rows).toBe(2);
    expect(g.cells).toHaveLength(8);
    expect(g.cells.every((c) => c.ch === " ")).toBe(true);
    const dirty = g.takeDirtyRows();
    expect(dirty.fullRedraw).toBe(true);
    expect(dirty.rows).toEqual([0, 1]);
  });

  test("rejects non-integer dimensions", () => {
    expect(() => new PaneGrid(1n, 0.5, 1)).toThrow(GridShapeError);
    expect(() => new PaneGrid(1n, 1, -1)).toThrow(GridShapeError);
  });
});

describe("PaneGrid.applyFullPaneSync", () => {
  test("replaces cells and marks full redraw", () => {
    const g = new PaneGrid(1n, 3, 2);
    // Drain the constructor's pending full redraw so the next take
    // reflects only the sync application.
    g.takeDirtyRows();
    g.applyFullPaneSync(makeSync());
    expect(g.cells[0]!.ch).toBe("H");
    expect(g.cells[1]!.ch).toBe("i");
    const dirty = g.takeDirtyRows();
    expect(dirty.fullRedraw).toBe(true);
    expect(dirty.rows).toEqual([0, 1]);
  });

  test("rejects pane-id mismatch", () => {
    const g = new PaneGrid(1n, 3, 2);
    expect(() => g.applyFullPaneSync(makeSync({ meta: { ...makeSync().meta, paneId: 99n } }))).toThrow(
      /paneId/,
    );
  });

  test("rejects cells length mismatch", () => {
    const g = new PaneGrid(1n, 3, 2);
    const sync = makeSync({ cells: [cell("X")] });
    expect(() => g.applyFullPaneSync(sync)).toThrow(/cells.length/);
  });

  test("scrollbackReplace overwrites scrollback", () => {
    const g = new PaneGrid(1n, 3, 1);
    g.applyFullPaneSync(
      makeSync({
        rows: 1,
        cells: [cell("a"), cell("b"), cell("c")],
        scrollback: [cell("o"), cell("l"), cell("d")],
        scrollbackRows: 1,
        scrollbackReplace: true,
      }),
    );
    expect(g.scrollback.map((c) => c.ch)).toEqual(["o", "l", "d"]);
    expect(g.scrollbackRows).toBe(1);
    g.applyFullPaneSync(
      makeSync({
        rows: 1,
        cells: [cell("a"), cell("b"), cell("c")],
        scrollback: [cell("n"), cell("e"), cell("w")],
        scrollbackRows: 1,
        scrollbackReplace: true,
      }),
    );
    expect(g.scrollback.map((c) => c.ch)).toEqual(["n", "e", "w"]);
    expect(g.scrollbackRows).toBe(1);
  });

  test("scrollbackReplace=false appends new history rows", () => {
    const g = new PaneGrid(1n, 3, 1);
    g.applyFullPaneSync(
      makeSync({
        rows: 1,
        cells: [cell("a"), cell("b"), cell("c")],
        scrollback: [cell("o"), cell("l"), cell("d")],
        scrollbackRows: 1,
        scrollbackReplace: true,
      }),
    );
    g.applyFullPaneSync(
      makeSync({
        rows: 1,
        cells: [cell("a"), cell("b"), cell("c")],
        scrollback: [cell("n"), cell("e"), cell("w")],
        scrollbackRows: 1,
        scrollbackReplace: false,
      }),
    );
    expect(g.scrollback.map((c) => c.ch)).toEqual(["o", "l", "d", "n", "e", "w"]);
    expect(g.scrollbackRows).toBe(2);
  });

  test("isolates internal cell array from caller's input slice", () => {
    const g = new PaneGrid(1n, 3, 1);
    const sync = makeSync({
      rows: 1,
      cells: [cell("a"), cell("b"), cell("c")],
    });
    g.applyFullPaneSync(sync);
    // Mutating the caller's cells AFTER apply must not poison the grid.
    sync.cells[0] = cell("Z");
    expect(g.cells[0]!.ch).toBe("a");
  });
});

describe("PaneGrid.applyCellDelta", () => {
  test("splices region cells and marks the row dirty", () => {
    const g = new PaneGrid(1n, 4, 2);
    g.applyFullPaneSync({
      meta: {
        paneId: 1n,
        generation: 1n,
        cursorLine: 0,
        cursorCol: 0,
        cursorShape: 0,
        modeFlags: 0,
        receivedAck: 0n,
        echoAck: 0n,
      },
      cols: 4,
      rows: 2,
      title: "",
      scrollback: [],
      scrollbackRows: 0,
      scrollbackReplace: false,
      cells: new Array(8).fill(cell(" ")),
      graphemeExtras: new Map(),
      cellLinks: new Map(),
      linkMap: new Map(),
      cwd: null,
    });
    g.takeDirtyRows(); // drain full-redraw

    g.applyCellDelta(
      makeDelta({
        cols: 4,
        regions: [
          { line: 1, left: 1, right: 2, cells: [cell("X"), cell("Y")] },
        ],
      }),
    );
    expect(g.cells[1 * 4 + 1]!.ch).toBe("X");
    expect(g.cells[1 * 4 + 2]!.ch).toBe("Y");
    expect(g.cells[1 * 4 + 0]!.ch).toBe(" ");
    const dirty = g.takeDirtyRows();
    expect(dirty.fullRedraw).toBe(false);
    expect(dirty.rows).toEqual([1]);
  });

  test("rejects delta cols mismatch (out-of-sync grid)", () => {
    const g = new PaneGrid(1n, 4, 2);
    expect(() =>
      g.applyCellDelta(makeDelta({ cols: 80, regions: [] })),
    ).toThrow(/cols/);
  });

  test("rejects region.line >= rows", () => {
    const g = new PaneGrid(1n, 4, 2);
    expect(() =>
      g.applyCellDelta(
        makeDelta({
          cols: 4,
          regions: [{ line: 5, left: 0, right: 0, cells: [cell("X")] }],
        }),
      ),
    ).toThrow(/line=5/);
  });

  test("rejects cells length not matching bounds", () => {
    const g = new PaneGrid(1n, 4, 2);
    expect(() =>
      g.applyCellDelta(
        makeDelta({
          cols: 4,
          regions: [{ line: 0, left: 0, right: 2, cells: [cell("X")] }],
        }),
      ),
    ).toThrow(/length=1/);
  });

  test("evicts stale grapheme/link entries for overwritten cells", () => {
    const g = new PaneGrid(1n, 4, 1);
    g.applyFullPaneSync(
      makeSync({
        cols: 4,
        rows: 1,
        cells: [cell("a"), cell("b"), cell("c"), cell("d")],
        scrollback: [],
        scrollbackRows: 0,
        scrollbackReplace: true,
        graphemeExtras: new Map([[1, "́"]]), // 'b' has combining acute
        cellLinks: new Map([[2, 1]]), // 'c' has a hyperlink
        linkMap: new Map([[1, "https://example.com"]]),
      }),
    );
    g.takeDirtyRows();

    // Overwrite cells 1..2 (b and c) — both their extras must clear so
    // the new glyphs don't inherit the old combining mark/link.
    g.applyCellDelta(
      makeDelta({
        cols: 4,
        regions: [{ line: 0, left: 1, right: 2, cells: [cell("X"), cell("Y")] }],
      }),
    );
    expect(g.graphemeExtras.has(1)).toBe(false);
    expect(g.cellLinks.has(2)).toBe(false);
    // Linkmap is a global table; the link-id entry is left alone.
    expect(g.linkMap.get(1)).toBe("https://example.com");
  });

  test("non-overwritten extras survive a CellDelta", () => {
    const g = new PaneGrid(1n, 4, 1);
    g.applyFullPaneSync(
      makeSync({
        cols: 4,
        rows: 1,
        cells: [cell("a"), cell("b"), cell("c"), cell("d")],
        scrollback: [],
        scrollbackRows: 0,
        scrollbackReplace: true,
        graphemeExtras: new Map([[1, "́"], [3, "̀"]]),
      }),
    );
    g.takeDirtyRows();
    g.applyCellDelta(
      makeDelta({
        cols: 4,
        regions: [{ line: 0, left: 1, right: 1, cells: [cell("X")] }],
      }),
    );
    expect(g.graphemeExtras.has(1)).toBe(false);
    expect(g.graphemeExtras.get(3)).toBe("̀");
  });

  test("multiple regions on the same row dedupe in dirty set", () => {
    const g = new PaneGrid(1n, 4, 2);
    g.takeDirtyRows(); // drain full-redraw
    g.applyCellDelta(
      makeDelta({
        cols: 4,
        regions: [
          { line: 0, left: 0, right: 0, cells: [cell("A")] },
          { line: 0, left: 3, right: 3, cells: [cell("B")] },
        ],
      }),
    );
    const dirty = g.takeDirtyRows();
    expect(dirty.rows).toEqual([0]);
  });
});

describe("PaneGrid.applyFullPaneSync — scrollback-only sync (rows=0)", () => {
  // Server convention: a `FullPaneSync` with `rows = 0` and `cells = []`
  // is a scrollback-only delivery — new history rows without touching
  // the live viewport. Round-1 codex review caught this: my earlier
  // code unconditionally wrote `this.rows = 0` and blanked the viewport.
  test("preserves viewport geometry and cells", () => {
    const g = new PaneGrid(1n, 3, 2);
    g.applyFullPaneSync(
      makeSync({
        rows: 2,
        cells: [
          cell("a"), cell("b"), cell("c"),
          cell("d"), cell("e"), cell("f"),
        ],
      }),
    );
    g.takeDirtyRows();

    g.applyFullPaneSync(
      makeSync({
        rows: 0,
        cells: [],
        scrollback: [cell("o"), cell("l"), cell("d")],
        scrollbackRows: 1,
        scrollbackReplace: false,
      }),
    );

    expect(g.rows).toBe(2);
    expect(g.cols).toBe(3);
    expect(g.cells).toHaveLength(6);
    expect(g.cells.map((c) => c.ch)).toEqual(["a", "b", "c", "d", "e", "f"]);
    expect(g.scrollbackRows).toBe(1);
    expect(g.scrollback.map((c) => c.ch)).toEqual(["o", "l", "d"]);
  });

  test("doesn't reject a follow-up CellDelta after a scrollback-only sync", () => {
    const g = new PaneGrid(1n, 3, 2);
    g.applyFullPaneSync(makeSync());
    g.takeDirtyRows();
    g.applyFullPaneSync(
      makeSync({
        rows: 0,
        cells: [],
        scrollback: [cell("x"), cell("y"), cell("z")],
        scrollbackRows: 1,
        scrollbackReplace: false,
      }),
    );
    // Without the rows=0 guard the previous call would have set
    // rows=0; this delta would then throw `region.line=0 >= rows=0`.
    g.applyCellDelta(
      makeDelta({
        cols: 3,
        regions: [{ line: 0, left: 0, right: 0, cells: [cell("Q")] }],
      }),
    );
    expect(g.cells[0]!.ch).toBe("Q");
  });
});

describe("PaneGrid.applyFullPaneSync — extras rebase", () => {
  // Sync extras keys live in `[sync.scrollback, sync.cells]`; after we
  // splice that into the existing grid, the absolute cell index that
  // each entry SHOULD point to has shifted forward by the existing
  // scrollback length (for !scrollback_replace). Round-1 codex caught
  // this — without the rebase, viewport graphemes land on stale cells.

  test("appends sync graphemes into absolute indices after scrollback append", () => {
    const g = new PaneGrid(1n, 2, 1);
    // First sync builds initial state: 1 scrollback row (2 cells) +
    // 1 viewport row (2 cells). No extras.
    g.applyFullPaneSync(
      makeSync({
        cols: 2,
        rows: 1,
        cells: [cell("a"), cell("b")],
        scrollback: [cell("s"), cell("t")],
        scrollbackRows: 1,
        scrollbackReplace: true,
      }),
    );
    // Second sync: append 1 sb row + replace viewport. New sync
    // carries a grapheme on its FIRST sb cell (index 0 in sync stream).
    // After applying, that grapheme should be at absolute index 2
    // (the existing scrollback is 2 cells long).
    g.applyFullPaneSync(
      makeSync({
        cols: 2,
        rows: 1,
        cells: [cell("c"), cell("\u{1F600}")],
        scrollback: [cell("u"), cell("v")],
        scrollbackRows: 1,
        scrollbackReplace: false,
        // Sync's stream: [u, v, c, 😀].
        //  - key 0 → 'u' in sync sb → absolute (oldSb=2) + 0 = 2
        //  - key 3 → '😀' in sync cells → absolute (oldSb=2) + (3-syncSb=2) + syncSb=2
        //    = oldSb + key = 2 + 3 = 5
        // Cleaner: viewport entry at key 3 is at sync-vp-offset 1,
        // post-sync absolute = newSbTotal(4) + 1 = 5.
        graphemeExtras: new Map([
          [0, "́"],
          [3, "‍"],
        ]),
      }),
    );
    expect(g.graphemeExtras.get(2)).toBe("́");
    expect(g.graphemeExtras.get(5)).toBe("‍");
    // Original entries (none) shouldn't surface.
    expect(g.graphemeExtras.size).toBe(2);
  });

  test("scrollback-only sync rebases sync sb graphemes by old scrollback length", () => {
    const g = new PaneGrid(1n, 2, 1);
    g.applyFullPaneSync(
      makeSync({
        cols: 2,
        rows: 1,
        cells: [cell("a"), cell("b")],
        scrollback: [cell("s"), cell("t")],
        scrollbackRows: 1,
        scrollbackReplace: true,
      }),
    );
    g.applyFullPaneSync(
      makeSync({
        cols: 2,
        rows: 0,
        cells: [],
        scrollback: [cell("u"), cell("v")],
        scrollbackRows: 1,
        scrollbackReplace: false,
        graphemeExtras: new Map([[0, "́"]]),
      }),
    );
    // Sync's sb cell at index 0 is 'u'. After appending past old sb
    // (2 cells), absolute index = 2.
    expect(g.graphemeExtras.get(2)).toBe("́");
    expect(g.graphemeExtras.size).toBe(1);
  });

  test("scrollback_replace drops old extras and uses sync indices as absolute", () => {
    const g = new PaneGrid(1n, 2, 1);
    // Seed an old extras entry.
    g.applyFullPaneSync(
      makeSync({
        cols: 2,
        rows: 1,
        cells: [cell("a"), cell("b")],
        scrollback: [cell("s"), cell("t")],
        scrollbackRows: 1,
        scrollbackReplace: true,
        graphemeExtras: new Map([[0, "_old_"]]),
      }),
    );
    expect(g.graphemeExtras.get(0)).toBe("_old_");

    // New sync REPLACES scrollback. Old extras must be dropped — they
    // pointed at scrollback cells that no longer exist.
    g.applyFullPaneSync(
      makeSync({
        cols: 2,
        rows: 1,
        cells: [cell("c"), cell("d")],
        scrollback: [cell("u"), cell("v")],
        scrollbackRows: 1,
        scrollbackReplace: true,
        graphemeExtras: new Map([[2, "_new_"]]),
      }),
    );
    // Sync's stream: [u, v, c, d]. Key 2 = 'c' in viewport, which is
    // at absolute index 2 (sb=[u,v] is 2 cells).
    expect(g.graphemeExtras.size).toBe(1);
    expect(g.graphemeExtras.get(2)).toBe("_new_");
    expect(g.graphemeExtras.has(0)).toBe(false);
  });

  test("scrollback-only append preserves old viewport extras shifted forward", () => {
    const g = new PaneGrid(1n, 2, 1);
    // First sync seeds 2-cell scrollback + 2-cell viewport, with a
    // grapheme on the viewport's first cell and a link on its second.
    g.applyFullPaneSync(
      makeSync({
        cols: 2,
        rows: 1,
        cells: [cell("a"), cell("b")],
        scrollback: [cell("s"), cell("t")],
        scrollbackRows: 1,
        scrollbackReplace: true,
        graphemeExtras: new Map([[2, "́"]]), // 'a' at viewport index 0 → abs 2
        cellLinks: new Map([[3, 1]]), // 'b' at viewport index 1 → abs 3
        linkMap: new Map([[1, "https://example.com"]]),
      }),
    );
    expect(g.graphemeExtras.get(2)).toBe("́");
    expect(g.cellLinks.get(3)).toBe(1);

    // Scrollback-only sync: appends 1 sb row (2 cells), viewport stays.
    // Viewport graphemes at abs 2 must shift forward by syncSbCells=2
    // to abs 4. cellLinks, however, are DROPPED — see round-4 codex
    // finding: link IDs are not stable across syncs, so retaining old
    // cellLinks could resolve them through the new linkMap's
    // re-allocated IDs.
    g.applyFullPaneSync(
      makeSync({
        cols: 2,
        rows: 0,
        cells: [],
        scrollback: [cell("u"), cell("v")],
        scrollbackRows: 1,
        scrollbackReplace: false,
      }),
    );
    expect(g.graphemeExtras.get(4)).toBe("́");
    expect(g.graphemeExtras.has(2)).toBe(false);
    // cellLinks at any key — wholesale dropped.
    expect(g.cellLinks.size).toBe(0);
  });

  test("scrollback-only sync also drops cellLinks anchored in old scrollback (round-5)", () => {
    // Round-5 codex fix: previously cellLinks whose key was inside
    // the existing scrollback (k < oldSbCells) survived the rebase
    // branch even though linkMap is replaced wholesale. With
    // per-sync ID re-allocation, that link would silently re-resolve
    // to a different URI (or to nothing).
    const g = new PaneGrid(1n, 2, 1);
    g.applyFullPaneSync(
      makeSync({
        cols: 2,
        rows: 1,
        cells: [cell("a"), cell("b")],
        scrollback: [cell("s"), cell("t")],
        scrollbackRows: 1,
        scrollbackReplace: true,
        cellLinks: new Map([[0, 1]]), // 's' at scrollback index 0
        linkMap: new Map([[1, "https://old.example.com"]]),
      }),
    );
    expect(g.cellLinks.get(0)).toBe(1);
    // Scrollback-only sync (rows=0). Same sync re-uses link id 1 for
    // a DIFFERENT URL — without the round-5 fix, the old scrollback
    // link would survive and re-resolve to the new URL.
    g.applyFullPaneSync(
      makeSync({
        cols: 2,
        rows: 0,
        cells: [],
        scrollback: [cell("u"), cell("v")],
        scrollbackRows: 1,
        scrollbackReplace: false,
        cellLinks: new Map([[1, 1]]), // sync's own link on its sb cell at index 1
        linkMap: new Map([[1, "https://NEW.example.com"]]),
      }),
    );
    // Old scrollback link at key 0 must be gone.
    expect(g.cellLinks.has(0)).toBe(false);
    // Only the sync's new link survives, rebased onto the new
    // absolute index space.
    expect(g.cellLinks.size).toBe(1);
    expect(g.linkMap.get(1)).toBe("https://NEW.example.com");
  });

  test("scrollback-only replace preserves old viewport extras rebased onto new sb", () => {
    const g = new PaneGrid(1n, 2, 1);
    g.applyFullPaneSync(
      makeSync({
        cols: 2,
        rows: 1,
        cells: [cell("a"), cell("b")],
        scrollback: [cell("s"), cell("t"), cell("u"), cell("v")],
        scrollbackRows: 2,
        scrollbackReplace: true,
        graphemeExtras: new Map([[5, "́"]]), // 'b' at viewport[1] → abs 4+1=5
      }),
    );
    expect(g.graphemeExtras.get(5)).toBe("́");

    // rows=0 + scrollback_replace=true: new sb has 1 row (2 cells),
    // viewport cells preserved but now start at abs 2 (was abs 4).
    g.applyFullPaneSync(
      makeSync({
        cols: 2,
        rows: 0,
        cells: [],
        scrollback: [cell("x"), cell("y")],
        scrollbackRows: 1,
        scrollbackReplace: true,
      }),
    );
    // 'b' moved from abs 5 to abs 2 + (5-4) = 3.
    expect(g.graphemeExtras.get(3)).toBe("́");
    expect(g.graphemeExtras.has(5)).toBe(false);
  });

  test("rebases cellLinks the same way as graphemeExtras", () => {
    const g = new PaneGrid(1n, 2, 1);
    g.applyFullPaneSync(
      makeSync({
        cols: 2,
        rows: 1,
        cells: [cell("a"), cell("b")],
        scrollback: [cell("s"), cell("t")],
        scrollbackRows: 1,
        scrollbackReplace: true,
      }),
    );
    g.applyFullPaneSync(
      makeSync({
        cols: 2,
        rows: 1,
        cells: [cell("c"), cell("d")],
        scrollback: [cell("u"), cell("v")],
        scrollbackRows: 1,
        scrollbackReplace: false,
        cellLinks: new Map([[2, 1]]), // sync-key 2 = 'c' in viewport
        linkMap: new Map([[1, "https://example.com"]]),
      }),
    );
    // Sync stream: [u, v, c, d]. Key 2 ('c') → vp offset 0.
    // Absolute = newSbTotal(4) + 0 = 4.
    expect(g.cellLinks.get(4)).toBe(1);
    expect(g.linkMap.get(1)).toBe("https://example.com");
  });
});

describe("PaneGrid.rowCells + viewportGlobalIndex", () => {
  test("rowCells returns a slice that doesn't mutate the underlying grid", () => {
    const g = new PaneGrid(1n, 3, 2);
    const slice = g.rowCells(1);
    expect(slice).toHaveLength(3);
    slice[0] = cell("Z");
    expect(g.cells[3]!.ch).toBe(" ");
  });

  test("viewportGlobalIndex accounts for scrollback length", () => {
    const g = new PaneGrid(1n, 3, 2);
    g.applyFullPaneSync(
      makeSync({
        scrollback: [cell("x"), cell("y"), cell("z")],
        scrollbackRows: 1,
        scrollbackReplace: true,
      }),
    );
    expect(g.viewportGlobalIndex(0, 0)).toBe(3);
    expect(g.viewportGlobalIndex(1, 1)).toBe(3 + 3 + 1);
  });

  describe("scrollback cap (max-rows trim)", () => {
    /// Build a one-row scrollback whose every cell encodes the row
    /// number as its `ch` — lets a test verify which rows survived
    /// after a trim without juggling positions.
    function sbRow(idx: number, cols: number): PackedCell[] {
      const ch = String(idx % 10);
      const out: PackedCell[] = new Array(cols);
      for (let i = 0; i < cols; i += 1) out[i] = cell(ch);
      return out;
    }

    test("appending past maxScrollbackRows evicts oldest rows from the front", () => {
      const g = new PaneGrid(1n, 2, 1, { maxScrollbackRows: 3 });
      // Seed: 3 rows of history (0,1,2), at the cap.
      g.applyFullPaneSync(
        makeSync({
          cols: 2,
          rows: 1,
          scrollback: [...sbRow(0, 2), ...sbRow(1, 2), ...sbRow(2, 2)],
          scrollbackRows: 3,
          scrollbackReplace: true,
          cells: [cell("L"), cell("L")],
        }),
      );
      expect(g.scrollbackRows).toBe(3);
      // Append 2 more (3,4) — would push to 5 rows; cap at 3 means
      // rows 0,1 get trimmed, surviving order = 2, 3, 4.
      g.applyFullPaneSync(
        makeSync({
          cols: 2,
          rows: 1,
          scrollback: [...sbRow(3, 2), ...sbRow(4, 2)],
          scrollbackRows: 2,
          scrollbackReplace: false,
          cells: [cell("L"), cell("L")],
        }),
      );
      expect(g.scrollbackRows).toBe(3);
      // Read each scrollback row's first cell to verify identity.
      expect(g.combinedRowCells(0)[0]!.ch).toBe("2");
      expect(g.combinedRowCells(1)[0]!.ch).toBe("3");
      expect(g.combinedRowCells(2)[0]!.ch).toBe("4");
    });

    test("trim rebases grapheme + hyperlink extras into the new index space", () => {
      const g = new PaneGrid(1n, 2, 1, { maxScrollbackRows: 2 });
      // Seed: 2 rows (0, 1), graphemeExtra at row 1 (global index 2)
      // and a cellLink at row 0 (global index 0).
      g.applyFullPaneSync(
        makeSync({
          cols: 2,
          rows: 1,
          scrollback: [...sbRow(0, 2), ...sbRow(1, 2)],
          scrollbackRows: 2,
          scrollbackReplace: true,
          graphemeExtras: new Map([[2, "́"]]),
          cellLinks: new Map([[0, 1]]),
          linkMap: new Map([[1, "https://example.com"]]),
          cells: [cell("L"), cell("L")],
        }),
      );
      // Sanity: keys live at their absolute indices.
      expect(g.graphemeExtras.get(2)).toBe("́");
      expect(g.cellLinks.get(0)).toBe(1);
      // Append 1 more row (2) — cap of 2 trims row 0. Surviving
      // scrollback = row 1 (now at index 0) + row 2 (now at index 1).
      // graphemeExtras key 2 → key 0 (row 1, col 0). cellLinks key
      // 0 sat on row 0 → trimmed, dropped.
      g.applyFullPaneSync(
        makeSync({
          cols: 2,
          rows: 1,
          scrollback: [...sbRow(2, 2)],
          scrollbackRows: 1,
          scrollbackReplace: false,
          cells: [cell("L"), cell("L")],
        }),
      );
      expect(g.scrollbackRows).toBe(2);
      expect(g.graphemeExtras.get(0)).toBe("́");
      expect(g.graphemeExtras.size).toBe(1);
      expect(g.cellLinks.size).toBe(0);
    });

    test("explicit maxScrollbackRows=0 disables scrollback entirely", () => {
      const g = new PaneGrid(1n, 2, 1, { maxScrollbackRows: 0 });
      g.applyFullPaneSync(
        makeSync({
          cols: 2,
          rows: 1,
          scrollback: [...sbRow(0, 2), ...sbRow(1, 2)],
          scrollbackRows: 2,
          scrollbackReplace: true,
          cells: [cell("L"), cell("L")],
        }),
      );
      expect(g.scrollbackRows).toBe(0);
      expect(g.scrollback.length).toBe(0);
    });

    test("default maxScrollbackRows is large enough that normal use never trims", () => {
      const g = new PaneGrid(1n, 2, 1); // default cap
      const rows = 5_000;
      const cells: PackedCell[] = new Array(rows * 2);
      for (let r = 0; r < rows; r += 1) {
        cells[r * 2] = cell("a");
        cells[r * 2 + 1] = cell("b");
      }
      g.applyFullPaneSync(
        makeSync({
          cols: 2,
          rows: 1,
          scrollback: cells,
          scrollbackRows: rows,
          scrollbackReplace: true,
          cells: [cell("L"), cell("L")],
        }),
      );
      expect(g.scrollbackRows).toBe(rows);
    });

    test("invalid maxScrollbackRows throws", () => {
      expect(() => new PaneGrid(1n, 2, 1, { maxScrollbackRows: -1 })).toThrow(
        GridShapeError,
      );
      expect(() => new PaneGrid(1n, 2, 1, { maxScrollbackRows: 1.5 })).toThrow(
        GridShapeError,
      );
    });
  });

  describe("selection helpers", () => {
    test("normalizeSelectionEnds returns the same pair when already ordered", () => {
      const [a, b] = normalizeSelectionEnds(
        { col: 1, srcRow: 0 },
        { col: 3, srcRow: 2 },
      );
      expect(a).toEqual({ col: 1, srcRow: 0 });
      expect(b).toEqual({ col: 3, srcRow: 2 });
    });

    test("normalizeSelectionEnds swaps when end is earlier than start", () => {
      const [a, b] = normalizeSelectionEnds(
        { col: 4, srcRow: 5 },
        { col: 2, srcRow: 1 },
      );
      expect(a).toEqual({ col: 2, srcRow: 1 });
      expect(b).toEqual({ col: 4, srcRow: 5 });
    });

    test("normalizeSelectionEnds breaks ties on the same row by column", () => {
      const [a, b] = normalizeSelectionEnds(
        { col: 7, srcRow: 3 },
        { col: 2, srcRow: 3 },
      );
      expect(a).toEqual({ col: 2, srcRow: 3 });
      expect(b).toEqual({ col: 7, srcRow: 3 });
    });

    test("selectionRangeEquals ignores end-swap and respects active flag", () => {
      const r1 = {
        start: { col: 1, srcRow: 0 },
        end: { col: 3, srcRow: 2 },
        active: true,
      };
      const r2 = {
        start: { col: 3, srcRow: 2 },
        end: { col: 1, srcRow: 0 },
        active: true,
      };
      const r3 = { ...r1, active: false };
      expect(selectionRangeEquals(r1, r2)).toBe(true);
      expect(selectionRangeEquals(r1, r3)).toBe(false);
      expect(selectionRangeEquals(null, null)).toBe(true);
      expect(selectionRangeEquals(r1, null)).toBe(false);
    });
  });

  describe("PaneGrid.extractText", () => {
    function syncText(rows: string[]): FullPaneSync {
      // Build a viewport-only sync where each row is rows[i] padded
      // to `cols` with spaces, no scrollback, paneId=1.
      const cols = rows.reduce((m, r) => Math.max(m, r.length), 0);
      const cells: PackedCell[] = [];
      for (const r of rows) {
        for (let c = 0; c < cols; c += 1) {
          cells.push(cell(r[c] ?? " "));
        }
      }
      return makeSync({
        cols,
        rows: rows.length,
        cells,
        graphemeExtras: new Map(),
        cellLinks: new Map(),
        linkMap: new Map(),
      });
    }

    test("single-row selection takes [left..=right] and trims trailing space", () => {
      const g = new PaneGrid(1n, 5, 1);
      g.applyFullPaneSync(syncText(["hello"]));
      const text = g.extractText({ col: 1, srcRow: 0 }, { col: 3, srcRow: 0 });
      expect(text).toBe("ell");
    });

    test("multi-row selection: first row from start, last row to end, full intermediate rows", () => {
      const g = new PaneGrid(1n, 4, 3);
      g.applyFullPaneSync(syncText(["abcd", "efgh", "ijkl"]));
      const text = g.extractText(
        { col: 2, srcRow: 0 },
        { col: 1, srcRow: 2 },
      );
      expect(text).toBe("cd\nefgh\nij");
    });

    test("reversed start/end produces the same text as forward", () => {
      const g = new PaneGrid(1n, 4, 2);
      g.applyFullPaneSync(syncText(["abcd", "efgh"]));
      const fwd = g.extractText(
        { col: 0, srcRow: 0 },
        { col: 3, srcRow: 1 },
      );
      const rev = g.extractText(
        { col: 3, srcRow: 1 },
        { col: 0, srcRow: 0 },
      );
      expect(fwd).toBe(rev);
    });

    test("trailing spaces inside the selection are stripped per row", () => {
      const g = new PaneGrid(1n, 5, 2);
      g.applyFullPaneSync(syncText(["hi   ", "bye  "]));
      const text = g.extractText({ col: 0, srcRow: 0 }, { col: 4, srcRow: 1 });
      expect(text).toBe("hi\nbye");
    });

    test("FLAG_WIDE_CHAR_SPACER cells are skipped (no duplicate glyph)", () => {
      const g = new PaneGrid(1n, 4, 1);
      const sync = makeSync({
        cols: 4,
        rows: 1,
        cells: [
          cell("中", DEFAULT_FG, DEFAULT_BG, FLAG_WIDE_CHAR),
          cell(" ", DEFAULT_FG, DEFAULT_BG, FLAG_WIDE_CHAR_SPACER),
          cell("a"),
          cell("b"),
        ],
      });
      g.applyFullPaneSync(sync);
      const text = g.extractText({ col: 0, srcRow: 0 }, { col: 3, srcRow: 0 });
      expect(text).toBe("中ab");
    });

    test("grapheme extras are concatenated", () => {
      const g = new PaneGrid(1n, 3, 1);
      const sync = makeSync({
        cols: 3,
        rows: 1,
        cells: [cell("a"), cell("e"), cell("o")],
        graphemeExtras: new Map([[1, "́"]]), // e + acute accent
      });
      g.applyFullPaneSync(sync);
      const text = g.extractText({ col: 0, srcRow: 0 }, { col: 2, srcRow: 0 });
      expect(text).toBe("aéo");
    });

    test("selection spanning scrollback + viewport reads the combined buffer", () => {
      const g = new PaneGrid(1n, 3, 2);
      g.applyFullPaneSync(
        makeSync({
          cols: 3,
          rows: 2,
          scrollback: [
            cell("o"), cell("l"), cell("d"),
          ],
          scrollbackRows: 1,
          scrollbackReplace: true,
          cells: [
            cell("n"), cell("e"), cell("w"),
            cell("y"), cell("e"), cell("s"),
          ],
        }),
      );
      // scrollback row 0 (srcRow=0), viewport row 0 (srcRow=1).
      const text = g.extractText(
        { col: 0, srcRow: 0 },
        { col: 2, srcRow: 1 },
      );
      expect(text).toBe("old\nnew");
    });

    test("out-of-range rows are skipped, no throw", () => {
      const g = new PaneGrid(1n, 2, 1);
      g.applyFullPaneSync(syncText(["ab"]));
      // srcRow 99 is past totalRows; should just return what fits.
      const text = g.extractText({ col: 0, srcRow: 0 }, { col: 1, srcRow: 99 });
      expect(text).toBe("ab");
    });

    test("col values outside [0, cols) are clamped to the row's edges", () => {
      const g = new PaneGrid(1n, 3, 1);
      g.applyFullPaneSync(syncText(["xyz"]));
      const text = g.extractText({ col: -5, srcRow: 0 }, { col: 99, srcRow: 0 });
      expect(text).toBe("xyz");
    });
  });
});

describe("PaneGrid.search", () => {
  function syncRows(rows: string[], scrollback: string[] = []): FullPaneSync {
    const cols = [...rows, ...scrollback].reduce(
      (m, r) => Math.max(m, r.length),
      1,
    );
    const toCells = (rs: string[]): PackedCell[] => {
      const out: PackedCell[] = [];
      for (const r of rs) {
        for (let c = 0; c < cols; c += 1) out.push(cell(r[c] ?? " "));
      }
      return out;
    };
    return makeSync({
      cols,
      rows: rows.length,
      cells: toCells(rows),
      scrollback: toCells(scrollback),
      scrollbackRows: scrollback.length,
    });
  }

  test("finds a substring and maps it to inclusive columns", () => {
    const g = new PaneGrid(1n, 11, 1);
    g.applyFullPaneSync(syncRows(["hello world"]));
    expect(g.search("world")).toEqual([{ srcRow: 0, startCol: 6, endCol: 10 }]);
  });

  test("is case-insensitive", () => {
    const g = new PaneGrid(1n, 5, 1);
    g.applyFullPaneSync(syncRows(["Hello"]));
    expect(g.search("HELLO")).toEqual([{ srcRow: 0, startCol: 0, endCol: 4 }]);
  });

  test("returns every occurrence in reading order, including scrollback", () => {
    const g = new PaneGrid(1n, 6, 1);
    // scrollback row 0, then viewport row (srcRow 1).
    g.applyFullPaneSync(syncRows(["abab x"], ["ab ab "]));
    expect(g.search("ab")).toEqual([
      { srcRow: 0, startCol: 0, endCol: 1 }, // scrollback "ab ab "
      { srcRow: 0, startCol: 3, endCol: 4 },
      { srcRow: 1, startCol: 0, endCol: 1 }, // viewport "abab x"
      { srcRow: 1, startCol: 2, endCol: 3 },
    ]);
  });

  test("empty query and no-match return []", () => {
    const g = new PaneGrid(1n, 5, 1);
    g.applyFullPaneSync(syncRows(["hello"]));
    expect(g.search("")).toEqual([]);
    expect(g.search("zzz")).toEqual([]);
  });

  test("does not match across a row boundary", () => {
    const g = new PaneGrid(1n, 2, 2);
    g.applyFullPaneSync(syncRows(["ab", "cd"]));
    // "bc" spans the row1→row2 break and must not match.
    expect(g.search("bc")).toEqual([]);
  });

  test("scrollbackTrimmed counts front-evicted rows; scrollbackEpoch bumps on replace", () => {
    const g = new PaneGrid(1n, 2, 1, { maxScrollbackRows: 2 });
    expect(g.scrollbackTrimmed).toBe(0);
    expect(g.scrollbackEpoch).toBe(0);
    // Append 3 scrollback rows over a cap of 2 → 1 row trimmed.
    g.applyFullPaneSync(
      makeSync({
        cols: 2,
        rows: 1,
        cells: [cell("v"), cell(" ")],
        scrollback: [cell("a"), cell(" "), cell("b"), cell(" "), cell("c"), cell(" ")],
        scrollbackRows: 3,
        scrollbackReplace: false,
      }),
    );
    expect(g.scrollbackRows).toBe(2);
    expect(g.scrollbackTrimmed).toBe(1);
    expect(g.scrollbackEpoch).toBe(0); // append, not replace
    // A replace sync bumps the epoch (and doesn't trim).
    g.applyFullPaneSync(
      makeSync({
        cols: 2,
        rows: 1,
        cells: [cell("v"), cell(" ")],
        scrollback: [cell("x"), cell(" ")],
        scrollbackRows: 1,
        scrollbackReplace: true,
      }),
    );
    expect(g.scrollbackEpoch).toBe(1);
    expect(g.scrollbackTrimmed).toBe(1); // unchanged by replace
  });

  test("wide-char spacer cells map matches to the glyph column", () => {
    const g = new PaneGrid(1n, 4, 1);
    // "你" (wide) at col 0, spacer at col 1, then "ok".
    g.applyFullPaneSync(
      makeSync({
        cols: 4,
        rows: 1,
        cells: [
          cell("你", DEFAULT_FG, DEFAULT_BG, FLAG_WIDE_CHAR),
          cell(" ", DEFAULT_FG, DEFAULT_BG, FLAG_WIDE_CHAR_SPACER),
          cell("o"),
          cell("k"),
        ],
      }),
    );
    expect(g.search("ok")).toEqual([{ srcRow: 0, startCol: 2, endCol: 3 }]);
    // The wide glyph match extends over its trailing spacer column so
    // the highlight covers the full 2-column cell.
    expect(g.search("你")).toEqual([{ srcRow: 0, startCol: 0, endCol: 1 }]);
  });

  test("matches a grapheme built from combining-mark extras", () => {
    const g = new PaneGrid(1n, 3, 1);
    // Cell 0 renders "e" with a combining acute (U+0301) stored in
    // graphemeExtras → the full grapheme "é" must be searchable.
    const sync = makeSync({
      cols: 3,
      rows: 1,
      cells: [cell("e"), cell("k"), cell(" ")],
      graphemeExtras: new Map([[0, "́"]]),
    });
    g.applyFullPaneSync(sync);
    expect(g.search("é")).toEqual([{ srcRow: 0, startCol: 0, endCol: 0 }]);
  });
});
