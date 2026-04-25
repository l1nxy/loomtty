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
- **0g.** [DEFERRED to Phase 1+] Split `render()` into four sub-methods.
  Hands-on attempt revealed the per-pane view loop, buffer take dance,
  and `cache`/`shaper`/`cached_views` borrow interleaving make a clean
  extraction impossible without first restructuring `App`. Doing this
  before Phase 1's `RenderState` substructure would just produce ugly
  parameter lists. Pick this back up after Phase 1 when the borrows
  collapse to `&mut self.render`.

### Phase 1 — `prepaint` phase + hitbox API

Adds `Element::prepaint(&mut self, bounds, request_layout: &mut S, cx) -> P`
between layout and paint. Walker calls `prepaint` after computing layout,
before paint. Window grows `insert_hitbox(bounds) -> Hitbox` plus a
hit-result lookup.

Concrete payoff: delete `Element::hit_test`, delete `build_hit_tree`
duals in tab_bar/top_bar, palette `hit_test` no longer rebuilds the
visual tree.

This is a change to `ciri-ui::Element` (adds an associated type and a
method), so every element implementor must update. There are five today
(Div, Text, plus three transient widgets that bypass the trait); not
expensive.

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

### Phase 3 — `Refineable` + declarative state styles

Pull in `refineable` crate from Zed's workspace (it is permissively
licensed, single dependency on `derive_refineable`). Annotate `Style`
with `#[derive(Refineable)]`. Generate `StyleRefinement`. Add
`Interactivity` substate to `Div` with `hover_style: Option<StyleRefinement>`
etc. Add `.hover / .active / .focus / .group_hover` methods on
`InteractiveElement` trait.

Concrete payoff: hover state stops living in `AppModel`. Every widget
that today reads `palette.hovered_idx == Some(i)` can use
`.hover(|s| s.bg(theme.accent_tint))` declaratively. The hover hitbox is
the same hitbox the framework already inserts in prepaint.

This depends on Phase 1 (hitbox in prepaint).

### Phase 4 — per-element persistent state

Add `Window::with_id(id, |w| ...)` to push to a `GlobalElementId` stack.
Add `element_states: HashMap<(GlobalElementId, TypeId), Box<dyn Any>>` on
`Window` (or a per-window context). Add `accessed_element_states`
tracking, with frame `finish()` migrating accessed entries.

Concrete payoff: hover hover-into / hover-out animation timers, scroll
offsets, click-pending state, all live at the framework level. Solves
the "ephemeral UI state has no home but `AppModel`" problem outright.

This depends on Phase 1 (the GlobalElementId path is constructed during
prepaint).

### Phase 5 — `Render` trait + view objects

Add `Render::render(&mut self, cx) -> impl IntoElement`. Refactor
`PaletteComponent` (and the other "Component" widgets in `app/ui/`) into
`impl Render` types that hold their own state and produce element trees
each frame. `App` creates them once, holds them as fields, calls
`render` on them.

Concrete payoff: `PaletteComponent::capture(app)` per-frame snapshot
allocation goes away. Component state (which today is split between
`AppModel.palette` and the snapshot) consolidates onto the component.

This depends on Phase 4.

### Phase 6 — virtualization

Port `uniform_list(id, count, render_range)` from GPUI. Palette adopts
it for its row list. Deletes the manual `skip(scroll_offset).take(visible_rows)`
logic in `PaletteComponent::capture`.

This depends on Phases 1, 2, 4.

### Phase 7 — arena element allocator

Port `Arena` + `ArenaBox` (`zed/crates/gpui/src/arena.rs`, ~290 lines —
`alloc::alloc`-based bump allocator with `Drop` registration). Wrap
`AnyElement = ArenaBox<dyn ElementObject>`. Per-window static
`ELEMENT_ARENA: RefCell<Arena>` cleared at frame end via
`ArenaClearNeeded` token RAII pattern.

Concrete payoff: amortized-zero element tree allocation. The largest
remaining heap pressure source.

This is largely independent and can be slotted in at any point after
Phase 2 (which gives us the dynamic-typed `AnyElement` boundary).

### Phase 8 — `Children: SmallVec<[..; 2]>`  [DONE]

`Div::children` now stores up to 2 children inline via
`SmallVec<[Box<dyn Element>; 2]>`. Containers with ≤2 children pay no
children-vec heap allocation; >2-child containers spill transparently.
`Element::children() -> &[Box<dyn Element>]` still returns a slice via
`SmallVec`'s `Deref`, so no caller needed updating.

### Phase 9 — `anchored` + `deferred`, retire fixed `Layer` enum

Port `anchored` and `deferred` elements. Migrate `context_menu` to
`anchored`. Migrate `Modal`/`Tooltip` z-ordering to `deferred`. Remove
the `Layer` enum and its inheritance threading from the walker.

This is a noticeable behavioral refactor — the chrome rendering will
look identical but the underlying ordering mechanism changes. Worth
doing once Phase 1 is done so the behavior is testable per widget.

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
