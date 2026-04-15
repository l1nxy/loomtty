use super::*;
use super::super::offset::Offset;
use super::super::test_support::{OffsetProbe, Probe, dummy_cx};

#[test]
fn border_center_fills_when_no_edges() {
    let border: Border<'static> = Border::new().center(Probe::fill_horizontal());
    let cx = dummy_cx();
    let slots = border.layout(UiRect::new(0.0, 0.0, 100.0, 50.0), &cx);
    assert_eq!(slots.center, UiRect::new(0.0, 0.0, 100.0, 50.0));
    assert!(slots.top.is_empty());
    assert!(slots.bottom.is_empty());
}

#[test]
fn border_top_takes_from_center_height() {
    use crate::app::ui::types::{UiContext, UiScene};
    struct FixedBar(f32);
    impl UiElement for FixedBar {
        fn size_hint(&self, axis: Axis, _cx: &UiContext<'_>) -> SizeHint {
            match axis {
                Axis::Vertical => SizeHint::Fixed(self.0),
                Axis::Horizontal => SizeHint::Fill,
            }
        }
        fn paint(&self, _r: UiRect, _cx: &UiContext<'_>, _s: &mut UiScene<'_>) {}
    }
    let border = Border::new()
        .top(FixedBar(15.0))
        .center(Probe::fill_horizontal());
    let cx = dummy_cx();
    let slots = border.layout(UiRect::new(0.0, 0.0, 100.0, 50.0), &cx);
    assert_eq!(slots.top, UiRect::new(0.0, 0.0, 100.0, 15.0));
    assert_eq!(slots.center, UiRect::new(0.0, 15.0, 100.0, 35.0));
}

/// Same property as `linear_paint_rect_includes_child_render_offset` but
/// for `Border` — exercises the `child_paint_slots` helper that
/// production `Border::paint` uses.
#[test]
fn border_paint_slots_include_child_render_offset() {
    let cx = dummy_cx();
    let border = Border::new()
        .top(OffsetProbe::new(Offset::new(0.0, 7.0)))
        .center(Probe::fill_horizontal());
    let slots = border.child_paint_slots(UiRect::new(0.0, 0.0, 100.0, 100.0), &cx);
    // Top's layout slot is (0, 0, 100, 50) (OffsetProbe size_hint
    // returns Fixed(50) on both axes, so vertical takes 50). With
    // Offset(0, 7) applied the paint rect shifts down by 7.
    assert_eq!(slots.top, UiRect::new(0.0, 7.0, 100.0, 50.0));
    // Center has no offset → unchanged.
    assert_eq!(slots.center, UiRect::new(0.0, 50.0, 100.0, 50.0));
}
