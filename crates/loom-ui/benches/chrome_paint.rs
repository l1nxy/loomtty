//! Chrome paint pipeline benchmarks.
//!
//! Measures the cost of `paint_tree_into_retained` on representative
//! chrome trees. The numbers feed into perf decisions about
//! [`crate::Style`] → `taffy::Style` translation, arena layout, and
//! per-paint allocator pressure.
//!
//! Run all benches: `cargo bench -p loom-ui --bench chrome_paint`.
//! Run one group:  `cargo bench -p loom-ui --bench chrome_paint -- palette_paint`.
//! Run one bench:  `cargo bench -p loom-ui --bench chrome_paint -- palette_paint/rows_30`.

use loom_ui::{
    Arena, ElementArenaScope, FluentBuilder, IntoElement, ResolvedTheme, Scene, Styled, deferred,
    div, paint_tree_into_retained, shaper::NullShaper, text,
};
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use std::cell::RefCell;

/// Build a palette-like tree with `n_rows` selectable rows. Mirrors the
/// shape (but not the styling fidelity) of `PaletteComponent::build_tree`
/// — a backdrop + deferred panel containing an input row, a separator,
/// the row list, and a footer.
fn build_palette_tree(n_rows: usize) -> impl IntoElement {
    let bg = [0.05, 0.05, 0.05, 0.95];
    let panel_bg = [0.1, 0.1, 0.12, 1.0];
    let row_bg = [0.12, 0.12, 0.14, 1.0];
    let row_hover = [0.18, 0.18, 0.22, 1.0];
    let accent = [0.4, 0.6, 1.0, 1.0];
    let dim = [0.6, 0.6, 0.6, 1.0];

    let panel_w = 600.0_f32;
    let panel_h = 480.0_f32;
    let row_h = 24.0_f32;

    let mut panel = div()
        .absolute()
        .left(100.0)
        .top(50.0)
        .w(panel_w)
        .h(panel_h)
        .flex_col()
        .bg(panel_bg)
        .rounded(4.0)
        .border(1.0, accent)
        .shadow_lg()
        .child(
            div()
                .w(panel_w - 8.0)
                .h(row_h * 1.5)
                .flex_row()
                .items_center()
                .child(div().w(8.0).h(row_h))
                .child(text("query goes here").color(accent)),
        )
        .child(div().w(panel_w - 8.0).h(1.0).bg(dim));
    for i in 0..n_rows {
        let is_selected = i == 5;
        let row = div()
            .w(panel_w - 8.0)
            .h(row_h)
            .flex_row()
            .items_center()
            .hit_id(1000 + i as u64)
            .cursor_pointer()
            .child(div().w(8.0).h(row_h))
            .child(text(format!("Entry {}", i)).color(if is_selected { accent } else { dim }))
            .when(is_selected, |d| d.bg(row_bg))
            .hover(move |s| s.bg(row_hover));
        panel = panel.child(row);
    }
    panel = panel.child(
        div()
            .w(panel_w - 8.0)
            .h(row_h)
            .flex_row()
            .items_center()
            .justify_end()
            .child(text(format!("{}/{}", 6, n_rows)).color(dim))
            .child(div().w(12.0).h(row_h)),
    );

    div()
        .w(1280.0)
        .h(720.0)
        .bg(bg)
        .hit_id(1)
        .child(deferred(panel))
}

fn bench_palette(c: &mut Criterion) {
    let theme = ResolvedTheme::default();
    let viewport = [1280.0, 720.0];
    let arena = RefCell::new(Arena::new(1024 * 1024));
    let mut group = c.benchmark_group("palette_paint");

    for &n in &[10usize, 30, 100] {
        group.bench_function(format!("rows_{}", n), |b| {
            let mut scene = Scene::new();
            let mut snapshot = loom_ui::LayoutSnapshot::new();
            let mut tree = taffy::TaffyTree::<loom_ui::NodeContext>::new();
            let mut shaper = NullShaper;
            b.iter(|| {
                arena.borrow_mut().clear();
                let _scope = ElementArenaScope::enter(&arena);
                let root = build_palette_tree(black_box(n)).into_element();
                scene.clear();
                snapshot.clear();
                paint_tree_into_retained(
                    &root,
                    &theme,
                    viewport,
                    1.0,
                    &mut shaper,
                    &mut scene,
                    &mut snapshot,
                    &mut tree,
                    None,
                    None,
                    None,
                );
                black_box(scene.sdf_len());
            });
        });
    }

    group.finish();
}

/// Isolates the cost of `to_taffy_style` translation by recursively
/// calling `Element::taffy_style` on every node in a palette tree.
/// Co-measures `children()` iteration and the trivial default-style
/// calls on `Text` / `Deferred` leaves — but the walker does the same
/// work in production, so this is a conservative upper bound on per-
/// frame translation cost. Use it to validate §1.3 #6 (whether
/// memoization is worth adding storage to every `Div`).
fn bench_taffy_style_translation(c: &mut Criterion) {
    let arena = RefCell::new(Arena::new(1024 * 1024));
    let _scope = ElementArenaScope::enter(&arena);

    // Build once outside the bench iter so we measure only translation,
    // not arena / element-tree construction.
    let root = build_palette_tree(30).into_element();

    c.bench_function("taffy_style_call_per_div_palette_30", |b| {
        b.iter(|| {
            walk(&root);
        });
    });

    fn walk(el: &dyn loom_ui::Element) {
        // `black_box(style)` keeps the full taffy::Style value live so
        // the optimizer cannot DCE the `to_taffy_style` translation.
        // `size_of_val(&style)` would only observe the type's static
        // size and let the call be elided.
        let style = el.taffy_style();
        black_box(style);
        for child in el.children() {
            walk(&**child);
        }
    }
}

criterion_group!(benches, bench_palette, bench_taffy_style_translation);
criterion_main!(benches);
