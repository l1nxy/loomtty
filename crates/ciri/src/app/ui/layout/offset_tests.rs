use super::*;

#[test]
fn rect_offset_by_is_pure_translation() {
    let r = UiRect::new(10.0, 20.0, 100.0, 50.0);
    let shifted = r.offset_by(Offset::new(-3.0, 7.0));
    assert_eq!(shifted, UiRect::new(7.0, 27.0, 100.0, 50.0));
    // Zero offset is identity.
    assert_eq!(r.offset_by(Offset::ZERO), r);
}

#[test]
fn offset_zero_is_zero() {
    assert!(Offset::ZERO.is_zero());
    assert!(Offset::default().is_zero());
    assert!(!Offset::new(0.1, 0.0).is_zero());
    assert!(!Offset::new(0.0, -0.1).is_zero());
}
