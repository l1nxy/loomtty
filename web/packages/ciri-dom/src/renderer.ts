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
  /// Rows above the live bottom of the viewport. `0` pins the display
  /// to the current grid (the default). `N > 0` shifts the display up
  /// by N rows so the top of the viewport shows scrollback rows;
  /// equivalent to "user scrolled back by N rows" in a typical
  /// terminal UI. Clamped to `grid.scrollbackRows` at render time.
  private scrollOffset = 0;
  /// Last `grid.scrollbackRows` observed by `render()`. When scrollback
  /// grows under a scrolled-up user, `scrollOffset` is bumped by the
  /// delta so the same historical rows stay visible (otherwise
  /// `srcRow = scrollbackRows - scrollOffset + displayRow` would drift
  /// toward the live bottom on every append). `null` means "not yet
  /// observed" — the first render skips the delta check.
  private lastScrollbackRows: number | null = null;

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
    // Keep the user's scrolled-up view locked when scrollback grows
    // underneath them: a FullPaneSync that appends N new history rows
    // shifts every historical row's absolute index forward by N, so
    // without a matching bump to `scrollOffset` the display drifts
    // toward live. When the user is pinned at the live bottom
    // (scrollOffset === 0) we leave it alone so they keep following
    // new output. Symmetric on shrink (eviction trim): pull the
    // offset back so the same rows stay visible if they still exist.
    if (this.lastScrollbackRows !== null) {
      const delta = grid.scrollbackRows - this.lastScrollbackRows;
      if (this.scrollOffset > 0 && delta !== 0) {
        this.scrollOffset = Math.max(0, this.scrollOffset + delta);
        this.rendererForcedRedraw = true;
      }
    }
    this.lastScrollbackRows = grid.scrollbackRows;
    // Clamp scrollOffset against the current grid: a FullPaneSync may
    // have shrunk scrollback to fewer rows than the user previously
    // scrolled into, and `setScrollOffset` couldn't see that yet. Any
    // adjustment also forces a full repaint because the visible window
    // shifted.
    if (this.scrollOffset > grid.scrollbackRows) {
      this.scrollOffset = grid.scrollbackRows;
      this.rendererForcedRedraw = true;
    }
    const dirty = grid.takeDirtyRows();
    // A theme change (or a scroll-offset clamp / setScrollOffset call)
    // forces a complete repaint even when the grid has no pending
    // damage of its own — every existing span's inline color came from
    // the old theme, and a scroll shift remaps every display row.
    const themeRedraw = this.rendererForcedRedraw;
    this.rendererForcedRedraw = false;
    const fullRedraw = dirty.fullRedraw || themeRedraw;
    if (fullRedraw || this.rowEls.length !== grid.rows) {
      this.reconcileRowContainers(grid.rows);
    }
    let displayRowsToRender: number[];
    if (fullRedraw) {
      // Repaint every display row regardless of dirty diff.
      displayRowsToRender = new Array(grid.rows);
      for (let i = 0; i < grid.rows; i += 1) displayRowsToRender[i] = i;
    } else {
      // Translate viewport-row damage into display-row positions.
      // A viewport row `r` shows at display row `r + scrollOffset`
      // (because scrolling up shifts existing viewport content down on
      // the screen). When offset = 0 this is a no-op pass-through; when
      // offset > 0 some dirty rows fall below the visible window and
      // are skipped.
      displayRowsToRender = [];
      for (const r of dirty.rows) {
        const d = r + this.scrollOffset;
        if (d >= 0 && d < grid.rows) displayRowsToRender.push(d);
      }
    }
    for (const d of displayRowsToRender) {
      if (d < 0 || d >= grid.rows) continue;
      this.renderDisplayRow(grid, d);
    }
  }

  /// Set the number of rows to shift the display upward into
  /// scrollback. `0` pins to the live viewport bottom (default).
  /// Clamped to `[0, +∞)` here; the actual upper bound is enforced
  /// against `grid.scrollbackRows` at render time, since the renderer
  /// has no live handle on a grid.
  setScrollOffset(rows: number): void {
    const clamped = Math.max(0, Math.floor(rows));
    if (clamped === this.scrollOffset) return;
    this.scrollOffset = clamped;
    this.rendererForcedRedraw = true;
  }

  /// Current scroll offset (rows above the live bottom).
  get scrollOffsetRows(): number {
    return this.scrollOffset;
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

  private renderDisplayRow(grid: PaneGrid, displayRow: number): void {
    const rowEl = this.rowEls[displayRow];
    if (rowEl === undefined) return;
    // Display row → source row in the concatenated buffer:
    //   srcRow = scrollbackRows - scrollOffset + displayRow
    // When scrollOffset = 0 and scrollbackRows = 0 this is just the
    // viewport row; the offset/scrollback math only kicks in when the
    // user has scrolled up.
    const srcRow = grid.scrollbackRows - this.scrollOffset + displayRow;
    // Guard the source-row math: a stale grid handed to a renderer
    // with a now-out-of-range offset would otherwise throw inside
    // `combinedRowCells`. We've already clamped at render-time entry,
    // but defend against per-cell math edge cases (e.g. scrollback
    // wiped between clamp and this call by a re-entrant flow).
    if (srcRow < 0 || srcRow >= grid.totalRows()) return;
    const cells: PackedCell[] = grid.combinedRowCells(srcRow);
    const globalStart = srcRow * grid.cols;
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
    // OSC 8 URIs come from untrusted terminal output. A malicious app
    // could emit `\e]8;;javascript:alert(1)\e\\` and turn the rendered
    // text into a clickable XSS vector — `target=_blank` doesn't help
    // here because `href` is evaluated on click. Restrict to a small
    // allowlist of harmless schemes; anything else falls through to a
    // plain `<span>` (the run text still renders, just not clickable).
    const safeLink = run.linkUri !== null && isSafeLinkScheme(run.linkUri)
      ? run.linkUri
      : null;
    const el = safeLink !== null
      ? this.doc.createElement("a")
      : this.doc.createElement("span");
    if (safeLink !== null && el instanceof HTMLAnchorElement) {
      el.href = safeLink;
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

// Schemes the OSC 8 renderer is willing to make clickable. Restricted
// to navigation forms that a typical user expects from a terminal: web
// links, email, file transfer. Everything else (`javascript:`, `data:`,
// `vbscript:`, `file:`, `chrome:` …) is rendered as plain text.
//
// `tel:` and `sms:` are deliberately omitted — the desktop browser has
// no useful behavior for them and they're a small but non-zero attack
// surface against the host's "open link" dispatch. Add them back if a
// real use case appears.
const SAFE_LINK_SCHEMES = ["http", "https", "ftp", "ftps", "mailto"];

function isSafeLinkScheme(uri: string): boolean {
  // Parse via WHATWG URL with a known-safe base — letting the URL
  // constructor handle protocol-relative and oddly-cased schemes for
  // us. Anything that fails to parse cleanly (raw garbage, fragment-
  // only, embedded control chars) falls through as not-safe.
  let parsed: URL;
  try {
    parsed = new URL(uri);
  } catch {
    return false;
  }
  // `URL.protocol` returns the scheme with a trailing colon, e.g. "https:".
  const scheme = parsed.protocol.replace(/:$/, "").toLowerCase();
  return SAFE_LINK_SCHEMES.includes(scheme);
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
