// One-row SGR-run partitioner. Walks the cells of a single row,
// resolves each cell's foreground/background through the theme, picks
// up grapheme extras + hyperlinks by global index, and groups adjacent
// cells with identical style into one `SgrRun` — the unit the renderer
// emits as a single `<span>`.
//
// Wide-char spacers (FLAG_WIDE_CHAR_SPACER) are dropped from the run
// text; the preceding wide cell carries the visible glyph and the
// spacer would just double-count column width. Hidden cells
// (FLAG_HIDDEN) are emitted as a space — they still consume a column
// but render no glyph.

import {
  FLAG_BOLD,
  FLAG_HIDDEN,
  FLAG_INVERSE,
  FLAG_ITALIC,
  FLAG_STRIKEOUT,
  FLAG_UNDERLINE,
  FLAG_UNDERLINE_STYLE_MASK,
  FLAG_UNDERLINE_CURLY,
  FLAG_UNDERLINE_DASHED,
  FLAG_UNDERLINE_DOTTED,
  FLAG_UNDERLINE_DOUBLE,
  FLAG_WIDE_CHAR_SPACER,
  type PackedCell,
} from "@ciri/codec";
import {
  type ColorString,
  type Theme,
  resolveColor,
} from "./theme.js";

export type UnderlineStyle =
  | "none"
  | "single"
  | "double"
  | "curly"
  | "dotted"
  | "dashed";

/// A contiguous span of cells with the same style. The renderer emits
/// one `<span>` (or `<a>` if `linkUri !== null`) per run.
export interface SgrRun {
  /// Already-resolved cell glyphs concatenated in row order (with
  /// grapheme extras applied, wide-char spacers omitted).
  text: string;
  /// Effective foreground after INVERSE swap + theme resolution.
  fg: ColorString;
  /// Effective background after INVERSE swap + theme resolution.
  bg: ColorString;
  bold: boolean;
  italic: boolean;
  /// Either `"none"` (no underline) or one of the five SGR styles.
  underline: UnderlineStyle;
  strikeout: boolean;
  /// OSC 8 hyperlink URI, or null when this run has no link.
  linkUri: string | null;
}

/// Lookups for sparse per-cell extras. Indices are over the
/// concatenated [scrollback..., viewport] cell stream — `rowRuns` adds
/// `rowGlobalStart` to a per-row index when probing.
export interface RowExtras {
  graphemeExtras: Map<number, string>;
  cellLinks: Map<number, number>;
  linkMap: Map<number, string>;
}

/// Partition one row of cells into runs.
///
/// - `cells` is `row * cols .. (row+1) * cols` from the grid.
/// - `rowGlobalStart` is the global cell index of `cells[0]` — used
///   to probe `graphemeExtras` and `cellLinks`.
/// - `theme` resolves named/indexed colors to CSS strings.
export function rowRuns(
  cells: readonly PackedCell[],
  rowGlobalStart: number,
  extras: RowExtras,
  theme: Theme,
): SgrRun[] {
  const runs: SgrRun[] = [];
  let current: SgrRun | null = null;
  let currentKey = "";

  for (let i = 0; i < cells.length; i += 1) {
    const cell = cells[i]!;
    if ((cell.flags & FLAG_WIDE_CHAR_SPACER) !== 0) {
      // The preceding cell's wide-char glyph already occupies this
      // column visually. Skipping the spacer keeps the run text
      // length in step with how many *visible* columns the run paints.
      continue;
    }
    const globalIdx = rowGlobalStart + i;
    const text = readCellText(cell, globalIdx, extras.graphemeExtras);
    const linkUri = readCellLink(globalIdx, extras.cellLinks, extras.linkMap);

    // Resolve fg / bg in their original roles first so DIM (applied by
    // `resolveColor` when `isForeground` is true) lands on the cell's
    // *original* foreground, then swap for INVERSE. This mirrors
    // Rust's `apply_color_modifiers` order — `dim → inverse` — so a
    // DIM+INVERSE cell renders the originally-fg color (now dimmed)
    // as the background, not the originally-bg color.
    let fg = resolveColor(cell.fg, /* isForeground */ true, cell.flags, theme);
    let bg = resolveColor(cell.bg, /* isForeground */ false, cell.flags, theme);
    if ((cell.flags & FLAG_INVERSE) !== 0) {
      [fg, bg] = [bg, fg];
    }

    const bold = (cell.flags & FLAG_BOLD) !== 0;
    const italic = (cell.flags & FLAG_ITALIC) !== 0;
    const strikeout = (cell.flags & FLAG_STRIKEOUT) !== 0;
    const underline = underlineStyleFromFlags(cell.flags);

    // The group key encodes everything the renderer puts into the
    // span's style. Two adjacent cells with the same key merge into
    // one run; differing keys break.
    const key = `${fg}|${bg}|${bold ? 1 : 0}|${italic ? 1 : 0}|${underline}|${strikeout ? 1 : 0}|${linkUri ?? ""}`;

    if (current !== null && key === currentKey) {
      current.text += text;
      continue;
    }
    current = {
      text,
      fg,
      bg,
      bold,
      italic,
      underline,
      strikeout,
      linkUri,
    };
    currentKey = key;
    runs.push(current);
  }

  return runs;
}

function readCellText(
  cell: PackedCell,
  globalIdx: number,
  graphemeExtras: Map<number, string>,
): string {
  if ((cell.flags & FLAG_HIDDEN) !== 0) {
    // Hidden text (SGR 8) still occupies its column — emit a space so
    // alignment doesn't drift, but render nothing the user can read.
    return " ";
  }
  const extra = graphemeExtras.get(globalIdx);
  if (extra === undefined) return cell.ch;
  return cell.ch + extra;
}

function readCellLink(
  globalIdx: number,
  cellLinks: Map<number, number>,
  linkMap: Map<number, string>,
): string | null {
  const linkId = cellLinks.get(globalIdx);
  if (linkId === undefined) return null;
  // A `cellLinks` entry for a missing `linkMap` row is wire corruption
  // — the encoder always emits both halves. Treat it as "no link"
  // rather than crashing the render path.
  return linkMap.get(linkId) ?? null;
}

function underlineStyleFromFlags(flags: number): UnderlineStyle {
  if ((flags & FLAG_UNDERLINE) === 0) return "none";
  switch (flags & FLAG_UNDERLINE_STYLE_MASK) {
    case 0:
      return "single";
    case FLAG_UNDERLINE_DOUBLE:
      return "double";
    case FLAG_UNDERLINE_CURLY:
      return "curly";
    case FLAG_UNDERLINE_DOTTED:
      return "dotted";
    case FLAG_UNDERLINE_DASHED:
      return "dashed";
    default:
      // Unknown style bits — fall back to plain single so the user
      // still sees an underline rather than nothing.
      return "single";
  }
}
