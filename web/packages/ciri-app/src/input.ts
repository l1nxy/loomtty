// Keyboard → terminal byte encoding.
//
// Translates a browser `KeyboardEvent` into the xterm-flavoured byte
// sequence the server's PTY expects. v1 stays close to standard xterm:
//
//   * Printable chars   → UTF-8 of `event.key`
//   * Enter             → 0x0d
//   * Tab               → 0x09
//   * Backspace         → 0x7f (DEL), 0x08 if Ctrl held
//   * Escape            → 0x1b
//   * Ctrl + letter     → C0 control (0x01..0x1f) via the standard
//                         `key & 0x1f` mapping
//   * Alt + char        → ESC + char  (xterm "meta sends escape")
//   * Arrows / Home / End / Insert / Delete / PageUp / PageDown / F1-F12 →
//                         the canonical CSI / SS3 sequences
//   * DECCKM mode       → callers can flip `applicationCursorKeys` to
//                         get SS3 (ESC O X) instead of CSI (ESC [ X)
//                         for arrows + Home/End. The app layer will
//                         track the mode from the server's `mode_flags`.
//
// What this DOES NOT do (deferred past v1):
//   * Modifier-encoded function keys (`ESC [1;5A` for Ctrl+Up etc.)
//   * IME composition  — composition events are a separate path
//   * Keypad / numlock handling
//   * Bracketed paste — `Input` is whatever the OS clipboard
//     reader hands us; the *paste* path lives in `app.ts`, not here.
//
// Browser-reserved shortcuts are explicitly NOT intercepted:
//   * Cmd / Win / Meta anything → returns `null` (system clipboard,
//     window management — the browser owns these)
//   * Ctrl + Shift + anything   → returns `null` (Ctrl+Shift+C/V is
//     the conventional web-terminal clipboard path, and the browser
//     reserves Ctrl+Shift+T/N/W for tab/window management; trying to
//     `preventDefault` on those is futile and we're better off being
//     consistent than mapping some Ctrl+Shift combos but not others)

/// What `encodeKeyboardEvent` hands back to the caller. `bytes` is the
/// wire payload to send through `CiriClient.sendInput`. `preventDefault`
/// is `true` whenever we produced bytes — the caller should call
/// `event.preventDefault()` so the browser doesn't double-handle the
/// key (Tab moving focus, Backspace navigating back, etc).
export interface KeyEncoding {
  bytes: Uint8Array;
  preventDefault: boolean;
}

export interface KeyEncoderOptions {
  /// When `true`, arrows and Home/End emit SS3 (`ESC O X`) instead of
  /// CSI (`ESC [ X`). The DECCKM-enabled PTU (vim, less, htop, …)
  /// expects SS3; cooked-mode shells expect CSI. The app layer tracks
  /// the mode via the server's `mode_flags` and flips this flag for
  /// the active pane.
  applicationCursorKeys?: boolean;
}

/// Translate a `KeyboardEvent` into a payload for the terminal, or
/// return `null` to defer to the browser's default action (e.g. Cmd+C
/// → system clipboard).
export function encodeKeyboardEvent(
  e: KeyboardEvent,
  opts?: KeyEncoderOptions,
): KeyEncoding | null {
  // Modifier-only events fire on press *and* release of Shift/Ctrl/...
  // by themselves; the terminal expects nothing on those. "Dead" is
  // the in-progress dead key (e.g. accent before vowel on a French
  // layout) — the eventual composed character arrives via a later
  // event, so we drop the dead-key event.
  if (isModifierOnlyKey(e.key)) return null;

  // System-reserved chord combos: don't intercept.
  if (e.metaKey) return null;
  if (e.ctrlKey && e.shiftKey) return null;

  const applicationCursorKeys = opts?.applicationCursorKeys ?? false;
  const cursorPrefix = applicationCursorKeys ? SS3 : CSI;

  switch (e.key) {
    case "Enter":
      return ok(BYTE_CR);
    case "Tab":
      return ok(BYTE_HT);
    case "Backspace":
      // Convention: bare Backspace sends DEL (0x7f); Ctrl+Backspace
      // sends BS (0x08). Mirrors xterm + most modern emulators.
      return ok(e.ctrlKey ? BYTE_BS : BYTE_DEL);
    case "Escape":
      return ok(BYTE_ESC);
    case "ArrowUp":
      return okSeq(cursorPrefix + "A");
    case "ArrowDown":
      return okSeq(cursorPrefix + "B");
    case "ArrowRight":
      return okSeq(cursorPrefix + "C");
    case "ArrowLeft":
      return okSeq(cursorPrefix + "D");
    case "Home":
      return okSeq(cursorPrefix + "H");
    case "End":
      return okSeq(cursorPrefix + "F");
    case "Insert":
      return okSeq("\x1b[2~");
    case "Delete":
      return okSeq("\x1b[3~");
    case "PageUp":
      return okSeq("\x1b[5~");
    case "PageDown":
      return okSeq("\x1b[6~");
    case "F1":
      return okSeq("\x1bOP");
    case "F2":
      return okSeq("\x1bOQ");
    case "F3":
      return okSeq("\x1bOR");
    case "F4":
      return okSeq("\x1bOS");
    case "F5":
      return okSeq("\x1b[15~");
    case "F6":
      return okSeq("\x1b[17~");
    case "F7":
      return okSeq("\x1b[18~");
    case "F8":
      return okSeq("\x1b[19~");
    case "F9":
      return okSeq("\x1b[20~");
    case "F10":
      return okSeq("\x1b[21~");
    case "F11":
      return okSeq("\x1b[23~");
    case "F12":
      return okSeq("\x1b[24~");
    default:
      break;
  }

  // AltGr (and macOS Option) often produce a printable grapheme
  // while the browser sets *both* `ctrlKey` and `altKey` (Windows /
  // Linux AltGr maps to Right Alt + synthetic Left Ctrl). The same
  // event can also flag `getModifierState("AltGraph")`. Detect that
  // shape before the Ctrl-chord branch — otherwise `@`, `{`, `€` and
  // friends would be misencoded as `Ctrl+@` (NUL), `Ctrl+[` (ESC),
  // etc. Send the produced grapheme straight through as UTF-8 with
  // no ESC prefix; this is *not* the xterm "meta sends escape"
  // path because the user did not press a meta modifier — the
  // AltGr was needed just to compose the printable.
  const isAltGr =
    (e.ctrlKey && e.altKey) || e.getModifierState("AltGraph");
  if (isAltGr && isPrintableKey(e.key)) {
    return { bytes: TEXT_ENCODER.encode(e.key), preventDefault: true };
  }

  // Ctrl + char (and bare `\` etc.) → C0 control byte.
  if (e.ctrlKey) {
    const ctrlByte = ctrlChord(e.key);
    if (ctrlByte === null) return null;
    return ok(ctrlByte);
  }

  // Plain printable. `KeyboardEvent.key` returns the produced grapheme
  // (e.g. "é" for á + e on a dead-key keyboard); UTF-8 encode whatever
  // shows up.
  if (!isPrintableKey(e.key)) return null;

  if (e.altKey) {
    // Alt prefixes ESC — xterm's "meta sends escape" mode. Browsers
    // sometimes also synthesize a precomposed grapheme (Alt+a → å on
    // macOS) and route it through `key`; we honour whatever `key`
    // says rather than trying to second-guess the platform layout.
    const body = TEXT_ENCODER.encode(e.key);
    const out = new Uint8Array(body.length + 1);
    out[0] = 0x1b;
    out.set(body, 1);
    return { bytes: out, preventDefault: true };
  }

  return { bytes: TEXT_ENCODER.encode(e.key), preventDefault: true };
}

// ─── helpers ─────────────────────────────────────────────────────

const CSI = "\x1b[";
const SS3 = "\x1bO";
const BYTE_CR = 0x0d;
const BYTE_HT = 0x09;
const BYTE_BS = 0x08;
const BYTE_DEL = 0x7f;
const BYTE_ESC = 0x1b;
const TEXT_ENCODER = new TextEncoder();

function ok(byte: number): KeyEncoding {
  return { bytes: new Uint8Array([byte]), preventDefault: true };
}

function okSeq(s: string): KeyEncoding {
  return { bytes: TEXT_ENCODER.encode(s), preventDefault: true };
}

function isModifierOnlyKey(key: string): boolean {
  switch (key) {
    case "Shift":
    case "Control":
    case "Alt":
    case "AltGraph":
    case "Meta":
    case "OS":
    case "CapsLock":
    case "NumLock":
    case "ScrollLock":
    case "Dead":
    case "Process":
    case "Unidentified":
      return true;
    default:
      return false;
  }
}

/// Given the `event.key` for a Ctrl-chord (e.g. "a", "[", "?"), produce
/// the C0/DEL byte the PTY expects. Returns `null` for combos that
/// don't have a standard mapping (the caller drops them).
function ctrlChord(key: string): number | null {
  if (key.length !== 1) return null;
  const c = key.charCodeAt(0);
  // A..Z / a..z → 0x01..0x1a   (the canonical Ctrl+letter mapping)
  if (c >= 0x41 && c <= 0x5a) return c & 0x1f;
  if (c >= 0x61 && c <= 0x7a) return c & 0x1f;
  // @  →  0x00
  // [  →  0x1b   (same as ESC — Ctrl+[ is the classic ESC sub)
  // \  →  0x1c
  // ]  →  0x1d
  // ^  →  0x1e
  // _  →  0x1f
  if (c >= 0x40 && c <= 0x5f) return c & 0x1f;
  if (key === " ") return 0x00;
  // Ctrl+? → DEL (terminals route it the same as Backspace; we mirror).
  if (key === "?") return 0x7f;
  return null;
}

function isPrintableKey(key: string): boolean {
  // `KeyboardEvent.key` is either:
  //   * a single character / grapheme produced by the key, or
  //   * a named key string ("Enter", "F13", "ContextMenu", "MediaPlayPause").
  // The named keys we recognise are matched in the top-level switch
  // statement, so by the time we reach here anything length > 1 is an
  // *unhandled* named key. The one exception is a UTF-16 surrogate
  // pair (length 2, first half in the high-surrogate range) which
  // encodes a single supplementary-plane code point — accept those.
  if (key.length === 0) return false;
  if (key.length === 1) {
    const c = key.charCodeAt(0);
    // Reject C0 control bytes and DEL.
    return c >= 0x20 && c !== 0x7f;
  }
  if (key.length === 2) {
    const hi = key.charCodeAt(0);
    const lo = key.charCodeAt(1);
    return (
      hi >= 0xd800 && hi <= 0xdbff && lo >= 0xdc00 && lo <= 0xdfff
    );
  }
  return false;
}
