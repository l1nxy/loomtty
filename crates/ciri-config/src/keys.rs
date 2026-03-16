use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindConfig {
    pub leader: String,
    pub bindings: HashMap<String, String>,
}

impl Default for KeybindConfig {
    fn default() -> Self {
        let mut bindings = HashMap::new();
        bindings.insert("n".to_string(), "new_column_right".to_string());
        bindings.insert("d".to_string(), "split_down".to_string());
        bindings.insert("x".to_string(), "close_pane".to_string());
        bindings.insert("h".to_string(), "focus_left".to_string());
        bindings.insert("l".to_string(), "focus_right".to_string());
        bindings.insert("k".to_string(), "focus_up".to_string());
        bindings.insert("j".to_string(), "focus_down".to_string());
        bindings.insert("shift+h".to_string(), "move_pane_left".to_string());
        bindings.insert("shift+l".to_string(), "move_pane_right".to_string());
        bindings.insert("1".to_string(), "column_width_one_third".to_string());
        bindings.insert("2".to_string(), "column_width_half".to_string());
        bindings.insert("3".to_string(), "column_width_two_thirds".to_string());
        bindings.insert("f".to_string(), "column_width_full".to_string());

        KeybindConfig {
            leader: "ctrl+space".to_string(),
            bindings,
        }
    }
}
