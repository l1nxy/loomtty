//! Paint-pipeline glue: build a Taffy tree from an `Element` tree, run
//! layout, then walk both in parallel to emit primitives.
//!
//! Design note: the host may call `paint_tree` once per dirty frame.
//! Taffy is cheap (microseconds for moderate trees) but the API is
//! allocation-heavy; both the Taffy tree and the side-car element list
//! are discarded at the end of the call. When the retained cache lands
//! in a follow-up PR these allocations move out of the hot path.

use taffy::TraversePartialTree;

use crate::element::{Element, PaintCtx};
use crate::scene::Scene;
use crate::style::{
    AlignItems as UiAlignItems, Display as UiDisplay, FlexDirection as UiFlexDirection,
    JustifyContent as UiJustifyContent, Length,
};
use crate::theme::ResolvedTheme;

/// Layout and paint an element tree into a [`Scene`].
///
/// `viewport` is the logical-pixel size of the painting area (usually the
/// window content size divided by `scale`). `scale` is the device-pixel
/// ratio; stored on `PaintCtx` so elements can round to physical pixels
/// if they need to. Taffy always works in logical pixels.
///
/// Returns a fresh [`Scene`]. Callers that want to amortise the
/// allocation can use [`paint_tree_into`].
pub fn paint_tree(
    root: &dyn Element,
    theme: &ResolvedTheme,
    viewport: [f32; 2],
    scale: f32,
) -> Scene {
    let mut scene = Scene::new();
    paint_tree_into(root, theme, viewport, scale, &mut scene);
    scene
}

/// Same as [`paint_tree`] but appends into an existing scene.
pub fn paint_tree_into(
    root: &dyn Element,
    theme: &ResolvedTheme,
    viewport: [f32; 2],
    scale: f32,
    scene: &mut Scene,
) {
    let mut tree = taffy::TaffyTree::<()>::new();
    let root_node = build_taffy(&mut tree, root);

    let available = taffy::Size {
        width: taffy::AvailableSpace::Definite(viewport[0].max(0.0)),
        height: taffy::AvailableSpace::Definite(viewport[1].max(0.0)),
    };
    if let Err(e) = tree.compute_layout(root_node, available) {
        log::warn!("ciri-ui: taffy compute_layout failed: {e:?}");
        return;
    }
    paint_node(&tree, root_node, root, [0.0, 0.0], theme, scale, scene);
}

fn build_taffy(tree: &mut taffy::TaffyTree<()>, el: &dyn Element) -> taffy::NodeId {
    let style = el.taffy_style();
    let children = el.children();
    if children.is_empty() {
        tree.new_leaf(style)
            .expect("taffy new_leaf should not fail")
    } else {
        let child_nodes: Vec<_> = children
            .iter()
            .map(|c| build_taffy(tree, &**c))
            .collect();
        tree.new_with_children(style, &child_nodes)
            .expect("taffy new_with_children should not fail")
    }
}

fn paint_node(
    tree: &taffy::TaffyTree<()>,
    node: taffy::NodeId,
    el: &dyn Element,
    parent_offset: [f32; 2],
    theme: &ResolvedTheme,
    scale: f32,
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
    let abs_x = parent_offset[0] + layout.location.x;
    let abs_y = parent_offset[1] + layout.location.y;
    let bounds = [abs_x, abs_y, layout.size.width, layout.size.height];

    let mut ctx = PaintCtx {
        theme,
        bounds,
        scene,
        scale,
        element_id: Default::default(),
    };
    el.paint(&mut ctx);

    let children = el.children();
    if children.is_empty() {
        return;
    }
    let taffy_children: Vec<_> = tree.child_ids(node).collect();
    for (child_node, child_el) in taffy_children.iter().zip(children.iter()) {
        paint_node(
            tree,
            *child_node,
            &**child_el,
            [abs_x, abs_y],
            theme,
            scale,
            scene,
        );
    }
}

// ─── Style translators (ciri-ui::style → taffy::Style) ────────────────

pub(crate) fn to_taffy_style(s: &crate::Style) -> taffy::Style {
    let mut t = taffy::Style::default();

    t.display = match s.display {
        Some(UiDisplay::Flex) => taffy::Display::Flex,
        Some(UiDisplay::None) => taffy::Display::None,
        _ => taffy::Display::Block,
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

    #[test]
    fn empty_div_produces_no_sdf_rects() {
        let scene = paint_tree(&div(), &theme(), [800.0, 600.0], 1.0);
        assert!(scene.is_empty());
    }

    #[test]
    fn div_with_bg_emits_one_sdf_rect() {
        let root = div().w(100.0).h(40.0).bg(ACCENT);
        let scene = paint_tree(&root, &theme(), [800.0, 600.0], 1.0);
        assert_eq!(scene.sdf_rects.len(), 1);
        let q = &scene.sdf_rects[0];
        assert_eq!(q.size, [100.0, 40.0]);
        assert_eq!(q.color, ACCENT);
    }

    #[test]
    fn flex_row_lays_out_children_horizontally() {
        let root = div()
            .w(300.0)
            .h(40.0)
            .flex_row()
            .child(div().w(100.0).h(40.0).bg(ACCENT))
            .child(div().w(100.0).h(40.0).bg(ACCENT));
        let scene = paint_tree(&root, &theme(), [800.0, 600.0], 1.0);
        assert_eq!(scene.sdf_rects.len(), 2);
        let a = &scene.sdf_rects[0];
        let b = &scene.sdf_rects[1];
        assert!((a.pos[0] - 0.0).abs() < 0.5, "a.x={}", a.pos[0]);
        assert!((b.pos[0] - 100.0).abs() < 0.5, "b.x={}", b.pos[0]);
        assert_eq!(a.pos[1], b.pos[1]);
    }

    #[test]
    fn flex_col_stacks_children_vertically() {
        let root = div()
            .w(100.0)
            .h(80.0)
            .flex_col()
            .child(div().w(100.0).h(40.0).bg(ACCENT))
            .child(div().w(100.0).h(40.0).bg(ACCENT));
        let scene = paint_tree(&root, &theme(), [800.0, 600.0], 1.0);
        assert_eq!(scene.sdf_rects.len(), 2);
        let a = &scene.sdf_rects[0];
        let b = &scene.sdf_rects[1];
        assert!((a.pos[1] - 0.0).abs() < 0.5);
        assert!((b.pos[1] - 40.0).abs() < 0.5, "b.y={}", b.pos[1]);
    }

    #[test]
    fn padding_offsets_children() {
        let root = div()
            .w(100.0)
            .h(100.0)
            .p(8.0)
            .flex_col()
            .child(div().w(40.0).h(40.0).bg(ACCENT));
        let scene = paint_tree(&root, &theme(), [800.0, 600.0], 1.0);
        assert_eq!(scene.sdf_rects.len(), 1);
        let c = &scene.sdf_rects[0];
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
        let scene = paint_tree(&root, &theme(), [800.0, 600.0], 1.0);
        let a = &scene.sdf_rects[0];
        let b = &scene.sdf_rects[1];
        assert!((b.pos[0] - (a.pos[0] + 100.0 + 12.0)).abs() < 0.5);
    }

    #[test]
    fn text_leaf_does_not_emit_sdf() {
        // Paint pass for Text is a no-op in PR-3a; glyph emission lands
        // with the UiTextShaper bridge in the follow-up PR. This test
        // locks that contract so the follow-up can remove it deliberately.
        let scene = paint_tree(&text("hello"), &theme(), [800.0, 600.0], 1.0);
        assert!(scene.is_empty());
    }

    #[test]
    fn display_none_removes_element() {
        let mut root = div().w(100.0).h(100.0).bg(ACCENT);
        root.style_mut().display = Some(Display::None);
        let scene = paint_tree(&root, &theme(), [800.0, 600.0], 1.0);
        assert!(scene.is_empty());
    }
}
