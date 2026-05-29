// Public entry point of `@ciri/app`.
//
// Wires a `CiriClient` (transport + codec) to per-pane `PaneGrid` /
// `PaneRenderer` instances (from `@ciri/dom`) and lays them out
// according to the server's `LayoutState`. Owns the input pipeline
// (KeyboardEvent → xterm bytes, click to focus, wheel to scroll
// scrollback) and the resize pipeline (ResizeObserver → measure cell
// px → send `Resize`).
//
// The barrel re-exports the public surface; concrete classes live in
// the individual files.

export { CiriApp, type CiriAppOptions } from "./app.js";
export { LayoutManager, type LayoutManagerOptions } from "./layout.js";
export {
  encodeKeyboardEvent,
  type KeyEncoding,
  type KeyEncoderOptions,
} from "./input.js";
export {
  cellsForViewport,
  measureCellSize,
  type CellSize,
  type MeasureOptions,
} from "./measure.js";
