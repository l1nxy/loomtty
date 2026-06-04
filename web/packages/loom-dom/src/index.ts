// Public entry point of `@loom/dom`.
//
// Consumes typed `CellDelta` / `FullPaneSync` values from `@loom/codec`
// and renders them into a DOM tree under a caller-supplied root.
// The grid model + run grouper + renderer are split across three
// modules; this barrel re-exports the public surface in one place.

export {
  DEFAULT_SELECTION_BACKGROUND,
  DEFAULT_THEME,
  resolveColor,
  type ColorString,
  type Theme,
} from "./theme.js";
export {
  DEFAULT_MAX_SCROLLBACK_ROWS,
  GridShapeError,
  PaneGrid,
  normalizeSelectionEnds,
  selectionRangeEquals,
  type DirtyRows,
  type PaneGridOptions,
  type SearchMatch,
  type SelectionAnchor,
  type SelectionRange,
} from "./grid.js";
export {
  rowRuns,
  type RowExtras,
  type SgrRun,
  type UnderlineStyle,
} from "./sgr-run.js";
export {
  PaneRenderer,
  type RendererOptions,
} from "./renderer.js";
