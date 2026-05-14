// CiriApp — top-level orchestrator for the web client.
//
// Owns:
//   - one `LayoutManager` mounted under the caller-supplied root
//   - one `PaneGrid` + `PaneRenderer` per known pane (keyed by paneId)
//   - one `CiriClient` (transport + codec) — constructed via a
//     pluggable factory so tests can swap in a fake
//   - the keyboard / mouse / wheel / ResizeObserver glue
//
// What CiriApp does NOT own:
//   - the WebSocket itself (delegated to `CiriClient`)
//   - the cell-level grid model (delegated to `PaneGrid`)
//   - the row diff + DOM rendering (delegated to `PaneRenderer`)
//   - the leader / modal / palette / selection layers (deferred past v1)
//
// Lifecycle:
//   const app = new CiriApp(rootEl, { url, sessionName, ... });
//   app.start();                  // connects, sends ClientHello
//   ...                            // events flow; renderers update
//   app.destroy();                // closes socket, tears down DOM

import {
  CiriClient,
  type CiriClientOptions,
  type CiriEvent,
  type ClientHello,
  type ClientMessage,
  type LayoutState,
  type ServerMessage,
} from "@ciri/client";
import {
  decodeCellDelta,
  decodeFullPaneSync,
  FrameBodyDecodeError,
  MODE_APP_CURSOR,
  MODE_BRACKETED_PASTE,
} from "@ciri/codec";
import {
  DEFAULT_THEME,
  GridShapeError,
  PaneGrid,
  PaneRenderer,
  type SelectionAnchor,
  type Theme,
} from "@ciri/dom";
import { encodeKeyboardEvent } from "./input.js";
import { LayoutManager } from "./layout.js";
import {
  cellsForViewport,
  measureCellSize,
  type CellSize,
} from "./measure.js";

/// Minimal client surface CiriApp consumes — everything the
/// orchestrator needs to drive a session. The default factory wraps
/// `CiriClient`; tests pass a custom factory that returns a fake.
export interface CiriAppClientLike {
  start(): void;
  close(): void;
  send(msg: ClientMessage): void;
  sendInput(paneId: bigint, data: Uint8Array): bigint;
}

export type CiriClientFactory = (
  opts: CiriClientOptions,
  onEvent: (e: CiriEvent) => void,
) => CiriAppClientLike;

export interface CiriAppOptions {
  /// WebSocket URL.
  url: string;
  /// Session name to attach to.
  sessionName: string;
  /// Optional auth token forwarded to the transport.
  token?: string;
  /// CSS font-family for terminal cells.
  fontFamily?: string;
  /// CSS font-size for terminal cells, e.g. `"14px"`.
  fontSize?: string;
  /// Color palette. Defaults to `DEFAULT_THEME`.
  theme?: Theme;
  /// How many lines a wheel "tick" should scroll (used for the
  /// browser's deltaMode = "line" path and as the floor for the pixel
  /// path). Defaults to 3 — same as native xterm.
  scrollLinesPerWheelTick?: number;
  /// Maximum scrollback rows retained per pane. See
  /// `DEFAULT_MAX_SCROLLBACK_ROWS` for the default. Lowering this
  /// trades scrollback depth for memory; raising it costs JS heap
  /// proportional to `max × cols`.
  maxScrollbackRows?: number;
  /// Inject an alternative client factory. The default constructs a
  /// real `CiriClient`. Tests pass in a fake to capture outgoing
  /// messages and feed synthetic events back.
  clientFactory?: CiriClientFactory;
  /// Lifecycle callbacks. Optional — defaults are silent.
  onOpen?: () => void;
  onClose?: (reason: string, reconnecting: boolean) => void;
  onError?: (err: Error) => void;
}

const DEFAULT_FONT_FAMILY = `"JetBrains Mono", "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace`;
const DEFAULT_FONT_SIZE = "14px";
/// How long the `ciri-tile-bell` class lingers after a Bell event.
/// Themes typically animate a brief opacity pulse over this window.
const BELL_FLASH_MS = 1000;

export class CiriApp {
  private readonly doc: Document;
  private readonly orphanRoot: HTMLElement;
  private readonly layout: LayoutManager;
  private readonly grids = new Map<string, PaneGrid>();
  private readonly renderers = new Map<string, PaneRenderer>();
  private currentLayout: LayoutState | null = null;
  /// Pane the user just clicked, waiting for the server's
  /// `LayoutUpdate` ack to officially mark it active. Until then,
  /// `onKeyDown` routes keystrokes here so a quick click-then-type
  /// doesn't land on the previously-active pane. Cleared on the next
  /// applyLayout regardless of what the server promotes (the server
  /// is authoritative once a new layout arrives).
  private pendingFocusedPaneId: bigint | null = null;
  /// Set to `true` after a `FrameBodyDecodeError` to mark the wire
  /// stream as no-longer-trustworthy. Subsequent frames are dropped
  /// even if they parse fine, because the partial CellDelta /
  /// FullPaneSync that failed mid-decode may have left grid state
  /// inconsistent with what the server thinks the client has.
  /// Cleared only on `destroy()` — recovery requires reconstructing
  /// the CiriApp.
  private connectionPoisoned = false;
  private cellSize: CellSize;
  private readonly theme: Theme;
  private readonly fontFamily: string;
  private readonly fontSize: string;
  private readonly scrollLinesPerWheelTick: number;
  private readonly maxScrollbackRows: number | undefined;
  private readonly client: CiriAppClientLike;
  private destroyed = false;
  private resizeObserver: ResizeObserver | null = null;
  // DECCKM tracking is now per-keystroke and per-pane — `onKeyDown`
  // reads the target pane's `meta.modeFlags & MODE_APP_CURSOR`.
  private readonly handlerOnOpen?: () => void;
  private readonly handlerOnClose?: (reason: string, reconnecting: boolean) => void;
  private readonly handlerOnError?: (err: Error) => void;
  // DOM event handlers stored as instance fields so `destroy()` can
  // remove them. Anonymous arrows installed at construction time
  // would otherwise stick around forever and a remounted CiriApp on
  // the same root would double-fire every keystroke.
  private readonly onKeyDownHandler: (e: KeyboardEvent) => void;
  private readonly onWheelHandler: (e: WheelEvent) => void;
  private readonly onMouseDownHandler: (e: MouseEvent) => void;
  /// Pending bell-flash clear timers, keyed by paneId-as-string so a
  /// repeat bell on the same pane re-arms cleanly instead of leaving
  /// the class permanently stuck.
  private readonly bellTimers = new Map<string, ReturnType<Window["setTimeout"]>>();
  /// In-flight mouse drag state, recorded on mousedown and updated on
  /// each window-level mousemove. `null` when no drag is active.
  private dragState: {
    paneId: bigint;
    renderer: PaneRenderer;
    grid: PaneGrid;
    anchor: SelectionAnchor;
    moved: boolean;
  } | null = null;
  private readonly onWindowMouseMove: (e: MouseEvent) => void;
  private readonly onWindowMouseUp: (e: MouseEvent) => void;

  constructor(
    private readonly root: HTMLElement,
    opts: CiriAppOptions,
  ) {
    const doc = root.ownerDocument;
    if (doc === null) {
      throw new Error("CiriApp: root element is not attached to a document");
    }
    this.doc = doc;
    this.theme = opts.theme ?? DEFAULT_THEME;
    this.fontFamily = opts.fontFamily ?? DEFAULT_FONT_FAMILY;
    this.fontSize = opts.fontSize ?? DEFAULT_FONT_SIZE;
    this.scrollLinesPerWheelTick = Math.max(
      1,
      Math.floor(opts.scrollLinesPerWheelTick ?? 3),
    );
    this.maxScrollbackRows = opts.maxScrollbackRows;
    if (opts.onOpen !== undefined) this.handlerOnOpen = opts.onOpen;
    if (opts.onClose !== undefined) this.handlerOnClose = opts.onClose;
    if (opts.onError !== undefined) this.handlerOnError = opts.onError;
    this.onKeyDownHandler = (e) => this.onKeyDown(e);
    this.onWheelHandler = (e) => this.onWheel(e);
    this.onMouseDownHandler = (e) => {
      this.handleRootMouseDown(e);
      queueMicrotask(() => {
        if (!this.destroyed) this.root.focus();
      });
    };
    this.onWindowMouseMove = (e) => this.handleWindowMouseMove(e);
    this.onWindowMouseUp = (e) => this.handleWindowMouseUp(e);

    // Hidden container for pane renderers whose tile isn't currently
    // mounted (inactive workspaces, transient layout reshapes).
    this.orphanRoot = doc.createElement("div");
    this.orphanRoot.className = "ciri-orphan-panes";
    this.orphanRoot.style.display = "none";
    doc.body.appendChild(this.orphanRoot);

    this.cellSize = this.measureCellsOrFallback();

    this.layout = new LayoutManager(this.root, {
      onPaneClick: (id) => this.onPaneClicked(id),
      onWorkspaceClick: (idx) => this.onWorkspaceClicked(idx),
      paneTitleFor: (id) => this.grids.get(id.toString())?.title ?? "",
    });

    this.installInteractionHandlers();

    const initialViewport = this.computeViewport(opts.sessionName);
    const clientOpts: CiriClientOptions = {
      url: opts.url,
      sessionName: opts.sessionName,
      viewport: initialViewport,
      ...(opts.token !== undefined ? { token: opts.token } : {}),
    };
    const factory: CiriClientFactory =
      opts.clientFactory ??
      ((o, h) => new CiriClient(o, h));
    this.client = factory(clientOpts, (e) => this.onClientEvent(e));
  }

  /// Start the underlying transport + install the ResizeObserver.
  /// Separate from the constructor so the caller has a chance to
  /// wire additional listeners (e.g. forwarding error events to a
  /// UI banner) before the first `open` lands.
  start(): void {
    if (this.destroyed) throw new Error("CiriApp: start called after destroy");
    this.client.start();
    this.installResizeObserver();
  }

  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;
    this.resizeObserver?.disconnect();
    this.resizeObserver = null;
    // Detach interaction handlers before closing the client so a
    // late-arriving DOM event can't try to send through it.
    this.root.removeEventListener("keydown", this.onKeyDownHandler);
    this.root.removeEventListener("wheel", this.onWheelHandler);
    this.root.removeEventListener("mousedown", this.onMouseDownHandler);
    const win = this.doc.defaultView;
    if (win !== null) {
      win.removeEventListener("mousemove", this.onWindowMouseMove);
      win.removeEventListener("mouseup", this.onWindowMouseUp);
    }
    this.dragState = null;
    // Cancel pending bell-flash clears so we don't run callbacks
    // against torn-down DOM.
    for (const t of this.bellTimers.values()) {
      win?.clearTimeout(t);
    }
    this.bellTimers.clear();
    this.client.close();
    for (const r of this.renderers.values()) r.destroy();
    this.renderers.clear();
    this.grids.clear();
    this.layout.destroy();
    this.orphanRoot.remove();
  }

  /// Re-measure cell size and notify the server. Call this after a
  /// font has finished loading (`document.fonts.ready.then(...)`),
  /// since the original measurement may have been done with a
  /// fallback font that has different metrics.
  remeasureCells(): void {
    if (this.destroyed) return;
    const size = this.measureCellsOrFallback();
    this.cellSize = size;
    this.sendResize();
  }

  /// Public entrypoint that funnels into the internal handler. Useful
  /// for tests / replay code that wants to feed synthetic events
  /// without a real transport.
  handleClientEvent(e: CiriEvent): void {
    this.onClientEvent(e);
  }

  // ─── Test surface ────────────────────────────────────────────────

  /// Number of pane renderers currently alive.
  get paneCount(): number {
    return this.renderers.size;
  }

  /// True if a grid exists for `paneId`. Doesn't say whether the
  /// renderer is attached to a layout slot.
  hasGrid(paneId: bigint): boolean {
    return this.grids.has(paneId.toString());
  }

  /// Read-only access to a pane's grid. Used by tests to inspect
  /// (and occasionally tweak) meta state without going through the
  /// full wire path. Returns `undefined` for unknown panes.
  paneGrid(paneId: bigint): PaneGrid | undefined {
    return this.grids.get(paneId.toString());
  }

  /// Read-only handle to the LayoutManager. Tests use this to inspect
  /// slot DOM nodes; application code should not need it.
  get layoutManager(): LayoutManager {
    return this.layout;
  }

  // ─── Interaction wiring ──────────────────────────────────────────

  private installInteractionHandlers(): void {
    // Make the root focusable so it captures keyboard events even
    // when the inner buttons / spans steal focus on click.
    this.root.tabIndex = 0;
    // `queueMicrotask` rather than synchronous focus — jsdom can
    // assert during construction if the element isn't yet visible.
    queueMicrotask(() => {
      if (!this.destroyed) this.root.focus();
    });
    this.root.addEventListener("keydown", this.onKeyDownHandler);
    this.root.addEventListener("wheel", this.onWheelHandler, {
      passive: false,
    });
    // Refocus the root after any inner mousedown so workspace-tab or
    // pane-tile clicks don't steal keyboard focus from the terminal.
    // Also starts the selection drag when the mousedown landed on a
    // tile (see `handleRootMouseDown`).
    this.root.addEventListener("mousedown", this.onMouseDownHandler);
  }

  /// Cell pixel size used for hit-testing mouse coords into (col,
  /// srcRow). Exposed so tests can stub it; production callers should
  /// not override.
  private get measuredCellSize(): CellSize {
    return this.cellSize;
  }

  /// Map a viewport `MouseEvent` to a `(col, srcRow)` anchor inside
  /// `renderer.container`. Returns `null` when the click landed
  /// outside the cell grid (e.g. between panes) or when the renderer
  /// has zero size (jsdom). Clamps col/displayRow to the grid bounds.
  private hitTestAnchor(
    e: MouseEvent,
    renderer: PaneRenderer,
    grid: PaneGrid,
  ): SelectionAnchor | null {
    const rect = renderer.container.getBoundingClientRect();
    const offsetX = e.clientX - rect.left;
    const offsetY = e.clientY - rect.top;
    if (offsetX < 0 || offsetY < 0) return null;
    const { cellWidth, cellHeight } = this.measuredCellSize;
    if (cellWidth <= 0 || cellHeight <= 0) return null;
    const rawCol = Math.floor(offsetX / cellWidth);
    const rawDisplayRow = Math.floor(offsetY / cellHeight);
    const col = clampNumber(rawCol, 0, Math.max(0, grid.cols - 1));
    const displayRow = clampNumber(
      rawDisplayRow,
      0,
      Math.max(0, grid.rows - 1),
    );
    const srcRow =
      displayRow + grid.scrollbackRows - renderer.scrollOffsetRows;
    return { col, srcRow };
  }

  private handleRootMouseDown(e: MouseEvent): void {
    // Only the primary mouse button starts a selection; secondary
    // (right) is reserved for a future context menu, middle for
    // X-style paste.
    if (e.button !== 0) return;
    const target = e.target;
    if (!(target instanceof Element)) return;
    const tileEl = target.closest(".ciri-tile");
    if (!(tileEl instanceof HTMLElement)) return;
    const paneIdStr = tileEl.dataset["paneId"];
    if (paneIdStr === undefined) return;
    const renderer = this.renderers.get(paneIdStr);
    const grid = this.grids.get(paneIdStr);
    if (renderer === undefined || grid === undefined) return;
    const anchor = this.hitTestAnchor(e, renderer, grid);
    if (anchor === null) return;
    // Existing selection from a previous drag is replaced wholesale —
    // the user is starting a fresh selection at the new anchor.
    this.dragState = {
      paneId: BigInt(paneIdStr),
      renderer,
      grid,
      anchor,
      moved: false,
    };
    renderer.setSelection({
      start: anchor,
      end: anchor,
      active: true,
    });
    renderer.render(grid);
    // Listen on the window so a drag that leaves the tile (or even
    // the document) still gets the matching `mouseup`.
    const win = this.doc.defaultView;
    if (win !== null) {
      win.addEventListener("mousemove", this.onWindowMouseMove);
      win.addEventListener("mouseup", this.onWindowMouseUp);
    }
  }

  private handleWindowMouseMove(e: MouseEvent): void {
    const drag = this.dragState;
    if (drag === null) return;
    const anchor = this.hitTestAnchor(e, drag.renderer, drag.grid);
    if (anchor === null) return;
    if (
      anchor.col !== drag.anchor.col ||
      anchor.srcRow !== drag.anchor.srcRow
    ) {
      drag.moved = true;
    }
    drag.renderer.setSelection({
      start: drag.anchor,
      end: anchor,
      active: true,
    });
    drag.renderer.render(drag.grid);
  }

  private handleWindowMouseUp(_e: MouseEvent): void {
    const drag = this.dragState;
    if (drag === null) return;
    const win = this.doc.defaultView;
    if (win !== null) {
      win.removeEventListener("mousemove", this.onWindowMouseMove);
      win.removeEventListener("mouseup", this.onWindowMouseUp);
    }
    if (!drag.moved) {
      // No drag motion — treat as a plain focus click. Clear the
      // empty selection so a stray block-on-anchor doesn't linger
      // visually.
      drag.renderer.setSelection(null);
      drag.renderer.render(drag.grid);
    } else {
      // Finalize: drop the `active` flag.
      const cur = drag.renderer.currentSelection;
      if (cur !== null) {
        drag.renderer.setSelection({ ...cur, active: false });
        drag.renderer.render(drag.grid);
      }
    }
    this.dragState = null;
  }

  private installResizeObserver(): void {
    // Some test environments (and older jsdom builds) don't ship
    // ResizeObserver. Skip silently — the caller can poll
    // `remeasureCells()` manually if needed.
    if (typeof ResizeObserver === "undefined") return;
    this.resizeObserver = new ResizeObserver(() => {
      if (!this.destroyed) this.sendResize();
    });
    // Observe the workspace viewport so a tab-strip height change
    // (or anything else outside the cell grid) doesn't trigger a
    // spurious resize. Round-6 codex fix.
    this.resizeObserver.observe(this.layout.viewportEl);
  }

  private measureCellsOrFallback(): CellSize {
    try {
      return measureCellSize({
        fontFamily: this.fontFamily,
        fontSize: this.fontSize,
        document: this.doc,
      });
    } catch {
      // Font hasn't loaded yet (or we're in a layout-less env like
      // jsdom). Fall back to typical 14px monospace metrics — the
      // first real ResizeObserver tick re-measures.
      return { cellWidth: 8.5, cellHeight: 16.8 };
    }
  }

  private computeViewport(sessionName: string): ClientHello {
    const rect = this.viewportRect();
    // jsdom reports {0,0} layout; clamp to >= 1 so `encodeClientHello`
    // doesn't bounce the value. The first real ResizeObserver tick
    // re-sends a proper Resize.
    const width = Math.max(1, Math.floor(rect.width));
    const height = Math.max(1, Math.floor(rect.height));
    return {
      sessionName,
      width,
      height,
      cellWidth: this.cellSize.cellWidth,
      cellHeight: this.cellSize.cellHeight,
    };
  }

  private sendResize(): void {
    const rect = this.viewportRect();
    const width = Math.max(1, Math.floor(rect.width));
    const height = Math.max(1, Math.floor(rect.height));
    const { cols, rows } = cellsForViewport(width, height, this.cellSize);
    this.client.send({
      tag: "Resize",
      cols,
      rows,
      width,
      height,
      cellWidth: this.cellSize.cellWidth,
      cellHeight: this.cellSize.cellHeight,
    });
  }

  /// Bounds of the area actually used to paint pane content — i.e.
  /// the active-workspace container, NOT the outer root that also
  /// includes the workspace tab strip and any future chrome (status
  /// bar, banner). Using the outer root would over-count vertical
  /// pixels and the server-allocated rows would push the bottom
  /// terminal lines past the visible area. Round-6 codex fix.
  private viewportRect(): DOMRect {
    return this.layout.viewportEl.getBoundingClientRect();
  }

  private onKeyDown(e: KeyboardEvent): void {
    // Clipboard chords intercept *before* the encoder runs: the
    // encoder rejects Ctrl+Shift+anything and Cmd+anything (the
    // browser-reserved escape hatches), but Ctrl+Shift+C / Cmd+C /
    // Ctrl+Shift+V / Cmd+V are the canonical terminal copy/paste
    // bindings and we DO want to handle them.
    if (this.tryHandleClipboardChord(e)) return;
    // Prefer the pending-focus target over the layout-derived active
    // pane so a click-then-type sequence reaches the just-clicked
    // pane even before the server's LayoutUpdate has confirmed the
    // focus change. Round-7 codex fix.
    const target =
      this.pendingFocusedPaneId ??
      (this.currentLayout !== null
        ? LayoutManager.activePaneId(this.currentLayout)
        : null);
    if (target === null) return;
    // Drop bytes for panes the app doesn't know about (e.g. a stale
    // pending target whose pane closed). Sending to a missing pane
    // is harmless on the server side, but suppressing here avoids
    // generating ack traffic for ghost cursors.
    const grid = this.grids.get(target.toString());
    if (grid === undefined) return;
    // DECCKM: arrows + Home/End emit SS3 (`ESC O X`) instead of CSI
    // when the active pane's terminal app has set application-cursor
    // mode (vim, less, htop, …). Read the bit per keystroke since a
    // single session toggles freely as apps come and go.
    const appCursor = (grid.meta.modeFlags & MODE_APP_CURSOR) !== 0;
    const r = encodeKeyboardEvent(e, { applicationCursorKeys: appCursor });
    if (r === null) return;
    if (r.preventDefault) e.preventDefault();
    this.client.sendInput(target, r.bytes);
  }

  private onWheel(e: WheelEvent): void {
    // Ctrl+wheel is browser zoom; don't hijack it.
    if (e.ctrlKey) return;
    const target = e.target;
    if (!(target instanceof Element)) return;
    const tileEl = target.closest(".ciri-tile");
    if (!(tileEl instanceof HTMLElement)) return;
    const paneIdStr = tileEl.dataset["paneId"];
    if (paneIdStr === undefined) return;
    const renderer = this.renderers.get(paneIdStr);
    const grid = this.grids.get(paneIdStr);
    if (renderer === undefined || grid === undefined) return;
    e.preventDefault();
    const linesUp = wheelDeltaToLines(
      e,
      this.cellSize.cellHeight,
      this.scrollLinesPerWheelTick,
    );
    if (linesUp === 0) return;
    const next = Math.max(0, renderer.scrollOffsetRows + linesUp);
    renderer.setScrollOffset(next);
    renderer.render(grid);
  }

  /// Intercept the Ctrl+Shift+C / Cmd+C copy and Ctrl+Shift+V /
  /// Cmd+V paste chords before the regular keystroke encoder gets a
  /// chance to reject them. Returns `true` if the event was handled
  /// (and `preventDefault` invoked).
  private tryHandleClipboardChord(e: KeyboardEvent): boolean {
    const key = e.key.toLowerCase();
    // metaKey on macOS Safari/Chrome maps to Cmd. We also accept the
    // Windows/Linux equivalent (Win key) — though typing Win+C there
    // is unusual, the symmetry mirrors how the encoder defers to the
    // platform clipboard via `metaKey`. ctrlKey + shiftKey is the
    // canonical web-terminal binding on Linux/Win.
    const isCmdChord = e.metaKey && !e.altKey && !e.ctrlKey && !e.shiftKey;
    const isCtrlShift = e.ctrlKey && e.shiftKey && !e.altKey && !e.metaKey;
    if (key === "c" && (isCtrlShift || isCmdChord)) {
      const copied = this.copySelectionToClipboard();
      if (copied) {
        e.preventDefault();
        return true;
      }
      // No selection to copy → let the browser do whatever its
      // default is (which for Cmd+C is the system copy, harmless).
      return false;
    }
    if (key === "v" && (isCtrlShift || isCmdChord)) {
      e.preventDefault();
      void this.pasteFromClipboard();
      return true;
    }
    return false;
  }

  /// Snapshot the first pane with a non-null selection and write its
  /// text content to `navigator.clipboard`. Returns whether anything
  /// was actually written — callers use this to decide whether to
  /// suppress the browser's default behavior.
  private copySelectionToClipboard(): boolean {
    for (const [key, renderer] of this.renderers) {
      const sel = renderer.currentSelection;
      if (sel === null) continue;
      const grid = this.grids.get(key);
      if (grid === undefined) continue;
      const text = grid.extractText(sel.start, sel.end);
      if (text.length === 0) continue;
      const clipboard = this.doc.defaultView?.navigator?.clipboard;
      if (clipboard === undefined) return false;
      // `writeText` returns a Promise; surface failures through
      // `onError` rather than throwing into the event loop.
      clipboard.writeText(text).catch((err: unknown) => {
        const e = err instanceof Error ? err : new Error(String(err));
        this.handlerOnError?.(e);
      });
      return true;
    }
    return false;
  }

  /// Read the system clipboard and send its text into the active
  /// pane's PTY, wrapping in `ESC[200~ ... ESC[201~` when the pane's
  /// terminal app has set `MODE_BRACKETED_PASTE` (mirror of the Rust
  /// client's paste path in `context_menu.rs`).
  private async pasteFromClipboard(): Promise<void> {
    if (this.currentLayout === null) return;
    const activePaneId =
      this.pendingFocusedPaneId ?? LayoutManager.activePaneId(this.currentLayout);
    if (activePaneId === null) return;
    const grid = this.grids.get(activePaneId.toString());
    if (grid === undefined) return;
    const win = this.doc.defaultView;
    const clipboard = win?.navigator?.clipboard;
    if (clipboard === undefined || typeof clipboard.readText !== "function") {
      return;
    }
    let text: string;
    try {
      text = await clipboard.readText();
    } catch (err: unknown) {
      // User-denied permission or browser without clipboard support —
      // surface but don't crash.
      const e = err instanceof Error ? err : new Error(String(err));
      this.handlerOnError?.(e);
      return;
    }
    if (this.destroyed) return;
    if (text.length === 0) return;
    const enc = new TextEncoder();
    const body = enc.encode(text);
    const bracketed = (grid.meta.modeFlags & MODE_BRACKETED_PASTE) !== 0;
    if (!bracketed) {
      this.client.sendInput(activePaneId, body);
      return;
    }
    const prefix = enc.encode("\x1b[200~");
    const suffix = enc.encode("\x1b[201~");
    const wrapped = new Uint8Array(prefix.length + body.length + suffix.length);
    wrapped.set(prefix, 0);
    wrapped.set(body, prefix.length);
    wrapped.set(suffix, prefix.length + body.length);
    this.client.sendInput(activePaneId, wrapped);
  }

  private onPaneClicked(paneId: bigint): void {
    // Record the local intent so the *next* keystroke routes to the
    // just-clicked pane, not the layout-derived active one. The
    // server is asked to confirm via FocusPane; the next LayoutUpdate
    // clears the pending state.
    this.pendingFocusedPaneId = paneId;
    this.client.send({ tag: "FocusPane", paneId });
  }

  private onWorkspaceClicked(idx: bigint): void {
    this.client.send({ tag: "SwitchWorkspace", workspaceIdx: idx });
  }

  // ─── Inbound client events ───────────────────────────────────────

  private onClientEvent(e: CiriEvent): void {
    switch (e.kind) {
      case "open":
        this.handlerOnOpen?.();
        return;
      case "close":
        this.handlerOnClose?.(e.reason, e.reconnecting);
        return;
      case "error":
        this.handlerOnError?.(e.error);
        return;
      case "server-msg":
        this.handleServerMsg(e.msg);
        return;
      case "cell-delta":
        this.handleCellDeltaBytes(e.payload);
        return;
      case "full-pane-sync":
        this.handleFullPaneSyncBytes(e.payload);
        return;
      default: {
        const _exhaustive: never = e;
        return _exhaustive;
      }
    }
  }

  private handleServerMsg(msg: ServerMessage): void {
    switch (msg.tag) {
      case "StateSync":
      case "LayoutUpdate":
      case "LayoutReply":
        this.applyLayout(msg.layout);
        return;
      case "PaneClosed":
        this.destroyPane(msg.paneId);
        return;
      case "TitleChanged":
        this.handleTitleChanged(msg.paneId, msg.title);
        return;
      case "Bell":
        this.handleBell(msg.paneId);
        return;
      default:
        // v1: ignore everything else (Bell, ImagePlacement,
        // SessionList, … — those grow their own handlers in later
        // phases).
        return;
    }
  }

  /// Server Bell event. Add a transient CSS class to the pane's tile
  /// so a theme can flash a visual indicator; clears automatically
  /// after `BELL_FLASH_MS`. Panes outside the active workspace have
  /// no slot to flash — for now we drop the visual cue on them; a
  /// later phase can surface a badge on the workspace tab.
  private handleBell(paneId: bigint): void {
    const slot = this.layout.getSlot(paneId);
    if (slot === null) return;
    slot.classList.add("ciri-tile-bell");
    // Re-arm the timer on a repeat bell — terminal apps can ring
    // many times in quick succession (e.g. tab-completion failure
    // spam), and we want the class to stay applied through the
    // whole burst rather than briefly dropping it between rings.
    const existingTimer = this.bellTimers.get(paneId.toString());
    if (existingTimer !== undefined) {
      this.doc.defaultView?.clearTimeout(existingTimer);
    }
    const win = this.doc.defaultView;
    if (win === null) return;
    const t = win.setTimeout(() => {
      slot.classList.remove("ciri-tile-bell");
      this.bellTimers.delete(paneId.toString());
    }, BELL_FLASH_MS);
    this.bellTimers.set(paneId.toString(), t);
  }

  private handleTitleChanged(paneId: bigint, title: string): void {
    const grid = this.grids.get(paneId.toString());
    if (grid !== undefined) grid.title = title;
    this.layout.setTileTitle(paneId, title);
    // Reflect the active pane's title on `document.title` so the
    // browser tab / window chrome updates without the caller having
    // to subscribe to a separate event.
    if (
      this.currentLayout !== null &&
      LayoutManager.activePaneId(this.currentLayout) === paneId
    ) {
      this.doc.title = title;
    }
  }

  private applyLayout(layout: LayoutState): void {
    this.currentLayout = layout;
    // Server is authoritative once a new layout arrives: clear the
    // optimistic click-to-focus shortcut regardless of whether the
    // server actually promoted the pending pane to active. Anything
    // else can drift the local view past what the server reports.
    this.pendingFocusedPaneId = null;
    this.layout.setLayout(layout);
    // Move renderer containers into their new slots; park orphans.
    const live = new Set<string>();
    for (const paneId of this.layout.paneIdsInLayout()) {
      const key = paneId.toString();
      live.add(key);
      const r = this.renderers.get(key);
      if (r === undefined) continue;
      const slot = this.layout.getSlot(paneId);
      if (slot !== null && r.container.parentNode !== slot) {
        slot.appendChild(r.container);
      }
    }
    for (const [key, r] of this.renderers) {
      if (live.has(key)) continue;
      if (r.container.parentNode !== this.orphanRoot) {
        this.orphanRoot.appendChild(r.container);
      }
    }
    // Keep document.title in sync with the active pane's known title.
    const activeId = LayoutManager.activePaneId(layout);
    if (activeId !== null) {
      const t = this.grids.get(activeId.toString())?.title ?? "";
      this.doc.title = t;
    }
  }

  private handleCellDeltaBytes(bytes: Uint8Array): void {
    if (this.connectionPoisoned) return;
    let cd;
    try {
      cd = decodeCellDelta(bytes);
    } catch (e) {
      this.reportBodyDecodeError(e);
      return;
    }
    const key = cd.meta.paneId.toString();
    const grid = this.grids.get(key);
    const renderer = this.renderers.get(key);
    // CellDelta arriving before the first FullPaneSync for this pane:
    // drop it. The server resends a sync on attach + on any reflow,
    // so the next sync will reconcile.
    if (grid === undefined || renderer === undefined) return;
    try {
      grid.applyCellDelta(cd);
    } catch (e) {
      if (e instanceof GridShapeError) {
        this.handlerOnError?.(e);
        return;
      }
      throw e;
    }
    renderer.render(grid);
  }

  private handleFullPaneSyncBytes(bytes: Uint8Array): void {
    if (this.connectionPoisoned) return;
    let sync;
    try {
      sync = decodeFullPaneSync(bytes);
    } catch (e) {
      this.reportBodyDecodeError(e);
      return;
    }
    const key = sync.meta.paneId.toString();
    let grid = this.grids.get(key);
    let renderer = this.renderers.get(key);
    if (grid === undefined) {
      // Scrollback-only first-sync (rows = 0) is theoretically
      // possible but unexpected — give the grid a single placeholder
      // row so the renderer has something to reconcile against; the
      // next non-empty sync will re-size correctly.
      const initialRows = sync.rows > 0 ? sync.rows : 1;
      grid = new PaneGrid(
        sync.meta.paneId,
        sync.cols || 1,
        initialRows,
        this.maxScrollbackRows !== undefined
          ? { maxScrollbackRows: this.maxScrollbackRows }
          : {},
      );
      this.grids.set(key, grid);
    }
    if (renderer === undefined) {
      // Mount in the orphan root; `applyLayout` will move into a
      // proper slot if the layout has one ready.
      renderer = new PaneRenderer(this.orphanRoot, {
        theme: this.theme,
        fontFamily: this.fontFamily,
        fontSize: this.fontSize,
      });
      this.renderers.set(key, renderer);
      const slot = this.layout.getSlot(sync.meta.paneId);
      if (slot !== null) slot.appendChild(renderer.container);
    }
    try {
      grid.applyFullPaneSync(sync);
    } catch (e) {
      if (e instanceof GridShapeError) {
        this.handlerOnError?.(e);
        return;
      }
      throw e;
    }
    renderer.render(grid);
  }

  private destroyPane(paneId: bigint): void {
    const key = paneId.toString();
    const r = this.renderers.get(key);
    if (r !== undefined) r.destroy();
    this.renderers.delete(key);
    this.grids.delete(key);
  }

  private reportBodyDecodeError(e: unknown): void {
    if (e instanceof FrameBodyDecodeError) {
      // Wire-level decode failure → the byte stream is corrupted or
      // a version mismatch shipped an unparseable frame. Either way
      // the server still thinks the client has the now-half-applied
      // state, so applying any further CellDelta / FullPaneSync
      // would build on the wrong baseline. Poison the connection,
      // close the transport, and surface to the caller; recovery is
      // a fresh `new CiriApp(...)`.
      this.connectionPoisoned = true;
      this.handlerOnError?.(e);
      try {
        this.client.close();
      } catch {
        // close is idempotent; swallow.
      }
      return;
    }
    // Anything else is a programming error — let it bubble so a real
    // crash trace lands in dev tools instead of being silently
    // swallowed by the wire path.
    throw e;
  }
}

function clampNumber(n: number, lo: number, hi: number): number {
  if (hi < lo) return lo;
  return n < lo ? lo : n > hi ? hi : n;
}

/// Translate a `WheelEvent` to a row count: positive = scroll up
/// (into scrollback), negative = scroll down (toward the live
/// bottom). Handles the three `deltaMode` values browsers may emit.
function wheelDeltaToLines(
  e: WheelEvent,
  cellHeight: number,
  linesPerTick: number,
): number {
  if (e.deltaMode === 1 /* DOM_DELTA_LINE */) {
    return -Math.round(e.deltaY * linesPerTick);
  }
  if (e.deltaMode === 2 /* DOM_DELTA_PAGE */) {
    // Treat a page as ~10 lines. The real PTU rarely reports DELTA_PAGE
    // events, so this is a safety net rather than a hot path.
    return -Math.round(e.deltaY * 10);
  }
  // DOM_DELTA_PIXEL (the common case on modern browsers).
  if (cellHeight > 0) {
    return -Math.round(e.deltaY / cellHeight);
  }
  // Pathological — cellHeight should never be zero given the
  // measurement fallback, but if it is, fall back to fixed-line.
  return e.deltaY < 0 ? linesPerTick : -linesPerTick;
}
