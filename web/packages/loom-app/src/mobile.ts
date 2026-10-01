// Phone / touch chrome for the web client.
//
// On a narrow or touch-first viewport the desktop chrome (session
// dropdown + workspace tabs + pane chip strip, three rows) costs ~15% of
// a phone screen and still leaves the terminal without the keys a soft
// keyboard lacks. `MobileChrome` replaces it with:
//
//   <div class="loom-m-header">          ← one compact row
//     [●] [ title / session · ws · n/N ▾ ] [A−] [A+] [⌨]
//   </div>
//   …LayoutManager's .loom-app (one pane, full width)…
//   <div class="loom-m-keys">            ← extra-keys bar above the
//     esc tab ⇧⇥ ctrl alt ← ↑ ↓ →          soft keyboard
//     ^C ⏎ | / - ~ ` _ …  (scrolls)
//   </div>
//   <div class="loom-m-sheet-backdrop">  ← switcher (sessions,
//     <div class="loom-m-sheet">…</div>     workspaces, panes, tools)
//   </div>
//
// The DOM is always mounted; CSS shows it only under `.loom-mobile`,
// which LoomApp toggles on the root from a media query. This module is
// presentation only: it owns no protocol state and reports every user
// intent through `MobileChromeCallbacks`; LoomApp pushes state back in
// via `update()` / `setModifiers()`.
//
// Focus discipline: every control cancels `mousedown` (and repeatable
// keys also `pointerdown`) so tapping it never moves focus off the
// hidden input sink — which is what keeps the on-screen keyboard up
// while the user hits Esc / arrows / Ctrl.

/// What a key on the extra-keys bar does.
export type ExtraKeyAction =
  /// A named (`KeyboardEvent.key`-style) or printable key, combined with
  /// the latched modifiers and DECCKM by LoomApp.
  | { kind: "key"; key: string; shift?: boolean }
  /// Literal bytes, sent as-is (control chords spelled out for clarity).
  | { kind: "bytes"; bytes: string }
  /// Sticky modifier toggle.
  | { kind: "mod"; mod: StickyModifier }
  /// Paste from the clipboard (needs the tap's user activation).
  | { kind: "paste" };

export type StickyModifier = "ctrl" | "alt";

/// 0 = off, 1 = one-shot (applies to the next key), 2 = locked.
export type ModifierLevel = 0 | 1 | 2;

export interface ExtraKey {
  label: string;
  aria: string;
  action: ExtraKeyAction;
  /// Auto-repeat while held (arrows).
  repeat?: boolean;
}

const k = (label: string, aria: string, key: string, repeat = false): ExtraKey => ({
  label,
  aria,
  action: { kind: "key", key },
  ...(repeat ? { repeat } : {}),
});
const b = (label: string, aria: string, bytes: string): ExtraKey => ({
  label,
  aria,
  action: { kind: "bytes", bytes },
});

/// Fixed row: fits a 320px screen without scrolling. Esc / Tab /
/// Shift+Tab / Ctrl / arrows are what Claude Code, readline, and every
/// TUI need and no phone keyboard has.
export const EXTRA_KEYS_PRIMARY: readonly ExtraKey[] = [
  k("esc", "Escape", "Escape"),
  k("tab", "Tab", "Tab"),
  { label: "⇧⇥", aria: "Shift+Tab", action: { kind: "key", key: "Tab", shift: true } },
  { label: "ctrl", aria: "Control (sticky)", action: { kind: "mod", mod: "ctrl" } },
  { label: "alt", aria: "Alt (sticky)", action: { kind: "mod", mod: "alt" } },
  k("←", "Left", "ArrowLeft", true),
  k("↑", "Up", "ArrowUp", true),
  k("↓", "Down", "ArrowDown", true),
  k("→", "Right", "ArrowRight", true),
];

/// Scrollable row: the most-used chords first (visible without
/// scrolling), then symbols that sit on a phone keyboard's 2nd/3rd
/// page, then navigation.
export const EXTRA_KEYS_SECONDARY: readonly ExtraKey[] = [
  b("^C", "Control+C", "\x03"),
  k("⏎", "Enter", "Enter"),
  k("|", "Pipe", "|"),
  k("/", "Slash", "/"),
  k("-", "Dash", "-"),
  k("~", "Tilde", "~"),
  k("`", "Backtick", "`"),
  k("_", "Underscore", "_"),
  k("\\", "Backslash", "\\"),
  k(":", "Colon", ":"),
  k("*", "Asterisk", "*"),
  k("&", "Ampersand", "&"),
  k("$", "Dollar", "$"),
  k("<", "Less than", "<"),
  k(">", "Greater than", ">"),
  b("^D", "Control+D", "\x04"),
  b("^Z", "Control+Z", "\x1a"),
  b("^R", "Control+R", "\x12"),
  b("^L", "Control+L", "\x0c"),
  b("^U", "Control+U", "\x15"),
  k("home", "Home", "Home"),
  k("end", "End", "End"),
  k("pgup", "Page up", "PageUp", true),
  k("pgdn", "Page down", "PageDown", true),
  { label: "paste", aria: "Paste", action: { kind: "paste" } },
];

export interface MobilePane {
  id: bigint;
  title: string;
  active: boolean;
}

export interface MobileSnapshot {
  /// Empty when connected; otherwise "connecting…", "reconnecting…", …
  statusText: string;
  connected: boolean;
  sessions: readonly string[];
  currentSession: string;
  /// Workspace count + which is active (0-based).
  workspaceCount: number;
  activeWorkspace: number;
  /// Panes of the active workspace, in column-major (swipe) order.
  panes: readonly MobilePane[];
  fontSizePx: number;
}

export interface MobileChromeCallbacks {
  onKey(key: ExtraKey): void;
  onToggleKeyboard(): void;
  onFontStep(delta: number): void;
  onSelectSession(name: string): void;
  onNewSession(): void;
  onSelectWorkspace(idx: number): void;
  onNewWorkspace(): void;
  onSelectPane(id: bigint): void;
  onClosePane(id: bigint): void;
  onNewPane(): void;
  onFind(): void;
  onPaste(): void;
}

/// Delay before a held repeatable key starts repeating, and the repeat
/// interval — close to the iOS / Android key-repeat feel.
const REPEAT_DELAY_MS = 380;
const REPEAT_INTERVAL_MS = 55;
/// How long a pane-close "Close?" confirmation stays armed.
const CLOSE_CONFIRM_MS = 3000;
/// How long the swipe "pane n/N" hint stays up.
const HINT_MS = 900;

export class MobileChrome {
  readonly headerEl: HTMLElement;
  readonly keysEl: HTMLElement;
  readonly sheetEl: HTMLElement;
  readonly hintEl: HTMLElement;
  private readonly doc: Document;
  private readonly dotEl: HTMLElement;
  private readonly titleEl: HTMLElement;
  private readonly subEl: HTMLElement;
  private readonly kbBtn: HTMLButtonElement;
  private readonly sheetBody: HTMLElement;
  private readonly modButtons = new Map<StickyModifier, HTMLButtonElement>();
  private snapshot: MobileSnapshot | null = null;
  private sheetOpen = false;
  private closeArmedId: bigint | null = null;
  private readonly timers = new Set<ReturnType<typeof setTimeout>>();
  private repeatTimer: ReturnType<typeof setTimeout> | null = null;
  private hintTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(
    doc: Document,
    private readonly cb: MobileChromeCallbacks,
  ) {
    this.doc = doc;

    // ── Header ────────────────────────────────────────────────────
    this.headerEl = el(doc, "div", "loom-m-header");
    this.dotEl = el(doc, "span", "loom-m-dot");
    this.dotEl.setAttribute("aria-hidden", "true");
    const switchBtn = el(doc, "button", "loom-m-switch") as HTMLButtonElement;
    switchBtn.type = "button";
    switchBtn.setAttribute("aria-label", "Switch session, workspace, or pane");
    switchBtn.setAttribute("aria-haspopup", "dialog");
    const labels = el(doc, "span", "loom-m-labels");
    this.titleEl = el(doc, "span", "loom-m-title");
    this.subEl = el(doc, "span", "loom-m-sub");
    labels.append(this.titleEl, this.subEl);
    const caret = el(doc, "span", "loom-m-caret", "▾");
    caret.setAttribute("aria-hidden", "true");
    switchBtn.append(labels, caret);
    this.tap(switchBtn, () => this.toggleSheet());
    const fontDown = this.iconButton("A−", "Smaller text", () => cb.onFontStep(-1));
    fontDown.classList.add("loom-m-font");
    const fontUp = this.iconButton("A+", "Larger text", () => cb.onFontStep(1));
    fontUp.classList.add("loom-m-font");
    this.kbBtn = this.iconButton("⌨", "Show keyboard", () => cb.onToggleKeyboard());
    this.kbBtn.classList.add("loom-m-kb");
    this.headerEl.append(this.dotEl, switchBtn, fontDown, fontUp, this.kbBtn);

    // ── Extra keys ────────────────────────────────────────────────
    this.keysEl = el(doc, "div", "loom-m-keys");
    this.keysEl.setAttribute("role", "toolbar");
    this.keysEl.setAttribute("aria-label", "Terminal keys");
    this.keysEl.append(
      this.keyRow(EXTRA_KEYS_PRIMARY, "loom-m-row loom-m-row-fixed"),
      this.keyRow(EXTRA_KEYS_SECONDARY, "loom-m-row loom-m-row-scroll"),
    );

    // ── Switcher sheet ────────────────────────────────────────────
    this.sheetEl = el(doc, "div", "loom-m-sheet-backdrop");
    this.sheetEl.hidden = true;
    const sheet = el(doc, "div", "loom-m-sheet");
    sheet.setAttribute("role", "dialog");
    sheet.setAttribute("aria-label", "Switcher");
    this.sheetBody = el(doc, "div", "loom-m-sheet-body");
    sheet.appendChild(this.sheetBody);
    this.sheetEl.appendChild(sheet);
    // Tap on the dimmed backdrop (not the sheet) closes it.
    this.sheetEl.addEventListener("mousedown", (e) => e.preventDefault());
    this.sheetEl.addEventListener("click", (e) => {
      if (e.target === this.sheetEl) this.closeSheet();
    });

    // ── Swipe hint ────────────────────────────────────────────────
    this.hintEl = el(doc, "div", "loom-m-hint");
    this.hintEl.hidden = true;
    this.hintEl.setAttribute("aria-live", "polite");
  }

  /// All top-level nodes, for the caller to mount / unmount.
  get nodes(): HTMLElement[] {
    return [this.headerEl, this.keysEl, this.sheetEl, this.hintEl];
  }

  get isSheetOpen(): boolean {
    return this.sheetOpen;
  }

  update(s: MobileSnapshot): void {
    this.snapshot = s;
    const active = s.panes.find((p) => p.active);
    const idx = active === undefined ? -1 : s.panes.indexOf(active);
    this.dotEl.className = `loom-m-dot ${s.connected ? "is-ok" : "is-wait"}`;
    if (!s.connected) {
      this.titleEl.textContent = s.statusText;
    } else {
      this.titleEl.textContent =
        active === undefined ? "loomtty" : paneLabel(active);
    }
    const parts: string[] = [];
    if (s.currentSession.length > 0 && !s.currentSession.startsWith("__")) {
      parts.push(s.currentSession);
    }
    if (s.workspaceCount > 1) parts.push(`ws ${s.activeWorkspace + 1}/${s.workspaceCount}`);
    if (s.panes.length > 1 && idx >= 0) parts.push(`pane ${idx + 1}/${s.panes.length}`);
    this.subEl.textContent = parts.join(" · ");
    if (this.sheetOpen) this.renderSheet();
  }

  setModifiers(mods: Record<StickyModifier, ModifierLevel>): void {
    for (const [mod, btn] of this.modButtons) {
      const lvl = mods[mod];
      btn.classList.toggle("is-latched", lvl === 1);
      btn.classList.toggle("is-locked", lvl === 2);
      btn.setAttribute("aria-pressed", lvl === 0 ? "false" : "true");
    }
  }

  setKeyboardOpen(open: boolean): void {
    this.kbBtn.classList.toggle("is-on", open);
    const label = open ? "Hide keyboard" : "Show keyboard";
    this.kbBtn.setAttribute("aria-label", label);
    this.kbBtn.title = label;
  }

  /// Brief centered "2/3 · title" pill after a swipe pane switch.
  showHint(text: string): void {
    this.hintEl.textContent = text;
    this.hintEl.hidden = false;
    if (this.hintTimer !== null) clearTimeout(this.hintTimer);
    this.hintTimer = setTimeout(() => {
      this.hintEl.hidden = true;
      this.hintTimer = null;
    }, HINT_MS);
  }

  toggleSheet(): void {
    if (this.sheetOpen) this.closeSheet();
    else this.openSheet();
  }

  openSheet(): void {
    this.sheetOpen = true;
    this.closeArmedId = null;
    this.renderSheet();
    this.sheetEl.hidden = false;
  }

  closeSheet(): void {
    this.sheetOpen = false;
    this.closeArmedId = null;
    this.sheetEl.hidden = true;
  }

  destroy(): void {
    for (const t of this.timers) clearTimeout(t);
    this.timers.clear();
    this.stopRepeat();
    if (this.hintTimer !== null) clearTimeout(this.hintTimer);
    for (const n of this.nodes) n.remove();
  }

  // ─── internals ───────────────────────────────────────────────────

  private renderSheet(): void {
    const s = this.snapshot;
    const doc = this.doc;
    const body: Node[] = [];
    const head = el(doc, "div", "loom-m-sheet-head");
    head.append(el(doc, "span", "loom-m-sheet-title", "Switch"));
    head.append(this.iconButton("✕", "Close switcher", () => this.closeSheet()));
    body.push(head);
    if (s === null) {
      this.sheetBody.replaceChildren(...body);
      return;
    }

    // Panes first — the most frequent switch.
    body.push(el(doc, "div", "loom-m-section", "Panes"));
    const paneList = el(doc, "div", "loom-m-list");
    s.panes.forEach((p, i) => {
      const row = el(doc, "div", `loom-m-pane${p.active ? " is-active" : ""}`);
      const pick = this.textButton(`${i + 1}  ${paneLabel(p)}`, "loom-m-pane-pick", () => {
        this.closeSheet();
        this.cb.onSelectPane(p.id);
      });
      if (p.active) pick.setAttribute("aria-current", "true");
      const armed = this.closeArmedId === p.id;
      const close = this.textButton(armed ? "Close?" : "✕", "loom-m-pane-close", () => {
        if (this.closeArmedId === p.id) {
          this.closeArmedId = null;
          this.cb.onClosePane(p.id);
          return;
        }
        // Two-step close: a stray tap on a phone shouldn't kill a
        // running agent. The first tap arms, the second confirms.
        this.closeArmedId = p.id;
        this.renderSheet();
        const t = setTimeout(() => {
          this.timers.delete(t);
          if (this.closeArmedId === p.id) {
            this.closeArmedId = null;
            if (this.sheetOpen) this.renderSheet();
          }
        }, CLOSE_CONFIRM_MS);
        this.timers.add(t);
      });
      if (armed) close.classList.add("is-armed");
      close.setAttribute("aria-label", `Close pane ${i + 1}: ${paneLabel(p)}`);
      row.append(pick, close);
      paneList.appendChild(row);
    });
    paneList.appendChild(
      this.textButton("+ New pane", "loom-m-add", () => {
        this.closeSheet();
        this.cb.onNewPane();
      }),
    );
    body.push(paneList);

    body.push(el(doc, "div", "loom-m-section", "Workspaces"));
    const wsRow = el(doc, "div", "loom-m-chips");
    for (let i = 0; i < s.workspaceCount; i += 1) {
      const btn = this.textButton(String(i + 1), "loom-m-chip", () => {
        this.closeSheet();
        this.cb.onSelectWorkspace(i);
      });
      btn.setAttribute("aria-label", `Workspace ${i + 1}`);
      if (i === s.activeWorkspace) btn.setAttribute("aria-current", "true");
      wsRow.appendChild(btn);
    }
    wsRow.appendChild(
      this.textButton("+", "loom-m-chip loom-m-chip-add", () => {
        this.closeSheet();
        this.cb.onNewWorkspace();
      }),
    );
    (wsRow.lastChild as HTMLElement).setAttribute("aria-label", "New workspace");
    body.push(wsRow);

    const sessions = s.sessions.filter((n) => !n.startsWith("__"));
    body.push(el(doc, "div", "loom-m-section", "Sessions"));
    const sessRow = el(doc, "div", "loom-m-chips");
    for (const name of sessions) {
      const btn = this.textButton(name, "loom-m-chip", () => {
        this.closeSheet();
        this.cb.onSelectSession(name);
      });
      if (name === s.currentSession) btn.setAttribute("aria-current", "true");
      sessRow.appendChild(btn);
    }
    sessRow.appendChild(
      this.textButton("+ new", "loom-m-chip loom-m-chip-add", () => {
        this.closeSheet();
        this.cb.onNewSession();
      }),
    );
    body.push(sessRow);

    body.push(el(doc, "div", "loom-m-section", "Tools"));
    const tools = el(doc, "div", "loom-m-chips");
    tools.append(
      this.textButton("Find", "loom-m-chip", () => {
        this.closeSheet();
        this.cb.onFind();
      }),
      this.textButton("Paste", "loom-m-chip", () => {
        this.closeSheet();
        this.cb.onPaste();
      }),
      this.textButton("A−", "loom-m-chip", () => this.cb.onFontStep(-1)),
      el(doc, "span", "loom-m-font-size", `${s.fontSizePx}px`),
      this.textButton("A+", "loom-m-chip", () => this.cb.onFontStep(1)),
    );
    body.push(tools);
    this.sheetBody.replaceChildren(...body);
  }

  private keyRow(keys: readonly ExtraKey[], cls: string): HTMLElement {
    const row = el(this.doc, "div", cls);
    for (const key of keys) {
      const btn = el(this.doc, "button", "loom-m-key", key.label) as HTMLButtonElement;
      btn.type = "button";
      btn.tabIndex = -1;
      btn.setAttribute("aria-label", key.aria);
      if (key.action.kind === "mod") {
        btn.classList.add("loom-m-mod");
        btn.setAttribute("aria-pressed", "false");
        this.modButtons.set(key.action.mod, btn);
      }
      if (key.repeat === true) {
        this.wireRepeat(btn, () => this.cb.onKey(key));
      } else {
        this.tap(btn, () => this.cb.onKey(key));
      }
      row.appendChild(btn);
    }
    return row;
  }

  /// Fire on `click` (a real user activation, which paste and focus
  /// need; and browsers don't fire it after a scroll-pan of the key
  /// row). `mousedown` is cancelled so focus stays on the input sink.
  private tap(btn: HTMLElement, onActivate: () => void): void {
    btn.addEventListener("mousedown", (e) => e.preventDefault());
    btn.addEventListener("click", (e) => {
      e.preventDefault();
      onActivate();
    });
  }

  /// Repeatable key: fire on press, then auto-repeat while held.
  private wireRepeat(btn: HTMLElement, fire: () => void): void {
    btn.addEventListener("mousedown", (e) => e.preventDefault());
    btn.addEventListener("pointerdown", (e) => {
      e.preventDefault();
      this.stopRepeat();
      try {
        btn.setPointerCapture(e.pointerId);
      } catch {
        /* pointer already gone */
      }
      fire();
      const loop = (): void => {
        fire();
        this.repeatTimer = setTimeout(loop, REPEAT_INTERVAL_MS);
      };
      this.repeatTimer = setTimeout(loop, REPEAT_DELAY_MS);
    });
    const stop = (): void => this.stopRepeat();
    btn.addEventListener("pointerup", stop);
    btn.addEventListener("pointercancel", stop);
    btn.addEventListener("lostpointercapture", stop);
    // Keyboard activation (Enter/Space on a focused key, a11y tools)
    // arrives as a click with `detail === 0`; pointer taps already fired.
    btn.addEventListener("click", (e) => {
      e.preventDefault();
      if (e.detail === 0) fire();
    });
  }

  private stopRepeat(): void {
    if (this.repeatTimer !== null) {
      clearTimeout(this.repeatTimer);
      this.repeatTimer = null;
    }
  }

  private iconButton(label: string, aria: string, onActivate: () => void): HTMLButtonElement {
    const btn = el(this.doc, "button", "loom-m-icon", label) as HTMLButtonElement;
    btn.type = "button";
    btn.setAttribute("aria-label", aria);
    btn.title = aria;
    this.tap(btn, onActivate);
    return btn;
  }

  private textButton(label: string, cls: string, onActivate: () => void): HTMLButtonElement {
    const btn = el(this.doc, "button", cls, label) as HTMLButtonElement;
    btn.type = "button";
    this.tap(btn, onActivate);
    return btn;
  }
}

function paneLabel(p: MobilePane): string {
  return p.title.length > 0 ? p.title : `Pane ${p.id.toString()}`;
}

function el(doc: Document, tag: string, cls: string, text?: string): HTMLElement {
  const n = doc.createElement(tag);
  n.className = cls;
  if (text !== undefined) n.textContent = text;
  return n;
}
