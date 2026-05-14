// Color palette + PackedColor resolver. Mirrors the Rust renderer's
// color resolution rules: named colors come from the active theme,
// xterm-style indexed colors are derived from the canonical 6×6×6
// cube + 24-step grayscale, and RGB colors pass through verbatim.
//
// The theme shape is intentionally minimal — just the 29 named slots
// plus a default fg/bg pair. The renderer's responsibility is to pick
// a CSS color string per cell; everything beyond that (terminal
// settings UI, theme switcher, file-format loaders) lives in the app
// layer.

import {
  FLAG_DIM,
  FLAG_INVERSE,
  NAMED_BACKGROUND,
  NAMED_BLACK,
  NAMED_BLUE,
  NAMED_BRIGHT_BLACK,
  NAMED_BRIGHT_BLUE,
  NAMED_BRIGHT_CYAN,
  NAMED_BRIGHT_FOREGROUND,
  NAMED_BRIGHT_GREEN,
  NAMED_BRIGHT_MAGENTA,
  NAMED_BRIGHT_RED,
  NAMED_BRIGHT_WHITE,
  NAMED_BRIGHT_YELLOW,
  NAMED_CURSOR,
  NAMED_CYAN,
  NAMED_DIM_BLACK,
  NAMED_DIM_FOREGROUND,
  NAMED_DIM_WHITE,
  NAMED_FOREGROUND,
  NAMED_GREEN,
  NAMED_MAGENTA,
  NAMED_RED,
  NAMED_WHITE,
  NAMED_YELLOW,
  type PackedColor,
} from "@ciri/codec";

/// CSS color string for one cell, after PackedColor + theme resolution.
/// Always `#rrggbb` for RGB sources or whatever the theme stored for
/// named slots — no `rgb(...)` form, no alpha.
export type ColorString = string;

/// Theme — 29 named slots + the default fg/bg pair the renderer falls
/// back to when a cell carries NAMED_FOREGROUND / NAMED_BACKGROUND.
/// Slots are indexed by the `NAMED_*` constants from `@ciri/codec`.
export interface Theme {
  /// Page background: rendered behind the row container. The cell-level
  /// `bg` field stays separate so reverse video and explicit per-cell
  /// background colors still composite correctly.
  background: ColorString;
  /// Default foreground: cells that carry NAMED_FOREGROUND fall back to this.
  foreground: ColorString;
  /// 29-entry table indexed by the NAMED_* constants. A `null` entry
  /// means "no override; use the standard derivation" (e.g. the cursor
  /// slot defaults to fg, dim variants compute from their base).
  named: (ColorString | null)[];
  /// Background color of the mouse-drag selection overlay. Rendered as
  /// a semi-transparent rect on top of the cell grid; pick an `rgba`
  /// value (or any CSS color with alpha) so the underlying glyphs
  /// stay visible. Optional — defaults to a translucent blue when
  /// omitted.
  selectionBackground?: ColorString;
}

/// Default selection overlay color. Picked to mirror xterm.js / vte's
/// "selection blue" look while staying transparent enough that cell
/// text reads through at any contrast level.
export const DEFAULT_SELECTION_BACKGROUND = "rgba(64, 113, 196, 0.4)";

/// Default 16-color ANSI palette. Picked to match the Rust renderer's
/// fallback theme so a freshly-attached browser client paints the
/// same colors a TUI session would.
///
/// The 16 standard slots also seed indexed colors 0..15 — xterm's
/// convention is that indexed[0..16] aliases the ANSI palette. The
/// resolver below relies on this.
const DEFAULT_NAMED: (ColorString | null)[] = new Array(29).fill(null);
DEFAULT_NAMED[NAMED_BLACK] = "#000000";
DEFAULT_NAMED[NAMED_RED] = "#cc0000";
DEFAULT_NAMED[NAMED_GREEN] = "#4e9a06";
DEFAULT_NAMED[NAMED_YELLOW] = "#c4a000";
DEFAULT_NAMED[NAMED_BLUE] = "#3465a4";
DEFAULT_NAMED[NAMED_MAGENTA] = "#75507b";
DEFAULT_NAMED[NAMED_CYAN] = "#06989a";
DEFAULT_NAMED[NAMED_WHITE] = "#d3d7cf";
DEFAULT_NAMED[NAMED_BRIGHT_BLACK] = "#555753";
DEFAULT_NAMED[NAMED_BRIGHT_RED] = "#ef2929";
DEFAULT_NAMED[NAMED_BRIGHT_GREEN] = "#8ae234";
DEFAULT_NAMED[NAMED_BRIGHT_YELLOW] = "#fce94f";
DEFAULT_NAMED[NAMED_BRIGHT_BLUE] = "#729fcf";
DEFAULT_NAMED[NAMED_BRIGHT_MAGENTA] = "#ad7fa8";
DEFAULT_NAMED[NAMED_BRIGHT_CYAN] = "#34e2e2";
DEFAULT_NAMED[NAMED_BRIGHT_WHITE] = "#eeeeec";
DEFAULT_NAMED[NAMED_FOREGROUND] = "#d3d7cf";
DEFAULT_NAMED[NAMED_BACKGROUND] = "#000000";
DEFAULT_NAMED[NAMED_CURSOR] = "#d3d7cf";
// Dim and bright-foreground slots fall back to programmatic
// derivations in the resolver; we leave them null so a theme that
// wants to override picks them up without inheriting an arbitrary
// hard-coded value.

export const DEFAULT_THEME: Theme = {
  background: "#000000",
  foreground: "#d3d7cf",
  named: DEFAULT_NAMED,
};

// 6×6×6 cube step values per xterm convention. 0 → 0, 1..5 → 95 +
// 40*(n-1). Inlined so the resolver does no math at lookup time.
const CUBE_STEPS = [0, 95, 135, 175, 215, 255];

/// Resolve a PackedColor + cell flags to a CSS color string against
/// the active theme. Returns the theme's foreground/background as the
/// ultimate fallback if the named slot has been left unset.
///
/// SGR behavior (mirroring `apply_color_modifiers` in
/// `crates/ciri-render/src/terminal/cell.rs`):
///   * BOLD → no color change (font weight only).
///   * DIM  → each fg channel is multiplied by 0.67. Applies uniformly
///     to RGB, indexed, and named-foreground sources; the legacy
///     "promote to NAMED_DIM_* slot" path is gone (Rust never honored
///     it either, and themes that set those slots silently disagreed
///     with the native renderer).
///   * INVERSE → handled by the run grouper *after* this function
///     runs, because dim must apply to the original fg before the
///     swap (Rust's order: resolve → dim fg → swap if inverse).
export function resolveColor(
  c: PackedColor,
  isForeground: boolean,
  flags: number,
  theme: Theme,
): ColorString {
  const raw = resolveRaw(c, isForeground, theme);
  if (isForeground && (flags & FLAG_DIM) !== 0) {
    return dimifyHex(raw);
  }
  return raw;
}

function resolveRaw(
  c: PackedColor,
  isForeground: boolean,
  theme: Theme,
): ColorString {
  switch (c.kind) {
    case "rgb":
      return rgbToHex(c.r, c.g, c.b);
    case "indexed":
      return resolveIndexed(c.index, theme);
    case "named":
      return resolveNamed(c.index, isForeground, theme);
  }
}

function resolveNamed(
  idx: number,
  isForeground: boolean,
  theme: Theme,
): ColorString {
  // Mirror `ColorTable::resolve_packed` in
  // `crates/ciri-render/src/terminal/color.rs:57-80`.
  //
  // The wire's NAMED slot space carries 5 logical groups:
  //   * 0..=15           plain ANSI / Bright variants
  //   * 16 (FG), 27 (BRIGHT_FG)  → always theme foreground
  //   * 17 (BG)                  → always theme background
  //   * 18 (CURSOR)              → fg as a cell paint color (the
  //                                cursor overlay reads
  //                                `theme.named[NAMED_CURSOR]` directly)
  //   * 19..=26 (DIM_*)          → dim(named[idx - 19])
  //   * 28 (DIM_FOREGROUND)      → dim(theme foreground)
  // Anything outside these ranges falls back to foreground so we
  // never return an empty string.
  if (
    idx === NAMED_FOREGROUND ||
    idx === NAMED_BRIGHT_FOREGROUND ||
    idx === NAMED_CURSOR
  ) {
    return theme.named[NAMED_FOREGROUND] ?? theme.foreground;
  }
  if (idx === NAMED_BACKGROUND) {
    return theme.named[NAMED_BACKGROUND] ?? theme.background;
  }
  if (idx === NAMED_DIM_FOREGROUND) {
    const fg = theme.named[NAMED_FOREGROUND] ?? theme.foreground;
    return dimifyHex(fg);
  }
  if (idx >= NAMED_DIM_BLACK && idx <= NAMED_DIM_WHITE) {
    const base = theme.named[idx - NAMED_DIM_BLACK];
    if (base !== null && base !== undefined) return dimifyHex(base);
    return isForeground ? theme.foreground : theme.background;
  }
  const slot = theme.named[idx];
  if (slot !== null && slot !== undefined) return slot;
  return isForeground ? theme.foreground : theme.background;
}

/// Multiply each channel of a `#rrggbb` string by 0.67, the same
/// factor Rust's `apply_color_modifiers` uses for SGR DIM. Non-hex
/// inputs (e.g. a theme that stored an `rgb(...)` or CSS keyword)
/// fall through unchanged — we don't try to parse arbitrary CSS.
function dimifyHex(hex: ColorString): ColorString {
  if (hex.length !== 7 || hex.charCodeAt(0) !== 0x23 /* '#' */) return hex;
  const r = Number.parseInt(hex.slice(1, 3), 16);
  const g = Number.parseInt(hex.slice(3, 5), 16);
  const b = Number.parseInt(hex.slice(5, 7), 16);
  if (!Number.isFinite(r) || !Number.isFinite(g) || !Number.isFinite(b)) {
    return hex;
  }
  return rgbToHex(
    Math.round(r * 0.67),
    Math.round(g * 0.67),
    Math.round(b * 0.67),
  );
}

function resolveIndexed(idx: number, theme: Theme): ColorString {
  if (idx < 16) {
    // Aliased to the ANSI palette. The first 8 slots index NAMED_BLACK..NAMED_WHITE;
    // 8..15 index NAMED_BRIGHT_BLACK..NAMED_BRIGHT_WHITE. Both ranges live
    // contiguously in the `named` table.
    const slot = theme.named[idx];
    if (slot !== null && slot !== undefined) return slot;
    return theme.foreground;
  }
  if (idx < 232) {
    // 6×6×6 cube: i = (idx - 16); r = i / 36; g = (i / 6) % 6; b = i % 6.
    const i = idx - 16;
    const r = CUBE_STEPS[Math.floor(i / 36)]!;
    const g = CUBE_STEPS[Math.floor(i / 6) % 6]!;
    const b = CUBE_STEPS[i % 6]!;
    return rgbToHex(r, g, b);
  }
  // 24-step grayscale: idx 232..255 maps to a linear ramp.
  const v = 8 + (idx - 232) * 10;
  return rgbToHex(v, v, v);
}

function rgbToHex(r: number, g: number, b: number): ColorString {
  const h = (n: number) => n.toString(16).padStart(2, "0");
  return `#${h(r & 0xff)}${h(g & 0xff)}${h(b & 0xff)}`;
}

// Re-export so the SGR run grouper (which decides whether to swap
// fg/bg) doesn't have to import from the codec separately. Kept
// here so the renderer module's import list stays short.
export { FLAG_INVERSE };
