use super::*;

#[test]
fn truncate_fits_exactly() {
    assert_eq!(truncate_to_cols("abcd", 4), "abcd");
}

#[test]
fn truncate_empty_when_no_budget() {
    assert_eq!(truncate_to_cols("abcd", 0), "");
}

#[test]
fn truncate_adds_ellipsis_when_overflows() {
    assert_eq!(truncate_to_cols("abcdef", 4), "abc…");
}

#[test]
fn truncate_handles_wide_chars() {
    // 全 = 2 cols. With budget 3: "全" takes 2 cols, next char needs
    // ellipsis reservation (col 2 + 1 ≤ 3), so "全" fits but the
    // second wide char does not → ellipsis replaces it.
    assert_eq!(truncate_to_cols("全角", 3), "全…");
}
