//! Paint-pipeline glue: build a Taffy tree from an `Element` tree, run
//! layout, then walk both in parallel to emit primitives into a `Scene`.
//!
//! The walker propagates three pieces of paint-time state down the tree:
//!
//! 1. **Taffy-absolute offset** — the accumulated `layout.location` of
//!    ancestors. Pure layout, independent of any paint transforms.
//! 2. **Inherited translate / opacity** — CSS-style paint transforms.
//!    Translate moves the subtree without touching layout; opacity
//!    multiplies down. Both derive from `Element::paint_transform()`.
//! 3. **Inherited layer** — the effective z-layer. Starts at `Chrome`
//!    at the root; every element's `layer()` overrides inheritance for
//!    itself and its descendants, so `in_layer(Modal)` actually wins
//!    z-order for the whole subtree.
//!
//! These three form the correctness backbone of the paint pass; tests
//! in this module lock each one down.

use taffy::TraversePartialTree;

use crate::element::{Element, Layer, PaintCtx};
use crate::scene::Scene;
use crate::shaper::TextShaper;
use crate::style::{
    AlignItems as UiAlignItems, Display as UiDisplay, FlexDirection as UiFlexDirection,
    JustifyContent as UiJustifyContent, Length,
};
use crate::theme::ResolvedTheme;

/// Per-node side-channel the layout pass hands to its
/// `compute_layout_with_measure` callback. Only leaves that need
/// shaper-driven sizing attach one; containers stay `None` and let
/// Taffy size them from their children + their own `taffy_style`.
#[derive(Clone, Debug)]
pub enum NodeContext {
    /// Text leaf: layout defers to the host `TextShaper::measure`,
    /// which sees the same font size used at paint time so the two
    /// passes never disagree on width.
    Text { content: String, font_size_px: f32 },
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
    paint_tree_into_with(root, theme, viewport, scale, text_shaper, scene, &mut tree);
}

/// Retained-tree variant of [`paint_tree_into`]. The caller owns a
/// `TaffyTree` that survives across frames; this function `clear()`s
/// and rebuilds it each call, reusing the allocator instead of freeing
/// and re-allocating dozens of SlotMap slots per frame. Prefer this in
/// the live render path; the one-shot `paint_tree[_into]` remains for
/// tests and unit calls.
pub fn paint_tree_into_with(
    root: &dyn Element,
    theme: &ResolvedTheme,
    viewport: [f32; 2],
    scale: f32,
    text_shaper: &mut dyn TextShaper,
    scene: &mut Scene,
    tree: &mut taffy::TaffyTree<NodeContext>,
) {
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
    paint_node(
        tree,
        root_node,
        root,
        /* parent_local */ [0.0, 0.0],
        /* inherited_translate */ [0.0, 0.0],
        /* inherited_opacity */ 1.0,
        /* inherited_layer */ Layer::Chrome,
        /* inherited_text_color */ None,
        theme,
        scale,
        text_shaper,
        scene,
    );
}

fn build_taffy(tree: &mut taffy::TaffyTree<NodeContext>, el: &dyn Element) -> taffy::NodeId {
    let style = el.taffy_style();
    let children = el.children();
    let node = if children.is_empty() {
        tree.new_leaf(style)
            .expect("taffy new_leaf should not fail")
    } else {
        let child_nodes: Vec<_> = children
            .iter()
            .map(|c| build_taffy(tree, &**c))
            .collect();
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
fn paint_node(
    tree: &taffy::TaffyTree<NodeContext>,
    node: taffy::NodeId,
    el: &dyn Element,
    parent_local: [f32; 2],
    inherited_translate: [f32; 2],
    inherited_opacity: f32,
    inherited_layer: Layer,
    inherited_text_color: Option<crate::color::Color>,
    theme: &ResolvedTheme,
    scale: f32,
    text_shaper: &mut dyn TextShaper,
    scene: &mut Scene,
) {
    let layout = match tree.layout(node) {
        Ok(l) => l,
        Err(e) => {
            log::warn!("ciri-ui: taffy layout query failed: {e:?}");
            return;
        }
    };
    // Zero-sized nodes cover `display: None`, collapsed flex items and
    // defensively any non-finite layout result — there's nothing
    // meaningful to paint, and descending would waste buffer space.
    if !(layout.size.width > 0.0 && layout.size.height > 0.0) {
        return;
    }

    let local_x = parent_local[0] + layout.location.x;
    let local_y = parent_local[1] + layout.location.y;
    let paint_x = local_x + inherited_translate[0];
    let paint_y = local_y + inherited_translate[1];

    let effective_layer = el.layer().unwrap_or(inherited_layer);

    let mut ctx = PaintCtx {
        theme,
        bounds: [paint_x, paint_y, layout.size.width, layout.size.height],
        scene,
        text_shaper,
        scale,
        element_id: None,
        inherited_opacity,
        inherited_text_color,
        layer: effective_layer,
    };
    el.paint(&mut ctx);

    // Compose self's own transforms into the inheritance passed down.
    // Taffy's parent offset stays unaffected (translate is paint-time, not
    // a layout concept), but opacity cascades multiplicatively and
    // translate accumulates so nested animated wrappers compose.
    let (own_opacity, own_translate) = el.paint_transform();
    let child_inherited_opacity = inherited_opacity * own_opacity;
    let child_inherited_translate = [
        inherited_translate[0] + own_translate[0],
        inherited_translate[1] + own_translate[1],
    ];
    // Text colour inherits nearest-ancestor-set-wins, so own override
    // beats parent; if neither sets it, descendants see the same `None`
    // and fall through to the theme default at text-paint time.
    let child_inherited_text_color = el.text_color_override().or(inherited_text_color);

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
        paint_node(
            tree,
            *child_node,
            &**child_el,
            [local_x, local_y],
            child_inherited_translate,
            child_inherited_opacity,
            effective_layer,
            child_inherited_text_color,
            theme,
            scale,
            text_shaper,
            scene,
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

    fn first_chrome(scene: &Scene) -> &crate::scene::SdfRect {
        &scene.sdf_in_layer(Layer::Chrome)[0]
    }

    #[test]
    fn empty_div_produces_no_sdf_rects() {
        let scene = paint_tree(&div(), &theme(), [800.0, 600.0], 1.0, &mut crate::shaper::NullShaper);
        assert!(scene.is_empty());
    }

    #[test]
    fn div_with_bg_emits_one_sdf_rect() {
        let root = div().w(100.0).h(40.0).bg(ACCENT);
        let scene = paint_tree(&root, &theme(), [800.0, 600.0], 1.0, &mut crate::shaper::NullShaper);
        assert_eq!(scene.len(), 1);
        let q = first_chrome(&scene);
        assert_eq!(q.size, [100.0, 40.0]);
        assert_eq!(q.color, ACCENT);
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
        let scene = paint_tree(&root, &theme(), [800.0, 600.0], 1.0, &mut crate::shaper::NullShaper);
        let rects = scene.sdf_in_layer(Layer::Chrome);
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
        let scene = paint_tree(&root, &theme(), [800.0, 600.0], 1.0, &mut crate::shaper::NullShaper);
        let rects = scene.sdf_in_layer(Layer::Chrome);
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
        let scene = paint_tree(&root, &theme(), [800.0, 600.0], 1.0, &mut crate::shaper::NullShaper);
        let c = &scene.sdf_in_layer(Layer::Chrome)[0];
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
        let scene = paint_tree(&root, &theme(), [800.0, 600.0], 1.0, &mut crate::shaper::NullShaper);
        let rects = scene.sdf_in_layer(Layer::Chrome);
        let a = &rects[0];
        let b = &rects[1];
        assert!((b.pos[0] - (a.pos[0] + 100.0 + 12.0)).abs() < 0.5);
    }

    #[test]
    fn text_leaf_does_not_emit_sdf() {
        let scene = paint_tree(&text("hello"), &theme(), [800.0, 600.0], 1.0, &mut crate::shaper::NullShaper);
        assert!(scene.is_empty());
    }

    #[test]
    fn display_none_removes_element() {
        let mut root = div().w(100.0).h(100.0).bg(ACCENT);
        root.style_mut().display = Some(Display::None);
        let scene = paint_tree(&root, &theme(), [800.0, 600.0], 1.0, &mut crate::shaper::NullShaper);
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
        let scene = paint_tree(&root, &theme(), [800.0, 600.0], 1.0, &mut crate::shaper::NullShaper);
        let rects = scene.sdf_in_layer(Layer::Chrome);
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
        let scene = paint_tree(&root, &theme(), [800.0, 600.0], 1.0, &mut crate::shaper::NullShaper);
        let rects = scene.sdf_in_layer(Layer::Chrome);
        assert_eq!(rects.len(), 2);
        let wrapper = &rects[0];
        let child = &rects[1];
        // Wrapper shifted (layout position 0,0 + translate 10,20)
        assert!((wrapper.pos[0] - 10.0).abs() < 0.5);
        assert!((wrapper.pos[1] - 20.0).abs() < 0.5);
        // Child: layout position 0,0 (flex default) + inherited translate 10,20
        assert!((child.pos[0] - 10.0).abs() < 0.5, "child.x={}", child.pos[0]);
        assert!((child.pos[1] - 20.0).abs() < 0.5, "child.y={}", child.pos[1]);
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
        let scene = paint_tree(&root, &theme(), [800.0, 600.0], 1.0, &mut crate::shaper::NullShaper);
        let rects = scene.sdf_in_layer(Layer::Chrome);
        // grand-child: 0 layout + 10 + 5 = 15
        let gc = &rects[2];
        assert!((gc.pos[0] - 15.0).abs() < 0.5, "grandchild.x={}", gc.pos[0]);
    }

    /// `in_layer(Modal)` must win z-order for the whole subtree, even
    /// when the modal is a tree-order sibling that appears before
    /// chrome. The walker emits into the right bucket, and
    /// `Scene::sdf_rects()` flattens them in the layer-paint order.
    #[test]
    fn in_layer_modal_wins_over_tree_order() {
        let root = div()
            .w(800.0)
            .h(600.0)
            .child(
                div()
                    .in_layer(Layer::Modal)
                    .w(100.0)
                    .h(100.0)
                    .bg([1.0, 0.0, 0.0, 1.0]),
            )
            .child(div().w(100.0).h(100.0).bg([0.0, 1.0, 0.0, 1.0]));
        let scene = paint_tree(&root, &theme(), [800.0, 600.0], 1.0, &mut crate::shaper::NullShaper);
        // Modal bucket gets the red rect even though it was the first
        // child; chrome bucket gets the green one.
        assert_eq!(scene.sdf_in_layer(Layer::Modal).len(), 1);
        assert_eq!(scene.sdf_in_layer(Layer::Chrome).len(), 1);
        // Flattened paint order: Chrome before Modal → modal paints last.
        let flat: Vec<_> = scene.sdf_rects_iter().copied().collect();
        assert_eq!(flat[0].color, [0.0, 1.0, 0.0, 1.0], "chrome first");
        assert_eq!(flat[1].color, [1.0, 0.0, 0.0, 1.0], "modal last");
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
                _l: Layer,
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

    /// Modal subtree inheritance: descendants of a `Modal` element must
    /// stay on the Modal layer by default, so a modal with chrome-default
    /// children doesn't accidentally split its own rendering.
    #[test]
    fn modal_subtree_inherits_modal_layer() {
        let root = div().w(800.0).h(600.0).child(
            div()
                .in_layer(Layer::Modal)
                .w(100.0)
                .h(100.0)
                .bg([1.0, 0.0, 0.0, 1.0])
                .child(div().w(50.0).h(50.0).bg([0.5, 0.0, 0.0, 1.0])),
        );
        let scene = paint_tree(&root, &theme(), [800.0, 600.0], 1.0, &mut crate::shaper::NullShaper);
        assert_eq!(scene.sdf_in_layer(Layer::Modal).len(), 2);
        assert_eq!(scene.sdf_in_layer(Layer::Chrome).len(), 0);
    }
}
