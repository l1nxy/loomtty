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
  COLOR_INDEXED,
  COLOR_NAMED,
  COLOR_RGB,
  FLAG_BOLD,
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
  NAMED_DIM_BLUE,
  NAMED_DIM_CYAN,
  NAMED_DIM_FOREGROUND,
  NAMED_DIM_GREEN,
  NAMED_DIM_MAGENTA,
  NAMED_DIM_RED,
  NAMED_DIM_WHITE,
  NAMED_DIM_YELLOW,
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
}

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

/// Resolve a PackedColor (and the bold/dim adjustment flags) to a CSS
/// color string against the active theme. Returns the theme's
/// foreground/background as the ultimate fallback if the named slot
/// has been left unset.
///
/// `flags` is the cell's full flag word; the resolver only inspects
/// BOLD and DIM. INVERSE is handled by the caller (the run grouper
/// swaps fg/bg before calling here), not by this function.
export function resolveColor(
  c: PackedColor,
  isForeground: boolean,
  flags: number,
  theme: Theme,
): ColorString {
  switch (c.kind) {
    case "rgb":
      return rgbToHex(c.r, c.g, c.b);
    case "indexed":
      return resolveIndexed(c.index, theme);
    case "named":
      return resolveNamed(c.index, isForeground, flags, theme);
  }
}

function resolveNamed(
  idx: number,
  isForeground: boolean,
  flags: number,
  theme: Theme,
): ColorString {
  if (idx === NAMED_FOREGROUND) {
    if ((flags & FLAG_BOLD) !== 0) {
      // Bold's traditional XTerm rendering brightens the foreground;
      // a theme that explicitly sets BRIGHT_FOREGROUND wins, otherwise
      // we leave it on the default fg.
      const bright = theme.named[NAMED_BRIGHT_FOREGROUND];
      if (bright !== null && bright !== undefined) return bright;
    }
    if ((flags & FLAG_DIM) !== 0) {
      const dim = theme.named[NAMED_DIM_FOREGROUND];
      if (dim !== null && dim !== undefined) return dim;
    }
    return theme.named[NAMED_FOREGROUND] ?? theme.foreground;
  }
  if (idx === NAMED_BACKGROUND) {
    return theme.named[NAMED_BACKGROUND] ?? theme.background;
  }
  // Bold on a basic-ANSI slot promotes to the bright variant (8..15).
  // This matches xterm/vte behavior — without it, applications using
  // SGR 1 on red look identical to plain red.
  if (isForeground && (flags & FLAG_BOLD) !== 0 && idx <= NAMED_WHITE) {
    const brightIdx = idx + (NAMED_BRIGHT_BLACK - NAMED_BLACK);
    const bright = theme.named[brightIdx];
    if (bright !== null && bright !== undefined) return bright;
  }
  // Dim on a basic-ANSI slot promotes to the dim variant (19..26).
  if (isForeground && (flags & FLAG_DIM) !== 0 && idx <= NAMED_WHITE) {
    const dimIdx = idx + (NAMED_DIM_BLACK - NAMED_BLACK);
    const dim = theme.named[dimIdx];
    if (dim !== null && dim !== undefined) return dim;
  }
  const slot = theme.named[idx];
  if (slot !== null && slot !== undefined) return slot;
  // Unset named slot (e.g. NAMED_DIM_* on a minimal theme) falls
  // through to the default fg/bg so the renderer never returns an
  // empty string.
  return isForeground ? theme.foreground : theme.background;
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
