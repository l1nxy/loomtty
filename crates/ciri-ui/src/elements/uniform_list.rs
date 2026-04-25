//! `uniform_list` — fixed-height virtual list helper.
//!
//! Packages the "render only visible rows of an N-item list" pattern
//! that every chrome list (palette, history, etc.) currently
//! re-implements. Caller supplies the visible range; the helper builds
//! a `Div` column whose height matches the visible portion and whose
//! children are produced lazily via the caller's `render` closure.
//!
//! This is the *helper-function* shape — not a stateful list element.
//! Scroll state is the caller's responsibility (can live on App,
//! `ElementStates`, or a parent struct). Intentionally simpler than
//! GPUI's `uniform_list` Element: ciri's chrome already pre-computes
//! visible rows during model rebuild (palette filtering, etc.), and a
//! pure helper drops in there without forcing a new Element-trait
//! integration.
//!
//! ```ignore
//! let scroll = …; // caller-managed
//! let visible_start = scroll;
//! let visible_count = layout.visible_rows;
//! let visible_end = (visible_start + visible_count).min(item_count);
//! let column = uniform_list(item_count, row_h, visible_start..visible_end, |range| {
//!     range.map(|i| text(format!("Item {}", i))).collect()
//! });
//! ```

use std::ops::Range;

use crate::element::IntoElement;
use crate::elements::div::{Div, div};
use crate::styled::Styled;

/// Build a `Div` column rendering only the visible items of a uniform-
/// height list.
///
/// The returned column has its own height set to `visible_range.len() *
/// item_height`, so the caller can place it inside an absolute-position
/// container without further height calculation. `render` is invoked
/// exactly once with `visible_range` and yields one `IntoElement` per
/// visible index.
///
/// `item_count` is accepted (alongside the visible range) so the
/// helper can validate that the visible range stays in-bounds — a
/// common off-by-one source when caller code derives the range from
/// scroll offsets.
pub fn uniform_list<F, E>(
    item_count: usize,
    item_height: f32,
    visible_range: Range<usize>,
    render: F,
) -> Div
where
    F: FnOnce(Range<usize>) -> Vec<E>,
    E: IntoElement,
{
    let clamped_start = visible_range.start.min(item_count);
    let clamped_end = visible_range.end.min(item_count);
    let clamped = clamped_start..clamped_end;
    let visible_count = clamped.len();
    let visible_h = visible_count as f32 * item_height.max(0.0);

    let mut col = div().w_full().h(visible_h).flex_col();
    for el in render(clamped) {
        col = col.child(el);
    }
    col
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Element;
    use crate::elements::text;

    #[test]
    fn renders_only_visible_range() {
        let visible = 3..6;
        let count_called = std::cell::RefCell::new(0usize);
        let col = uniform_list(20, 24.0, visible.clone(), |range| {
            *count_called.borrow_mut() = range.len();
            range.map(|i| text(format!("row {i}"))).collect()
        });
        assert_eq!(*count_called.borrow(), 3);
        // The Div should hold exactly the visible item count as children.
        assert_eq!(col.children().len(), 3);
    }

    #[test]
    fn clamps_visible_range_to_item_count() {
        let col = uniform_list(5, 16.0, 3..10, |range| {
            // Clamped end must be 5 (item_count), not 10.
            assert_eq!(range, 3..5);
            range.map(|_| text("x")).collect()
        });
        assert_eq!(col.children().len(), 2);
    }

    #[test]
    fn empty_visible_range_yields_zero_children() {
        let col = uniform_list(10, 16.0, 4..4, |range| {
            assert!(range.is_empty());
            Vec::<crate::Text>::new()
        });
        assert_eq!(col.children().len(), 0);
    }
}
