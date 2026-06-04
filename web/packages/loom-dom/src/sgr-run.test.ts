// Per-row SGR run partitioner tests.

import { describe, expect, test } from "vitest";
import {
  DEFAULT_BG,
  DEFAULT_FG,
  FLAG_BOLD,
  FLAG_DIM,
  FLAG_HIDDEN,
  FLAG_INVERSE,
  FLAG_ITALIC,
  FLAG_UNDERLINE,
  FLAG_UNDERLINE_CURLY,
  FLAG_WIDE_CHAR,
  FLAG_WIDE_CHAR_SPACER,
  NAMED_RED,
  type PackedCell,
  type PackedColor,
} from "@loom/codec";
import { DEFAULT_THEME } from "./theme.js";
import { rowRuns, type RowExtras } from "./sgr-run.js";

const RED: PackedColor = { kind: "named", index: NAMED_RED };

function cell(
  ch: string,
  fg = DEFAULT_FG,
  bg = DEFAULT_BG,
  flags = 0,
): PackedCell {
  return { ch, fg, bg, flags };
}

function emptyExtras(): RowExtras {
  return {
    graphemeExtras: new Map(),
    cellLinks: new Map(),
    linkMap: new Map(),
  };
}

describe("rowRuns", () => {
  test("groups adjacent cells with identical style into one run", () => {
    const cells = [cell("H"), cell("i")];
    const runs = rowRuns(cells, 0, emptyExtras(), DEFAULT_THEME);
    expect(runs).toHaveLength(1);
    expect(runs[0]!.text).toBe("Hi");
  });

  test("style change breaks the run", () => {
    const cells = [cell("a"), cell("b", RED, DEFAULT_BG, 0)];
    const runs = rowRuns(cells, 0, emptyExtras(), DEFAULT_THEME);
    expect(runs).toHaveLength(2);
    expect(runs[0]!.text).toBe("a");
    expect(runs[1]!.text).toBe("b");
  });

  test("bold/italic/underline are surfaced as bool fields", () => {
    const cells = [
      cell("B", DEFAULT_FG, DEFAULT_BG, FLAG_BOLD),
      cell("I", DEFAULT_FG, DEFAULT_BG, FLAG_ITALIC),
      cell("U", DEFAULT_FG, DEFAULT_BG, FLAG_UNDERLINE),
    ];
    const runs = rowRuns(cells, 0, emptyExtras(), DEFAULT_THEME);
    expect(runs).toHaveLength(3);
    expect(runs[0]!.bold).toBe(true);
    expect(runs[1]!.italic).toBe(true);
    expect(runs[2]!.underline).toBe("single");
  });

  test("curly underline style is preserved", () => {
    const cells = [
      cell("X", DEFAULT_FG, DEFAULT_BG, FLAG_UNDERLINE | FLAG_UNDERLINE_CURLY),
    ];
    const runs = rowRuns(cells, 0, emptyExtras(), DEFAULT_THEME);
    expect(runs[0]!.underline).toBe("curly");
  });

  test("inverse swaps fg/bg after resolution", () => {
    const cells = [cell("R", RED, DEFAULT_BG, FLAG_INVERSE)];
    const runs = rowRuns(cells, 0, emptyExtras(), DEFAULT_THEME);
    // Inverted: the run's fg is what the original bg resolved to,
    // and vice-versa.
    expect(runs[0]!.fg).toBe(DEFAULT_THEME.background);
    expect(runs[0]!.bg).toBe(DEFAULT_THEME.named[NAMED_RED]);
  });

  test("dim + inverse: dim applies to the original fg, then swap", () => {
    // Rust's order is dim-then-inverse. So a cell with fg=red,
    // bg=default, DIM+INVERSE should render with:
    //   * run.fg = original bg → theme.background (NOT dimmed)
    //   * run.bg = dimmed original fg → dimify(red)
    // Round-4 codex regression: the previous arg-swap-before-resolve
    // path would have dimmed the wrong color.
    const baseRed = DEFAULT_THEME.named[NAMED_RED]!;
    const r = Math.round(Number.parseInt(baseRed.slice(1, 3), 16) * 0.67);
    const g = Math.round(Number.parseInt(baseRed.slice(3, 5), 16) * 0.67);
    const b = Math.round(Number.parseInt(baseRed.slice(5, 7), 16) * 0.67);
    const dimRed = `#${[r, g, b].map((n) => n.toString(16).padStart(2, "0")).join("")}`;
    const cells = [cell("R", RED, DEFAULT_BG, FLAG_INVERSE | FLAG_DIM)];
    const runs = rowRuns(cells, 0, emptyExtras(), DEFAULT_THEME);
    expect(runs[0]!.fg).toBe(DEFAULT_THEME.background);
    expect(runs[0]!.bg).toBe(dimRed);
  });

  test("dim alone: original fg is dimmed, bg unchanged", () => {
    const cells = [cell("R", RED, DEFAULT_BG, FLAG_DIM)];
    const runs = rowRuns(cells, 0, emptyExtras(), DEFAULT_THEME);
    const baseRed = DEFAULT_THEME.named[NAMED_RED]!;
    expect(runs[0]!.fg).not.toBe(baseRed); // dimmed
    expect(runs[0]!.bg).toBe(DEFAULT_THEME.background);
  });

  test("hidden flag renders a space in the text", () => {
    const cells = [
      cell("a", DEFAULT_FG, DEFAULT_BG, FLAG_HIDDEN),
      cell("b", DEFAULT_FG, DEFAULT_BG, FLAG_HIDDEN),
    ];
    const runs = rowRuns(cells, 0, emptyExtras(), DEFAULT_THEME);
    expect(runs).toHaveLength(1);
    expect(runs[0]!.text).toBe("  ");
  });

  test("wide-char spacer is skipped from run text", () => {
    const cells = [
      cell("中", DEFAULT_FG, DEFAULT_BG, FLAG_WIDE_CHAR),
      cell(" ", DEFAULT_FG, DEFAULT_BG, FLAG_WIDE_CHAR_SPACER),
      cell("a"),
    ];
    const runs = rowRuns(cells, 0, emptyExtras(), DEFAULT_THEME);
    // The run grouper drops the spacer cell entirely. FLAG_WIDE_CHAR
    // on the lead cell has no visual effect — the run key intentionally
    // ignores positional bits — so the wide and following `a` cell
    // merge into one run.
    expect(runs).toHaveLength(1);
    expect(runs[0]!.text).toBe("中a");
    // The wide glyph is isolated into its own segment so the renderer
    // can pin it to a two-column box; the trailing narrow `a` is a
    // separate narrow segment.
    expect(runs[0]!.segments).toEqual([
      { text: "中", wide: true },
      { text: "a", wide: false },
    ]);
  });

  test("adjacent wide chars each get their own segment", () => {
    const cells = [
      cell("中", DEFAULT_FG, DEFAULT_BG, FLAG_WIDE_CHAR),
      cell(" ", DEFAULT_FG, DEFAULT_BG, FLAG_WIDE_CHAR_SPACER),
      cell("文", DEFAULT_FG, DEFAULT_BG, FLAG_WIDE_CHAR),
      cell(" ", DEFAULT_FG, DEFAULT_BG, FLAG_WIDE_CHAR_SPACER),
    ];
    const runs = rowRuns(cells, 0, emptyExtras(), DEFAULT_THEME);
    expect(runs).toHaveLength(1);
    expect(runs[0]!.segments).toEqual([
      { text: "中", wide: true },
      { text: "文", wide: true },
    ]);
  });

  test("narrow glyphs coalesce into one segment", () => {
    const cells = [cell("a"), cell("b"), cell("c")];
    const runs = rowRuns(cells, 0, emptyExtras(), DEFAULT_THEME);
    expect(runs[0]!.segments).toEqual([{ text: "abc", wide: false }]);
  });

  test("wide-char run breaks when a style flag differs after the spacer", () => {
    const cells = [
      cell("中", DEFAULT_FG, DEFAULT_BG, FLAG_WIDE_CHAR),
      cell(" ", DEFAULT_FG, DEFAULT_BG, FLAG_WIDE_CHAR_SPACER),
      cell("a", DEFAULT_FG, DEFAULT_BG, FLAG_BOLD),
    ];
    const runs = rowRuns(cells, 0, emptyExtras(), DEFAULT_THEME);
    expect(runs).toHaveLength(2);
    expect(runs[0]!.text).toBe("中");
    expect(runs[1]!.text).toBe("a");
    expect(runs[1]!.bold).toBe(true);
  });

  test("grapheme extras splice into cell text by global index", () => {
    const cells = [cell("a"), cell("\u{1F600}"), cell("b")];
    const extras: RowExtras = {
      graphemeExtras: new Map([[101, "‍\u{1F4BB}"]]),
      cellLinks: new Map(),
      linkMap: new Map(),
    };
    const runs = rowRuns(cells, 100, extras, DEFAULT_THEME);
    expect(runs).toHaveLength(1);
    // Cell @ globalIdx 101 carries the ZWJ + laptop extras.
    expect(runs[0]!.text).toBe(
      "a\u{1F600}‍\u{1F4BB}b",
    );
  });

  test("hyperlink groups cells into a linked run", () => {
    const cells = [cell("c"), cell("l"), cell("k")];
    const extras: RowExtras = {
      graphemeExtras: new Map(),
      cellLinks: new Map([
        [10, 1],
        [11, 1],
      ]),
      linkMap: new Map([[1, "https://example.com"]]),
    };
    const runs = rowRuns(cells, 10, extras, DEFAULT_THEME);
    // First two cells share linkUri='https://example.com', third has
    // no link → break.
    expect(runs).toHaveLength(2);
    expect(runs[0]!.text).toBe("cl");
    expect(runs[0]!.linkUri).toBe("https://example.com");
    expect(runs[1]!.text).toBe("k");
    expect(runs[1]!.linkUri).toBeNull();
  });

  test("missing linkMap entry treats cell as unlinked rather than crashing", () => {
    const cells = [cell("a"), cell("b")];
    const extras: RowExtras = {
      graphemeExtras: new Map(),
      cellLinks: new Map([[0, 99]]), // link_id 99 has no linkMap entry
      linkMap: new Map(),
    };
    const runs = rowRuns(cells, 0, extras, DEFAULT_THEME);
    expect(runs).toHaveLength(1);
    expect(runs[0]!.linkUri).toBeNull();
  });
});
