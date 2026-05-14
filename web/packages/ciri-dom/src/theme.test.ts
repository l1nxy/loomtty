// Spot-checks for the PackedColor resolver. Exercises:
//   - rgb → hex passthrough
//   - named slot lookup
//   - named-foreground fallback to theme.foreground
//   - BOLD does NOT change color (font weight only — matches Rust)
//   - DIM multiplies fg channels by 0.67 (matches Rust)
//   - indexed 6×6×6 cube derivation
//   - indexed grayscale ramp derivation

import { describe, expect, test } from "vitest";
import {
  FLAG_BOLD,
  FLAG_DIM,
  NAMED_BACKGROUND,
  NAMED_FOREGROUND,
  NAMED_RED,
  type PackedColor,
} from "@ciri/codec";
import { DEFAULT_THEME, resolveColor, type Theme } from "./theme.js";

const RGB_ORANGE: PackedColor = { kind: "rgb", r: 0xff, g: 0x88, b: 0x00 };
const NAMED_FG: PackedColor = { kind: "named", index: NAMED_FOREGROUND };
const NAMED_BG: PackedColor = { kind: "named", index: NAMED_BACKGROUND };
const NAMED_RED_C: PackedColor = { kind: "named", index: NAMED_RED };

describe("resolveColor", () => {
  test("rgb passes through as #rrggbb", () => {
    expect(resolveColor(RGB_ORANGE, true, 0, DEFAULT_THEME)).toBe("#ff8800");
  });

  test("named foreground returns theme foreground", () => {
    expect(resolveColor(NAMED_FG, true, 0, DEFAULT_THEME)).toBe(
      DEFAULT_THEME.foreground,
    );
  });

  test("named background returns theme background", () => {
    expect(resolveColor(NAMED_BG, false, 0, DEFAULT_THEME)).toBe(
      DEFAULT_THEME.background,
    );
  });

  test("named red without bold returns red slot", () => {
    expect(resolveColor(NAMED_RED_C, true, 0, DEFAULT_THEME)).toBe(
      DEFAULT_THEME.named[NAMED_RED],
    );
  });

  test("bold does NOT brighten color (Rust parity: font weight only)", () => {
    // Round-4 codex fix: previously this promoted red → bright-red,
    // which the native renderer explicitly stopped doing. SGR 1 +
    // foreground red should stay plain red.
    expect(resolveColor(NAMED_RED_C, true, FLAG_BOLD, DEFAULT_THEME)).toBe(
      DEFAULT_THEME.named[NAMED_RED],
    );
  });

  test("bold on background also leaves color unchanged", () => {
    expect(resolveColor(NAMED_RED_C, false, FLAG_BOLD, DEFAULT_THEME)).toBe(
      DEFAULT_THEME.named[NAMED_RED],
    );
  });

  test("indexed 0..15 aliases the ANSI palette", () => {
    const c0: PackedColor = { kind: "indexed", index: 0 };
    expect(resolveColor(c0, true, 0, DEFAULT_THEME)).toBe(
      DEFAULT_THEME.named[0],
    );
  });

  test("indexed cube: 16 → black", () => {
    const c16: PackedColor = { kind: "indexed", index: 16 };
    expect(resolveColor(c16, true, 0, DEFAULT_THEME)).toBe("#000000");
  });

  test("indexed cube: 196 → primary red", () => {
    // i = 196 - 16 = 180; r = 5, g = 0, b = 0 → CUBE_STEPS[5]=255 → #ff0000
    const c: PackedColor = { kind: "indexed", index: 196 };
    expect(resolveColor(c, true, 0, DEFAULT_THEME)).toBe("#ff0000");
  });

  test("indexed grayscale: 232 → dark gray (#080808)", () => {
    const c: PackedColor = { kind: "indexed", index: 232 };
    expect(resolveColor(c, true, 0, DEFAULT_THEME)).toBe("#080808");
  });

  test("indexed grayscale: 255 → near-white (#eeeeee)", () => {
    const c: PackedColor = { kind: "indexed", index: 255 };
    expect(resolveColor(c, true, 0, DEFAULT_THEME)).toBe("#eeeeee");
  });

  /// Helper: compute the expected `#rrggbb` for a hex×0.67 channel
  /// multiply, matching the resolver's rounding.
  function dim(hex: string): string {
    const r = Math.round(Number.parseInt(hex.slice(1, 3), 16) * 0.67);
    const g = Math.round(Number.parseInt(hex.slice(3, 5), 16) * 0.67);
    const b = Math.round(Number.parseInt(hex.slice(5, 7), 16) * 0.67);
    const h = (n: number) => n.toString(16).padStart(2, "0");
    return `#${h(r)}${h(g)}${h(b)}`;
  }

  test("DIM on named foreground multiplies channels by 0.67", () => {
    // Round-4 codex fix: previously this honored the NAMED_DIM_*
    // theme slots. Rust's apply_color_modifiers ignores those slots
    // and always multiplies the resolved foreground by 0.67.
    expect(resolveColor(NAMED_FG, true, FLAG_DIM, DEFAULT_THEME)).toBe(
      dim(DEFAULT_THEME.foreground),
    );
  });

  test("DIM on RGB foreground multiplies channels by 0.67", () => {
    // Round-4 codex fix: RGB used to short-circuit through `flags`
    // and ignore DIM entirely.
    expect(resolveColor(RGB_ORANGE, true, FLAG_DIM, DEFAULT_THEME)).toBe(
      dim("#ff8800"),
    );
  });

  test("DIM on indexed foreground multiplies channels by 0.67", () => {
    const c: PackedColor = { kind: "indexed", index: 196 }; // #ff0000 from cube
    expect(resolveColor(c, true, FLAG_DIM, DEFAULT_THEME)).toBe(
      dim("#ff0000"),
    );
  });

  test("DIM on named red multiplies channels by 0.67", () => {
    const baseRed = DEFAULT_THEME.named[NAMED_RED]!;
    expect(resolveColor(NAMED_RED_C, true, FLAG_DIM, DEFAULT_THEME)).toBe(
      dim(baseRed),
    );
  });

  test("DIM is ignored on background colors (Rust only dims fg)", () => {
    expect(resolveColor(NAMED_RED_C, false, FLAG_DIM, DEFAULT_THEME)).toBe(
      DEFAULT_THEME.named[NAMED_RED],
    );
  });

  test("DIM + BOLD combine: bold no-op, dim still applies", () => {
    expect(
      resolveColor(NAMED_RED_C, true, FLAG_DIM | FLAG_BOLD, DEFAULT_THEME),
    ).toBe(dim(DEFAULT_THEME.named[NAMED_RED]!));
  });

  test("custom theme: DIM_FOREGROUND slot no longer overrides the multiplier", () => {
    // Round-4 codex fix: a custom theme that set NAMED_DIM_FOREGROUND
    // used to take precedence, but Rust never honored it — the slot
    // is now unused for SGR DIM. The renderer always multiplies by
    // 0.67 regardless of theme.
    const theme: Theme = {
      ...DEFAULT_THEME,
      named: DEFAULT_THEME.named.slice(),
    };
    theme.named[28 /* NAMED_DIM_FOREGROUND */] = "#777777";
    expect(resolveColor(NAMED_FG, true, FLAG_DIM, theme)).toBe(
      dim(theme.foreground),
    );
  });

  test("non-hex theme color passes through DIM unchanged (parser doesn't crash)", () => {
    // Themes that stored an `rgb(...)` or CSS keyword (not supported
    // by our resolver's contract but worth guarding against) fall
    // through verbatim — better than returning NaN-tinged garbage.
    // Clear the NAMED_FOREGROUND slot so the resolver falls all the
    // way through to theme.foreground.
    const named = DEFAULT_THEME.named.slice();
    named[NAMED_FOREGROUND] = null;
    const theme: Theme = {
      ...DEFAULT_THEME,
      foreground: "rgb(255, 128, 0)",
      named,
    };
    expect(resolveColor(NAMED_FG, true, FLAG_DIM, theme)).toBe(
      "rgb(255, 128, 0)",
    );
  });
});
