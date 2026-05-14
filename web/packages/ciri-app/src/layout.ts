// LayoutState → DOM chrome.
//
// The server is the source of truth for workspace / column / tile
// layout. Every `StateSync` or `LayoutUpdate` carries the full
// `LayoutState`; we mirror it into a flex tree:
//
//   <div class="ciri-app">
//     <div class="ciri-workspaces">                  ← tab strip
//       <button class="ciri-ws[-active]" data-idx>...</button>
//     </div>
//     <div class="ciri-workspace">                   ← active workspace content
//       <div class="ciri-column[-active]" data-idx style="flex: <prop>">
//         <div class="ciri-tile[-active]" data-pane-id style="flex: <weight>">
//           ← pane renderer container lives here
//         </div>
//         ...
//       </div>
//       ...
//     </div>
//   </div>
//
// LayoutManager owns the chrome; it knows nothing about cell rendering
// or input. The caller (CiriApp) reads `getSlot(paneId)` to discover
// where to mount each pane renderer's container, and listens to
// `onPaneClick` / `onWorkspaceClick` for input routing.
//
// Diffing: v1 rebuilds the entire active workspace's column/tile DOM
// on every `setLayout()` call. The workspaces' panes don't change
// shape often enough to make that costly, and the per-pane renderers
// (which DO get re-attached, not re-created) carry their own internal
// dirty-row diff. The cost is one `replaceChildren()` plus a handful
// of new <div>s — well under one frame.

import type { LayoutState } from "@ciri/client";

export interface LayoutManagerOptions {
  /// Fired when the user clicks anywhere inside a pane's tile slot —
  /// typically wired to `client.send({ tag: "FocusPane", paneId })`.
  onPaneClick?: (paneId: bigint) => void;
  /// Fired when the user clicks a workspace tab — typically wired to
  /// `client.send({ tag: "SwitchWorkspace", workspaceIdx })`.
  onWorkspaceClick?: (idx: bigint) => void;
  /// Optional resolver that returns the current title for a pane,
  /// used to populate the tile's `data-title` attribute on layout
  /// reshape. Defaults to empty. Themes can read this attribute via
  /// `[data-title]::before` to render a title bar without the
  /// renderer or layout owning the chrome.
  paneTitleFor?: (paneId: bigint) => string;
}

export class LayoutManager {
  private readonly doc: Document;
  private readonly chrome: HTMLElement;
  private readonly wsTabs: HTMLElement;
  private readonly wsContent: HTMLElement;
  /// `paneId.toString()` → tile slot DOM node, populated by
  /// `setLayout`. We key by string because `bigint` is not a valid
  /// `Map` key when used across realms (it works in-realm, but the
  /// stringified form survives a `structuredClone` / a worker
  /// boundary that we may want later).
  private slots = new Map<string, HTMLElement>();
  private destroyed = false;

  constructor(
    private readonly root: HTMLElement,
    private readonly opts: LayoutManagerOptions = {},
  ) {
    const doc = root.ownerDocument;
    if (doc === null) {
      throw new Error(
        "LayoutManager: root element is not attached to a document",
      );
    }
    this.doc = doc;
    this.chrome = doc.createElement("div");
    this.chrome.className = "ciri-app";
    this.wsTabs = doc.createElement("div");
    this.wsTabs.className = "ciri-workspaces";
    this.wsContent = doc.createElement("div");
    this.wsContent.className = "ciri-workspace";
    this.chrome.appendChild(this.wsTabs);
    this.chrome.appendChild(this.wsContent);
    this.root.appendChild(this.chrome);
  }

  /// Reconcile DOM chrome to match `layout`. Existing tile slots are
  /// replaced wholesale — the caller is responsible for moving each
  /// pane renderer's container into its new slot via `getSlot()`.
  setLayout(layout: LayoutState): void {
    if (this.destroyed) {
      throw new Error("LayoutManager: setLayout called after destroy");
    }
    const activeWsIdx = Number(layout.activeWorkspaceIdx);

    // Workspace tabs.
    const tabs: Node[] = new Array(layout.workspaces.length);
    for (let i = 0; i < layout.workspaces.length; i += 1) {
      const btn = this.doc.createElement("button");
      btn.className =
        i === activeWsIdx ? "ciri-ws ciri-ws-active" : "ciri-ws";
      btn.textContent = String(i + 1);
      btn.dataset["idx"] = String(i);
      const idxBig = BigInt(i);
      btn.addEventListener("click", () => {
        // Defer the callback through opts in case the consumer
        // refreshes its handler between layouts.
        this.opts.onWorkspaceClick?.(idxBig);
      });
      tabs[i] = btn;
    }
    this.wsTabs.replaceChildren(...tabs);

    // Active workspace content. Empty layout (no workspaces) leaves
    // wsContent blank.
    this.slots = new Map();
    const ws = layout.workspaces[activeWsIdx];
    if (ws === undefined) {
      this.wsContent.replaceChildren();
      return;
    }
    const activeColIdx = Number(ws.activeColumnIdx);
    const columns: Node[] = new Array(ws.columns.length);
    for (let ci = 0; ci < ws.columns.length; ci += 1) {
      const col = ws.columns[ci]!;
      const colEl = this.doc.createElement("div");
      colEl.className =
        ci === activeColIdx ? "ciri-column ciri-column-active" : "ciri-column";
      // CSS flex-grow proportional to the server's widthProportion.
      // `flex: <n>` is equivalent to `flex: <n> 1 0%` — the browser
      // distributes free space in the same shape the server's layout
      // crate did. We do NOT honor widthFixedPx in v1 because resize
      // observers will renegotiate anyway.
      colEl.style.flex = String(col.widthProportion);
      colEl.style.display = "flex";
      colEl.style.flexDirection = "column";
      colEl.dataset["columnIdx"] = String(ci);

      const activeTileIdx = Number(col.activeTileIdx);
      const tileEls: Node[] = new Array(col.tiles.length);
      for (let ti = 0; ti < col.tiles.length; ti += 1) {
        const tile = col.tiles[ti]!;
        const tileEl = this.doc.createElement("div");
        const isActive = ci === activeColIdx && ti === activeTileIdx;
        tileEl.className = isActive ? "ciri-tile ciri-tile-active" : "ciri-tile";
        tileEl.style.flex = String(tile.weight);
        tileEl.style.minHeight = "0"; // let flexbox shrink children below content
        tileEl.style.overflow = "hidden";
        const paneIdStr = tile.paneId.toString();
        tileEl.dataset["paneId"] = paneIdStr;
        // Surface the latest known title for theme-level rendering;
        // empty string when no resolver was provided or the pane has
        // no title yet.
        const title = this.opts.paneTitleFor?.(tile.paneId) ?? "";
        tileEl.dataset["title"] = title;
        const paneIdBig = tile.paneId;
        // `mousedown` (not `click`) so focus moves before the click's
        // default text-selection start; matches how native terminals
        // give the focused pane the selection cursor immediately.
        tileEl.addEventListener("mousedown", () => {
          this.opts.onPaneClick?.(paneIdBig);
        });
        tileEls[ti] = tileEl;
        this.slots.set(paneIdStr, tileEl);
      }
      colEl.replaceChildren(...tileEls);
      columns[ci] = colEl;
    }
    this.wsContent.replaceChildren(...columns);
  }

  /// Lookup the tile slot DOM node for `paneId`, or `null` if the
  /// pane is not present in the currently-active workspace. The
  /// caller mounts a renderer container into this slot.
  getSlot(paneId: bigint): HTMLElement | null {
    return this.slots.get(paneId.toString()) ?? null;
  }

  /// Update a tile's `data-title` attribute in-place without
  /// rebuilding the chrome. Used by `TitleChanged` so the title bar
  /// (rendered by a theme via `[data-title]::before`) refreshes
  /// without a layout reshape. No-op when the pane isn't in the
  /// active workspace.
  setTileTitle(paneId: bigint, title: string): void {
    const slot = this.slots.get(paneId.toString());
    if (slot === undefined) return;
    slot.dataset["title"] = title;
  }

  /// Pane IDs currently rendered in the chrome (i.e. live in the
  /// active workspace). Useful to know which panes' renderers should
  /// stay attached vs. detached for inactive workspaces.
  paneIdsInLayout(): bigint[] {
    return Array.from(this.slots.keys(), (s) => BigInt(s));
  }

  /// Pane ID at `(activeWorkspaceIdx, activeColumnIdx, activeTileIdx)`
  /// for `layout`, or `null` if the position doesn't resolve to a
  /// tile (empty workspace, out-of-range indices, etc.).
  static activePaneId(layout: LayoutState): bigint | null {
    const ws = layout.workspaces[Number(layout.activeWorkspaceIdx)];
    if (ws === undefined) return null;
    const col = ws.columns[Number(ws.activeColumnIdx)];
    if (col === undefined) return null;
    const tile = col.tiles[Number(col.activeTileIdx)];
    return tile === undefined ? null : tile.paneId;
  }

  /// Read-only handle to the active-workspace content node. Useful
  /// for the app layer to attach a single ResizeObserver to the
  /// rendered viewport rather than per-tile.
  get viewportEl(): HTMLElement {
    return this.wsContent;
  }

  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;
    this.chrome.remove();
    this.slots.clear();
  }
}
