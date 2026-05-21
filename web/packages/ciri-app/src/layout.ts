// LayoutState → DOM chrome.
//
// The server is the source of truth for workspace / column / tile
// layout. Every `StateSync` or `LayoutUpdate` carries the full
// `LayoutState`; we mirror it into a flex tree:
//
//   <div class="ciri-app">
//     <div class="ciri-workspaces">                  ← workspace tab strip
//       <button class="ciri-ws[-active]" data-idx>...</button>
//     </div>
//     <nav class="ciri-panes" role="toolbar" aria-label="Switch active pane">
//       <button class="ciri-pane-chip"             ← pane switcher chips
//               type="button"
//               data-chip-pane-id
//               aria-current="true|false">title</button>
//       ...
//     </nav>
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
// The pane switcher (`.ciri-panes`) is a primary mobile / narrow-
// viewport navigation hook: when columns shrink to unusable widths,
// chips remain tappable. ARIA roles follow the toolbar pattern — a
// flat row of related command buttons that each set the active
// pane via the same `onPaneClick` callback the tile uses, with the
// active chip carrying `aria-current="true"`. (Strict `role="tab"` +
// `aria-selected` was considered but doesn't fit — that pattern
// implies showing one tabpanel at a time, while our column/tile
// layout keeps every pane visible.)
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

/// One button in the pane-bar's action group. `id` is opaque to
/// LayoutManager — it just routes back through `onAction`. `label`
/// is the `aria-label` (announced by screen readers) and `icon` is
/// the visible glyph (a single Unicode character is fine; consumers
/// who want SVG can wrap the call site).
export interface PaneAction {
  id: string;
  label: string;
  icon: string;
}

export interface LayoutManagerOptions {
  /// Fired when the user clicks anywhere inside a pane's tile slot —
  /// typically wired to `client.send({ tag: "FocusPane", paneId })`.
  onPaneClick?: (paneId: bigint) => void;
  /// Fired when the user clicks a chip's per-pane close (×) button —
  /// typically wired to `client.send({ tag: "ClosePane", paneId })`.
  /// Separate callback (not piggybacked on `onPaneClick`) so the
  /// caller can distinguish "switch to this pane" from "kill this
  /// pane" without parsing event targets.
  onPaneClose?: (paneId: bigint) => void;
  /// Fired when the user clicks a workspace tab — typically wired to
  /// `client.send({ tag: "SwitchWorkspace", workspaceIdx })`.
  onWorkspaceClick?: (idx: bigint) => void;
  /// Optional resolver that returns the current title for a pane,
  /// used to populate the tile's `data-title` attribute on layout
  /// reshape. Defaults to empty. Themes can read this attribute via
  /// `[data-title]::before` to render a title bar without the
  /// renderer or layout owning the chrome.
  paneTitleFor?: (paneId: bigint) => string;
  /// Optional list of quick-action buttons rendered after the pane
  /// chips. Primarily a touch / mobile affordance for operations the
  /// keyboard usually handles (new pane, focus directional, …). An
  /// empty / omitted list renders no action group, no divider.
  actions?: readonly PaneAction[];
  /// Fired when the user clicks one of `actions`. The id is the same
  /// value the caller supplied. LayoutManager doesn't know what each
  /// action means — the wiring lives in the consumer.
  onAction?: (id: string) => void;
  /// Fired when the user picks a different session from the session
  /// dropdown — typically wired to
  /// `client.send({ tag: "SwitchSession", sessionName })`. The session
  /// list itself is pushed in via `setSessions`; this only fires on a
  /// user-driven change to a name other than the current one.
  onSessionSelect?: (sessionName: string) => void;
}

export class LayoutManager {
  private readonly doc: Document;
  private readonly chrome: HTMLElement;
  private readonly sessionBar: HTMLElement;
  private readonly sessionSelect: HTMLSelectElement;
  private readonly wsTabs: HTMLElement;
  private readonly paneBar: HTMLElement;
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
    // Session switcher. Lives outside the per-layout chrome (wsTabs /
    // paneBar are wiped on every `setLayout`); its options are managed
    // separately via `setSessions`, so a layout reshape doesn't drop
    // the dropdown or its open state. Hidden until ≥1 session is known.
    this.sessionBar = doc.createElement("div");
    this.sessionBar.className = "ciri-sessions";
    this.sessionBar.hidden = true;
    this.sessionSelect = doc.createElement("select");
    this.sessionSelect.className = "ciri-session-select";
    this.sessionSelect.setAttribute("aria-label", "Switch session");
    this.sessionSelect.addEventListener("change", () => {
      const name = this.sessionSelect.value;
      if (name.length > 0) this.opts.onSessionSelect?.(name);
    });
    this.sessionBar.appendChild(this.sessionSelect);
    this.wsTabs = doc.createElement("div");
    this.wsTabs.className = "ciri-workspaces";
    this.paneBar = doc.createElement("nav");
    this.paneBar.className = "ciri-panes";
    this.paneBar.setAttribute("role", "toolbar");
    this.paneBar.setAttribute("aria-label", "Switch active pane");
    this.paneBar.setAttribute("aria-orientation", "horizontal");
    this.wsContent = doc.createElement("div");
    this.wsContent.className = "ciri-workspace";
    this.chrome.appendChild(this.sessionBar);
    this.chrome.appendChild(this.wsTabs);
    this.chrome.appendChild(this.paneBar);
    this.chrome.appendChild(this.wsContent);
    this.root.appendChild(this.chrome);
  }

  /// Populate the session dropdown. `names` is the full session list
  /// (server sorts it current-first); `activeName` is the session this
  /// client is attached to and becomes the selected option. Rebuilds
  /// only the `<option>`s — the `<select>` element itself persists, so
  /// repeated calls don't disturb focus or an open native picker. The
  /// bar hides when there are no sessions (e.g. a `__control__`-only
  /// list) and shows otherwise, including the single-session case so
  /// the user can always see which session they're in.
  setSessions(names: readonly string[], activeName: string): void {
    if (this.destroyed) {
      throw new Error("LayoutManager: setSessions called after destroy");
    }
    const visible = names.filter((n) => !n.startsWith("__"));
    if (visible.length === 0) {
      this.sessionBar.hidden = true;
      this.sessionSelect.replaceChildren();
      return;
    }
    const options: Node[] = visible.map((name) => {
      const opt = this.doc.createElement("option");
      opt.value = name;
      opt.textContent = name;
      if (name === activeName) opt.selected = true;
      return opt;
    });
    this.sessionSelect.replaceChildren(...options);
    // If the active session isn't in the list (e.g. an unresolved
    // `__auto__` before SessionSwitched lands), leave the browser's
    // default (first option) selected rather than forcing a value.
    if (visible.includes(activeName)) this.sessionSelect.value = activeName;
    this.sessionBar.hidden = false;
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
    // wsContent + paneBar blank.
    this.slots = new Map();
    const ws = layout.workspaces[activeWsIdx];
    if (ws === undefined) {
      this.wsContent.replaceChildren();
      this.paneBar.replaceChildren();
      return;
    }
    const activeColIdx = Number(ws.activeColumnIdx);
    // Pane chips traverse column-major so visual ordering matches the
    // column/tile layout below. Each chip shares the tile's
    // `onPaneClick` callback, so wire-level focus behavior is identical
    // regardless of which control the user picks.
    const chips: Node[] = [];
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
      //
      // `display: flex` / `flex-direction: column` USED to be set
      // inline here too, but inline styles override external CSS, so
      // a "one-pane-per-screen" theme that wants `.ciri-column { display: none }`
      // for inactive columns couldn't override them — every inactive
      // column kept its `widthProportion` slice of the workspace and
      // showed up as blank space. Themes should declare the column
      // display rule in their own stylesheet now.
      colEl.style.flex = String(col.widthProportion);
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

        // Matching chip in the pane bar. `data-chip-pane-id` (not
        // `data-pane-id`) keeps `document.querySelector(
        // "[data-pane-id=N]")` resolving to the tile slot — the
        // tile is the render target, the chip is just a navigation
        // affordance, and test / external callers reaching for
        // "the pane's DOM" mean the tile.
        const chip = this.doc.createElement("button");
        chip.type = "button";
        chip.className = "ciri-pane-chip";
        chip.dataset["chipPaneId"] = paneIdStr;
        chip.textContent = title.length > 0 ? title : `Pane ${paneIdStr}`;
        // `aria-current="true"` flags the active item in a set —
        // a closer fit than `aria-pressed` (which implies a toggle,
        // but our chips don't deactivate when re-clicked) or
        // `aria-selected` (which is tab-pattern-only). Inactive
        // chips omit the attribute rather than carrying
        // `aria-current="false"` to keep the DOM lean.
        if (isActive) chip.setAttribute("aria-current", "true");
        chip.addEventListener("click", () => {
          this.opts.onPaneClick?.(paneIdBig);
        });
        chips.push(chip);

        // Per-chip close (×) button — same pattern as a browser tab.
        // Sibling button (not nested inside the chip) because HTML
        // forbids `<button>` inside `<button>`; pairing them in a
        // wrapping `<span class="ciri-pane-chip-group">` keeps the
        // visual coupling without breaking the toolbar's flat focus
        // order requirement either.
        const closeBtn = this.doc.createElement("button");
        closeBtn.type = "button";
        closeBtn.className = "ciri-pane-close";
        closeBtn.dataset["closePaneId"] = paneIdStr;
        closeBtn.textContent = "✕";
        closeBtn.setAttribute(
          "aria-label",
          `Close pane: ${title.length > 0 ? title : `Pane ${paneIdStr}`}`,
        );
        closeBtn.title = "Close pane";
        closeBtn.addEventListener("click", () => {
          this.opts.onPaneClose?.(paneIdBig);
        });
        chips.push(closeBtn);
      }
      colEl.replaceChildren(...tileEls);
      columns[ci] = colEl;
    }
    this.wsContent.replaceChildren(...columns);

    // Action buttons follow the chips, separated by a divider so the
    // two groups read as distinct ("which pane to focus" vs "what to
    // do"). Both live inside the same `role="toolbar"` because they
    // share keyboard navigation semantics (Tab cycles, screen reader
    // announces "toolbar Switch active pane").
    const actions = this.opts.actions ?? [];
    const paneBarChildren: Node[] = [...chips];
    if (chips.length > 0 && actions.length > 0) {
      const divider = this.doc.createElement("span");
      divider.className = "ciri-actions-divider";
      // Pure visual separator — screen readers should skip it.
      divider.setAttribute("role", "presentation");
      divider.setAttribute("aria-hidden", "true");
      paneBarChildren.push(divider);
    }
    for (const action of actions) {
      const btn = this.doc.createElement("button");
      btn.type = "button";
      btn.className = "ciri-action";
      btn.dataset["actionId"] = action.id;
      btn.textContent = action.icon;
      // `aria-label` carries the accessible name because the icon
      // glyph alone is opaque to screen readers (and ambiguous to
      // sighted users until they hover for a tooltip).
      btn.setAttribute("aria-label", action.label);
      btn.title = action.label;
      btn.addEventListener("click", () => {
        this.opts.onAction?.(action.id);
      });
      paneBarChildren.push(btn);
    }
    this.paneBar.replaceChildren(...paneBarChildren);
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
    const paneIdStr = paneId.toString();
    const slot = this.slots.get(paneIdStr);
    if (slot === undefined) return;
    slot.dataset["title"] = title;
    // Keep the chip text in sync with the live title so the pane
    // switcher reflects what the shell / program is doing right now,
    // not the stale label captured at `setLayout` time. The pane id
    // is always a stringified `bigint` (decimal digits only) so the
    // selector value never needs escaping — and `CSS.escape` isn't
    // implemented in some test runtimes (jsdom).
    const display = title.length > 0 ? title : `Pane ${paneIdStr}`;
    const chip = this.paneBar.querySelector<HTMLElement>(
      `.ciri-pane-chip[data-chip-pane-id="${paneIdStr}"]`,
    );
    if (chip !== null) {
      chip.textContent = display;
    }
    // Keep the close button's accessible name in sync with the
    // pane's title — screen readers should announce "Close pane:
    // nvim main.rs" rather than the stale "Pane 4".
    const closeBtn = this.paneBar.querySelector<HTMLElement>(
      `.ciri-pane-close[data-close-pane-id="${paneIdStr}"]`,
    );
    if (closeBtn !== null) {
      closeBtn.setAttribute("aria-label", `Close pane: ${display}`);
    }
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
