# UI Layer Plan

## Target Architecture

The UI layer should have one source of truth for structure, layout, paint, and hit testing.

1. App code builds a `loom-ui` element tree from immutable UI state.
2. `loom-ui` runs one Taffy layout pass.
3. The same pass emits paint primitives and a `LayoutSnapshot`.
4. Pointer hit testing uses the snapshot, walking by layer and paint order.
5. Event dispatch targets element IDs or app-provided actions, then state changes rebuild the tree.

This avoids the current split where chrome paints through `loom-ui` but hit testing is still partially hand-computed in `crates/loom/src/app/ui`.

## Migration Steps

1. Add `LayoutSnapshot` to `loom-ui`.
   Status: started in this branch. The paint pass now can return rendered primitives plus layout records and topmost hit testing.

2. Add stable element/action identity.
   Introduce element IDs or action tags on interactive elements so app code can map a hit node to `UiAction` without separate geometry code.

3. Replace app-side modal hit tests first.
   Migrate command palette, context menu, and paste dialog clicks/hover to snapshot hit regions. These are self-contained and should not affect terminal pane hit testing.

4. Replace chrome bar hit tests.
   Move top bar, integrated pane tabs, side tab bar, and hints bar to snapshot-based dispatch. Keep `UiRect/Border/Linear` only as an internal layout compatibility layer until every caller is moved.

5. Remove duplicated geometry.
   Delete obsolete hand-written hit rectangles and any layout helpers no longer needed by rendering or dispatch.

6. Keep terminal rendering separate.
   Terminal grid backgrounds, selection, links, focus rings, scrollbars, images, and overview pane thumbnails remain renderer-domain primitives unless they become real widgets.
   Pane corner rounding follows that boundary: `appearance.pane_corner_radius` is carried through renderer pane ranges and SDF focus-ring rects, not through `loom-ui` chrome widgets.

## Non-Goals

- Do not force terminal cell rendering into `loom-ui`.
- Do not remove `loom_ui_bridge` until the renderer natively accepts `loom-ui::Scene` and text shaping is owned by the renderer/UI boundary.
- Do not rewrite all UI components in one change; migrate hit testing component-by-component with tests.
