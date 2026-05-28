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
  MODE_MOUSE_REPORT,
} from "@ciri/codec";
import type { PaneAction } from "./layout.js";
import {
  DEFAULT_THEME,
  GridShapeError,
  PaneGrid,
  PaneRenderer,
  type SearchMatch,
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
  /// Fired when the server reports the attached session name via
  /// `SessionSwitched` — including the resolved name after an
  /// `__auto__` auto-attach handshake. Lets the page pin its URL to
  /// the concrete session so a reload/share reattaches to the same one.
  onSessionChange?: (sessionName: string) => void;
  /// Fired when the server sends `ServerShutdown` (graceful daemon
  /// stop). Distinct from `onClose` (a transport drop, which may
  /// reconnect) so the page can show a terminal "server shut down"
  /// state rather than a hopeful "reconnecting…".
  onServerShutdown?: () => void;
}

const DEFAULT_FONT_FAMILY = `"JetBrains Mono", "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace`;
const DEFAULT_FONT_SIZE = "14px";
/// How long the `ciri-tile-bell` class lingers after a Bell event.
/// Themes typically animate a brief opacity pulse over this window.
const BELL_FLASH_MS = 1000;
/// How long an in-app toast stays before auto-dismissing. Long enough
/// to read a "command finished" / "session killed" line, short enough
/// not to pile up.
const TOAST_MS = 6000;

/// Quick-action buttons rendered after the pane chip strip. The web
/// renders one pane at a time (chips are the visual navigation), so
/// the desktop client's "focus left / right / up / down" arrows have
/// no spatial meaning here — only operations that produce a new chip
/// or remove one belong on the bar. "New workspace" lives on the
/// workspace tab strip (its "+") and "New session" on the session bar
/// (its "+"), next to the things they create.
///
/// "Close" is rendered per-chip (next to the chip itself) rather
/// than as a global action, matching the browser tab UX where each
/// tab carries its own × — see the close button push in
/// `LayoutManager.setLayout`.
///
/// `id` strings are matched in `CiriApp.onAction` below — keep them
/// in sync when adding new actions.
const DEFAULT_PANE_ACTIONS: readonly PaneAction[] = [
  { id: "new-pane", icon: "+", label: "New pane" },
];

export class CiriApp {
  private readonly doc: Document;
  private readonly orphanRoot: HTMLElement;
  private readonly layout: LayoutManager;
  private readonly grids = new Map<string, PaneGrid>();
  private readonly renderers = new Map<string, PaneRenderer>();
  private currentLayout: LayoutState | null = null;
  /// Session this client is currently attached to. Seeded from the
  /// constructor's `sessionName` (which may be the `__auto__` sentinel)
  /// and updated to the resolved/real name on every `SessionSwitched`.
  /// Drives the session dropdown's selected value.
  private currentSessionName: string;
  /// Last session list received from a `SessionList` reply, cached so
  /// `SessionSwitched` can refresh the dropdown's active selection
  /// without waiting for a fresh round-trip.
  private knownSessions: string[] = [];
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
  /// Last `Resize` dims fingerprint we put on the wire. Used by
  /// `sendResizeForPane` to skip identical retransmits — the eager
  /// chip-click path may compute the same lie as the last one
  /// (e.g. symmetric two-column layout where every chip has the
  /// same proportion), and we don't want to spam the server.
  /// Format is opaque (the comparison is purely string equality);
  /// see `sendResizeForPane` for the encoding.
  private lastSentResizeKey: string | null = null;
  /// Has the post-attach lied Resize gone out? We only do this
  /// ONCE per attach because every other LayoutUpdate-triggered
  /// Resize creates a server-side feedback loop:
  /// `LayoutUpdate → web Resize → server resize_all_panes →
  /// LayoutUpdate + FullPaneSync broadcast` is a CPU burst that
  /// any co-attached desktop client also pays. After the initial
  /// Resize, we rely on the eager send in `onPaneClicked` and the
  /// `ResizeObserver` tick to keep the lie current — both are
  /// driven by explicit user actions, so the server reflow is
  /// proportional to user input, not to layout churn.
  private initialResizeSentAfterAttach = false;
  /// Set when the web client has just queued a structural change
  /// (`CreatePane`, `ClosePane`, …) that we KNOW will shift the
  /// active pane's column/tile proportion. The next `LayoutUpdate`
  /// then re-aims the viewport lie at the freshly-active pane.
  /// Without this, the lie computed for the old layout shape would
  /// stick around: e.g., open a 2nd pane then close it → first pane
  /// is rendered with a 2-column lied viewport, server allocates it
  /// at 2× real width → blank right half.
  ///
  /// Cleared by the next `applyLayout`. NOT set for `FocusPane`
  /// (that path already eager-resizes in `onPaneClicked`) and NOT
  /// set for external (desktop-initiated) changes — those keep
  /// silently to avoid amplifying every co-attached client's
  /// layout edit into an extra reflow round-trip.
  private expectStructuralChange = false;
  /// Hidden textarea that owns keyboard focus so the browser actually
  /// dispatches IME composition events. Modern Chrome/Firefox will fire
  /// composition events on any focused element, but Safari + most
  /// mobile browsers require an editable target (textarea or
  /// contenteditable) before the IME engine engages. We route all
  /// `.focus()` calls here; keyboard / composition events bubble up
  /// to the root listeners, so the rest of the dispatch path is
  /// unchanged. Off-screen + `aria-hidden` so it never appears in the
  /// visual layout or assistive-tech tree.
  private readonly compositionSinkEl: HTMLTextAreaElement;
  // DECCKM tracking is now per-keystroke and per-pane — `onKeyDown`
  // reads the target pane's `meta.modeFlags & MODE_APP_CURSOR`.
  private readonly handlerOnOpen?: () => void;
  private readonly handlerOnClose?: (reason: string, reconnecting: boolean) => void;
  private readonly handlerOnError?: (err: Error) => void;
  private readonly handlerOnSessionChange?: (sessionName: string) => void;
  private readonly handlerOnServerShutdown?: () => void;
  // DOM event handlers stored as instance fields so `destroy()` can
  // remove them. Anonymous arrows installed at construction time
  // would otherwise stick around forever and a remounted CiriApp on
  // the same root would double-fire every keystroke.
  private readonly onKeyDownHandler: (e: KeyboardEvent) => void;
  private readonly onWheelHandler: (e: WheelEvent) => void;
  private readonly onMouseDownHandler: (e: MouseEvent) => void;
  /// Suppress the browser's native right-click context menu when the
  /// pane under the pointer has `MODE_MOUSE_REPORT` set, so the TUI
  /// gets the button=2 event without the menu painting over it.
  /// Doesn't fire `MouseInput` itself — that already went out via
  /// the mousedown handler.
  private readonly onContextMenuHandler: (e: MouseEvent) => void;
  /// Capture-phase mousedown that runs **before** the LayoutManager's
  /// tile-level bubble listener can fire `FocusPane`. The tile lives
  /// deeper in the DOM than the root, so a bubble-phase root handler
  /// is too late — by the time it sees the event the tile has already
  /// queued a `FocusPane`, which round-trips through the server, comes
  /// back as `LayoutUpdate`, and rebuilds every `.ciri-column` /
  /// `.ciri-tile` node mid-drag. Stopping propagation here keeps the
  /// drag's element refs valid for the whole interaction.
  private readonly onCaptureMouseDownHandler: (e: MouseEvent) => void;
  /// Always-on root-level mousemove for resize-cursor hover feedback.
  /// Unlike `onWindowMouseMove` (only registered during an active
  /// drag), this fires on every cursor move so the user sees
  /// col-resize / row-resize hints when nearing a border. Cheap:
  /// the hit-test reads a handful of element rects.
  private readonly onRootMouseMoveHandler: (e: MouseEvent) => void;
  private readonly onCompositionStartHandler: (e: CompositionEvent) => void;
  private readonly onCompositionUpdateHandler: (e: CompositionEvent) => void;
  private readonly onCompositionEndHandler: (e: CompositionEvent) => void;
  private readonly onSinkBeforeInputHandler: (e: Event) => void;
  /// `input`-event sibling of `beforeinput`. Some Safari/mobile
  /// builds skip `beforeinput` for IME commits and only surface them
  /// here. Listening to both eliminates the per-engine guesswork.
  private readonly onSinkInputHandler: (e: Event) => void;
  /// Redirects keyboard focus to the composition sink whenever the
  /// root receives focus directly (Tab navigation, programmatic
  /// `root.focus()`). Without this, a user who tabs into the
  /// terminal lands on the focusable root and never engages the
  /// IME — Safari/mobile especially won't fire composition events
  /// on a non-editable element.
  private readonly onRootFocusHandler: (e: FocusEvent) => void;
  private readonly onTouchPointerDownHandler: (e: PointerEvent) => void;
  private readonly onTouchPointerMoveHandler: (e: PointerEvent) => void;
  private readonly onTouchPointerUpHandler: (e: PointerEvent) => void;
  private readonly onTouchPointerCancelHandler: (e: PointerEvent) => void;
  private readonly onPasteHandler: (e: ClipboardEvent) => void;
  private readonly onVisualViewportResizeHandler: () => void;
  /// `true` between `compositionstart` and `compositionend`. Used as a
  /// backstop so keydown events that slip through `e.isComposing` (an
  /// older spec quirk in some browsers — the flag isn't always set on
  /// the `compositionstart`-triggering keydown itself) still get
  /// suppressed.
  private composing = false;
  /// Pane currently displaying the pre-edit overlay. Updated on every
  /// `compositionupdate` to follow the user's active focus, so a
  /// server-driven LayoutUpdate that promotes a different pane mid-
  /// composition pulls the visible preedit along with it (matches
  /// Rust's behavior of storing preedit state globally and anchoring
  /// the paint on the active pane).
  private composingPaneId: bigint | null = null;
  /// Most recent committed text inferred from a `beforeinput` /
  /// `input` event on the sink. Used as a fallback when
  /// `compositionend.data` is empty (Safari + a handful of mobile
  /// IMEs surface the commit only through the editable-element input
  /// path). Tagged with the composition generation it was captured
  /// in so a late event from a previous session can't be consumed
  /// by a new one. Reset on every `compositionstart`; set only on
  /// commit-signaling `inputType` values during the appropriate
  /// state window, not on intermediate `insertCompositionText`
  /// ticks, so a cancel-with-dirty-sink doesn't silently turn into
  /// a commit.
  private compositionCommitData: { generation: number; text: string } | null = null;
  /// Bumps on every `compositionstart`. Used by deferred commit
  /// finalization (see `onCompositionEnd`) to detect when a new
  /// composition session has begun in the gap between
  /// `compositionend` and the post-event input fire — in that case
  /// the deferred work for the previous session should silently
  /// abandon rather than committing into the new one.
  private compositionGeneration = 0;
  /// Non-null only while we're inside the brief deferred-commit
  /// window: `compositionend` fired with empty data, the macrotask
  /// hasn't fired yet, and a Firefox-style post-compositionend
  /// `input` is expected. Used to gate acceptance of
  /// `insertText`/`insertFromComposition` events outside of an
  /// active composition — stale events from previous sessions
  /// that arrive after a new `compositionstart` (which wipes this
  /// flag) are rejected. Carries the snapshotted commit target
  /// so `flushPendingLateCommit()` can route consistently with
  /// the macrotask path. Round-6/8 codex fix.
  private pendingLateCommit:
    | { generation: number; target: bigint | null }
    | null = null;
  /// True for one microtask after a direct (non-composition) IME insert
  /// was handled on `beforeinput`, so the paired `input` event for the
  /// same insert doesn't double-send. See `handleDirectInsert`.
  private directInsertGuard = false;
  /// Pending OSC 52 clipboard payload awaiting a user gesture to write
  /// (browsers gate `clipboard.writeText` on user activation, which a
  /// server message lacks). Flushed by `flushPendingClipboard` from the
  /// keydown / tap handlers. Latest yank wins.
  private pendingClipboardWrite: string | null = null;
  /// Scrollback find bar (Ctrl/Cmd+Shift+F). Its own focus island —
  /// see `buildSearchBar`. `searchPaneId` is the pane being searched
  /// (snapshotted at open); `searchMatches` is the current result set
  /// in reading order; `searchMatchIdx` the highlighted one. The
  /// renderer's scroll offset is snapshotted at open and restored on
  /// close so dismissing the bar returns the user to where they were.
  private readonly searchBarEl: HTMLElement;
  private readonly searchInputEl: HTMLInputElement;
  private readonly searchStatusEl: HTMLElement;
  private searchPaneId: bigint | null = null;
  private searchMatches: SearchMatch[] = [];
  private searchMatchIdx = 0;
  private searchPrevScrollOffset = 0;
  /// Right-click context menu (Copy / Paste / Find). App-owned DOM,
  /// `position: fixed` at the click point; hidden until a contextmenu
  /// event over a pane. `contextMenuDismiss` is the one-shot outside-
  /// click listener installed while the menu is open.
  private readonly contextMenuEl: HTMLElement;
  private readonly contextMenuCopyBtn: HTMLButtonElement;
  private contextMenuDismiss: ((e: Event) => void) | null = null;
  /// Floating "show keyboard" button. CSS hides it except on coarse
  /// pointers (touch); tapping it focuses the sink inside the gesture
  /// as a reliable fallback when tap-to-focus didn't raise the keyboard.
  private readonly keyboardBtnEl: HTMLButtonElement;
  /// Pending bell-flash clear timers, keyed by paneId-as-string so a
  /// repeat bell on the same pane re-arms cleanly instead of leaving
  /// the class permanently stuck.
  private readonly bellTimers = new Map<string, ReturnType<Window["setTimeout"]>>();
  /// Transient in-app notification stack ("toast"). The universal,
  /// always-available surface for server-pushed Notification /
  /// SessionKilled / CommandCompleted cues — it works where OS
  /// notifications don't (iOS Safari pages, denied permission), and is
  /// the thing the user sees the moment they return to the tab.
  private readonly toastEl: HTMLElement;
  /// Live toast auto-dismiss timers, cleared on destroy.
  private readonly toastTimers = new Set<ReturnType<Window["setTimeout"]>>();
  /// Set true the first time we'd have fired an OS notification but the
  /// permission was still "default". The next user gesture upgrades it
  /// into an actual permission prompt — so we only ever ask *after*
  /// something relevant happened, and only inside a gesture (which some
  /// browsers require for `Notification.requestPermission`).
  private notifyWanted = false;
  /// One-shot guard so we prompt for notification permission at most once.
  private notifyPermissionAsked = false;
  /// In-flight mouse drag state, recorded on mousedown and updated on
  /// each window-level mousemove. `null` when no drag is active.
  private dragState: {
    paneId: bigint;
    renderer: PaneRenderer;
    grid: PaneGrid;
    anchor: SelectionAnchor;
    moved: boolean;
  } | null = null;
  /// In-flight touch gesture on a (non-mouse-reporting) pane. A quick
  /// tap focuses the keyboard sink; a stationary long-press starts a
  /// text selection (drag to extend) and pops the context menu on
  /// release. While `selecting`, the renderer's swipe-scroll stands
  /// down (`setTouchSelecting`). Vertical drags before the long-press
  /// fires are left to the renderer (scrollback swipe). See
  /// `installTouchHandlers`.
  private touchGesture: {
    pointerId: number;
    startX: number;
    startY: number;
    paneId: bigint;
    renderer: PaneRenderer;
    grid: PaneGrid;
    anchor: SelectionAnchor | null;
    longPressTimer: number | null;
    selecting: boolean;
    selectMoved: boolean;
  } | null = null;
  /// In-flight column / tile resize-drag state. Set on mousedown
  /// landing within `RESIZE_HIT_PX` of a column or tile border;
  /// `mousemove` adjusts the relevant CSS `flex` value optimistically,
  /// `mouseup` sends the final wire message (`AdjustColumnSplitAt`
  /// or `SetTileWeights`). The server-driven `LayoutUpdate` that
  /// follows reconciles any clamping the server applied. Mutually
  /// exclusive with `dragState` and `mouseForward` — the press path
  /// checks resize first.
  private resizeDrag: {
    kind: "column";
    leftEl: HTMLElement;
    rightEl: HTMLElement;
    leftIdx: number;
    /// Anchor X for incremental delta math. Advanced on every
    /// mousemove so accumulated rounding doesn't drift.
    anchorX: number;
    /// Inner viewport width captured at drag start. The server's
    /// `AdjustColumnSplitAt.delta` is a proportion of inner_vw, so
    /// we have to divide pixel deltas by the same baseline. Captured
    /// once because re-reading on every move would catch the
    /// optimistic widths we just wrote and drift.
    innerVw: number;
    /// Accumulated proportion delta sent on `mouseup`.
    totalDelta: number;
    /// Latest `flex` values applied to the two columns; kept as
    /// state so the local update is monotonic against repeated
    /// mousemove ticks within the same frame.
    leftFlex: number;
    rightFlex: number;
  } | {
    kind: "tile";
    topEl: HTMLElement;
    bottomEl: HTMLElement;
    columnIdx: number;
    topTileIdx: number;
    anchorY: number;
    /// Pixel heights at drag start — flex-grow values are
    /// proportional to these initial heights, so updates can preserve
    /// the relative scale.
    topHeight: number;
    bottomHeight: number;
    /// Latest `flex` values for the two tiles, mirrored from the
    /// running pixel-height calculation. Sent verbatim as
    /// `SetTileWeights.top_weight` / `bottom_weight` on release.
    topFlex: number;
    bottomFlex: number;
  } | null = null;
  /// In-flight mouse-forwarding state, recorded when a press lands
  /// on a pane that has `MODE_MOUSE_REPORT` set and shift is not held.
  /// While set, `mousemove`/`mouseup` translate to `MouseInput` frames
  /// (button=32+offset drag, button=3 release) instead of selection
  /// drag. Mirrors the Rust client's `mouse_left_passthrough`
  /// (mouse.rs:295), extended to all primary buttons because the web
  /// client has no context-menu / middle-click UI to reserve them for.
  /// Mutually exclusive with `dragState` — the press path picks exactly
  /// one branch.
  private mouseForward: {
    paneId: bigint;
    renderer: PaneRenderer;
    grid: PaneGrid;
    /// Last (col, row) we sent so we can suppress duplicate motion
    /// frames when the pointer moves within a single cell. Server's
    /// SGR encoder doesn't dedupe — without this, a single drag would
    /// burn through hundreds of frames per second on a fast mouse.
    lastCell: { col: number; row: number };
    /// Browser-button index of the press (0=left, 1=middle, 2=right).
    /// Drag motion encodes as `32 + button` so the TUI can tell which
    /// button is being dragged (xterm convention: 32=B1, 33=B2, 34=B3).
    button: number;
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
    this.currentSessionName = opts.sessionName;
    if (opts.onOpen !== undefined) this.handlerOnOpen = opts.onOpen;
    if (opts.onClose !== undefined) this.handlerOnClose = opts.onClose;
    if (opts.onError !== undefined) this.handlerOnError = opts.onError;
    if (opts.onSessionChange !== undefined) this.handlerOnSessionChange = opts.onSessionChange;
    if (opts.onServerShutdown !== undefined) this.handlerOnServerShutdown = opts.onServerShutdown;
    this.onKeyDownHandler = (e) => this.onKeyDown(e);
    this.onWheelHandler = (e) => this.onWheel(e);
    this.onMouseDownHandler = (e) => {
      this.handleRootMouseDown(e);
      queueMicrotask(() => {
        if (!this.destroyed) this.focusInputSink();
      });
    };
    this.onContextMenuHandler = (e) => this.handleContextMenu(e);
    this.onCaptureMouseDownHandler = (e) => this.handleCaptureMouseDown(e);
    this.onRootMouseMoveHandler = (e) => this.handleRootMouseMove(e);
    this.onWindowMouseMove = (e) => this.handleWindowMouseMove(e);
    this.onWindowMouseUp = (e) => this.handleWindowMouseUp(e);
    this.onCompositionStartHandler = (e) => this.onCompositionStart(e);
    this.onCompositionUpdateHandler = (e) => this.onCompositionUpdate(e);
    this.onCompositionEndHandler = (e) => this.onCompositionEnd(e);
    this.onSinkBeforeInputHandler = (e) => this.onSinkBeforeInput(e);
    this.onSinkInputHandler = (e) => this.onSinkBeforeInput(e);
    this.onRootFocusHandler = (e) => this.onRootFocus(e);
    this.onTouchPointerDownHandler = (e) => this.onTouchPointerDown(e);
    this.onTouchPointerMoveHandler = (e) => this.onTouchPointerMove(e);
    this.onTouchPointerUpHandler = (e) => this.onTouchPointerUp(e);
    this.onTouchPointerCancelHandler = (e) => this.onTouchPointerCancel(e);
    this.onPasteHandler = (e) => this.onPaste(e);
    this.onVisualViewportResizeHandler = () => this.onVisualViewportResize();

    // Hidden container for pane renderers whose tile isn't currently
    // mounted (inactive workspaces, transient layout reshapes).
    this.orphanRoot = doc.createElement("div");
    this.orphanRoot.className = "ciri-orphan-panes";
    this.orphanRoot.style.display = "none";
    doc.body.appendChild(this.orphanRoot);

    // Composition sink — see field doc for why this exists. Tab order
    // is `-1` so a sighted keyboard user tabbing through the page
    // can't accidentally focus it; programmatic `.focus()` still
    // works. `autocomplete=off` + `spellcheck=false` so neither the
    // browser nor the OS adds suggestions overlay that could leak
    // visual artifacts. `aria-hidden=true` keeps it out of screen
    // reader navigation; the terminal UI is the actual semantic
    // surface, not this textarea.
    this.compositionSinkEl = doc.createElement("textarea");
    this.compositionSinkEl.className = "ciri-composition-sink";
    this.compositionSinkEl.setAttribute("aria-hidden", "true");
    this.compositionSinkEl.setAttribute("autocomplete", "off");
    this.compositionSinkEl.setAttribute("autocorrect", "off");
    this.compositionSinkEl.setAttribute("autocapitalize", "off");
    this.compositionSinkEl.spellcheck = false;
    this.compositionSinkEl.tabIndex = -1;
    this.compositionSinkEl.style.position = "absolute";
    this.compositionSinkEl.style.left = "0";
    this.compositionSinkEl.style.top = "0";
    this.compositionSinkEl.style.width = "1em";
    this.compositionSinkEl.style.height = "1em";
    this.compositionSinkEl.style.opacity = "0";
    this.compositionSinkEl.style.pointerEvents = "none";
    this.compositionSinkEl.style.zIndex = "-1";
    this.compositionSinkEl.style.resize = "none";
    this.compositionSinkEl.style.border = "0";
    this.compositionSinkEl.style.padding = "0";
    this.compositionSinkEl.style.overflow = "hidden";

    // ── Scrollback find bar (hidden until Ctrl/Cmd+Shift+F) ──────────
    this.searchBarEl = doc.createElement("div");
    this.searchBarEl.className = "ciri-search";
    this.searchBarEl.hidden = true;
    this.searchInputEl = doc.createElement("input");
    this.searchInputEl.type = "text";
    this.searchInputEl.className = "ciri-search-input";
    this.searchInputEl.setAttribute("aria-label", "Find in scrollback");
    this.searchInputEl.placeholder = "Find…";
    this.searchStatusEl = doc.createElement("span");
    this.searchStatusEl.className = "ciri-search-status";
    const mkSearchBtn = (label: string, aria: string, onClick: () => void) => {
      const b = doc.createElement("button");
      b.type = "button";
      b.className = "ciri-search-btn";
      b.textContent = label;
      b.setAttribute("aria-label", aria);
      b.title = aria;
      b.addEventListener("click", () => {
        onClick();
        // Keep the input focused so the user can keep typing/cycling.
        this.searchInputEl.focus();
      });
      return b;
    };
    this.searchInputEl.addEventListener("input", () => {
      this.runSearch(this.searchInputEl.value);
    });
    this.searchInputEl.addEventListener("keydown", (e) => {
      // This input owns its keys — never let them reach the PTY.
      e.stopPropagation();
      if (e.key === "Enter") {
        e.preventDefault();
        this.stepSearch(e.shiftKey ? -1 : 1);
      } else if (e.key === "Escape") {
        e.preventDefault();
        this.closeSearch();
      }
    });
    this.searchBarEl.appendChild(this.searchInputEl);
    this.searchBarEl.appendChild(this.searchStatusEl);
    this.searchBarEl.appendChild(mkSearchBtn("‹", "Previous match", () => this.stepSearch(-1)));
    this.searchBarEl.appendChild(mkSearchBtn("›", "Next match", () => this.stepSearch(1)));
    this.searchBarEl.appendChild(mkSearchBtn("✕", "Close find", () => this.closeSearch()));

    // ── Right-click context menu (Copy / Paste / Find) ──────────────
    this.contextMenuEl = doc.createElement("div");
    this.contextMenuEl.className = "ciri-contextmenu";
    this.contextMenuEl.setAttribute("role", "menu");
    this.contextMenuEl.hidden = true;
    const mkMenuItem = (label: string, onClick: () => void): HTMLButtonElement => {
      const b = doc.createElement("button");
      b.type = "button";
      b.className = "ciri-contextmenu-item";
      b.setAttribute("role", "menuitem");
      b.textContent = label;
      // `mousedown` (not `click`): the outside-dismiss listener also
      // runs on mousedown; acting here ensures the item fires before
      // the menu is torn down, and preventDefault keeps focus put.
      b.addEventListener("mousedown", (e) => {
        e.preventDefault();
        e.stopPropagation();
        this.hideContextMenu();
        onClick();
      });
      return b;
    };
    this.contextMenuCopyBtn = mkMenuItem("Copy", () => {
      this.copySelectionToClipboard();
      this.focusInputSink();
    });
    this.contextMenuEl.appendChild(this.contextMenuCopyBtn);
    this.contextMenuEl.appendChild(
      mkMenuItem("Paste", () => {
        void this.pasteFromClipboard();
      }),
    );
    this.contextMenuEl.appendChild(
      mkMenuItem("Find…", () => {
        this.openSearch();
      }),
    );

    // Floating keyboard toggle (touch only — hidden via CSS otherwise).
    this.keyboardBtnEl = doc.createElement("button");
    this.keyboardBtnEl.type = "button";
    this.keyboardBtnEl.className = "ciri-keyboard-toggle";
    this.keyboardBtnEl.textContent = "⌨";
    this.keyboardBtnEl.setAttribute("aria-label", "Show keyboard");
    this.keyboardBtnEl.title = "Show keyboard";
    // `pointerdown` + preventDefault keeps the button from stealing
    // focus, so the synchronous `focus()` lands on the sink within the
    // trusted gesture (what raises the on-screen keyboard).
    this.keyboardBtnEl.addEventListener("pointerdown", (e) => {
      e.preventDefault();
      this.focusInputSink();
    });

    // Toast stack — server-pushed notification / session / command cues
    // land here. `aria-live` so a screen reader announces new entries.
    this.toastEl = doc.createElement("div");
    this.toastEl.className = "ciri-toast-stack";
    this.toastEl.setAttribute("aria-live", "polite");

    this.cellSize = this.measureCellsOrFallback();

    this.layout = new LayoutManager(this.root, {
      onPaneClick: (id) => this.onPaneClicked(id),
      onPaneClose: (id) => this.onPaneClose(id),
      onWorkspaceClick: (idx) => this.onWorkspaceClicked(idx),
      paneTitleFor: (id) => this.grids.get(id.toString())?.title ?? "",
      actions: DEFAULT_PANE_ACTIONS,
      onAction: (id) => this.onAction(id),
      onSessionSelect: (name) => this.onSessionSelected(name),
      onNewWorkspace: () => this.createWorkspace(),
      onNewSession: () => this.createSession(),
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
    // Reset the one-shot initial-Resize gate so a `destroy()` →
    // `new CiriApp(...)` → `start()` cycle on the same root behaves
    // like a fresh attach.
    this.initialResizeSentAfterAttach = false;
    this.expectStructuralChange = false;
    this.lastSentResizeKey = null;
    this.client.start();
    this.installResizeObserver();
  }

  destroy(): void {
    if (this.destroyed) return;
    // Tear down any open context menu's window-level listeners before
    // flipping `destroyed` (hideContextMenu is a no-op when closed).
    this.hideContextMenu();
    this.destroyed = true;
    this.resizeObserver?.disconnect();
    this.resizeObserver = null;
    // Detach interaction handlers before closing the client so a
    // late-arriving DOM event can't try to send through it.
    this.root.removeEventListener("keydown", this.onKeyDownHandler);
    this.root.removeEventListener("wheel", this.onWheelHandler);
    this.root.removeEventListener(
      "mousedown",
      this.onCaptureMouseDownHandler,
      { capture: true },
    );
    this.root.removeEventListener("mousedown", this.onMouseDownHandler);
    this.root.removeEventListener("mousemove", this.onRootMouseMoveHandler);
    this.root.removeEventListener("contextmenu", this.onContextMenuHandler);
    this.root.removeEventListener(
      "compositionstart",
      this.onCompositionStartHandler,
    );
    this.root.removeEventListener(
      "compositionupdate",
      this.onCompositionUpdateHandler,
    );
    this.root.removeEventListener(
      "compositionend",
      this.onCompositionEndHandler,
    );
    this.compositionSinkEl.removeEventListener(
      "beforeinput",
      this.onSinkBeforeInputHandler,
    );
    this.compositionSinkEl.removeEventListener(
      "input",
      this.onSinkInputHandler,
    );
    this.compositionSinkEl.removeEventListener("paste", this.onPasteHandler);
    this.root.removeEventListener("focus", this.onRootFocusHandler);
    this.root.removeEventListener("pointerdown", this.onTouchPointerDownHandler);
    this.root.removeEventListener("pointermove", this.onTouchPointerMoveHandler);
    this.root.removeEventListener("pointerup", this.onTouchPointerUpHandler);
    this.root.removeEventListener("pointercancel", this.onTouchPointerCancelHandler);
    this.clearTouchLongPressTimer();
    const win = this.doc.defaultView;
    if (win !== null) {
      win.visualViewport?.removeEventListener(
        "resize",
        this.onVisualViewportResizeHandler,
      );
      win.removeEventListener("mousemove", this.onWindowMouseMove);
      win.removeEventListener("mouseup", this.onWindowMouseUp);
    }
    this.dragState = null;
    // Mouse-forward state holds renderer/grid references that we just
    // tore the window listeners off of; null it so a stray late
    // callback (testing harnesses sometimes synthesize events past
    // destroy) can't reach into freed renderers.
    this.mouseForward = null;
    // Resize-drag state pins DOM nodes inside the LayoutManager that
    // is about to be torn down; drop it so the GC can collect.
    this.resizeDrag = null;
    this.setResizeCursor(null);
    this.composing = false;
    this.composingPaneId = null;
    this.pendingLateCommit = null;
    this.compositionCommitData = null;
    // Cancel pending bell-flash clears so we don't run callbacks
    // against torn-down DOM.
    for (const t of this.bellTimers.values()) {
      win?.clearTimeout(t);
    }
    this.bellTimers.clear();
    for (const t of this.toastTimers) {
      win?.clearTimeout(t);
    }
    this.toastTimers.clear();
    this.toastEl.remove();
    this.client.close();
    for (const r of this.renderers.values()) r.destroy();
    this.renderers.clear();
    this.grids.clear();
    this.layout.destroy();
    this.orphanRoot.remove();
    // The composition sink was appended to `root` (which is owned by
    // the caller, not us), so we own its lifecycle explicitly.
    // Repeated mount/destroy cycles on the same root would otherwise
    // leave a stack of inert hidden textareas inside it.
    this.compositionSinkEl.remove();
  }

  /// Re-measure cell size and notify the server. Call this after a
  /// font has finished loading (`document.fonts.ready.then(...)`),
  /// since the original measurement may have been done with a
  /// fallback font that has different metrics.
  remeasureCells(): void {
    if (this.destroyed) return;
    const size = this.measureCellsOrFallback();
    this.cellSize = size;
    // Caller explicitly asked us to (re)report metrics — clear the
    // dedup cache so a same-dims-but-different-font remeasure still
    // hits the wire. In production the cell metrics usually do
    // change after a font load, but a test or a webfont identical
    // to the fallback would otherwise no-op through `sendResize`.
    this.lastSentResizeKey = null;
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
    // when the inner buttons / spans steal focus on click — and so
    // CSS `:focus-within` selectors can style the active terminal.
    this.root.tabIndex = 0;
    // Mount the composition sink. It lives inside the root so a
    // `:focus-within` from the textarea cascades up; keyboard +
    // composition events that fire on it bubble to the root's
    // listeners.
    this.root.appendChild(this.compositionSinkEl);
    this.root.appendChild(this.searchBarEl);
    this.root.appendChild(this.contextMenuEl);
    this.root.appendChild(this.keyboardBtnEl);
    this.root.appendChild(this.toastEl);
    // `queueMicrotask` rather than synchronous focus — jsdom can
    // assert during construction if the element isn't yet visible.
    // Focus targets the composition sink so the browser engages its
    // IME engine. Keyboard events bubble to the root listener via
    // the normal capture/bubble path.
    queueMicrotask(() => {
      if (!this.destroyed) this.focusInputSink();
    });
    this.root.addEventListener("keydown", this.onKeyDownHandler);
    this.root.addEventListener("wheel", this.onWheelHandler, {
      passive: false,
    });
    // Refocus the root after any inner mousedown so workspace-tab or
    // pane-tile clicks don't steal keyboard focus from the terminal.
    // Also starts the selection drag when the mousedown landed on a
    // tile (see `handleRootMouseDown`).
    // Capture-phase mousedown must register before the bubble-phase
    // sibling so DOM-order doesn't matter for our intercept.
    this.root.addEventListener(
      "mousedown",
      this.onCaptureMouseDownHandler,
      { capture: true },
    );
    this.root.addEventListener("mousedown", this.onMouseDownHandler);
    // Hover feedback for column / tile resize borders. Passive — we
    // never call `preventDefault`, just toggle the cursor style.
    this.root.addEventListener("mousemove", this.onRootMouseMoveHandler, {
      passive: true,
    });
    // Right-click context-menu suppression for mouse-reporting panes.
    // `preventDefault` on the mousedown alone isn't enough — browsers
    // fire `contextmenu` as a separate event and only honor a default-
    // prevent on *that* one consistently across engines.
    this.root.addEventListener("contextmenu", this.onContextMenuHandler);
    // IME composition. Browsers fire these on any focusable element
    // that receives keyboard input — the root is `tabIndex=0`, so the
    // events bubble here while the user is composing (Pinyin, Kana,
    // dead-keys, …). The commit goes through `sendInput` on
    // `compositionend`; intermediate state drives the pre-edit
    // overlay on the active pane's renderer.
    this.root.addEventListener("compositionstart", this.onCompositionStartHandler);
    this.root.addEventListener("compositionupdate", this.onCompositionUpdateHandler);
    this.root.addEventListener("compositionend", this.onCompositionEndHandler);
    // `beforeinput` + `input` on the sink let us pick up the commit
    // text on browsers that surface it via the editable-element
    // input path (Safari, some mobile IMEs) rather than via
    // `compositionend.data`. Listening on both is intentional: some
    // engines skip `beforeinput` for IME paths, others fire `input`
    // after `compositionend` (Firefox's order — see the deferred
    // finalization in `onCompositionEnd`). We don't listen on the
    // root because the spec doesn't guarantee bubbling for
    // `beforeinput` from form controls in every engine.
    this.compositionSinkEl.addEventListener(
      "beforeinput",
      this.onSinkBeforeInputHandler,
    );
    this.compositionSinkEl.addEventListener(
      "input",
      this.onSinkInputHandler,
    );
    // Native paste (Cmd/Ctrl+V, right-click "Paste", mobile) lands on
    // the focused sink — the permission-free paste path.
    this.compositionSinkEl.addEventListener("paste", this.onPasteHandler);
    // `focus` doesn't bubble, so listening on root catches only
    // focus events whose target is the root itself (Tab-in, or a
    // programmatic `root.focus()` from host code). Inner focus
    // (textarea, tile elements) doesn't reach here.
    this.root.addEventListener("focus", this.onRootFocusHandler);
    // Touch gesture layer (tap → keyboard, long-press → select + menu).
    // pointermove is non-passive so selection-drag can preventDefault
    // the browser's native pan/scroll once the long-press takes over.
    this.root.addEventListener("pointerdown", this.onTouchPointerDownHandler, {
      passive: true,
    });
    this.root.addEventListener("pointermove", this.onTouchPointerMoveHandler, {
      passive: false,
    });
    this.root.addEventListener("pointerup", this.onTouchPointerUpHandler, {
      passive: true,
    });
    this.root.addEventListener(
      "pointercancel",
      this.onTouchPointerCancelHandler,
      { passive: true },
    );
    // Keep the terminal above the on-screen keyboard: when the visual
    // viewport shrinks, clamp the root height to it (a no-op on desktop
    // where the visual and layout viewports match).
    this.doc.defaultView?.visualViewport?.addEventListener(
      "resize",
      this.onVisualViewportResizeHandler,
    );
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
  /// Long-press delay before a stationary touch becomes a text-select
  /// gesture. ~450ms matches the platform long-press feel.
  private static readonly TOUCH_LONGPRESS_MS = 450;
  /// Finger travel (px) that cancels a pending long-press — at/above
  /// this the gesture is a scroll/swipe, handed back to the renderer.
  /// Kept equal to the renderer's `TOUCH_SCROLL_COMMIT_PX` (8) so there
  /// is no window where the renderer commits a scroll while the app
  /// still thinks a long-press is pending.
  private static readonly TOUCH_LONGPRESS_SLOP_PX = 8;

  private clearTouchLongPressTimer(): void {
    if (this.touchGesture?.longPressTimer != null) {
      this.doc.defaultView?.clearTimeout(this.touchGesture.longPressTimer);
      this.touchGesture.longPressTimer = null;
    }
  }

  /// Touch lands on a pane: arm a long-press timer. We do NOT
  /// preventDefault — the compat `mousedown` must bubble so the tile's
  /// FocusPane fires and the renderer's swipe handler can still claim a
  /// scroll. Mouse-reporting panes are skipped (the TUI owns touches,
  /// forwarded via the mouse path).
  private onTouchPointerDown(e: PointerEvent): void {
    if (e.pointerType !== "touch") return;
    // A tap is a gesture too — touch-only users (no hardware keyboard)
    // need this path to ever reach the notification-permission prompt.
    this.maybeAskNotifyPermission();
    if (!(e.target instanceof Element)) return;
    const tileEl = e.target.closest(".ciri-tile");
    if (!(tileEl instanceof HTMLElement)) return;
    const paneIdStr = tileEl.dataset["paneId"];
    if (paneIdStr === undefined) return;
    const paneId = BigInt(paneIdStr);
    const renderer = this.renderers.get(paneIdStr);
    const grid = this.grids.get(paneIdStr);
    if (renderer === undefined || grid === undefined) return;
    if (this.paneReportsMouse(paneId)) return; // TUI owns the touch
    this.clearTouchLongPressTimer();
    const win = this.doc.defaultView;
    const timer =
      win === null
        ? null
        : win.setTimeout(
            () => this.beginTouchSelection(),
            CiriApp.TOUCH_LONGPRESS_MS,
          );
    this.touchGesture = {
      pointerId: e.pointerId,
      startX: e.clientX,
      startY: e.clientY,
      paneId,
      renderer,
      grid,
      anchor: null,
      longPressTimer: timer,
      selecting: false,
      selectMoved: false,
    };
  }

  /// The long-press fired while the finger was held still: take over
  /// the touch for text selection. Anchor the selection at the press
  /// point and tell the renderer to stand down its swipe-scroll.
  private beginTouchSelection(): void {
    const g = this.touchGesture;
    if (g === null) return;
    g.longPressTimer = null;
    g.selecting = true;
    g.renderer.setTouchSelecting(true);
    const anchor = this.hitTestAnchor(
      { clientX: g.startX, clientY: g.startY } as MouseEvent,
      g.renderer,
      g.grid,
    );
    g.anchor = anchor;
    if (anchor !== null) {
      g.renderer.setSelection({ start: anchor, end: anchor, active: true });
      g.renderer.render(g.grid);
    }
  }

  private onTouchPointerMove(e: PointerEvent): void {
    const g = this.touchGesture;
    if (g === null || e.pointerType !== "touch" || e.pointerId !== g.pointerId) {
      return;
    }
    if (!g.selecting) {
      // Before the long-press fires, enough travel means this is a
      // scroll/swipe — cancel the long-press and let the renderer run.
      const dx = e.clientX - g.startX;
      const dy = e.clientY - g.startY;
      if (Math.hypot(dx, dy) >= CiriApp.TOUCH_LONGPRESS_SLOP_PX) {
        this.clearTouchLongPressTimer();
        this.touchGesture = null;
      }
      return;
    }
    // Selecting: extend to the cell under the finger and suppress the
    // browser's native pan so the drag stays a selection.
    e.preventDefault();
    if (g.anchor === null) return;
    const head = this.hitTestAnchor(e, g.renderer, g.grid);
    if (head === null) return;
    g.selectMoved = true;
    g.renderer.setSelection({ start: g.anchor, end: head, active: true });
    g.renderer.render(g.grid);
  }

  private onTouchPointerUp(e: PointerEvent): void {
    const g = this.touchGesture;
    if (g === null || (e.pointerType === "touch" && e.pointerId !== g.pointerId)) {
      return;
    }
    this.clearTouchLongPressTimer();
    this.touchGesture = null;
    if (g.selecting) {
      g.renderer.setTouchSelecting(false);
      if (g.selectMoved && g.anchor !== null) {
        // Real selection made — finalize it (inactive) and offer Copy.
        const cur = g.renderer.currentSelection;
        if (cur !== null) {
          g.renderer.setSelection({ ...cur, active: false });
          g.renderer.render(g.grid);
        }
        this.showContextMenu(e.clientX, e.clientY);
      } else {
        // Long-press without a drag → just the menu (Copy disabled).
        g.renderer.setSelection(null);
        g.renderer.render(g.grid);
        this.showContextMenu(e.clientX, e.clientY);
      }
      return;
    }
    // A plain tap (no long-press, no scroll commit): focus the sink
    // SYNCHRONOUSLY inside this trusted gesture so mobile browsers
    // raise the on-screen keyboard (a deferred focus does not).
    const dx = e.clientX - g.startX;
    const dy = e.clientY - g.startY;
    if (Math.hypot(dx, dy) < CiriApp.TOUCH_LONGPRESS_SLOP_PX) {
      this.focusInputSink();
      // A tap is a user gesture — good moment to drain a pending OSC 52
      // clipboard write that the server pushed without activation.
      this.flushPendingClipboard();
    }
  }

  /// Pointer sequence cancelled by the browser/OS. Tear down the touch
  /// gesture WITHOUT finalizing: no menu, no focus, no kept selection —
  /// the user didn't complete an action, the system aborted it.
  private onTouchPointerCancel(e: PointerEvent): void {
    const g = this.touchGesture;
    if (g === null || (e.pointerType === "touch" && e.pointerId !== g.pointerId)) {
      return;
    }
    this.clearTouchLongPressTimer();
    this.touchGesture = null;
    if (g.selecting) {
      g.renderer.setTouchSelecting(false);
      g.renderer.setSelection(null);
      g.renderer.render(g.grid);
    }
  }

  /// Visual-viewport shrank/grew (on-screen keyboard show/hide, URL bar
  /// collapse, …). Pin the root's height to the visible region so the
  /// terminal isn't hidden behind the keyboard; the ResizeObserver then
  /// re-measures rows/cols and sends a Resize. No-op when the visual
  /// viewport matches the layout viewport (desktop / no keyboard).
  private onVisualViewportResize(): void {
    const vv = this.doc.defaultView?.visualViewport;
    if (vv === undefined || vv === null) return;
    const win = this.doc.defaultView;
    const full = win?.innerHeight ?? vv.height;
    if (vv.height < full - 1) {
      this.root.style.height = `${Math.round(vv.height)}px`;
    } else {
      // Keyboard dismissed / no shrink — release the override.
      this.root.style.removeProperty("height");
    }
  }

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

  /// Translate a `MouseEvent`'s client coordinates into the
  /// viewport-relative (col, row) the server expects in `MouseInput`.
  /// Returns `null` when the pointer is outside the pane's content
  /// rect or cell metrics are unusable.
  ///
  /// The result is the screen-row index (pixel-y / cellHeight) clamped
  /// to `[0, rows-1]` — **not** offset by `scrollOffsetRows`. Mouse-
  /// aware TUIs (tmux, vim, htop, …) don't know the renderer is
  /// scrolled up: they paint the live viewport, so the row coord must
  /// match the live-viewport grid that the SGR sequence references.
  /// Subtracting the scrollback offset would map clicks below the
  /// offset onto the wrong terminal rows (and clicks above to row 0).
  /// Matches the Rust client's `pixel_to_viewport_cell` (mouse.rs:74),
  /// which similarly takes pixel-y and clamps to `grid.rows-1` without
  /// any scrollback adjustment.
  private hitTestViewportCell(
    e: MouseEvent,
    renderer: PaneRenderer,
    grid: PaneGrid,
  ): { col: number; row: number } | null {
    const rect = renderer.container.getBoundingClientRect();
    const offsetX = e.clientX - rect.left;
    const offsetY = e.clientY - rect.top;
    if (offsetX < 0 || offsetY < 0) return null;
    if (offsetX > rect.width || offsetY > rect.height) return null;
    const { cellWidth, cellHeight } = this.measuredCellSize;
    if (cellWidth <= 0 || cellHeight <= 0) return null;
    if (grid.cols < 1 || grid.rows < 1) return null;
    const col = clampNumber(
      Math.floor(offsetX / cellWidth),
      0,
      grid.cols - 1,
    );
    const row = clampNumber(
      Math.floor(offsetY / cellHeight),
      0,
      grid.rows - 1,
    );
    return { col, row };
  }

  /// `true` iff the pane's most recent grid meta has `MODE_MOUSE_REPORT`
  /// set — i.e. the TUI inside the pane enabled DECSET 1000/1002/1003
  /// and wants to receive button events instead of the terminal handling
  /// them locally for selection/scrollback.
  private paneReportsMouse(paneId: bigint): boolean {
    const grid = this.grids.get(paneId.toString());
    if (grid === undefined) return false;
    return (grid.meta.modeFlags & MODE_MOUSE_REPORT) !== 0;
  }

  /// Pixel distance from a column / tile border that counts as "on
  /// the resize handle". Matches the Rust client's `4.0` in
  /// `start_column_resize_drag` / `start_tile_resize_drag`
  /// (`crates/ciri/src/app/resize.rs`) so feel is consistent across
  /// renderers.
  private static readonly RESIZE_HIT_PX = 4;

  /// Walk the active workspace's column / tile DOM and return the
  /// first border whose edge is within `RESIZE_HIT_PX` of the
  /// pointer. Column borders take priority over tile borders — a
  /// corner shared by both reports as a column hit so the user can
  /// always grab the column edge without having to thread between
  /// stacked tile boundaries.
  private hitTestResizeBorder(
    e: MouseEvent,
  ):
    | { kind: "column"; leftEl: HTMLElement; rightEl: HTMLElement; leftIdx: number }
    | { kind: "tile"; topEl: HTMLElement; bottomEl: HTMLElement; columnIdx: number; topTileIdx: number }
    | null {
    const ws = this.layout.viewportEl;
    // jsdom (and any unmounted state) hands us a 0×0 workspace rect;
    // every border would then sit at (0,0), and a default-coords
    // mousedown would spuriously look like a resize. Bail until the
    // layout has real dimensions.
    const wsRect = ws.getBoundingClientRect();
    if (wsRect.width <= 0 || wsRect.height <= 0) return null;
    // Filter out `display: none` columns up front. A one-pane-per-
    // screen theme hides every non-active column with `display: none`
    // — but `querySelectorAll` still returns them and their
    // `getBoundingClientRect()` is `0×0`. Without this filter, the
    // computed border between the visible active column (right edge
    // ≈ workspace width) and a hidden sibling (left edge = 0) lands
    // at (workspace_width + 0) / 2 = workspace_width / 2 — i.e. the
    // middle of the visible terminal. Clicking there starts an
    // invisible resize and the capture handler swallows the click.
    //
    // The "original index" is preserved alongside each visible
    // entry so the wire `AdjustColumnSplitAt.columnIdx` still
    // references the server-side column the user is actually
    // dragging the boundary of, not the visible-list index.
    const visibleColumns: { el: HTMLElement; originalIdx: number }[] = [];
    const allColumnEls = ws.querySelectorAll<HTMLElement>(
      ":scope > .ciri-column",
    );
    for (let i = 0; i < allColumnEls.length; i += 1) {
      const el = allColumnEls[i]!;
      const r = el.getBoundingClientRect();
      if (r.width > 0 && r.height > 0) {
        visibleColumns.push({ el, originalIdx: i });
      }
    }
    // Column borders first. In a one-pane-per-screen theme there's
    // exactly one visible column → length-1 = 0 → loop body never
    // runs and we fall through to tile borders (which also won't
    // match because only one tile is visible).
    for (let i = 0; i < visibleColumns.length - 1; i += 1) {
      const left = visibleColumns[i]!;
      const right = visibleColumns[i + 1]!;
      const leftRect = left.el.getBoundingClientRect();
      const rightRect = right.el.getBoundingClientRect();
      // Use the midpoint of the gap (if any) as the border line — the
      // CSS layout may insert a small gutter via `margin` later, and
      // averaging avoids one column "owning" both sides of the gap.
      const borderX = (leftRect.right + rightRect.left) / 2;
      const inVerticalSpan =
        e.clientY >= Math.min(leftRect.top, rightRect.top) &&
        e.clientY <= Math.max(leftRect.bottom, rightRect.bottom);
      if (
        inVerticalSpan &&
        Math.abs(e.clientX - borderX) < CiriApp.RESIZE_HIT_PX
      ) {
        return {
          kind: "column",
          leftEl: left.el,
          rightEl: right.el,
          // Server-side index (not the visible-list index) so the
          // `AdjustColumnSplitAt.columnIdx` still references the
          // correct boundary.
          leftIdx: left.originalIdx,
        };
      }
    }
    // Tile borders within whichever column the pointer is inside.
    // Same `display: none` filter as columns above — a one-pane theme
    // would otherwise compute a phantom border between the active
    // tile and a hidden sibling.
    for (const col of visibleColumns) {
      const colEl = col.el;
      const colRect = colEl.getBoundingClientRect();
      if (e.clientX < colRect.left || e.clientX > colRect.right) continue;
      const visibleTiles: { el: HTMLElement; originalIdx: number }[] = [];
      const allTileEls = colEl.querySelectorAll<HTMLElement>(
        ":scope > .ciri-tile",
      );
      for (let ti = 0; ti < allTileEls.length; ti += 1) {
        const tEl = allTileEls[ti]!;
        const r = tEl.getBoundingClientRect();
        if (r.width > 0 && r.height > 0) {
          visibleTiles.push({ el: tEl, originalIdx: ti });
        }
      }
      for (let ti = 0; ti < visibleTiles.length - 1; ti += 1) {
        const top = visibleTiles[ti]!;
        const bot = visibleTiles[ti + 1]!;
        const topRect = top.el.getBoundingClientRect();
        const botRect = bot.el.getBoundingClientRect();
        const borderY = (topRect.bottom + botRect.top) / 2;
        if (Math.abs(e.clientY - borderY) < CiriApp.RESIZE_HIT_PX) {
          return {
            kind: "tile",
            topEl: top.el,
            bottomEl: bot.el,
            columnIdx: col.originalIdx,
            topTileIdx: top.originalIdx,
          };
        }
      }
    }
    return null;
  }

  /// Set the resize cursor on the workspace viewport so the user sees
  /// the hint regardless of which sub-element the pointer is over.
  /// Pass `null` to reset.
  private setResizeCursor(kind: "column" | "tile" | null): void {
    const el = this.layout.viewportEl;
    if (kind === "column") {
      el.style.cursor = "col-resize";
    } else if (kind === "tile") {
      el.style.cursor = "row-resize";
    } else {
      el.style.cursor = "";
    }
  }

  private handleRootMouseMove(e: MouseEvent): void {
    // Don't perturb the cursor while a drag is in flight — the drag
    // already pinned an appropriate icon via `setResizeCursor`.
    if (this.resizeDrag !== null) return;
    const hit = this.hitTestResizeBorder(e);
    this.setResizeCursor(hit === null ? null : hit.kind);
  }

  /// Start a column-resize drag. Captures the current column flex
  /// values and the inner viewport width so the running pixel delta
  /// can be translated to the proportion the server expects.
  private startColumnResize(
    e: MouseEvent,
    leftEl: HTMLElement,
    rightEl: HTMLElement,
    leftIdx: number,
  ): void {
    const wsRect = this.layout.viewportEl.getBoundingClientRect();
    const leftFlex = parseFloat(leftEl.style.flex) || 1;
    const rightFlex = parseFloat(rightEl.style.flex) || 1;
    this.resizeDrag = {
      kind: "column",
      leftEl,
      rightEl,
      leftIdx,
      anchorX: e.clientX,
      innerVw: wsRect.width,
      totalDelta: 0,
      leftFlex,
      rightFlex,
    };
    this.setResizeCursor("column");
    const win = this.doc.defaultView;
    if (win !== null) {
      win.addEventListener("mousemove", this.onWindowMouseMove);
      win.addEventListener("mouseup", this.onWindowMouseUp);
    }
  }

  /// Start a tile-resize drag inside a single column. Captures the
  /// current pixel heights so the running delta produces a new
  /// weight pair preserving the column's flex layout invariants.
  private startTileResize(
    e: MouseEvent,
    topEl: HTMLElement,
    bottomEl: HTMLElement,
    columnIdx: number,
    topTileIdx: number,
  ): void {
    const topRect = topEl.getBoundingClientRect();
    const botRect = bottomEl.getBoundingClientRect();
    const topFlex = parseFloat(topEl.style.flex) || 1;
    const bottomFlex = parseFloat(bottomEl.style.flex) || 1;
    this.resizeDrag = {
      kind: "tile",
      topEl,
      bottomEl,
      columnIdx,
      topTileIdx,
      anchorY: e.clientY,
      topHeight: topRect.height,
      bottomHeight: botRect.height,
      topFlex,
      bottomFlex,
    };
    this.setResizeCursor("tile");
    const win = this.doc.defaultView;
    if (win !== null) {
      win.addEventListener("mousemove", this.onWindowMouseMove);
      win.addEventListener("mouseup", this.onWindowMouseUp);
    }
  }

  /// Apply an incremental resize-drag mousemove. Returns `true` if it
  /// was handled and the caller should bail out (skip selection /
  /// mouse-forward branches). Optimistically rewrites the CSS `flex`
  /// values on the two affected elements; the eventual server
  /// `LayoutUpdate` reconciles any clamping that happened upstream.
  private applyResizeDrag(e: MouseEvent): boolean {
    const drag = this.resizeDrag;
    if (drag === null) return false;
    if (drag.kind === "column") {
      const deltaPx = e.clientX - drag.anchorX;
      if (drag.innerVw <= 0) return true;
      // Raw viewport-relative delta — NOT scaled by the pair's
      // combined proportion. The server's `resize_column_pair` adds
      // this number directly to the left column's stored viewport
      // proportion, so on a 3-column layout where each column owns
      // 1/3 of the viewport, dragging the border 80px in an 800px
      // viewport must send 0.10 (not 0.10 × 0.66) — otherwise the
      // server reconciliation snaps back to a smaller move than the
      // user drew on screen.
      const deltaProportion = deltaPx / drag.innerVw;
      // Match the server's `resized_pair_proportions` clamp
      // (`crates/ciri-layout/src/workspace.rs:749`):
      //   min_width = min(MIN_COLUMN_PROPORTION=0.05, pair/2)
      //   nextLeft  = clamp(left + delta, min_width, pair - min_width)
      // Using a 5%-of-pair clamp here (the obvious-looking math) is
      // wrong in two directions: a 3-column layout (pair ≈ 0.66)
      // would let the user drag below 0.05 absolute, and the server
      // would snap-back on the next `LayoutUpdate`; conversely a
      // pair > 1 (e.g. a column-set whose proportions sum > 1 after
      // server-side reconciliation) would forbid widths the server
      // is happy to accept.
      const pairTotal = drag.leftFlex + drag.rightFlex;
      const minWidth = Math.min(0.05, pairTotal / 2);
      const nextLeft = clampNumber(
        drag.leftFlex + deltaProportion,
        minWidth,
        pairTotal - minWidth,
      );
      const nextRight = pairTotal - nextLeft;
      drag.leftEl.style.flex = String(nextLeft);
      drag.rightEl.style.flex = String(nextRight);
      // Accumulate the clamp-adjusted delta — what the user actually
      // moved, not what they asked for past the limit. Re-anchor on
      // every move so the next frame's `deltaPx` is incremental
      // (matches Rust's `col_start_x = mx` update in
      // `apply_column_resize_drag`).
      drag.totalDelta += nextLeft - drag.leftFlex;
      drag.leftFlex = nextLeft;
      drag.rightFlex = nextRight;
      drag.anchorX = e.clientX;
      return true;
    }
    // Tile resize.
    const deltaPx = e.clientY - drag.anchorY;
    const totalH = drag.topHeight + drag.bottomHeight;
    if (totalH <= 0) return true;
    // Match server's `clamped_tile_pair_height`
    // (`crates/ciri-layout/src/workspace.rs:792-794`): a hard 30px
    // floor on each tile, not a percentage. A 5%-of-pair clamp would
    // let a 300px-tall column shrink one tile to 15px — the wire
    // message would round-trip but the server snaps back on the
    // first `LayoutUpdate`.
    const MIN_TILE_HEIGHT_PX = 30;
    const lo = Math.min(MIN_TILE_HEIGHT_PX, totalH / 2);
    const nextTopH = clampNumber(drag.topHeight + deltaPx, lo, totalH - lo);
    const nextBotH = totalH - nextTopH;
    // Preserve the column's weight sum so neighbor tiles outside
    // this pair aren't visually rescaled by the local update.
    const totalFlex = drag.topFlex + drag.bottomFlex;
    const nextTopFlex = (nextTopH / totalH) * totalFlex;
    const nextBotFlex = totalFlex - nextTopFlex;
    drag.topEl.style.flex = String(nextTopFlex);
    drag.bottomEl.style.flex = String(nextBotFlex);
    drag.topFlex = nextTopFlex;
    drag.bottomFlex = nextBotFlex;
    drag.topHeight = nextTopH;
    drag.bottomHeight = nextBotH;
    drag.anchorY = e.clientY;
    return true;
  }

  /// Commit the running resize-drag to the server and tear the local
  /// state down. Called from the window-level `mouseup` path. Returns
  /// `true` if a resize was active so the caller knows to skip its
  /// selection / mouse-forward release branches.
  private finishResizeDrag(): boolean {
    const drag = this.resizeDrag;
    if (drag === null) return false;
    if (drag.kind === "column") {
      this.client.send({
        tag: "AdjustColumnSplitAt",
        columnIdx: BigInt(drag.leftIdx),
        delta: drag.totalDelta,
      });
    } else {
      this.client.send({
        tag: "SetTileWeights",
        columnIdx: BigInt(drag.columnIdx),
        topTileIdx: BigInt(drag.topTileIdx),
        topWeight: drag.topFlex,
        bottomWeight: drag.bottomFlex,
      });
    }
    this.resizeDrag = null;
    this.setResizeCursor(null);
    const win = this.doc.defaultView;
    if (win !== null) {
      win.removeEventListener("mousemove", this.onWindowMouseMove);
      win.removeEventListener("mouseup", this.onWindowMouseUp);
    }
    return true;
  }

  private handleContextMenu(e: MouseEvent): void {
    // Any right-click resets a previously-open menu first, so a stale
    // one never lingers (e.g. right-click pane A, then pane B).
    this.hideContextMenu();
    // Only swallow the menu when the click lands on a mouse-reporting
    // pane: a contextmenu over the workspace tab strip or a non-
    // reporting pane should still surface the platform menu so the
    // user can paste / inspect / etc.
    const target = e.target;
    if (!(target instanceof Element)) return;
    const tileEl = target.closest(".ciri-tile");
    if (!(tileEl instanceof HTMLElement)) return;
    const paneIdStr = tileEl.dataset["paneId"];
    if (paneIdStr === undefined) return;
    const paneId = BigInt(paneIdStr);
    // Mouse-reporting panes (vim/tmux/…) own the right-click unless the
    // user holds Shift — same passthrough gate as left-click. In that
    // case swallow the browser menu so the TUI gets the event and we
    // show nothing of our own.
    if (this.paneReportsMouse(paneId) && !e.shiftKey) {
      e.preventDefault();
      return;
    }
    // Otherwise show our Copy / Paste / Find menu at the click point.
    e.preventDefault();
    this.showContextMenu(e.clientX, e.clientY);
  }

  /// Show the context menu at viewport coordinates `(x, y)`. The Copy
  /// item is disabled when no pane has a selection. Installs a one-shot
  /// outside-pointerdown / Escape / scroll listener to dismiss.
  private showContextMenu(x: number, y: number): void {
    // Detach any prior open menu's window listeners before installing a
    // new set — reopening (right-click again, or long-press after a
    // right-click) would otherwise leak the previous dismiss listeners.
    this.hideContextMenu();
    this.contextMenuCopyBtn.disabled = !this.hasSelection();
    this.contextMenuEl.style.left = `${x}px`;
    this.contextMenuEl.style.top = `${y}px`;
    this.contextMenuEl.hidden = false;
    const win = this.doc.defaultView;
    if (win === null) return;
    // Defer install one tick so the contextmenu event that opened the
    // menu doesn't immediately dismiss it.
    const dismiss = (ev: Event): void => {
      if (
        ev.type === "pointerdown" &&
        ev.target instanceof Node &&
        this.contextMenuEl.contains(ev.target)
      ) {
        return; // click inside the menu — let the item handler run
      }
      if (ev.type === "keydown" && (ev as KeyboardEvent).key !== "Escape") {
        return;
      }
      this.hideContextMenu();
    };
    this.contextMenuDismiss = dismiss;
    win.setTimeout(() => {
      if (this.destroyed || this.contextMenuDismiss !== dismiss) return;
      win.addEventListener("pointerdown", dismiss, true);
      win.addEventListener("keydown", dismiss, true);
      win.addEventListener("blur", dismiss, true);
      this.root.addEventListener("wheel", dismiss, true);
    }, 0);
  }

  private hideContextMenu(): void {
    if (this.contextMenuEl.hidden) return;
    this.contextMenuEl.hidden = true;
    const win = this.doc.defaultView;
    if (win !== null && this.contextMenuDismiss !== null) {
      win.removeEventListener("pointerdown", this.contextMenuDismiss, true);
      win.removeEventListener("keydown", this.contextMenuDismiss, true);
      win.removeEventListener("blur", this.contextMenuDismiss, true);
      this.root.removeEventListener("wheel", this.contextMenuDismiss, true);
    }
    this.contextMenuDismiss = null;
  }

  /// Whether any pane currently has a non-empty selection — gates the
  /// Copy menu item.
  private hasSelection(): boolean {
    for (const renderer of this.renderers.values()) {
      if (renderer.currentSelection !== null) return true;
    }
    return false;
  }

  private handleCaptureMouseDown(e: MouseEvent): void {
    // Only left button starts a resize — a right-click on the border
    // should still surface the browser context menu, and middle is
    // unused here.
    if (e.button !== 0) return;
    const hit = this.hitTestResizeBorder(e);
    if (hit === null) return;
    // Suppress the tile-level bubble listener that would otherwise
    // queue a `FocusPane` (and trigger an immediate `LayoutUpdate`
    // that rebuilds the DOM nodes this drag is about to mutate).
    e.preventDefault();
    e.stopImmediatePropagation();
    if (hit.kind === "column") {
      this.startColumnResize(e, hit.leftEl, hit.rightEl, hit.leftIdx);
    } else {
      this.startTileResize(
        e,
        hit.topEl,
        hit.bottomEl,
        hit.columnIdx,
        hit.topTileIdx,
      );
    }
    // The bubble-phase mousedown handler (`onMouseDownHandler`)
    // would normally queue a `focusInputSink()` so subsequent
    // typing routes to the terminal. We just suppressed it via
    // `stopImmediatePropagation`, so do the focus pass ourselves —
    // otherwise a resize started from outside-the-terminal focus
    // (e.g. the status strip or the page chrome) finishes with the
    // sink still unfocused and keystrokes go to the wrong target.
    queueMicrotask(() => {
      if (!this.destroyed) this.focusInputSink();
    });
  }

  private handleRootMouseDown(e: MouseEvent): void {
    // Browser button indices: 0=left, 1=middle, 2=right. Everything
    // else (back/forward buttons, stylus tips, …) is too platform-
    // specific to forward usefully; let the browser keep them.
    if (e.button !== 0 && e.button !== 1 && e.button !== 2) return;
    // Border-hit resize is handled by the capture-phase sibling
    // (`handleCaptureMouseDown`) so it fires before the tile's
    // FocusPane listener; if we're here in the bubble phase, the
    // capture handler already let the event through, so this is
    // definitely not a border press.
    const target = e.target;
    if (!(target instanceof Element)) return;
    const tileEl = target.closest(".ciri-tile");
    if (!(tileEl instanceof HTMLElement)) return;
    const paneIdStr = tileEl.dataset["paneId"];
    if (paneIdStr === undefined) return;
    const renderer = this.renderers.get(paneIdStr);
    const grid = this.grids.get(paneIdStr);
    if (renderer === undefined || grid === undefined) return;
    const paneId = BigInt(paneIdStr);
    // Mouse-aware TUI (tmux, vim, htop, lazygit, …) handles its own
    // clicks; shift-click overrides so the user can still pull text
    // out of the pane for copy. Mirrors the Rust client's
    // `pane_prefers_mouse_passthrough` (mouse.rs:159).
    const passthrough = this.paneReportsMouse(paneId) && !e.shiftKey;
    if (passthrough) {
      const cell = this.hitTestViewportCell(e, renderer, grid);
      if (cell === null) return;
      e.preventDefault();
      // Drop any leftover selection — entering a mouse-forward drag
      // gives the TUI ownership of the click stream.
      if (renderer.currentSelection !== null) {
        renderer.setSelection(null);
        renderer.render(grid);
      }
      this.mouseForward = {
        paneId,
        renderer,
        grid,
        lastCell: cell,
        button: e.button,
      };
      // Only forward the initial press when the user clicks the
      // already-active pane. A click that switches focus would
      // otherwise inject a phantom click into the newly-focused TUI
      // (e.g. neovim would enter visual mode on the first focus
      // click). Subsequent motion/release still forward — matches
      // the Rust client's `was_already_focused` gate (mouse.rs:339).
      // `currentLayout` is the server-confirmed active pane; the
      // optimistic `pendingFocusedPaneId` shortcut is intentionally
      // not consulted — it can race with click-to-focus and lie
      // about which pane "currently" has focus.
      const serverActive =
        this.currentLayout !== null
          ? LayoutManager.activePaneId(this.currentLayout)
          : null;
      if (serverActive === paneId) {
        this.client.send({
          tag: "MouseInput",
          paneId,
          // Xterm uses the same 0/1/2 numbering as the browser's
          // `MouseEvent.button` field for left/middle/right press.
          button: e.button,
          col: cell.col,
          row: cell.row,
          pressed: true,
          modifiers: mouseModifierMask(e),
        });
      }
      const win = this.doc.defaultView;
      if (win !== null) {
        win.addEventListener("mousemove", this.onWindowMouseMove);
        win.addEventListener("mouseup", this.onWindowMouseUp);
      }
      return;
    }
    if (e.button !== 0) return;
    const anchor = this.hitTestAnchor(e, renderer, grid);
    if (anchor === null) return;
    // Existing selection from a previous drag is replaced wholesale —
    // the user is starting a fresh selection at the new anchor.
    this.dragState = {
      paneId,
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
    if (this.applyResizeDrag(e)) return;
    const fwd = this.mouseForward;
    if (fwd !== null) {
      const cell = this.hitTestViewportCell(e, fwd.renderer, fwd.grid);
      if (cell === null) return;
      // Suppress motion frames that don't cross a cell boundary —
      // browsers fire `mousemove` per pixel, but the wire encoding
      // is per cell. Mirrors xterm's "report on cell change" behavior
      // (DECSET 1002); 1003 sends every motion regardless of button
      // but we don't enable it from the client side.
      if (
        cell.col === fwd.lastCell.col &&
        cell.row === fwd.lastCell.row
      ) {
        return;
      }
      fwd.lastCell = cell;
      // Xterm motion encoding: 32 + button-index (32=B1, 33=B2,
      // 34=B3). Server's SGR encoder shifts modifiers into the
      // upper bits separately; we send the base motion code here.
      // `pressed: true` because the SGR `M`/`m` suffix marks
      // press vs release — motion frames always use `M`.
      this.client.send({
        tag: "MouseInput",
        paneId: fwd.paneId,
        button: 32 + fwd.button,
        col: cell.col,
        row: cell.row,
        pressed: true,
        modifiers: mouseModifierMask(e),
      });
      return;
    }
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

  private handleWindowMouseUp(e: MouseEvent): void {
    if (this.finishResizeDrag()) return;
    const fwd = this.mouseForward;
    if (fwd !== null) {
      const win = this.doc.defaultView;
      if (win !== null) {
        win.removeEventListener("mousemove", this.onWindowMouseMove);
        win.removeEventListener("mouseup", this.onWindowMouseUp);
      }
      // Use the press's last-known cell when the up-event lands
      // outside the pane's content rect (e.g. the user dragged
      // the pointer off the window before releasing) — sending
      // `null` viewport coords would silently drop the release and
      // strand the TUI with a never-finished press.
      const cell =
        this.hitTestViewportCell(e, fwd.renderer, fwd.grid) ??
        fwd.lastCell;
      this.client.send({
        tag: "MouseInput",
        paneId: fwd.paneId,
        // Button 3 = "any release" in xterm. The protocol carries
        // press/release in the `pressed` field separately, but the
        // wire convention for SGR is to use button-3 on release
        // regardless of which button was pressed.
        button: 3,
        col: cell.col,
        row: cell.row,
        pressed: false,
        modifiers: mouseModifierMask(e),
      });
      this.mouseForward = null;
      return;
    }
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
      if (this.destroyed) return;
      this.sendResize();
      // Re-anchor the sink: a window resize moves the pane rect
      // and thus the absolute cursor coords the sink relies on.
      // Round-8 codex P2.
      if (this.composing) this.repositionCompositionSink();
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

  /// Server-side dim cap (`crates/ciri-server/.../session_mgmt.rs`
  /// validates `width <= 16384, height <= 16384`). The viewport lie
  /// below can blow past this on highly-asymmetric layouts (an
  /// active column with `widthProportion = 0.02`, say, would scale
  /// real_W × 50). Clamp before sending — a partial lie is still
  /// better than a Resize the server rejects outright.
  private static readonly MAX_VIEWPORT_DIM = 16384;

  /// Lie about the viewport so the server's `widthProportion`-based
  /// column / `weight`-based tile math ends up allocating the
  /// targeted pane at the real screen dimensions.
  ///
  /// Math (see protocol discussion in commit message): for the
  /// active column with share `p_col` of the total column-weight
  /// sum, server gives that column `lied_w × p_col` px. We want
  /// that = real_w. So `lied_w = real_w / p_col`. Same for tiles
  /// within the active column via `p_tile`.
  ///
  /// `targetPaneId === null` (no layout yet, or no active pane)
  /// → fall through to real dims so the initial `ClientHello`
  /// doesn't over-allocate.
  ///
  /// Cell metrics are NOT lied about — server takes the min across
  /// clients for the cell size, and the active pane's grid size is
  /// `real_w / cell_w` regardless of the lied viewport.
  private computeLiedViewportFor(targetPaneId: bigint | null): {
    width: number;
    height: number;
    cols: number;
    rows: number;
    cellWidth: number;
    cellHeight: number;
  } {
    const rect = this.viewportRect();
    const realW = Math.max(1, Math.floor(rect.width));
    const realH = Math.max(1, Math.floor(rect.height));
    const { cellWidth, cellHeight } = this.cellSize;
    // Active pane's grid in cells — what the renderer will paint
    // into once the server's resize round-trip settles.
    const { cols, rows } = cellsForViewport(realW, realH, this.cellSize);

    // No layout yet (initial `ClientHello`) or no specific target —
    // report real dims, no lie. The first `LayoutUpdate` will
    // trigger a follow-up `sendResize` with the correct lie.
    if (this.currentLayout === null || targetPaneId === null) {
      return {
        width: realW,
        height: realH,
        cols,
        rows,
        cellWidth,
        cellHeight,
      };
    }

    // Walk the layout to find which column / tile owns `targetPaneId`.
    //
    // Important asymmetry between column and tile sizing on the
    // server side:
    //
    //   - Column width is computed as `inner_vw × widthProportion`,
    //     using the **raw** proportion. The proportions do NOT
    //     sum to 1 across columns (the server's `MIN_COLUMN_PROPORTION`
    //     clamp + per-pair `AdjustColumnSplit` math intentionally
    //     leaves them un-normalized — 3 cols at p=0.5 each is the
    //     default after pressing `+` twice). Web must mirror this:
    //     `lied_w = real_w / p_col` with raw p_col.
    //
    //   - Tile height inside a column IS normalized by the column's
    //     `total_weight`: `tile_h = auto_h × (weight / total_weight)`
    //     (`crates/ciri-layout/src/column.rs:tile_rects`). Mirror
    //     that for `p_tile`.
    let pCol = 1;
    let pTile = 1;
    let found = false;
    outer: for (const ws of this.currentLayout.workspaces) {
      for (const col of ws.columns) {
        let totalTileWeight = 0;
        for (const t of col.tiles) totalTileWeight += t.weight;
        if (totalTileWeight <= 0) totalTileWeight = 1;
        for (const tile of col.tiles) {
          if (tile.paneId === targetPaneId) {
            pCol = col.widthProportion;
            pTile = tile.weight / totalTileWeight;
            found = true;
            break outer;
          }
        }
      }
    }
    if (!found) {
      // Pane not in the current layout (race against a `PaneClosed`).
      // Use real dims; the next `LayoutUpdate` reconciles.
      return {
        width: realW,
        height: realH,
        cols,
        rows,
        cellWidth,
        cellHeight,
      };
    }

    // Guard against zero proportions (server's `MIN_COLUMN_PROPORTION`
    // is 0.05, so 0.001 here is just a div-by-zero safety net).
    const safePCol = Math.max(0.001, pCol);
    const safePTile = Math.max(0.001, pTile);
    const width = Math.min(
      CiriApp.MAX_VIEWPORT_DIM,
      Math.max(1, Math.floor(realW / safePCol)),
    );
    const height = Math.min(
      CiriApp.MAX_VIEWPORT_DIM,
      Math.max(1, Math.floor(realH / safePTile)),
    );
    return { width, height, cols, rows, cellWidth, cellHeight };
  }

  private computeViewport(sessionName: string): ClientHello {
    // First handshake has no `currentLayout` yet, so this resolves
    // to real dims. The first `LayoutUpdate`'s `sendResizeForPane`
    // re-issues a proper lied Resize.
    const v = this.computeLiedViewportFor(null);
    return {
      sessionName,
      width: v.width,
      height: v.height,
      cellWidth: v.cellWidth,
      cellHeight: v.cellHeight,
    };
  }

  /// Re-send a `Resize` for the pane the user is (or is about to be)
  /// focused on. Dedup'd: identical-dims back-to-back sends are
  /// skipped, so calling this on every `LayoutUpdate` and every
  /// `ResizeObserver` tick is cheap and converges quickly.
  private sendResizeForPane(targetPaneId: bigint | null): void {
    const v = this.computeLiedViewportFor(targetPaneId);
    const key = `${v.width}x${v.height}@${v.cols}x${v.rows}@${v.cellWidth.toFixed(2)}x${v.cellHeight.toFixed(2)}`;
    if (key === this.lastSentResizeKey) return;
    this.lastSentResizeKey = key;
    this.client.send({
      tag: "Resize",
      cols: v.cols,
      rows: v.rows,
      width: v.width,
      height: v.height,
      cellWidth: v.cellWidth,
      cellHeight: v.cellHeight,
    });
  }

  private sendResize(): void {
    // ResizeObserver tick: re-aim the lie at the current active pane.
    const active =
      this.currentLayout !== null
        ? LayoutManager.activePaneId(this.currentLayout)
        : null;
    this.sendResizeForPane(active);
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
    // Keys aimed at the chrome's own buttons (workspace tabs, pane
    // chips, future toolbars) belong to those controls — Enter/Space
    // activates them, arrow keys may move focus within a toolbar.
    // Forwarding to the PTY would double-effect: the chip fires
    // *and* the terminal eats a CR/space. Skip when the focused
    // element is a `<button>` inside our chrome.
    // A keystroke is a user gesture — drain any OSC 52 clipboard write
    // that arrived without activation. Cheap no-op when nothing pends.
    this.flushPendingClipboard();
    if (e.target instanceof HTMLButtonElement) return;
    // The find bar is a focus island: its own input/buttons handle
    // their keys (and stop propagation), but guard here too so a key
    // that does bubble (e.g. an unhandled one) never reaches the PTY.
    if (e.target instanceof Node && this.searchBarEl.contains(e.target)) return;
    // While an IME composition is in flight the keystrokes belong to
    // the input method — Enter/Space "select candidate", Esc "cancel",
    // arrow keys "navigate candidate list". Forwarding them to the
    // PTY would double-send characters as the candidate moves. The
    // encoder itself also drops `isComposing` events; this is the
    // outer layer that also guards against the rare event where
    // `isComposing` isn't set on the very first keydown that
    // triggered `compositionstart`.
    if (e.isComposing || this.composing) return;
    // If a deferred late-commit is pending AND its commit data has
    // already been captured (Chrome-style: `beforeinput` fired
    // before this keystroke), flush synchronously so the IME bytes
    // hit the wire BEFORE this keystroke. Without this, the
    // macrotask order would scramble user-perceived input order.
    //
    // If the commit data hasn't been captured yet (Firefox-style:
    // `input` fires AFTER `compositionend`), do NOT flush — that
    // would close the late-commit window prematurely and DROP the
    // committed glyph entirely. Let the macrotask handle it; the
    // wire order may end up keystroke-then-commit, which is a
    // visible weirdness the user can correct, while dropping the
    // commit is silent data loss. Round-13 codex P2 trade-off
    // (refines round-8's blanket flush).
    if (
      this.pendingLateCommit !== null &&
      this.compositionCommitData !== null &&
      this.compositionCommitData.generation === this.pendingLateCommit.generation
    ) {
      this.flushPendingLateCommit();
    }
    // Clipboard chords intercept *before* the encoder runs: the
    // encoder rejects Ctrl+Shift+anything and Cmd+anything (the
    // browser-reserved escape hatches), but Ctrl+Shift+C / Cmd+C /
    // Ctrl+Shift+V / Cmd+V are the canonical terminal copy/paste
    // bindings and we DO want to handle them.
    if (this.tryHandleClipboardChord(e)) return;
    // Take this gesture as the chance to ask for notification permission
    // (cheap no-op unless something wanted to notify). Deliberately
    // AFTER the clipboard chords: `Notification.requestPermission()`
    // would otherwise spend the transient user activation that
    // Ctrl+Shift+V's `clipboard.readText()` depends on.
    this.maybeAskNotifyPermission();
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

  /// Composition just started — the user has begun a multi-keystroke
  /// glyph entry (Pinyin sequence, IME prompt, dead-key chain). Note
  /// the target pane (used to drive the overlay) but allow it to
  /// follow active focus on subsequent updates / layout changes.
  private onCompositionStart(_e: CompositionEvent): void {
    this.composing = true;
    this.composingPaneId = this.activePaneId();
    this.compositionCommitData = null;
    // Close the deferred-commit window for any previous session —
    // a stale late `input` arriving now must be rejected rather
    // than allowed to leak into this session's commit slot.
    this.pendingLateCommit = null;
    // Also flush any leftover text in the sink. The macrotask
    // scheduled by a previous `compositionend` would normally clear
    // it, but a generation mismatch (rapid back-to-back sessions)
    // makes the macrotask bail before doing so. Clearing here
    // closes that gap. Round-11 codex P3.
    this.compositionSinkEl.value = "";
    // Bump the generation so any deferred finalization scheduled by a
    // previous `compositionend` knows its session is over and bails.
    this.compositionGeneration += 1;
    this.repositionCompositionSink();
  }

  /// In-progress preedit text. Browsers fire one of these per
  /// candidate-list keystroke; `data` is the cumulative preedit
  /// string. Drive the renderer overlay; do NOT touch the wire (the
  /// commit happens on `compositionend`).
  ///
  /// Critically: this handler does NOT re-resolve the composing
  /// pane via `activePaneId()` (round-7 codex P1 fix). Once
  /// composition is in flight, the target is locked to whatever
  /// was captured at `compositionstart` plus any *server-driven*
  /// retargets via `applyLayout`. Consulting `activePaneId()` here
  /// would route the preedit (and the eventual commit) through
  /// `pendingFocusedPaneId`, the optimistic local click shortcut,
  /// which means a mid-composition click on another pane could
  /// steer the user's in-flight glyph to the wrong terminal —
  /// potentially leaking command text or secrets across panes.
  private onCompositionUpdate(e: CompositionEvent): void {
    const target = this.composingPaneId;
    if (target === null) return;
    const key = target.toString();
    const renderer = this.renderers.get(key);
    const grid = this.grids.get(key);
    if (renderer === undefined || grid === undefined) return;
    renderer.setPreedit(e.data);
    renderer.render(grid);
    // NOTE: do NOT mutate `compositionSinkEl.value` here. The
    // browser owns an active composition range on this textarea
    // during preedit; assigning to `.value` invalidates that range
    // and can cancel or corrupt the in-flight composition on real
    // engines (round-10 codex P2 — round-8's clipboard-leak
    // mitigation reverted; the cure was worse than the disease).
    // The sink is cleared on compositionend instead, which is
    // when the composition range is gone anyway.
    this.repositionCompositionSink();
  }

  /// Capture the commit text from an editable-element `beforeinput`
  /// path. Browsers that don't ship a useful `compositionend.data`
  /// (Safari, certain mobile IMEs) surface the committed glyph here
  /// via `inputType=insertFromComposition` or via a post-composition
  /// `insertText`. Intermediate `insertCompositionText` ticks fire
  /// while the user is still picking a candidate — those carry
  /// preedit content, not a commit, so we filter them out: otherwise
  /// a cancel-with-dirty-sink would incorrectly send the last
  /// preedit text to the PTY.
  ///
  /// Each accepted commit-signal is tagged with the current
  /// `compositionGeneration`, and the two `inputType` branches
  /// require distinct state windows so a stale event from a
  /// previous session can't populate the next session's commit
  /// slot (round-6 codex fix):
  ///   - `insertFromComposition` is only honored while
  ///     `this.composing` is true.
  ///   - `insertText` with `!isComposing` is only honored while
  ///     `this.pendingLateCommit` is set (we're inside the brief
  ///     post-compositionend window). The `compositionstart` of a
  ///     new session wipes that flag.
  private onSinkBeforeInput(e: Event): void {
    const ie = e as InputEvent;
    const data = ie.data;
    if (typeof data !== "string" || data.length === 0) return;
    if (ie.inputType === "insertFromComposition") {
      if (!this.composing) return; // stale / anomalous
      this.compositionCommitData = {
        generation: this.compositionGeneration,
        text: data,
      };
      return;
    }
    if (ie.inputType === "insertText" && !ie.isComposing) {
      if (this.pendingLateCommit === null) {
        // Direct IME commit OUTSIDE any composition. Chinese/Japanese
        // full-width punctuation (，。？！「」…) and other "commit on a
        // single keystroke" inputs arrive as a standalone `insertText`
        // with no surrounding compositionstart/end. The matching
        // keydown is IME-routed (`key="Process"` / keyCode 229) so the
        // encoder drops it — without this branch the glyph is lost
        // entirely. The native client gets a clean `Ime::Commit` from
        // winit (app/ime.rs); in the browser we reconstruct it from the
        // editable sink, the same way xterm.js reads text from its
        // hidden textarea and ignores the 229 keydown.
        //
        // Gate on `!this.composing`: a stray `insertText` (isComposing
        // reported false) that arrives while we KNOW a composition is
        // active is a stale event from a prior session whose
        // compositionstart already wiped the late-commit flag — not a
        // real direct commit. Dropping it avoids leaking old preedit
        // text (round-6 codex P2 scenario).
        if (!this.composing) this.handleDirectInsert(e, data);
        return;
      }
      // Single-acceptance: the first matching late `input` wins.
      // Subsequent insertText events during the same window are
      // likely stray keystrokes / dead-key processing — don't let
      // them overwrite the legitimate IME commit. Round-8 codex
      // P2 defense.
      if (this.compositionCommitData !== null) return;
      this.compositionCommitData = {
        generation: this.pendingLateCommit.generation,
        text: data,
      };
    }
    // Deliberate skip: `insertCompositionText` is an intermediate
    // preedit step (NOT a commit) per the Input Events spec; using
    // it as commit data would conflate cancel with commit.
  }

  /// Composition ended. `e.data` carries the final committed text
  /// (empty string when the user canceled — e.g. Esc out of the
  /// candidate list). On commit, push the bytes through the regular
  /// `sendInput` path so server-side terminal apps see the same UTF-8
  /// they would have seen from a non-IME keystroke. Always clear the
  /// preedit overlay regardless.
  private onCompositionEnd(e: CompositionEvent): void {
    // Snapshot the commit target — the pane the preedit was on at
    // composition end. Deliberately NOT `activePaneId()`: that helper
    // honors the optimistic `pendingFocusedPaneId` shortcut used to
    // route keystrokes immediately after a click, but a mid-
    // composition click would then mis-route the commit to a pane
    // the user wasn't composing in (round-5 codex fix —
    // wrong-terminal-input bug). `composingPaneId` tracks the
    // overlay's actual pane and is kept in sync by `applyLayout`
    // when a server-driven LayoutUpdate moves the active pane
    // during composition, so server-driven retargets still flow
    // through.
    const target = this.composingPaneId;
    this.composing = false;
    this.composingPaneId = null;
    // Clear the overlay on whatever pane was showing it.
    if (target !== null) this.clearPreeditOn(target);

    const generation = this.compositionGeneration;
    const eventData = e.data ?? "";
    if (eventData.length > 0) {
      // Spec-compliant path: `compositionend.data` carries the commit.
      this.finalizeCompositionCommit(eventData, target);
      return;
    }
    const captured = this.compositionCommitData;
    if (captured !== null && captured.generation === generation) {
      // Chrome-style order: `beforeinput`/`input` fired BEFORE
      // `compositionend`, so the commit is already captured.
      this.finalizeCompositionCommit(captured.text, target);
      return;
    }
    // Empty data and no captured commit yet. Could be either:
    //   (a) a real cancel (Esc out of the candidate list), or
    //   (b) Firefox-style order where the commit-signaling `input`
    //       event hasn't fired yet — it'll arrive on a later task.
    // Open the late-commit window and defer one macrotask to give
    // (b) a chance. The generation counter guards against a new
    // composition starting in the interim (in which case this
    // deferred work belongs to a stale session and must abandon).
    // Store the snapshotted target on the pending record so a
    // synchronous flush triggered by an interleaved keystroke can
    // route to the same pane the macrotask would have used.
    this.pendingLateCommit = { generation, target };
    const win = this.doc.defaultView;
    if (win === null) {
      // No window in the test environment — finalize synchronously
      // as cancel; tests can opt into deferred behavior by using
      // their own fake timers.
      this.pendingLateCommit = null;
      this.compositionCommitData = null;
      this.compositionSinkEl.value = "";
      return;
    }
    win.setTimeout(() => {
      if (this.destroyed) return;
      if (generation !== this.compositionGeneration) return;
      this.flushPendingLateCommit();
    }, 0);
  }

  /// Finalize a pending late commit synchronously. Used both by the
  /// macrotask deferred path and by `onKeyDown` to ensure that any
  /// IME commit bytes hit the wire BEFORE the keystroke that
  /// arrived next — preserves the user-perceived input order.
  /// Idempotent: a no-op when no late commit is pending. Round-8
  /// codex P1 fix.
  private flushPendingLateCommit(): void {
    const pending = this.pendingLateCommit;
    if (pending === null) return;
    this.pendingLateCommit = null;
    const late = this.compositionCommitData;
    this.compositionCommitData = null;
    this.compositionSinkEl.value = "";
    if (late === null || late.generation !== pending.generation) return;
    this.finalizeCompositionCommit(late.text, pending.target);
  }

  /// Send a composition commit through `sendInput`. The target is
  /// snapshotted at `compositionend` (or commit-data capture) time
  /// rather than re-resolved here: the optimistic
  /// `pendingFocusedPaneId` would otherwise steer commits to a pane
  /// the user clicked into mid-composition rather than the one they
  /// were actually composing in. Server-driven active-pane changes
  /// during composition still propagate through `applyLayout`, which
  /// keeps `composingPaneId` (the snapshot source) in sync.
  /// Mirrors the spirit of the native Rust client's `Ime::Commit`
  /// path in `app/ime.rs:36-41` — Rust has no equivalent of the
  /// "pending click" optimistic state, so its `active_pane_id()`
  /// already reflects server truth.
  private finalizeCompositionCommit(
    data: string,
    target: bigint | null,
  ): void {
    this.compositionCommitData = null;
    this.compositionSinkEl.value = "";
    if (data.length === 0) return;
    if (target === null) return;
    // Drop bytes for unknown panes — same guard as the regular
    // keystroke path (a pane may have closed mid-composition).
    if (this.grids.get(target.toString()) === undefined) return;
    this.client.sendInput(target, new TextEncoder().encode(data));
  }

  /// Send a direct (non-composition) IME insert — see the call site in
  /// `onSinkBeforeInput`. `beforeinput` AND `input` both fire for one
  /// insert; we must send exactly once. Prefer the cancelable
  /// `beforeinput`: `preventDefault` stops the DOM mutation so the
  /// paired `input` never fires and the sink stays clean. The
  /// `directInsertGuard` defends against engines that deliver both
  /// anyway (or a non-cancelable `beforeinput`), and the `input`-only
  /// branch covers Safari/mobile builds that skip `beforeinput` for IME.
  private handleDirectInsert(e: Event, data: string): void {
    if (e.type === "beforeinput") {
      if (e.cancelable) e.preventDefault();
      this.directInsertGuard = true;
      queueMicrotask(() => {
        this.directInsertGuard = false;
      });
      this.sendDirectText(data);
      return;
    }
    // `input` path: the textarea has already mutated — clear it. Only
    // send if the cancelable `beforeinput` didn't already handle this
    // same insert.
    this.compositionSinkEl.value = "";
    if (!this.directInsertGuard) this.sendDirectText(data);
  }

  /// Route text straight to the active pane (no composition target to
  /// snapshot). Unlike `finalizeCompositionCommit`, this honors
  /// `activePaneId()` (incl. the optimistic pending-click target) since
  /// a direct insert isn't anchored to an in-flight composition.
  private sendDirectText(data: string): void {
    if (data.length === 0) return;
    const target = this.activePaneId();
    if (target === null) return;
    if (this.grids.get(target.toString()) === undefined) return;
    this.client.sendInput(target, new TextEncoder().encode(data));
  }

  /// Best-effort active-pane resolver: prefer the pending click
  /// target (so a click-then-IME-compose sequence routes to the
  /// just-clicked pane even before the server has acked the focus
  /// change), fall back to the server-reported active pane.
  private activePaneId(): bigint | null {
    if (this.pendingFocusedPaneId !== null) return this.pendingFocusedPaneId;
    if (this.currentLayout === null) return null;
    return LayoutManager.activePaneId(this.currentLayout);
  }

  /// Root received keyboard focus directly (Tab into the terminal,
  /// or a host caller invoking `root.focus()`). Redirect to the
  /// composition sink so subsequent keystrokes route through the
  /// editable target that engages browser IME. No-op if root focus
  /// arrives via a child (focus on textarea / a tile would not
  /// reach this handler because `focus` doesn't bubble — we only
  /// listen on root itself).
  private onRootFocus(e: FocusEvent): void {
    if (this.destroyed) return;
    if (e.target !== this.root) return;
    // Defer to a microtask so the browser settles the focus state
    // before we move it again; some browsers (Firefox) get angry
    // about synchronous focus-from-focus re-entrancy.
    queueMicrotask(() => {
      if (this.destroyed) return;
      this.focusInputSink();
    });
  }

  /// Move keyboard focus to the composition sink so the browser
  /// engages its IME engine. jsdom occasionally throws from `focus()`
  /// if the element isn't fully attached yet; swallow because a
  /// missed focus only matters for the first keystroke. Also
  /// repositions the sink to anchor any subsequent IME candidate
  /// window at the active cursor.
  private focusInputSink(): void {
    try {
      this.compositionSinkEl.focus({ preventScroll: true });
    } catch {
      // Swallow — see method doc.
    }
    this.repositionCompositionSink();
  }

  /// Clear any preedit overlay currently shown on the given pane,
  /// triggering a render so the change is visible. Used when active
  /// focus moves mid-composition and the overlay needs to transfer
  /// to a different pane.
  private clearPreeditOn(paneId: bigint): void {
    const key = paneId.toString();
    const renderer = this.renderers.get(key);
    const grid = this.grids.get(key);
    if (renderer === undefined) return;
    renderer.setPreedit(null);
    if (grid !== undefined) renderer.render(grid);
  }

  /// Anchor the hidden composition sink at the active pane's cursor
  /// screen position so the OS IME candidate window (Pinyin lookup
  /// list, Kana suggestions, …) appears near the actual caret
  /// instead of the top-left of the page. Mirrors what the native
  /// Rust client does via `window.set_ime_cursor_area(...)` (see
  /// `crates/ciri/src/app/render.rs` — search for `ime_input_anchor`).
  ///
  /// Uses `position: fixed` so the math is straight viewport
  /// coordinates and doesn't depend on the root having a particular
  /// CSS containing block. When there's no active pane (initial
  /// state, no layout yet) we leave the sink at its initial corner
  /// — composition can't actually start in that state anyway.
  private repositionCompositionSink(): void {
    if (this.currentLayout === null) return;
    // While composing, anchor at the locked composing pane — NOT
    // `activePaneId()` (which honors the optimistic
    // `pendingFocusedPaneId`). Otherwise the OS IME candidate
    // popup would shift to a just-clicked pane while the actual
    // commit (and preedit overlay) stays on the original target,
    // leaving the user with a misaligned UI and possible
    // wrong-terminal mental model. Round-8 codex P1 fix.
    const activeId = this.composing
      ? this.composingPaneId
      : this.activePaneId();
    if (activeId === null) return;
    const key = activeId.toString();
    const renderer = this.renderers.get(key);
    const grid = this.grids.get(key);
    if (renderer === undefined || grid === undefined) return;
    const rect = renderer.container.getBoundingClientRect();
    if (rect.width === 0 && rect.height === 0) {
      // jsdom or off-screen renderer: skip positioning rather than
      // anchor at (0, 0), which would defeat the purpose.
      return;
    }
    const { cursorLine, cursorCol } = grid.meta;
    // Display row mirrors the renderer's cursor + preedit math
    // (renderer.ts updateCursor / updatePreedit): when the user has
    // scrolled back into history the live cursor shifts DOWN in
    // display coordinates by `scrollOffset`. If the live cursor
    // sits past the viewport bottom — i.e. the user scrolled so
    // far that the live row is no longer visible — we can't paint
    // the IME anchor anywhere sensible. Pin the sink at the
    // viewport's bottom-left so OS candidate windows stay within
    // the terminal rather than anchoring over unrelated scrollback.
    // Round-6 codex P3 fix.
    const displayRow = cursorLine + renderer.scrollOffsetRows;
    let left: number;
    let top: number;
    if (displayRow < 0 || displayRow >= grid.rows) {
      left = rect.left;
      top = rect.bottom - this.cellSize.cellHeight;
    } else {
      left = rect.left + cursorCol * this.cellSize.cellWidth;
      top = rect.top + displayRow * this.cellSize.cellHeight;
    }
    this.compositionSinkEl.style.position = "fixed";
    this.compositionSinkEl.style.left = `${left}px`;
    this.compositionSinkEl.style.top = `${top}px`;
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
    const paneId = BigInt(paneIdStr);
    e.preventDefault();
    const linesUp = wheelDeltaToLines(
      e,
      this.cellSize.cellHeight,
      this.scrollLinesPerWheelTick,
    );
    if (linesUp === 0) return;
    // Mouse-aware TUI claims the wheel; shift-bypass returns it to
    // the renderer's scrollback. Mirrors the Rust client's
    // `handle_discrete_scroll` branch on `has_mouse` (mouse.rs:1108).
    if (this.paneReportsMouse(paneId) && !e.shiftKey) {
      const cell = this.hitTestViewportCell(e, renderer, grid);
      if (cell === null) return;
      // Cap the per-tick burst at 10 to match the Rust client — a
      // trackpad pixel-mode delta can otherwise spew dozens of
      // SGR frames the TUI then chases for tens of ms.
      const ticks = Math.min(10, Math.abs(linesUp));
      const button = linesUp > 0 ? 64 : 65; // 64=up, 65=down
      // Shift is consumed as the local-scrollback bypass above, and
      // Ctrl is reserved for browser zoom at the top of this
      // handler, so the modifier mask here only carries Alt/Meta.
      // Compute once outside the loop — the burst can be 10 frames.
      const mods = mouseModifierMask(e);
      for (let i = 0; i < ticks; i += 1) {
        this.client.send({
          tag: "MouseInput",
          paneId,
          button,
          col: cell.col,
          row: cell.row,
          // SGR wheel frames are encoded as press events (`M`
          // suffix); the TUI sees button-64/65 with no matching
          // release. Standard xterm convention.
          pressed: true,
          modifiers: mods,
        });
      }
      return;
    }
    const next = Math.max(0, renderer.scrollOffsetRows + linesUp);
    renderer.setScrollOffset(next);
    renderer.render(grid);
    // Sink anchor depends on the cursor's display row, which shifts
    // when scrollback moves. Refresh while composing so the OS IME
    // candidate popup tracks the visible cursor. Round-8 codex P2.
    if (this.composing) this.repositionCompositionSink();
  }

  /// Intercept the copy / paste chords before the regular keystroke
  /// encoder runs. Returns `true` when the chord owns the event and the
  /// caller should stop. Most branches also call `preventDefault`, but
  /// the plain Ctrl+V / Cmd+V paste branch deliberately does NOT — it
  /// lets the browser's native paste proceed (see the inline note).
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
    // Plain Ctrl+V (and Cmd+V) are the browser's *native* paste triggers.
    // Return "handled" WITHOUT preventDefault so the keydown's default
    // proceeds: the browser then fires a real `paste` event on the
    // focused sink, which `onPaste` turns into (bracketed) PTY input —
    // the permission-free path that needs no Clipboard-API grant.
    //
    // The key bit is what we DON'T do. If this fell through to the key
    // encoder, Ctrl+V would map to the SYN control byte (0x16) and the
    // encoder's `preventDefault` would cancel the keydown — so the
    // browser would never fire `paste` and Ctrl+V "wouldn't paste".
    // That was the bug. The cost of fixing it: Ctrl+V no longer sends a
    // literal SYN (readline "quoted-insert"), which is the right call
    // for a browser terminal where Ctrl+V = paste is what users expect.
    // Cmd+V already fell through (the encoder returns null on metaKey);
    // handling it here too keeps the paste affordance explicit.
    const isPlainCtrlV = e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey;
    if (key === "v" && (isPlainCtrlV || isCmdChord)) {
      // No preventDefault — let the native paste event fire.
      return true;
    }
    // Ctrl+Shift+V can't trigger a native `paste` event (it isn't a
    // browser paste shortcut), so it goes through the async Clipboard
    // API as a secondary path for users who've granted clipboard-read.
    if (key === "v" && isCtrlShift) {
      e.preventDefault();
      void this.pasteFromClipboard();
      return true;
    }
    // Ctrl+Shift+F (or Cmd+F) opens the scrollback find bar. Cmd+F is
    // the browser's own find on macOS, but over a terminal the buffer
    // search is what the user means; preventDefault claims it.
    if (key === "f" && (isCtrlShift || isCmdChord)) {
      e.preventDefault();
      this.openSearch();
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
      if (this.writeSystemClipboard(text)) return true;
      return false;
    }
    return false;
  }

  /// Write `text` to the system clipboard. Shared by the local-selection
  /// copy path and the OSC 52 `ClipboardStore` server message. Returns
  /// whether the clipboard API was available (the write itself is
  /// async; failures surface through `onError`).
  private writeSystemClipboard(text: string): boolean {
    const clipboard = this.doc.defaultView?.navigator?.clipboard;
    if (clipboard === undefined) return false;
    clipboard.writeText(text).catch((err: unknown) => {
      const e = err instanceof Error ? err : new Error(String(err));
      this.handlerOnError?.(e);
    });
    return true;
  }

  /// Queue an OSC 52 clipboard write and try it immediately. The
  /// immediate attempt only succeeds if the page still has transient
  /// user activation; otherwise the payload stays pending and
  /// `flushPendingClipboard` retries inside the next user gesture.
  /// Latest write wins (a newer yank overwrites an unflushed one).
  private queueClipboardWrite(text: string): void {
    this.pendingClipboardWrite = text;
    this.flushPendingClipboard();
  }

  /// Attempt the pending OSC 52 write. Call from within a user-gesture
  /// handler (keydown / tap) so the browser's activation gate is
  /// satisfied. Clears the pending payload on success; on failure
  /// (still no activation) leaves it for the next gesture. Does NOT
  /// surface failures through `onError` — an unflushed OSC 52 write is
  /// expected, not an error.
  private flushPendingClipboard(): void {
    const text = this.pendingClipboardWrite;
    if (text === null) return;
    const clipboard = this.doc.defaultView?.navigator?.clipboard;
    if (clipboard === undefined) {
      this.pendingClipboardWrite = null; // no clipboard API — drop it
      return;
    }
    clipboard.writeText(text).then(
      () => {
        // Only clear if a newer yank didn't replace it in the meantime.
        if (this.pendingClipboardWrite === text) this.pendingClipboardWrite = null;
      },
      () => {
        // No activation yet — keep pending; the next gesture retries.
      },
    );
  }

  /// Read the system clipboard and send its text into the active
  /// pane's PTY, wrapping in `ESC[200~ ... ESC[201~` when the pane's
  /// terminal app has set `MODE_BRACKETED_PASTE` (mirror of the Rust
  /// client's paste path in `context_menu.rs`).
  /// Native `paste` event on the focused sink — the robust, permission-
  /// free paste path (web.dev async-clipboard guidance; the fallback
  /// xterm.js/VSCode rely on). `clipboardData` grants temporary read
  /// access with no permission prompt, so this works where
  /// `navigator.clipboard.readText()` is blocked (Firefox pages, no
  /// activation, unfocused). Fires for Cmd/Ctrl+V, the browser's
  /// right-click "Paste", and mobile paste. preventDefault keeps the
  /// text out of the hidden textarea.
  private onPaste(e: ClipboardEvent): void {
    const text = e.clipboardData?.getData("text/plain") ?? "";
    if (text.length === 0) return;
    e.preventDefault();
    this.sendPasteText(text);
  }

  /// Send pasted `text` to the active pane, wrapping in
  /// `ESC[200~ … ESC[201~` when the pane has `MODE_BRACKETED_PASTE`.
  /// Shared by the native `paste` event and the readText fallback.
  private sendPasteText(text: string): void {
    if (text.length === 0 || this.destroyed) return;
    if (this.currentLayout === null) return;
    const activePaneId =
      this.pendingFocusedPaneId ?? LayoutManager.activePaneId(this.currentLayout);
    if (activePaneId === null) return;
    const grid = this.grids.get(activePaneId.toString());
    if (grid === undefined) return;
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

  /// Explicit-shortcut (Ctrl+Shift+V) and context-menu "Paste". These
  /// can't ride a native `paste` event, so they use the async Clipboard
  /// API — which the browser may block (Firefox web pages don't
  /// implement readText; Chrome needs focus + activation + permission).
  /// On failure we can't read here, so point the user at a native paste
  /// (Ctrl/Cmd+V or the right-click menu), which `onPaste` handles.
  private async pasteFromClipboard(): Promise<void> {
    const view = this.doc.defaultView;
    const clipboard = view?.navigator?.clipboard;
    if (clipboard === undefined || typeof clipboard.readText !== "function") {
      this.handlerOnError?.(
        new Error(
          "This browser won't let the page read the clipboard — paste with Ctrl+V / Cmd+V or the browser's own right-click menu.",
        ),
      );
      return;
    }
    let text: string;
    try {
      // Call `readText()` first — before any other `await`. The async
      // Clipboard API only grants a read while the triggering gesture's
      // transient user activation is still live, and awaiting anything
      // (e.g. a permission probe) beforehand would consume it and make
      // the read fail spuriously even where it's allowed.
      text = await clipboard.readText();
    } catch {
      // The read was rejected. Probe the permission *now* (activation no
      // longer matters) so a hard site-level block gets a precise,
      // actionable hint instead of the generic "blocked" line. The
      // descriptor is non-standard — Firefox throws on the query — so a
      // failure here just falls back to the generic message.
      let denied = false;
      try {
        const status = await view?.navigator?.permissions?.query?.({
          name: "clipboard-read" as PermissionName,
        });
        denied = status?.state === "denied";
      } catch {
        /* permission descriptor unsupported — keep the generic hint */
      }
      this.handlerOnError?.(
        new Error(
          denied
            ? "Clipboard access is blocked for this site — allow it via the address-bar site settings, or paste with Ctrl+V / Cmd+V (the right-click \"Paste\" in the browser's own menu also works)."
            : "Clipboard read was blocked — paste with Ctrl+V / Cmd+V or the browser's right-click menu.",
        ),
      );
      return;
    }
    this.sendPasteText(text);
  }

  private onPaneClicked(paneId: bigint): void {
    // Record the local intent so the *next* keystroke routes to the
    // just-clicked pane, not the layout-derived active one. The
    // server is asked to confirm via FocusPane; the next LayoutUpdate
    // clears the pending state.
    this.pendingFocusedPaneId = paneId;
    // Eager Resize: re-aim the viewport lie at the clicked pane
    // *before* FocusPane lands so the server's resize_all_panes
    // runs against the new dims in a single batch. Without this,
    // we'd render the OLD FullPaneSync (server-allocated at the
    // previous active pane's proportions) for a frame before the
    // LayoutUpdate-triggered Resize round-trip settles. ClientMessages
    // are processed serially server-side, so the Resize-then-Focus
    // pair lands as one cohesive update.
    this.sendResizeForPane(paneId);
    this.client.send({ tag: "FocusPane", paneId });
  }

  private onWorkspaceClicked(idx: bigint): void {
    // Workspaces can carry completely different column/tile shapes,
    // so the lie computed for the outgoing workspace's active pane
    // is meaningless for the incoming one. Flag the upcoming
    // LayoutUpdate to re-aim the viewport lie, same shape as
    // CreatePane / ClosePane.
    this.expectStructuralChange = true;
    this.client.send({ tag: "SwitchWorkspace", workspaceIdx: idx });
  }

  /// User picked a session from the dropdown. SwitchSession swaps the
  /// whole attached session — the server replies with `SessionSwitched`
  /// (which updates `currentSessionName` + the URL) followed by a full
  /// sync for the new session's panes, so the next LayoutUpdate has a
  /// completely different shape: re-aim the viewport lie like the other
  /// structural changes. No-op if the user re-picked the current
  /// session (the `<select>` can fire `change` on programmatic value
  /// sets in some engines).
  private onSessionSelected(sessionName: string): void {
    if (sessionName === this.currentSessionName) return;
    this.expectStructuralChange = true;
    this.client.send({ tag: "SwitchSession", sessionName });
    // Keep typing flowing into the terminal after the dropdown closes.
    queueMicrotask(() => {
      if (!this.destroyed) this.focusInputSink();
    });
  }

  /// Ask the server for the session list to populate the dropdown.
  /// `all: false` lists only *running* sessions — matching the desktop
  /// client's in-app switcher (`ciri-app` palette, `app/mod.rs`,
  /// `sync.rs` all send `all: false`); `all: true` (saved-but-detached
  /// sessions) is reserved for the CLI `ls --all` path. The server also
  /// filters out `__`-prefixed internal sessions. Sent on connect and
  /// refreshed after each `SessionSwitched` (the active-first ordering
  /// changes when the attached session changes).
  private requestSessionList(): void {
    this.client.send({ tag: "ListSessions", all: false });
  }

  /// Create a new workspace (the "+" on the workspace tab strip).
  /// `SplitDown` opens a pane in a brand-new workspace below and the
  /// server promotes it to active, so the next LayoutUpdate has a
  /// different active-workspace shape — re-aim the viewport lie just
  /// like CreatePane / SwitchWorkspace.
  private createWorkspace(): void {
    this.expectStructuralChange = true;
    this.client.send({ tag: "SplitDown" });
  }

  /// Create and switch to a new session (the "+" next to the session
  /// dropdown). The protocol has no dedicated "new session" message —
  /// `SwitchSession` to a name the server doesn't know creates it
  /// (server `get_or_create_session`). We pick the lowest unused
  /// `session-N` against the known list (the desktop uses a random
  /// name; a predictable one is friendlier here and the server's
  /// `validate_name` accepts lowercase + digits + hyphens). A collision
  /// with a saved-but-not-listed session just attaches to it instead,
  /// which is harmless.
  private createSession(): void {
    const taken = new Set(this.knownSessions);
    let n = 1;
    while (taken.has(`session-${n}`)) n += 1;
    this.onSessionSelected(`session-${n}`);
  }

  /// Open the scrollback find bar against the active pane. No-op when
  /// there's no active pane/grid. Re-opening while already open just
  /// re-focuses the input (and re-targets if the active pane changed).
  private openSearch(): void {
    const target = this.activePaneId();
    if (target === null) return;
    const renderer = this.renderers.get(target.toString());
    if (renderer === undefined) return;
    this.searchPaneId = target;
    this.searchPrevScrollOffset = renderer.scrollOffsetRows;
    this.searchBarEl.hidden = false;
    this.searchInputEl.focus();
    this.searchInputEl.select();
    // Re-run against whatever is already typed (e.g. re-open after close).
    this.runSearch(this.searchInputEl.value);
  }

  /// Close the find bar, drop the highlight, and restore the pane's
  /// pre-search scroll position. Returns keyboard focus to the terminal.
  private closeSearch(): void {
    this.searchBarEl.hidden = true;
    const key = this.searchPaneId?.toString();
    this.searchMatches = [];
    this.searchMatchIdx = 0;
    if (key !== undefined) {
      const grid = this.grids.get(key);
      const renderer = this.renderers.get(key);
      if (grid !== undefined && renderer !== undefined) {
        renderer.setSelection(null);
        renderer.setScrollOffset(this.searchPrevScrollOffset);
        renderer.render(grid);
      }
    }
    this.searchPaneId = null;
    this.focusInputSink();
  }

  /// Run a query against the search pane's combined buffer, pick the
  /// first match at or below the current viewport top (so the nearest
  /// hit is selected first, mirroring the desktop client), and reveal
  /// it.
  private runSearch(query: string): void {
    const key = this.searchPaneId?.toString();
    if (key === undefined) return;
    const grid = this.grids.get(key);
    if (grid === undefined) return;
    this.searchMatches = grid.search(query);
    this.searchMatchIdx = 0;
    if (this.searchMatches.length > 0) {
      const renderer = this.renderers.get(key);
      const viewportTop =
        grid.scrollbackRows - (renderer?.scrollOffsetRows ?? 0);
      const near = this.searchMatches.findIndex((m) => m.srcRow >= viewportTop);
      this.searchMatchIdx = near < 0 ? 0 : near;
    }
    this.revealCurrentMatch();
  }

  /// Move to the next (`+1`) / previous (`-1`) match, wrapping around.
  private stepSearch(delta: number): void {
    const n = this.searchMatches.length;
    if (n === 0) return;
    this.searchMatchIdx = (this.searchMatchIdx + delta + n) % n;
    this.revealCurrentMatch();
  }

  /// Update the "n/N" status, highlight the current match (reusing the
  /// selection overlay), and scroll it into view if it isn't already.
  private revealCurrentMatch(): void {
    const key = this.searchPaneId?.toString();
    if (key === undefined) return;
    const grid = this.grids.get(key);
    const renderer = this.renderers.get(key);
    if (grid === undefined || renderer === undefined) return;
    const n = this.searchMatches.length;
    if (n === 0) {
      this.searchStatusEl.textContent =
        this.searchInputEl.value.length === 0 ? "" : "0/0";
      renderer.setSelection(null);
      renderer.render(grid);
      return;
    }
    this.searchStatusEl.textContent = `${this.searchMatchIdx + 1}/${n}`;
    const m = this.searchMatches[this.searchMatchIdx]!;
    renderer.setSelection({
      start: { col: m.startCol, srcRow: m.srcRow },
      end: { col: m.endCol, srcRow: m.srcRow },
      active: false,
    });
    // Scroll into view only when the match row isn't already visible,
    // so cycling between on-screen matches doesn't jolt the viewport.
    const displayRow = m.srcRow - grid.scrollbackRows + renderer.scrollOffsetRows;
    if (displayRow < 0 || displayRow >= grid.rows) {
      renderer.setScrollOffset(grid.scrollbackRows - m.srcRow);
    }
    renderer.render(grid);
  }

  /// Pane-bar quick-action button clicked. Action ids come from the
  /// `actions` list passed to LayoutManager — keep this switch in
  /// sync with `DEFAULT_PANE_ACTIONS`.
  private onAction(id: string): void {
    switch (id) {
      case "new-pane":
        // CreatePane shifts every column's `widthProportion`, so the
        // viewport lie we sent for the old shape is stale by the time
        // the next `LayoutUpdate` arrives. Flag the upcoming
        // applyLayout to recompute and re-send.
        this.expectStructuralChange = true;
        this.client.send({ tag: "CreatePane" });
        return;
      default:
        // Unknown id — surfaced rather than silently swallowed so a
        // typo in a future action entry doesn't ghost a button.
        this.handlerOnError?.(
          new Error(`unknown pane-bar action id: ${id}`),
        );
    }
  }

  /// Per-chip close (×) button clicked. Sends `ClosePane` for the
  /// specific pane the user tapped — distinct from a global "close
  /// active" action because each chip carries its own close target.
  private onPaneClose(paneId: bigint): void {
    // Mirror `onAction("new-pane")`: ClosePane changes the set of
    // columns and the proportion of whichever pane the server
    // promotes to active, so the next LayoutUpdate must re-aim the
    // viewport lie. Without this, closing a 2nd pane back to a
    // 1-pane session leaves the survivor rendered at 2× width (the
    // lied viewport still targets a 2-column shape).
    this.expectStructuralChange = true;
    this.client.send({ tag: "ClosePane", paneId });
  }

  // ─── Inbound client events ───────────────────────────────────────

  private onClientEvent(e: CiriEvent): void {
    switch (e.kind) {
      case "open":
        this.handlerOnOpen?.();
        // Populate the session dropdown once the transport is up.
        this.requestSessionList();
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
      case "SessionSwitched":
        // The transport has already pinned its replay ClientHello to
        // this name; surface it so the page can update its URL (the
        // resolved name after an `__auto__` auto-attach arrives here).
        this.currentSessionName = msg.sessionName;
        this.handlerOnSessionChange?.(msg.sessionName);
        // Reflect the new active session in the dropdown immediately
        // from the cached list, then re-query so the active-first
        // ordering (and any newly created session) is up to date.
        this.layout.setSessions(this.knownSessions, this.currentSessionName);
        this.requestSessionList();
        return;
      case "SessionList":
        this.knownSessions = msg.sessions.map((s) => s.name);
        this.layout.setSessions(this.knownSessions, this.currentSessionName);
        return;
      case "ClipboardStore":
        // OSC 52: a TUI (vim/tmux yank) asked to set the system
        // clipboard. `data` is already-decoded plaintext — the
        // server's ciri-term layer base64-decodes the OSC sequence.
        // Unlike the desktop client's `cb.set_text(&data)`, browsers
        // reject `clipboard.writeText()` outside a user gesture, and
        // this arrives on a server message (no activation). So queue it
        // and flush on the next keystroke / tap (almost always imminent
        // in a terminal) — see `flushPendingClipboard`.
        this.queueClipboardWrite(msg.data);
        return;
      case "Error":
        // Surface server-side protocol/runtime errors that would
        // otherwise vanish into the default-ignore arm.
        this.handlerOnError?.(new Error(`server: ${msg.message}`));
        return;
      case "ServerShutdown":
        this.handlerOnServerShutdown?.();
        return;
      case "ImagePlacement":
        this.handleImagePlacement(msg);
        return;
      case "ImageDeleted":
        this.renderers.get(msg.paneId.toString())?.clearImages();
        return;
      case "Notification":
        // A TUI explicitly asked to notify (OSC 9 / OSC 777). Always
        // surface it — the app requested it on purpose.
        this.notify(msg.title, msg.body);
        return;
      case "SessionKilled":
        // The session this client is (or was) attached to went away.
        // Warn prominently — the panes may be about to vanish.
        this.notify("Session killed", msg.sessionName, "warn");
        return;
      case "CommandCompleted":
        this.handleCommandCompleted(msg);
        return;
      default:
        // Reply-only messages the web doesn't request yet (template /
        // session-info / pane-list / command-result, …) fall through
        // here harmlessly.
        return;
    }
  }

  /// Server Bell event. Add a transient CSS class to the pane's tile
  /// so a theme can flash a visual indicator; clears automatically
  /// after `BELL_FLASH_MS`. Panes outside the active workspace have
  /// no slot to flash — for now we drop the visual cue on them; a
  /// later phase can surface a badge on the workspace tab.
  /// Place an inline image (sixel/kitty/iTerm). The server has already
  /// decoded it to RGBA; we hand the placement + the pane's grid (for
  /// the scrollback-row anchor) to the renderer. Dropped for unknown
  /// panes — same guard as the keystroke / commit paths.
  private handleImagePlacement(msg: {
    paneId: bigint;
    imageId: bigint;
    col: number;
    row: number;
    widthCells: number;
    heightCells: number;
    pixelWidth: number;
    pixelHeight: number;
    format: string;
    data: Uint8Array;
  }): void {
    const key = msg.paneId.toString();
    const grid = this.grids.get(key);
    const renderer = this.renderers.get(key);
    if (grid === undefined || renderer === undefined) return;
    renderer.setImage(grid, msg);
  }

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

  /// Push a transient line into the toast stack. Always available
  /// (works on iOS pages and with notifications denied) and tap-to-
  /// dismiss so it never sits on top of content the user wants to reach.
  private showToast(text: string, kind: "info" | "warn" = "info"): void {
    if (this.destroyed) return;
    const win = this.doc.defaultView;
    const el = this.doc.createElement("div");
    el.className = `ciri-toast ciri-toast-${kind}`;
    el.setAttribute("role", "status");
    el.textContent = text;
    // One dismissal path for both the tap and the auto-timeout — it
    // detaches the element AND clears/forgets the timer, so a burst of
    // tapped-away toasts doesn't leave dead timers ticking until
    // `TOAST_MS`.
    let timer: ReturnType<Window["setTimeout"]> | null = null;
    const dismiss = (): void => {
      el.remove();
      if (timer !== null) {
        win?.clearTimeout(timer);
        this.toastTimers.delete(timer);
        timer = null;
      }
    };
    el.addEventListener("pointerdown", dismiss);
    this.toastEl.appendChild(el);
    if (win === null) return;
    timer = win.setTimeout(dismiss, TOAST_MS);
    this.toastTimers.add(timer);
  }

  /// Surface an event as an in-app toast (always) and, when the user has
  /// granted permission, an OS notification too — the only thing visible
  /// when the tab is backgrounded, which is the whole point on mobile.
  /// When permission is still "default", remember the intent so the next
  /// gesture can ask (see `maybeAskNotifyPermission`).
  private notify(title: string, body: string, kind: "info" | "warn" = "info"): void {
    const text = body.length > 0 ? `${title} — ${body}` : title;
    this.showToast(text, kind);
    const win = this.doc.defaultView;
    const Notif = win?.Notification;
    if (Notif === undefined) return;
    if (Notif.permission !== "granted") {
      if (Notif.permission === "default") this.notifyWanted = true;
      return;
    }
    // Prefer a service-worker notification when one is controlling the
    // page — Android Chrome throws on `new Notification()` and only
    // delivers via `registration.showNotification`. Gate on
    // `controller` (not a bare `serviceWorker.ready`, which never
    // resolves when no SW is registered, e.g. in dev) and fall back to
    // the page constructor on browsers that allow it.
    const sw = win?.navigator?.serviceWorker;
    if (sw?.controller != null && typeof sw.ready?.then === "function") {
      sw.ready
        .then((reg) => reg.showNotification(title, { body }))
        .catch(() => this.firePageNotification(Notif, title, body));
      return;
    }
    this.firePageNotification(Notif, title, body);
  }

  /// Page-context OS notification (`new Notification`). Throws on
  /// browsers that only support service-worker notifications (Android
  /// Chrome) — the in-app toast already covered the event, so swallow.
  private firePageNotification(
    Notif: typeof Notification,
    title: string,
    body: string,
  ): void {
    try {
      void new Notif(title, { body });
    } catch {
      /* SW-only browser; toast is the fallback. */
    }
  }

  /// Ask for OS-notification permission lazily, once, on a real user
  /// gesture — but only after something actually wanted to notify
  /// (`notifyWanted`). Prompting out of context gets reflexively blocked,
  /// and several browsers now require a gesture for `requestPermission`.
  /// Called from the keydown / touch entry points.
  private maybeAskNotifyPermission(): void {
    if (this.notifyPermissionAsked || !this.notifyWanted) return;
    const Notif = this.doc.defaultView?.Notification;
    if (Notif === undefined || Notif.permission !== "default") return;
    this.notifyPermissionAsked = true;
    try {
      void Notif.requestPermission();
    } catch {
      // Safari historically only accepted the callback form and throws
      // on the promise call; ignore — toasts remain the fallback.
    }
  }

  /// Shell-integration cue (OSC 133): a foreground command in `paneId`
  /// finished. Only notify when the tab is hidden — when it's visible
  /// the user is watching the output already and a toast per command
  /// would be noise. This is the "kick off a build, switch away, get
  /// pinged when it's done" path that matters on mobile.
  private handleCommandCompleted(msg: {
    paneId: bigint;
    durationSecs: bigint;
    exitCode: number | null;
  }): void {
    if (!this.doc.hidden) return;
    const title = this.grids.get(msg.paneId.toString())?.title ?? "";
    const head = title.length > 0 ? `Done: ${title}` : "Command finished";
    const dur = `${msg.durationSecs.toString()}s`;
    const failed = msg.exitCode !== null && msg.exitCode !== 0;
    const status =
      msg.exitCode === null
        ? dur
        : failed
          ? `exit ${msg.exitCode} · ${dur}`
          : `ok · ${dur}`;
    this.notify(head, status, failed ? "warn" : "info");
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
    // `setLayout` rebuilds every `.ciri-column` / `.ciri-tile` node —
    // any in-flight resize-drag's stored element refs are about to
    // become detached. Cancel the drag cleanly: drop the window
    // listeners, reset the cursor, null the state. The user has to
    // re-grab if they want to continue — which is the right UX
    // anyway because the layout the user was resizing is no longer
    // the layout on screen.
    if (this.resizeDrag !== null) {
      this.resizeDrag = null;
      this.setResizeCursor(null);
      const win = this.doc.defaultView;
      if (win !== null) {
        win.removeEventListener("mousemove", this.onWindowMouseMove);
        win.removeEventListener("mouseup", this.onWindowMouseUp);
      }
    }
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
    // Re-aim the viewport lie at the active pane in three cases:
    //   1. First LayoutUpdate after attach — initial lie setup.
    //   2. Web just queued a structural change (`CreatePane` /
    //      `ClosePane`) — proportions definitely shifted, the lie
    //      MUST refresh or the new active pane renders at the
    //      old shape's dimensions.
    //   3. (Implicit) Chip click — handled separately by the eager
    //      Resize in `onPaneClicked`, never reaches here.
    //
    // External LayoutUpdates (another client's CreatePane, server
    // restore, …) are NOT included. They DO leave the lie stale,
    // but resending here would create a feedback loop with
    // `resize_all_panes`: every LayoutUpdate → web Resize → server
    // reflow → broadcast LayoutUpdate + FullPaneSync to every
    // client → CPU burst on any co-attached desktop. The user's
    // next interaction (chip switch, window resize) corrects the
    // lie on demand.
    const shouldResize =
      (!this.initialResizeSentAfterAttach || this.expectStructuralChange) &&
      activeId !== null;
    if (shouldResize) {
      this.sendResizeForPane(activeId);
      this.initialResizeSentAfterAttach = true;
      this.expectStructuralChange = false;
    }
    // If a composition is in flight and the LayoutUpdate moved the
    // active pane, retarget. Two sub-cases:
    //   (1) activeId !== null AND differs from composingPaneId:
    //       transfer the preedit overlay to the new active pane,
    //       and commit will follow at compositionend.
    //   (2) activeId === null (no active pane in the new layout —
    //       e.g. workspace emptied, or all panes closed mid-
    //       composition): clear the composition entirely so a later
    //       compositionend doesn't commit into the previously
    //       focused (and now logically detached) pane. Round-9
    //       codex P2 fix; matches the spirit of Rust's
    //       `active_pane_id()?` short-circuit in
    //       `crates/ciri/src/app/ime.rs:36`.
    if (this.composing && this.composingPaneId !== null) {
      if (activeId === null) {
        this.clearPreeditOn(this.composingPaneId);
        this.composingPaneId = null;
        // Also drop the in-app `composing` flag so subsequent
        // keystrokes aren't suppressed by `onKeyDown`'s `composing`
        // guard. The browser's IME session may still be active
        // (e.isComposing would still be true on a keystroke from
        // that session — the encoder's own `isComposing` short-
        // circuit covers that), but from our perspective there's
        // no longer a target to render or commit against.
        // Round-13 codex P3.
        this.composing = false;
        this.pendingLateCommit = null;
        this.compositionCommitData = null;
      } else if (activeId !== this.composingPaneId) {
        const oldId = this.composingPaneId;
        const oldRenderer = this.renderers.get(oldId.toString());
        const preeditText = oldRenderer?.currentPreedit ?? null;
        this.clearPreeditOn(oldId);
        this.composingPaneId = activeId;
        if (preeditText !== null && preeditText.length > 0) {
          const newRenderer = this.renderers.get(activeId.toString());
          const newGrid = this.grids.get(activeId.toString());
          if (newRenderer !== undefined && newGrid !== undefined) {
            newRenderer.setPreedit(preeditText);
            newRenderer.render(newGrid);
          }
        }
      }
      // The pane / cursor screen position may have moved as a
      // result of this layout shuffle. Re-anchor the sink so the
      // OS IME candidate popup follows. Mirrors Rust's
      // `ime_input_anchor()` recalc on every render in
      // `crates/ciri/src/app/render.rs`. Round-12 codex P3.
      if (this.composing) this.repositionCompositionSink();
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
    // Cursor may have moved as a side effect of terminal output —
    // refresh the sink anchor while composing so the OS IME
    // candidate popup tracks the new cursor position. Round-8
    // codex P2.
    if (this.composing) this.repositionCompositionSink();
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
    // A full sync may have moved the cursor or reshaped the pane —
    // mirror the cell-delta path and refresh the sink anchor while
    // composing so the OS IME candidate popup tracks the new
    // cursor. Round-9 codex P2.
    if (this.composing) this.repositionCompositionSink();
  }

  private destroyPane(paneId: bigint): void {
    const key = paneId.toString();
    const r = this.renderers.get(key);
    if (r !== undefined) r.destroy();
    this.renderers.delete(key);
    this.grids.delete(key);
    // Composing in a pane that just closed: bail the IME session
    // entirely. The downstream guards (grid existence check in
    // `finalizeCompositionCommit`) cover the safety hole, but
    // explicit cleanup keeps the invariants self-documenting and
    // avoids dispatching ghost preedit renders against a destroyed
    // renderer. Round-8 codex P1/P2 housekeeping.
    if (this.composingPaneId === paneId) {
      this.composing = false;
      this.composingPaneId = null;
      this.pendingLateCommit = null;
      this.compositionCommitData = null;
    }
    // Pane closed mid-drag: drop the in-flight mouse-forward session
    // so the window listeners don't fire `MouseInput` frames against
    // a destroyed grid. The server has already torn the pane down
    // and would drop the frames anyway, but quitting cleanly here
    // also avoids a confusing "send to non-existent pane" log line.
    if (this.mouseForward !== null && this.mouseForward.paneId === paneId) {
      const win = this.doc.defaultView;
      if (win !== null) {
        win.removeEventListener("mousemove", this.onWindowMouseMove);
        win.removeEventListener("mouseup", this.onWindowMouseUp);
      }
      this.mouseForward = null;
    }
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

/// Pack the standard xterm mouse-modifier bitmask from a DOM mouse /
/// wheel event. Server-side `send_mouse_input` ORs `(modifiers << 2)`
/// into the SGR button code, so these bits land at xterm's expected
/// positions (Shift=4, Alt=8, Ctrl=16 *after* the left-shift, i.e.
/// bits 1, 2, 3 of `modifiers`). `metaKey` is intentionally omitted
/// for xterm parity — terminals split on whether to map it to Alt,
/// and forwarding it would conflict with the OS's Cmd/Win shortcuts.
function mouseModifierMask(e: MouseEvent | WheelEvent): number {
  let mask = 0;
  if (e.shiftKey) mask |= 0x01;
  if (e.altKey) mask |= 0x02;
  if (e.ctrlKey) mask |= 0x04;
  return mask;
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
