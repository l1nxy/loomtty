import { describe, expect, test, vi } from "vitest";
import type { LayoutState } from "@loom/client";
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
    expect(root.querySelector(".loom-app")).not.toBeNull();
    expect(root.querySelector(".loom-workspaces")).not.toBeNull();
    expect(root.querySelector(".loom-workspace")).not.toBeNull();
    root.remove();
  });

  test("renders one workspace tab per workspace", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const lm = new LayoutManager(root);
    lm.setLayout(mkLayout([[[1n]], [[2n]], [[3n]]]));
    const tabs = root.querySelectorAll(".loom-ws");
    expect(tabs.length).toBe(3);
    // Active tab tagged with the active class.
    expect(tabs[0]!.classList.contains("loom-ws-active")).toBe(true);
    expect(tabs[1]!.classList.contains("loom-ws-active")).toBe(false);
    root.remove();
  });

  test("renders columns + tiles for the active workspace only", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const lm = new LayoutManager(root);
    // ws 0: cols [[1, 2], [3]]
    // ws 1: cols [[42]] — should NOT appear since ws 0 is active.
    lm.setLayout(mkLayout([[[1n, 2n], [3n]], [[42n]]]));
    expect(root.querySelectorAll(".loom-column").length).toBe(2);
    expect(root.querySelectorAll(".loom-tile").length).toBe(3);
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
    const cols = root.querySelectorAll<HTMLElement>(".loom-column");
    expect(cols[0]!.style.flex).toContain("0.3");
    expect(cols[1]!.style.flex).toContain("0.7");
    const tiles = root.querySelectorAll<HTMLElement>(".loom-tile");
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
    const cols = root.querySelectorAll<HTMLElement>(".loom-column");
    expect(cols[0]!.classList.contains("loom-column-active")).toBe(false);
    expect(cols[1]!.classList.contains("loom-column-active")).toBe(true);
    const tiles = root.querySelectorAll<HTMLElement>(".loom-tile");
    expect(tiles[0]!.classList.contains("loom-tile-active")).toBe(false);
    expect(tiles[1]!.classList.contains("loom-tile-active")).toBe(false);
    expect(tiles[2]!.classList.contains("loom-tile-active")).toBe(true);
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
    const activeTile = root.querySelector(".loom-tile-active") as HTMLElement;
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
    const tabs = root.querySelectorAll(".loom-ws");
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
    // The caller (LoomApp) is responsible for re-attaching the
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
    expect(root.querySelector(".loom-app")).not.toBeNull();
    lm.destroy();
    expect(root.querySelector(".loom-app")).toBeNull();
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
    expect(root.querySelectorAll(".loom-column").length).toBe(0);
    expect(root.querySelector(".loom-workspace")).not.toBeNull();
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

  describe("pane chip switcher", () => {
    test("renders the chrome with a toolbar role on the chip bar", () => {
      const root = document.createElement("div");
      document.body.appendChild(root);
      new LayoutManager(root);
      const bar = root.querySelector(".loom-panes");
      expect(bar).not.toBeNull();
      expect(bar!.getAttribute("role")).toBe("toolbar");
      expect(bar!.getAttribute("aria-label")).toBe("Switch active pane");
      root.remove();
    });

    test("renders one chip per pane in column-major order with the active one marked", () => {
      const root = document.createElement("div");
      document.body.appendChild(root);
      const lm = new LayoutManager(root);
      // Two columns, first one stacks 2 tiles, second is single tile.
      // Active column is 1 (right), so paneId=3 is the active.
      lm.setLayout(
        mkLayout([[[1n, 2n], [3n]]], {
          activeColumns: [1n],
        }),
      );
      const chips = root.querySelectorAll<HTMLButtonElement>(".loom-pane-chip");
      expect(chips.length).toBe(3);
      expect(chips[0]!.dataset["chipPaneId"]).toBe("1");
      expect(chips[1]!.dataset["chipPaneId"]).toBe("2");
      expect(chips[2]!.dataset["chipPaneId"]).toBe("3");
      // Only the active pane carries `aria-current="true"`; others
      // omit the attribute (cleaner DOM than `aria-current="false"`).
      expect(chips[0]!.getAttribute("aria-current")).toBeNull();
      expect(chips[1]!.getAttribute("aria-current")).toBeNull();
      expect(chips[2]!.getAttribute("aria-current")).toBe("true");
      // Chip is a real button (Enter/Space activate it for free).
      expect(chips[0]!.tagName).toBe("BUTTON");
      expect(chips[0]!.type).toBe("button");
      root.remove();
    });

    test("chip text falls back to `Pane N` when no title resolver is wired", () => {
      const root = document.createElement("div");
      document.body.appendChild(root);
      const lm = new LayoutManager(root);
      lm.setLayout(mkLayout([[[42n]]]));
      const chip = root.querySelector(".loom-pane-chip")!;
      expect(chip.textContent).toBe("Pane 42");
      root.remove();
    });

    test("chip text uses paneTitleFor when provided, and refreshes on setTileTitle", () => {
      const root = document.createElement("div");
      document.body.appendChild(root);
      const titles = new Map<bigint, string>([[1n, "zsh"]]);
      const lm = new LayoutManager(root, {
        paneTitleFor: (id) => titles.get(id) ?? "",
      });
      lm.setLayout(mkLayout([[[1n]]]));
      const chip = root.querySelector(".loom-pane-chip")!;
      expect(chip.textContent).toBe("zsh");
      // setTileTitle updates the chip label in place.
      lm.setTileTitle(1n, "nvim main.rs");
      expect(chip.textContent).toBe("nvim main.rs");
      // Clearing the title falls back to the `Pane N` placeholder so
      // the chip stays visible (not blank) when the program exits or
      // OSC 0/1/2 hasn't fired yet.
      lm.setTileTitle(1n, "");
      expect(chip.textContent).toBe("Pane 1");
      root.remove();
    });

    test("clicking a chip fires the same onPaneClick the tile uses", () => {
      const root = document.createElement("div");
      document.body.appendChild(root);
      const fn = vi.fn();
      const lm = new LayoutManager(root, { onPaneClick: fn });
      lm.setLayout(mkLayout([[[1n], [2n]]]));
      const chip2 = root.querySelector<HTMLButtonElement>(
        '.loom-pane-chip[data-chip-pane-id="2"]',
      )!;
      chip2.click();
      expect(fn).toHaveBeenCalledTimes(1);
      expect(fn).toHaveBeenCalledWith(2n);
      root.remove();
    });

    test("chips do NOT shadow tile lookups via data-pane-id", () => {
      // Existing tests and external callers reach for `the pane's
      // DOM` via `[data-pane-id='N']`. The chip uses
      // `data-chip-pane-id` so this selector still resolves to the
      // tile slot, not the chip.
      const root = document.createElement("div");
      document.body.appendChild(root);
      const lm = new LayoutManager(root);
      lm.setLayout(mkLayout([[[1n]]]));
      const hit = root.querySelector("[data-pane-id='1']")!;
      expect(hit.classList.contains("loom-tile")).toBe(true);
      expect(lm.getSlot(1n)).toBe(hit);
      root.remove();
    });

    test("empty workspace renders an empty chip bar (no chips, no error)", () => {
      const root = document.createElement("div");
      document.body.appendChild(root);
      const lm = new LayoutManager(root);
      lm.setLayout(mkLayout([[]]));
      expect(root.querySelector(".loom-panes")).not.toBeNull();
      expect(root.querySelectorAll(".loom-pane-chip").length).toBe(0);
      root.remove();
    });

    test("each chip is followed by a per-pane close button with the matching id", () => {
      const root = document.createElement("div");
      document.body.appendChild(root);
      const lm = new LayoutManager(root);
      lm.setLayout(mkLayout([[[1n], [2n]]]));
      const closes = root.querySelectorAll<HTMLButtonElement>(".loom-pane-close");
      expect(closes.length).toBe(2);
      expect(closes[0]!.dataset["closePaneId"]).toBe("1");
      expect(closes[1]!.dataset["closePaneId"]).toBe("2");
      // Accessible name carries the pane title (or the `Pane N`
      // fallback) so screen readers don't just announce "close".
      expect(closes[0]!.getAttribute("aria-label")).toBe("Close pane: Pane 1");
      root.remove();
    });

    test("close button click fires onPaneClose with the chip's pane id (not onPaneClick)", () => {
      const root = document.createElement("div");
      document.body.appendChild(root);
      const onClick = vi.fn();
      const onClose = vi.fn();
      const lm = new LayoutManager(root, {
        onPaneClick: onClick,
        onPaneClose: onClose,
      });
      lm.setLayout(mkLayout([[[1n], [2n]]]));
      const closeBtn = root.querySelector<HTMLButtonElement>(
        '.loom-pane-close[data-close-pane-id="2"]',
      )!;
      closeBtn.click();
      expect(onClose).toHaveBeenCalledTimes(1);
      expect(onClose).toHaveBeenCalledWith(2n);
      // The close click does NOT bubble through to onPaneClick — they
      // are separate callbacks for separate intents.
      expect(onClick).not.toHaveBeenCalled();
      root.remove();
    });

    test("setTileTitle refreshes the close button's aria-label", () => {
      const root = document.createElement("div");
      document.body.appendChild(root);
      const lm = new LayoutManager(root, {
        paneTitleFor: () => "zsh",
      });
      lm.setLayout(mkLayout([[[1n]]]));
      const closeBtn = root.querySelector(".loom-pane-close")!;
      expect(closeBtn.getAttribute("aria-label")).toBe("Close pane: zsh");
      lm.setTileTitle(1n, "nvim main.rs");
      expect(closeBtn.getAttribute("aria-label")).toBe(
        "Close pane: nvim main.rs",
      );
      root.remove();
    });

    test("actions: opts.actions renders one button per entry, with a divider after the chips", () => {
      const root = document.createElement("div");
      document.body.appendChild(root);
      const lm = new LayoutManager(root, {
        actions: [
          { id: "new-pane", icon: "+", label: "New pane" },
          { id: "settings", icon: "⚙", label: "Settings" },
        ],
      });
      lm.setLayout(mkLayout([[[1n]]]));
      const actionEls = root.querySelectorAll<HTMLButtonElement>(".loom-action");
      expect(actionEls.length).toBe(2);
      expect(actionEls[0]!.dataset["actionId"]).toBe("new-pane");
      expect(actionEls[0]!.textContent).toBe("+");
      expect(actionEls[0]!.getAttribute("aria-label")).toBe("New pane");
      expect(actionEls[0]!.title).toBe("New pane");
      // Divider appears between the chip group and the action group.
      const divider = root.querySelector(".loom-actions-divider")!;
      expect(divider).not.toBeNull();
      expect(divider.getAttribute("aria-hidden")).toBe("true");
      // No divider when there are no actions.
      const lm2 = new LayoutManager(document.body.appendChild(document.createElement("div")));
      lm2.setLayout(mkLayout([[[1n]]]));
      expect(document.querySelectorAll(".loom-actions-divider").length).toBe(1);
      root.remove();
    });

    test("clicking an action button fires onAction with the id", () => {
      const root = document.createElement("div");
      document.body.appendChild(root);
      const onAction = vi.fn();
      const lm = new LayoutManager(root, {
        actions: [{ id: "new-pane", icon: "+", label: "New pane" }],
        onAction,
      });
      lm.setLayout(mkLayout([[[1n]]]));
      const btn = root.querySelector<HTMLButtonElement>(".loom-action")!;
      btn.click();
      expect(onAction).toHaveBeenCalledWith("new-pane");
      root.remove();
    });

    test("no divider when the chip group is empty (but actions still render — empty workspace needs `+`)", () => {
      // An empty workspace is exactly the state in which a "+ new
      // pane" button is most useful — it's the way out of empty
      // state. Keep the actions visible; the divider is what's
      // skipped because there's no left-hand group to separate from.
      const root = document.createElement("div");
      document.body.appendChild(root);
      const lm = new LayoutManager(root, {
        actions: [{ id: "new-pane", icon: "+", label: "New pane" }],
      });
      lm.setLayout(mkLayout([[]]));
      expect(root.querySelectorAll(".loom-actions-divider").length).toBe(0);
      expect(root.querySelectorAll(".loom-action").length).toBe(1);
      root.remove();
    });
  });
});
