import { describe, expect, test, vi } from "vitest";
import type { LayoutState } from "@ciri/client";
import { LayoutManager } from "./layout.js";

/// Build a `LayoutState` from a compact array-of-arrays-of-paneIds.
/// `[[1, 2], [3]]` means workspace 0 has two columns; col 0 has tiles
/// 1 and 2, col 1 has tile 3. The first workspace is always active;
/// the first column / first tile of each column is active.
function mkLayout(
  workspaces: bigint[][][],
  opts: {
    activeWorkspaceIdx?: bigint;
    activeColumns?: bigint[]; // per-workspace
    activeTiles?: bigint[][]; // [ws][col]
    widthProportions?: number[][]; // [ws][col]
    tileWeights?: number[][][]; // [ws][col][tile]
  } = {},
): LayoutState {
  return {
    activeWorkspaceIdx: opts.activeWorkspaceIdx ?? 0n,
    workspaces: workspaces.map((cols, wsi) => ({
      activeColumnIdx: opts.activeColumns?.[wsi] ?? 0n,
      columns: cols.map((tiles, ci) => ({
        activeTileIdx: opts.activeTiles?.[wsi]?.[ci] ?? 0n,
        widthProportion: opts.widthProportions?.[wsi]?.[ci] ?? 1.0,
        widthFixedPx: null,
        tiles: tiles.map((paneId, ti) => ({
          paneId,
          weight: opts.tileWeights?.[wsi]?.[ci]?.[ti] ?? 1.0,
        })),
      })),
    })),
  };
}

describe("LayoutManager", () => {
  test("mounts the chrome under root", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    new LayoutManager(root);
    expect(root.querySelector(".ciri-app")).not.toBeNull();
    expect(root.querySelector(".ciri-workspaces")).not.toBeNull();
    expect(root.querySelector(".ciri-workspace")).not.toBeNull();
    root.remove();
  });

  test("renders one workspace tab per workspace", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const lm = new LayoutManager(root);
    lm.setLayout(mkLayout([[[1n]], [[2n]], [[3n]]]));
    const tabs = root.querySelectorAll(".ciri-ws");
    expect(tabs.length).toBe(3);
    // Active tab tagged with the active class.
    expect(tabs[0]!.classList.contains("ciri-ws-active")).toBe(true);
    expect(tabs[1]!.classList.contains("ciri-ws-active")).toBe(false);
    root.remove();
  });

  test("renders columns + tiles for the active workspace only", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const lm = new LayoutManager(root);
    // ws 0: cols [[1, 2], [3]]
    // ws 1: cols [[42]] — should NOT appear since ws 0 is active.
    lm.setLayout(mkLayout([[[1n, 2n], [3n]], [[42n]]]));
    expect(root.querySelectorAll(".ciri-column").length).toBe(2);
    expect(root.querySelectorAll(".ciri-tile").length).toBe(3);
    // pane 42 (workspace 1) is NOT mounted.
    expect(
      root.querySelector("[data-pane-id='42']"),
    ).toBeNull();
    root.remove();
  });

  test("applies widthProportion to column flex and tile weight to tile flex", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const lm = new LayoutManager(root);
    lm.setLayout(
      mkLayout([[[1n], [2n]]], {
        widthProportions: [[0.3, 0.7]],
        tileWeights: [[[2.5], [4.0]]],
      }),
    );
    const cols = root.querySelectorAll<HTMLElement>(".ciri-column");
    expect(cols[0]!.style.flex).toContain("0.3");
    expect(cols[1]!.style.flex).toContain("0.7");
    const tiles = root.querySelectorAll<HTMLElement>(".ciri-tile");
    expect(tiles[0]!.style.flex).toContain("2.5");
    expect(tiles[1]!.style.flex).toContain("4");
    root.remove();
  });

  test("marks active column + active tile with the active class", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const lm = new LayoutManager(root);
    // ws 0: two columns. Active = col 1. Active tile in col 1 = tile 1.
    lm.setLayout(
      mkLayout([[[1n], [2n, 3n]]], {
        activeColumns: [1n],
        activeTiles: [[0n, 1n]],
      }),
    );
    const cols = root.querySelectorAll<HTMLElement>(".ciri-column");
    expect(cols[0]!.classList.contains("ciri-column-active")).toBe(false);
    expect(cols[1]!.classList.contains("ciri-column-active")).toBe(true);
    const tiles = root.querySelectorAll<HTMLElement>(".ciri-tile");
    expect(tiles[0]!.classList.contains("ciri-tile-active")).toBe(false);
    expect(tiles[1]!.classList.contains("ciri-tile-active")).toBe(false);
    expect(tiles[2]!.classList.contains("ciri-tile-active")).toBe(true);
    root.remove();
  });

  test("active tile sits in the active column only", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const lm = new LayoutManager(root);
    // col 0 active tile = 1 (3n), col 1 active tile = 0 (4n).
    // Active column = 1 → only the col 1 tile 0 gets active class.
    lm.setLayout(
      mkLayout([[[1n, 3n], [4n, 5n]]], {
        activeColumns: [1n],
        activeTiles: [[1n, 0n]],
      }),
    );
    const activeTile = root.querySelector(".ciri-tile-active") as HTMLElement;
    expect(activeTile).not.toBeNull();
    expect(activeTile.dataset["paneId"]).toBe("4");
    root.remove();
  });

  test("getSlot returns the tile DOM node for a pane in the layout", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const lm = new LayoutManager(root);
    lm.setLayout(mkLayout([[[7n, 8n]]]));
    const slot = lm.getSlot(7n);
    expect(slot).not.toBeNull();
    expect(slot!.dataset["paneId"]).toBe("7");
    expect(lm.getSlot(99n)).toBeNull();
    root.remove();
  });

  test("paneIdsInLayout enumerates active-workspace panes", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const lm = new LayoutManager(root);
    lm.setLayout(mkLayout([[[1n, 2n], [3n]], [[42n]]]));
    expect(lm.paneIdsInLayout().sort()).toEqual([1n, 2n, 3n]);
    root.remove();
  });

  test("activePaneId resolves the active workspace/column/tile triple", () => {
    const layout = mkLayout([[[1n, 3n], [4n, 5n]]], {
      activeColumns: [1n],
      activeTiles: [[1n, 0n]],
    });
    expect(LayoutManager.activePaneId(layout)).toBe(4n);
  });

  test("activePaneId returns null when layout is empty", () => {
    expect(LayoutManager.activePaneId(mkLayout([]))).toBeNull();
  });

  test("clicking a tile fires onPaneClick with its paneId", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const clicks: bigint[] = [];
    const lm = new LayoutManager(root, {
      onPaneClick: (id) => clicks.push(id),
    });
    lm.setLayout(mkLayout([[[10n], [20n]]]));
    const slot = lm.getSlot(20n);
    expect(slot).not.toBeNull();
    slot!.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    expect(clicks).toEqual([20n]);
    root.remove();
  });

  test("clicking a workspace tab fires onWorkspaceClick with its idx", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const switches: bigint[] = [];
    const lm = new LayoutManager(root, {
      onWorkspaceClick: (idx) => switches.push(idx),
    });
    lm.setLayout(mkLayout([[[1n]], [[2n]], [[3n]]]));
    const tabs = root.querySelectorAll(".ciri-ws");
    (tabs[2] as HTMLElement).click();
    expect(switches).toEqual([2n]);
    root.remove();
  });

  test("layout change moves a pane renderer container to its new slot", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const lm = new LayoutManager(root);
    lm.setLayout(mkLayout([[[1n, 2n]]]));
    const slotA = lm.getSlot(1n)!;
    const fakeRenderer = document.createElement("div");
    fakeRenderer.className = "fake-pane";
    slotA.appendChild(fakeRenderer);
    expect(slotA.contains(fakeRenderer)).toBe(true);
    // Layout reshape: column split, pane 1 moves to its own column.
    lm.setLayout(mkLayout([[[1n], [2n]]]));
    const slotB = lm.getSlot(1n)!;
    // Old slot is gone; new slot exists.
    expect(document.body.contains(slotA)).toBe(false);
    expect(slotB).not.toBe(slotA);
    // The caller (CiriApp) is responsible for re-attaching the
    // renderer to the new slot. After doing so the chrome should
    // contain it again.
    slotB.appendChild(fakeRenderer);
    expect(slotB.contains(fakeRenderer)).toBe(true);
    root.remove();
  });

  test("destroy detaches chrome and is idempotent", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const lm = new LayoutManager(root);
    expect(root.querySelector(".ciri-app")).not.toBeNull();
    lm.destroy();
    expect(root.querySelector(".ciri-app")).toBeNull();
    lm.destroy();
    root.remove();
  });

  test("setLayout after destroy throws", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const lm = new LayoutManager(root);
    lm.destroy();
    expect(() => lm.setLayout(mkLayout([[[1n]]]))).toThrow(/destroy/);
    root.remove();
  });

  test("empty workspace at activeIdx leaves wsContent empty without throwing", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const lm = new LayoutManager(root);
    // 1 workspace, 0 columns. Active idx = 0.
    lm.setLayout(mkLayout([[]]));
    expect(root.querySelectorAll(".ciri-column").length).toBe(0);
    expect(root.querySelector(".ciri-workspace")).not.toBeNull();
    root.remove();
  });

  test("onPaneClick handler can be unchanged across setLayout calls", () => {
    // Make sure registering a single handler doesn't accumulate
    // listeners that fire multiple times per click after a reshape.
    const root = document.createElement("div");
    document.body.appendChild(root);
    const fn = vi.fn();
    const lm = new LayoutManager(root, { onPaneClick: fn });
    lm.setLayout(mkLayout([[[1n]]]));
    lm.setLayout(mkLayout([[[1n]]]));
    lm.getSlot(1n)!.dispatchEvent(new MouseEvent("mousedown"));
    expect(fn).toHaveBeenCalledTimes(1);
    root.remove();
  });
});
