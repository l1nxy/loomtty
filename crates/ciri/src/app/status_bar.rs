use ciri_render::glyph_cache::{GlyphCache, GlyphInstance};

/// Find the key bound to a given action in a bindings map. Returns "?" if not found.
pub(crate) fn find_key_for_action(
    bindings: &std::collections::HashMap<String, String>,
    action: &str,
) -> String {
    for (key, act) in bindings {
        if act == action {
            return key.clone();
        }
    }
    "?".to_string()
}

/// Build hints string from a bindings map and an ordered list of (action, label) pairs.
/// Groups adjacent keys that share the same label and only includes bound actions.
pub(crate) fn build_hints_from_bindings(
    bindings: &std::collections::HashMap<String, String>,
    action_labels: &[(&str, &str)],
) -> String {
    let mut action_to_keys: std::collections::HashMap<&str, Vec<&str>> =
        std::collections::HashMap::new();
    for (key, action) in bindings {
        action_to_keys
            .entry(action.as_str())
            .or_default()
            .push(key.as_str());
    }
    for keys in action_to_keys.values_mut() {
        keys.sort_by_key(|k| k.len());
    }

    let mut parts = Vec::new();
    for (action, label) in action_labels {
        if let Some(keys) = action_to_keys.get(action) {
            let key_str = if keys.len() == 1 {
                keys[0].to_string()
            } else {
                keys.iter().take(2).copied().collect::<Vec<_>>().join("/")
            };
            parts.push(format!("{}:{}", key_str, label));
        }
    }
    parts.join("  ")
}

pub(crate) fn emit_status_text(
    atlas: &mut GlyphCache,
    text: &str,
    x_start: f32,
    text_y: f32,
    cell_width: f32,
    baseline: f32,
    color: [f32; 4],
    glyphs: &mut Vec<GlyphInstance>,
) {
    for (i, ch) in text.chars().enumerate() {
        if let Some(entry) = atlas.ensure_char(ch)
            && entry.width > 0
            && entry.height > 0
        {
            let sx = x_start + i as f32 * cell_width + entry.bearing_x as f32;
            let sy = text_y + baseline - entry.bearing_y as f32;
            glyphs.push(GlyphInstance {
                pos: [sx, sy],
                size: [entry.width as f32, entry.height as f32],
                uv_pos: [entry.u0, entry.v0],
                uv_size: [entry.u1 - entry.u0, entry.v1 - entry.v0],
                color,
            });
        }
    }
}
