// Public entry point of `@loom/app`.
//
// Wires a `LoomClient` (transport + codec) to per-pane `PaneGrid` /
// `PaneRenderer` instances (from `@loom/dom`) and lays them out
// according to the server's `LayoutState`. Owns the input pipeline
// (KeyboardEvent → xterm bytes, click to focus, wheel to scroll
// scrollback) and the resize pipeline (ResizeObserver → measure cell
// px → send `Resize`).
//
// The barrel re-exports the public surface; concrete classes live in
// the individual files.

export { LoomApp, type LoomAppOptions } from "./app.js";
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
