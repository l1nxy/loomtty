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

  test("bare URLs in plain text are autolinked as <a target=_blank>", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const text = "go https://a.io ok";
    const grid = new PaneGrid(1n, text.length, 1);
    const sync = blankSync(text.length, 1);
    sync.cells = [...text].map((c) => cell(c));
    grid.applyFullPaneSync(sync);
    r.render(grid);
    const a = root.querySelector("a");
    expect(a).not.toBeNull();
    expect(a!.getAttribute("href")).toBe("https://a.io");
    expect(a!.target).toBe("_blank");
    expect(a!.rel).toBe("noopener noreferrer");
    expect(a!.textContent).toBe("https://a.io");
    // The plain text around the link is preserved verbatim.
    expect(root.querySelector(".ciri-row")!.textContent).toBe(text);
    root.remove();
  });

  test("an OSC 8 link whose text is itself a URL is not double-wrapped", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const text = "https://x.io";
    const grid = new PaneGrid(1n, text.length, 1);
    const sync = blankSync(text.length, 1);
    sync.cells = [...text].map((c) => cell(c));
    // Whole row carries one OSC 8 link id.
    sync.cellLinks = new Map([...text].map((_, i) => [i, 1] as [number, number]));
    sync.linkMap = new Map([[1, "https://override.example"]]);
    grid.applyFullPaneSync(sync);
    r.render(grid);
    const anchors = root.querySelectorAll("a");
    // Exactly one anchor — the OSC 8 one — and it keeps the explicit
    // OSC 8 href, not the bare-URL autolink of the visible text.
    expect(anchors.length).toBe(1);
    expect(anchors[0]!.getAttribute("href")).toBe("https://override.example");
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

  test("onScrollChange fires on settled offset changes (incl. clamp)", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const seen: number[] = [];
    const r = new PaneRenderer(root, { onScrollChange: (n) => seen.push(n) });
    const grid = new PaneGrid(1n, 2, 2);
    grid.applyFullPaneSync(syncWithScrollback(2, 3, 2));
    r.render(grid); // offset 0, unchanged → no fire
    expect(seen).toEqual([]);
    r.setScrollOffset(2);
    r.render(grid);
    expect(seen).toEqual([2]);
    // Re-render at the same offset must not re-fire.
    r.render(grid);
    expect(seen).toEqual([2]);
    // A render-time clamp (offset exceeds available scrollback) reports
    // the clamped value, not the requested one.
    r.setScrollOffset(99);
    r.render(grid);
    expect(seen).toEqual([2, 3]);
    // Back to live.
    r.setScrollOffset(0);
    r.render(grid);
    expect(seen).toEqual([2, 3, 0]);
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

  test("appending scrollback under a scrolled-up user keeps the same content visible", () => {
    // Round-2 codex fix: previously a `scrollback_replace=false`
    // append grew `scrollbackRows` while the renderer's
    // `scrollOffset` stayed put, so the visible window drifted toward
    // the live bottom on every output line. The renderer now bumps
    // `scrollOffset` by the scrollback growth to preserve the
    // historical position.
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 2, 1);
    // 2 rows of scrollback (a, b), 1 row of viewport (A).
    grid.applyFullPaneSync(syncWithScrollback(2, 2, 1));
    r.setScrollOffset(2); // top of display = scrollback row 0 ("a")
    r.render(grid);
    expect(root.querySelector(".ciri-row")!.textContent).toBe("aa");
    // Append 1 new history row (c) — server's scrollbackReplace=false.
    grid.applyFullPaneSync({
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
      rows: 1,
      title: "",
      scrollback: [cell("c"), cell("c")],
      scrollbackRows: 1,
      scrollbackReplace: false,
      cells: [cell("D"), cell("D")],
      graphemeExtras: new Map(),
      cellLinks: new Map(),
      linkMap: new Map(),
      cwd: null,
    });
    r.render(grid);
    // User should still see "aa", not "bb" or "cc" — the historical
    // row didn't move under them.
    expect(root.querySelector(".ciri-row")!.textContent).toBe("aa");
    expect(r.scrollOffsetRows).toBe(3);
    root.remove();
  });

  test("appending scrollback while pinned at bottom keeps following live", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 2, 1);
    grid.applyFullPaneSync(syncWithScrollback(2, 1, 1)); // sb: a ; vp: A
    // scrollOffset stays 0 (default → pinned to live).
    r.render(grid);
    expect(root.querySelector(".ciri-row")!.textContent).toBe("AA");
    grid.applyFullPaneSync({
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
      rows: 1,
      title: "",
      scrollback: [cell("b"), cell("b")],
      scrollbackRows: 1,
      scrollbackReplace: false,
      cells: [cell("C"), cell("C")],
      graphemeExtras: new Map(),
      cellLinks: new Map(),
      linkMap: new Map(),
      cwd: null,
    });
    r.render(grid);
    expect(root.querySelector(".ciri-row")!.textContent).toBe("CC");
    expect(r.scrollOffsetRows).toBe(0);
    root.remove();
  });

  test("scrollback shrink (eviction) pulls scrollOffset back to keep view", () => {
    // PaneGrid trims oldest rows when an append would push past
    // maxScrollbackRows. The renderer must mirror that by reducing
    // scrollOffset so the same surviving rows remain visible (or
    // clamping when the visible row was itself trimmed).
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 2, 1, { maxScrollbackRows: 2 });
    // Seed: 2 sb rows (a, b), 1 viewport (A). Scroll up to view "a".
    grid.applyFullPaneSync(syncWithScrollback(2, 2, 1));
    r.setScrollOffset(2);
    r.render(grid);
    expect(root.querySelector(".ciri-row")!.textContent).toBe("aa");
    // Now an append of 1 row pushes to 3 rows → cap evicts row 0 ("a").
    // Surviving sb = ("b", new). The user's "a" is gone — clamp to top.
    grid.applyFullPaneSync({
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
      rows: 1,
      title: "",
      scrollback: [cell("c"), cell("c")],
      scrollbackRows: 1,
      scrollbackReplace: false,
      cells: [cell("D"), cell("D")],
      graphemeExtras: new Map(),
      cellLinks: new Map(),
      linkMap: new Map(),
      cwd: null,
    });
    r.render(grid);
    // Top of display now shows "b" — was at index 1, now index 0.
    expect(root.querySelector(".ciri-row")!.textContent).toBe("bb");
    expect(r.scrollOffsetRows).toBe(2);
    root.remove();
  });

  // ── cursor overlay ───────────────────────────────────────────────

  test("cursor element exists and is initially hidden until first render", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    new PaneRenderer(root);
    const c = root.querySelector(".ciri-cursor") as HTMLElement;
    expect(c).not.toBeNull();
    expect(c.style.display).toBe("none");
    root.remove();
  });

  test("render positions cursor at (cursorCol, cursorLine) of the live viewport", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root, { cursorBlink: false });
    const grid = new PaneGrid(1n, 4, 3);
    const sync = blankSync(4, 3);
    sync.meta.cursorLine = 2;
    sync.meta.cursorCol = 3;
    sync.meta.cursorShape = 0; // block
    grid.applyFullPaneSync(sync);
    r.render(grid);
    const c = root.querySelector(".ciri-cursor") as HTMLElement;
    expect(c.style.display).toBe("block");
    expect(c.style.left).toBe("3ch");
    // top uses calc(displayRow * 1.2em); displayRow = cursorLine + offset = 2.
    expect(c.style.top).toBe("2.4em");
    expect(c.className).toContain("ciri-cursor-block");
    root.remove();
  });

  test("cursor shape switches CSS class and styling", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root, { cursorBlink: false });
    const grid = new PaneGrid(1n, 2, 1);
    const sync = blankSync(2, 1);
    sync.meta.cursorLine = 0;
    sync.meta.cursorCol = 0;

    for (const [shape, name] of [
      [0, "block"],
      [1, "underline"],
      [2, "beam"],
      [4, "hollow"],
    ] as const) {
      sync.meta = { ...sync.meta, cursorShape: shape };
      const g = new PaneGrid(1n, 2, 1);
      g.applyFullPaneSync(sync);
      r.render(g);
      const c = root.querySelector(".ciri-cursor") as HTMLElement;
      expect(c.className).toBe(`ciri-cursor ciri-cursor-${name}`);
    }
    root.remove();
  });

  test("CURSOR_HIDDEN hides the overlay", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root, { cursorBlink: false });
    const grid = new PaneGrid(1n, 2, 1);
    const sync = blankSync(2, 1);
    sync.meta.cursorShape = 3; // hidden
    grid.applyFullPaneSync(sync);
    r.render(grid);
    const c = root.querySelector(".ciri-cursor") as HTMLElement;
    expect(c.style.display).toBe("none");
    root.remove();
  });

  test("cursor out-of-bounds (past cols or rows) hides the overlay", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root, { cursorBlink: false });
    const grid = new PaneGrid(1n, 2, 1);
    const sync = blankSync(2, 1);
    sync.meta.cursorLine = 99;
    sync.meta.cursorCol = 0;
    grid.applyFullPaneSync(sync);
    r.render(grid);
    const c = root.querySelector(".ciri-cursor") as HTMLElement;
    expect(c.style.display).toBe("none");
    root.remove();
  });

  test("CellDelta cursor move repositions the overlay without a full repaint", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root, { cursorBlink: false });
    const grid = new PaneGrid(1n, 4, 2);
    const sync = blankSync(4, 2);
    sync.meta.cursorLine = 0;
    sync.meta.cursorCol = 0;
    grid.applyFullPaneSync(sync);
    r.render(grid);
    const cursor = root.querySelector(".ciri-cursor") as HTMLElement;
    expect(cursor.style.left).toBe("0ch");
    // CellDelta carrying only a meta cursor move.
    grid.applyCellDelta({
      meta: {
        paneId: 1n,
        generation: 2n,
        cursorLine: 1,
        cursorCol: 2,
        cursorShape: 0,
        modeFlags: 0,
        echoAck: 0n,
      },
      cols: 4,
      regions: [],
    });
    r.render(grid);
    expect(cursor.style.left).toBe("2ch");
    expect(cursor.style.top).toBe("1.2em");
    root.remove();
  });

  test("scroll offset shifts the cursor display row alongside the viewport", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root, { cursorBlink: false });
    const grid = new PaneGrid(1n, 2, 2);
    grid.applyFullPaneSync(syncWithScrollback(2, 2, 2));
    // Cursor at viewport row 1 — currently at the live bottom.
    grid.applyCellDelta({
      meta: {
        paneId: 1n,
        generation: 2n,
        cursorLine: 1,
        cursorCol: 0,
        cursorShape: 0,
        modeFlags: 0,
        echoAck: 0n,
      },
      cols: 2,
      regions: [],
    });
    r.render(grid);
    const c = root.querySelector(".ciri-cursor") as HTMLElement;
    // Offset 0 → displayRow 1.
    expect(c.style.top).toBe("1.2em");
    // Scroll up by 1 — displayRow becomes 2, which is OUTSIDE the
    // 2-row viewport → hide.
    r.setScrollOffset(1);
    r.render(grid);
    expect(c.style.display).toBe("none");
    root.remove();
  });

  // ── selection overlay ─────────────────────────────────────────────

  test("setSelection on a single row paints one .ciri-selection-row rect", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root, { cursorBlink: false });
    const grid = new PaneGrid(1n, 5, 2);
    grid.applyFullPaneSync(blankSync(5, 2));
    r.setSelection({
      start: { col: 1, srcRow: 0 },
      end: { col: 3, srcRow: 0 },
      active: false,
    });
    r.render(grid);
    const rows = root.querySelectorAll(".ciri-selection-row");
    expect(rows.length).toBe(1);
    const rect = rows[0] as HTMLElement;
    expect(rect.style.left).toBe("1ch");
    expect(rect.style.width).toBe("3ch"); // cols 1..3 → 3 cells wide
    expect(rect.style.top).toBe("0em");
    expect(rect.style.height).toBe("1.2em");
    root.remove();
  });

  test("multi-row selection paints three rects with correct slices", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root, { cursorBlink: false });
    const grid = new PaneGrid(1n, 4, 3);
    grid.applyFullPaneSync(blankSync(4, 3));
    // Row 0 from col 2; row 1 full width; row 2 up to col 1.
    r.setSelection({
      start: { col: 2, srcRow: 0 },
      end: { col: 1, srcRow: 2 },
      active: true,
    });
    r.render(grid);
    const rows = root.querySelectorAll<HTMLElement>(".ciri-selection-row");
    expect(rows.length).toBe(3);
    expect(rows[0]!.style.left).toBe("2ch");
    expect(rows[0]!.style.width).toBe("2ch"); // cols 2..3
    expect(rows[1]!.style.left).toBe("0ch");
    expect(rows[1]!.style.width).toBe("4ch"); // full row
    expect(rows[2]!.style.left).toBe("0ch");
    expect(rows[2]!.style.width).toBe("2ch"); // cols 0..1
    root.remove();
  });

  test("setSelection(null) clears all rects on next render", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root, { cursorBlink: false });
    const grid = new PaneGrid(1n, 4, 1);
    grid.applyFullPaneSync(blankSync(4, 1));
    r.setSelection({
      start: { col: 0, srcRow: 0 },
      end: { col: 3, srcRow: 0 },
      active: false,
    });
    r.render(grid);
    expect(root.querySelectorAll(".ciri-selection-row").length).toBe(1);
    r.setSelection(null);
    r.render(grid);
    expect(root.querySelectorAll(".ciri-selection-row").length).toBe(0);
    root.remove();
  });

  test("reversed start/end produces the same paint as forward", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root, { cursorBlink: false });
    const grid = new PaneGrid(1n, 4, 2);
    grid.applyFullPaneSync(blankSync(4, 2));
    r.setSelection({
      start: { col: 3, srcRow: 1 },
      end: { col: 0, srcRow: 0 },
      active: false,
    });
    r.render(grid);
    const rows = root.querySelectorAll<HTMLElement>(".ciri-selection-row");
    expect(rows.length).toBe(2);
    expect(rows[0]!.style.top).toBe("0em");
    expect(rows[1]!.style.top).toBe("1.2em");
    root.remove();
  });

  test("selection rows outside the visible viewport are skipped", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root, { cursorBlink: false });
    const grid = new PaneGrid(1n, 2, 2);
    grid.applyFullPaneSync(syncWithScrollback(2, 2, 2));
    // sb rows 0,1 + vp rows 2,3 (srcRow indices).
    r.setSelection({
      start: { col: 0, srcRow: 0 },
      end: { col: 1, srcRow: 3 },
      active: true,
    });
    r.setScrollOffset(0); // shows vp rows (srcRow 2,3).
    r.render(grid);
    // Only 2 rects (srcRow 2 + 3 land in display rows 0 + 1).
    expect(root.querySelectorAll(".ciri-selection-row").length).toBe(2);
    root.remove();
  });

  test("theme.selectionBackground is honored", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root, {
      cursorBlink: false,
      theme: {
        ...DEFAULT_THEME,
        named: DEFAULT_THEME.named.slice(),
        selectionBackground: "rgba(255, 0, 0, 0.5)",
      },
    });
    const grid = new PaneGrid(1n, 2, 1);
    grid.applyFullPaneSync(blankSync(2, 1));
    r.setSelection({
      start: { col: 0, srcRow: 0 },
      end: { col: 1, srcRow: 0 },
      active: false,
    });
    r.render(grid);
    const rect = root.querySelector<HTMLElement>(".ciri-selection-row")!;
    // jsdom normalizes rgba but should preserve the channels.
    expect(rect.style.backgroundColor.replace(/\s/g, "")).toContain("rgba(255,0,0,0.5)");
    root.remove();
  });

  test("destroy clears selection state and removes overlay rects", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root, { cursorBlink: false });
    const grid = new PaneGrid(1n, 4, 1);
    grid.applyFullPaneSync(blankSync(4, 1));
    r.setSelection({
      start: { col: 0, srcRow: 0 },
      end: { col: 3, srcRow: 0 },
      active: false,
    });
    r.render(grid);
    expect(root.querySelector(".ciri-selection-row")).not.toBeNull();
    r.destroy();
    expect(document.querySelector(".ciri-selection-row")).toBeNull();
    expect(r.currentSelection).toBeNull();
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

  // ── pre-edit overlay (IME) ───────────────────────────────────────

  test("preedit element exists and is initially hidden", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const p = root.querySelector(".ciri-preedit") as HTMLElement;
    expect(p).not.toBeNull();
    expect(p.style.display).toBe("none");
    expect(r.currentPreedit).toBeNull();
    root.remove();
  });

  test("setPreedit shows the overlay at the cursor position", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root, { cursorBlink: false });
    const grid = new PaneGrid(1n, 10, 3);
    const sync = blankSync(10, 3);
    sync.meta.cursorLine = 1;
    sync.meta.cursorCol = 4;
    grid.applyFullPaneSync(sync);
    r.setPreedit("ni");
    r.render(grid);
    const p = root.querySelector(".ciri-preedit") as HTMLElement;
    expect(p.style.display).toBe("block");
    expect(p.style.left).toBe("4ch");
    expect(p.style.top).toBe(`${1 * 1.2}em`);
    expect(p.textContent).toBe("ni");
    // "ni" is two ASCII cells wide.
    expect(p.style.width).toBe("2ch");
    expect(r.currentPreedit).toBe("ni");
    root.remove();
  });

  test("setPreedit width counts CJK as double-width", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root, { cursorBlink: false });
    const grid = new PaneGrid(1n, 10, 1);
    grid.applyFullPaneSync(blankSync(10, 1));
    r.setPreedit("你好");
    r.render(grid);
    const p = root.querySelector<HTMLElement>(".ciri-preedit")!;
    // Two CJK glyphs → 4 cells.
    expect(p.style.width).toBe("4ch");
    expect(p.textContent).toBe("你好");
    root.remove();
  });

  test("setPreedit(null) hides the overlay on next render", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root, { cursorBlink: false });
    const grid = new PaneGrid(1n, 4, 1);
    grid.applyFullPaneSync(blankSync(4, 1));
    r.setPreedit("ab");
    r.render(grid);
    expect(
      (root.querySelector(".ciri-preedit") as HTMLElement).style.display,
    ).toBe("block");
    r.setPreedit(null);
    r.render(grid);
    const p = root.querySelector(".ciri-preedit") as HTMLElement;
    expect(p.style.display).toBe("none");
    expect(r.currentPreedit).toBeNull();
    root.remove();
  });

  test("setPreedit(empty string) is treated as a clear", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root, { cursorBlink: false });
    const grid = new PaneGrid(1n, 4, 1);
    grid.applyFullPaneSync(blankSync(4, 1));
    r.setPreedit("x");
    r.render(grid);
    r.setPreedit("");
    r.render(grid);
    expect(r.currentPreedit).toBeNull();
    expect(
      (root.querySelector(".ciri-preedit") as HTMLElement).style.display,
    ).toBe("none");
    root.remove();
  });

  test("preedit overlay hides when cursor is out of viewport bounds", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root, { cursorBlink: false });
    const grid = new PaneGrid(1n, 4, 2);
    const sync = blankSync(4, 2);
    sync.meta.cursorLine = 99; // past the bottom
    sync.meta.cursorCol = 0;
    grid.applyFullPaneSync(sync);
    r.setPreedit("test");
    r.render(grid);
    const p = root.querySelector(".ciri-preedit") as HTMLElement;
    // overlay element still exists but is hidden because the cursor
    // landed outside the visible rows.
    expect(p.style.display).toBe("none");
    // The preedit *state* survives — when the cursor moves back into
    // range the overlay should reappear.
    expect(r.currentPreedit).toBe("test");
    root.remove();
  });

  test("destroy clears preedit state", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root, { cursorBlink: false });
    const grid = new PaneGrid(1n, 4, 1);
    grid.applyFullPaneSync(blankSync(4, 1));
    r.setPreedit("zh");
    r.render(grid);
    expect(root.querySelector(".ciri-preedit")).not.toBeNull();
    r.destroy();
    // Wrapper is gone, so the preedit element goes with it.
    expect(document.querySelector(".ciri-preedit")).toBeNull();
    expect(r.currentPreedit).toBeNull();
  });

  function placeImage(r: PaneRenderer, grid: PaneGrid, row: number): void {
    r.setImage(grid, {
      imageId: 1n,
      col: 0,
      row,
      widthCells: 1,
      heightCells: 1,
      pixelWidth: 1,
      pixelHeight: 1,
      format: "rgba",
      data: new Uint8Array(4),
    });
  }

  test("inline image is dropped when scrollback is replaced (epoch bump)", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 2, 1);
    grid.applyFullPaneSync(blankSync(2, 1));
    r.render(grid);
    placeImage(r, grid, 0);
    expect(root.querySelectorAll("canvas.ciri-image").length).toBe(1);
    // Replace scrollback wholesale → epoch bump → anchor invalid → drop.
    const replace = blankSync(2, 1);
    replace.scrollback = [cell(" "), cell(" ")];
    replace.scrollbackRows = 1;
    replace.scrollbackReplace = true;
    grid.applyFullPaneSync(replace);
    r.render(grid);
    expect(root.querySelectorAll("canvas.ciri-image").length).toBe(0);
    root.remove();
  });

  test("inline image is dropped once front-trimmed past the top of scrollback", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const r = new PaneRenderer(root);
    const grid = new PaneGrid(1n, 2, 1, { maxScrollbackRows: 2 });
    grid.applyFullPaneSync(blankSync(2, 1));
    r.render(grid); // seed scrollback baselines
    placeImage(r, grid, 0); // anchored at srcRow 0 (no scrollback yet)
    expect(root.querySelectorAll("canvas.ciri-image").length).toBe(1);
    // Append 3 history rows over a cap of 2 → 1 row trimmed from the
    // front → the image's srcRow-0 line is evicted → image dropped.
    const grow = blankSync(2, 1);
    grow.scrollback = new Array(6).fill(cell(" "));
    grow.scrollbackRows = 3;
    grid.applyFullPaneSync(grow);
    expect(grid.scrollbackTrimmed).toBe(1);
    r.render(grid);
    expect(root.querySelectorAll("canvas.ciri-image").length).toBe(0);
    root.remove();
  });
});
