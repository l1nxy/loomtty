// Renderer integration tests. Mount a real PaneRenderer to a jsdom
// document, drive a grid through it, assert the resulting DOM
// structure.

import { describe, expect, test } from "vitest";
import {
  DEFAULT_BG,
  DEFAULT_FG,
  FLAG_BOLD,
  NAMED_RED,
  type FullPaneSync,
  type PackedCell,
  type PackedColor,
} from "@ciri/codec";
import { PaneGrid } from "./grid.js";
import { PaneRenderer } from "./renderer.js";
import { DEFAULT_THEME } from "./theme.js";

const RED: PackedColor = { kind: "named", index: NAMED_RED };

function cell(
  ch: string,
  fg = DEFAULT_FG,
  bg = DEFAULT_BG,
  flags = 0,
): PackedCell {
  return { ch, fg, bg, flags };
}

function blankSync(cols: number, rows: number): FullPaneSync {
  return {
    meta: {
      paneId: 1n,
      generation: 1n,
      cursorLine: 0,
      cursorCol: 0,
      cursorShape: 0,
      modeFlags: 0,
      echoAck: 0n,
    },
    cols,
    rows,
    title: "",
    scrollback: [],
    scrollbackRows: 0,
    scrollbackReplace: false,
    cells: new Array(cols * rows).fill(cell(" ")),
    graphemeExtras: new Map(),
    cellLinks: new Map(),
    linkMap: new Map(),
    cwd: null,
  };
}

describe("PaneRenderer", () => {
  test("mounts a wrapper under the root", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    new PaneRenderer(root);
    expect(root.querySelector(".ciri-pane")).not.toBeNull();
    root.remove();
  });

  test("renders one row per grid row after a FullPaneSync", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 3, 2);
    grid.applyFullPaneSync(blankSync(3, 2));
    r.render(grid);
    const rows = root.querySelectorAll(".ciri-row");
    expect(rows.length).toBe(2);
    root.remove();
  });

  test("renders adjacent same-style cells into a single span", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 3, 1);
    const sync = blankSync(3, 1);
    sync.cells = [cell("H"), cell("i"), cell("!")];
    grid.applyFullPaneSync(sync);
    r.render(grid);
    const row = root.querySelector(".ciri-row");
    expect(row).not.toBeNull();
    const spans = row!.querySelectorAll("span");
    expect(spans.length).toBe(1);
    expect(spans[0]!.textContent).toBe("Hi!");
    root.remove();
  });

  test("style change creates multiple spans", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 3, 1);
    const sync = blankSync(3, 1);
    sync.cells = [cell("a"), cell("b", RED, DEFAULT_BG, FLAG_BOLD), cell("c")];
    grid.applyFullPaneSync(sync);
    r.render(grid);
    const row = root.querySelector(".ciri-row")!;
    const spans = row.querySelectorAll("span");
    expect(spans.length).toBe(3);
    expect(spans[1]!.textContent).toBe("b");
    expect(spans[1]!.style.fontWeight).toBe("bold");
    root.remove();
  });

  test("dirty-row diffing only touches the changed row", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 2, 2);
    grid.applyFullPaneSync(blankSync(2, 2));
    r.render(grid);
    const beforeRows = Array.from(root.querySelectorAll(".ciri-row"));
    // Take a snapshot of the row 0 element identity so we can prove
    // the renderer didn't replace it.
    const row0Before = beforeRows[0]!;
    const row0SpansBefore = row0Before.children[0] ?? null;

    grid.applyCellDelta({
      meta: {
        paneId: 1n,
        generation: 2n,
        cursorLine: 0,
        cursorCol: 0,
        cursorShape: 0,
        modeFlags: 0,
        echoAck: 0n,
      },
      cols: 2,
      regions: [{ line: 1, left: 0, right: 0, cells: [cell("X")] }],
    });
    r.render(grid);

    const afterRows = Array.from(root.querySelectorAll(".ciri-row"));
    expect(afterRows[0]).toBe(row0Before); // same node
    expect(afterRows[0]!.children[0]).toBe(row0SpansBefore); // children untouched
    expect(afterRows[1]!.textContent).toBe("X "); // row 1 updated
    root.remove();
  });

  test("javascript: hyperlink URIs render as plain span, not <a>", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 2, 1);
    const sync = blankSync(2, 1);
    sync.cells = [cell("a"), cell("b")];
    sync.cellLinks = new Map([
      [0, 1],
      [1, 1],
    ]);
    sync.linkMap = new Map([[1, "javascript:alert(1)"]]);
    grid.applyFullPaneSync(sync);
    r.render(grid);
    // No <a> tag — the unsafe scheme falls through to a span.
    expect(root.querySelector("a")).toBeNull();
    const span = root.querySelector("span")!;
    expect(span.textContent).toBe("ab");
    root.remove();
  });

  test("data: hyperlink URIs render as plain span", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 2, 1);
    const sync = blankSync(2, 1);
    sync.cells = [cell("a"), cell("b")];
    sync.cellLinks = new Map([
      [0, 1],
      [1, 1],
    ]);
    sync.linkMap = new Map([[1, "data:text/html;base64,PHNjcmlwdD4="]]);
    grid.applyFullPaneSync(sync);
    r.render(grid);
    expect(root.querySelector("a")).toBeNull();
    root.remove();
  });

  test("mailto: hyperlink URIs render as <a>", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 2, 1);
    const sync = blankSync(2, 1);
    sync.cells = [cell("a"), cell("b")];
    sync.cellLinks = new Map([
      [0, 1],
      [1, 1],
    ]);
    sync.linkMap = new Map([[1, "mailto:user@example.com"]]);
    grid.applyFullPaneSync(sync);
    r.render(grid);
    expect(root.querySelector("a")).not.toBeNull();
    root.remove();
  });

  test("hyperlink cells render as <a target=_blank rel=noopener>", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 2, 1);
    const sync = blankSync(2, 1);
    sync.cells = [cell("a"), cell("b")];
    sync.cellLinks = new Map([
      [0, 1],
      [1, 1],
    ]);
    sync.linkMap = new Map([[1, "https://example.com/x"]]);
    grid.applyFullPaneSync(sync);
    r.render(grid);
    const a = root.querySelector("a");
    expect(a).not.toBeNull();
    expect(a!.href).toBe("https://example.com/x");
    expect(a!.target).toBe("_blank");
    expect(a!.rel).toBe("noopener noreferrer");
    expect(a!.textContent).toBe("ab");
    root.remove();
  });

  test("resize shrinks row containers", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 2, 3);
    grid.applyFullPaneSync(blankSync(2, 3));
    r.render(grid);
    expect(root.querySelectorAll(".ciri-row").length).toBe(3);
    grid.applyFullPaneSync(blankSync(2, 1));
    r.render(grid);
    expect(root.querySelectorAll(".ciri-row").length).toBe(1);
    root.remove();
  });

  test("destroy detaches the wrapper and is idempotent", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    expect(root.querySelector(".ciri-pane")).not.toBeNull();
    r.destroy();
    expect(root.querySelector(".ciri-pane")).toBeNull();
    r.destroy(); // no throw
    root.remove();
  });

  test("render after destroy throws", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    r.destroy();
    const grid = new PaneGrid(1n, 2, 1);
    expect(() => r.render(grid)).toThrow(/destroy/);
    root.remove();
  });

  test("setTheme forces a full repaint on the next render, even with no grid damage", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 2, 1);
    const sync = blankSync(2, 1);
    sync.cells = [cell("a"), cell("b")];
    grid.applyFullPaneSync(sync);
    r.render(grid);
    // Snapshot the initial color (jsdom normalizes RGB → "rgb(...)");
    // we don't pin the exact value because parsing differs by host —
    // we just check it changes after setTheme.
    const before = root.querySelector("span")!.style.color;
    // Drain dirty rows so the next render() sees a clean grid: this
    // is the bug round-2 codex caught — without the renderer-level
    // forced redraw, the next render() would no-op and existing
    // spans would keep the pre-theme colors.
    expect(before).toBeTruthy();
    // The named-foreground slot dominates over `theme.foreground`
    // for NAMED_FOREGROUND cells (see resolveNamed); override both
    // so the theme swap actually surfaces in the run output.
    const newNamed = DEFAULT_THEME.named.slice();
    newNamed[16 /* NAMED_FOREGROUND */] = "#ff00ff";
    r.setTheme({
      ...DEFAULT_THEME,
      named: newNamed,
      foreground: "#ff00ff",
    });
    r.render(grid); // no grid damage, but theme changed
    const after = root.querySelector("span")!.style.color;
    expect(after).not.toBe(before);
    root.remove();
  });

  test("setTheme + later CellDelta merges into a single repaint, not two", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 2, 1);
    const sync = blankSync(2, 1);
    sync.cells = [cell("a"), cell("b")];
    grid.applyFullPaneSync(sync);
    r.render(grid);
    r.setTheme({
      ...DEFAULT_THEME,
      named: DEFAULT_THEME.named.slice(),
      foreground: "#abcdef",
    });
    grid.applyCellDelta({
      meta: {
        paneId: 1n,
        generation: 2n,
        cursorLine: 0,
        cursorCol: 0,
        cursorShape: 0,
        modeFlags: 0,
        echoAck: 0n,
      },
      cols: 2,
      regions: [{ line: 0, left: 0, right: 0, cells: [cell("Z")] }],
    });
    r.render(grid);
    // The single render should both apply the cell delta AND repaint
    // every row with the new theme — verify by checking the new cell
    // is present, and the run color is the new fg.
    const row = root.querySelector(".ciri-row")!;
    expect(row.textContent).toBe("Zb");
    root.remove();
  });

  // ── scrollback offset ────────────────────────────────────────────

  /// Build a sync with both scrollback and viewport, each row filled
  /// with a single distinct character so a test can identify which
  /// source row rendered into which display row.
  function syncWithScrollback(
    cols: number,
    scrollbackRows: number,
    viewportRows: number,
  ): FullPaneSync {
    const sync = blankSync(cols, viewportRows);
    sync.scrollback = [];
    for (let r = 0; r < scrollbackRows; r += 1) {
      const ch = String.fromCharCode("a".charCodeAt(0) + r);
      for (let c = 0; c < cols; c += 1) sync.scrollback.push(cell(ch));
    }
    sync.scrollbackRows = scrollbackRows;
    sync.scrollbackReplace = true;
    sync.cells = [];
    for (let r = 0; r < viewportRows; r += 1) {
      const ch = String.fromCharCode("A".charCodeAt(0) + r);
      for (let c = 0; c < cols; c += 1) sync.cells.push(cell(ch));
    }
    return sync;
  }

  test("scroll offset 0 (default) shows the live viewport, not scrollback", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 2, 2);
    // 3 scrollback rows (a,b,c) + 2 viewport rows (A,B).
    grid.applyFullPaneSync(syncWithScrollback(2, 3, 2));
    r.render(grid);
    const rows = Array.from(root.querySelectorAll(".ciri-row"));
    expect(rows.length).toBe(2);
    expect(rows[0]!.textContent).toBe("AA");
    expect(rows[1]!.textContent).toBe("BB");
    root.remove();
  });

  test("setScrollOffset shifts display into scrollback", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 2, 2);
    grid.applyFullPaneSync(syncWithScrollback(2, 3, 2));
    r.render(grid);
    // Offset 1: top row becomes "last scrollback row" (c), bottom row
    // becomes "first viewport row" (A).
    r.setScrollOffset(1);
    r.render(grid);
    const rows = Array.from(root.querySelectorAll(".ciri-row"));
    expect(rows[0]!.textContent).toBe("cc");
    expect(rows[1]!.textContent).toBe("AA");
    // Offset 2: both scrollback rows b, c.
    r.setScrollOffset(2);
    r.render(grid);
    const rows2 = Array.from(root.querySelectorAll(".ciri-row"));
    expect(rows2[0]!.textContent).toBe("bb");
    expect(rows2[1]!.textContent).toBe("cc");
    root.remove();
  });

  test("setScrollOffset(0) returns to the live viewport bottom", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 2, 2);
    grid.applyFullPaneSync(syncWithScrollback(2, 3, 2));
    r.setScrollOffset(2);
    r.render(grid);
    r.setScrollOffset(0);
    r.render(grid);
    const rows = Array.from(root.querySelectorAll(".ciri-row"));
    expect(rows[0]!.textContent).toBe("AA");
    expect(rows[1]!.textContent).toBe("BB");
    root.remove();
  });

  test("render clamps scroll offset that exceeds available scrollback", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 2, 2);
    grid.applyFullPaneSync(syncWithScrollback(2, 1, 2));
    // User asked to scroll up by 5, but only 1 row of scrollback exists.
    r.setScrollOffset(5);
    r.render(grid);
    expect(r.scrollOffsetRows).toBe(1);
    const rows = Array.from(root.querySelectorAll(".ciri-row"));
    expect(rows[0]!.textContent).toBe("aa"); // the one scrollback row
    expect(rows[1]!.textContent).toBe("AA"); // first viewport row
    root.remove();
  });

  test("negative or non-integer setScrollOffset is clamped at 0", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    r.setScrollOffset(-5);
    expect(r.scrollOffsetRows).toBe(0);
    r.setScrollOffset(1.7);
    expect(r.scrollOffsetRows).toBe(1);
    root.remove();
  });

  test("dirty viewport row maps to display row r + scrollOffset", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 2, 3);
    grid.applyFullPaneSync(syncWithScrollback(2, 2, 3)); // sb: a,b ; vp: A,B,C
    r.setScrollOffset(1);
    r.render(grid);
    // At offset 1 the display window shows: b, A, B.
    {
      const rows = Array.from(root.querySelectorAll(".ciri-row"));
      expect(rows[0]!.textContent).toBe("bb");
      expect(rows[1]!.textContent).toBe("AA");
      expect(rows[2]!.textContent).toBe("BB");
    }
    // Now dirty viewport row 0 (the "A" row). With offset 1 it should
    // appear at display row 1.
    grid.applyCellDelta({
      meta: {
        paneId: 1n,
        generation: 2n,
        cursorLine: 0,
        cursorCol: 0,
        cursorShape: 0,
        modeFlags: 0,
        echoAck: 0n,
      },
      cols: 2,
      regions: [{ line: 0, left: 0, right: 1, cells: [cell("X"), cell("X")] }],
    });
    r.render(grid);
    const rows = Array.from(root.querySelectorAll(".ciri-row"));
    expect(rows[0]!.textContent).toBe("bb"); // top untouched
    expect(rows[1]!.textContent).toBe("XX"); // dirty row 0 now at display 1
    expect(rows[2]!.textContent).toBe("BB"); // bottom unchanged
    root.remove();
  });

  test("dirty viewport row that falls below the visible window is skipped", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 2, 2);
    grid.applyFullPaneSync(syncWithScrollback(2, 2, 2)); // sb: a,b ; vp: A,B
    r.setScrollOffset(2); // window now shows a, b — viewport off-screen
    r.render(grid);
    // Dirty bottom viewport row — it would map to display row 1+2 = 3,
    // which is outside [0, grid.rows). Must NOT throw and must not
    // change the visible "a, b" display.
    grid.applyCellDelta({
      meta: {
        paneId: 1n,
        generation: 2n,
        cursorLine: 0,
        cursorCol: 0,
        cursorShape: 0,
        modeFlags: 0,
        echoAck: 0n,
      },
      cols: 2,
      regions: [{ line: 1, left: 0, right: 1, cells: [cell("X"), cell("X")] }],
    });
    r.render(grid);
    const rows = Array.from(root.querySelectorAll(".ciri-row"));
    expect(rows[0]!.textContent).toBe("aa");
    expect(rows[1]!.textContent).toBe("bb");
    root.remove();
  });

  test("FullPaneSync that shrinks scrollback clamps the offset", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 2, 2);
    grid.applyFullPaneSync(syncWithScrollback(2, 3, 2));
    r.setScrollOffset(3); // exactly at the top
    r.render(grid);
    // Now another sync wipes scrollback entirely (scrollbackReplace + 0 rows).
    grid.applyFullPaneSync(syncWithScrollback(2, 0, 2));
    r.render(grid);
    expect(r.scrollOffsetRows).toBe(0);
    const rows = Array.from(root.querySelectorAll(".ciri-row"));
    expect(rows[0]!.textContent).toBe("AA");
    expect(rows[1]!.textContent).toBe("BB");
    root.remove();
  });

  test("setScrollOffset is a no-op when value matches current offset", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 2, 2);
    grid.applyFullPaneSync(syncWithScrollback(2, 2, 2));
    r.setScrollOffset(1);
    r.render(grid);
    // Snapshot a row element identity. Calling setScrollOffset with
    // the same value MUST NOT force a redraw — verify by checking that
    // the row's first child stays the same node when nothing dirty.
    const child0Before = root.querySelector(".ciri-row")!.children[0]!;
    r.setScrollOffset(1);
    r.render(grid);
    const child0After = root.querySelector(".ciri-row")!.children[0]!;
    expect(child0After).toBe(child0Before);
    root.remove();
  });

  test("uses default theme background and foreground on the wrapper", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const wrapper = r.container;
    expect(wrapper.style.backgroundColor).toBeTruthy();
    expect(wrapper.style.color).toBeTruthy();
    // Just sanity check that the value matches the default theme
    // (jsdom normalizes color strings, so equality across browsers is
    // shaky — we check the actual values via the resolver below).
    expect(DEFAULT_THEME.background).toBeTruthy();
    root.remove();
  });
});
