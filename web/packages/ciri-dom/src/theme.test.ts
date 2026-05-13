// Spot-checks for the PackedColor resolver. Exercises:
//   - rgb → hex passthrough
//   - named slot lookup
//   - named-foreground fallback to theme.foreground
//   - bold-promotion of basic-ANSI to bright
//   - dim-promotion of basic-ANSI to dim
//   - indexed 6×6×6 cube derivation
//   - indexed grayscale ramp derivation

import { describe, expect, test } from "vitest";
import {
  FLAG_BOLD,
  FLAG_DIM,
  NAMED_BACKGROUND,
  NAMED_BRIGHT_RED,
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

  test("named red with bold promotes to bright-red", () => {
    expect(resolveColor(NAMED_RED_C, true, FLAG_BOLD, DEFAULT_THEME)).toBe(
      DEFAULT_THEME.named[NAMED_BRIGHT_RED],
    );
  });

  test("background does not bold-promote", () => {
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

  test("dim flag falls through to theme.foreground when DIM_FOREGROUND unset", () => {
    expect(resolveColor(NAMED_FG, true, FLAG_DIM, DEFAULT_THEME)).toBe(
      DEFAULT_THEME.foreground,
    );
  });

  test("custom theme with DIM_FOREGROUND set: dim flag uses it", () => {
    const theme: Theme = {
      ...DEFAULT_THEME,
      named: DEFAULT_THEME.named.slice(),
    };
    theme.named[28 /* NAMED_DIM_FOREGROUND */] = "#777777";
    expect(resolveColor(NAMED_FG, true, FLAG_DIM, theme)).toBe("#777777");
  });
});
