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
