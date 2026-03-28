//! Key combo parsing and binding config helpers.

use std::collections::HashMap;

use crate::action::Action;

use super::types::KeyCombo;

#[derive(Clone, Copy)]
pub(super) enum Modifier {
    Shift,
    Alt,
    Ctrl,
    Super,
}

#[derive(Clone, Copy)]
enum BindingSource {
    KeybindingConfig,
    OverviewKeybindingConfig,
}

impl BindingSource {
    fn label(self) -> &'static str {
        match self {
            BindingSource::KeybindingConfig => "keybinding config",
            BindingSource::OverviewKeybindingConfig => "overview keybinding config",
        }
    }
}

pub(super) fn parse_modifier_prefix(input: &str) -> Option<(Modifier, &str)> {
    for (prefix, modifier) in [
        ("shift+", Modifier::Shift),
        ("alt+", Modifier::Alt),
        ("ctrl+", Modifier::Ctrl),
        ("super+", Modifier::Super),
    ] {
        if input
            .get(..prefix.len())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
        {
            let rest = &input[prefix.len()..];
            return Some((modifier, rest));
        }
    }

    None
}

pub(super) fn split_modifier_prefixes(mut input: &str) -> (&str, Vec<Modifier>) {
    let mut modifiers = Vec::new();

    while let Some((modifier, rest)) = parse_modifier_prefix(input) {
        modifiers.push(modifier);
        input = rest;
    }

    (input, modifiers)
}

pub(super) fn apply_modifiers(combo: &mut KeyCombo, modifiers: Vec<Modifier>) {
    for modifier in modifiers {
        match modifier {
            Modifier::Shift => combo.shift = true,
            Modifier::Alt => combo.alt = true,
            Modifier::Ctrl => combo.ctrl = true,
            Modifier::Super => combo.super_key = true,
        }
    }
}

pub(super) fn parse_bindings(
    bindings: &HashMap<String, String>,
) -> HashMap<KeyCombo, Action> {
    parse_bindings_with_source(bindings, BindingSource::KeybindingConfig)
}

fn parse_bindings_with_source(
    bindings: &HashMap<String, String>,
    source: BindingSource,
) -> HashMap<KeyCombo, Action> {
    let mut parsed = HashMap::new();

    for (key_str, action_str) in bindings {
        match Action::from_name(action_str) {
            Some(action) => {
                parsed.insert(KeyCombo::parse(key_str), action);
            }
            None => warn_unknown_action(source, key_str, action_str),
        }
    }

    parsed
}

pub(super) fn parse_overview_bindings(
    bindings: &HashMap<String, String>,
) -> HashMap<KeyCombo, Action> {
    parse_bindings_with_source(bindings, BindingSource::OverviewKeybindingConfig)
}

fn warn_unknown_action(source: BindingSource, key: &str, action: &str) {
    log::warn!(
        "unknown action in {}: {action:?} (key: {key:?})",
        source.label()
    );
}
