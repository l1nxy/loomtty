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
    /// Increase the active column's width proportion.
    ColumnWidthIncrease,
    /// Decrease the active column's width proportion.
    ColumnWidthDecrease,
    /// Switch to workspace row by index (0-8).
    SwitchWorkspace(usize),
    ToggleOverview,
    /// Exit overview mode (return to normal).
    ExitOverview,
    SendLeaderKey,
}

impl Action {
    /// Parse an action from its string name (used for config-driven keybindings).
    pub fn from_name(s: &str) -> Option<Action> {
        match s {
            "new_column_right" => Some(Action::NewColumnRight),
            "split_down" | "new_row_below" => Some(Action::NewRowBelow),
            "close_pane" => Some(Action::ClosePane),
            "focus_left" => Some(Action::FocusLeft),
            "focus_right" => Some(Action::FocusRight),
            "focus_down" => Some(Action::FocusDown),
            "focus_up" => Some(Action::FocusUp),
            "move_pane_left" => Some(Action::MovePaneLeft),
            "move_pane_right" => Some(Action::MovePaneRight),
            "column_width_one_third" => Some(Action::ColumnWidthOneThird),
            "column_width_half" => Some(Action::ColumnWidthHalf),
            "column_width_two_thirds" => Some(Action::ColumnWidthTwoThirds),
            "column_width_full" => Some(Action::ColumnWidthFull),
            "column_width_increase" => Some(Action::ColumnWidthIncrease),
            "column_width_decrease" => Some(Action::ColumnWidthDecrease),
            "toggle_overview" => Some(Action::ToggleOverview),
            "exit_overview" => Some(Action::ExitOverview),
            "send_leader_key" => Some(Action::SendLeaderKey),
            _ => {
                // Handle switch_workspace_N
                if let Some(rest) = s.strip_prefix("switch_workspace_")
                    && let Ok(n) = rest.parse::<usize>() {
                        return Some(Action::SwitchWorkspace(n));
                    }
                None
            }
        }
    }
}
