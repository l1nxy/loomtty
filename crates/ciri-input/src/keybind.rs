use crate::action::Action;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyCombo {
    pub key: String,
    pub shift: bool,
}

impl KeyCombo {
    pub fn new(key: &str) -> Self {
        KeyCombo { key: key.to_lowercase(), shift: false }
    }
    pub fn with_shift(key: &str) -> Self {
        KeyCombo { key: key.to_lowercase(), shift: true }
    }
}

pub struct KeybindMap {
    pub bindings: HashMap<KeyCombo, Action>,
}

impl Default for KeybindMap {
    fn default() -> Self {
        let mut b = HashMap::new();

        // Pane lifecycle
        b.insert(KeyCombo::new("n"), Action::NewColumnRight);   // new column in this row
        b.insert(KeyCombo::new("d"), Action::NewRowBelow);      // new row below
        b.insert(KeyCombo::new("x"), Action::ClosePane);

        // Navigation: h/l = within row, j/k = between rows
        b.insert(KeyCombo::new("h"), Action::FocusLeft);
        b.insert(KeyCombo::new("l"), Action::FocusRight);
        b.insert(KeyCombo::new("j"), Action::FocusDown);        // next row
        b.insert(KeyCombo::new("k"), Action::FocusUp);          // prev row
        b.insert(KeyCombo::new("left"), Action::FocusLeft);
        b.insert(KeyCombo::new("right"), Action::FocusRight);
        b.insert(KeyCombo::new("up"), Action::FocusUp);
        b.insert(KeyCombo::new("down"), Action::FocusDown);

        // Move column within row
        b.insert(KeyCombo::with_shift("h"), Action::MovePaneLeft);
        b.insert(KeyCombo::with_shift("l"), Action::MovePaneRight);

        // Column width
        b.insert(KeyCombo::new("f"), Action::ColumnWidthFull);
        b.insert(KeyCombo::new("e"), Action::ColumnWidthHalf);
        b.insert(KeyCombo::new("["), Action::ColumnWidthOneThird);
        b.insert(KeyCombo::new("]"), Action::ColumnWidthTwoThirds);

        // Workspace switching by number
        for i in 1u8..=9 {
            b.insert(KeyCombo::new(&format!("{i}")), Action::SwitchWorkspace((i - 1) as usize));
        }

        // Overview
        b.insert(KeyCombo::new("o"), Action::ToggleOverview);
        b.insert(KeyCombo::new("tab"), Action::ToggleOverview);

        KeybindMap { bindings: b }
    }
}

impl KeybindMap {
    pub fn lookup(&self, combo: &KeyCombo) -> Option<Action> {
        self.bindings.get(combo).copied()
    }
}
