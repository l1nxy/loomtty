use super::super::offset::Offset;
use super::super::test_support::{OffsetProbe, Probe, dummy_cx};
use super::*;
use crate::app::ui::types::UiAction;

#[test]
fn linear_horizontal_splits_by_fixed_then_fill() {
    let linear = Linear::new(Axis::Horizontal)
        .push(Probe::fixed_horizontal(20.0))
        .push(Probe::fill_horizontal())
        .push(Probe::fixed_horizontal(30.0));
    let cx = dummy_cx();
    let slots = linear.layout(UiRect::new(0.0, 0.0, 100.0, 10.0), &cx);
    assert_eq!(slots.len(), 3);
    assert_eq!(slots[0], UiRect::new(0.0, 0.0, 20.0, 10.0));
    assert_eq!(slots[1], UiRect::new(20.0, 0.0, 50.0, 10.0));
    assert_eq!(slots[2], UiRect::new(70.0, 0.0, 30.0, 10.0));
}

#[test]
fn linear_two_fills_share_remaining_space_equally() {
    let linear = Linear::new(Axis::Horizontal)
        .push(Probe::fill_horizontal())
        .push(Probe::fixed_horizontal(10.0))
        .push(Probe::fill_horizontal());
    let cx = dummy_cx();
    let slots = linear.layout(UiRect::new(0.0, 0.0, 50.0, 10.0), &cx);
    assert_eq!(slots[0].w, 20.0);
    assert_eq!(slots[1].w, 10.0);
    assert_eq!(slots[2].w, 20.0);
}

#[test]
fn linear_fixed_overflow_leaves_zero_fill() {
    let linear = Linear::new(Axis::Horizontal)
        .push(Probe::fixed_horizontal(80.0))
        .push(Probe::fill_horizontal())
        .push(Probe::fixed_horizontal(80.0));
    let cx = dummy_cx();
    let slots = linear.layout(UiRect::new(0.0, 0.0, 100.0, 10.0), &cx);
    // First fixed consumes 80, leaving 20; fill gets 0 since 80+80 > 100.
    // Second fixed then still asks for 80 but we only have 20 left; it
    // clamps.
    assert_eq!(slots[0].w, 80.0);
    assert_eq!(slots[1].w, 0.0);
    assert_eq!(slots[2].w, 20.0);
}

#[test]
fn linear_hit_finds_child_slot() {
    use crate::app::ui::types::{UiContext, UiScene};
    struct ClickProbe;
    impl UiElement for ClickProbe {
        fn size_hint(&self, _axis: Axis, _cx: &UiContext<'_>) -> SizeHint {
            SizeHint::Fixed(10.0)
        }
        fn paint(&self, _r: UiRect, _cx: &UiContext<'_>, _s: &mut UiScene<'_>) {}
        fn hit(&self, _rect: UiRect, _mx: f32, _my: f32, _cx: &UiContext<'_>) -> Option<UiAction> {
            Some(UiAction::CycleWorkspace)
        }
    }
    let linear = Linear::new(Axis::Horizontal)
        .push(ClickProbe)
        .push(ClickProbe);
    let cx = dummy_cx();
    let action = linear.hit(UiRect::new(0.0, 0.0, 20.0, 10.0), 5.0, 5.0, &cx);
    assert_eq!(action, Some(UiAction::CycleWorkspace));
}

/// Calls the same `child_paint_rects` helper that production
/// `Linear::paint` calls, so a regression that removes the
/// `slot.offset_by(off)` step from the helper fails this test.
#[test]
fn linear_paint_rect_includes_child_render_offset() {
    let cx = dummy_cx();
    let linear = Linear::new(Axis::Horizontal)
        .push(Probe::fixed_horizontal(20.0))
        .push(OffsetProbe::new(Offset::new(3.0, 5.0)));
    let rects = linear.child_paint_rects(UiRect::new(0.0, 0.0, 100.0, 10.0), &cx);
    // First child has no offset → paint rect == layout slot.
    assert_eq!(rects[0], UiRect::new(0.0, 0.0, 20.0, 10.0));
    // Second child has Offset(3, 5) → paint rect == slot + offset.
    assert_eq!(rects[1], UiRect::new(23.0, 5.0, 50.0, 10.0));
}

/// Hit-testing must use the *layout* slot, never the post-offset
/// paint rect — otherwise mid-animation clicks would chase moving
/// targets. Verified by giving a child an absurd 500-px offset and
/// confirming clicks at the layout position hit and clicks at the
/// offset position miss.
#[test]
fn linear_hit_ignores_render_offset() {
    let cx = dummy_cx();
    let linear = Linear::new(Axis::Horizontal)
        .push(Probe::fixed_horizontal(20.0))
        .push(OffsetProbe::new(Offset::new(500.0, 500.0)));
    // Hit at child[1]'s *layout* position (slot is x∈[20,70], y∈[0,10]).
    let action = linear.hit(UiRect::new(0.0, 0.0, 100.0, 10.0), 30.0, 5.0, &cx);
    assert_eq!(action, Some(UiAction::CycleWorkspace));

    // Hit at the *visually offset* position. Inside the outer
    // Linear rect but outside child[1]'s layout slot — must miss.
    let miss = linear.hit(UiRect::new(0.0, 0.0, 1000.0, 1000.0), 530.0, 505.0, &cx);
    assert_eq!(miss, None);
}

#[test]
fn default_render_offset_is_zero() {
    let p = Probe::fixed_horizontal(10.0);
    assert!(p.render_offset().is_zero());
}
