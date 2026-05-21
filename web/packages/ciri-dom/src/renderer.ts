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
  MODE_MOUSE_REPORT,
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
  /// Inline images (sixel / kitty / iTerm), keyed by `imageId`. Each
  /// entry is a `<canvas>` painted once with the server-decoded RGBA
  /// pixels and CSS-scaled to its cell box. `srcRow` is the absolute
  /// combined-buffer row (scrollback + viewport) captured at placement
  /// time, so `updateImages` can scroll/clip it exactly like a cell
  /// run. The server clears all of a pane's images via `ImageDeleted`
  /// (→ `clearImages`); re-placing the same `imageId` updates in place.
  private readonly images = new Map<
    string,
    {
      canvas: HTMLCanvasElement;
      srcRow: number;
      col: number;
      widthCells: number;
      heightCells: number;
    }
  >();
  /// `grid.scrollbackTrimmed` / `grid.scrollbackEpoch` as of the last
  /// `render()`. Diffed each render to keep image `srcRow` anchors valid
  /// when scrollback front-trims (rebase down) or is replaced wholesale
  /// (drop — the anchor's buffer identity is gone). `null` until the
  /// first render seeds them. See `rebaseImagesForScrollback`.
  private lastScrollbackTrimmed: number | null = null;
  private lastScrollbackEpoch: number | null = null;
  /// IME pre-edit overlay. Shown above cell content at the cursor's
  /// position while the user is composing a glyph through an input
  /// method (Pinyin, Kana, Hangul Jamo, dead-key chains, …). Text is
  /// driven by `setPreedit`; positioning tracks `grid.meta.cursorLine`
  /// / `cursorCol` so the box always lines up with where the commit
  /// will land. The actual commit goes through `client.sendInput`
  /// from the application layer (this renderer never touches the
  /// wire).
  ///
  /// Caret-within-preedit gap (Rust parity): the native client
  /// renders a caret *inside* the preedit panel at the IME's
  /// reported cursor position (winit's
  /// `Ime::Preedit(text, Some((start, end)))` → `preedit_cursor` in
  /// `crates/ciri/src/app/ime.rs:45-48`). The browser DOM
  /// `CompositionEvent` interface has no equivalent: there is no
  /// standard property surfacing the in-preedit caret position. The
  /// newer `EditContext` API does expose it but is Chrome-only and
  /// behind an opt-in. Until EditContext is broadly available we
  /// render only the text run with a trailing underline; consumers
  /// who need the in-preedit caret can layer it themselves once a
  /// richer composition source is plumbed in.
  private readonly preeditEl: HTMLElement;
  /// Active preedit content (`null` when no composition in flight).
  private preedit: { text: string } | null = null;
  /// Vertical overlay scrollbar. Mounted absolute-positioned on the
  /// right edge of the wrapper. Track holds a thumb whose top + height
  /// reflect the current `scrollOffset` against `grid.scrollbackRows`.
  /// Hidden via `style.display = none` when:
  ///   - the grid has no scrollback rows (`scrollbackRows === 0`)
  ///   - the pane reports mouse (`grid.meta.modeFlags & MODE_MOUSE_REPORT`),
  ///     because mouse-aware TUIs (tmux / vim / htop / …) own their
  ///     own scrollback via copy-mode; an external scrollbar here
  ///     would mislead the user into thinking they can drag it.
  ///
  /// Both `pointerdown` on the thumb (drag) and `pointerdown` on the
  /// surrounding track (page jump) route to `setScrollOffset` so the
  /// downstream render uses the same code path as the wheel.
  private readonly scrollbarEl: HTMLElement;
  private readonly scrollbarThumbEl: HTMLElement;
  /// In-flight thumb drag state. `null` when not dragging. The drag
  /// captures the pointer on its element so a finger that leaves the
  /// thumb mid-drag still keeps updating the offset.
  private scrollbarDrag: {
    /// clientY at pointerdown.
    startClientY: number;
    /// `scrollOffset` at pointerdown.
    startScrollOffset: number;
    /// Pointer id we captured.
    pointerId: number;
    /// `scrollbackRows` snapshot at drag-start. We deliberately
    /// don't react to scrollback growth during drag — keeping the
    /// scale stable is the only way the thumb can track the finger
    /// 1:1.
    startScrollbackRows: number;
    /// Track-usable length (`track_height - thumb_height`) at
    /// drag-start. Same rationale.
    startTrackUsablePx: number;
  } | null = null;
  /// `true` if the latest `render()` decided the scrollbar should be
  /// visible. Cached so the pointer-down handler can short-circuit
  /// without re-doing the visibility math; cleared by `updateScrollbar`.
  private scrollbarVisible = false;
  /// Grid reference snapshot, set on every `render()` so the
  /// scrollbar event handlers can resolve `grid.scrollbackRows` etc.
  /// without re-plumbing the grid through every callback.
  private gridRef: PaneGrid | null = null;
  /// Auto-hide timer for the scrollbar. The thumb is invisible by
  /// default (CSS `opacity: 0`); on any scroll-related interaction
  /// (wheel via `setScrollOffset`, swipe, scrollbar drag, track
  /// click) we add `ciri-scrollbar-active`, reset this timer, and
  /// remove the class after the idle window. CSS `:hover` provides
  /// the desktop discovery affordance independent of this timer.
  private scrollbarHideTimer: ReturnType<typeof setTimeout> | null = null;
  /// In-flight one-finger touch swipe state. On non-mouse-mode panes
  /// a vertical finger drag scrolls the scrollback (drag DOWN reveals
  /// history, drag UP returns toward live bottom — matches the
  /// platform `overflow: scroll` convention). Decision to commit
  /// happens on the first non-trivial move; before that we leave the
  /// synthesized mouse events alone so a quick tap still works as
  /// focus / chip-click. Mouse-mode panes (tmux / vim / …) keep
  /// their touch unforwarded; the TUI gets the synthesized mouse
  /// events via `app.ts`'s `mouseForward` path.
  /// When `true`, the app has taken over the current touch for text
  /// selection (after a long-press), so the swipe-scroll handlers stand
  /// down — otherwise a select-drag would also scroll the buffer.
  /// Toggled by `setTouchSelecting`.
  private touchSelecting = false;
  private touchScrollState:
    | {
        startY: number;
        lastY: number;
        startX: number;
        lastMoveTime: number;
        pointerId: number;
        committed: boolean;
        /// Fractional row accumulator. A 4px finger move at a 16px
        /// row height is 0.25 rows — `Math.floor`ing each tick
        /// loses those fractions; we accumulate and apply whole
        /// rows when they cross 1.
        accumRows: number;
      }
    | null = null;
  /// Smoothed finger velocity at pointerup time, in rows-per-second.
  /// Updated on every `pointermove` via an EMA so the value is
  /// resilient to one-frame stutters. The pointerup handler reads
  /// this to decide whether to kick a momentum animation.
  private touchScrollVel = 0;
  /// requestAnimationFrame handle for the momentum-decay loop.
  /// `null` outside an active fling.
  private touchMomentumRaf: number | null = null;
  /// Last `requestAnimationFrame` timestamp during a fling. Used to
  /// compute frame-time-independent velocity decay (without this
  /// the animation would feel different on 60 / 90 / 120 fps
  /// displays).
  private touchMomentumLastTime = 0;
  /// Fractional row accumulator for the momentum loop — same role
  /// as `touchScrollState.accumRows` but kept separate so the
  /// fling can continue after the gesture state is cleared.
  private touchMomentumAccum = 0;
  /// rAF handle for scroll-driven render coalescing. Mobile (and
  /// fast trackpads) fire `pointermove` faster than the display
  /// refresh rate; without batching, each fractional-pixel move
  /// would call `render()` which rebuilds every visible row — the
  /// scroll feels visibly choppy because we're spending the main
  /// thread on DOM thrash instead of letting the browser paint.
  /// `scheduleRender` sets this and only re-renders once per
  /// animation frame, no matter how many pointermoves landed.
  private pendingScrollRenderRaf: number | null = null;

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

    // Vertical overlay scrollbar — track (full pane height) + thumb
    // (proportional to visible/total ratio, clamped at a minimum).
    // The track sits on the right edge of the wrapper as an
    // absolutely-positioned overlay; cell rendering keeps full
    // width and the thumb floats above it (default opacity is
    // partial so cells underneath stay readable). Themes can
    // restyle via `.ciri-scrollbar` / `.ciri-scrollbar-thumb`.
    this.scrollbarEl = this.doc.createElement("div");
    this.scrollbarEl.className = "ciri-scrollbar";
    this.scrollbarEl.style.position = "absolute";
    this.scrollbarEl.style.right = "0";
    this.scrollbarEl.style.top = "0";
    this.scrollbarEl.style.bottom = "0";
    this.scrollbarEl.style.display = "none";
    // Allow touch / pointer events on the track for page-jump, but
    // don't let the browser claim them for scroll/zoom gestures.
    this.scrollbarEl.style.touchAction = "none";
    this.wrapper.appendChild(this.scrollbarEl);

    this.scrollbarThumbEl = this.doc.createElement("div");
    this.scrollbarThumbEl.className = "ciri-scrollbar-thumb";
    this.scrollbarThumbEl.style.position = "absolute";
    // NOTE: only set `right` here. Inline `left: 0; right: 0`
    // together makes the thumb stretch the full 12px track width
    // and silently drops the CSS-declared `width` — the thumb
    // would render mid-track instead of hugging the right edge.
    this.scrollbarThumbEl.style.right = "0";
    this.scrollbarThumbEl.style.touchAction = "none";
    this.scrollbarEl.appendChild(this.scrollbarThumbEl);

    this.scrollbarThumbEl.addEventListener("pointerdown", (e) =>
      this.onScrollbarThumbPointerDown(e),
    );
    this.scrollbarThumbEl.addEventListener("pointermove", (e) =>
      this.onScrollbarThumbPointerMove(e),
    );
    this.scrollbarThumbEl.addEventListener("pointerup", (e) =>
      this.onScrollbarThumbPointerUp(e),
    );
    this.scrollbarThumbEl.addEventListener("pointercancel", (e) =>
      this.onScrollbarThumbPointerUp(e),
    );
    // Track click → page jump in the direction of the click relative
    // to the thumb. Handled on the track (NOT thumb) so a click that
    // happens to land on the thumb starts a drag instead.
    this.scrollbarEl.addEventListener("pointerdown", (e) =>
      this.onScrollbarTrackPointerDown(e),
    );

    // Touch swipe vertical → scrollback. Listener on the wrapper
    // itself (NOT the scrollbar) so a finger that lands on the
    // pane content's body scrolls; a finger on the scrollbar
    // routes through the thumb/track handlers above.
    //
    // `passive: false` on EVERY pointer/touch path because we need
    // to call `preventDefault` to suppress the browser's
    // synthesized mouse events. Without that suppression, the
    // touch sequence ALSO fires `mousedown`/`mousemove` that
    // bubble out to `app.ts`'s root listener and start a text-
    // selection drag — fighting our scroll handler for the same
    // touch. (Native browser scroll panning isn't our concern
    // because `.ciri-pane` doesn't have `overflow: scroll`; what
    // we're blocking is purely compat mouse-event spawning.)
    // pointerdown stays `passive: true` — do NOT preventDefault on
    // it. Tap-to-focus on touch relies on the compat `mousedown`
    // bubbling to the tile's listener (`LayoutManager.setLayout`
    // wires that to `FocusPane`); preempting `mousedown` here
    // would silently break focus on every tap once the pane has
    // scrollback. Only after the gesture commits to a scroll
    // (see `pointermove`) do we preventDefault subsequent compat
    // events.
    this.wrapper.addEventListener(
      "pointerdown",
      (e) => this.onPanePointerDown(e),
      { passive: true },
    );
    this.wrapper.addEventListener(
      "pointermove",
      (e) => this.onPanePointerMove(e),
      { passive: false },
    );
    this.wrapper.addEventListener(
      "pointerup",
      (e) => this.onPanePointerUp(e),
      { passive: true },
    );
    this.wrapper.addEventListener(
      "pointercancel",
      (e) => this.onPanePointerCancel(e),
      { passive: true },
    );
    // `touch-action: none` declares to the browser that we own ALL
    // gestures on this element — no native pinch-zoom, no double-
    // tap-to-zoom, no fastclick delay. Required for the gesture
    // commit threshold below to feel responsive: with default
    // touch-action, mobile browsers add ~300ms of "what's this
    // gesture?" hesitation before firing pointer events to us.
    this.wrapper.style.touchAction = "none";
  }

  /// Minimum thumb height in pixels. Without a floor, a long scrollback
  /// produces a thumb so small the user can't grab it. 24 px is the
  /// established mobile-touch hit minimum (matches the chip strip's
  /// `--hit-cozy`).
  private static readonly SCROLLBAR_MIN_THUMB_PX = 24;

  /// Inline clamp — `Math.min(max, Math.max(min, n))` reads
  /// awkwardly when `max < min` is a real edge case (track height 0
  /// for a zero-size pane). Returns `min` for inverted ranges, same
  /// shape as `clampNumber` in `@ciri/app`.
  private static clamp(n: number, min: number, max: number): number {
    if (max < min) return min;
    return n < min ? min : n > max ? max : n;
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
    // Inline images: first rebase anchors against any scrollback
    // trim/replace since the last render, then reposition for the
    // current scroll offset (they scroll with the buffer like cells).
    this.rebaseImagesForScrollback(grid);
    this.updateImages(grid);
    // Scrollbar last: it sits on top of everything and is purely a
    // navigation affordance. `gridRef` is also stashed here so the
    // drag handlers can resolve `grid.scrollbackRows` etc. without
    // a callback closure.
    this.gridRef = grid;
    this.updateScrollbar(grid);
  }

  private updateScrollbar(grid: PaneGrid): void {
    const mouseMode = (grid.meta.modeFlags & MODE_MOUSE_REPORT) !== 0;
    const visible = !mouseMode && grid.scrollbackRows > 0;
    this.scrollbarVisible = visible;
    if (!visible) {
      this.scrollbarEl.style.display = "none";
      return;
    }
    this.scrollbarEl.style.display = "block";
    const trackRect = this.scrollbarEl.getBoundingClientRect();
    const trackH = trackRect.height;
    if (trackH <= 0) {
      // The pane hasn't been laid out yet (initial render, or
      // mounted into an `display: none` container). Nothing useful
      // we can compute — bail until the next render with real
      // dimensions.
      return;
    }
    const total = grid.scrollbackRows + grid.rows;
    const visibleRows = grid.rows;
    const thumbRatio = total > 0 ? visibleRows / total : 1;
    const thumbH = Math.max(
      PaneRenderer.SCROLLBAR_MIN_THUMB_PX,
      Math.floor(thumbRatio * trackH),
    );
    // `scrollOffset === 0` → live bottom → thumb at the bottom of
    // the track. `scrollOffset === scrollbackRows` → thumb at top.
    const trackUsable = Math.max(0, trackH - thumbH);
    const topFraction =
      grid.scrollbackRows > 0
        ? 1 - this.scrollOffset / grid.scrollbackRows
        : 1;
    const top = Math.floor(trackUsable * topFraction);
    this.scrollbarThumbEl.style.top = `${top}px`;
    this.scrollbarThumbEl.style.height = `${thumbH}px`;
  }

  private onScrollbarThumbPointerDown(e: PointerEvent): void {
    if (!this.scrollbarVisible || this.gridRef === null) return;
    e.preventDefault();
    e.stopPropagation();
    this.scrollbarThumbEl.setPointerCapture(e.pointerId);
    const trackRect = this.scrollbarEl.getBoundingClientRect();
    const thumbRect = this.scrollbarThumbEl.getBoundingClientRect();
    this.scrollbarDrag = {
      startClientY: e.clientY,
      startScrollOffset: this.scrollOffset,
      pointerId: e.pointerId,
      startScrollbackRows: this.gridRef.scrollbackRows,
      startTrackUsablePx: Math.max(1, trackRect.height - thumbRect.height),
    };
    this.scrollbarEl.classList.add("ciri-scrollbar-dragging");
    this.revealScrollbar();
  }

  private onScrollbarThumbPointerMove(e: PointerEvent): void {
    const drag = this.scrollbarDrag;
    if (drag === null || drag.pointerId !== e.pointerId) return;
    if (this.gridRef === null) return;
    const dy = e.clientY - drag.startClientY;
    // Track-px → scrollback-rows scale, locked to drag-start state so
    // newly-arriving PTY output during the drag doesn't warp the
    // thumb-to-finger mapping.
    const rowsPerPx = drag.startScrollbackRows / drag.startTrackUsablePx;
    // Dragging the thumb DOWN means moving toward the live bottom →
    // `scrollOffset` decreases. Hence the sign flip.
    // `Math.round` before assignment — `scrollOffset` flows into
    // `renderDisplayRow`'s integer `srcRow` math (and into
    // `combinedRowCells`'s array slicing). A fractional offset
    // would slice rows at non-row-aligned indices and corrupt the
    // visible scrollback for the duration of the drag.
    const nextOffset = Math.round(
      PaneRenderer.clamp(
        drag.startScrollOffset - dy * rowsPerPx,
        0,
        this.gridRef.scrollbackRows,
      ),
    );
    if (nextOffset !== this.scrollOffset) {
      this.scrollOffset = nextOffset;
      // Force a full repaint — every visible display row's srcRow
      // changes when scrollOffset moves, so the per-row dirty diff
      // doesn't cover this case on its own. Coalesce to rAF so a
      // burst of pointermove events doesn't pile up renders.
      this.rendererForcedRedraw = true;
      this.scheduleRender();
    }
    this.revealScrollbar();
  }

  private onScrollbarThumbPointerUp(e: PointerEvent): void {
    const drag = this.scrollbarDrag;
    if (drag === null || drag.pointerId !== e.pointerId) return;
    try {
      this.scrollbarThumbEl.releasePointerCapture(e.pointerId);
    } catch {
      // Releasing a capture that's already gone isn't an error worth
      // surfacing — happens when pointercancel races with pointerup.
    }
    this.scrollbarDrag = null;
    this.scrollbarEl.classList.remove("ciri-scrollbar-dragging");
  }

  private onScrollbarTrackPointerDown(e: PointerEvent): void {
    // Only react to clicks that landed on the track itself — clicks
    // on the thumb get their own handler (drag start).
    if (e.target !== this.scrollbarEl) return;
    if (!this.scrollbarVisible || this.gridRef === null) return;
    e.preventDefault();
    const trackRect = this.scrollbarEl.getBoundingClientRect();
    const thumbRect = this.scrollbarThumbEl.getBoundingClientRect();
    // Page jump in the direction of the click. Match common
    // scrollbar UX: clicking above the thumb scrolls UP by a
    // page (more history visible); clicking below scrolls DOWN.
    const pageRows = Math.max(1, this.gridRef.rows - 1);
    const nextOffset =
      e.clientY < thumbRect.top
        ? Math.min(
            this.gridRef.scrollbackRows,
            this.scrollOffset + pageRows,
          )
        : Math.max(0, this.scrollOffset - pageRows);
    if (nextOffset !== this.scrollOffset) {
      this.scrollOffset = nextOffset;
      this.rendererForcedRedraw = true;
      this.scheduleRender();
    }
    this.revealScrollbar();
  }

  /// How long the scrollbar stays visible after the last
  /// scroll-related interaction before auto-hiding. ~1.5s matches
  /// the iOS / macOS overlay-scrollbar idle window — long enough to
  /// see the new position settle, short enough to get out of the
  /// way of the content underneath.
  private static readonly SCROLLBAR_HIDE_AFTER_MS = 1500;

  /// Bring the scrollbar in (or keep it in), then schedule the
  /// idle fade-out. Idempotent — safe to call on every move.
  private revealScrollbar(): void {
    // No point fading anything in when there's no scrollback to
    // show or the pane is mouse-mode (`updateScrollbar` toggles
    // `display: none` for both cases). Bail to keep the idle
    // timer from ticking against a hidden element.
    if (!this.scrollbarVisible) return;
    this.scrollbarEl.classList.add("ciri-scrollbar-active");
    if (this.scrollbarHideTimer !== null) {
      clearTimeout(this.scrollbarHideTimer);
    }
    this.scrollbarHideTimer = setTimeout(() => {
      this.scrollbarHideTimer = null;
      // A drag in flight means the user is actively interacting —
      // keep the thumb visible until pointerup releases. Re-arm
      // the timer with a fresh window in case the drag finishes
      // before the next `revealScrollbar` call.
      if (this.scrollbarDrag !== null) {
        this.revealScrollbar();
        return;
      }
      this.scrollbarEl.classList.remove("ciri-scrollbar-active");
    }, PaneRenderer.SCROLLBAR_HIDE_AFTER_MS);
  }

  /// Pixel-distance the finger must travel before we commit to
  /// "this is a vertical scroll gesture". Small enough to feel
  /// responsive, large enough to filter taps.
  private static readonly TOUCH_SCROLL_COMMIT_PX = 8;
  /// Horizontal-vs-vertical ratio gate. Mirror of the chip strip's
  /// swipe-axis ratio: bias toward vertical so a slightly diagonal
  /// drag still scrolls instead of getting cancelled.
  private static readonly TOUCH_SCROLL_AXIS_RATIO = 1.5;
  /// Time constant for the momentum decay (seconds). Tuned for
  /// iOS-feel: τ = 0.2s gives ~37% of initial velocity at 200ms,
  /// ~5% at 600ms, ~0.7% at 1s. The "flick to scroll a screenful"
  /// arc users expect from native mobile lists.
  private static readonly TOUCH_MOMENTUM_TAU_SEC = 0.2;
  /// Below this velocity (rows/sec), stop the fling — anything
  /// slower is below visual perception and just burns CPU.
  private static readonly TOUCH_MOMENTUM_STOP_VEL = 1;
  /// Minimum pointerup velocity (rows/sec) to bother spawning a
  /// fling. A slow / deliberate drag-and-stop should snap to its
  /// final position, not glide past.
  private static readonly TOUCH_MOMENTUM_MIN_LAUNCH_VEL = 8;
  /// Exponential moving average factor for velocity smoothing. The
  /// most recent sample carries this weight, the prior smoothed
  /// value carries `1 - α`. 0.3 favors recency but still rejects
  /// one-frame jitter.
  private static readonly TOUCH_VEL_EMA_ALPHA = 0.3;

  /// Suspend (or resume) the swipe-scroll handlers for the active
  /// touch. The app sets this `true` on long-press so a subsequent
  /// finger drag extends a text selection instead of scrolling, and
  /// clears it when the gesture ends. Also cancels any in-flight scroll
  /// state / momentum so the takeover is clean.
  setTouchSelecting(active: boolean): void {
    this.touchSelecting = active;
    if (active) {
      this.touchScrollState = null;
      this.cancelTouchMomentum();
    }
  }

  private onPanePointerDown(e: PointerEvent): void {
    if (e.pointerType !== "touch") return;
    if (this.touchSelecting) return;
    if (this.gridRef === null) return;
    // Mouse-aware TUI owns the touch — it'll be forwarded as a
    // MouseInput via `app.ts`'s `mouseForward` path. Skip swipe.
    if ((this.gridRef.meta.modeFlags & MODE_MOUSE_REPORT) !== 0) return;
    // Touch on the scrollbar belongs to the thumb/track handlers
    // installed in the constructor — let those run instead.
    if (
      e.target instanceof Element &&
      e.target.closest(".ciri-scrollbar") !== null
    ) {
      return;
    }
    // No scrollback means nothing to scroll into. Saves work and
    // avoids preventDefault'ing a tap that would otherwise focus
    // the pane.
    if (this.gridRef.scrollbackRows === 0 && this.scrollOffset === 0) return;
    // Do NOT preventDefault here — the compat `mousedown` MUST
    // bubble so the tile's `FocusPane` listener fires on tap.
    // Once the gesture clearly commits to vertical scrolling
    // (in `pointermove` below), we preventDefault subsequent
    // events. The compat `mousedown` that already fired will
    // cause `app.ts` to set its `dragState`, but with
    // `mousemove` suppressed by our pointermove preventDefault
    // the drag never accumulates motion — the eventual compat
    // `mouseup` then resolves to a no-motion focus click and
    // clears state cleanly.
    //
    // Stop any in-flight momentum from a previous fling — the new
    // touch is the user reasserting control. iOS / Android both
    // do this: tap-to-stop, then drag.
    this.cancelTouchMomentum();
    this.touchScrollState = {
      startY: e.clientY,
      lastY: e.clientY,
      startX: e.clientX,
      lastMoveTime: e.timeStamp,
      pointerId: e.pointerId,
      committed: false,
      accumRows: 0,
    };
    this.touchScrollVel = 0;
  }

  private onPanePointerMove(e: PointerEvent): void {
    if (this.touchSelecting) return;
    const state = this.touchScrollState;
    if (state === null || state.pointerId !== e.pointerId) return;
    if (this.gridRef === null) return;
    const dy = e.clientY - state.startY;
    const dx = e.clientX - state.startX;
    if (!state.committed) {
      // Wait until the gesture clears the noise floor — below the
      // commit threshold it's still ambiguous (tap that wandered?
      // press that's about to be a long-press?).
      if (Math.abs(dy) < PaneRenderer.TOUCH_SCROLL_COMMIT_PX) return;
      if (
        Math.abs(dx) > Math.abs(dy) * PaneRenderer.TOUCH_SCROLL_AXIS_RATIO
      ) {
        // Horizontal-dominant — not a scroll. Cancel state so the
        // user can still pan / select sideways without us blocking.
        this.touchScrollState = null;
        return;
      }
      state.committed = true;
      // Capture the pointer so the wrapper keeps getting events
      // even if the finger drags off the pane (onto the chrome).
      try {
        this.wrapper.setPointerCapture(e.pointerId);
      } catch {
        // setPointerCapture can throw if the pointer is no longer
        // active — harmless, just skip the capture.
      }
    }
    e.preventDefault();
    const deltaY = e.clientY - state.lastY;
    const deltaT = e.timeStamp - state.lastMoveTime;
    state.lastY = e.clientY;
    state.lastMoveTime = e.timeStamp;
    const rowHeightPx = this.estimateRowHeightPx();
    if (rowHeightPx <= 0) return;
    // Drag DOWN (positive deltaY) reveals HISTORY above. Match the
    // platform `overflow: scroll` convention: finger movement and
    // content movement are coupled — drag finger down, content
    // moves down, top of content (history) becomes visible.
    state.accumRows += deltaY / rowHeightPx;
    const wholeRows = Math.trunc(state.accumRows);
    if (wholeRows !== 0) {
      state.accumRows -= wholeRows;
      const next = PaneRenderer.clamp(
        this.scrollOffset + wholeRows,
        0,
        this.gridRef.scrollbackRows,
      );
      if (next !== this.scrollOffset) {
        this.scrollOffset = next;
        this.rendererForcedRedraw = true;
        // rAF-coalesce — pointermove on touch screens can fire at
        // 90/120Hz, well above display refresh. Batching here is
        // what keeps swipe smooth instead of jankily redrawing
        // every input event.
        this.scheduleRender();
      }
    }
    // EMA-smooth velocity in rows/sec. Skip samples with implausible
    // dt (>200ms — finger paused or browser hiccup) so the velocity
    // doesn't tank to zero on a single laggy frame.
    if (deltaT > 0 && deltaT < 200) {
      const sampleVel = (deltaY / rowHeightPx / deltaT) * 1000;
      this.touchScrollVel =
        this.touchScrollVel * (1 - PaneRenderer.TOUCH_VEL_EMA_ALPHA) +
        sampleVel * PaneRenderer.TOUCH_VEL_EMA_ALPHA;
    }
    this.revealScrollbar();
  }

  /// Pointer was cancelled by the browser/OS (gesture stolen, palm
  /// rejection, …). Unlike `onPanePointerUp` this must NOT launch
  /// momentum — the user didn't flick, the system aborted. Just release
  /// capture and drop the in-flight scroll state.
  private onPanePointerCancel(e: PointerEvent): void {
    const state = this.touchScrollState;
    if (state === null || state.pointerId !== e.pointerId) return;
    if (state.committed) {
      try {
        this.wrapper.releasePointerCapture(e.pointerId);
      } catch {
        // Capture already released — fine.
      }
    }
    this.touchScrollState = null;
    this.touchScrollVel = 0;
  }

  private onPanePointerUp(e: PointerEvent): void {
    const state = this.touchScrollState;
    if (state === null || state.pointerId !== e.pointerId) return;
    if (state.committed) {
      try {
        this.wrapper.releasePointerCapture(e.pointerId);
      } catch {
        // Capture already released — fine.
      }
      // Launch momentum if the finger was still moving fast enough.
      // Below the threshold we treat the gesture as "intentional
      // stop" — the user lifted off without flicking.
      if (
        Math.abs(this.touchScrollVel) >=
        PaneRenderer.TOUCH_MOMENTUM_MIN_LAUNCH_VEL
      ) {
        this.startTouchMomentum();
      }
    }
    this.touchScrollState = null;
  }

  private startTouchMomentum(): void {
    this.cancelTouchMomentum();
    this.touchMomentumAccum = 0;
    this.touchMomentumLastTime = performance.now();
    const step = (frameTime: number): void => {
      if (this.destroyed || this.gridRef === null) {
        this.touchMomentumRaf = null;
        return;
      }
      const dtSec = (frameTime - this.touchMomentumLastTime) / 1000;
      this.touchMomentumLastTime = frameTime;
      // Apply current velocity to the row accumulator, commit
      // whole rows.
      this.touchMomentumAccum += this.touchScrollVel * dtSec;
      const whole = Math.trunc(this.touchMomentumAccum);
      if (whole !== 0) {
        this.touchMomentumAccum -= whole;
        const next = PaneRenderer.clamp(
          this.scrollOffset + whole,
          0,
          this.gridRef.scrollbackRows,
        );
        if (next !== this.scrollOffset) {
          this.scrollOffset = next;
          this.rendererForcedRedraw = true;
          this.render(this.gridRef);
          this.revealScrollbar();
        } else {
          // Hit a boundary — stop drifting against the edge.
          this.touchScrollVel = 0;
        }
      }
      // Frame-rate-independent exponential decay:
      //   v(t+dt) = v(t) × exp(-dt / τ)
      // Gives the same visible motion on 60 / 90 / 120 fps.
      this.touchScrollVel *= Math.exp(
        -dtSec / PaneRenderer.TOUCH_MOMENTUM_TAU_SEC,
      );
      if (
        Math.abs(this.touchScrollVel) < PaneRenderer.TOUCH_MOMENTUM_STOP_VEL
      ) {
        this.touchScrollVel = 0;
        this.touchMomentumRaf = null;
        return;
      }
      this.touchMomentumRaf = requestAnimationFrame(step);
    };
    this.touchMomentumRaf = requestAnimationFrame(step);
  }

  private cancelTouchMomentum(): void {
    if (this.touchMomentumRaf !== null) {
      cancelAnimationFrame(this.touchMomentumRaf);
      this.touchMomentumRaf = null;
    }
    this.touchScrollVel = 0;
    this.touchMomentumAccum = 0;
  }

  /// Schedule a render at the next animation frame, or no-op if one
  /// is already pending. Use this from scroll paths (pointermove,
  /// scrollbar drag) instead of calling `render()` synchronously
  /// — pointer events fire faster than the display refresh, and a
  /// per-event full-row repaint pegs the main thread. Coalescing
  /// to once-per-frame keeps interactive scroll feeling smooth.
  private scheduleRender(): void {
    if (this.pendingScrollRenderRaf !== null) return;
    this.pendingScrollRenderRaf = requestAnimationFrame(() => {
      this.pendingScrollRenderRaf = null;
      if (this.destroyed || this.gridRef === null) return;
      this.render(this.gridRef);
    });
  }

  /// Pixel height of a rendered row. Reads the first live row
  /// container when present; otherwise approximates from the
  /// wrapper's font-size × line-height (1.2 by convention; see the
  /// inline-style setup in the constructor). Used by the touch
  /// scroll handler to translate pixel deltas to row deltas.
  private estimateRowHeightPx(): number {
    if (this.rowEls.length > 0) {
      const h = this.rowEls[0]!.getBoundingClientRect().height;
      if (h > 0) return h;
    }
    const fs = parseFloat(this.wrapper.style.fontSize) || 14;
    return fs * 1.2;
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
    // Surface the scrollbar so the user sees the position update.
    // External callers (the app's wheel handler) reach scrolling
    // through here, so this one site covers the wheel path.
    this.revealScrollbar();
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
    this.scrollbarDrag = null;
    this.touchScrollState = null;
    this.cancelTouchMomentum();
    if (this.pendingScrollRenderRaf !== null) {
      cancelAnimationFrame(this.pendingScrollRenderRaf);
      this.pendingScrollRenderRaf = null;
    }
    if (this.scrollbarHideTimer !== null) {
      clearTimeout(this.scrollbarHideTimer);
      this.scrollbarHideTimer = null;
    }
    this.gridRef = null;
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

  /// Place (or replace) an inline image. `data` is raw RGBA bytes
  /// (`pixelWidth × pixelHeight × 4`) — the server decodes sixel/kitty/
  /// iTerm into RGBA before sending (`format: "rgba"`). The image is
  /// painted once onto a `<canvas>` at native pixel size, then
  /// CSS-scaled to its `widthCells × heightCells` box. `viewportRow` is
  /// converted to an absolute buffer row so the image scrolls with the
  /// content. Non-`rgba` formats are ignored (forward-compat guard).
  setImage(
    grid: PaneGrid,
    image: {
      imageId: bigint;
      col: number;
      row: number;
      widthCells: number;
      heightCells: number;
      pixelWidth: number;
      pixelHeight: number;
      format: string;
      data: Uint8Array;
    },
  ): void {
    if (image.format !== "rgba") return;
    const expected = image.pixelWidth * image.pixelHeight * 4;
    if (image.pixelWidth <= 0 || image.pixelHeight <= 0) return;
    if (image.data.length < expected) return; // truncated / malformed
    const key = image.imageId.toString();
    let entry = this.images.get(key);
    if (entry === undefined) {
      const canvas = this.doc.createElement("canvas");
      canvas.className = "ciri-image";
      canvas.style.position = "absolute";
      canvas.style.pointerEvents = "none";
      canvas.style.imageRendering = "pixelated";
      this.wrapper.appendChild(canvas);
      entry = {
        canvas,
        srcRow: 0,
        col: image.col,
        widthCells: image.widthCells,
        heightCells: image.heightCells,
      };
      this.images.set(key, entry);
    }
    entry.canvas.width = image.pixelWidth;
    entry.canvas.height = image.pixelHeight;
    // `getContext` is absent in non-DOM test environments (jsdom) and
    // can throw under strict canvas policies — tolerate both; the
    // canvas element is still placed (sized/positioned) for layout.
    let ctx: CanvasRenderingContext2D | null = null;
    try {
      ctx = entry.canvas.getContext("2d");
    } catch {
      ctx = null;
    }
    if (ctx !== null && typeof ImageData !== "undefined") {
      // ImageData needs an exactly-sized, ArrayBuffer-backed clamped
      // array (a view over `data.buffer`, which is `ArrayBufferLike`,
      // doesn't satisfy the DOM type and may include trailing bytes).
      const pixels = new Uint8ClampedArray(expected);
      pixels.set(image.data.subarray(0, expected));
      ctx.putImageData(
        new ImageData(pixels, image.pixelWidth, image.pixelHeight),
        0,
        0,
      );
    }
    // Anchor to the absolute combined-buffer row at placement time.
    entry.srcRow = grid.scrollbackRows + image.row;
    entry.col = image.col;
    entry.widthCells = image.widthCells;
    entry.heightCells = image.heightCells;
    this.updateImages(grid);
  }

  /// Remove every placed image. Driven by the server's `ImageDeleted`
  /// (cleared on screen-clear / alt-screen exit / pane reset).
  clearImages(): void {
    for (const { canvas } of this.images.values()) canvas.remove();
    this.images.clear();
  }

  /// Keep image `srcRow` anchors valid as scrollback mutates. A
  /// wholesale replace (epoch bump) invalidates every anchor's buffer
  /// identity → drop all images (the server replays the live ones). A
  /// front-trim shifts every surviving row's absolute index down by the
  /// trim count → subtract it from each anchor, dropping any image whose
  /// content was fully evicted. Plain append leaves srcRows stable, so
  /// it needs no adjustment here.
  private rebaseImagesForScrollback(grid: PaneGrid): void {
    if (this.lastScrollbackEpoch === null) {
      // First render — seed the baselines; nothing to rebase yet.
      this.lastScrollbackEpoch = grid.scrollbackEpoch;
      this.lastScrollbackTrimmed = grid.scrollbackTrimmed;
      return;
    }
    if (grid.scrollbackEpoch !== this.lastScrollbackEpoch) {
      this.lastScrollbackEpoch = grid.scrollbackEpoch;
      this.lastScrollbackTrimmed = grid.scrollbackTrimmed;
      if (this.images.size > 0) this.clearImages();
      return;
    }
    const trimDelta = grid.scrollbackTrimmed - (this.lastScrollbackTrimmed ?? 0);
    this.lastScrollbackTrimmed = grid.scrollbackTrimmed;
    if (trimDelta <= 0 || this.images.size === 0) return;
    for (const [key, entry] of this.images) {
      entry.srcRow -= trimDelta;
      // Entire image scrolled off the top of the (trimmed) buffer.
      if (entry.srcRow + entry.heightCells <= 0) {
        entry.canvas.remove();
        this.images.delete(key);
      }
    }
  }

  /// Reposition / clip image canvases for the current scroll offset,
  /// using the same `srcRow → displayRow` mapping as cells + selection.
  private updateImages(grid: PaneGrid): void {
    if (this.images.size === 0) return;
    for (const entry of this.images.values()) {
      const displayRow = entry.srcRow - grid.scrollbackRows + this.scrollOffset;
      // Hide when the image's top row scrolls out of the viewport.
      // (Partial clipping at the viewport edge is good enough for v1;
      // `overflow: hidden` on the wrapper trims the overshoot.)
      if (
        displayRow + entry.heightCells <= 0 ||
        displayRow >= grid.rows
      ) {
        entry.canvas.style.display = "none";
        continue;
      }
      entry.canvas.style.display = "block";
      entry.canvas.style.left = `${entry.col}ch`;
      entry.canvas.style.top = `${displayRow * 1.2}em`;
      entry.canvas.style.width = `${entry.widthCells}ch`;
      entry.canvas.style.height = `${entry.heightCells * 1.2}em`;
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
      // Variation selectors (e.g. `U+FE0E` text presentation,
      // `U+FE0F` emoji presentation). These attach to the
      // preceding glyph cluster and contribute zero advance — a
      // sequence like `❤️` is `U+2764 U+FE0F` and renders as one
      // visible cluster, not two cells. Round-8 codex P2.
      (cp >= 0xfe00 && cp <= 0xfe0f) ||
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
