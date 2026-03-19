/// Actions triggered by keybindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// New column to the right in the current workspace.
    NewColumnRight,
    /// New workspace below with a new pane.
    NewWorkspaceBelow,
    ClosePane,
    /// h/l: navigate columns within the current workspace.
    FocusLeft,
    FocusRight,
    /// j/k: navigate between workspaces.
    FocusDown,
    FocusUp,
    /// Shift+H/L: move column position within the workspace.
    MovePaneLeft,
    MovePaneRight,
    /// Cycle forward through configured width presets.
    CyclePresetWidth,
    /// Cycle backward through configured width presets.
    CyclePresetWidthReverse,
    /// Set column to one-third viewport width.
    ColumnWidthOneThird,
    /// Set column to half viewport width.
    ColumnWidthHalf,
    /// Set column to two-thirds viewport width.
    ColumnWidthTwoThirds,
    /// Set column to full viewport width.
    ColumnWidthFull,
    /// Increase the active column's width proportion.
    ColumnWidthIncrease,
    /// Decrease the active column's width proportion.
    ColumnWidthDecrease,
    /// Equalize active column and its right neighbor to the same width.
    EqualizeAdjacentColumns,
    /// Consume the right neighbor column's active pane into the current column as a new tile.
    ConsumeIntoColumn,
    /// Expel the current column's active tile into a new column to the right.
    ExpelFromColumn,
    /// Toggle broadcast mode: send input to all panes in current workspace.
    ToggleBroadcast,
    /// Switch to workspace by index (0-8).
    SwitchWorkspace(usize),
    ToggleOverview,
    /// Exit overview mode (return to normal).
    ExitOverview,
    SendLeaderKey,
    ScrollPageUp,
    ScrollPageDown,
    ScrollTop,
    ScrollBottom,
}

impl Action {
    /// Parse an action from its string name (used for config-driven keybindings).
    pub fn from_name(s: &str) -> Option<Action> {
        match s {
            "new_column_right" => Some(Action::NewColumnRight),
            "split_down" | "new_row_below" | "new_workspace_below" => Some(Action::NewWorkspaceBelow),
            "close_pane" => Some(Action::ClosePane),
            "focus_left" => Some(Action::FocusLeft),
            "focus_right" => Some(Action::FocusRight),
            "focus_down" => Some(Action::FocusDown),
            "focus_up" => Some(Action::FocusUp),
            "move_pane_left" => Some(Action::MovePaneLeft),
            "move_pane_right" => Some(Action::MovePaneRight),
            "cycle_preset_width" => Some(Action::CyclePresetWidth),
            "cycle_preset_width_reverse" => Some(Action::CyclePresetWidthReverse),
            "column_width_one_third" => Some(Action::ColumnWidthOneThird),
            "column_width_half" => Some(Action::ColumnWidthHalf),
            "column_width_two_thirds" => Some(Action::ColumnWidthTwoThirds),
            "column_width_full" => Some(Action::ColumnWidthFull),
            "column_width_increase" => Some(Action::ColumnWidthIncrease),
            "column_width_decrease" => Some(Action::ColumnWidthDecrease),
            "equalize_adjacent_columns" => Some(Action::EqualizeAdjacentColumns),
            "consume_into_column" => Some(Action::ConsumeIntoColumn),
            "expel_from_column" => Some(Action::ExpelFromColumn),
            "toggle_broadcast" => Some(Action::ToggleBroadcast),
            "toggle_overview" => Some(Action::ToggleOverview),
            "exit_overview" => Some(Action::ExitOverview),
            "send_leader_key" => Some(Action::SendLeaderKey),
            "scroll_page_up" => Some(Action::ScrollPageUp),
            "scroll_page_down" => Some(Action::ScrollPageDown),
            "scroll_top" => Some(Action::ScrollTop),
            "scroll_bottom" => Some(Action::ScrollBottom),
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
