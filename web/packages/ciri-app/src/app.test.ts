// CiriApp integration tests. We construct the app against a fake
// client factory that captures outbound messages and exposes the
// inbound onEvent callback. The fake never opens a WebSocket, so the
// test runs purely against jsdom + decoded fixture payloads.

import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type {
  CiriEvent,
  ClientMessage,
  LayoutState,
} from "@ciri/client";
import {
  CELL_DELTA_FIXTURES,
  FULL_PANE_SYNC_FIXTURES,
} from "@ciri/codec/fixtures";
import { CiriApp, type CiriAppClientLike } from "./app.js";

// Bootstrap a CiriApp + collect what its fake client sees.
function bootstrap(opts: { sessionName?: string } = {}) {
  const root = document.createElement("div");
  // Give the root a non-zero layout so initial measurement falls back
  // cleanly. jsdom won't actually compute a real layout, but our
  // measure helper has a fallback path that catches that.
  document.body.appendChild(root);

  const sent: ClientMessage[] = [];
  const inputs: { paneId: bigint; data: Uint8Array }[] = [];
  let capturedEvent: ((e: CiriEvent) => void) | null = null;
  let started = false;
  let closed = false;
  const fakeClient: CiriAppClientLike = {
    start() {
      started = true;
    },
    close() {
      closed = true;
    },
    send(msg) {
      sent.push(msg);
    },
    sendInput(paneId, data) {
      inputs.push({ paneId, data });
      return 1n;
    },
  };

  const onErrorCalls: Error[] = [];
  const app = new CiriApp(root, {
    url: "ws://test",
    sessionName: opts.sessionName ?? "default",
    clientFactory: (_o, onEvent) => {
      capturedEvent = onEvent;
      return fakeClient;
    },
    onError: (e) => onErrorCalls.push(e),
  });

  const fire = (e: CiriEvent) => {
    if (capturedEvent === null) throw new Error("client not yet hooked");
    capturedEvent(e);
  };

  return {
    root,
    app,
    sent,
    inputs,
    onErrorCalls,
    fire,
    state: { get started() { return started; }, get closed() { return closed; } },
  };
}

function hexToBytes(hex: string): Uint8Array {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < hex.length; i += 2) {
    out[i / 2] = Number.parseInt(hex.slice(i, i + 2), 16);
  }
  return out;
}

function mkLayoutSingle(paneId: bigint): LayoutState {
  return {
    activeWorkspaceIdx: 0n,
    workspaces: [
      {
        activeColumnIdx: 0n,
        columns: [
          {
            activeTileIdx: 0n,
            widthProportion: 1.0,
            widthFixedPx: null,
            tiles: [{ paneId, weight: 1.0 }],
          },
        ],
      },
    ],
  };
}

function mkLayoutTwo(pane1: bigint, pane2: bigint): LayoutState {
  return {
    activeWorkspaceIdx: 0n,
    workspaces: [
      {
        activeColumnIdx: 0n,
        columns: [
          {
            activeTileIdx: 0n,
            widthProportion: 0.5,
            widthFixedPx: null,
            tiles: [{ paneId: pane1, weight: 1.0 }],
          },
          {
            activeTileIdx: 0n,
            widthProportion: 0.5,
            widthFixedPx: null,
            tiles: [{ paneId: pane2, weight: 1.0 }],
          },
        ],
      },
    ],
  };
}

const FULL_SYNC_3X2 = FULL_PANE_SYNC_FIXTURES.find((f) => f.name === "3x2 default")!;
const FULL_SYNC_2X1 = FULL_PANE_SYNC_FIXTURES.find((f) =>
  f.name === "2x1 with 1-row scrollback (replace)",
)!;
const FULL_SYNC_HYPER = FULL_PANE_SYNC_FIXTURES.find((f) =>
  f.name === "4x1 with title, cwd, grapheme + hyperlink",
)!;

afterEach(() => {
  // jsdom keeps body across tests by default; clean every time so our
  // ".ciri-app" / ".ciri-orphan-panes" probes don't leak.
  document.body.replaceChildren();
});

describe("CiriApp lifecycle", () => {
  test("constructor mounts chrome + orphan root", () => {
    const { root } = bootstrap();
    expect(root.querySelector(".ciri-app")).not.toBeNull();
    expect(document.body.querySelector(".ciri-orphan-panes")).not.toBeNull();
  });

  test("start() boots the underlying client", () => {
    const { app, state } = bootstrap();
    expect(state.started).toBe(false);
    app.start();
    expect(state.started).toBe(true);
  });

  test("destroy() closes client and clears DOM", () => {
    const { app, root, state } = bootstrap();
    app.start();
    app.destroy();
    expect(state.closed).toBe(true);
    expect(root.querySelector(".ciri-app")).toBeNull();
    expect(document.body.querySelector(".ciri-orphan-panes")).toBeNull();
  });

  test("destroy() is idempotent", () => {
    const { app } = bootstrap();
    app.destroy();
    app.destroy();
  });

  test("start() after destroy() throws", () => {
    const { app } = bootstrap();
    app.destroy();
    expect(() => app.start()).toThrow(/destroy/);
  });

  test("destroy() unregisters root DOM listeners", () => {
    // Round-3 codex fix: anonymous handler arrows couldn't be
    // removeEventListener'd. A SPA tearing CiriApp down and recreating
    // it on the same root previously double-fired every keystroke and
    // forwarded events into the closed client.
    const { app, root, fire, inputs } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({
      kind: "full-pane-sync",
      payload: hexToBytes(FULL_SYNC_3X2.hex),
    });
    // Sanity check: a key on the live app forwards to the client.
    root.dispatchEvent(new KeyboardEvent("keydown", { key: "a", bubbles: true }));
    expect(inputs.length).toBe(1);
    // Destroy and replay — the listener should be detached, so the
    // event reaches no handler.
    app.destroy();
    root.dispatchEvent(new KeyboardEvent("keydown", { key: "b", bubbles: true }));
    root.dispatchEvent(new WheelEvent("wheel", { deltaY: 100, bubbles: true }));
    root.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    expect(inputs.length).toBe(1); // unchanged
  });
});

describe("CiriApp — layout + full-pane-sync", () => {
  test("LayoutUpdate then FullPaneSync mounts a renderer in the right slot", () => {
    const { app, fire } = bootstrap();
    app.start();
    // Bootstrap the layout for paneId=1.
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    // Feed the 3x2 FullPaneSync (paneId=1).
    fire({
      kind: "full-pane-sync",
      payload: hexToBytes(FULL_SYNC_3X2.hex),
    });
    const tile = document.querySelector<HTMLElement>(
      "[data-pane-id='1']",
    );
    expect(tile).not.toBeNull();
    const pane = tile!.querySelector(".ciri-pane");
    expect(pane).not.toBeNull();
    expect(pane!.querySelectorAll(".ciri-row").length).toBe(2); // rows=2
    expect(app.paneCount).toBe(1);
    expect(app.hasGrid(1n)).toBe(true);
  });

  test("FullPaneSync arriving before layout parks renderer in the orphan root", () => {
    const { app, fire } = bootstrap();
    app.start();
    fire({
      kind: "full-pane-sync",
      payload: hexToBytes(FULL_SYNC_3X2.hex),
    });
    expect(app.hasGrid(1n)).toBe(true);
    const orphans = document.querySelector(".ciri-orphan-panes")!;
    expect(orphans.querySelector(".ciri-pane")).not.toBeNull();
    // After a later LayoutUpdate, the renderer should move into its slot.
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    expect(orphans.querySelector(".ciri-pane")).toBeNull();
    const tile = document.querySelector("[data-pane-id='1']")!;
    expect(tile.querySelector(".ciri-pane")).not.toBeNull();
  });

  test("PaneClosed destroys the grid and removes the renderer", () => {
    const { app, fire } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({
      kind: "full-pane-sync",
      payload: hexToBytes(FULL_SYNC_3X2.hex),
    });
    expect(app.paneCount).toBe(1);
    fire({
      kind: "server-msg",
      msg: { tag: "PaneClosed", paneId: 1n },
    });
    expect(app.paneCount).toBe(0);
    expect(app.hasGrid(1n)).toBe(false);
  });

  test("CellDelta for an unknown paneId is silently dropped", () => {
    const { app, fire, onErrorCalls } = bootstrap();
    app.start();
    // Use a CellDelta fixture for paneId=42 — no grid for it.
    const fx = CELL_DELTA_FIXTURES.find((f) => f.name === "Hello on row 0")!;
    fire({ kind: "cell-delta", payload: hexToBytes(fx.hex) });
    // No grid was created, no error reported, no DOM produced.
    expect(app.hasGrid(42n)).toBe(false);
    expect(onErrorCalls).toEqual([]);
  });

  test("hyperlink fixture renders the OSC 8 anchor", () => {
    const { app, fire } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(2n) },
    });
    fire({
      kind: "full-pane-sync",
      payload: hexToBytes(FULL_SYNC_HYPER.hex),
    });
    expect(app.hasGrid(2n)).toBe(true);
    const tile = document.querySelector("[data-pane-id='2']")!;
    const anchor = tile.querySelector("a");
    expect(anchor).not.toBeNull();
    expect(anchor!.getAttribute("href")).toBe("https://example.com");
  });
});

describe("CiriApp — layout reshape moves renderers", () => {
  test("renderer container follows its pane to a new tile slot", () => {
    const { app, fire } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({
      kind: "full-pane-sync",
      payload: hexToBytes(FULL_SYNC_3X2.hex),
    });
    const tileA = document.querySelector("[data-pane-id='1']")!;
    expect(tileA.querySelector(".ciri-pane")).not.toBeNull();
    // New layout: pane 1 + pane 2 side by side. Pane 1 has a NEW slot.
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutTwo(1n, 2n) },
    });
    expect(document.body.contains(tileA)).toBe(false);
    const tileA2 = document.querySelector("[data-pane-id='1']")!;
    expect(tileA2.querySelector(".ciri-pane")).not.toBeNull();
    expect(app.paneCount).toBe(1);
  });

  test("renderer of a now-inactive workspace is parked in orphan root", () => {
    const { app, fire } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({
      kind: "full-pane-sync",
      payload: hexToBytes(FULL_SYNC_3X2.hex),
    });
    // Switch to a workspace that doesn't contain pane 1.
    const layoutOther: LayoutState = {
      activeWorkspaceIdx: 1n,
      workspaces: [
        {
          activeColumnIdx: 0n,
          columns: [
            {
              activeTileIdx: 0n,
              widthProportion: 1.0,
              widthFixedPx: null,
              tiles: [{ paneId: 1n, weight: 1.0 }],
            },
          ],
        },
        {
          activeColumnIdx: 0n,
          columns: [
            {
              activeTileIdx: 0n,
              widthProportion: 1.0,
              widthFixedPx: null,
              tiles: [{ paneId: 99n, weight: 1.0 }],
            },
          ],
        },
      ],
    };
    fire({ kind: "server-msg", msg: { tag: "LayoutUpdate", layout: layoutOther } });
    // Pane 1 should be parked.
    const orphans = document.querySelector(".ciri-orphan-panes")!;
    expect(orphans.querySelectorAll(".ciri-pane").length).toBe(1);
    expect(document.querySelector("[data-pane-id='1']")).toBeNull();
    expect(app.paneCount).toBe(1);
  });
});

describe("CiriApp — keyboard routes to active pane", () => {
  test("Enter on the root sends an Input to the active pane", () => {
    const { root, app, fire, inputs } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({
      kind: "full-pane-sync",
      payload: hexToBytes(FULL_SYNC_3X2.hex),
    });
    root.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(inputs.length).toBe(1);
    expect(inputs[0]!.paneId).toBe(1n);
    expect(Array.from(inputs[0]!.data)).toEqual([0x0d]);
  });

  test("with no active pane the key is dropped", () => {
    const { root, inputs } = bootstrap();
    // No layout fed → currentLayout is null → no active pane.
    root.dispatchEvent(new KeyboardEvent("keydown", { key: "a", bubbles: true }));
    expect(inputs.length).toBe(0);
  });

  test("click-then-type routes the keystroke to the just-clicked pane, not the previous active (round-7)", () => {
    // Round-7 codex race: before the server's LayoutUpdate confirms
    // the focus change, the new key would still target the stale
    // currentLayout's active pane. The app now optimistically routes
    // to the most-recently-clicked pane until the next LayoutUpdate.
    const { root, app, fire, inputs } = bootstrap();
    app.start();
    // Bootstrap two-pane layout, active=pane1; create grids for both
    // so the input pre-flight check has something to find.
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutTwo(1n, 2n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    // Set up pane 2 too. Construct a sync for pane 2 by reusing the
    // FULL_SYNC_2X1 fixture (paneId=3) — we need ANY grid for paneId
    // 2 so the input pre-flight passes. Build a minimal payload via
    // the typed decoder by feeding through handleClientEvent with a
    // hand-crafted typed value. Simpler: feed FULL_SYNC_HYPER which
    // has paneId=2.
    fire({
      kind: "full-pane-sync",
      payload: hexToBytes(FULL_SYNC_HYPER.hex),
    });
    expect(app.hasGrid(2n)).toBe(true);
    // User clicks pane 2 — no server response yet.
    const tile2 = document.querySelector<HTMLElement>("[data-pane-id='2']")!;
    tile2.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    // Type immediately.
    root.dispatchEvent(new KeyboardEvent("keydown", { key: "x", bubbles: true }));
    // The keystroke should land on pane 2, not pane 1.
    const xInputs = inputs.filter((i) => i.paneId === 2n && i.data[0] === 0x78);
    expect(xInputs.length).toBe(1);
    expect(inputs.find((i) => i.paneId === 1n && i.data[0] === 0x78)).toBeUndefined();
  });

  test("LayoutUpdate clears the pending click-focus so server stays authoritative", () => {
    const { root, app, fire, inputs } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutTwo(1n, 2n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_HYPER.hex) });
    // Click pane 2 (optimistic pending = 2).
    const tile2 = document.querySelector<HTMLElement>("[data-pane-id='2']")!;
    tile2.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    // Server replies with a LayoutUpdate that, for whatever reason,
    // keeps pane 1 active. The pending optimistic state must clear.
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutTwo(1n, 2n) },
    });
    root.dispatchEvent(new KeyboardEvent("keydown", { key: "y", bubbles: true }));
    // The "y" key should reach pane 1 (current active), not the
    // stale pane-2 click target.
    expect(inputs.find((i) => i.paneId === 1n && i.data[0] === 0x79)).toBeDefined();
    expect(inputs.find((i) => i.paneId === 2n && i.data[0] === 0x79)).toBeUndefined();
  });

  test("DECCKM (MODE_APP_CURSOR) flips arrow encoding from CSI to SS3", () => {
    // Without app-cursor mode: ArrowUp → ESC[A (CSI).
    // With app-cursor mode: ArrowUp → ESCOA (SS3). vim/less/htop
    // enable this; we read the per-pane meta flag for every keystroke.
    const { root, app, fire, inputs } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    // Default fixture has modeFlags = 0 → CSI path.
    root.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true }));
    expect(inputs.length).toBe(1);
    expect(Array.from(inputs[0]!.data)).toEqual([0x1b, 0x5b, 0x41]); // ESC[A
    // Now toggle MODE_APP_CURSOR via direct grid-meta poke (Rust
    // would toggle this in response to DECSET 1; we simulate the
    // post-decode effect here).
    const grid = app.paneGrid(1n)!;
    grid.meta = { ...grid.meta, modeFlags: 0x2000 /* MODE_APP_CURSOR */ };
    root.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true }));
    expect(inputs.length).toBe(2);
    expect(Array.from(inputs[1]!.data)).toEqual([0x1b, 0x4f, 0x41]); // ESCOA
  });

  test("modifier-only and metaKey events are ignored", () => {
    const { root, app, fire, inputs } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({
      kind: "full-pane-sync",
      payload: hexToBytes(FULL_SYNC_3X2.hex),
    });
    root.dispatchEvent(new KeyboardEvent("keydown", { key: "Shift", bubbles: true }));
    root.dispatchEvent(
      new KeyboardEvent("keydown", { key: "c", metaKey: true, bubbles: true }),
    );
    expect(inputs.length).toBe(0);
  });
});

describe("CiriApp — mouse + wheel", () => {
  test("mousedown on a tile sends FocusPane", () => {
    const { app, fire, sent } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutTwo(1n, 2n) },
    });
    const tile2 = document.querySelector<HTMLElement>("[data-pane-id='2']")!;
    tile2.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    const focuses = sent.filter((m) => m.tag === "FocusPane");
    expect(focuses).toEqual([{ tag: "FocusPane", paneId: 2n }]);
  });

  test("workspace tab click sends SwitchWorkspace", () => {
    const { app, fire, sent } = bootstrap();
    app.start();
    // Two workspaces, currently on 0.
    fire({
      kind: "server-msg",
      msg: {
        tag: "LayoutUpdate",
        layout: {
          activeWorkspaceIdx: 0n,
          workspaces: [
            {
              activeColumnIdx: 0n,
              columns: [
                {
                  activeTileIdx: 0n,
                  widthProportion: 1.0,
                  widthFixedPx: null,
                  tiles: [{ paneId: 1n, weight: 1.0 }],
                },
              ],
            },
            {
              activeColumnIdx: 0n,
              columns: [
                {
                  activeTileIdx: 0n,
                  widthProportion: 1.0,
                  widthFixedPx: null,
                  tiles: [{ paneId: 2n, weight: 1.0 }],
                },
              ],
            },
          ],
        },
      },
    });
    const tabs = document.querySelectorAll<HTMLElement>(".ciri-ws");
    tabs[1]!.click();
    const switches = sent.filter((m) => m.tag === "SwitchWorkspace");
    expect(switches).toEqual([{ tag: "SwitchWorkspace", workspaceIdx: 1n }]);
  });

  test("wheel without Ctrl scrolls the targeted pane's scrollback", () => {
    const { app, root, fire } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(3n) },
    });
    // FULL_SYNC_2X1 has 1 row of scrollback.
    fire({
      kind: "full-pane-sync",
      payload: hexToBytes(FULL_SYNC_2X1.hex),
    });
    const tile = document.querySelector<HTMLElement>("[data-pane-id='3']")!;
    const pane = tile.querySelector(".ciri-pane")!;
    // Before wheel: bottom row is the live viewport.
    expect(pane.querySelector(".ciri-row")!.textContent).toBe("hi");
    // Wheel up (negative deltaY).
    const wheelEvt = new WheelEvent("wheel", {
      deltaY: -100,
      deltaMode: 0, // DOM_DELTA_PIXEL
      bubbles: true,
      cancelable: true,
    });
    tile.dispatchEvent(wheelEvt);
    // After scrolling, the visible row should be the scrollback "ok".
    expect(pane.querySelector(".ciri-row")!.textContent).toBe("ok");
    expect(wheelEvt.defaultPrevented).toBe(true);
    // Avoid unused-var lint when only checking dispatch.
    void root;
  });

  test("wheel with Ctrl is left alone (browser zoom)", () => {
    const { app, fire } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(3n) },
    });
    fire({
      kind: "full-pane-sync",
      payload: hexToBytes(FULL_SYNC_2X1.hex),
    });
    const tile = document.querySelector<HTMLElement>("[data-pane-id='3']")!;
    const evt = new WheelEvent("wheel", {
      deltaY: -100,
      deltaMode: 0,
      ctrlKey: true,
      bubbles: true,
      cancelable: true,
    });
    tile.dispatchEvent(evt);
    expect(evt.defaultPrevented).toBe(false);
  });
});

describe("CiriApp — title sync", () => {
  test("TitleChanged updates grid.title, tile data-title, and document.title (when active)", () => {
    const { app, fire } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    fire({
      kind: "server-msg",
      msg: { tag: "TitleChanged", paneId: 1n, title: "vim ~/notes.md" },
    });
    expect(app.paneGrid(1n)!.title).toBe("vim ~/notes.md");
    const tile = document.querySelector<HTMLElement>("[data-pane-id='1']")!;
    expect(tile.dataset["title"]).toBe("vim ~/notes.md");
    expect(document.title).toBe("vim ~/notes.md");
  });

  test("TitleChanged for an inactive pane updates grid.title but NOT document.title", () => {
    const { app, fire } = bootstrap();
    app.start();
    // pane 1 active, pane 2 inactive
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutTwo(1n, 2n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_HYPER.hex) });
    document.title = "before";
    fire({
      kind: "server-msg",
      msg: { tag: "TitleChanged", paneId: 2n, title: "htop" },
    });
    expect(app.paneGrid(2n)!.title).toBe("htop");
    expect(document.title).toBe("before"); // unchanged
    const tile2 = document.querySelector<HTMLElement>("[data-pane-id='2']")!;
    expect(tile2.dataset["title"]).toBe("htop");
  });

  test("LayoutUpdate switches document.title to the new active pane's title", () => {
    const { app, fire } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutTwo(1n, 2n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_HYPER.hex) });
    fire({
      kind: "server-msg",
      msg: { tag: "TitleChanged", paneId: 1n, title: "alpha" },
    });
    fire({
      kind: "server-msg",
      msg: { tag: "TitleChanged", paneId: 2n, title: "beta" },
    });
    expect(document.title).toBe("alpha"); // 1 active
    // Switch active to pane 2.
    fire({
      kind: "server-msg",
      msg: {
        tag: "LayoutUpdate",
        layout: {
          activeWorkspaceIdx: 0n,
          workspaces: [
            {
              activeColumnIdx: 1n,
              columns: [
                {
                  activeTileIdx: 0n,
                  widthProportion: 0.5,
                  widthFixedPx: null,
                  tiles: [{ paneId: 1n, weight: 1.0 }],
                },
                {
                  activeTileIdx: 0n,
                  widthProportion: 0.5,
                  widthFixedPx: null,
                  tiles: [{ paneId: 2n, weight: 1.0 }],
                },
              ],
            },
          ],
        },
      },
    });
    expect(document.title).toBe("beta");
  });

  test("FullPaneSync's embedded title flows through to data-title on next layout", () => {
    const { app, fire } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(2n) },
    });
    // FULL_SYNC_HYPER (paneId=2) carries title="shell" per the fixture.
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_HYPER.hex) });
    // The layout was set BEFORE the grid existed, so the tile's
    // data-title was empty at that point. A subsequent LayoutUpdate
    // (typical when server re-publishes) picks up the title.
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(2n) },
    });
    const tile = document.querySelector<HTMLElement>("[data-pane-id='2']")!;
    expect(tile.dataset["title"]).toBe("shell");
    expect(document.title).toBe("shell");
  });
});

describe("CiriApp — bell", () => {
  test("Bell adds .ciri-tile-bell to the pane's tile and clears it after a timeout", async () => {
    vi.useFakeTimers();
    try {
      const { app, fire } = bootstrap();
      app.start();
      fire({
        kind: "server-msg",
        msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
      });
      fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
      const tile = document.querySelector<HTMLElement>("[data-pane-id='1']")!;
      expect(tile.classList.contains("ciri-tile-bell")).toBe(false);
      fire({ kind: "server-msg", msg: { tag: "Bell", paneId: 1n } });
      expect(tile.classList.contains("ciri-tile-bell")).toBe(true);
      // Run the flash window's clear timer.
      vi.advanceTimersByTime(1000);
      expect(tile.classList.contains("ciri-tile-bell")).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  test("repeat Bell on the same pane keeps the class through the burst", () => {
    vi.useFakeTimers();
    try {
      const { app, fire } = bootstrap();
      app.start();
      fire({
        kind: "server-msg",
        msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
      });
      fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
      const tile = document.querySelector<HTMLElement>("[data-pane-id='1']")!;
      fire({ kind: "server-msg", msg: { tag: "Bell", paneId: 1n } });
      vi.advanceTimersByTime(500); // halfway through first timer
      fire({ kind: "server-msg", msg: { tag: "Bell", paneId: 1n } }); // re-arm
      vi.advanceTimersByTime(500); // would have fired the FIRST timer here
      expect(tile.classList.contains("ciri-tile-bell")).toBe(true);
      vi.advanceTimersByTime(500); // now 1s after the re-arm
      expect(tile.classList.contains("ciri-tile-bell")).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  test("Bell for a pane outside the active workspace is a no-op (no slot to flash)", () => {
    const { app, fire } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    // pane 99 isn't in the layout; Bell shouldn't throw or affect
    // any tile.
    expect(() =>
      fire({ kind: "server-msg", msg: { tag: "Bell", paneId: 99n } }),
    ).not.toThrow();
    expect(document.querySelector(".ciri-tile-bell")).toBeNull();
  });

  test("destroy() cancels pending bell-flash timers without erroring", () => {
    vi.useFakeTimers();
    try {
      const { app, fire } = bootstrap();
      app.start();
      fire({
        kind: "server-msg",
        msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
      });
      fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
      fire({ kind: "server-msg", msg: { tag: "Bell", paneId: 1n } });
      app.destroy();
      // The timer would have run a callback against a now-detached
      // tile DOM node. Make sure no exception escapes the timer.
      expect(() => vi.advanceTimersByTime(2000)).not.toThrow();
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("CiriApp — selection + clipboard", () => {
  /// Mock getBoundingClientRect for cell-grid hit-testing. We pretend
  /// every `.ciri-pane` wrapper is anchored at (0,0) with a generous
  /// 800×600 size so the renderer's pixel math becomes deterministic.
  /// cellSize defaults to (8.5, 16.8) from the measure fallback —
  /// stub the renderer container rect; CiriApp's `measuredCellSize`
  /// stays at that fallback because the measure probe also lands on
  /// jsdom's zero-size body.
  function stubContainerRect(): void {
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(
      function (this: Element): DOMRect {
        if (this.classList?.contains("ciri-pane")) {
          return {
            x: 0, y: 0, top: 0, left: 0, right: 800, bottom: 600,
            width: 800, height: 600, toJSON: () => ({}),
          } as DOMRect;
        }
        return {
          x: 0, y: 0, top: 0, left: 0, right: 0, bottom: 0,
          width: 0, height: 0, toJSON: () => ({}),
        } as DOMRect;
      },
    );
  }

  afterEach(() => {
    vi.restoreAllMocks();
  });

  test("mousedown + drag inside one pane creates a selection range", () => {
    stubContainerRect();
    const { app, fire } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    const tile = document.querySelector<HTMLElement>("[data-pane-id='1']")!;
    // Mousedown at pixel (10, 10).
    tile.dispatchEvent(
      new MouseEvent("mousedown", { bubbles: true, clientX: 10, clientY: 10 }),
    );
    // Drag to (50, 30).
    window.dispatchEvent(
      new MouseEvent("mousemove", { clientX: 50, clientY: 30 }),
    );
    window.dispatchEvent(
      new MouseEvent("mouseup", { clientX: 50, clientY: 30 }),
    );
    const renderer = app.paneGrid(1n);
    expect(renderer).toBeDefined();
    // Selection rects appeared in the DOM during the drag.
    // Sanity: the pane has a renderer with a non-null selection that
    // is finalized (active=false).
    const rects = document.querySelectorAll(".ciri-selection-row");
    expect(rects.length).toBeGreaterThan(0);
  });

  test("mousedown + mouseup without movement clears any selection", () => {
    stubContainerRect();
    const { fire } = bootstrap();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    const tile = document.querySelector<HTMLElement>("[data-pane-id='1']")!;
    tile.dispatchEvent(
      new MouseEvent("mousedown", { bubbles: true, clientX: 10, clientY: 10 }),
    );
    // Same coords on mouseup — no movement registered.
    window.dispatchEvent(
      new MouseEvent("mouseup", { clientX: 10, clientY: 10 }),
    );
    expect(document.querySelectorAll(".ciri-selection-row").length).toBe(0);
  });

  test("drag is bounded by grid.cols / grid.rows; mouse outside clamps to edges", () => {
    stubContainerRect();
    const { fire } = bootstrap();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    // 3x2 grid. With cellSize fallback ~ (8.5, 16.8), pixel (10, 10)
    // lands inside col 1, displayRow 0. (1000, 1000) clamps to
    // col=2, displayRow=1.
    const tile = document.querySelector<HTMLElement>("[data-pane-id='1']")!;
    tile.dispatchEvent(
      new MouseEvent("mousedown", { bubbles: true, clientX: 10, clientY: 10 }),
    );
    window.dispatchEvent(
      new MouseEvent("mousemove", { clientX: 1000, clientY: 1000 }),
    );
    window.dispatchEvent(
      new MouseEvent("mouseup", { clientX: 1000, clientY: 1000 }),
    );
    // Two rows fully selected → two row rects.
    const rects = document.querySelectorAll(".ciri-selection-row");
    expect(rects.length).toBe(2);
  });

  function installClipboardMock(): {
    written: string[];
    setReadValue: (s: string) => void;
  } {
    const written: string[] = [];
    let readValue = "";
    const clipboard = {
      writeText: vi.fn(async (s: string) => {
        written.push(s);
      }),
      readText: vi.fn(async () => readValue),
    };
    Object.defineProperty(window.navigator, "clipboard", {
      value: clipboard,
      configurable: true,
    });
    return {
      written,
      setReadValue(s: string) {
        readValue = s;
      },
    };
  }

  test("Ctrl+Shift+C copies the selection's extracted text to the clipboard", async () => {
    stubContainerRect();
    const cb = installClipboardMock();
    const { app, fire } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    // FULL_SYNC_3X2 has 3x2 of empty cells — not very interesting to
    // copy, but the test just needs to verify wiring. Drive the
    // selection directly via the renderer to keep the test scope
    // tight to the chord handler.
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_HYPER.hex) });
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(2n) },
    });
    // FULL_SYNC_HYPER (paneId=2) cells = "a😀bc". Select [0..3].
    // Selection state — use the renderer setter directly since the
    // mouse-drag path is covered above.
    const grid = app.paneGrid(2n)!;
    // Look up renderer via the layout slot's child.
    const slot = document.querySelector("[data-pane-id='2']")!;
    const pane = slot.querySelector(".ciri-pane") as HTMLElement | null;
    expect(pane).not.toBeNull();
    // The renderer's `setSelection` is exposed via paneRenderer
    // lookup; the simplest path is to dispatch a click-drag to set
    // it via the mouse path. Use simulated coords for a 4-col row.
    const tile = slot as HTMLElement;
    tile.dispatchEvent(
      new MouseEvent("mousedown", { bubbles: true, clientX: 0, clientY: 0 }),
    );
    // cellWidth fallback ≈ 8.5; col 3 ~ pixel 25.5. Use 100 to be
    // safely past col 3 (would clamp to cols-1 = 3).
    window.dispatchEvent(
      new MouseEvent("mousemove", { clientX: 100, clientY: 0 }),
    );
    window.dispatchEvent(
      new MouseEvent("mouseup", { clientX: 100, clientY: 0 }),
    );
    // Now Ctrl+Shift+C on the root.
    const root = document.querySelector(".ciri-app")!.parentElement as HTMLElement;
    root.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "C",
        ctrlKey: true,
        shiftKey: true,
        bubbles: true,
      }),
    );
    // The Promise chain inside copySelectionToClipboard resolves on
    // the next microtask.
    await Promise.resolve();
    expect(cb.written.length).toBe(1);
    // Text comes from extractText on the full first row "a😀bc"; the
    // wide-char spacer collapses to "a😀bc" (4 cells but 3 graphemes).
    expect(cb.written[0]).toContain("a");
    expect(cb.written[0]).toContain("bc");
    // Touch the unused locals to satisfy strict TS.
    void grid;
    void pane;
  });

  test("Cmd+C (metaKey) on macOS-style keybinds also triggers copy", async () => {
    stubContainerRect();
    const cb = installClipboardMock();
    const { fire } = bootstrap();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(2n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_HYPER.hex) });
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(2n) },
    });
    const tile = document.querySelector<HTMLElement>("[data-pane-id='2']")!;
    tile.dispatchEvent(
      new MouseEvent("mousedown", { bubbles: true, clientX: 0, clientY: 0 }),
    );
    window.dispatchEvent(
      new MouseEvent("mousemove", { clientX: 100, clientY: 0 }),
    );
    window.dispatchEvent(
      new MouseEvent("mouseup", { clientX: 100, clientY: 0 }),
    );
    const root = document.querySelector(".ciri-app")!.parentElement as HTMLElement;
    root.dispatchEvent(
      new KeyboardEvent("keydown", { key: "c", metaKey: true, bubbles: true }),
    );
    await Promise.resolve();
    expect(cb.written.length).toBe(1);
  });

  test("Ctrl+Shift+C with no selection is a no-op (browser default not preempted)", async () => {
    const cb = installClipboardMock();
    const { fire } = bootstrap();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    const root = document.querySelector(".ciri-app")!.parentElement as HTMLElement;
    root.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "C",
        ctrlKey: true,
        shiftKey: true,
        bubbles: true,
      }),
    );
    await Promise.resolve();
    expect(cb.written.length).toBe(0);
  });

  test("Ctrl+Shift+V reads the clipboard and sends Input to the active pane", async () => {
    const cb = installClipboardMock();
    cb.setReadValue("hello");
    const { fire, inputs } = bootstrap();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    const root = document.querySelector(".ciri-app")!.parentElement as HTMLElement;
    root.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "V",
        ctrlKey: true,
        shiftKey: true,
        bubbles: true,
      }),
    );
    // Two microtasks: the .readText() Promise + the awaiting handler.
    await Promise.resolve();
    await Promise.resolve();
    expect(inputs.length).toBe(1);
    expect(inputs[0]!.paneId).toBe(1n);
    expect(new TextDecoder().decode(inputs[0]!.data)).toBe("hello");
  });

  test("Ctrl+Shift+V with MODE_BRACKETED_PASTE wraps the payload in ESC[200~/ESC[201~", async () => {
    const cb = installClipboardMock();
    cb.setReadValue("safe");
    const { app, fire, inputs } = bootstrap();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    // Bracketed-paste flag bit.
    const grid = app.paneGrid(1n)!;
    grid.meta = { ...grid.meta, modeFlags: 0x0010 /* MODE_BRACKETED_PASTE */ };
    const root = document.querySelector(".ciri-app")!.parentElement as HTMLElement;
    root.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "v",
        ctrlKey: true,
        shiftKey: true,
        bubbles: true,
      }),
    );
    await Promise.resolve();
    await Promise.resolve();
    expect(inputs.length).toBe(1);
    expect(new TextDecoder().decode(inputs[0]!.data)).toBe("\x1b[200~safe\x1b[201~");
  });

  test("destroy() detaches window mousemove/mouseup listeners", () => {
    stubContainerRect();
    const { app, fire } = bootstrap();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    const tile = document.querySelector<HTMLElement>("[data-pane-id='1']")!;
    tile.dispatchEvent(
      new MouseEvent("mousedown", { bubbles: true, clientX: 10, clientY: 10 }),
    );
    app.destroy();
    // After destroy, window-level move/up should be inert. Dispatch
    // them and verify nothing throws.
    expect(() => {
      window.dispatchEvent(new MouseEvent("mousemove", { clientX: 50, clientY: 30 }));
      window.dispatchEvent(new MouseEvent("mouseup", { clientX: 50, clientY: 30 }));
    }).not.toThrow();
  });
});

describe("CiriApp — error reporting", () => {
  test("invalid cell-delta bytes go to onError, no crash", () => {
    const { app, fire, onErrorCalls } = bootstrap();
    app.start();
    fire({ kind: "cell-delta", payload: new Uint8Array([0x00]) });
    expect(onErrorCalls.length).toBe(1);
    expect(onErrorCalls[0]!.constructor.name).toContain("DecodeError");
  });

  test("invalid full-pane-sync bytes go to onError, no crash", () => {
    const { app, fire, onErrorCalls } = bootstrap();
    app.start();
    fire({ kind: "full-pane-sync", payload: new Uint8Array([0x00]) });
    expect(onErrorCalls.length).toBe(1);
  });

  test("body decode error closes the client and poisons further frames (round-2)", () => {
    // Phase 2.6 round-2 codex fix: previously we just surfaced the
    // error and kept the WebSocket alive, so the next FullPaneSync /
    // CellDelta would be applied to a half-broken grid baseline.
    // Now any FrameBodyDecodeError closes the underlying client and
    // drops every subsequent frame from the same wire stream.
    const { app, fire, onErrorCalls, state } = bootstrap();
    app.start();
    expect(state.closed).toBe(false);
    fire({ kind: "cell-delta", payload: new Uint8Array([0x00]) });
    expect(onErrorCalls.length).toBe(1);
    expect(state.closed).toBe(true);
    // A well-formed FullPaneSync that arrives after the poison should
    // NOT be applied — the wire is no longer trusted.
    fire({
      kind: "full-pane-sync",
      payload: hexToBytes(FULL_SYNC_3X2.hex),
    });
    expect(app.hasGrid(1n)).toBe(false);
    // Same for a subsequent cell-delta (even valid hex would be
    // dropped without re-reporting the error).
    fire({ kind: "cell-delta", payload: new Uint8Array([0x00]) });
    expect(onErrorCalls.length).toBe(1);
  });

  test("close event surfaces via callback", () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const closes: { reason: string; reconnecting: boolean }[] = [];
    let captured: ((e: CiriEvent) => void) | null = null;
    const app = new CiriApp(root, {
      url: "ws://test",
      sessionName: "x",
      clientFactory: (_o, on) => {
        captured = on;
        return {
          start() {},
          close() {},
          send() {},
          sendInput() {
            return 1n;
          },
        };
      },
      onClose: (reason, reconnecting) => closes.push({ reason, reconnecting }),
    });
    app.start();
    captured!({ kind: "close", reason: "test", reconnecting: false });
    expect(closes).toEqual([{ reason: "test", reconnecting: false }]);
  });
});

describe("CiriApp — resize integration", () => {
  beforeEach(() => {
    // jsdom doesn't ship ResizeObserver. Provide a minimal stub that
    // fires synchronously on `observe` so we can drive the resize path.
    interface ROInst {
      cb: ResizeObserverCallback;
      observe: (el: Element) => void;
      disconnect: () => void;
    }
    (globalThis as unknown as { ResizeObserver: typeof ResizeObserver }).ResizeObserver =
      class StubResizeObserver implements ROInst {
        cb: ResizeObserverCallback;
        constructor(cb: ResizeObserverCallback) {
          this.cb = cb;
        }
        observe(_el: Element): void {
          // Fire one synchronous callback to mimic the initial entry
          // every real implementation emits.
          this.cb([] as unknown as ResizeObserverEntry[], this as unknown as ResizeObserver);
        }
        unobserve(): void {}
        disconnect(): void {}
      } as unknown as typeof ResizeObserver;
  });

  test("start() installs ResizeObserver and sends a Resize", () => {
    const { app, sent } = bootstrap();
    app.start();
    const resizes = sent.filter((m) => m.tag === "Resize");
    expect(resizes.length).toBeGreaterThanOrEqual(1);
    const r = resizes[0]!;
    expect(r.tag).toBe("Resize");
    if (r.tag === "Resize") {
      expect(r.cellWidth).toBeGreaterThan(0);
      expect(r.cellHeight).toBeGreaterThan(0);
    }
  });

  test("remeasureCells() sends a fresh Resize", () => {
    const { app, sent } = bootstrap();
    app.start();
    const before = sent.length;
    app.remeasureCells();
    const after = sent.filter((m) => m.tag === "Resize");
    expect(after.length).toBeGreaterThan(0);
    expect(sent.length).toBeGreaterThan(before);
  });

  test("Resize uses the workspace viewport rect, not the root (excludes tab strip)", () => {
    // Round-6 codex fix: if Resize measured the outer root, it would
    // over-count by the tab-strip height and the PTY would think it
    // has more rows than the actual cell grid does. Mock both rects
    // distinctly and check the Resize message reflects the viewport.
    const { app, sent } = bootstrap();
    // Stub getBoundingClientRect: root pretends to be 800×600,
    // viewport pretends to be 800×560 (the missing 40px is the tab
    // strip). The wrong implementation would send height=600.
    const root = app.layoutManager.viewportEl.closest(".ciri-app")
      ?.parentElement as HTMLElement;
    const viewport = app.layoutManager.viewportEl;
    // Both nodes share Element.prototype, so spy with conditional
    // returns keyed by element identity.
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(
      function (this: Element): DOMRect {
        if (this === root) {
          return { x: 0, y: 0, top: 0, left: 0, right: 800, bottom: 600, width: 800, height: 600, toJSON: () => ({}) } as DOMRect;
        }
        if (this === viewport) {
          return { x: 0, y: 40, top: 40, left: 0, right: 800, bottom: 600, width: 800, height: 560, toJSON: () => ({}) } as DOMRect;
        }
        return { x: 0, y: 0, top: 0, left: 0, right: 0, bottom: 0, width: 0, height: 0, toJSON: () => ({}) } as DOMRect;
      },
    );
    app.start();
    const resize = sent.find((m) => m.tag === "Resize");
    expect(resize).toBeDefined();
    if (resize?.tag === "Resize") {
      expect(resize.width).toBe(800);
      expect(resize.height).toBe(560); // viewport, not root's 600
    }
    vi.restoreAllMocks();
  });
});

describe("CiriApp — IME composition", () => {
  /// Build a CompositionEvent. jsdom supports the constructor but
  /// requires the `data` field via the init dict.
  function compositionEvent(
    type: "compositionstart" | "compositionupdate" | "compositionend",
    data: string,
  ): CompositionEvent {
    return new CompositionEvent(type, { data, bubbles: true });
  }

  test("constructor mounts a hidden composition sink textarea", () => {
    // Codex round-1: a focusable <div> alone isn't enough to engage
    // the browser's IME engine on Safari and most mobile browsers.
    // The textarea sink is the canonical fix (xterm.js, hyper, …):
    // off-screen + opacity=0 so it's invisible, tabIndex=-1 so it
    // never appears in the keyboard tab order.
    const { root } = bootstrap();
    const sink = root.querySelector<HTMLTextAreaElement>(
      "textarea.ciri-composition-sink",
    );
    expect(sink).not.toBeNull();
    expect(sink!.tabIndex).toBe(-1);
    expect(sink!.getAttribute("aria-hidden")).toBe("true");
    expect(sink!.spellcheck).toBe(false);
    expect(sink!.style.opacity).toBe("0");
  });

  test("compositionend clears the sink textarea contents", () => {
    // Some IMEs leave the composed glyph in the textarea after
    // commit; clearing it prevents unbounded growth over a long
    // session and keeps the next composition fresh.
    const { root, fire } = bootstrap();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    const sink = root.querySelector<HTMLTextAreaElement>(
      "textarea.ciri-composition-sink",
    )!;
    sink.value = "ni";
    root.dispatchEvent(compositionEvent("compositionstart", ""));
    root.dispatchEvent(compositionEvent("compositionupdate", "ni"));
    root.dispatchEvent(compositionEvent("compositionend", "你"));
    expect(sink.value).toBe("");
  });

  test("compositionstart + update show the preedit on the active pane", () => {
    const { root, app, fire, inputs } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    root.dispatchEvent(compositionEvent("compositionstart", ""));
    root.dispatchEvent(compositionEvent("compositionupdate", "ni"));
    const p = document.querySelector<HTMLElement>(".ciri-preedit")!;
    expect(p.style.display).toBe("block");
    expect(p.textContent).toBe("ni");
    // No bytes are written to the wire during composition.
    expect(inputs.length).toBe(0);
    void app;
  });

  test("compositionend sends the committed text and clears the overlay", () => {
    const { root, app, fire, inputs } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    root.dispatchEvent(compositionEvent("compositionstart", ""));
    root.dispatchEvent(compositionEvent("compositionupdate", "ni"));
    root.dispatchEvent(compositionEvent("compositionend", "你"));
    expect(inputs.length).toBe(1);
    expect(inputs[0]!.paneId).toBe(1n);
    // UTF-8 of "你" → e4 bd a0
    expect(Array.from(inputs[0]!.data)).toEqual([0xe4, 0xbd, 0xa0]);
    const p = document.querySelector<HTMLElement>(".ciri-preedit")!;
    expect(p.style.display).toBe("none");
    void app;
  });

  test("compositionend with empty data (canceled) sends nothing", () => {
    const { root, app, fire, inputs } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    root.dispatchEvent(compositionEvent("compositionstart", ""));
    root.dispatchEvent(compositionEvent("compositionupdate", "ni"));
    // User pressed Esc / clicked outside the candidate list → empty data.
    root.dispatchEvent(compositionEvent("compositionend", ""));
    expect(inputs.length).toBe(0);
    const p = document.querySelector<HTMLElement>(".ciri-preedit")!;
    expect(p.style.display).toBe("none");
    void app;
  });

  test("keydown during composition is suppressed (composing flag)", () => {
    const { root, app, fire, inputs } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    root.dispatchEvent(compositionEvent("compositionstart", ""));
    // Mid-composition keystroke: even without `isComposing` set the
    // app's own flag suppresses it.
    root.dispatchEvent(
      new KeyboardEvent("keydown", { key: "a", bubbles: true }),
    );
    // And explicitly with `isComposing` true.
    const e = new KeyboardEvent("keydown", { key: "Enter", bubbles: true });
    Object.defineProperty(e, "isComposing", { value: true });
    root.dispatchEvent(e);
    expect(inputs.length).toBe(0);
    // After composition ends the keystroke path resumes.
    root.dispatchEvent(compositionEvent("compositionend", ""));
    root.dispatchEvent(
      new KeyboardEvent("keydown", { key: "a", bubbles: true }),
    );
    expect(inputs.length).toBe(1);
    expect(Array.from(inputs[0]!.data)).toEqual([0x61]);
    void app;
  });

  test("compositionend resolves the commit target at commit time (Rust parity)", () => {
    // Rust's `Ime::Commit` path (crates/ciri/src/app/ime.rs:36-41)
    // reads `active_pane_id()` inside the commit branch, NOT a value
    // captured at composition start. Match that: if a server-driven
    // LayoutUpdate promotes a new active pane between compositionstart
    // and compositionend, the commit follows the new active pane.
    const { root, app, fire, inputs } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutTwo(1n, 2n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_HYPER.hex) });
    // Start composing with pane 1 active.
    root.dispatchEvent(compositionEvent("compositionstart", ""));
    root.dispatchEvent(compositionEvent("compositionupdate", "ni"));
    // Server promotes pane 2 to active (e.g. a leader-driven focus
    // change). The preedit overlay was visible on pane 1, but the
    // commit should now land on pane 2.
    fire({
      kind: "server-msg",
      msg: {
        tag: "LayoutUpdate",
        layout: {
          activeWorkspaceIdx: 0n,
          workspaces: [
            {
              activeColumnIdx: 1n,
              columns: [
                {
                  activeTileIdx: 0n,
                  widthProportion: 0.5,
                  widthFixedPx: null,
                  tiles: [{ paneId: 1n, weight: 1.0 }],
                },
                {
                  activeTileIdx: 0n,
                  widthProportion: 0.5,
                  widthFixedPx: null,
                  tiles: [{ paneId: 2n, weight: 1.0 }],
                },
              ],
            },
          ],
        },
      },
    });
    root.dispatchEvent(compositionEvent("compositionend", "你"));
    expect(inputs.length).toBe(1);
    expect(inputs[0]!.paneId).toBe(2n);
    void app;
  });

  test("compositionend drops bytes when the target pane has been closed", () => {
    // Pane closure mid-composition: the commit target resolved at
    // commit time (Rust parity) points to a pane the app no longer
    // tracks. Skip the send rather than crashing or sending to a
    // ghost pane.
    const { root, app, fire, inputs } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    root.dispatchEvent(compositionEvent("compositionstart", ""));
    root.dispatchEvent(compositionEvent("compositionupdate", "ni"));
    // Server kills the pane while the IME is still composing.
    fire({ kind: "server-msg", msg: { tag: "PaneClosed", paneId: 1n } });
    root.dispatchEvent(compositionEvent("compositionend", "你"));
    expect(inputs.length).toBe(0);
    void app;
  });

  test("commit falls back to beforeinput-tracked data when compositionend.data is empty (Safari quirk)", () => {
    // Some browsers (notably Safari + a handful of mobile IMEs)
    // surface the committed glyph only through the textarea's
    // `beforeinput` event with `inputType=insertFromComposition`
    // (or a post-composition `insertText`) and leave
    // `compositionend.data` empty. Treating empty data uniformly
    // as "canceled" would silently drop those commits.
    const { root, fire, inputs } = bootstrap();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    const sink = root.querySelector<HTMLTextAreaElement>(
      "textarea.ciri-composition-sink",
    )!;
    root.dispatchEvent(compositionEvent("compositionstart", ""));
    root.dispatchEvent(compositionEvent("compositionupdate", "ni"));
    // Simulate Safari's "commit via insertFromComposition" path.
    const beforeInput = new InputEvent("beforeinput", {
      inputType: "insertFromComposition",
      data: "你",
      bubbles: true,
    });
    sink.dispatchEvent(beforeInput);
    // compositionend arrives with empty data — the fallback kicks in.
    root.dispatchEvent(compositionEvent("compositionend", ""));
    expect(inputs.length).toBe(1);
    expect(Array.from(inputs[0]!.data)).toEqual([0xe4, 0xbd, 0xa0]);
    expect(sink.value).toBe("");
  });

  test("cancel with dirty sink does NOT commit (filtered inputType)", () => {
    // Round-3 codex: blindly using sink.value as a fallback would
    // turn a cancel-with-dirty-sink into an erroneous commit. The
    // beforeinput path filters intermediate `insertCompositionText`
    // (which fires on every candidate-list keystroke) and only
    // honors `insertFromComposition` / post-composition `insertText`
    // — so this Esc-style cancel path stays empty.
    const { root, fire, inputs } = bootstrap();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    const sink = root.querySelector<HTMLTextAreaElement>(
      "textarea.ciri-composition-sink",
    )!;
    root.dispatchEvent(compositionEvent("compositionstart", ""));
    // Intermediate composition step: fires beforeinput with the
    // *preedit* text, NOT a commit. Must be ignored by the commit
    // tracker.
    sink.dispatchEvent(
      new InputEvent("beforeinput", {
        inputType: "insertCompositionText",
        data: "ni",
        bubbles: true,
      }),
    );
    sink.value = "ni"; // dirty sink, as some IMEs leave it after cancel
    root.dispatchEvent(compositionEvent("compositionend", ""));
    expect(inputs.length).toBe(0);
    // Sink still cleared on compositionend.
    expect(sink.value).toBe("");
  });

  test("preedit overlay follows the active pane when LayoutUpdate changes focus mid-composition", () => {
    // Codex round 3 P3: the overlay was previously pinned to the
    // pane captured at compositionstart while commit routing
    // resolved at commit time — a server-driven mid-composition
    // promotion could show the preedit on pane A and commit to
    // pane B. The overlay now tracks `activePaneId()` on every
    // update so the visible glyph and the eventual commit
    // destination stay consistent.
    const { root, fire } = bootstrap();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutTwo(1n, 2n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_HYPER.hex) });
    root.dispatchEvent(compositionEvent("compositionstart", ""));
    root.dispatchEvent(compositionEvent("compositionupdate", "ni"));
    const tile1 = document.querySelector<HTMLElement>("[data-pane-id='1']")!;
    // Preedit lands on the initially active pane (pane 1).
    const p1Before = tile1.querySelector<HTMLElement>(".ciri-preedit")!;
    expect(p1Before.textContent).toBe("ni");
    expect(p1Before.style.display).toBe("block");
    // Server promotes pane 2 to active.
    fire({
      kind: "server-msg",
      msg: {
        tag: "LayoutUpdate",
        layout: {
          activeWorkspaceIdx: 0n,
          workspaces: [
            {
              activeColumnIdx: 1n,
              columns: [
                {
                  activeTileIdx: 0n,
                  widthProportion: 0.5,
                  widthFixedPx: null,
                  tiles: [{ paneId: 1n, weight: 1.0 }],
                },
                {
                  activeTileIdx: 0n,
                  widthProportion: 0.5,
                  widthFixedPx: null,
                  tiles: [{ paneId: 2n, weight: 1.0 }],
                },
              ],
            },
          ],
        },
      },
    });
    // Next compositionupdate transfers the overlay.
    root.dispatchEvent(compositionEvent("compositionupdate", "ni"));
    // applyLayout may have rebuilt the tile DOM nodes, so re-query.
    const tile2After = document.querySelector<HTMLElement>(
      "[data-pane-id='2']",
    )!;
    const p2 = tile2After.querySelector<HTMLElement>(".ciri-preedit")!;
    expect(p2.textContent).toBe("ni");
    expect(p2.style.display).toBe("block");
    const tile1After = document.querySelector<HTMLElement>(
      "[data-pane-id='1']",
    )!;
    const p1After = tile1After.querySelector<HTMLElement>(".ciri-preedit")!;
    expect(p1After.style.display).toBe("none");
  });

  test("compositionstart anchors the sink near the cursor (IME candidate window positioning)", () => {
    // Round-3 codex P2: the OS-level IME candidate window anchors
    // to the editable's caret. With the sink pinned at (0, 0) the
    // popup would appear at the page corner. Mirror's
    // `set_ime_cursor_area` in the native client.
    const { root, fire } = bootstrap();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    // Stub getBoundingClientRect so jsdom gives us a non-zero rect
    // and the reposition path takes effect.
    const tile = document.querySelector<HTMLElement>("[data-pane-id='1']")!;
    const pane = tile.querySelector<HTMLElement>(".ciri-pane")!;
    vi.spyOn(pane, "getBoundingClientRect").mockReturnValue({
      x: 100,
      y: 200,
      left: 100,
      top: 200,
      right: 500,
      bottom: 400,
      width: 400,
      height: 200,
      toJSON: () => ({}),
    } as DOMRect);
    root.dispatchEvent(compositionEvent("compositionstart", ""));
    const sink = root.querySelector<HTMLTextAreaElement>(
      "textarea.ciri-composition-sink",
    )!;
    expect(sink.style.position).toBe("fixed");
    // cursorLine/cursorCol both = 0 in the fixture, so the anchor
    // is just at the pane's top-left.
    expect(sink.style.left).toBe("100px");
    expect(sink.style.top).toBe("200px");
    vi.restoreAllMocks();
  });

  test("destroy() removes the composition sink from the root", () => {
    // Round-2 codex: the sink lives inside the user-owned root, so
    // `destroy()` must remove it explicitly — otherwise repeated
    // mount/destroy cycles on the same root leave a stack of
    // hidden textareas.
    const { app, root } = bootstrap();
    expect(
      root.querySelector("textarea.ciri-composition-sink"),
    ).not.toBeNull();
    app.destroy();
    expect(root.querySelector("textarea.ciri-composition-sink")).toBeNull();
  });

  test("destroy() unregisters composition handlers", () => {
    const { root, app, fire, inputs } = bootstrap();
    app.start();
    fire({
      kind: "server-msg",
      msg: { tag: "LayoutUpdate", layout: mkLayoutSingle(1n) },
    });
    fire({ kind: "full-pane-sync", payload: hexToBytes(FULL_SYNC_3X2.hex) });
    app.destroy();
    // Post-destroy composition events must not push bytes through the
    // (now-closed) client. Mirror of the destroy-listener teardown
    // round-3 codex fix for keydown/wheel/mousedown.
    root.dispatchEvent(compositionEvent("compositionstart", ""));
    root.dispatchEvent(compositionEvent("compositionupdate", "ni"));
    root.dispatchEvent(compositionEvent("compositionend", "你"));
    expect(inputs.length).toBe(0);
  });
});
