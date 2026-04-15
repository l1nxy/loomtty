use super::*;

#[test]
fn rect_split_left_basic() {
    let r = UiRect::new(0.0, 0.0, 100.0, 20.0);
    let (a, b) = r.split_left(30.0);
    assert_eq!(a, UiRect::new(0.0, 0.0, 30.0, 20.0));
    assert_eq!(b, UiRect::new(30.0, 0.0, 70.0, 20.0));
}

#[test]
fn rect_split_right_basic() {
    let r = UiRect::new(10.0, 0.0, 100.0, 20.0);
    let (remain, taken) = r.split_right(30.0);
    assert_eq!(remain, UiRect::new(10.0, 0.0, 70.0, 20.0));
    assert_eq!(taken, UiRect::new(80.0, 0.0, 30.0, 20.0));
}

#[test]
fn rect_split_overflow_clamps() {
    let r = UiRect::new(0.0, 0.0, 50.0, 20.0);
    let (a, b) = r.split_left(999.0);
    assert_eq!(a, UiRect::new(0.0, 0.0, 50.0, 20.0));
    assert!(b.is_empty());
}

#[test]
fn rect_split_top_bottom_symmetry() {
    let r = UiRect::new(0.0, 0.0, 100.0, 80.0);
    let (t, rest1) = r.split_top(20.0);
    let (rest2, b) = r.split_bottom(20.0);
    assert_eq!(t.h, 20.0);
    assert_eq!(b.h, 20.0);
    assert_eq!(rest1.h, 60.0);
    assert_eq!(rest2.h, 60.0);
    assert_eq!(rest1.y, 20.0);
    assert_eq!(rest2.y, 0.0);
}

#[test]
fn rect_contains_respects_right_and_bottom_exclusive() {
    let r = UiRect::new(10.0, 10.0, 20.0, 20.0);
    assert!(r.contains(10.0, 10.0));
    assert!(r.contains(29.9, 29.9));
    assert!(!r.contains(30.0, 20.0));
    assert!(!r.contains(20.0, 30.0));
    assert!(!r.contains(9.99, 20.0));
}
