//! System font family enumeration for the settings UI.
//!
//! Powers the Font Family / UI Font Family dropdowns in the settings
//! panel. fontdb's `load_system_fonts()` is a few-hundred-ms scan on
//! cold caches, so the result is memoised in `OnceLock`s — first
//! dropdown open pays the cost; subsequent opens are instant. Two
//! views are exposed: `monospaced_families` (terminal-friendly subset)
//! and `all_families` (used by the UI font picker, where proportional
//! faces like Inter / Segoe UI are desirable).

use std::collections::BTreeSet;
use std::sync::OnceLock;

static MONOSPACED_FAMILIES: OnceLock<Vec<String>> = OnceLock::new();
static ALL_FAMILIES: OnceLock<Vec<String>> = OnceLock::new();

/// Sorted, de-duplicated list of monospaced family names discovered
/// by fontdb's system-font scan. Cached after the first call.
pub fn monospaced_families() -> &'static [String] {
    MONOSPACED_FAMILIES.get_or_init(|| load_families(true))
}

/// Sorted, de-duplicated list of every font family fontdb finds —
/// monospaced and proportional. Cached after the first call.
pub fn all_families() -> &'static [String] {
    ALL_FAMILIES.get_or_init(|| load_families(false))
}

fn load_families(only_monospaced: bool) -> Vec<String> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let mut names: BTreeSet<String> = BTreeSet::new();
    for face in db.faces() {
        if only_monospaced && !face.monospaced {
            continue;
        }
        // `face.families` is `Vec<(name, language)>`; the first entry
        // is the primary localisation. Use it verbatim — the user
        // matches against this string in their TOML.
        if let Some((name, _)) = face.families.first() {
            names.insert(name.clone());
        }
    }
    names.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke test: the cache returns the same slice pointer on
    /// repeated calls. Doesn't assert presence of any specific font
    /// because the test runner's font environment is unknowable.
    #[test]
    fn cache_returns_stable_reference() {
        let a = monospaced_families().as_ptr();
        let b = monospaced_families().as_ptr();
        assert_eq!(a, b);
        let c = all_families().as_ptr();
        let d = all_families().as_ptr();
        assert_eq!(c, d);
    }

    /// All-families list is a superset of monospaced-families.
    #[test]
    fn all_is_superset_of_monospaced() {
        let mono: std::collections::HashSet<&String> = monospaced_families().iter().collect();
        for m in mono {
            assert!(
                all_families().contains(m),
                "{m} is monospaced but missing from all_families()",
            );
        }
    }
}
