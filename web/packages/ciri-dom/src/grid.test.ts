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
} from "@ciri/codec";
import { GridShapeError, PaneGrid } from "./grid.js";

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
});
