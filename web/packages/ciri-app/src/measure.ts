// Cell-size measurement.
//
// The web pane grid is built from `<div class="ciri-row">` rows of
// `<span>`s in a fixed monospace font; the server needs `Resize` with
// concrete `width`, `height`, `cellWidth`, and `cellHeight` so its
// own row/col reflow matches the actual layout. We measure by mounting
// a hidden probe with the same CSS style as the renderer's wrapper,
// reading its bounding box, and dividing by the known glyph count.
//
// The measurement is approximate (sub-pixel rendering, font fallback,
// line-height rounding all leak in) but it's the same approach xterm.js
// uses and the server's PTY tolerates an off-by-pixel here without
// causing visible misalignment.

export interface CellSize {
  /// Width of a single monospace cell, in CSS pixels.
  cellWidth: number;
  /// Height of a single monospace cell, in CSS pixels.
  cellHeight: number;
}

export interface MeasureOptions {
  /// CSS font-family string. Must match what the renderer uses or
  /// the measurement under-counts kerning / advance-width.
  fontFamily: string;
  /// CSS font-size string, e.g. "14px".
  fontSize: string;
  /// CSS line-height (number or string). Defaults to 1.2 — same as
  /// `PaneRenderer`'s wrapper.
  lineHeight?: string | number;
  /// Number of glyphs to average over. Larger samples smooth out
  /// sub-pixel rounding from getBoundingClientRect. Default 100.
  sampleSize?: number;
  /// The DOM document to mount the probe under. Defaults to the
  /// ambient `document` — useful for SSR / multi-window contexts to
  /// pass an explicit doc reference.
  document?: Document;
}

/// Measure cell size in CSS pixels. The probe is mounted under
/// `document.body`, sized off-screen so it never flashes, and removed
/// before this function returns. Throws if `document` isn't available
/// (a non-browser host) or if the probe's bounding box has zero size
/// (font hasn't loaded yet — caller should retry once `document.fonts.ready`
/// resolves).
export function measureCellSize(opts: MeasureOptions): CellSize {
  const doc = opts.document ?? globalThis.document;
  if (!doc || !doc.body) {
    throw new Error(
      "measureCellSize: no document.body available — run after the DOM is mounted, or pass `opts.document`",
    );
  }
  const sample = Math.max(1, Math.floor(opts.sampleSize ?? 100));

  const probe = doc.createElement("div");
  probe.style.position = "absolute";
  // Keep the probe off-screen so it doesn't visibly flash on first
  // mount. `visibility: hidden` would also work but loses
  // `getBoundingClientRect` precision on some engines, hence the
  // negative-position trick.
  probe.style.top = "-9999px";
  probe.style.left = "-9999px";
  probe.style.fontFamily = opts.fontFamily;
  probe.style.fontSize = opts.fontSize;
  probe.style.lineHeight =
    typeof opts.lineHeight === "number"
      ? String(opts.lineHeight)
      : (opts.lineHeight ?? "1.2");
  probe.style.whiteSpace = "pre";
  // Single row of `sample` glyphs. We use a mid-Latin character
  // ("M") because:
  //   * it's wider than digits / lowercase, giving conservative bounds
  //   * it's an ASCII char so font fallback can't accidentally
  //     substitute a wider glyph from a different family
  //   * xterm.js does the same.
  probe.textContent = "M".repeat(sample);

  doc.body.appendChild(probe);
  let cellWidth = 0;
  let cellHeight = 0;
  try {
    const rect = probe.getBoundingClientRect();
    cellWidth = rect.width / sample;
    cellHeight = rect.height;
  } finally {
    probe.remove();
  }

  if (!Number.isFinite(cellWidth) || !Number.isFinite(cellHeight)) {
    throw new Error(
      `measureCellSize: probe produced non-finite size (cellWidth=${cellWidth}, cellHeight=${cellHeight})`,
    );
  }
  if (cellWidth <= 0 || cellHeight <= 0) {
    throw new Error(
      `measureCellSize: probe has zero size (cellWidth=${cellWidth}, cellHeight=${cellHeight}); the font likely hasn't loaded yet`,
    );
  }
  return { cellWidth, cellHeight };
}

/// Compute the cols/rows that fit inside a container of `widthPx` ×
/// `heightPx` given a measured `cellSize`. Both dims are rounded down
/// (the server stays inside the container) and clamped to >= 1.
export function cellsForViewport(
  widthPx: number,
  heightPx: number,
  cellSize: CellSize,
): { cols: number; rows: number } {
  const cols = Math.max(1, Math.floor(widthPx / cellSize.cellWidth));
  const rows = Math.max(1, Math.floor(heightPx / cellSize.cellHeight));
  return { cols, rows };
}
