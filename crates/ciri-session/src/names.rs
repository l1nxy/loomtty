/// Generate readable random session names in "adjective-animal" format.

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

/// Generate a random "adjective-animal" name.
pub fn random_name() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::SystemTime;

    let mut hasher = DefaultHasher::new();
    SystemTime::now().hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    // Mix in a counter to reduce collisions within the same ms
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    COUNTER
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        .hash(&mut hasher);
    let h = hasher.finish();

    let adj = ADJECTIVES[(h as usize) % ADJECTIVES.len()];
    let animal = ANIMALS[((h >> 32) as usize) % ANIMALS.len()];
    format!("{adj}-{animal}")
}

/// Generate a unique name that doesn't collide with existing names.
pub fn unique_name(existing: &[String]) -> String {
    let base = random_name();
    if !existing.contains(&base) {
        return base;
    }
    for i in 2..1000 {
        let candidate = format!("{base}-{i}");
        if !existing.contains(&candidate) {
            return candidate;
        }
    }
    // Fallback: use timestamp
    format!("{base}-{}", std::process::id())
}

/// Validate a session name. Only allows [a-z0-9][a-z0-9\-]*, max 64 chars.
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("session name cannot be empty".to_string());
    }
    if name.len() > 64 {
        return Err("session name too long (max 64 chars)".to_string());
    }
    if name.starts_with('-') {
        return Err("session name cannot start with '-'".to_string());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(
            "session name can only contain lowercase letters, digits, and hyphens".to_string(),
        );
    }
    Ok(())
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
        let name = unique_name(&[first.clone()]);
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
