// Public entry point of `@ciri/dom`.
//
// Consumes typed `CellDelta` / `FullPaneSync` values from `@ciri/codec`
// and renders them into a DOM tree under a caller-supplied root.
// The grid model + run grouper + renderer are split across three
// modules; this barrel re-exports the public surface in one place.

export {
  DEFAULT_THEME,
  resolveColor,
  type ColorString,
  type Theme,
} from "./theme.js";
export {
  GridShapeError,
  PaneGrid,
  type DirtyRows,
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
