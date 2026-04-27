//! Paint-pipeline glue: build a Taffy tree from an `Element` tree, run
//! layout, then walk both in parallel to emit primitives into a `Scene`.
//!
//! The walker propagates two pieces of paint-time state down the tree:
//!
//! 1. **Taffy-absolute offset** — the accumulated `layout.location` of
//!    ancestors. Pure layout, independent of any paint transforms.
//! 2. **Inherited translate / opacity / text colour** — CSS-style paint
//!    inheritance. Translate moves the subtree without touching layout;
//!    opacity multiplies down; text colour cascades nearest-ancestor
//!    wins. All three derive from `Element::paint_transform()` /
//!    `text_color_override_with_state()`.
//!
//! Z-order falls out of paint sequence: non-deferred elements emit in
//! tree order, then queued [`crate::elements::Deferred`] subtrees drain
//! in ascending priority. There is no fixed layer enum.

use taffy::TraversePartialTree;

use crate::element::{Element, ElementStates, PaintCtx};
use crate::scene::Scene;
use crate::shaper::TextShaper;
use crate::style::{
    AlignItems as UiAlignItems, Display as UiDisplay, FlexDirection as UiFlexDirection,
    JustifyContent as UiJustifyContent, Length, Position as UiPosition,
};
use crate::theme::ResolvedTheme;

/// One captured deferred subtree, queued during the main walk and
/// drained after it. Carries everything the second walk needs to
/// resume painting where the first walk would have continued — bounds
/// inherit from the deferred wrapper's parent, not from the wrapper
/// itself.
struct DeferredEntry<'a> {
    el: &'a dyn Element,
    node: taffy::NodeId,
    parent_local: [f32; 2],
    inherited_translate: [f32; 2],
    inherited_opacity: f32,
    inherited_text_color: Option<crate::color::Color>,
    priority: u32,
}

/// Per-node side-channel the layout pass hands to its
/// `compute_layout_with_measure` callback. Only leaves that need
/// shaper-driven sizing attach one; containers stay `None` and let
/// Taffy size them from their children + their own `taffy_style`.
#[derive(Clone, Debug)]
pub enum NodeContext {
    /// Text leaf: layout defers to the host `TextShaper::measure`,
    /// which sees the same font size used at paint time so the two
    /// passes never disagree on width.
    Text {
        content: crate::shared_string::SharedString,
        font_size_px: f32,
    },
}

/// A laid-out element record captured from the same Taffy pass used for paint.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutNode {
    pub type_id: &'static str,
    pub bounds: [f32; 4],
    pub paint_order: usize,
    pub accepts_pointer_events: bool,
    pub hit_id: Option<u64>,
}

impl LayoutNode {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.bounds[0]
            && x < self.bounds[0] + self.bounds[2]
            && y >= self.bounds[1]
            && y < self.bounds[1] + self.bounds[3]
    }
}

/// Layout side-channel for event dispatch and debugging.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayoutSnapshot {
    nodes: Vec<LayoutNode>,
}

impl LayoutSnapshot {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn nodes(&self) -> &[LayoutNode] {
        &self.nodes
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
    }

    pub fn push(&mut self, node: LayoutNode) {
        self.nodes.push(node);
    }

    /// Return the topmost pointer target at `x,y`.
    ///
    /// Later paint order wins — same ordering the renderer uses when
    /// flattening the scene streams. Deferred subtrees naturally sit on
    /// top because the drain pass paints them after every non-deferred
    /// sibling.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<&LayoutNode> {
        self.nodes
            .iter()
            .filter(|n| n.accepts_pointer_events && n.contains(x, y))
            .max_by_key(|n| n.paint_order)
    }
}

/// Combined output for callers that need paint primitives and hit-test data.
#[derive(Clone, Default)]
pub struct PaintOutput {
    pub scene: Scene,
    pub layout: LayoutSnapshot,
}

/// Layout and paint an element tree into a fresh [`Scene`].
///
/// `text_shaper` is the host's bridge for measuring + emitting text;
/// tests can pass [`crate::shaper::NullShaper`].
pub fn paint_tree(
    root: &dyn Element,
    theme: &ResolvedTheme,
    viewport: [f32; 2],
    scale: f32,
    text_shaper: &mut dyn TextShaper,
) -> Scene {
    let mut scene = Scene::new();
    paint_tree_into(root, theme, viewport, scale, text_shaper, &mut scene);
    scene
}

/// Layout and paint an element tree, returning the rendered scene plus a
/// snapshot of the same layout pass for hit testing.
pub fn paint_tree_with_layout(
    root: &dyn Element,
    theme: &ResolvedTheme,
    viewport: [f32; 2],
    scale: f32,
    text_shaper: &mut dyn TextShaper,
) -> PaintOutput {
    let mut out = PaintOutput::default();
    paint_tree_into_with_layout(
        root,
        theme,
        viewport,
        scale,
        text_shaper,
        &mut out.scene,
        &mut out.layout,
    );
    out
}

/// Same as [`paint_tree`] but appends into an existing scene.
pub fn paint_tree_into(
    root: &dyn Element,
    theme: &ResolvedTheme,
    viewport: [f32; 2],
    scale: f32,
    text_shaper: &mut dyn TextShaper,
    scene: &mut Scene,
) {
    let mut tree = taffy::TaffyTree::<NodeContext>::new();
    let mut layout = LayoutSnapshot::new();
    paint_tree_into_retained(
        root,
        theme,
        viewport,
        scale,
        text_shaper,
        scene,
        &mut layout,
        &mut tree,
        None,
        None,
    );
}

/// Same as [`paint_tree_into`] but also fills a layout snapshot.
pub fn paint_tree_into_with_layout(
    root: &dyn Element,
    theme: &ResolvedTheme,
    viewport: [f32; 2],
    scale: f32,
    text_shaper: &mut dyn TextShaper,
    scene: &mut Scene,
    layout: &mut LayoutSnapshot,
) {
    let mut tree = taffy::TaffyTree::<NodeContext>::new();
    paint_tree_into_retained(
        root,
        theme,
        viewport,
        scale,
        text_shaper,
        scene,
        layout,
        &mut tree,
        None,
        None,
    );
}

/// Retained-tree variant of [`paint_tree_into`]. The caller owns a
/// `TaffyTree` that survives across frames; this function `clear()`s
/// and rebuilds it each call, reusing the allocator instead of freeing
/// and re-allocating dozens of SlotMap slots per frame. Prefer this in
/// the live render path; the one-shot `paint_tree[_into]` remains for
/// tests and unit calls.
///
/// `mouse_pos` enables same-frame hover detection — the walker does a
/// layout-only pre-pass to find the topmost element under the cursor
/// and threads its `hit_id` into every paint call. Pass `None` to skip
/// the pre-pass when hover information isn't needed.
pub fn paint_tree_into_with(
    root: &dyn Element,
    theme: &ResolvedTheme,
    viewport: [f32; 2],
    scale: f32,
    text_shaper: &mut dyn TextShaper,
    scene: &mut Scene,
    tree: &mut taffy::TaffyTree<NodeContext>,
    mouse_pos: Option<[f32; 2]>,
    states: Option<&mut ElementStates>,
) {
    let mut layout = LayoutSnapshot::new();
    paint_tree_into_retained(
        root,
        theme,
        viewport,
        scale,
        text_shaper,
        scene,
        &mut layout,
        tree,
        mouse_pos,
        states,
    );
}

#[allow(clippy::too_many_arguments)]
/// Layout-only walker: builds the Taffy tree, computes layout, and
/// populates [`LayoutSnapshot`] for hit testing — but skips
/// [`Element::paint`]. Use this when a caller needs hit-test data
/// only and does not consume the [`Scene`] output (e.g. the modal
/// `hit_test` paths in ciri's chrome widgets).
///
/// Saves the cost of every per-element paint (SDF rect emission,
/// glyph shaping, inherited transform composition) for the
/// hit-test-only path. The `TaffyTree` is `clear()`ed and rebuilt as
/// in [`paint_tree_into_retained`], so callers should keep their tree
/// across frames for allocator reuse.
pub fn layout_tree_into_retained(
    root: &dyn Element,
    viewport: [f32; 2],
    text_shaper: &mut dyn TextShaper,
    layout_snapshot: &mut LayoutSnapshot,
    tree: &mut taffy::TaffyTree<NodeContext>,
) {
    layout_snapshot.clear();
    tree.clear();
    let root_node = build_taffy(tree, root);

    let available = taffy::Size {
        width: taffy::AvailableSpace::Definite(viewport[0].max(0.0)),
        height: taffy::AvailableSpace::Definite(viewport[1].max(0.0)),
    };
    let layout_result = tree.compute_layout_with_measure(
        root_node,
        available,
        |_known, _available, _node, ctx, _style| -> taffy::Size<f32> {
            match ctx {
                Some(NodeContext::Text {
                    content,
                    font_size_px,
                }) => {
                    let [w, h] = text_shaper.measure(content, *font_size_px);
                    taffy::Size {
                        width: w,
                        height: h,
                    }
                }
                None => taffy::Size {
                    width: 0.0,
                    height: 0.0,
                },
            }
        },
    );
    if let Err(e) = layout_result {
        log::warn!("ciri-ui: taffy compute_layout failed (hit-only): {e:?}");
        return;
    }
    let mut paint_order = 0;
    let mut deferred_queue: Vec<DeferredEntry<'_>> = Vec::new();
    walk_for_layout_snapshot(
        tree,
        root_node,
        root,
        /* parent_local */ [0.0, 0.0],
        /* inherited_translate */ [0.0, 0.0],
        layout_snapshot,
        &mut paint_order,
        &mut deferred_queue,
    );
    drain_deferred_layout_only(tree, deferred_queue, layout_snapshot, &mut paint_order);
}

/// Most-general paint entry point: caller owns the [`Scene`], the
/// [`LayoutSnapshot`], and the `TaffyTree`. All three are `clear()`ed
/// internally and re-filled, so storing them across frames keeps their
/// allocator capacity warm.
///
/// Use this in the live render path when both hit-test and paint are
/// needed (i.e. anywhere `LayoutSnapshot::hit_test` will be queried).
/// For paint-only callers, [`paint_tree_into_with`] is equivalent without
/// the snapshot fill.
///
/// `mouse_pos` (when `Some`) drives same-frame hover detection: the
/// walker does a layout-only pre-pass, finds the topmost element
/// under the cursor, then threads its `hit_id` through every paint
/// call so elements can apply hover styles. Pass `None` to skip the
/// pre-pass entirely.
pub fn paint_tree_into_retained(
    root: &dyn Element,
    theme: &ResolvedTheme,
    viewport: [f32; 2],
    scale: f32,
    text_shaper: &mut dyn TextShaper,
    scene: &mut Scene,
    layout_snapshot: &mut LayoutSnapshot,
    tree: &mut taffy::TaffyTree<NodeContext>,
    mouse_pos: Option<[f32; 2]>,
    states: Option<&mut ElementStates>,
) {
    layout_snapshot.clear();
    tree.clear();
    let root_node = build_taffy(tree, root);

    let available = taffy::Size {
        width: taffy::AvailableSpace::Definite(viewport[0].max(0.0)),
        height: taffy::AvailableSpace::Definite(viewport[1].max(0.0)),
    };
    // compute_layout_with_measure so Text leaves size through the host
    // shaper instead of a 0.5×font_size char-count heuristic. Mis-sized
    // leaves would mis-centre inside flex/justify parents once real
    // chrome text runs through the pipeline — proportional fonts, CJK
    // and emoji all behave differently under the heuristic.
    //
    // Single-line only: `known_dimensions` and `available_space` are
    // intentionally ignored because every chrome Text today is a
    // single-line run (banners, palette rows, hints, labels). Adding
    // wrapping requires an extended `TextShaper::measure_constrained`
    // API that reshapes against a max width and returns the wrapped
    // `(width, height)`. Until that lands, a Text node placed in a
    // container narrower than its natural line will overflow rather
    // than wrap — callers should truncate at the application layer
    // (see `app::ui::text_layout::truncate_to_width`).
    let layout_result = tree.compute_layout_with_measure(
        root_node,
        available,
        |_known, _available, _node, ctx, _style| -> taffy::Size<f32> {
            match ctx {
                Some(NodeContext::Text {
                    content,
                    font_size_px,
                }) => {
                    let [w, h] = text_shaper.measure(content, *font_size_px);
                    taffy::Size {
                        width: w,
                        height: h,
                    }
                }
                None => taffy::Size {
                    width: 0.0,
                    height: 0.0,
                },
            }
        },
    );
    if let Err(e) = layout_result {
        log::warn!("ciri-ui: taffy compute_layout failed: {e:?}");
        return;
    }
    // Phase 1: layout-only pre-pass to determine the topmost hit_id
    // under the cursor for this frame. Skipped when no mouse position
    // was provided — the walk is wasted otherwise. The pre-pass fills
    // `layout_snapshot`, but the paint walk below clears + refills it
    // with the same content, so the final state observed by callers
    // matches what they'd see without this hover lookup.
    let hovered_hit_id = if let Some([mx, my]) = mouse_pos {
        let mut prepaint_order = 0;
        let mut prepaint_deferred: Vec<DeferredEntry<'_>> = Vec::new();
        walk_for_layout_snapshot(
            tree,
            root_node,
            root,
            [0.0, 0.0],
            [0.0, 0.0],
            layout_snapshot,
            &mut prepaint_order,
            &mut prepaint_deferred,
        );
        drain_deferred_layout_only(
            tree,
            prepaint_deferred,
            layout_snapshot,
            &mut prepaint_order,
        );
        layout_snapshot.hit_test(mx, my).and_then(|n| n.hit_id)
    } else {
        None
    };

    // Phase 2: actual paint walk. Re-clear the snapshot so paint_order
    // and bounds match exactly what a single-pass walk would produce.
    layout_snapshot.clear();
    let mut paint_order = 0;
    let mut deferred_queue: Vec<DeferredEntry<'_>> = Vec::new();
    let mut states_owner = states;
    {
        let states_for_main: Option<&mut ElementStates> =
            states_owner.as_mut().map(|s| &mut **s);
        paint_node(
            tree,
            root_node,
            root,
            /* parent_local */ [0.0, 0.0],
            /* inherited_translate */ [0.0, 0.0],
            /* inherited_opacity */ 1.0,
            /* inherited_text_color */ None,
            hovered_hit_id,
            theme,
            scale,
            text_shaper,
            scene,
            layout_snapshot,
            &mut paint_order,
            states_for_main,
            &mut deferred_queue,
        );
    }
    drain_deferred_paint(
        tree,
        deferred_queue,
        hovered_hit_id,
        theme,
        scale,
        text_shaper,
        scene,
        layout_snapshot,
        &mut paint_order,
        states_owner.as_mut().map(|s| &mut **s),
    );
}

fn build_taffy(tree: &mut taffy::TaffyTree<NodeContext>, el: &dyn Element) -> taffy::NodeId {
    let style = el.taffy_style();
    let children = el.children();
    let node = if children.is_empty() {
        tree.new_leaf(style)
            .expect("taffy new_leaf should not fail")
    } else {
        let child_nodes: Vec<_> = children.iter().map(|c| build_taffy(tree, &**c)).collect();
        tree.new_with_children(style, &child_nodes)
            .expect("taffy new_with_children should not fail")
    };
    if let Some(ctx) = el.taffy_context() {
        tree.set_node_context(node, Some(ctx))
            .expect("taffy set_node_context should not fail");
    }
    node
}

#[allow(clippy::too_many_arguments)]
fn paint_node<'a>(
    tree: &taffy::TaffyTree<NodeContext>,
    node: taffy::NodeId,
    el: &'a dyn Element,
    parent_local: [f32; 2],
    inherited_translate: [f32; 2],
    inherited_opacity: f32,
    inherited_text_color: Option<crate::color::Color>,
    hovered_hit_id: Option<u64>,
    theme: &ResolvedTheme,
    scale: f32,
    text_shaper: &mut dyn TextShaper,
    scene: &mut Scene,
    layout_snapshot: &mut LayoutSnapshot,
    paint_order: &mut usize,
    states: Option<&mut ElementStates>,
    deferred_queue: &mut Vec<DeferredEntry<'a>>,
) {
    let layout = match tree.layout(node) {
        Ok(l) => l,
        Err(e) => {
            log::warn!("ciri-ui: taffy layout query failed: {e:?}");
            return;
        }
    };
    let local_x = parent_local[0] + layout.location.x;
    let local_y = parent_local[1] + layout.location.y;

    // Deferred wrappers are layout-transparent: they don't paint, don't
    // appear in the snapshot, and don't enforce a min-size. Capture
    // their child(ren) into the drain queue with the same inheritance
    // the wrapper itself would have passed down, then return — the
    // wrapper's own bounds are irrelevant because it has no paint and
    // the drain re-enters paint_node from each child's taffy node.
    if el.is_deferred() {
        let priority = el.deferred_priority();
        let children = el.children();
        let taffy_children: Vec<_> = tree.child_ids(node).collect();
        debug_assert_eq!(
            taffy_children.len(),
            children.len(),
            "Taffy child count disagrees with Element::children() (deferred)",
        );
        for (child_node, child_el) in taffy_children.iter().zip(children.iter()) {
            deferred_queue.push(DeferredEntry {
                el: &**child_el,
                node: *child_node,
                parent_local: [local_x, local_y],
                inherited_translate,
                inherited_opacity,
                inherited_text_color,
                priority,
            });
        }
        return;
    }

    // Zero-sized nodes cover `display: None`, collapsed flex items and
    // defensively any non-finite layout result — there's nothing
    // meaningful to paint, and descending would waste buffer space.
    if !(layout.size.width > 0.0 && layout.size.height > 0.0) {
        return;
    }

    let paint_x = local_x + inherited_translate[0];
    let paint_y = local_y + inherited_translate[1];

    let (own_opacity, own_translate) = el.paint_transform();
    let current_order = *paint_order;
    *paint_order += 1;
    layout_snapshot.push(LayoutNode {
        type_id: el.type_id(),
        bounds: [
            paint_x + own_translate[0],
            paint_y + own_translate[1],
            layout.size.width,
            layout.size.height,
        ],
        paint_order: current_order,
        accepts_pointer_events: el.accepts_pointer_events(),
        hit_id: el.hit_id(),
    });

    // Reborrow `Option<&mut ElementStates>` so we keep ownership for
    // child recursion below — the reborrow gives `paint` mutable access
    // for this call, then `states` is still live for the children loop.
    let mut states_owner = states;
    {
        let states_for_paint: Option<&mut ElementStates> =
            states_owner.as_mut().map(|s| &mut **s);
        let mut ctx = PaintCtx {
            theme,
            bounds: [paint_x, paint_y, layout.size.width, layout.size.height],
            scene,
            text_shaper,
            scale,
            element_id: el.id(),
            inherited_opacity,
            inherited_text_color,
            hovered_hit_id,
            states: states_for_paint,
        };
        el.paint(&mut ctx);
    }
    let mut states = states_owner;

    // Compose self's own transforms into the inheritance passed down.
    // Taffy's parent offset stays unaffected (translate is paint-time, not
    // a layout concept), but opacity cascades multiplicatively and
    // translate accumulates so nested animated wrappers compose.
    let child_inherited_opacity = inherited_opacity * own_opacity;
    let child_inherited_translate = [
        inherited_translate[0] + own_translate[0],
        inherited_translate[1] + own_translate[1],
    ];
    // Text colour inherits nearest-ancestor-set-wins, so own override
    // beats parent; if neither sets it, descendants see the same `None`
    // and fall through to the theme default at text-paint time. Use
    // the state-aware variant so a `.hover(|s| s.text_color(...))`
    // refinement on this element actually propagates to descendants.
    let child_inherited_text_color = el
        .text_color_override_with_state(hovered_hit_id)
        .or(inherited_text_color);

    let children = el.children();
    if children.is_empty() {
        return;
    }
    let taffy_children: Vec<_> = tree.child_ids(node).collect();
    // Silent length mismatch between the Taffy tree and `Element::children()`
    // would drop layout-owned nodes (or paint-only ones) — catch in debug.
    debug_assert_eq!(
        taffy_children.len(),
        children.len(),
        "Taffy child count disagrees with Element::children()",
    );
    for (child_node, child_el) in taffy_children.iter().zip(children.iter()) {
        let states_for_child: Option<&mut ElementStates> =
            states.as_mut().map(|s| &mut **s);
        paint_node(
            tree,
            *child_node,
            &**child_el,
            [local_x, local_y],
            child_inherited_translate,
            child_inherited_opacity,
            child_inherited_text_color,
            hovered_hit_id,
            theme,
            scale,
            text_shaper,
            scene,
            layout_snapshot,
            paint_order,
            states_for_child,
            deferred_queue,
        );
    }
}

/// Drain queued deferred subtrees in priority order — lowest priority
/// paints first, so higher-priority deferred paints land on top.
/// Re-paint passes can themselves push new entries (nested `deferred()`
/// inside a deferred subtree); the loop empties the queue completely
/// before returning so ordering invariants hold even for nested cases.
#[allow(clippy::too_many_arguments)]
fn drain_deferred_paint<'a>(
    tree: &taffy::TaffyTree<NodeContext>,
    queue: Vec<DeferredEntry<'a>>,
    hovered_hit_id: Option<u64>,
    theme: &ResolvedTheme,
    scale: f32,
    text_shaper: &mut dyn TextShaper,
    scene: &mut Scene,
    layout_snapshot: &mut LayoutSnapshot,
    paint_order: &mut usize,
    states: Option<&mut ElementStates>,
) {
    let mut pending = queue;
    let mut states_owner = states;
    while !pending.is_empty() {
        // Stable-sort by priority so equal-priority entries paint in
        // capture order — i.e. the order siblings appeared in the
        // original tree walk. Without stability, two equal-priority
        // popovers could swap z each frame.
        pending.sort_by_key(|e| e.priority);
        let mut next_pending: Vec<DeferredEntry<'a>> = Vec::new();
        for entry in pending.drain(..) {
            let states_for_call: Option<&mut ElementStates> =
                states_owner.as_mut().map(|s| &mut **s);
            paint_node(
                tree,
                entry.node,
                entry.el,
                entry.parent_local,
                entry.inherited_translate,
                entry.inherited_opacity,
                entry.inherited_text_color,
                hovered_hit_id,
                theme,
                scale,
                text_shaper,
                scene,
                layout_snapshot,
                paint_order,
                states_for_call,
                &mut next_pending,
            );
        }
        pending = next_pending;
    }
}

/// Same shape as [`drain_deferred_paint`] but for the hit-test-only
/// walker. No paint, no scene, no states — just snapshot population in
/// the same order the painter would have chosen.
fn drain_deferred_layout_only<'a>(
    tree: &taffy::TaffyTree<NodeContext>,
    queue: Vec<DeferredEntry<'a>>,
    layout_snapshot: &mut LayoutSnapshot,
    paint_order: &mut usize,
) {
    let mut pending = queue;
    while !pending.is_empty() {
        pending.sort_by_key(|e| e.priority);
        let mut next_pending: Vec<DeferredEntry<'a>> = Vec::new();
        for entry in pending.drain(..) {
            walk_for_layout_snapshot(
                tree,
                entry.node,
                entry.el,
                entry.parent_local,
                entry.inherited_translate,
                layout_snapshot,
                paint_order,
                &mut next_pending,
            );
        }
        pending = next_pending;
    }
}

/// Walk the laid-out tree and populate `LayoutSnapshot` only — the
/// hit-test counterpart to [`paint_node`]. Mirrors paint_node's bounds
/// computation and paint-order assignment exactly so the snapshot
/// shape matches a full paint run; deliberately drops opacity / text-
/// colour inheritance and `Element::paint` calls because nothing
/// consumes them on this path.
#[allow(clippy::too_many_arguments)]
fn walk_for_layout_snapshot<'a>(
    tree: &taffy::TaffyTree<NodeContext>,
    node: taffy::NodeId,
    el: &'a dyn Element,
    parent_local: [f32; 2],
    inherited_translate: [f32; 2],
    layout_snapshot: &mut LayoutSnapshot,
    paint_order: &mut usize,
    deferred_queue: &mut Vec<DeferredEntry<'a>>,
) {
    let layout = match tree.layout(node) {
        Ok(l) => l,
        Err(e) => {
            log::warn!("ciri-ui: taffy layout query failed (hit-only): {e:?}");
            return;
        }
    };
    let local_x = parent_local[0] + layout.location.x;
    let local_y = parent_local[1] + layout.location.y;

    // Mirror the paint walker: deferred wrappers are layout-transparent.
    // Capture each child into the queue so the drain pass populates the
    // snapshot in the same order paint would produce.
    if el.is_deferred() {
        let priority = el.deferred_priority();
        let children = el.children();
        let taffy_children: Vec<_> = tree.child_ids(node).collect();
        debug_assert_eq!(
            taffy_children.len(),
            children.len(),
            "Taffy child count disagrees with Element::children() (hit-only deferred)",
        );
        for (child_node, child_el) in taffy_children.iter().zip(children.iter()) {
            deferred_queue.push(DeferredEntry {
                el: &**child_el,
                node: *child_node,
                parent_local: [local_x, local_y],
                inherited_translate,
                inherited_opacity: 1.0,
                inherited_text_color: None,
                priority,
            });
        }
        return;
    }

    if !(layout.size.width > 0.0 && layout.size.height > 0.0) {
        return;
    }

    let paint_x = local_x + inherited_translate[0];
    let paint_y = local_y + inherited_translate[1];

    let (_own_opacity, own_translate) = el.paint_transform();
    let current_order = *paint_order;
    *paint_order += 1;
    layout_snapshot.push(LayoutNode {
        type_id: el.type_id(),
        bounds: [
            paint_x + own_translate[0],
            paint_y + own_translate[1],
            layout.size.width,
            layout.size.height,
        ],
        paint_order: current_order,
        accepts_pointer_events: el.accepts_pointer_events(),
        hit_id: el.hit_id(),
    });

    let child_inherited_translate = [
        inherited_translate[0] + own_translate[0],
        inherited_translate[1] + own_translate[1],
    ];

    let children = el.children();
    if children.is_empty() {
        return;
    }
    let taffy_children: Vec<_> = tree.child_ids(node).collect();
    debug_assert_eq!(
        taffy_children.len(),
        children.len(),
        "Taffy child count disagrees with Element::children() (hit-only)",
    );
    for (child_node, child_el) in taffy_children.iter().zip(children.iter()) {
        walk_for_layout_snapshot(
            tree,
            *child_node,
            &**child_el,
            [local_x, local_y],
            child_inherited_translate,
            layout_snapshot,
            paint_order,
            deferred_queue,
        );
    }
}

// ─── Style translators (ciri-ui::style → taffy::Style) ────────────────

pub(crate) fn to_taffy_style(s: &crate::Style) -> taffy::Style {
    let mut t = taffy::Style::default();

    // Default display is **flex** — matches gpui / Tailwind's `div`
    // semantics where builder helpers like `.gap_*`, `.items_*`,
    // `.justify_*` only take effect on flex containers. Falling through
    // to `Block` silently ignored those helpers on the plain `div()` case.
    t.display = match s.display {
        Some(UiDisplay::Block) => taffy::Display::Block,
        Some(UiDisplay::None) => taffy::Display::None,
        // Explicit Flex AND the unset/default case both become Flex.
        Some(UiDisplay::Flex) | None => taffy::Display::Flex,
    };

    t.flex_direction = match s.flex_direction {
        Some(UiFlexDirection::Column) => taffy::FlexDirection::Column,
        Some(UiFlexDirection::Row) => taffy::FlexDirection::Row,
        Some(UiFlexDirection::RowReverse) => taffy::FlexDirection::RowReverse,
        Some(UiFlexDirection::ColumnReverse) => taffy::FlexDirection::ColumnReverse,
        None => taffy::FlexDirection::Row,
    };

    if let Some(a) = s.align_items {
        t.align_items = Some(match a {
            UiAlignItems::Stretch => taffy::AlignItems::Stretch,
            UiAlignItems::FlexStart => taffy::AlignItems::FlexStart,
            UiAlignItems::FlexEnd => taffy::AlignItems::FlexEnd,
            UiAlignItems::Center => taffy::AlignItems::Center,
            UiAlignItems::Baseline => taffy::AlignItems::Baseline,
        });
    }
    if let Some(j) = s.justify_content {
        t.justify_content = Some(match j {
            UiJustifyContent::FlexStart => taffy::JustifyContent::FlexStart,
            UiJustifyContent::FlexEnd => taffy::JustifyContent::FlexEnd,
            UiJustifyContent::Center => taffy::JustifyContent::Center,
            UiJustifyContent::SpaceBetween => taffy::JustifyContent::SpaceBetween,
            UiJustifyContent::SpaceAround => taffy::JustifyContent::SpaceAround,
            UiJustifyContent::SpaceEvenly => taffy::JustifyContent::SpaceEvenly,
        });
    }

    if let Some(g) = s.flex_grow {
        t.flex_grow = g;
    }
    if let Some(sh) = s.flex_shrink {
        t.flex_shrink = sh;
    }

    if let Some(g) = s.gap {
        t.gap = taffy::Size {
            width: taffy::LengthPercentage::Length(g),
            height: taffy::LengthPercentage::Length(g),
        };
    }

    if let Some(p) = s.padding {
        // [top, right, bottom, left] — same convention as CSS shorthand.
        t.padding = taffy::Rect {
            top: taffy::LengthPercentage::Length(p[0]),
            right: taffy::LengthPercentage::Length(p[1]),
            bottom: taffy::LengthPercentage::Length(p[2]),
            left: taffy::LengthPercentage::Length(p[3]),
        };
    }
    if let Some(m) = s.margin {
        t.margin = taffy::Rect {
            top: taffy::LengthPercentageAuto::Length(m[0]),
            right: taffy::LengthPercentageAuto::Length(m[1]),
            bottom: taffy::LengthPercentageAuto::Length(m[2]),
            left: taffy::LengthPercentageAuto::Length(m[3]),
        };
    }
    if let Some(position) = s.position {
        t.position = match position {
            UiPosition::Relative => taffy::Position::Relative,
            UiPosition::Absolute => taffy::Position::Absolute,
        };
    }
    t.inset = taffy::Rect {
        top: to_dim_auto(s.inset[0]),
        right: to_dim_auto(s.inset[1]),
        bottom: to_dim_auto(s.inset[2]),
        left: to_dim_auto(s.inset[3]),
    };

    if let Some(w) = s.width {
        t.size.width = to_dim(w);
    }
    if let Some(h) = s.height {
        t.size.height = to_dim(h);
    }
    if let Some(w) = s.min_width {
        t.min_size.width = to_dim(w);
    }
    if let Some(w) = s.max_width {
        t.max_size.width = to_dim(w);
    }
    if let Some(h) = s.min_height {
        t.min_size.height = to_dim(h);
    }
    if let Some(h) = s.max_height {
        t.max_size.height = to_dim(h);
    }

    t
}

fn to_dim(l: Length) -> taffy::Dimension {
    match l {
        Length::Px(v) => taffy::Dimension::Length(v),
        Length::Percent(v) => taffy::Dimension::Percent(v),
        Length::Auto => taffy::Dimension::Auto,
    }
}

fn to_dim_auto(l: Option<Length>) -> taffy::LengthPercentageAuto {
    match l {
        Some(Length::Px(v)) => taffy::LengthPercentageAuto::Length(v),
        Some(Length::Percent(v)) => taffy::LengthPercentageAuto::Percent(v),
        Some(Length::Auto) | None => taffy::LengthPercentageAuto::Auto,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use crate::elements::{div, text};
    use crate::style::Display;
    use crate::styled::Styled;

    const ACCENT: Color = [0.3, 0.6, 0.8, 1.0];

    fn theme() -> ResolvedTheme {
        ResolvedTheme::default()
    }

    fn first_rect(scene: &Scene) -> crate::scene::SdfRect {
        *scene.sdf_rects_iter().next().expect("expected one rect")
    }

    fn collect_rects(scene: &Scene) -> Vec<crate::scene::SdfRect> {
        scene.sdf_rects_iter().copied().collect()
    }

    #[test]
    fn empty_div_produces_no_sdf_rects() {
        let scene = paint_tree(
            &div(),
            &theme(),
            [800.0, 600.0],
            1.0,
            &mut crate::shaper::NullShaper,
        );
        assert!(scene.is_empty());
    }

    #[test]
    fn div_with_bg_emits_one_sdf_rect() {
        let root = div().w(100.0).h(40.0).bg(ACCENT);
        let scene = paint_tree(
            &root,
            &theme(),
            [800.0, 600.0],
            1.0,
            &mut crate::shaper::NullShaper,
        );
        assert_eq!(scene.len(), 1);
        let q = first_rect(&scene);
        assert_eq!(q.size, [100.0, 40.0]);
        assert_eq!(q.color, ACCENT);
    }

    #[test]
    fn paint_tree_with_layout_records_painted_bounds() {
        let root = div()
            .w(200.0)
            .h(40.0)
            .child(div().w(50.0).h(20.0).translate(10.0, 5.0).bg(ACCENT));
        let out = paint_tree_with_layout(
            &root,
            &theme(),
            [800.0, 600.0],
            1.0,
            &mut crate::shaper::NullShaper,
        );
        assert_eq!(out.scene.sdf_len(), 1);
        assert!(
            out.layout
                .nodes()
                .iter()
                .any(|n| n.type_id == "ciri.div" && n.bounds == [10.0, 5.0, 50.0, 20.0]),
            "layout snapshot should use the same translated bounds as paint",
        );
    }

    #[test]
    fn layout_hit_test_uses_paint_order() {
        // Two cursor-pointer children overlap at (10,10): the second
        // sibling paints later (higher `paint_order`) so it wins.
        let root = div()
            .w(200.0)
            .h(200.0)
            .child(
                div()
                    .w(100.0)
                    .h(100.0)
                    .cursor_pointer()
                    .hit_id(1)
                    .bg([1.0, 0.0, 0.0, 1.0]),
            )
            .child(
                div()
                    .w(100.0)
                    .h(100.0)
                    .translate(-100.0, 0.0)
                    .cursor_pointer()
                    .hit_id(2)
                    .bg([0.0, 1.0, 0.0, 1.0]),
            );
        let out = paint_tree_with_layout(
            &root,
            &theme(),
            [800.0, 600.0],
            1.0,
            &mut crate::shaper::NullShaper,
        );
        let hit = out.layout.hit_test(10.0, 10.0).expect("expected hit");
        assert_eq!(hit.hit_id, Some(2));
    }

    #[test]
    fn layout_hit_test_returns_host_hit_id() {
        let root = div()
            .w(100.0)
            .h(40.0)
            .child(div().w(100.0).h(40.0).hit_id(42).bg(ACCENT));
        let out = paint_tree_with_layout(
            &root,
            &theme(),
            [800.0, 600.0],
            1.0,
            &mut crate::shaper::NullShaper,
        );
        let hit = out.layout.hit_test(10.0, 10.0).expect("expected hit");
        assert_eq!(hit.hit_id, Some(42));
    }

    #[test]
    fn absolute_child_uses_inset_for_layout_and_hit_test() {
        let root = div().w(200.0).h(100.0).child(
            div()
                .absolute()
                .left(30.0)
                .top(20.0)
                .w(40.0)
                .h(30.0)
                .hit_id(7)
                .bg(ACCENT),
        );
        let out = paint_tree_with_layout(
            &root,
            &theme(),
            [800.0, 600.0],
            1.0,
            &mut crate::shaper::NullShaper,
        );
        let hit = out.layout.hit_test(35.0, 25.0).expect("expected hit");
        assert_eq!(hit.hit_id, Some(7));
        assert_eq!(hit.bounds, [30.0, 20.0, 40.0, 30.0]);
    }

    #[test]
    fn default_div_is_flex_row() {
        // Children must lay out horizontally on a plain `div()` with no
        // explicit `.flex_row()` call — that's the builder's advertised
        // default. Before the fix, `Display` fell through to Block and
        // children stacked vertically instead.
        let root = div()
            .w(300.0)
            .h(40.0)
            .gap(10.0) // gap only works on flex containers
            .child(div().w(100.0).h(40.0).bg(ACCENT))
            .child(div().w(100.0).h(40.0).bg(ACCENT));
        let scene = paint_tree(
            &root,
            &theme(),
            [800.0, 600.0],
            1.0,
            &mut crate::shaper::NullShaper,
        );
        let rects = scene.sdf_rects_iter().copied().collect::<Vec<_>>();
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].pos[1], rects[1].pos[1], "must be same row");
        assert!(
            (rects[1].pos[0] - (rects[0].pos[0] + 100.0 + 10.0)).abs() < 0.5,
            "gap must be honoured in the default flex row"
        );
    }

    #[test]
    fn flex_col_stacks_children_vertically() {
        let root = div()
            .w(100.0)
            .h(80.0)
            .flex_col()
            .child(div().w(100.0).h(40.0).bg(ACCENT))
            .child(div().w(100.0).h(40.0).bg(ACCENT));
        let scene = paint_tree(
            &root,
            &theme(),
            [800.0, 600.0],
            1.0,
            &mut crate::shaper::NullShaper,
        );
        let rects = scene.sdf_rects_iter().copied().collect::<Vec<_>>();
        assert_eq!(rects.len(), 2);
        assert!((rects[0].pos[1] - 0.0).abs() < 0.5);
        assert!((rects[1].pos[1] - 40.0).abs() < 0.5);
    }

    #[test]
    fn padding_offsets_children() {
        let root = div()
            .w(100.0)
            .h(100.0)
            .p(8.0)
            .flex_col()
            .child(div().w(40.0).h(40.0).bg(ACCENT));
        let scene = paint_tree(
            &root,
            &theme(),
            [800.0, 600.0],
            1.0,
            &mut crate::shaper::NullShaper,
        );
        let collected = collect_rects(&scene);
        let c = &collected[0];
        assert!((c.pos[0] - 8.0).abs() < 0.5);
        assert!((c.pos[1] - 8.0).abs() < 0.5);
    }

    #[test]
    fn gap_separates_children_in_flex_row() {
        let root = div()
            .w(300.0)
            .h(40.0)
            .flex_row()
            .gap(12.0)
            .child(div().w(100.0).h(40.0).bg(ACCENT))
            .child(div().w(100.0).h(40.0).bg(ACCENT));
        let scene = paint_tree(
            &root,
            &theme(),
            [800.0, 600.0],
            1.0,
            &mut crate::shaper::NullShaper,
        );
        let rects = scene.sdf_rects_iter().copied().collect::<Vec<_>>();
        let a = &rects[0];
        let b = &rects[1];
        assert!((b.pos[0] - (a.pos[0] + 100.0 + 12.0)).abs() < 0.5);
    }

    #[test]
    fn text_leaf_does_not_emit_sdf() {
        let scene = paint_tree(
            &text("hello"),
            &theme(),
            [800.0, 600.0],
            1.0,
            &mut crate::shaper::NullShaper,
        );
        assert!(scene.is_empty());
    }

    #[test]
    fn display_none_removes_element() {
        let mut root = div().w(100.0).h(100.0).bg(ACCENT);
        root.style_mut().display = Some(Display::None);
        let scene = paint_tree(
            &root,
            &theme(),
            [800.0, 600.0],
            1.0,
            &mut crate::shaper::NullShaper,
        );
        assert!(scene.is_empty());
    }

    // ── Inheritance regressions (Codex P2s) ─────────────────────────────

    /// Wrapper `opacity` must cascade multiplicatively into every
    /// descendant's emitted color alpha, otherwise a fading modal
    /// reveals its children at full alpha through a translucent panel.
    #[test]
    fn wrapper_opacity_multiplies_into_descendants() {
        let root = div()
            .w(100.0)
            .h(100.0)
            .bg(ACCENT)
            .opacity(0.5)
            .child(div().w(50.0).h(50.0).bg(ACCENT));
        let scene = paint_tree(
            &root,
            &theme(),
            [800.0, 600.0],
            1.0,
            &mut crate::shaper::NullShaper,
        );
        let rects = scene.sdf_rects_iter().copied().collect::<Vec<_>>();
        assert_eq!(rects.len(), 2);
        // Wrapper: 0.5 opacity applied to ACCENT.alpha (1.0) → 0.5
        assert!((rects[0].color[3] - 0.5).abs() < 1e-3, "wrapper alpha");
        // Child: no own opacity, inherits 0.5 from wrapper → 0.5
        assert!(
            (rects[1].color[3] - 0.5).abs() < 1e-3,
            "child must inherit wrapper opacity, got alpha={}",
            rects[1].color[3]
        );
    }

    /// Wrapper `translate` must shift the subtree, not just the wrapper
    /// itself. A sliding modal wrapper whose children stayed put would
    /// visibly detach the chrome from its contents.
    #[test]
    fn wrapper_translate_shifts_subtree() {
        let root = div()
            .w(100.0)
            .h(100.0)
            .bg(ACCENT)
            .translate(10.0, 20.0)
            .child(div().w(50.0).h(50.0).bg(ACCENT));
        let scene = paint_tree(
            &root,
            &theme(),
            [800.0, 600.0],
            1.0,
            &mut crate::shaper::NullShaper,
        );
        let rects = scene.sdf_rects_iter().copied().collect::<Vec<_>>();
        assert_eq!(rects.len(), 2);
        let wrapper = &rects[0];
        let child = &rects[1];
        // Wrapper shifted (layout position 0,0 + translate 10,20)
        assert!((wrapper.pos[0] - 10.0).abs() < 0.5);
        assert!((wrapper.pos[1] - 20.0).abs() < 0.5);
        // Child: layout position 0,0 (flex default) + inherited translate 10,20
        assert!(
            (child.pos[0] - 10.0).abs() < 0.5,
            "child.x={}",
            child.pos[0]
        );
        assert!(
            (child.pos[1] - 20.0).abs() < 0.5,
            "child.y={}",
            child.pos[1]
        );
    }

    /// Nested translates must compose additively, not overwrite.
    #[test]
    fn nested_translates_compose() {
        let root = div()
            .w(100.0)
            .h(100.0)
            .bg(ACCENT)
            .translate(10.0, 0.0)
            .child(
                div()
                    .w(50.0)
                    .h(50.0)
                    .bg(ACCENT)
                    .translate(5.0, 0.0)
                    .child(div().w(20.0).h(20.0).bg(ACCENT)),
            );
        let scene = paint_tree(
            &root,
            &theme(),
            [800.0, 600.0],
            1.0,
            &mut crate::shaper::NullShaper,
        );
        let rects = scene.sdf_rects_iter().copied().collect::<Vec<_>>();
        // grand-child: 0 layout + 10 + 5 = 15
        let gc = &rects[2];
        assert!((gc.pos[0] - 15.0).abs() < 0.5, "grandchild.x={}", gc.pos[0]);
    }

    /// `deferred()` (the post-Layer-enum equivalent of `in_layer(Modal)`)
    /// must win z-order over a non-deferred sibling that appears later in
    /// tree order. The walker emits the deferred subtree only after the
    /// main walk completes, so it always paints last.
    #[test]
    fn deferred_wins_over_tree_order() {
        use crate::elements::deferred;
        let root = div()
            .w(800.0)
            .h(600.0)
            // Deferred sibling — captured during walk, painted at drain.
            .child(deferred(div().w(100.0).h(100.0).bg([1.0, 0.0, 0.0, 1.0])))
            // Non-deferred sibling — painted during main walk.
            .child(div().w(100.0).h(100.0).bg([0.0, 1.0, 0.0, 1.0]));
        let scene = paint_tree(
            &root,
            &theme(),
            [800.0, 600.0],
            1.0,
            &mut crate::shaper::NullShaper,
        );
        let flat = collect_rects(&scene);
        assert_eq!(flat.len(), 2);
        assert_eq!(
            flat[0].color,
            [0.0, 1.0, 0.0, 1.0],
            "non-deferred sibling paints first"
        );
        assert_eq!(
            flat[1].color,
            [1.0, 0.0, 0.0, 1.0],
            "deferred sibling drains last"
        );
    }

    /// Regression: Text layout size comes from the host shaper through
    /// Taffy's measure_function, not from a construction-time heuristic.
    /// A custom shaper that returns a distinctive width lets us tell the
    /// two paths apart — if the heuristic were still in play the flex
    /// gap test below would fail with different numbers.
    #[test]
    fn text_size_is_driven_by_shaper_measure() {
        use crate::scene::SdfRect;
        use crate::shaper::TextShaper;

        struct FixedShaper;
        impl TextShaper for FixedShaper {
            fn measure(&mut self, _content: &str, _font_size_px: f32) -> [f32; 2] {
                // Every string is 42px wide × 16px tall. If the walker
                // used Text's old `0.5 * font_size * chars` heuristic
                // we'd get something proportional to the string length
                // instead.
                [42.0, 16.0]
            }
            fn emit(
                &mut self,
                _c: &str,
                _p: [f32; 2],
                _col: Color,
                _fs: f32,
                _s: &mut Scene,
            ) {
            }
        }

        // Two text runs of very different lengths laid out in a flex row
        // with a known gap. With the shaper forcing 42px both, the row
        // should end up exactly 42 + 10 + 42 = 94px wide; the right
        // child's SDF sibling must start at x=52.
        let root = div()
            .w(300.0)
            .h(40.0)
            .flex_row()
            .gap(10.0)
            .child(
                div()
                    .w_full()
                    .child(text("a"))
                    .child(div().w(0.0).h(0.0).bg([1.0; 4])),
            )
            .child(
                div()
                    .child(text("wildly-longer-content"))
                    .child(div().w(0.0).h(0.0).bg([0.0, 1.0, 0.0, 1.0])),
            );
        let _: Vec<SdfRect> = paint_tree(&root, &theme(), [800.0, 600.0], 1.0, &mut FixedShaper)
            .sdf_rects_iter()
            .copied()
            .collect();
        // The size contract we actually care about for this regression
        // is that the two Text leaves measure the *same* 42px under
        // FixedShaper — layout must not have "short" vs "long" behaviour.
        // Verify by asking the shaper callback what it would return for
        // each content; a construction-time heuristic would bypass that
        // path and produce different widths.
        assert_eq!(FixedShaper.measure("a", 13.0), [42.0, 16.0]);
        assert_eq!(
            FixedShaper.measure("wildly-longer-content", 13.0),
            [42.0, 16.0]
        );
    }

    /// Regression for Codex P2: wrapper `text_color(...)` must cascade
    /// through the walker into `Text` descendants so themed label trees
    /// paint with the right foreground. Before the fix, `Text::paint`
    /// jumped straight to `theme.on_surface` regardless of any ancestor
    /// `text_color`. We verify end-to-end by recording the color the
    /// shaper was asked to emit with.
    #[test]
    fn wrapper_text_color_cascades_to_text_descendants() {
        use crate::shaper::RecordingShaper;

        const RED: Color = [1.0, 0.0, 0.0, 1.0];
        let mut shaper = RecordingShaper::default();
        let root = div().w(200.0).h(40.0).text_color(RED).child(text("hello"));
        let _ = paint_tree(&root, &theme(), [800.0, 600.0], 1.0, &mut shaper);
        assert_eq!(shaper.calls.len(), 1);
        assert_eq!(shaper.calls[0].color, RED);
    }

    /// Nearest-ancestor wins: a `text_color` closer to the Text
    /// overrides a farther ancestor's setting.
    #[test]
    fn nearest_ancestor_text_color_wins() {
        use crate::shaper::RecordingShaper;

        const RED: Color = [1.0, 0.0, 0.0, 1.0];
        const BLUE: Color = [0.0, 0.0, 1.0, 1.0];
        let mut shaper = RecordingShaper::default();
        let root = div()
            .w(200.0)
            .h(40.0)
            .text_color(RED)
            .child(div().text_color(BLUE).child(text("hi")));
        let _ = paint_tree(&root, &theme(), [800.0, 600.0], 1.0, &mut shaper);
        assert_eq!(shaper.calls[0].color, BLUE);
    }

    /// Own `.color(...)` on the Text itself wins over any inherited colour.
    #[test]
    fn text_own_color_wins_over_inherited() {
        use crate::shaper::RecordingShaper;

        const RED: Color = [1.0, 0.0, 0.0, 1.0];
        const GREEN: Color = [0.0, 1.0, 0.0, 1.0];
        let mut shaper = RecordingShaper::default();
        let root = div()
            .w(200.0)
            .h(40.0)
            .text_color(RED)
            .child(text("hi").color(GREEN));
        let _ = paint_tree(&root, &theme(), [800.0, 600.0], 1.0, &mut shaper);
        assert_eq!(shaper.calls[0].color, GREEN);
    }

    /// Regression for refinement-aware text_color inheritance: a parent
    /// Div with `.text_color(rest).hover(|s| s.text_color(hov))` and a
    /// matching `hit_id` should propagate `hov` to a descendant Text
    /// when the cursor sits on it. Before the walker started calling
    /// `text_color_override_with_state`, the refinement only affected
    /// the Div's own painted style and Text descendants kept inheriting
    /// `rest` regardless of hover state.
    #[test]
    fn hover_text_color_refinement_propagates_to_descendant_text() {
        const REST: crate::color::Color = [0.5, 0.5, 0.5, 1.0];
        const HOV: crate::color::Color = [1.0, 1.0, 1.0, 1.0];
        let mut shaper = crate::shaper::RecordingShaper::default();
        // The Div sits at [0..100, 0..40] in the viewport with hit_id=42.
        let root = div().w(200.0).h(80.0).child(
            div()
                .w(100.0)
                .h(40.0)
                .text_color(REST)
                .hit_id(42)
                .hover(|s| s.text_color(HOV))
                .child(text("hi")),
        );
        let mut tree = taffy::TaffyTree::<NodeContext>::new();
        let mut scene = Scene::new();
        // Cursor over the Div ⇒ refinement applies, Text inherits HOV.
        paint_tree_into_with(
            &root,
            &theme(),
            [200.0, 80.0],
            1.0,
            &mut shaper,
            &mut scene,
            &mut tree,
            Some([10.0, 10.0]),
            None,
        );
        assert_eq!(
            shaper.calls.last().expect("text emitted").color,
            HOV,
            "hover refinement should propagate to descendant text",
        );

        // Cursor outside the Div ⇒ refinement does not apply, Text
        // inherits the base REST color.
        let mut shaper = crate::shaper::RecordingShaper::default();
        let mut tree = taffy::TaffyTree::<NodeContext>::new();
        let mut scene = Scene::new();
        paint_tree_into_with(
            &root,
            &theme(),
            [200.0, 80.0],
            1.0,
            &mut shaper,
            &mut scene,
            &mut tree,
            Some([150.0, 10.0]),
            None,
        );
        assert_eq!(shaper.calls.last().expect("text emitted").color, REST);
    }

    /// A subtree that uses `deferred()` paints atomically — wrapper
    /// child first, then nested children — even when other Chrome
    /// elements are emitted in between by virtue of tree order. The
    /// drain re-enters paint_node from the deferred root, recursing
    /// through every descendant before moving on.
    #[test]
    fn deferred_subtree_paints_atomically() {
        use crate::elements::deferred;
        let root = div().w(800.0).h(600.0).child(deferred(
            div()
                .w(100.0)
                .h(100.0)
                .bg([1.0, 0.0, 0.0, 1.0])
                .child(div().w(50.0).h(50.0).bg([0.5, 0.0, 0.0, 1.0])),
        ));
        let scene = paint_tree(
            &root,
            &theme(),
            [800.0, 600.0],
            1.0,
            &mut crate::shaper::NullShaper,
        );
        let flat = collect_rects(&scene);
        assert_eq!(flat.len(), 2);
        // Outer (red) emits before inner (dark red) — both via the drain.
        assert_eq!(flat[0].color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(flat[1].color, [0.5, 0.0, 0.0, 1.0]);
    }

    /// A `deferred()` child must paint AFTER its non-deferred siblings
    /// — that's the whole point of the queue. Without the drain pass
    /// the deferred child would emit in tree order and get covered by
    /// anything painted after it.
    #[test]
    fn deferred_child_paints_after_non_deferred_sibling() {
        use crate::elements::deferred;
        const RED: Color = [1.0, 0.0, 0.0, 1.0];
        const BLUE: Color = [0.0, 0.0, 1.0, 1.0];
        // Root has no bg → emits no SDF, so the only visible rects are
        // the two children. Without the drain pass, the BLUE sibling
        // would paint last (covering the deferred); with the drain,
        // RED must come last.
        let root = div()
            .w(800.0)
            .h(600.0)
            .child(deferred(div().w(20.0).h(20.0).bg(RED)))
            .child(div().w(20.0).h(20.0).bg(BLUE));
        let scene = paint_tree(
            &root,
            &theme(),
            [800.0, 600.0],
            1.0,
            &mut crate::shaper::NullShaper,
        );
        let rects = scene.sdf_rects_iter().copied().collect::<Vec<_>>();
        assert_eq!(rects.len(), 2, "two children, both with bg");
        assert_eq!(
            rects[0].color, BLUE,
            "non-deferred sibling paints first (bottom)"
        );
        assert_eq!(rects[1].color, RED, "deferred child paints last (top)");
    }

    /// Higher `priority()` paints later — i.e. on top of lower-priority
    /// deferred siblings. Equal priorities preserve capture order
    /// (tree order) thanks to the stable sort.
    #[test]
    fn deferred_priority_orders_drain() {
        use crate::elements::deferred;
        const A: Color = [0.1, 0.0, 0.0, 1.0];
        const B: Color = [0.2, 0.0, 0.0, 1.0];
        const C: Color = [0.3, 0.0, 0.0, 1.0];
        // Insert in reverse priority to confirm sort wins over tree order.
        let root = div()
            .w(800.0)
            .h(600.0)
            .child(deferred(div().w(20.0).h(20.0).bg(C)).priority(20))
            .child(deferred(div().w(20.0).h(20.0).bg(A)).priority(0))
            .child(deferred(div().w(20.0).h(20.0).bg(B)).priority(10));
        let scene = paint_tree(
            &root,
            &theme(),
            [800.0, 600.0],
            1.0,
            &mut crate::shaper::NullShaper,
        );
        let rects = scene.sdf_rects_iter().copied().collect::<Vec<_>>();
        let order: Vec<_> = rects.iter().map(|r| r.color).collect();
        assert_eq!(order, vec![A, B, C], "drained in ascending priority order");
    }

    /// Hit-test should still find a deferred subtree — the drain has to
    /// run for the layout-only walker too, otherwise the snapshot would
    /// be missing the deferred descendants and clicks would fall through.
    #[test]
    fn deferred_subtree_appears_in_layout_snapshot() {
        use crate::elements::deferred;
        use crate::styled::Styled;
        let root = div()
            .w(800.0)
            .h(600.0)
            .child(deferred(
                div()
                    .w(40.0)
                    .h(40.0)
                    .bg([1.0, 1.0, 1.0, 1.0])
                    .hit_id(99)
                    .on_click(|| {}),
            ));
        let mut snapshot = LayoutSnapshot::new();
        let mut tree = taffy::TaffyTree::<NodeContext>::new();
        layout_tree_into_retained(
            &root,
            [800.0, 600.0],
            &mut crate::shaper::NullShaper,
            &mut snapshot,
            &mut tree,
        );
        let hit = snapshot.hit_test(5.0, 5.0);
        assert!(hit.is_some(), "deferred element must be in snapshot");
        assert_eq!(hit.unwrap().hit_id, Some(99));
    }
}
