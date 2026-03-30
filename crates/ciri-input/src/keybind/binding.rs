//! Binding and BindingSet: mode-aware keybinding matching.

use std::collections::HashMap;

use crate::action::Action;

use super::map::KeybindMap;
use super::parse::parse_bindings;
use super::types::{BindingMode, KeyCombo};

/// A single keybinding with mode-awareness.
#[derive(Debug, Clone)]
pub struct Binding {
    pub combo: KeyCombo,
    pub action: Action,
    /// Mode flags that MUST all be active for this binding to match.
    pub mode: BindingMode,
    /// Mode flags that must NOT be active.
    pub notmode: BindingMode,
    /// If non-empty, only matches when this named key table is at the top of the stack.
    pub key_table: String,
}

impl Binding {
    /// Create a binding that is active in any mode (no mode requirement).
    pub fn global(combo: KeyCombo, action: Action) -> Self {
        Self {
            combo,
            action,
            mode: BindingMode::EMPTY,
            notmode: BindingMode::EMPTY,
            key_table: String::new(),
        }
    }

    /// Create a binding scoped to a specific mode.
    pub fn in_mode(combo: KeyCombo, action: Action, mode: BindingMode) -> Self {
        Self {
            combo,
            action,
            mode,
            notmode: BindingMode::EMPTY,
            key_table: String::new(),
        }
    }

    /// Create a binding scoped to a named key table.
    pub fn in_table(combo: KeyCombo, action: Action, table: &str) -> Self {
        Self {
            combo,
            action,
            mode: BindingMode::KEY_TABLE,
            notmode: BindingMode::EMPTY,
            key_table: table.to_string(),
        }
    }

    /// Check whether this binding matches the current context.
    pub fn matches(
        &self,
        current_mode: BindingMode,
        active_table: Option<&str>,
        combo: &KeyCombo,
    ) -> bool {
        self.combo == *combo
            && current_mode.contains(self.mode)
            && !current_mode.intersects(self.notmode)
            && (self.key_table.is_empty() || active_table == Some(self.key_table.as_str()))
    }
}

// ── BindingSet ──

/// Ordered collection of bindings. First-match-wins lookup.
/// User bindings are inserted before defaults so they take priority.
#[derive(Debug, Clone, Default)]
pub struct BindingSet {
    bindings: Vec<Binding>,
}

impl BindingSet {
    pub fn new() -> Self {
        Self {
            bindings: Vec::new(),
        }
    }

    /// Add a binding. Later bindings have lower priority (first-match-wins).
    pub fn push(&mut self, binding: Binding) {
        self.bindings.push(binding);
    }

    /// Prepend a binding (gives it highest priority).
    pub fn push_front(&mut self, binding: Binding) {
        self.bindings.insert(0, binding);
    }

    /// Append all bindings from another set (lower priority).
    pub fn extend(&mut self, other: BindingSet) {
        self.bindings.extend(other.bindings);
    }

    /// Look up the first matching binding for the given context.
    pub fn lookup(
        &self,
        current_mode: BindingMode,
        active_table: Option<&str>,
        combo: &KeyCombo,
    ) -> Option<&Binding> {
        self.bindings
            .iter()
            .find(|b| b.matches(current_mode, active_table, combo))
    }

    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Build a BindingSet from legacy config maps (backward compatibility).
    pub fn from_legacy(
        leader_bindings: &KeybindMap,
        direct_bindings: &KeybindMap,
        mode_keybinds: &HashMap<String, KeybindMap>,
        overview_bindings: &KeybindMap,
        search_bindings: &HashMap<String, String>,
        palette_bindings: &HashMap<String, String>,
        paste_confirm_bindings: &HashMap<String, String>,
    ) -> Self {
        let mut set = BindingSet::new();

        // Direct bindings: not in text-input overlays or paste confirm
        for (combo, action) in &direct_bindings.bindings {
            set.push(Binding {
                combo: combo.clone(),
                action: action.clone(),
                mode: BindingMode::EMPTY,
                notmode: BindingMode::SEARCH | BindingMode::PALETTE | BindingMode::PASTE_CONFIRM,
                key_table: String::new(),
            });
        }

        // Leader bindings: active when leader is pressed
        for (combo, action) in &leader_bindings.bindings {
            set.push(Binding::in_mode(
                combo.clone(),
                action.clone(),
                BindingMode::LEADER,
            ));
        }

        // Mode/key-table bindings
        for (table_name, table_map) in mode_keybinds {
            for (combo, action) in &table_map.bindings {
                set.push(Binding::in_table(combo.clone(), action.clone(), table_name));
            }
        }

        // Overview bindings
        for (combo, action) in &overview_bindings.bindings {
            set.push(Binding::in_mode(
                combo.clone(),
                action.clone(),
                BindingMode::OVERVIEW,
            ));
        }

        // Search bindings
        let search_parsed = parse_bindings(search_bindings);
        for (combo, action) in search_parsed {
            set.push(Binding::in_mode(combo, action, BindingMode::SEARCH));
        }

        // Palette bindings
        let palette_parsed = parse_bindings(palette_bindings);
        for (combo, action) in palette_parsed {
            set.push(Binding::in_mode(combo, action, BindingMode::PALETTE));
        }

        // Paste confirmation bindings
        let paste_parsed = parse_bindings(paste_confirm_bindings);
        for (combo, action) in paste_parsed {
            set.push(Binding::in_mode(combo, action, BindingMode::PASTE_CONFIRM));
        }

        set
    }
}
