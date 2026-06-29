use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindConfig {
    pub leader: String,
    pub bindings: HashMap<String, String>,
    pub overview_bindings: HashMap<String, String>,
    /// Named key tables (e.g. "resize" mode).
    pub modes: HashMap<String, HashMap<String, String>>,
    /// Direct bindings that work without leader (e.g. "ctrl+g" = "toggle_lock").
    pub direct_bindings: HashMap<String, String>,
    /// Search mode control keys (e.g. "escape" = "close_search").
    pub search_bindings: HashMap<String, String>,
    /// Command palette control keys (e.g. "Up" = "palette_up").
    pub palette_bindings: HashMap<String, String>,
    /// Paste confirmation dialog keys (e.g. "enter" = "confirm_paste").
    pub paste_confirm_bindings: HashMap<String, String>,
}

impl Default for KeybindConfig {
    fn default() -> Self {
        // Must match config/default.toml [keys] section exactly.
        let mut bindings = HashMap::new();
        bindings.insert("n".to_string(), "new_column_right".to_string());
        bindings.insert("d".to_string(), "new_row_below".to_string());
        bindings.insert("shift+d".to_string(), "new_tile_below".to_string());
        bindings.insert("x".to_string(), "close_pane".to_string());
        bindings.insert("h".to_string(), "focus_left".to_string());
        bindings.insert("l".to_string(), "focus_right".to_string());
        bindings.insert("k".to_string(), "focus_up".to_string());
        bindings.insert("j".to_string(), "focus_down".to_string());
        bindings.insert("shift+h".to_string(), "move_pane_left".to_string());
        bindings.insert("shift+l".to_string(), "move_pane_right".to_string());
        bindings.insert("shift+k".to_string(), "move_pane_up".to_string());
        bindings.insert("shift+j".to_string(), "move_pane_down".to_string());
        bindings.insert("r".to_string(), "enter_mode:resize".to_string());
        bindings.insert("s".to_string(), "enter_mode:scroll".to_string());
        bindings.insert("m".to_string(), "enter_mode:move".to_string());
        bindings.insert("f".to_string(), "column_width_full".to_string());
        bindings.insert("b".to_string(), "toggle_broadcast".to_string());
        bindings.insert("c".to_string(), "consume_into_column".to_string());
        bindings.insert("e".to_string(), "expel_from_column".to_string());
        bindings.insert("o".to_string(), "toggle_overview".to_string());
        bindings.insert("q".to_string(), "detach".to_string());
        bindings.insert("p".to_string(), "toggle_command_palette".to_string());
        bindings.insert("g".to_string(), "toggle_lock".to_string());
        bindings.insert("/".to_string(), "toggle_help".to_string());

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

        let mut modes = HashMap::new();
        let mut resize = HashMap::new();
        resize.insert("h".to_string(), "column_width_decrease".to_string());
        resize.insert("l".to_string(), "column_width_increase".to_string());
        resize.insert("j".to_string(), "tile_height_increase".to_string());
        resize.insert("k".to_string(), "tile_height_decrease".to_string());
        resize.insert("[".to_string(), "column_width_decrease".to_string());
        resize.insert("]".to_string(), "column_width_increase".to_string());
        resize.insert("r".to_string(), "cycle_preset_width".to_string());
        resize.insert(
            "shift+r".to_string(),
            "cycle_preset_width_reverse".to_string(),
        );
        resize.insert("f".to_string(), "column_width_full".to_string());
        resize.insert("=".to_string(), "equalize_adjacent_columns".to_string());
        modes.insert("resize".to_string(), resize);

        let mut scroll = HashMap::new();
        scroll.insert("j".to_string(), "scroll_line_down".to_string());
        scroll.insert("k".to_string(), "scroll_line_up".to_string());
        scroll.insert("Down".to_string(), "scroll_line_down".to_string());
        scroll.insert("Up".to_string(), "scroll_line_up".to_string());
        scroll.insert("d".to_string(), "scroll_half_page_down".to_string());
        scroll.insert("u".to_string(), "scroll_half_page_up".to_string());
        scroll.insert("f".to_string(), "scroll_page_down".to_string());
        scroll.insert("b".to_string(), "scroll_page_up".to_string());
        scroll.insert("g".to_string(), "scroll_top".to_string());
        scroll.insert("shift+g".to_string(), "scroll_bottom".to_string());
        scroll.insert("[".to_string(), "prev_prompt".to_string());
        scroll.insert("]".to_string(), "next_prompt".to_string());
        modes.insert("scroll".to_string(), scroll);

        let mut mv = HashMap::new();
        mv.insert("h".to_string(), "move_pane_left".to_string());
        mv.insert("l".to_string(), "move_pane_right".to_string());
        mv.insert("k".to_string(), "move_pane_up".to_string());
        mv.insert("j".to_string(), "move_pane_down".to_string());
        mv.insert("Left".to_string(), "move_pane_left".to_string());
        mv.insert("Right".to_string(), "move_pane_right".to_string());
        mv.insert("Up".to_string(), "move_pane_up".to_string());
        mv.insert("Down".to_string(), "move_pane_down".to_string());
        modes.insert("move".to_string(), mv);

        let mut direct_bindings = HashMap::new();
        direct_bindings.insert("ctrl+g".to_string(), "toggle_lock".to_string());
        // Move the active pane across workspaces / columns without the leader.
        direct_bindings.insert("alt+shift+h".to_string(), "move_pane_left".to_string());
        direct_bindings.insert("alt+shift+l".to_string(), "move_pane_right".to_string());
        direct_bindings.insert("alt+shift+k".to_string(), "move_pane_up".to_string());
        direct_bindings.insert("alt+shift+j".to_string(), "move_pane_down".to_string());
        #[cfg(target_os = "macos")]
        {
            direct_bindings.insert("super+f".to_string(), "open_search".to_string());
            direct_bindings.insert("super+c".to_string(), "clipboard_copy".to_string());
            direct_bindings.insert("super+v".to_string(), "clipboard_paste".to_string());
            direct_bindings.insert("super+q".to_string(), "quit".to_string());
            direct_bindings.insert("super+w".to_string(), "close_pane".to_string());
            direct_bindings.insert("super+n".to_string(), "create_pane".to_string());
        }
        #[cfg(not(target_os = "macos"))]
        {
            direct_bindings.insert("ctrl+shift+f".to_string(), "open_search".to_string());
            direct_bindings.insert("ctrl+shift+c".to_string(), "clipboard_copy".to_string());
            direct_bindings.insert("ctrl+shift+v".to_string(), "clipboard_paste".to_string());
        }

        let mut search_bindings = HashMap::new();
        search_bindings.insert("escape".to_string(), "close_search".to_string());
        search_bindings.insert("enter".to_string(), "search_next_match".to_string());
        search_bindings.insert("shift+enter".to_string(), "search_prev_match".to_string());
        search_bindings.insert("backspace".to_string(), "text_backspace".to_string());

        let mut palette_bindings = HashMap::new();
        palette_bindings.insert("escape".to_string(), "close_command_palette".to_string());
        palette_bindings.insert("up".to_string(), "palette_up".to_string());
        palette_bindings.insert("down".to_string(), "palette_down".to_string());
        palette_bindings.insert("enter".to_string(), "palette_confirm".to_string());
        palette_bindings.insert("backspace".to_string(), "text_backspace".to_string());

        let mut paste_confirm_bindings = HashMap::new();
        paste_confirm_bindings.insert("enter".to_string(), "confirm_paste".to_string());
        paste_confirm_bindings.insert("y".to_string(), "confirm_paste".to_string());
        paste_confirm_bindings.insert("escape".to_string(), "dismiss_paste_confirm".to_string());
        paste_confirm_bindings.insert("n".to_string(), "dismiss_paste_confirm".to_string());

        let leader = "alt".to_string();

        KeybindConfig {
            leader,
            bindings,
            overview_bindings,
            modes,
            direct_bindings,
            search_bindings,
            palette_bindings,
            paste_confirm_bindings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// When a user config specifies `[keys]` with only a custom leader,
    /// omitted fields like palette_bindings must still get their defaults
    /// from `KeybindConfig::default()` (not empty HashMaps).
    #[test]
    fn partial_keys_config_preserves_defaults() {
        let toml = r#"
            leader = "ctrl+a"
        "#;

        let config: KeybindConfig = toml::from_str(toml).unwrap();

        assert_eq!(config.leader, "ctrl+a");

        // palette_bindings should have defaults, not be empty
        let defaults = KeybindConfig::default();
        assert_eq!(config.palette_bindings, defaults.palette_bindings);
        assert!(!config.palette_bindings.is_empty());
        assert!(config.palette_bindings.contains_key("escape"));
        assert!(config.palette_bindings.contains_key("enter"));

        // search_bindings should also have defaults
        assert_eq!(config.search_bindings, defaults.search_bindings);
        assert!(!config.search_bindings.is_empty());

        // paste_confirm_bindings should also have defaults
        assert_eq!(
            config.paste_confirm_bindings,
            defaults.paste_confirm_bindings
        );
        assert!(!config.paste_confirm_bindings.is_empty());
    }

    #[test]
    fn custom_bindings_override_defaults() {
        let toml = r#"
            [bindings]
            n = "custom_action"
            z = "some_new_action"
        "#;

        let config: KeybindConfig = toml::from_str(toml).unwrap();

        // Overridden binding
        assert_eq!(config.bindings["n"], "custom_action");
        // New binding added
        assert_eq!(config.bindings["z"], "some_new_action");
        // Other default bindings are NOT present because TOML replaces the whole map
        assert!(!config.bindings.contains_key("h"));
    }

    #[test]
    fn custom_mode_replaces_entire_modes_map() {
        // TOML replaces the whole `modes` HashMap, not individual entries
        let toml = r#"
            [modes.resize]
            x = "custom_resize_action"
        "#;

        let config: KeybindConfig = toml::from_str(toml).unwrap();

        assert_eq!(config.modes["resize"]["x"], "custom_resize_action");
        // Other modes are lost because TOML replaces the whole map
        assert!(!config.modes.contains_key("scroll"));
        assert!(!config.modes.contains_key("move"));
    }

    #[test]
    fn empty_bindings_map_is_valid() {
        let toml = r#"
            leader = "ctrl+a"
            [bindings]
        "#;

        let config: KeybindConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.leader, "ctrl+a");
        assert!(config.bindings.is_empty());
        // Other binding maps still have defaults
        assert!(!config.search_bindings.is_empty());
    }

    #[test]
    fn default_keybinds_cover_essential_actions() {
        let defaults = KeybindConfig::default();

        // Core navigation must exist
        assert!(defaults.bindings.contains_key("h"), "missing focus_left");
        assert!(defaults.bindings.contains_key("l"), "missing focus_right");
        assert!(defaults.bindings.contains_key("j"), "missing focus_down");
        assert!(defaults.bindings.contains_key("k"), "missing focus_up");

        // Core pane management must exist
        assert!(defaults.bindings.contains_key("n"), "missing new_column");
        assert!(defaults.bindings.contains_key("x"), "missing close_pane");

        // All 3 mode tables must exist
        assert!(defaults.modes.contains_key("resize"));
        assert!(defaults.modes.contains_key("scroll"));
        assert!(defaults.modes.contains_key("move"));

        // Search must have escape to close
        assert_eq!(
            defaults.search_bindings.get("escape"),
            Some(&"close_search".to_string())
        );
    }
}
