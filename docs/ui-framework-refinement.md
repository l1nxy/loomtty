# UI Framework Refinement Plan

This document records the audit of the `ciri` bin crate and `ciri-ui` library
performed against GPUI as a reference, and lays out the refactor sequence.

Sources are cited as `path/to/file.rs:LINE` for ciri (this repo) and
`zed/crates/gpui/src/file.rs:LINE` for GPUI (cloned at `/home/linxy/repo/zed/`).
This is not aspirational — every claim was checked at least once.

## Status of the existing `ui-layer-plan.md`

`docs/ui-layer-plan.md` covers the migration from hand-written hit rectangles
to a single Taffy + LayoutSnapshot pass. That migration is done — chrome,
modals, and tab bars all build `ciri-ui` element trees and dispatch through
`ui_hit_id`. This document picks up *after* that work and is concerned with
the framework's structural shape, not the painting pipeline integration.

---

## 1. Audit summary

### 1.1 What turned out to be **fine** (corrected from the initial impression)

- The `app/X.rs` ↔ `app/ui/X.rs` parallel files are not "mechanical MVC".
  Model state lives in `ciri-app::AppModel`, the view in `app/ui/X`, and the
  bin-crate `app/X` carries IO-bearing side effects (clipboard, network,
  `self.send`). The split is clean. Verified for palette, context_menu,
  paste_dialog.
- `build_hit_tree` vs `build_tree` (in `app/ui/tab_bar`, `app/ui/top_bar`)
  is intentional: a coarse hit zone tree separate from the visual tree gives
  click-friendlier UX. Not duplicate work.
- Render-cache invalidation is centralized at
  `crates/ciri/src/app/mod.rs:1274` (`invalidate_pane_cache`) and `:1280`
  (`clear_render_caches`). All call sites use the helpers.
- `app/ui/top_bar/` (mode/session_label/workspace/pane_tabs/mod) and
  `app/key_encode/` (kitty/legacy/mod) are textbook decomposition.
- `process_server_events` is 420 lines but is a clean batch-drain + 23-arm
  match. Cosmetic refactor, not design debt.
- `sync.rs` has 0 `unwrap`s in production code. All 34 are in tests.

### 1.2 What is real

- `App` has 31 fields mixing four concerns (`AppModel`, GPU resources, input
  ephemeral state, six `cached_*` fields): `crates/ciri/src/app/mod.rs:164`.
  The downstream symptom is `Self::` static helpers in `render.rs` taking
  five `&mut Vec` outputs because `&mut self` cannot coexist with a borrow
  of `self.render_bufs.X`.
- `render()` is 570 lines mixing per-pane view update, buffer assembly, GPU
  draw, and atlas/animation post-processing: `app/render.rs:1940-2509`.
  Cleanly splittable into four sub-methods.
- `tile_paint_config()` body is duplicated **byte-for-byte** between
  `app/render.rs:73-90` and `app/render.rs:1490-1507`. 18 lines, including
  identical comments. `build_tiles` should call `self.tile_paint_config()`.
- `SectionHeader` skip logic is duplicated three times: `app/action.rs:307`,
  `app/action.rs:323`, `app/mouse.rs:760`. Should live on
  `CommandPaletteState` as `move_selection(delta: i32)`.
- `image_atlas_entries.clear()` at `app/sync.rs:185` and `:288` is over-
  invalidation: the map is keyed `(pane_id, image_id)` so it should be
  filtered by pane, not nuked. Should be folded into `invalidate_pane_cache`.
- `truncate_label` in `app/ui/palette.rs:67` hard-codes
  `panel_w - 16.0`. The 16.0 is the value of `text_pad * 2` but is not
  referenced through the constant. Symptom of no `text-overflow: ellipsis`
  in the framework.
- `filter_palette` (`crates/ciri-app/src/app/palette.rs:478`) re-runs
  `e.label.to_lowercase()` on every entry per keystroke. `PaletteEntry`
  should pre-store a lowercase form at `rebuild_palette_entries` time.

### 1.3 Allocation profile (verified)

The render path itself uses pooled buffers correctly: `RenderBuffers`
(`crates/ciri/src/app/mod.rs:88`) is taken/refilled/put-back via `mem::take`
preserving capacity (`app/render.rs:2227-2234, 2417-2423`). Idle frames hit
`last_render_snapshot` early-return and allocate nothing.

The allocation problems live above the render buffers, in the chrome
painting path:

1. **Non-retained TaffyTree.** ciri-ui ships `paint_tree_into_with` for
   exactly this case (`crates/ciri-ui/src/layout.rs:194`, with a doc comment
   that says "Prefer this in the live render path"). The bin crate calls
   the non-retained `paint_tree` and `paint_tree_with_layout` instead
   (`app/ciri_ui_adapter.rs:185`, `app/ui/types.rs:168, 185`). With ~12
   chrome widgets per frame and an extra `ui_hit_id` call per mouse move,
   this is dozens of `TaffyTree::new()` and `LayoutSnapshot::new()` per
   frame. **ciri-ui's retained API does not yet expose a tree-and-snapshot
   variant** — the internal `paint_tree_into_with_snapshot`
   (`layout.rs:217`) is `pub(crate)` only.
2. **`Box::new` per Element child.** `Div::child` does
   `self.children.push(Box::new(child))` (`crates/ciri-ui/src/elements/div.rs:51`).
   A palette frame with 30 rows × ~3 nodes/row is ~150 system mallocs.
   GPUI uses an arena allocator (`zed/crates/gpui/src/arena.rs`) with
   `AnyElement(ArenaBox<dyn ElementObject>)` (`zed/crates/gpui/src/element.rs:593`)
   bumped from a 1 MB per-window arena (`zed/crates/gpui/src/window.rs:240`)
   and cleared at frame end. Element trees become amortized zero-alloc.
3. **`Vec<Box<dyn Element>>` children grow from cap 0.** GPUI uses
   `SmallVec<[StackSafe<AnyElement>; 2]>` (`zed/crates/gpui/src/elements/div.rs:1391`)
   so divs with ≤2 children store inline.
4. **`String` clone for static labels.** `Text::content: String`
   (`crates/ciri-ui/src/elements/text.rs:28`) means `text("Copy")` runs
   `String::from("Copy")` per frame. GPUI:
   `impl Element for &'static str` (`zed/crates/gpui/src/elements/text.rs:21`)
   plus `SharedString = SmolStr` (`zed/crates/gpui_shared_string/gpui_shared_string.rs:14`)
   with `new_static`. Static labels are zero-alloc; short dynamic labels
   are inline (≤22 bytes).
5. **`cached_ui_scene.sdf_rects.clone()` per frame.**
   `crates/ciri/src/app/render.rs:2369` clones the cached chrome SDF Vec so
   transient UI can extend it without polluting the cache. Cheap fix:
   `take → extend → truncate-to-cached-len → put back`, or pass two slices
   to `draw_frame`.
6. **`taffy::Style` rebuilt per node per frame.** `el.taffy_style()` runs
   `to_taffy_style(&self.style)` for every node every paint
   (`crates/ciri-ui/src/layout.rs:296` → `:428`). No memoization despite
   `Style` being plain `Option<T>` data.
7. **`filter_palette` per-keystroke `e.label.to_lowercase()`** as called
   out in §1.2.

These are independent fixes. The TaffyTree retention fix alone is worth
shipping on its own.

---

## 2. ciri-ui vs GPUI: structural gap

ciri-ui has the right skeleton — `Element` trait, Taffy-driven layout, a
`Scene` IR, `Layer` z-ordering, a Tailwind-shaped builder — but the
ergonomics and the state model are missing critical pieces. The gap is
not "ciri-ui is bad", it is "ciri-ui is the 30% of GPUI that is enough
to draw chrome and not yet enough to host stateful interactive widgets
without leaking state into `AppModel`".

### 2.1 Trait surface

| Concern | GPUI | ciri-ui |
|---|---|---|
| Low-level renderable | `Element` w/ `request_layout`/`prepaint`/`paint` + `RequestLayoutState`/`PrepaintState` (`element.rs:51-104`) | `Element::paint` only (`element.rs:225`) |
| Auto-coercion to element | `IntoElement` (`element.rs:113`); `impl Element for &'static str / SharedString / String → SharedString` (`elements/text.rs:21,79,95`) | none — children must be `impl Element` |
| Reusable view | `Render::render(&mut self, win, cx) -> impl IntoElement` (`element.rs:131`) | none |
| Reusable component | `RenderOnce::render(self, win, cx) -> impl IntoElement` + `#[derive(IntoElement)]` (`element.rs:147`) | none |
| Children-accepting trait | `ParentElement::child(impl IntoElement)` (`element.rs:156`) | `Div::child<E: Element>` only on `Div` |

Every gap above is felt at the call site: `div().child(text("Copy"))`
where GPUI writes `div().child("Copy")`; `PaletteComponent::capture(app)`
producing a per-frame snapshot where GPUI puts the same fields on a
`Render`-implementing struct that lives across frames.

### 2.2 Style + state

| Concern | GPUI | ciri-ui |
|---|---|---|
| Sparse override type | `#[derive(Refineable)]` auto-generates `StyleRefinement` (`refineable/src/refineable.rs:30`); `Style::refine(&StyleRefinement)` overlays only set fields | `Style` itself is all-`Option<T>` and serves both roles ambiguously (`style.rs:109`) |
| Conditional state styles | `.hover(\|s\| s.bg(...))` / `.active(...)` / `.focus(...)` / `.in_focus(...)` / `.focus_visible(...)` produce a `StyleRefinement`, applied at paint when state matches (`elements/div.rs:752, 1148, 1158, 1229, 2799`) | `on_hover(\|over\| ...)` callback only (`style.rs:99`); caller must store hover bool and rebuild |
| Group selectors | `.group("name")` + `.group_hover("name", \|s\| ...)` for sibling/descendant pseudo-classes (`elements/div.rs:760`) | none |
| Builder conditionals | `FluentBuilder::when / when_some / map` (`util.rs:11-65`) | none — every conditional breaks the chain |

The hover-via-callback model is why `palette.hovered_idx`, `top_bar.hovered_tab`,
`tab_bar.hovered_tab` all live in `AppModel` rather than on the widget.

### 2.3 Identity + persistent state

| Concern | GPUI | ciri-ui |
|---|---|---|
| Per-element identity | `GlobalElementId(Arc<[ElementId]>)`, path-stack maintained by walker (`window.rs:2266-2269`) | `ElementId(u64)`, flat (`element.rs:21`) |
| Cross-frame state map | `element_states: HashMap<(GlobalElementId, TypeId), ElementStateBox>` (`window.rs:777`); `Frame::finish` migrates accessed entries from `prev_frame` to `next_frame`, drops the rest (`window.rs:917-924`) — auto GC | none |
| Frame lifecycle | `next_frame` / `rendered_frame` swap (`window.rs:2495`); element tree allocates into per-window arena, cleared at frame end | `cached_ui_scene` keyed by global hash; full chrome rebuild on any state change |

Ciri's `App.cached_*` HashMaps and per-widget hover state are filling
the gap that `element_states` would fill.

### 2.4 Element library

| Need | GPUI | ciri-ui |
|---|---|---|
| Fixed-height virtual list | `uniform_list(id, count, render_range)` (`elements/uniform_list.rs:24`) | manual: palette only renders visible rows but allocates all entries |
| Variable-height virtual list | `list(state)` with intrusive `ListState` (`elements/list.rs`) | none |
| Anchored popup | `anchored` (`elements/anchored.rs`) | absolute positioning by hand; fixed `Layer` enum |
| Z-order escape hatch | `deferred(child)` defers paint to last (`elements/deferred.rs`) | fixed 5-level `Layer` enum (`element.rs:27`) |
| Custom drawing | `canvas(prepaint, paint)` (`elements/canvas.rs:11`) | none — drop to `paint(&self, cx)` directly |
| Asset elements | `svg`, `img`, `image_cache` | none |
| Animation wrapper | `animation` element (`elements/animation.rs`) | per-property transitions on 4 fields |

### 2.5 Hitboxes + focus + key dispatch

| Concern | GPUI | ciri-ui |
|---|---|---|
| Hitbox registration | `Window::insert_hitbox(bounds, behavior)` during prepaint (`window.rs:3945`); `Hitbox::is_hovered(window)` queryable in paint | external `hit_test(&self, x, y, bounds)` re-walks the tree |
| Occlusion | `HitboxBehavior::BlockMouse` / `.occlude_mouse()` / `.block_mouse_except_scroll()` (`elements/div.rs:651, 665`) | none — manual `HIT_PANEL` / `HIT_CLOSE` ID convention per widget |
| Focus | `FocusHandle`, `.track_focus(handle)` (`elements/div.rs:696`); `is_focused`/`within_focused` | none |
| Key dispatch | `DispatchTree` (`key_dispatch.rs:71`), per-node `KeyContext`, `Action` registry, `.on_action::<T>(\|e, win, cx\| ...)` | global `BindingMode` switch on `command_palette.is_some()` etc. (`crates/ciri/src/app/keyboard.rs:301`) |

Ciri's modal binding-mode switch is fine for the current modal set
(palette / search / overview / paste-confirm). It will not scale to
in-pane focusable widgets (think: multi-cursor, inline rename) without
turning into a chain of `if`s.

---

## 3. Refactor sequence

Each item is independently shippable. Earlier items reduce the friction
of later ones; items can be re-ordered as long as that ordering is
respected.

### Phase 0 — clean wins (no architecture change)

These are bugfixes / small refactors found during the audit. None
require any framework change.

- **0a.** [DONE] Use the retained TaffyTree variant. ciri-ui already has
  `paint_tree_into_with` (`layout.rs:194`); promoted internal
  `paint_tree_into_with_snapshot` to public `paint_tree_into_retained`
  for the hit-test path. Adapter holds `RefCell<TaffyTree<NodeContext>>`
  on `App`, threaded through `UiContext::taffy_tree`.
- **0b.** [DONE] Dedupe `tile_paint_config`. `build_tiles` now calls
  `self.tile_paint_config()`; the duplicated 18-line literal is gone.
- **0c.** [DONE] `SectionHeader` skip lives on
  `CommandPaletteState::move_selection(delta, wrap)` in ciri-app.
  Keyboard nav passes `wrap=true`, mouse-wheel passes `wrap=false`.
  Three duplicated loops collapsed.
- **0d.** [DONE] Per-pane image-atlas eviction split into its own
  `invalidate_pane_images(pane_id)` method. `invalidate_pane_cache`
  keeps the cheap per-pane caches only (views, tile glyphs, tile
  backgrounds) so it stays cheap to call on every scroll / resize /
  selection. The two `sync.rs` sites that previously did
  `image_atlas_entries.clear() + invalidate_pane_cache(pid)` (PaneClosed
  and ImageDeleted) now call `invalidate_pane_images(pid) +
  invalidate_pane_cache(pid)`. (An earlier draft folded the image
  eviction into `invalidate_pane_cache` — Codex caught the regression
  before it shipped.)
- **0e.** [DONE] `PaletteEntry::new(label, kind)` constructor stores
  `lowercase_label` once at construction; `filter_palette` reads
  `e.lowercase_label` directly. All 21 struct-literal sites converted.
- **0f.** [DONE] `cached_ui_scene.sdf_rects.clone()` replaced with
  `mem::take` + record `cached_len` + transient extend + post-draw
  `truncate(cached_len)` + put-back. One Vec memcpy per frame
  eliminated; capacity reused.
- **0g.** [PARTIAL] Split `render()` into four sub-methods. Hands-on
  attempt revealed the per-pane view loop, buffer take dance, and
  `cache`/`shaper`/`cached_views` borrow interleaving make a clean
  full-extraction impossible without first restructuring `App`. The
  borrow-clean preamble (~37 lines: surface check, fallback-arena
  clear, `dt` computation, focus-change pre-tick) has been extracted
  to `App::prepare_frame()`. The remaining three splits (per-pane
  view update, buffer assembly, atlas/cleanup) still want a
  `RenderState` substructure on `App` first; the natural prerequisite
  is finishing Phase 5's state migration so palette/top_bar/tab_bar
  state stops sharing `&mut self` with the render loop. Pick this
  back up once a real chrome widget has moved its state onto an
  `impl Render` view.

### Phase 1 — same-frame hover detection (light prepaint)  [DONE]

Shipped a smaller scope than the original "prepaint phase + hitbox
API" plan: the walker now does a layout-only pre-pass when a mouse
position is supplied, queries the resulting `LayoutSnapshot` for the
topmost element under the cursor, and threads that `hit_id` through
every paint call via `PaintCtx::hovered_hit_id` (with a
`PaintCtx::is_hovered(hit_id)` helper).

This unblocks Phase 3's declarative `.hover(|s| ...)` styles without
adding a formal `prepaint` method to `Element`. The dual-tree
pattern in `tab_bar` / `top_bar` (coarse hit zones vs. visual
elements) was kept — Phase 1's audit revealed it's intentional UX,
not duplicate work.

Phase 1 light also delivered the layout-only hit-test walker
(`layout_tree_into_retained`) — `ui_hit_id` and `ui_hit_bounds`
skip `Element::paint` entirely, dropping ~30-50 SDF rect emissions
per palette hit-test call.

### Phase 2 — `IntoElement` + `SharedString`  [DONE]

Shipped:

- `SharedString` newtype over `SmolStr` (`crates/ciri-ui/src/shared_string.rs`).
  Inlines short strings (≤22 bytes), `Arc<str>`-shares longer ones.
  `new_static(&'static str)` is `const` for zero-alloc literals.
- `Text::content` and `NodeContext::Text { content }` switched from
  `String` to `SharedString`. The per-frame `taffy_context()` clone is
  no longer a `String::clone()` — it is now a SmolStr ref-count bump
  (or stack memcpy for inline strings).
- `IntoElement` trait in `ciri-ui::element`. Identity impls for `Div`
  and `Text`. String coercions: `&str`, `String`, `SharedString` all
  convert to `Text`.
- `Div::child(impl IntoElement)` replaces `Div::child<E: Element>`.
  Existing call sites that pass a built element keep compiling.

Diverged from the original sketch:

- `impl Element for &'static str` was not added. ciri-ui's `Element`
  trait carries fields specific to chrome text (color override, font
  size override, layer-aware paint inheritance) that don't make sense
  on a bare `&'static str`. Going through `Text` keeps the trait small
  and matches GPUI's `IntoElement<Element = SharedString>` pattern
  rather than its `impl Element for &'static str` direct path.

### Phase 3 — declarative hover styles via `Div::hover`  [DONE]

Avoided the `refineable` crate dependency by using the existing
all-`Option<T>` `Style` as both the base and the refinement type.
`Style::merge(&mut self, other)` already overlays non-`None` fields,
so a refinement closure builds a fresh `Style`, stashes it on `Div`
as `hover_style: Option<Box<Style>>`, and `Div::paint` resolves the
effective style by merging when the matching hit_id is hovered.

Surface: `div().bg(...).hit_id(7).hover(|s| s.bg(accent))`.
Refinement closures take a `Style` argument so the same Tailwind-
shaped builder (`Style: Styled` impl added) chains identically to
the base. `effective_style` returns `Cow::Borrowed(&self.style)`
when no refinement matches, so static-styled elements pay zero
cloning overhead.

Production usage: palette entry rows, context-menu rows, paste-
dialog buttons, top-bar session label, top-bar workspace indicator.
The pattern dropped per-row `is_hovered: bool` snapshots from each
widget's capture step — hover styling is now resolved at paint time
from `cx.is_hovered(hit_id)` and the refinement merges over the
base.

**Refinement inheritance** (added later): the walker's
`text_color_override_with_state(hovered_hit_id)` lets a
`.hover(|s| s.text_color(...))` refinement on a parent Div
propagate to descendant Text. Without this, refinement-only
overrides were stuck affecting just the Div's own paint.

Future `.active(|s| ...)` / `.focus(|s| ...)` slot into the same
`effective_style` resolver — they need only the corresponding state
flag exposed on `PaintCtx`.

### Phase 4 — `ElementStates` for cross-frame persistence  [DONE]

Shipped a flat `(ElementId, TypeId) -> Box<dyn Any>` map on
`PaintCtx::states`. Elements with a stable `Element::id()` call
`cx.states.use_state::<MyState>(id)` to borrow a typed mutable slot
that survives across frames. Default-constructed on first use.

Skipped GPUI's path-based `GlobalElementId` (works on a single
window with non-conflicting id namespace) and the auto-GC pattern
(callers explicitly `clear_id` when an element is permanently
gone). Both can be retrofitted later if the chrome grows enough
elements that flat ids collide.

`App.ui_states: RefCell<ElementStates>` is published through
`UiContext::element_states`; the adapter scope-borrows once per
paint pass and threads `Option<&mut ElementStates>` through the
walker. Each child reborrow keeps the slot accessible during deep
trees.

### Phase 5 — `Render` trait for stateful view objects  [DONE]

Shipped the trait shape:

```rust
pub trait Render: 'static + Sized {
    fn render(&mut self, cx: &RenderCtx<'_>) -> impl IntoElement;
}
```

`RenderCtx { theme, viewport, scale }` is intentionally minimal so
the trait can grow fields without breaking implementors.

**Production users:** `PaletteComponent`, `ContextMenuComponent`,
and `PasteDialogComponent` all `impl Render`. Each widget's
`paint(&mut self, cx, scene)` projects `UiContext → RenderCtx` via
a private `render_cx` helper, then calls
`<Self as Render>::render(self, &render_cx).into_element()`. Hit-
test still goes through `build_tree` directly with `&self` since
those paths don't need to mutate.

What's deferred: moving long-lived view state onto the `Render`
implementor (the original Phase 5 vision had `PaletteView` owning
filtered/scroll/hover state, replacing the per-frame
`*Component::capture` snapshot). The current widgets still use
the snapshot pattern. Migrating that requires restructuring
`AppModel.command_palette` etc. into view-owned data, which is a
much larger refactor than the trait wiring — most of the value is
already captured by combining Phase 1's hover walker (so display
is stateless), Phase 3's declarative `.hover()` (so per-frame
hover bools went away), and the AppModel-cleanup pass that
evicted UI ephemera into `App` or derived helpers.

### Phase 6 — `uniform_list` helper  [DONE]

Shipped as a function helper rather than a stateful Element type:

```rust
pub fn uniform_list<F, E>(
    item_count: usize,
    item_height: f32,
    visible_range: Range<usize>,
    render: F,
) -> Div
where F: FnOnce(Range<usize>) -> Vec<E>, E: IntoElement
```

Caller computes the visible range from their own scroll state (which
can live on `ElementStates` per Phase 4 or on a parent struct). The
helper builds the `Div` column with the right height for the visible
slice and invokes the `render` closure exactly once.

Skipped GPUI's full Element-based `uniform_list` because ciri's
chrome already pre-computes visible row data during model rebuild
(palette filtering is the prime example). A pure helper drops in to
that flow without forcing a prepaint integration.

Future stateful list element (auto-virtualizes from parent clip
area, owns its own scroll state, handles wheel events) would build
on Phase 4's `ElementStates` and a real `prepaint` phase.

### Phase 7 — arena element allocator  [DONE]

Shipped in `crates/ciri-ui/src/arena.rs`. Chunked bump allocator
(`Arena` / `ArenaBox`) ported from GPUI; `AnyElement` is now
`ArenaBox<dyn Element>` and constructed via the active arena published
by an `ElementArenaScope` RAII guard. `App` owns one `RefCell<Arena>`
which the chrome and transient paint paths each enter for the duration
of their pass; a thread-local fallback arena covers test / hit-test
callers that don't push a scope, and is cleared at the head of every
`render()` to keep its growth bounded.

### Phase 8 — `Children: SmallVec<[..; 2]>`  [DONE]

`Div::children` now stores up to 2 children inline via
`SmallVec<[Box<dyn Element>; 2]>`. Containers with ≤2 children pay no
children-vec heap allocation; >2-child containers spill transparently.
`Element::children() -> &[Box<dyn Element>]` still returns a slice via
`SmallVec`'s `Deref`, so no caller needed updating.

### Phase 9 — `anchored` + `deferred`, retire fixed `Layer` enum  [DEFERRED]

Port `anchored` and `deferred` elements. Migrate `context_menu` to
`anchored`. Migrate `Modal`/`Tooltip` z-ordering to `deferred`. Remove
the `Layer` enum and its inheritance threading from the walker.

This is a noticeable behavioral refactor — the chrome rendering will
look identical but the underlying ordering mechanism changes. Worth
doing once Phase 1 is done so the behavior is testable per widget.

**Why deferred:** GPUI's `anchored` relies on a `prepaint` phase (which
ciri-ui only has in "light" form — Phase 1 didn't add a formal trait
method) and `deferred` needs a `Window::defer_draw(child, offset,
priority, …)` queue ciri-ui has no analogue for. Porting both honestly
requires (a) Phase 1 promotion to a real `Element::prepaint`, (b) a
scene-level deferred-paint queue or a walker post-pass, and (c)
removing `Element::layer()` + the walker's `inherited_layer` thread.
That's a 1-2 day refactor with workspace-wide blast radius and is
better landed as its own focused branch rather than squeezed into a
multi-step series. Until then, the existing 5-level `Layer` enum keeps
covering ciri's chrome (palette / paste_dialog / context_menu /
connection_banner / tooltip) without trouble.

### AppModel UI-ephemera cleanup pass  [DONE]

A separate sweep through `AppModel` (the platform-agnostic core in
`ciri-app`) removed every UI-only field that had crept into the
model layer. The motivating observation: `AppModel` was carrying
hover indices, drag bookkeeping, gesture accumulators, frame
timestamps, scroll offsets, and cursor blink state — none of which
have any meaning outside the bin-crate UI shell. They were all
read and written exclusively by `crates/ciri/src/app/`.

Two relocation patterns:

**Derive at hash-time** (no storage). Used when the value is a
function of `last_mouse_pos` + a known geometry that the chrome
cache hash can recompute cheaply. Display reads it via
`cx.is_hovered(hit_id)` declaratively. No close-site bookkeeping
because nothing is stored — the helper sees only what's currently
true.

- `palette.hovered_idx` → `App::current_palette_hover()` (Step 6).
- `ContextMenu.hovered_index` → `App::current_context_menu_hover()`
  (Step 7). Also extracted `idx` from
  `handle_context_menu_click(idx)` so the field wasn't doing
  double duty as a hidden parameter.
- `PendingPaste.hovered_button` → `App::current_paste_dialog_hover()`
  (Step 10). Buttons additionally use `.hover(|s| s.bg(...))`
  declarative styling instead of imperative branching.

**Move to `App`** (storage stays, owner changes). Used when the
value can't be cheaply re-derived (drag offsets, scroll positions
accumulated over time, animation timestamps) or when a widget's
paint path is still imperative and needs the cached value. The
relocation trims `AppModel` without changing semantics.

- `hovered_top_bar_region` + `hovered_pane_tab` (Step 8).
- `OverviewState.hovered_pane` + `overview_action_hover` (Step 9 —
  `App::exit_overview` / `toggle_overview` wrappers got an explicit
  reset since the model-level reset in `AppModel::exit_overview`
  was no longer touching these App-owned fields).
- `pane_tab_scroll` (Step 11).
- `ResizeDragState` (Step 12 — 8-field struct with ~30 reader
  sites in mouse / resize / sync / render).
- `GestureState` (Step 13).
- `cursor_blink_visible` + `cursor_blink_timer` +
  `last_focus_follows_mouse` (Step 14).
- `last_frame` (Step 15).

After this pass, `AppModel`'s remaining fields all encode genuine
model state (config, workspaces, pane grids, selection, paste
content, connection state, search state, multi-click counters that
feed selection mode, link hover that feeds URL-open). The
last-mile candidates (`last_left_click`, `hovered_link`,
`workspace_last_pane_ids`) all have `ciri-app` consumers that read
them, so they stay on the model.

### Top-bar sub-widgets — declarative hover  [DONE]

`SessionLabel` and `WorkspaceIndicator` were converted from
imperative `if hovered { fg } else { dim }` text-color switching to
declarative `.hit_id(...).text_color(rest).hover(|s|
s.text_color(hov))` on a parent Div. Required adding
`Element::text_color_override_with_state(hovered_hit_id)` so the
walker propagates the refinement-time text colour to descendant
Text nodes — the stateless variant only saw `self.style.text_color`
and missed hover refinements. `Div` is the only override; default
impl falls back to the stateless method.

Wrapper geometry intentionally spans the full slot rect (not just
`cell_h`) so the paint hit area matches the click hit area in
`build_hit_tree`. Click-padding regions now light up the hover
style, matching pre-refactor behaviour.

`PaneTabsElement` and `ModeIndicator` keep imperative paint for
now: pane-tabs draws multiple absolute siblings (bg, separator,
indicator, label) per tab, which would need restructuring into a
per-tab wrapper Div for declarative hover; mode label has no hover
state. `App.hovered_top_bar_region` and `App.hovered_pane_tab`
remain because tab paint and the chrome cache hash still consume
them.

### Out of scope (explicit non-goals)

- **GPUI's `Action` + `KeyContext` system** in full. Ciri already has
  `ciri-input::action::Action` + leader/key_table; the modal
  `BindingMode` works. Adopting `FocusHandle` + `track_focus` is
  enough for the hypothetical future where chrome widgets need their
  own keys.
- **`list.rs` (variable-height virtualization)**. 2060 lines, complex
  intrusive state model. ciri's lists are uniform-height. `uniform_list`
  alone is sufficient.
- **`image_cache` / `Asset` framework**. Chrome doesn't load remote
  assets.
- **CSS Grid (`grid_cols / grid_rows / grid_location`)**. Flexbox
  covers chrome layout entirely.
- **Inspector GUI**. `source_location()` on Element is cheap to add and
  worth having for log/debug; the inspector tree-explorer GUI is not.

---

## 4. First commit

Phase 0a — retained TaffyTree. Smallest possible change that demonstrates
the doc → branch → ship loop, and removes the largest single allocation
hotspot per frame.

Plan:

1. In `crates/ciri-ui/src/layout.rs`, add a public function that takes
   both an external `TaffyTree<NodeContext>` and writes into an external
   `LayoutSnapshot`. Internal `paint_tree_into_with_snapshot` already
   does this — promote it or wrap it.
2. In `crates/ciri/src/app/ciri_ui_adapter.rs`, hold a
   `RefCell<TaffyTree<NodeContext>>` (placed on `App` or a new
   `UiResources` struct). Have `paint_element_tree` borrow it and call
   `paint_tree_into_with` (the no-snapshot variant).
3. In `crates/ciri/src/app/ui/types.rs`, change `ui_hit_id` and
   `ui_hit_bounds` to use the new with-tree-and-snapshot variant,
   borrowing the same `TaffyTree`.
4. Verify with the existing `app/ui/palette.rs:417, 471` tests and
   `app/ui/top_bar/tests.rs`.

No public-API breakage. `paint_tree` / `paint_tree_with_layout` /
`paint_tree_into_with_layout` remain for tests and one-shot callers.
