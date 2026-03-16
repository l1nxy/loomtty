/// Actions triggered by keybindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// New column to the right in the current row.
    NewColumnRight,
    /// New row below with a new pane.
    NewRowBelow,
    ClosePane,
    /// h/l: navigate columns within the current row.
    FocusLeft,
    FocusRight,
    /// j/k: navigate between workspace rows.
    FocusDown,
    FocusUp,
    /// Shift+H/L: move column position within the row.
    MovePaneLeft,
    MovePaneRight,
    ColumnWidthOneThird,
    ColumnWidthHalf,
    ColumnWidthTwoThirds,
    ColumnWidthFull,
    /// Switch to workspace row by index (0-8).
    SwitchWorkspace(usize),
    ToggleOverview,
    SendLeaderKey,
}
