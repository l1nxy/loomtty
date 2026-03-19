use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindConfig {
    pub leader: String,
    pub bindings: HashMap<String, String>,
    pub overview_bindings: HashMap<String, String>,
}

impl Default for KeybindConfig {
    fn default() -> Self {
        // Must match config/default.toml [keys] section exactly.
        let mut bindings = HashMap::new();
        bindings.insert("n".to_string(), "new_column_right".to_string());
        bindings.insert("d".to_string(), "new_row_below".to_string());
        bindings.insert("x".to_string(), "close_pane".to_string());
        bindings.insert("h".to_string(), "focus_left".to_string());
        bindings.insert("l".to_string(), "focus_right".to_string());
        bindings.insert("k".to_string(), "focus_up".to_string());
        bindings.insert("j".to_string(), "focus_down".to_string());
        bindings.insert("shift+h".to_string(), "move_pane_left".to_string());
        bindings.insert("shift+l".to_string(), "move_pane_right".to_string());
        bindings.insert("[".to_string(), "column_width_decrease".to_string());
        bindings.insert("]".to_string(), "column_width_increase".to_string());
        bindings.insert("1".to_string(), "column_width_one_third".to_string());
        bindings.insert("2".to_string(), "column_width_half".to_string());
        bindings.insert("3".to_string(), "column_width_two_thirds".to_string());
        bindings.insert("f".to_string(), "column_width_full".to_string());
        bindings.insert("o".to_string(), "toggle_overview".to_string());
        bindings.insert("tab".to_string(), "toggle_overview".to_string());
        bindings.insert("q".to_string(), "detach".to_string());

        let mut overview_bindings = HashMap::new();
        overview_bindings.insert("h".to_string(), "focus_left".to_string());
        overview_bindings.insert("l".to_string(), "focus_right".to_string());
        overview_bindings.insert("j".to_string(), "focus_down".to_string());
        overview_bindings.insert("k".to_string(), "focus_up".to_string());
        overview_bindings.insert("x".to_string(), "close_pane".to_string());
        overview_bindings.insert("n".to_string(), "new_column_right".to_string());
        overview_bindings.insert("escape".to_string(), "exit_overview".to_string());
        overview_bindings.insert("enter".to_string(), "exit_overview".to_string());
        overview_bindings.insert("o".to_string(), "exit_overview".to_string());
        overview_bindings.insert("tab".to_string(), "exit_overview".to_string());

        KeybindConfig {
            leader: "ctrl+w".to_string(),
            bindings,
            overview_bindings,
        }
    }
}
