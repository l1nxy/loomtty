import { describe, expect, test, vi } from "vitest";
import { encodeKeyboardEvent } from "./input.js";

/// Build a `KeyboardEvent` via jsdom's constructor — `key` plus
/// modifier flags is what we read in the encoder, so the rest of the
/// fields (code, location, repeat, …) don't matter for these tests.
function ev(
  key: string,
  opts: {
    ctrlKey?: boolean;
    altKey?: boolean;
    shiftKey?: boolean;
    metaKey?: boolean;
  } = {},
): KeyboardEvent {
  return new KeyboardEvent("keydown", {
    key,
    ctrlKey: opts.ctrlKey ?? false,
    altKey: opts.altKey ?? false,
    shiftKey: opts.shiftKey ?? false,
    metaKey: opts.metaKey ?? false,
  });
}

function decodeUtf8(bytes: Uint8Array): string {
  return new TextDecoder().decode(bytes);
}

describe("encodeKeyboardEvent — printable", () => {
  test("plain ASCII passes through as UTF-8", () => {
    const r = encodeKeyboardEvent(ev("a"))!;
    expect(decodeUtf8(r.bytes)).toBe("a");
    expect(r.preventDefault).toBe(true);
  });

  test("uppercase passes through as the literal key produced", () => {
    // Browser already applied shift to produce "A" — we encode key,
    // not whatever we'd compute from key + modifiers.
    const r = encodeKeyboardEvent(ev("A", { shiftKey: true }))!;
    expect(decodeUtf8(r.bytes)).toBe("A");
  });

  test("non-ASCII (CJK) is UTF-8 encoded", () => {
    const r = encodeKeyboardEvent(ev("中"))!;
    expect(Array.from(r.bytes)).toEqual([0xe4, 0xb8, 0xad]);
  });

  test("space encodes as 0x20", () => {
    const r = encodeKeyboardEvent(ev(" "))!;
    expect(Array.from(r.bytes)).toEqual([0x20]);
  });
});

describe("encodeKeyboardEvent — control keys", () => {
  test("Enter → CR", () => {
    expect(encodeKeyboardEvent(ev("Enter"))!.bytes).toEqual(new Uint8Array([0x0d]));
  });

  test("Tab → HT", () => {
    expect(encodeKeyboardEvent(ev("Tab"))!.bytes).toEqual(new Uint8Array([0x09]));
  });

  test("Backspace → DEL (0x7f) by default", () => {
    expect(encodeKeyboardEvent(ev("Backspace"))!.bytes).toEqual(new Uint8Array([0x7f]));
  });

  test("Ctrl+Backspace → BS (0x08)", () => {
    expect(
      encodeKeyboardEvent(ev("Backspace", { ctrlKey: true }))!.bytes,
    ).toEqual(new Uint8Array([0x08]));
  });

  test("Escape → ESC (0x1b)", () => {
    expect(encodeKeyboardEvent(ev("Escape"))!.bytes).toEqual(new Uint8Array([0x1b]));
  });
});

describe("encodeKeyboardEvent — cursor keys (CSI/SS3)", () => {
  test("ArrowUp default emits CSI A", () => {
    expect(decodeUtf8(encodeKeyboardEvent(ev("ArrowUp"))!.bytes)).toBe("\x1b[A");
  });

  test("ArrowDown / Left / Right CSI", () => {
    expect(decodeUtf8(encodeKeyboardEvent(ev("ArrowDown"))!.bytes)).toBe("\x1b[B");
    expect(decodeUtf8(encodeKeyboardEvent(ev("ArrowRight"))!.bytes)).toBe("\x1b[C");
    expect(decodeUtf8(encodeKeyboardEvent(ev("ArrowLeft"))!.bytes)).toBe("\x1b[D");
  });

  test("Application cursor keys → SS3", () => {
    const opts = { applicationCursorKeys: true };
    expect(decodeUtf8(encodeKeyboardEvent(ev("ArrowUp"), opts)!.bytes)).toBe("\x1bOA");
    expect(decodeUtf8(encodeKeyboardEvent(ev("ArrowDown"), opts)!.bytes)).toBe("\x1bOB");
  });

  test("Home / End default CSI; SS3 under appcursor", () => {
    expect(decodeUtf8(encodeKeyboardEvent(ev("Home"))!.bytes)).toBe("\x1b[H");
    expect(decodeUtf8(encodeKeyboardEvent(ev("End"))!.bytes)).toBe("\x1b[F");
    expect(
      decodeUtf8(encodeKeyboardEvent(ev("Home"), { applicationCursorKeys: true })!.bytes),
    ).toBe("\x1bOH");
    expect(
      decodeUtf8(encodeKeyboardEvent(ev("End"), { applicationCursorKeys: true })!.bytes),
    ).toBe("\x1bOF");
  });
});

describe("encodeKeyboardEvent — editing/page", () => {
  test("Insert, Delete, PageUp, PageDown", () => {
    expect(decodeUtf8(encodeKeyboardEvent(ev("Insert"))!.bytes)).toBe("\x1b[2~");
    expect(decodeUtf8(encodeKeyboardEvent(ev("Delete"))!.bytes)).toBe("\x1b[3~");
    expect(decodeUtf8(encodeKeyboardEvent(ev("PageUp"))!.bytes)).toBe("\x1b[5~");
    expect(decodeUtf8(encodeKeyboardEvent(ev("PageDown"))!.bytes)).toBe("\x1b[6~");
  });
});

describe("encodeKeyboardEvent — function keys", () => {
  test("F1..F4 → SS3 letters", () => {
    expect(decodeUtf8(encodeKeyboardEvent(ev("F1"))!.bytes)).toBe("\x1bOP");
    expect(decodeUtf8(encodeKeyboardEvent(ev("F2"))!.bytes)).toBe("\x1bOQ");
    expect(decodeUtf8(encodeKeyboardEvent(ev("F3"))!.bytes)).toBe("\x1bOR");
    expect(decodeUtf8(encodeKeyboardEvent(ev("F4"))!.bytes)).toBe("\x1bOS");
  });

  test("F5..F12 → CSI ##~", () => {
    expect(decodeUtf8(encodeKeyboardEvent(ev("F5"))!.bytes)).toBe("\x1b[15~");
    expect(decodeUtf8(encodeKeyboardEvent(ev("F6"))!.bytes)).toBe("\x1b[17~");
    expect(decodeUtf8(encodeKeyboardEvent(ev("F7"))!.bytes)).toBe("\x1b[18~");
    expect(decodeUtf8(encodeKeyboardEvent(ev("F8"))!.bytes)).toBe("\x1b[19~");
    expect(decodeUtf8(encodeKeyboardEvent(ev("F9"))!.bytes)).toBe("\x1b[20~");
    expect(decodeUtf8(encodeKeyboardEvent(ev("F10"))!.bytes)).toBe("\x1b[21~");
    expect(decodeUtf8(encodeKeyboardEvent(ev("F11"))!.bytes)).toBe("\x1b[23~");
    expect(decodeUtf8(encodeKeyboardEvent(ev("F12"))!.bytes)).toBe("\x1b[24~");
  });
});

describe("encodeKeyboardEvent — Ctrl chord", () => {
  test("Ctrl+letter → C0 control byte", () => {
    // Ctrl+C, Ctrl+A, Ctrl+Z, Ctrl+lowercase-c
    expect(encodeKeyboardEvent(ev("c", { ctrlKey: true }))!.bytes).toEqual(
      new Uint8Array([0x03]),
    );
    expect(encodeKeyboardEvent(ev("a", { ctrlKey: true }))!.bytes).toEqual(
      new Uint8Array([0x01]),
    );
    expect(encodeKeyboardEvent(ev("z", { ctrlKey: true }))!.bytes).toEqual(
      new Uint8Array([0x1a]),
    );
    // Uppercase too — some keyboard layouts emit caps even with Ctrl.
    expect(encodeKeyboardEvent(ev("Z", { ctrlKey: true }))!.bytes).toEqual(
      new Uint8Array([0x1a]),
    );
  });

  test("Ctrl+[ → ESC; Ctrl+] → GS; Ctrl+\\ → FS", () => {
    expect(encodeKeyboardEvent(ev("[", { ctrlKey: true }))!.bytes).toEqual(
      new Uint8Array([0x1b]),
    );
    expect(encodeKeyboardEvent(ev("]", { ctrlKey: true }))!.bytes).toEqual(
      new Uint8Array([0x1d]),
    );
    expect(encodeKeyboardEvent(ev("\\", { ctrlKey: true }))!.bytes).toEqual(
      new Uint8Array([0x1c]),
    );
  });

  test("Ctrl+Space → NUL; Ctrl+? → DEL", () => {
    expect(encodeKeyboardEvent(ev(" ", { ctrlKey: true }))!.bytes).toEqual(
      new Uint8Array([0x00]),
    );
    expect(encodeKeyboardEvent(ev("?", { ctrlKey: true }))!.bytes).toEqual(
      new Uint8Array([0x7f]),
    );
  });

  test("unmapped Ctrl chord drops (returns null)", () => {
    // Ctrl+F1 has no canonical mapping in our v1 encoder — we'd need
    // the xterm modifier-CSI extension to encode it, deferred past
    // v1. The arrow / function-key cases above don't drop because
    // they have a non-modified encoding regardless.
    expect(encodeKeyboardEvent(ev("§", { ctrlKey: true }))).toBeNull();
  });
});

describe("encodeKeyboardEvent — AltGr (Ctrl+Alt printable)", () => {
  test("Ctrl+Alt+printable is treated as plain UTF-8, not a Ctrl chord", () => {
    // Round-3 codex regression: Windows / Linux AltGr layouts emit
    // KeyboardEvent { ctrlKey: true, altKey: true, key: "@" } for the
    // German keyboard's `@`. The encoder must NOT take this through
    // ctrlChord("@") = 0x00.
    const r = encodeKeyboardEvent(ev("@", { ctrlKey: true, altKey: true }))!;
    expect(decodeUtf8(r.bytes)).toBe("@");
  });

  test("AltGr produces non-ASCII (€) as multi-byte UTF-8, no ESC prefix", () => {
    const r = encodeKeyboardEvent(ev("€", { ctrlKey: true, altKey: true }))!;
    expect(Array.from(r.bytes)).toEqual([0xe2, 0x82, 0xac]);
  });

  test("getModifierState('AltGraph') alone also treats key as printable", () => {
    // Some browsers (Chrome on certain Linux setups) report only the
    // AltGraph modifier without setting ctrl+alt. Honor that signal
    // so the keystroke still encodes to its printable bytes.
    const e = ev("{", { ctrlKey: true });
    vi.spyOn(e, "getModifierState").mockImplementation(
      (m) => m === "AltGraph",
    );
    const r = encodeKeyboardEvent(e)!;
    expect(decodeUtf8(r.bytes)).toBe("{");
  });

  test("Ctrl+Alt with non-printable key still resolves to a control byte", () => {
    // Ctrl+Alt+Backspace shouldn't be misclassified as "AltGr". The
    // printability check protects this path: Backspace is a named
    // key, not printable, so we fall through to its dedicated entry.
    const r = encodeKeyboardEvent(
      ev("Backspace", { ctrlKey: true, altKey: true }),
    )!;
    // Ctrl+Backspace path → BS (0x08); the altKey is otherwise
    // irrelevant for the Backspace branch.
    expect(r.bytes).toEqual(new Uint8Array([0x08]));
  });
});

describe("encodeKeyboardEvent — Alt chord", () => {
  test("Alt+letter → ESC prefix + char", () => {
    const r = encodeKeyboardEvent(ev("a", { altKey: true }))!;
    expect(Array.from(r.bytes)).toEqual([0x1b, 0x61]);
  });

  test("Alt+non-ASCII still preserves the produced grapheme", () => {
    // Many macOS layouts emit "å" for Alt+a — the browser hands us
    // the composed character on `event.key`; we just prepend ESC.
    const r = encodeKeyboardEvent(ev("å", { altKey: true }))!;
    // ESC + "å" (UTF-8: c3 a5)
    expect(Array.from(r.bytes)).toEqual([0x1b, 0xc3, 0xa5]);
  });
});

describe("encodeKeyboardEvent — browser-reserved fall-through", () => {
  test("Cmd/Win (metaKey) → null (defer to system)", () => {
    expect(encodeKeyboardEvent(ev("c", { metaKey: true }))).toBeNull();
    expect(encodeKeyboardEvent(ev("v", { metaKey: true }))).toBeNull();
    expect(encodeKeyboardEvent(ev("t", { metaKey: true }))).toBeNull();
  });

  test("Ctrl+Shift+anything → null (browser clipboard / shortcuts)", () => {
    expect(encodeKeyboardEvent(ev("C", { ctrlKey: true, shiftKey: true }))).toBeNull();
    expect(encodeKeyboardEvent(ev("V", { ctrlKey: true, shiftKey: true }))).toBeNull();
    expect(encodeKeyboardEvent(ev("T", { ctrlKey: true, shiftKey: true }))).toBeNull();
  });

  test("modifier-only keys → null", () => {
    expect(encodeKeyboardEvent(ev("Shift"))).toBeNull();
    expect(encodeKeyboardEvent(ev("Control"))).toBeNull();
    expect(encodeKeyboardEvent(ev("Alt"))).toBeNull();
    expect(encodeKeyboardEvent(ev("Meta"))).toBeNull();
    expect(encodeKeyboardEvent(ev("Dead"))).toBeNull();
    expect(encodeKeyboardEvent(ev("CapsLock"))).toBeNull();
    expect(encodeKeyboardEvent(ev("Unidentified"))).toBeNull();
  });

  test("unknown named key → null (no encoding)", () => {
    // "ContextMenu" is a valid browser-named key but has no terminal
    // mapping in v1.
    expect(encodeKeyboardEvent(ev("ContextMenu"))).toBeNull();
    // F13+ likewise.
    expect(encodeKeyboardEvent(ev("F13"))).toBeNull();
  });
});

describe("encodeKeyboardEvent — preventDefault flag", () => {
  test("Every produced encoding requests preventDefault=true", () => {
    // The contract is uniform: if we hand back bytes, the caller must
    // suppress the browser default to avoid double-handling. Spot-check
    // a handful of representative branches.
    const samples = [
      encodeKeyboardEvent(ev("a")),
      encodeKeyboardEvent(ev("Enter")),
      encodeKeyboardEvent(ev("ArrowUp")),
      encodeKeyboardEvent(ev("c", { ctrlKey: true })),
      encodeKeyboardEvent(ev("a", { altKey: true })),
      encodeKeyboardEvent(ev("F5")),
    ];
    for (const r of samples) {
      expect(r).not.toBeNull();
      expect(r!.preventDefault).toBe(true);
    }
  });
});
