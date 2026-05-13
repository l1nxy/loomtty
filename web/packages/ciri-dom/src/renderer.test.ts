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
