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

import {
  CURSOR_BEAM,
  CURSOR_BLOCK,
  CURSOR_HIDDEN,
  CURSOR_HOLLOW_BLOCK,
  CURSOR_UNDERLINE,
  NAMED_CURSOR,
  type PackedCell,
} from "@ciri/codec";
import {
  normalizeSelectionEnds,
  selectionRangeEquals,
  type PaneGrid,
  type SelectionRange,
} from "./grid.js";
import {
  DEFAULT_SELECTION_BACKGROUND,
  DEFAULT_THEME,
  type Theme,
} from "./theme.js";
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
  /// Blink the cursor via the Web Animations API. Defaults to `true`.
  /// Disabling produces a static cursor that may be easier for users
  /// who find blinking distracting; callers can also override with a
  /// `prefers-reduced-motion` media query at the page level.
  cursorBlink?: boolean;
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
  /// Cursor overlay element. Absolute-positioned inside the wrapper;
  /// hidden until the first render with a usable meta. Position is
  /// driven by `grid.meta.cursorLine` / `cursorCol`, sized via
  /// CSS `ch` (column width) + `em` line height so the overlay tracks
  /// the same metrics the cell grid lays out against.
  private readonly cursorEl: HTMLElement;
  private readonly cursorBlink: boolean;
  /// Set when the cursor is actively blinking — used by `destroy()`
  /// to cancel the Web Animations API handle.
  private cursorAnim: Animation | null = null;
  /// Active selection (or `null` for no selection). Rendered as a set
  /// of absolute-positioned overlay rects layered on top of the cell
  /// grid. The renderer doesn't own the *interpretation* of the
  /// selection (e.g. "what text does this cover"); it just paints
  /// rects per its anchors. `app` derives text via `grid.extractText`.
  private selection: SelectionRange | null = null;
  private readonly selectionRowEls: HTMLElement[] = [];
  /// IME pre-edit overlay. Shown above cell content at the cursor's
  /// position while the user is composing a glyph through an input
  /// method (Pinyin, Kana, Hangul Jamo, dead-key chains, …). Text is
  /// driven by `setPreedit`; positioning tracks `grid.meta.cursorLine`
  /// / `cursorCol` so the box always lines up with where the commit
  /// will land. The actual commit goes through `client.sendInput`
  /// from the application layer (this renderer never touches the
  /// wire).
  private readonly preeditEl: HTMLElement;
  /// Active preedit content (`null` when no composition in flight).
  private preedit: { text: string } | null = null;

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
    // Make the wrapper the positioning context for the cursor
    // overlay (which is absolute-positioned inside it).
    this.wrapper.style.position = "relative";
    // The cell font and the cell size combine to set the character
    // grid; keeping line-height a fixed ratio (1.2) means a future
    // resize math layer can derive cellHeight from the rendered
    // font-size without a `getBoundingClientRect` roundtrip.
    this.root.appendChild(this.wrapper);

    this.cursorBlink = opts.cursorBlink ?? true;
    this.cursorEl = this.doc.createElement("div");
    this.cursorEl.className = "ciri-cursor";
    this.cursorEl.style.position = "absolute";
    this.cursorEl.style.pointerEvents = "none";
    this.cursorEl.style.boxSizing = "border-box";
    this.cursorEl.style.display = "none";
    this.wrapper.appendChild(this.cursorEl);

    // IME pre-edit overlay. Painted on top of cells but under the
    // cursor caret would normally be — so we append AFTER the cursor
    // and rely on later-sibling z-stacking. (Cursor blink animates
    // opacity, not z-index, so the preedit sits above the dimmed
    // cursor cell without flicker.) The visual treatment mirrors the
    // native Rust client's `ImePreeditComponent`: dark translucent
    // box, light text, blue underline.
    this.preeditEl = this.doc.createElement("div");
    this.preeditEl.className = "ciri-preedit";
    this.preeditEl.style.position = "absolute";
    this.preeditEl.style.pointerEvents = "none";
    this.preeditEl.style.display = "none";
    this.preeditEl.style.boxSizing = "content-box";
    this.preeditEl.style.backgroundColor = "rgba(38, 38, 64, 0.95)";
    this.preeditEl.style.color = "#ffffff";
    this.preeditEl.style.borderBottom = "2px solid #80b3ff";
    this.preeditEl.style.whiteSpace = "pre";
    this.wrapper.appendChild(this.preeditEl);
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
    // Cursor sits on top of the cell grid; refresh on every render so
    // a CellDelta that only updated meta (cursor moved without
    // visible cell churn) still repositions the overlay. Cheap —
    // mostly a handful of inline-style assignments.
    this.updateCursor(grid);
    // Selection overlay last so it lays cleanly over the cursor's
    // shape (visually consistent with native terminals — a selection
    // includes the cursor cell).
    this.updateSelection(grid);
    // Pre-edit overlay sits on top of the cursor + selection: the
    // composing glyph must be visible even when its target cell falls
    // inside an active selection (rare in practice but possible).
    this.updatePreedit(grid);
  }

  /// Set (or clear, with `null`) the selection overlay. The renderer
  /// repaints the overlay on the next `render()` call; calling
  /// `setSelection` does NOT immediately repaint so callers can
  /// coalesce a selection update with the next CellDelta-driven
  /// render.
  setSelection(range: SelectionRange | null): void {
    if (selectionRangeEquals(this.selection, range)) return;
    this.selection = range === null ? null : { ...range };
  }

  /// Current selection (or `null`). Read-only view for tests and
  /// app-level introspection.
  get currentSelection(): SelectionRange | null {
    return this.selection;
  }

  /// Set (or clear, with `null` / empty string) the IME pre-edit
  /// overlay. The renderer repaints the overlay on the next `render()`
  /// call. Empty string clears (matches winit's `Ime::Preedit("", ...)`
  /// idiom for "composition ended without a commit"). Passing the
  /// same text twice in a row is a no-op.
  setPreedit(text: string | null): void {
    if (text === null || text.length === 0) {
      if (this.preedit === null) return;
      this.preedit = null;
      return;
    }
    if (this.preedit !== null && this.preedit.text === text) return;
    this.preedit = { text };
  }

  /// Current pre-edit text (or `null`). Read-only view for tests and
  /// app-level introspection.
  get currentPreedit(): string | null {
    return this.preedit === null ? null : this.preedit.text;
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
    this.stopCursorBlink();
    // `Element.remove()` is supported on every browser engine; we
    // don't gate on parentNode because the wrapper is owned by the
    // renderer and we know it was appended in the constructor.
    this.wrapper.remove();
    this.rowEls = [];
    this.selectionRowEls.length = 0;
    this.selection = null;
    this.preedit = null;
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

  private updateCursor(grid: PaneGrid): void {
    const { cursorLine, cursorCol, cursorShape } = grid.meta;
    // Cursor belongs to the live viewport row; it has no meaning over
    // scrollback. Map viewport row → display row using the same
    // scrollOffset math used by `renderDisplayRow`.
    const displayRow = cursorLine + this.scrollOffset;
    if (
      cursorShape === CURSOR_HIDDEN ||
      displayRow < 0 ||
      displayRow >= grid.rows ||
      cursorCol < 0 ||
      cursorCol >= grid.cols
    ) {
      this.cursorEl.style.display = "none";
      this.stopCursorBlink();
      return;
    }

    const color =
      this.theme.named[NAMED_CURSOR] ?? this.theme.foreground;
    // Reset prior shape styling so a shape change (block → beam etc.)
    // doesn't leak the previous look. We re-apply per-shape below.
    this.cursorEl.style.backgroundColor = "";
    this.cursorEl.style.border = "";
    this.cursorEl.style.opacity = "";
    this.cursorEl.style.left = `${cursorCol}ch`;
    // Multiply on the JS side; JSDOM (and at least one Chromium build)
    // normalizes `calc(N * 1.2em)` down to `calc(N.Mem)`, so emitting
    // the resolved value avoids round-trip surprises in the inline
    // `style.top` string.
    this.cursorEl.style.top = `${displayRow * 1.2}em`;
    this.cursorEl.style.width = "1ch";
    this.cursorEl.style.height = "1.2em";
    this.cursorEl.style.display = "block";

    switch (cursorShape) {
      case CURSOR_BLOCK:
        this.cursorEl.style.backgroundColor = color;
        // Semi-transparent so the underlying glyph remains legible
        // (real terminals invert the glyph color, but that requires
        // touching the cell's span; matched-Rust opaque would hide
        // the cell. 0.6 is the best-effort fallback for v1).
        this.cursorEl.style.opacity = "0.6";
        break;
      case CURSOR_UNDERLINE:
        // 2px stripe at the cell bottom. Use a thick border-bottom
        // rather than positioning a separate sub-element.
        this.cursorEl.style.border = `0`;
        this.cursorEl.style.borderBottom = `2px solid ${color}`;
        break;
      case CURSOR_BEAM:
        this.cursorEl.style.width = "2px";
        this.cursorEl.style.backgroundColor = color;
        break;
      case CURSOR_HOLLOW_BLOCK:
        this.cursorEl.style.border = `1px solid ${color}`;
        break;
      default:
        // Unknown shape — fall back to a translucent block so the
        // user sees *something* rather than an invisible cursor.
        this.cursorEl.style.backgroundColor = color;
        this.cursorEl.style.opacity = "0.6";
        break;
    }
    // Class tag for caller-level styling overrides (e.g. theme
    // designers who want a custom beam thickness).
    this.cursorEl.className = `ciri-cursor ciri-cursor-${cursorShapeName(cursorShape)}`;
    this.startCursorBlinkIfEnabled();
  }

  private startCursorBlinkIfEnabled(): void {
    if (!this.cursorBlink) return;
    if (this.cursorAnim !== null) return;
    // Web Animations API. Not all hosts ship it (older jsdom doesn't)
    // — fall back to a static cursor on those.
    if (typeof (this.cursorEl as Element).animate !== "function") return;
    this.cursorAnim = this.cursorEl.animate(
      [
        { opacity: this.cursorEl.style.opacity || "1" },
        { opacity: this.cursorEl.style.opacity || "1" },
        { opacity: "0" },
        { opacity: "0" },
      ],
      { duration: 1000, iterations: Infinity, easing: "steps(1, end)" },
    );
  }

  private updateSelection(grid: PaneGrid): void {
    // Rebuild row rects from scratch. The set is bounded by viewport
    // rows so even a full-screen selection costs O(rows) divs —
    // cheaper than maintaining a row-keyed map with diffing.
    for (const el of this.selectionRowEls) el.remove();
    this.selectionRowEls.length = 0;
    if (this.selection === null) return;

    const [first, last] = normalizeSelectionEnds(
      this.selection.start,
      this.selection.end,
    );
    const bg = this.theme.selectionBackground ?? DEFAULT_SELECTION_BACKGROUND;
    for (let srcRow = first.srcRow; srcRow <= last.srcRow; srcRow += 1) {
      // Same display-row mapping as `renderDisplayRow`.
      const displayRow = srcRow - grid.scrollbackRows + this.scrollOffset;
      if (displayRow < 0 || displayRow >= grid.rows) continue;
      const left = srcRow === first.srcRow ? Math.max(0, first.col) : 0;
      const rawRight =
        srcRow === last.srcRow ? Math.min(grid.cols - 1, last.col) : grid.cols - 1;
      if (rawRight < left) continue;
      const el = this.doc.createElement("div");
      el.className = "ciri-selection-row";
      el.style.position = "absolute";
      el.style.pointerEvents = "none";
      el.style.left = `${left}ch`;
      el.style.top = `${displayRow * 1.2}em`;
      el.style.width = `${rawRight - left + 1}ch`;
      el.style.height = "1.2em";
      el.style.backgroundColor = bg;
      this.wrapper.appendChild(el);
      this.selectionRowEls.push(el);
    }
  }

  private updatePreedit(grid: PaneGrid): void {
    if (this.preedit === null) {
      this.preeditEl.style.display = "none";
      this.preeditEl.textContent = "";
      return;
    }
    const { cursorLine, cursorCol } = grid.meta;
    // Pre-edit anchors to the cursor cell, same scroll-offset math as
    // `updateCursor`. If the cursor has scrolled out of the visible
    // viewport (user paged back into scrollback), hide the overlay —
    // the commit will still land at the live position, but visually
    // showing the preedit on top of unrelated scrollback rows is more
    // confusing than helpful.
    const displayRow = cursorLine + this.scrollOffset;
    if (
      displayRow < 0 ||
      displayRow >= grid.rows ||
      cursorCol < 0 ||
      cursorCol >= grid.cols
    ) {
      this.preeditEl.style.display = "none";
      return;
    }
    const cols = Math.max(1, displayWidth(this.preedit.text));
    this.preeditEl.textContent = this.preedit.text;
    this.preeditEl.style.left = `${cursorCol}ch`;
    this.preeditEl.style.top = `${displayRow * 1.2}em`;
    this.preeditEl.style.width = `${cols}ch`;
    this.preeditEl.style.height = "1.2em";
    this.preeditEl.style.display = "block";
  }

  private stopCursorBlink(): void {
    if (this.cursorAnim !== null) {
      this.cursorAnim.cancel();
      this.cursorAnim = null;
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

/// Class-suffix string for a cursor shape constant, used to tag the
/// overlay element so callers can override per-shape styling in CSS.
function cursorShapeName(shape: number): string {
  switch (shape) {
    case CURSOR_BLOCK:
      return "block";
    case CURSOR_UNDERLINE:
      return "underline";
    case CURSOR_BEAM:
      return "beam";
    case CURSOR_HIDDEN:
      return "hidden";
    case CURSOR_HOLLOW_BLOCK:
      return "hollow";
    default:
      return "unknown";
  }
}

/// Best-effort display width for an IME pre-edit string in terminal
/// cells. Mirrors `unicode-width::UnicodeWidthStr::width` for the
/// ranges the input methods we care about actually produce — full
/// CJK Unified Ideographs, kana, Hangul syllables, fullwidth ASCII,
/// the supplementary CJK extensions. Combining marks and zero-width
/// joiners contribute zero so they don't push the box wider than the
/// final glyph cluster. Anything outside the table contributes 1
/// (the safe default for Latin and most non-wide scripts).
///
/// This function never has to be *exactly* right — the preedit box
/// is a visual hint, not a layout-critical measurement. Off-by-one
/// at extremes (some emoji, ZWJ sequences) is acceptable. The wire
/// commit goes through the regular UTF-8 path; this width math only
/// affects the local overlay.
function displayWidth(s: string): number {
  let w = 0;
  for (const ch of s) {
    const cp = ch.codePointAt(0)!;
    if (
      // Combining marks (Mn).
      (cp >= 0x0300 && cp <= 0x036f) ||
      // Zero-width / direction marks.
      (cp >= 0x200b && cp <= 0x200f) ||
      cp === 0xfeff
    ) {
      // width 0
    } else if (
      (cp >= 0x1100 && cp <= 0x115f) || // Hangul Jamo init.
      (cp >= 0x2e80 && cp <= 0x303e) || // CJK Radicals / Kangxi / punctuation
      (cp >= 0x3041 && cp <= 0x33ff) || // Hiragana, Katakana, Bopomofo, Hangul Compat …
      (cp >= 0x3400 && cp <= 0x4dbf) || // CJK Extension A
      (cp >= 0x4e00 && cp <= 0x9fff) || // CJK Unified Ideographs
      (cp >= 0xa000 && cp <= 0xa4cf) || // Yi
      (cp >= 0xac00 && cp <= 0xd7a3) || // Hangul Syllables
      (cp >= 0xf900 && cp <= 0xfaff) || // CJK Compatibility Ideographs
      (cp >= 0xfe30 && cp <= 0xfe4f) || // CJK Compatibility Forms
      (cp >= 0xff00 && cp <= 0xff60) || // Fullwidth ASCII
      (cp >= 0xffe0 && cp <= 0xffe6) || // Fullwidth signs
      (cp >= 0x20000 && cp <= 0x2fffd) || // CJK Ext B..F
      (cp >= 0x30000 && cp <= 0x3fffd) // CJK Ext G
    ) {
      w += 2;
    } else {
      w += 1;
    }
  }
  return w;
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
