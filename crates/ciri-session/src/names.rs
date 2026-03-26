//! Generate readable random session names in "adjective-animal" format.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

const ADJECTIVES: &[&str] = &[
    "aged", "bold", "brave", "brief", "bright", "calm", "clean", "clear", "cold", "cool", "crisp",
    "dark", "deep", "dry", "early", "fair", "fast", "firm", "flat", "free", "fresh", "full",
    "glad", "gold", "grand", "gray", "green", "happy", "keen", "kind", "late", "lean", "light",
    "live", "long", "lost", "loud", "mild", "neat", "new", "nice", "odd", "old", "pale", "plain",
    "proud", "pure", "quick", "rare", "raw", "red", "rich", "ripe", "rough", "round", "rusty",
    "safe", "sharp", "short", "shy", "slim", "slow", "small", "smart", "soft", "solid", "stark",
    "steep", "still", "stout", "sweet", "swift", "tall", "tame", "thin", "tiny", "tough", "trim",
    "true", "vast", "vivid", "warm", "weak", "wet", "white", "whole", "wide", "wild", "wise",
    "worn", "young",
];

const ANIMALS: &[&str] = &[
    "ant", "ape", "bat", "bear", "bee", "bird", "boar", "bull", "calf", "cat", "clam", "cod",
    "colt", "cow", "crab", "crane", "crow", "cub", "deer", "dog", "dove", "duck", "eagle", "eel",
    "elk", "ewe", "fawn", "finch", "fish", "fly", "fox", "frog", "goat", "goose", "grouse", "gull",
    "hare", "hawk", "hen", "hog", "horse", "hound", "jay", "kit", "kite", "lark", "lion", "lynx",
    "mare", "mink", "mole", "moth", "mouse", "mule", "newt", "owl", "ox", "parrot", "perch", "pig",
    "pike", "pony", "pug", "quail", "ram", "rat", "raven", "robin", "seal", "shark", "sheep",
    "shrew", "slug", "snail", "snake", "sole", "squid", "stag", "stork", "swan", "toad", "trout",
    "tuna", "vole", "wasp", "whale", "wolf", "worm", "wren", "yak",
];

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generate a random "adjective-animal" name.
pub fn random_name() -> String {
    let mut hasher = DefaultHasher::new();
    SystemTime::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    COUNTER.fetch_add(1, Ordering::Relaxed).hash(&mut hasher);
    let h = hasher.finish();

    let adj = ADJECTIVES[(h as usize) % ADJECTIVES.len()];
    let animal = ANIMALS[((h >> 32) as usize) % ANIMALS.len()];
    format!("{adj}-{animal}")
}

/// Generate a unique name that doesn't collide with existing names.
pub fn unique_name(existing: &[String]) -> String {
    let base = random_name();
    if !name_exists(existing, &base) {
        return base;
    }

    for i in 2..1000 {
        let candidate = format!("{base}-{i}");
        if !name_exists(existing, &candidate) {
            return candidate;
        }
    }

    format!("{base}-{}", std::process::id())
}

/// Validate a session name. Only allows [a-z0-9][a-z0-9\-]*, max 64 chars.
pub fn validate_name(name: &str) -> Result<(), String> {
    validate_not_empty(name)?;
    validate_length(name)?;
    validate_first_char(name)?;
    validate_characters(name)
}

fn name_exists(existing: &[String], candidate: &str) -> bool {
    existing.iter().any(|name| name == candidate)
}

fn validate_not_empty(name: &str) -> Result<(), String> {
    (!name.is_empty())
        .then_some(())
        .ok_or_else(|| "session name cannot be empty".to_string())
}

fn validate_length(name: &str) -> Result<(), String> {
    (name.len() <= 64)
        .then_some(())
        .ok_or_else(|| "session name too long (max 64 chars)".to_string())
}

fn validate_first_char(name: &str) -> Result<(), String> {
    (!name.starts_with('-'))
        .then_some(())
        .ok_or_else(|| "session name cannot start with '-'".to_string())
}

fn validate_characters(name: &str) -> Result<(), String> {
    name.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        .then_some(())
        .ok_or_else(|| {
            "session name can only contain lowercase letters, digits, and hyphens".to_string()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_name_is_valid() {
        for _ in 0..100 {
            let name = random_name();
            assert!(validate_name(&name).is_ok(), "invalid name: {name}");
            assert!(name.contains('-'));
        }
    }

    #[test]
    fn unique_name_avoids_collisions() {
        let first = random_name();
        let name = unique_name(std::slice::from_ref(&first));
        assert_ne!(name, first);
        assert!(validate_name(&name).is_ok());
    }

    #[test]
    fn validate_rejects_bad_names() {
        assert!(validate_name("").is_err());
        assert!(validate_name("-foo").is_err());
        assert!(validate_name("Foo").is_err());
        assert!(validate_name("foo/bar").is_err());
        assert!(validate_name("foo\\bar").is_err());
        assert!(validate_name("foo..bar").is_err());
        assert!(validate_name("a".repeat(65).as_str()).is_err());
        assert!(validate_name("CON").is_err()); // uppercase rejected
    }

    #[test]
    fn validate_accepts_good_names() {
        assert!(validate_name("default").is_ok());
        assert!(validate_name("my-session").is_ok());
        assert!(validate_name("dev-2").is_ok());
        assert!(validate_name("swift-owl").is_ok());
        assert!(validate_name("a").is_ok());
    }
}
