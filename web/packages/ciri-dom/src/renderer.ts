// DOM renderer: pane grid → per-row <div> / per-SGR-run <span>.
//
// Mount lifecycle:
//   1. Caller constructs `new PaneRenderer(root, { theme })`. The
//      renderer attaches a wrapper element to `root`.
//   2. Caller drives a grid: applies CellDelta / FullPaneSync, calls
//      `render(grid)` after each batch. The renderer reads
//      `grid.takeDirtyRows()` and rebuilds only those rows.
//   3. Caller calls `destroy()` on tear-down — removes the wrapper
//      and drops references.
//
// Why a class and not a function: we need to retain the per-row DOM
// node handles between calls so the dirty-row diff actually skips
// untouched rows. Functional-style "replace innerHTML" would force a
// full repaint on every `render()`.

import type { PackedCell } from "@ciri/codec";
import type { PaneGrid } from "./grid.js";
import { DEFAULT_THEME, type Theme } from "./theme.js";
import {
  rowRuns,
  type SgrRun,
} from "./sgr-run.js";

export interface RendererOptions {
  /// Color palette. Defaults to `DEFAULT_THEME` (16-color ANSI + a
  /// terminal-friendly fg/bg).
  theme?: Theme;
  /// CSS font-family applied to the wrapper. Defaults to a common
  /// monospace stack; callers usually override.
  fontFamily?: string;
  /// CSS font-size applied to the wrapper, e.g. `"14px"`.
  fontSize?: string;
}

const DEFAULT_FONT_FAMILY = `"JetBrains Mono", "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace`;
const DEFAULT_FONT_SIZE = "14px";

/// Live DOM renderer for a single pane.
///
/// Owns:
///   - one wrapper `<div class="ciri-pane">` mounted under `root`
///   - per-row `<div class="ciri-row">` children, indexed by grid row
///
/// Does NOT own:
///   - the grid model (passed to `render()` each call)
///   - cursor positioning, blink, selection (later phase)
///   - event handlers (key/mouse — later phase)
export class PaneRenderer {
  private readonly doc: Document;
  private readonly wrapper: HTMLElement;
  /// Pre-allocated row containers indexed by row index. Length is kept
  /// in lockstep with `grid.rows`; a shrunken grid drops the trailing
  /// rows, a grown grid appends fresh ones.
  private rowEls: HTMLElement[] = [];
  private theme: Theme;
  private destroyed = false;
  /// Set by `setTheme()` so the next `render()` repaints every row
  /// even if the grid itself has no pending damage — the run
  /// grouper's color output depends on the active theme.
  private rendererForcedRedraw = false;

  constructor(
    private readonly root: HTMLElement,
    opts: RendererOptions = {},
  ) {
    this.theme = opts.theme ?? DEFAULT_THEME;
    // `root.ownerDocument` is non-null for any attached element — the
    // type system widens to `Document | null` because detached element
    // pseudo-roots exist, but the renderer is only useful against a
    // mounted node. Take the document up-front so later
    // `createElement` calls don't have to revalidate.
    const doc = root.ownerDocument;
    if (doc === null) {
      throw new Error("PaneRenderer: root element is not attached to a document");
    }
    this.doc = doc;
    this.wrapper = this.doc.createElement("div");
    this.wrapper.className = "ciri-pane";
    this.wrapper.style.fontFamily = opts.fontFamily ?? DEFAULT_FONT_FAMILY;
    this.wrapper.style.fontSize = opts.fontSize ?? DEFAULT_FONT_SIZE;
    this.wrapper.style.whiteSpace = "pre";
    this.wrapper.style.backgroundColor = this.theme.background;
    this.wrapper.style.color = this.theme.foreground;
    this.wrapper.style.lineHeight = "1.2";
    // The cell font and the cell size combine to set the character
    // grid; keeping line-height a fixed ratio (1.2) means a future
    // resize math layer can derive cellHeight from the rendered
    // font-size without a `getBoundingClientRect` roundtrip.
    this.root.appendChild(this.wrapper);
  }

  /// Apply pending grid state to the DOM. Reads
  /// `grid.takeDirtyRows()` and rebuilds only those rows; on a full
  /// redraw also re-sizes the row container list to match `grid.rows`.
  render(grid: PaneGrid): void {
    if (this.destroyed) {
      throw new Error("PaneRenderer: render called after destroy");
    }
    const dirty = grid.takeDirtyRows();
    // A theme change forces a complete repaint even when the grid
    // has no pending damage of its own — every existing span's
    // inline color came from the old theme.
    const themeRedraw = this.rendererForcedRedraw;
    this.rendererForcedRedraw = false;
    const fullRedraw = dirty.fullRedraw || themeRedraw;
    if (fullRedraw || this.rowEls.length !== grid.rows) {
      this.reconcileRowContainers(grid.rows);
    }
    let rowsToRender: number[];
    if (themeRedraw && !dirty.fullRedraw) {
      // takeDirtyRows() returned the grid's diff (possibly empty);
      // expand to all viewport rows so the theme change actually
      // reaches the DOM. Don't trust `dirty.rows` to already cover
      // everything — the grid may have been quiescent.
      rowsToRender = new Array(grid.rows);
      for (let i = 0; i < grid.rows; i += 1) rowsToRender[i] = i;
    } else {
      rowsToRender = dirty.rows;
    }
    for (const r of rowsToRender) {
      if (r < 0 || r >= grid.rows) continue;
      this.renderRow(grid, r);
    }
  }

  /// Replace the active theme. Sets a renderer-level forced-redraw
  /// flag so the next `render()` repaints every viewport row, since
  /// each cell span's inline foreground/background was resolved
  /// against the previous theme.
  setTheme(theme: Theme): void {
    this.theme = theme;
    this.wrapper.style.backgroundColor = theme.background;
    this.wrapper.style.color = theme.foreground;
    this.rendererForcedRedraw = true;
  }

  /// Detach the wrapper from `root` and drop references. Idempotent —
  /// a second `destroy()` is a no-op.
  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;
    // `Element.remove()` is supported on every browser engine; we
    // don't gate on parentNode because the wrapper is owned by the
    // renderer and we know it was appended in the constructor.
    this.wrapper.remove();
    this.rowEls = [];
  }

  /// Public read-only handle to the wrapper element — useful for
  /// scrolling, focus management, and other compose ops the
  /// application layer owns.
  get container(): HTMLElement {
    return this.wrapper;
  }

  // ─── Internal ───────────────────────────────────────────────────

  private reconcileRowContainers(targetRows: number): void {
    while (this.rowEls.length < targetRows) {
      const div = this.doc.createElement("div");
      div.className = "ciri-row";
      this.wrapper.appendChild(div);
      this.rowEls.push(div);
    }
    while (this.rowEls.length > targetRows) {
      const drop = this.rowEls.pop();
      if (drop !== undefined) drop.remove();
    }
  }

  private renderRow(grid: PaneGrid, row: number): void {
    const rowEl = this.rowEls[row];
    if (rowEl === undefined) return;
    const cells: PackedCell[] = grid.rowCells(row);
    const globalStart = grid.viewportGlobalIndex(row, 0);
    const runs = rowRuns(
      cells,
      globalStart,
      {
        graphemeExtras: grid.graphemeExtras,
        cellLinks: grid.cellLinks,
        linkMap: grid.linkMap,
      },
      this.theme,
    );

    // `replaceChildren` clears and appends in one DOM operation —
    // fewer reflows than the `innerHTML = ""; appendChild()` pattern,
    // and it works equally well in jsdom.
    const spans: Node[] = new Array(runs.length);
    for (let i = 0; i < runs.length; i += 1) {
      spans[i] = this.renderRun(runs[i]!);
    }
    rowEl.replaceChildren(...spans);
  }

  private renderRun(run: SgrRun): HTMLElement {
    const el = run.linkUri !== null
      ? this.doc.createElement("a")
      : this.doc.createElement("span");
    if (run.linkUri !== null && el instanceof HTMLAnchorElement) {
      el.href = run.linkUri;
      // OSC 8 links in real terminals open in a new context; mirror
      // that so a click can't accidentally navigate the host page.
      el.target = "_blank";
      el.rel = "noopener noreferrer";
    }
    // Bunch all style flags into one inline `style` assignment so the
    // DOM doesn't recompute repeatedly. JSDOM accepts both
    // `style.cssText = ...` and individual property setters; we use
    // setters here because `cssText` requires fully-formed CSS strings
    // and is fussier about quoting.
    el.style.color = run.fg;
    el.style.backgroundColor = run.bg;
    if (run.bold) el.style.fontWeight = "bold";
    if (run.italic) el.style.fontStyle = "italic";
    if (run.strikeout) el.style.textDecorationLine = "line-through";
    if (run.underline !== "none") {
      // Combine with strikeout if both are set — the spec allows
      // `text-decoration-line` to take multiple values.
      const line = run.strikeout ? "underline line-through" : "underline";
      el.style.textDecorationLine = line;
      el.style.textDecorationStyle = mapUnderlineStyle(run.underline);
    }
    el.textContent = run.text;
    return el;
  }
}

function mapUnderlineStyle(
  s: "single" | "double" | "curly" | "dotted" | "dashed",
): string {
  switch (s) {
    case "single":
      return "solid";
    case "double":
      return "double";
    case "curly":
      return "wavy";
    case "dotted":
      return "dotted";
    case "dashed":
      return "dashed";
  }
}
